use super::{grok, grok_billing, http, tokens, usage};
use crate::models::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::json;

#[test]
fn codex_keeps_distinct_windows_credits_and_unknown_values() {
    let result = usage::parse_codex(&json!({
        "plan_type":"plus", "rate_limit":{
            "primary_window":{"used_percent":12.34567,"limit_window_seconds":18000,"reset_at":1900000000},
            "secondary_window":{"used_percent":80,"limit_window_seconds":604800,"reset_at":1900000500}
        },
        "code_review_rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":604800}},
        "credits":{"unlimited":false,"balance":"4.50"}
    })).unwrap();
    assert_eq!(result.windows.len(), 3);
    assert_eq!(result.windows[0].label, "5 小时");
    assert_eq!(result.windows[1].label, "周额度");
    assert_eq!(result.credits, Some(4.5));
    assert_eq!(result.windows[2].used_percent, Some(0.0));
    assert!(usage::parse_codex(&json!({"rate_limit":{"primary_window":{}}})).is_err());
}

#[test]
fn claude_preserves_model_windows_and_converts_cents_once() {
    let result = usage::parse_claude(&json!({
        "five_hour":{"utilization":14,"resets_at":"2030-03-17T17:46:40Z"},
        "seven_day":{"utilization":27},
        "seven_day_sonnet":{"utilization":8},
        "extra_usage":{"is_enabled":true,"utilization":25,"used_credits":1250,"monthly_limit":5000}
    }))
    .unwrap();
    assert_eq!(result.windows.len(), 4);
    assert_eq!(result.windows[0].reset_at, Some(1900000000));
    let extra = result
        .windows
        .iter()
        .find(|w| w.id == "extra_usage")
        .unwrap();
    assert_eq!((extra.used, extra.limit), (Some(12.5), Some(50.0)));
    assert!(usage::parse_claude(
        &json!({"five_hour":{"utilization":null},"extra_usage":{"is_enabled":false}})
    )
    .is_err());
}

#[test]
fn gemini_buckets_are_model_specific_and_unknown_is_not_zero() {
    let result = usage::parse_gemini(&json!({"buckets":[
        {"modelId":"gemini-test-pro","remainingFraction":0.6,"resetTime":"2030-03-17T17:46:40Z"},
        {"modelId":"gemini-test-flash","remainingFraction":"1"},
        {"modelId":"unknown"}, {"modelId":"invalid","remainingFraction":1.2}
    ]}))
    .unwrap();
    assert_eq!(result.windows.len(), 2);
    assert_eq!(result.windows[0].used_percent, Some(40.0));
    assert_eq!(result.windows[1].used_percent, Some(0.0));
    assert_eq!(result.windows[0].model.as_deref(), Some("gemini-test-pro"));
    assert!(usage::parse_gemini(&json!({"buckets":[]})).is_err());
}

#[test]
fn gemini_onboarding_reads_project_and_picks_the_default_tier() {
    // `loadCodeAssist` 的项目字段可能是字符串，也可能是带 `id` 的对象。
    assert_eq!(
        usage::gemini_project(&json!({"cloudaicompanionProject":"project-string"})),
        "project-string"
    );
    assert_eq!(
        usage::gemini_project(&json!({"cloudaicompanionProject":{"id":"project-object"}})),
        "project-object"
    );
    assert!(usage::gemini_project(&json!({})).is_empty());

    // 只有完全拿不到项目时才开通，所以优先默认（免费）档，其次当前档，最后旧档。
    let tiers = json!({
        "allowedTiers":[{"id":"paid-tier","isDefault":false},{"id":"free-tier","isDefault":true}],
        "currentTier":{"id":"current-tier"}
    });
    assert_eq!(usage::gemini_tier(&tiers), "free-tier");
    assert_eq!(
        usage::gemini_tier(&json!({
            "allowedTiers":[{"id":"paid-tier","isDefault":false}],
            "currentTier":{"id":"current-tier"}
        })),
        "current-tier"
    );
    assert_eq!(usage::gemini_tier(&json!({})), "legacy-tier");
}

#[test]
fn grok_billing_distinguishes_weekly_resets_and_monthly_money() {
    let result = grok_billing::parse(&json!({"config":{
        "creditUsagePercent":37.5,"currentPeriod":{"type":"weekly","end":"2030-03-17T17:46:40Z"},
        "productUsage":[{"product":"CLI","usagePercent":20}],
        "monthlyLimit":{"val":15000},"used":{"val":"3000"},
        "onDemandCap":{"val":12},"onDemandUsed":3,"prepaidBalance":"8",
        "billingPeriodEnd":"2030-04-01T00:00:00Z"
    }}))
    .unwrap();
    assert_eq!(result.windows.len(), 4);
    let monthly = result
        .windows
        .iter()
        .find(|w| w.id == "grok-monthly")
        .unwrap();
    assert_eq!(
        (monthly.used, monthly.limit, monthly.used_percent),
        (Some(30.0), Some(150.0), Some(20.0))
    );
    let weekly = result
        .windows
        .iter()
        .find(|w| w.id == "grok-weekly")
        .unwrap();
    assert_ne!(monthly.reset_at, weekly.reset_at);
    let ondemand = result
        .windows
        .iter()
        .find(|w| w.id == "grok-on-demand")
        .unwrap();
    assert_eq!((ondemand.used, ondemand.limit), (Some(3.0), Some(12.0)));
    assert_eq!(result.credits, Some(8.0));
    let absent = grok_billing::parse(
        &json!({"config":{"creditUsagePercent":0,"billingPeriodEnd":"2030-04-01T00:00:00Z"}}),
    )
    .unwrap();
    assert_eq!(absent.windows[0].reset_at, None);
}

#[test]
fn grok_unknown_proto_and_grpc_error_are_not_successful_zero_usage() {
    assert!(grok::parse(&[0, 0, 0, 0, 2, 8, 0]).is_err());
    let trailer = b"grpc-status: 16\r\n";
    let mut frame = vec![128];
    frame.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
    frame.extend_from_slice(trailer);
    assert!(grok::parse(&frame).unwrap_err().contains("授权已失效"));
    assert!(grok::parse(&[0, 0, 0, 0, 255]).is_err());
    let mut percent = vec![13];
    percent.extend_from_slice(&37.5f32.to_le_bytes());
    let mut body = vec![10, percent.len() as u8];
    body.extend_from_slice(&percent);
    let mut frame = vec![0];
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    assert_eq!(
        grok::parse(&frame).unwrap().windows[0].used_percent,
        Some(37.5)
    );
}

#[test]
fn identity_resolution_fills_google_profile_and_grok_access_claims() {
    let mut auth = ProviderInput::default().auth;
    auth.oauth = Some(OAuthCredential::new(OAuthVendor::Gemini));
    tokens::apply_identity(
        &mut auth,
        &json!({"email":"person@example.test","id":"google-subject"}),
    );
    assert_eq!(auth.oauth.as_ref().unwrap().subject, "google-subject");
    assert_eq!(auth.oauth.as_ref().unwrap().email, "person@example.test");
    let mut identity = OAuthCredential::new(OAuthVendor::Grok);
    identity.id_token = format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(json!({"sub":"grok-user"}).to_string())
    );
    auth.oauth = Some(identity);
    auth.access_token = format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(
            json!({"email":"person@example.test","team_id":"team-fixture","tier":5}).to_string()
        )
    );
    tokens::enrich_identity(&mut auth);
    let identity = auth.oauth.unwrap();
    assert_eq!(identity.subject, "grok-user");
    assert_eq!(identity.organization_id, "team-fixture");
    assert_eq!(identity.plan, "SuperGrok Heavy");
}

#[test]
fn token_response_preserves_unrotated_refresh_and_clears_pending() {
    let mut auth = ProviderInput::default().auth;
    auth.oauth = Some(OAuthCredential::new(OAuthVendor::Claude));
    auth.oauth.as_mut().unwrap().refresh_pending = true;
    auth.refresh_token = "previous-refresh".into();
    let next = tokens::apply_response(
        auth.clone(),
        &json!({"access_token":"new-access","expires_in":3600}),
    )
    .unwrap();
    assert_eq!(next.refresh_token, "previous-refresh");
    assert_eq!(next.mode, AuthMode::AccessToken);
    assert!(!next.oauth.unwrap().refresh_pending);
    assert!(tokens::apply_response(auth, &json!({"expires_in":3600})).is_err());
}

#[test]
fn models_are_dynamic_deduplicated_and_exclude_hidden_entries() {
    let models = usage::parse_models(&json!({"models":[{"slug":"future-model"},{"slug":"hidden","hidden":true},{"slug":"future-model"}]})).unwrap();
    assert_eq!(models, vec!["future-model"]);
    assert!(usage::parse_models(&json!({"error":"invalid"})).is_err());
    assert!(http::number(&json!("NaN")).is_none());
    assert_eq!(http::timestamp(&json!(1900000000000i64)), Some(1900000000));
    assert!(http::timestamp(&json!(i64::MIN)).is_none());
    assert!(http::timestamp(&json!(i64::MAX)).is_none());
}

fn quota_window(
    id: &str,
    used_percent: Option<f64>,
    history: Vec<AccountQuotaSample>,
) -> AccountQuotaWindow {
    AccountQuotaWindow {
        id: id.into(),
        label: "5 小时".into(),
        used_percent,
        history,
        ..Default::default()
    }
}

fn quota_subscription(window: AccountQuotaWindow) -> AccountSubscription {
    AccountSubscription {
        windows: vec![window],
        ..Default::default()
    }
}

#[test]
fn quota_history_extends_with_the_new_reading() {
    let previous = quota_subscription(quota_window(
        "five_hour",
        Some(12.0),
        vec![AccountQuotaSample {
            at: 10,
            used_percent: 12.0,
        }],
    ));
    let mut next = quota_subscription(quota_window("five_hour", Some(13.0), Vec::new()));

    super::record_quota_history(&mut next, Some(&previous), 20);

    assert_eq!(
        next.windows[0].history,
        vec![
            AccountQuotaSample {
                at: 10,
                used_percent: 12.0
            },
            AccountQuotaSample {
                at: 20,
                used_percent: 13.0
            },
        ]
    );
}

#[test]
fn quota_history_skips_a_reading_the_meter_cannot_show() {
    let previous = quota_subscription(quota_window(
        "five_hour",
        Some(12.0),
        vec![AccountQuotaSample {
            at: 10,
            used_percent: 12.0,
        }],
    ));
    let mut next = quota_subscription(quota_window("five_hour", Some(12.02), Vec::new()));

    super::record_quota_history(&mut next, Some(&previous), 20);

    assert_eq!(
        next.windows[0].history,
        vec![AccountQuotaSample {
            at: 10,
            used_percent: 12.0
        }]
    );
}

#[test]
fn quota_history_starts_a_new_window_and_drops_the_oldest_point() {
    let history = (0..super::QUOTA_HISTORY_SAMPLES as i64)
        .map(|index| AccountQuotaSample {
            at: index,
            used_percent: index as f64,
        })
        .collect::<Vec<_>>();
    let previous = subscription_with_windows(vec![
        quota_window("five_hour", Some(23.0), history),
        quota_window("seven_day", Some(40.0), Vec::new()),
    ]);
    let mut next = subscription_with_windows(vec![
        quota_window("five_hour", Some(24.0), Vec::new()),
        quota_window("seven_day", Some(41.0), Vec::new()),
    ]);

    super::record_quota_history(&mut next, Some(&previous), 99);

    let five_hour = &next.windows[0].history;
    assert_eq!(five_hour.len(), super::QUOTA_HISTORY_SAMPLES);
    assert_eq!(five_hour.first().unwrap().at, 1);
    assert_eq!(five_hour.last().unwrap().at, 99);
    // 新窗口没有历史可继承，但本次读数必须成为它的起点。
    assert_eq!(
        next.windows[1].history,
        vec![AccountQuotaSample {
            at: 99,
            used_percent: 41.0
        }]
    );
}

#[test]
fn quota_history_is_kept_when_a_refresh_reports_no_percentage() {
    let previous = quota_subscription(quota_window(
        "extra_usage",
        Some(37.0),
        vec![AccountQuotaSample {
            at: 10,
            used_percent: 37.0,
        }],
    ));
    let mut next = quota_subscription(quota_window("extra_usage", None, Vec::new()));

    super::record_quota_history(&mut next, Some(&previous), 20);

    assert_eq!(next.windows[0].history, previous.windows[0].history);
}

fn subscription_with_windows(windows: Vec<AccountQuotaWindow>) -> AccountSubscription {
    AccountSubscription {
        windows,
        ..Default::default()
    }
}

#[test]
fn saved_quota_windows_without_history_still_load() {
    // 旧版本保存的额度窗口没有 history 字段；serde(default) 必须让它们照常读出，
    // 否则升级后整份应用数据会解析失败。
    let saved: AccountSubscription = serde_json::from_value(json!({
        "plan": "Pro",
        "organization": "",
        "projectId": "",
        "windows": [{
            "id": "five_hour",
            "label": "5 小时",
            "model": null,
            "usedPercent": 12.0,
            "used": null,
            "limit": null,
            "unit": null,
            "resetAt": null,
            "windowSeconds": null
        }],
        "credits": null,
        "creditsUnit": null,
        "creditsUnlimited": false,
        "updatedAt": null,
        "error": null
    }))
    .unwrap();
    assert!(saved.windows[0].history.is_empty());

    let round_trip = serde_json::to_value(&saved).unwrap();
    assert_eq!(round_trip["windows"][0]["history"], json!([]));
}
