use super::{grok_billing, http};
use crate::models::*;
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Value};
use std::time::Duration;

/// 开通长任务的轮询间隔与次数上限，与 Gemini CLI 一致（5 秒一次）；
/// 次数封顶避免一次额度刷新被开通流程拖住。
const ONBOARD_POLL: Duration = Duration::from_secs(5);
const ONBOARD_POLLS: usize = 3;

fn authorized(request: RequestBuilder, provider: &Provider) -> RequestBuilder {
    let mut request = request
        .bearer_auth(&provider.auth.access_token)
        .header("User-Agent", "BalanceHub");
    if let Some(identity) = &provider.auth.oauth {
        match identity.vendor {
            OAuthVendor::Codex => {
                request = request.header("originator", "codex_cli_rs");
                if !identity.account_id.is_empty() {
                    request = request.header("chatgpt-account-id", &identity.account_id);
                }
            }
            OAuthVendor::Claude => {
                request = request
                    .header("anthropic-beta", "oauth-2025-04-20")
                    .header("anthropic-version", "2023-06-01")
            }
            OAuthVendor::Grok => request = grok_billing::with_cli_identity(request, provider),
            OAuthVendor::Gemini => {}
        }
    }
    request
}

pub(super) async fn subscription(
    client: &Client,
    provider: &Provider,
) -> Result<AccountSubscription, String> {
    let identity = provider.auth.oauth.as_ref().ok_or("缺少账号授权")?;
    if provider.auth.access_token.trim().is_empty() {
        return Err("请先授权或启用本机续期".into());
    }
    let mut result = match identity.vendor {
        OAuthVendor::Codex => parse_codex(
            &http::json(
                authorized(
                    client.get("https://chatgpt.com/backend-api/wham/usage"),
                    provider,
                ),
                "读取 Codex 额度",
            )
            .await?,
        )?,
        OAuthVendor::Claude => {
            let usage = http::json(
                authorized(
                    client.get("https://api.anthropic.com/api/oauth/usage"),
                    provider,
                ),
                "读取 Claude 额度",
            )
            .await?;
            parse_claude(&usage)?
        }
        OAuthVendor::Gemini => {
            let mut lookup = json!({"metadata":{"ideType":"GEMINI_CLI","pluginType":"GEMINI"}});
            if !identity.project_id.is_empty() {
                lookup["cloudaicompanionProject"] = identity.project_id.clone().into();
            }
            let load = http::json(
                authorized(
                    client.post("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"),
                    provider,
                )
                .json(&lookup),
                "读取 Gemini 套餐",
            )
            .await?;
            let mut project = gemini_project(&load);
            if project.is_empty() {
                project = identity.project_id.clone();
            }
            // 新账号还没有项目：上游 CLI 在登录后开通一次，跳过这一步额度查询没有 project 可用。
            if project.is_empty() {
                project = onboard_gemini_project(client, provider, &load)
                    .await
                    .unwrap_or_default();
            }
            let mut body = json!({});
            if !project.is_empty() {
                body["project"] = project.clone().into();
            }
            let quota = http::json(
                authorized(
                    client.post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota"),
                    provider,
                )
                .json(&body),
                "读取 Gemini 额度",
            )
            .await?;
            let mut snapshot = parse_gemini(&quota)?;
            snapshot.project_id = project;
            snapshot.plan = load
                .get("currentTier")
                .map(|tier| http::text(tier, &["name", "id"]))
                .unwrap_or_default();
            snapshot
        }
        OAuthVendor::Grok => grok_billing::subscription(client, provider).await?,
    };
    if result.plan.is_empty() {
        result.plan.clone_from(&identity.plan);
    }
    if result.organization.is_empty() {
        result.organization.clone_from(&identity.organization_name);
    }
    result.updated_at = Some(chrono::Utc::now().timestamp());
    Ok(result)
}

pub(super) async fn models(client: &Client, provider: &Provider) -> Result<Vec<String>, String> {
    let identity = provider.auth.oauth.as_ref().ok_or("缺少账号授权")?;
    if provider.auth.access_token.trim().is_empty() {
        return Err("请先授权或启用本机续期".into());
    }
    if identity.vendor == OAuthVendor::Gemini {
        let quota = subscription(client, provider).await?;
        let mut models: Vec<_> = quota.windows.into_iter().filter_map(|w| w.model).collect();
        models.sort();
        models.dedup();
        return Ok(models);
    }
    let url = match identity.vendor {
        OAuthVendor::Codex => "https://chatgpt.com/backend-api/codex/models",
        OAuthVendor::Claude => "https://api.anthropic.com/v1/models",
        OAuthVendor::Grok => "https://cli-chat-proxy.grok.com/v1/models",
        OAuthVendor::Gemini => unreachable!(),
    };
    let mut all = Vec::new();
    let mut after = String::new();
    for _ in 0..20 {
        let mut request = authorized(client.get(url), provider);
        if identity.vendor == OAuthVendor::Codex {
            request = request.query(&[("client_version", "0.162.0")]);
        }
        if identity.vendor == OAuthVendor::Claude {
            request = request.query(&[("limit", "100")]);
        }
        if !after.is_empty() {
            request = request.query(&[("after_id", &after)]);
        }
        let value = http::json(request, "读取账号模型").await?;
        all.extend(parse_models(&value)?);
        if value.get("has_more").and_then(Value::as_bool) != Some(true) {
            all.sort();
            all.dedup();
            return Ok(all);
        }
        let next = http::text(&value, &["last_id"]);
        if next.is_empty() || next == after {
            return Err("模型列表分页信息无效".into());
        }
        after = next;
    }
    Err("模型列表分页过多，本次未覆盖已有列表".into())
}

pub(crate) fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let values = value
        .as_array()
        .or_else(|| value.get("data").and_then(Value::as_array))
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array));
    let mut models = Vec::new();
    if let Some(values) = values {
        for entry in values {
            if entry.get("hidden").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let name = entry
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| http::text(entry, &["slug", "id", "model", "name"]));
            if !name.trim().is_empty() {
                models.push(name);
            }
        }
    } else if let Some(values) = value.get("models").and_then(Value::as_object) {
        models.extend(values.keys().cloned());
    } else {
        return Err("模型接口未返回模型列表".into());
    }
    models.sort();
    models.dedup();
    Ok(models)
}

fn window_label(seconds: Option<i64>, fallback: &str) -> String {
    match seconds {
        Some(18_000) => "5 小时".into(),
        Some(604_800) => "周额度".into(),
        Some(2_592_000) => "30 天".into(),
        Some(s) if s > 0 && s % 86400 == 0 => format!("{} 天", s / 86400),
        Some(s) if s > 0 && s % 3600 == 0 => format!("{} 小时", s / 3600),
        Some(s) if s > 0 => format!("{} 分钟", s / 60),
        _ => fallback.into(),
    }
}
fn percent(value: Option<&Value>) -> Option<f64> {
    value.and_then(http::number).filter(|v| *v >= 0.0)
}

pub(crate) fn parse_codex(value: &Value) -> Result<AccountSubscription, String> {
    let mut result = AccountSubscription {
        plan: http::text(value, &["plan_type"]),
        ..Default::default()
    };
    let mut groups = vec![
        ("rate_limit".to_string(), value.get("rate_limit")),
        (
            "code_review_rate_limit".into(),
            value.get("code_review_rate_limit"),
        ),
    ];
    if let Some(additional) = value
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        groups.extend(additional.iter().enumerate().map(|(i, v)| {
            (
                format!(
                    "additional-{i}-{}",
                    http::text(v, &["limit_name", "metered_feature"])
                ),
                v.get("rate_limit"),
            )
        }));
    }
    for (group, value) in groups {
        let Some(value) = value else {
            continue;
        };
        for key in ["primary_window", "secondary_window"] {
            let Some(window) = value.get(key).filter(|v| v.is_object()) else {
                continue;
            };
            let used = percent(window.get("used_percent"));
            let seconds = window.get("limit_window_seconds").and_then(Value::as_i64);
            if used.is_none() {
                continue;
            }
            let label = window_label(
                seconds,
                if key == "primary_window" {
                    "主额度"
                } else {
                    "次级额度"
                },
            );
            result.windows.push(AccountQuotaWindow {
                id: format!("{group}/{key}"),
                label: if group == "rate_limit" {
                    label
                } else if group == "code_review_rate_limit" {
                    format!("代码审查 · {label}")
                } else {
                    format!("附加额度 · {label}")
                },
                used_percent: used,
                reset_at: window.get("reset_at").and_then(http::timestamp),
                window_seconds: seconds,
                ..Default::default()
            });
        }
    }
    if let Some(credits) = value.get("credits") {
        result.credits_unlimited = credits
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        result.credits = credits.get("balance").and_then(http::number);
    }
    if result.windows.is_empty() && result.credits.is_none() && !result.credits_unlimited {
        return Err("Codex 尚未返回可用额度数据".into());
    }
    Ok(result)
}

pub(crate) fn parse_claude(value: &Value) -> Result<AccountSubscription, String> {
    let mut result = AccountSubscription::default();
    let object = value.as_object().ok_or("Claude 额度格式无效")?;
    for (id, window) in object {
        if id == "extra_usage" {
            continue;
        }
        let Some(used) = percent(window.get("utilization")) else {
            continue;
        };
        let (label, seconds) = match id.as_str() {
            "five_hour" => ("5 小时".into(), Some(18000)),
            "seven_day" => ("周额度".into(), Some(604800)),
            "seven_day_opus" => ("Opus · 周额度".into(), Some(604800)),
            "seven_day_sonnet" => ("Sonnet · 周额度".into(), Some(604800)),
            "seven_day_fable" => ("Fable · 周额度".into(), Some(604800)),
            _ => (id.replace('_', " "), None),
        };
        result.windows.push(AccountQuotaWindow {
            id: id.clone(),
            label,
            used_percent: Some(used),
            reset_at: window.get("resets_at").and_then(http::timestamp),
            window_seconds: seconds,
            ..Default::default()
        });
    }
    if let Some(limits) = value.get("limits").and_then(Value::as_array) {
        for (index, limit) in limits.iter().enumerate() {
            let Some(used) = percent(limit.get("percent")) else {
                continue;
            };
            let model = limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str);
            let surface = limit.pointer("/scope/surface").and_then(Value::as_str);
            let weekly = limit.get("group").and_then(Value::as_str) == Some("weekly");
            let id = model
                .filter(|_| weekly && surface.is_none())
                .map(|m| format!("seven_day_{}", m.to_lowercase()))
                .unwrap_or_else(|| format!("scoped-{index}"));
            result.windows.retain(|w| w.id != id);
            let label = [
                model.unwrap_or("专属额度"),
                surface.unwrap_or_default(),
                if weekly { "周额度" } else { "额度" },
            ]
            .into_iter()
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
            result.windows.push(AccountQuotaWindow {
                id,
                label,
                model: model.map(str::to_owned),
                used_percent: Some(used),
                reset_at: limit.get("resets_at").and_then(http::timestamp),
                window_seconds: weekly.then_some(604800),
                ..Default::default()
            });
        }
    }
    if let Some(extra) = value
        .get("extra_usage")
        .filter(|v| v.get("is_enabled").and_then(Value::as_bool) == Some(true))
    {
        result.windows.push(AccountQuotaWindow {
            id: "extra_usage".into(),
            label: "额外用量 · 月额度".into(),
            used_percent: percent(extra.get("utilization")),
            used: extra
                .get("used_credits")
                .and_then(http::number)
                .map(|v| v / 100.0),
            limit: extra
                .get("monthly_limit")
                .and_then(http::number)
                .map(|v| v / 100.0),
            unit: Some("USD".into()),
            ..Default::default()
        });
    }
    result
        .windows
        .sort_by_key(|w| (w.window_seconds.unwrap_or(i64::MAX), w.id.clone()));
    if result.windows.is_empty() {
        return Err("Claude 尚未返回可用额度数据".into());
    }
    Ok(result)
}

/// 从 `loadCodeAssist` 的结果里取项目 ID：可以是字符串，也可以是带 `id` 的对象。
pub(super) fn gemini_project(load: &Value) -> String {
    load.get("cloudaicompanionProject")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("id").and_then(Value::as_str))
        })
        .unwrap_or_default()
        .to_owned()
}

/// 开通时使用的档位：优先 `allowedTiers` 里标记为默认的（免费档），
/// 其次当前档位，最后才是旧档位。只有完全拿不到项目时才会用到它。
pub(super) fn gemini_tier(load: &Value) -> &str {
    load.get("allowedTiers")
        .and_then(Value::as_array)
        .and_then(|tiers| {
            tiers
                .iter()
                .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
        })
        .and_then(|tier| tier.get("id").and_then(Value::as_str))
        .or_else(|| {
            load.get("currentTier")
                .and_then(|tier| tier.get("id"))
                .and_then(Value::as_str)
        })
        .unwrap_or("legacy-tier")
}

/// 为还没有项目的账号开通一次项目，返回上游给出的项目 ID。
/// 接口顺序与 Gemini CLI 相同：`onboardUser` 返回长任务，再按任务名轮询结果。
async fn onboard_gemini_project(
    client: &Client,
    provider: &Provider,
    load: &Value,
) -> Option<String> {
    const BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
    let mut operation = http::json(
        authorized(client.post(format!("{BASE}:onboardUser")), provider)
            .json(&json!({"tierId": gemini_tier(load)})),
        "开通 Gemini 项目",
    )
    .await
    .ok()?;
    for _ in 0..ONBOARD_POLLS {
        if operation.get("done").and_then(Value::as_bool) == Some(true) {
            break;
        }
        let name = operation.get("name").and_then(Value::as_str)?.to_owned();
        tokio::time::sleep(ONBOARD_POLL).await;
        operation = http::json(
            authorized(client.get(format!("{BASE}/{name}")), provider),
            "查询 Gemini 项目开通结果",
        )
        .await
        .ok()?;
    }
    if operation.get("done").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let project = operation
        .get("response")
        .map(gemini_project)
        .unwrap_or_default();
    (!project.is_empty()).then_some(project)
}

pub(crate) fn parse_gemini(value: &Value) -> Result<AccountSubscription, String> {
    let buckets = value
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or("Gemini 未返回模型额度")?;
    let mut result = AccountSubscription::default();
    for (index, bucket) in buckets.iter().enumerate() {
        let remaining = bucket
            .get("remainingFraction")
            .and_then(http::number)
            .filter(|v| (0.0..=1.0).contains(v));
        let model = http::text(bucket, &["modelId"]);
        let reset = bucket.get("resetTime").and_then(http::timestamp);
        if remaining.is_none() {
            continue;
        }
        result.windows.push(AccountQuotaWindow {
            id: format!("gemini-{index}"),
            label: if model.is_empty() {
                "模型额度".into()
            } else {
                model.clone()
            },
            model: (!model.is_empty()).then_some(model),
            used_percent: remaining.map(|v| (1.0 - v) * 100.0),
            reset_at: reset,
            ..Default::default()
        });
    }
    if result.windows.is_empty() {
        return Err("Gemini 尚未返回可用额度数据".into());
    }
    Ok(result)
}
