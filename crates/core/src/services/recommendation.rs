//! 中转站推荐：把卡片上已经有的余额、额度窗口、测活记录和模型价格放在一起排序。
//!
//! 这里是纯函数，不发起网络请求，也不重新探测账号；调用方先把最新读数刷新到
//! `Provider` 里，再由它生成一份可解释的排序。评分只用于同一份报告内部比较。

use crate::models::{
    ModelPrice, Provider, ProviderModelPerformance, ProviderQuotaWindow, ProviderRecommendation,
    ProviderRecommendationReport, ProviderStatus,
};
use chrono::{DateTime, Utc};

/// 推荐只看最近这么多次测活；更早的记录已经不能代表当前线路状态。
pub const LIVENESS_WINDOW: usize = 24;
/// 连续失败达到这个次数，且最近一次测活在一天内，视为当前不可用。
pub const LIVENESS_FAILURE_STREAK: usize = 3;
/// 测活数据超过这个天数后降低权重，但仍作为参考。
const LIVENESS_STALE_DAYS: i64 = 7;
/// 余额超过这个天数没有同步时，不再按最新读数给满权重。
const QUOTA_STALE_DAYS: i64 = 1;
/// 站点价格超过这个天数没有更新时，只作为参考价参与比较。
const PRICE_STALE_DAYS: i64 = 7;
/// 额度窗口在这么多小时内重置时，给“先用掉快过期额度”的加分。
const RESET_HORIZON_HOURS: f64 = 168.0;

pub fn recommend(
    providers: &[Provider],
    model: &str,
    now: DateTime<Utc>,
) -> ProviderRecommendationReport {
    let model = model.trim();
    let mut drafts = providers
        .iter()
        .map(|provider| draft(provider, model, now))
        .collect::<Vec<_>>();
    let min_price = drafts
        .iter()
        .filter(|draft| draft.entry.eligible)
        .filter_map(|draft| draft.price)
        .filter(|price| price.is_finite())
        .min_by(f64::total_cmp);
    for draft in &mut drafts {
        draft.finish(min_price);
    }
    drafts.sort_by(|left, right| {
        right
            .entry
            .eligible
            .cmp(&left.entry.eligible)
            .then_with(|| right.entry.score.cmp(&left.entry.score))
            .then_with(|| {
                right
                    .entry
                    .liveness_success_rate
                    .unwrap_or(-1.0)
                    .total_cmp(&left.entry.liveness_success_rate.unwrap_or(-1.0))
            })
            .then_with(|| {
                right
                    .entry
                    .site_success_rate
                    .unwrap_or(-1.0)
                    .total_cmp(&left.entry.site_success_rate.unwrap_or(-1.0))
            })
            .then_with(|| left.entry.label.cmp(&right.entry.label))
    });
    let entries = drafts
        .into_iter()
        .map(|draft| draft.entry)
        .collect::<Vec<_>>();
    ProviderRecommendationReport {
        model: model.to_string(),
        generated_at: now.to_rfc3339(),
        recommended_provider_id: entries
            .iter()
            .find(|entry| entry.eligible)
            .map(|entry| entry.provider_id.clone()),
        entries,
    }
}

struct Draft {
    entry: ProviderRecommendation,
    /// 0..1，测活成功率；没有记录时取中性值。
    health: f64,
    /// 0..1，额度余量；无法判断时取中性值。
    quota: f64,
    /// 0..1，余额读数的可信度；过期数据向中性值收敛。
    quota_confidence: f64,
    /// 用于互相比较的价格；不是展示字段。
    price: Option<f64>,
    /// 0..1，价格读数的可信度；过期或读取失败时向中性值收敛。
    price_confidence: f64,
    /// 0..1，模型可用性。
    model: f64,
    /// 0..1，额度窗口即将重置的紧迫度。
    reset: f64,
}

impl Draft {
    fn finish(&mut self, min_price: Option<f64>) {
        let health = self.health * 35.0;
        // 30 分和 20 分的原始区间都以中性值作为锚点：数据越旧，越接近
        // “无法判断”，而不是让三天前的余额继续拿满分。
        let quota_raw = self.quota * 30.0;
        let quota = 15.0 + (quota_raw - 15.0) * self.quota_confidence;
        let price_raw = match (self.price, min_price) {
            (Some(price), Some(min)) if min > 0.0 => (min / price).clamp(0.0, 1.0) * 20.0,
            (Some(price), Some(_)) if price <= 0.0 => 20.0,
            (Some(_), Some(_)) => 0.0,
            // 没有可用账号带价格时，价格不参与区分；不可用账号也不因此被抬高。
            (Some(_), None) | (None, _) => 10.0,
        };
        let price = 10.0 + (price_raw - 10.0) * self.price_confidence;
        let model = self.model * 10.0;
        let reset = self.reset * 5.0;
        self.entry.score = (health + quota + price + model + reset)
            .round()
            .clamp(0.0, 100.0) as u8;
    }
}

fn draft(provider: &Provider, model: &str, now: DateTime<Utc>) -> Draft {
    let base_url_valid = valid_base_url(&provider.identity.base_url);
    let liveness = liveness_stats(provider, model, now);
    let quota = quota_stats(provider, now);
    let support = model_support(provider, model);
    let site = site_performance(provider, model);
    let quote = price_quote(provider, model, now);
    let price = quote.price.clone();

    let mut reasons = Vec::new();
    let mut blockers = Vec::new();

    if !provider.runtime.enabled {
        blockers.push("已停用".to_string());
    }
    if !base_url_valid {
        blockers.push("中转站地址无效".to_string());
    }
    if quota.exhausted {
        blockers.push(quota.exhausted_message.clone());
    }
    if let Some(until) = provider.runtime.cooldown_until {
        if until > now.timestamp() {
            let reason = provider
                .runtime
                .cooldown_reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .unwrap_or("自动刷新冷却中");
            blockers.push(format!("{reason}，暂不可用"));
        }
    }
    if support == Some(false) {
        blockers.push(format!("站点模型列表不包含 {model}"));
    }
    if liveness.blocked {
        blockers.push(format!("最近 {LIVENESS_FAILURE_STREAK} 次测活均失败"));
    }

    if let Some(rate) = liveness.success_rate {
        reasons.push(format!(
            "{}成功率 {:.0}%（近 {} 次）",
            if liveness.model_matched {
                "该模型测活"
            } else {
                "账号测活"
            },
            rate * 100.0,
            liveness.samples
        ));
        if !liveness.model_matched {
            reasons.push(format!("暂无 {model} 的测活记录，按账号整体测活参考"));
        }
        if liveness.stale {
            reasons.push("测活数据已超过 7 天".to_string());
        }
    } else {
        reasons.push("暂无测活记录".to_string());
    }
    reasons.push(quota.label.clone());
    if quota.stale && (provider.quota.known || !provider.quota.windows.is_empty()) {
        reasons.push(match quota_age_label(quota.updated_at.as_deref(), now) {
            Some(label) => format!("余额数据已超过 {label}，按参考值比较"),
            None => "余额同步时间未知，按参考值比较".to_string(),
        });
    }
    if quote.stale {
        reasons.push(match quote_age_label(quote.updated_at.as_deref(), now) {
            Some(label) => format!("价格数据已超过 {label}，按参考价比较"),
            None => "价格同步时间未知，按参考价比较".to_string(),
        });
    }
    if provider
        .capabilities
        .pricing
        .error
        .as_deref()
        .is_some_and(|error| !error.trim().is_empty())
    {
        reasons.push("价格读取失败，沿用上次成功读数".to_string());
    }
    let price_label = match quote.source {
        PriceSource::Manual => "卡片覆盖价格".to_string(),
        PriceSource::Site if !quote.group.trim().is_empty() => {
            format!("站点价格（分组 {}）", quote.group.trim())
        }
        PriceSource::Site => "站点价格".to_string(),
        PriceSource::None => String::new(),
    };
    if let Some(input) = price.input {
        reasons.push(price_reason(&price_label, "输入", input));
    }
    if let Some(output) = price.output {
        reasons.push(price_reason(&price_label, "输出", output));
    }
    if let Some(per_request) = quote.per_request {
        reasons.push(price_reason(&price_label, "按次", per_request));
    }
    if support == Some(true) {
        reasons.push(format!("模型列表包含 {model}"));
    } else if support.is_none() && !model.is_empty() {
        reasons.push("模型列表未同步".to_string());
    }
    if let Some(rate) = site.success_rate {
        reasons.push(format!("站点近 24 小时成功率 {:.1}%", rate * 100.0));
    }
    if let Some(tps) = site.avg_tps {
        reasons.push(format!("站点平均 {tps:.1} t/s"));
    }
    if let Some(resets_at) = quota.resets_at {
        if let Some(hours) = hours_until(resets_at, now) {
            if (0.0..=RESET_HORIZON_HOURS).contains(&hours) {
                reasons.push(format!(
                    "额度窗口约 {} 小时后重置",
                    hours.max(1.0).round() as i64
                ));
            }
        }
    }
    if let Some(message) = provider.runtime.error_message.as_deref() {
        let message = message.trim();
        if !message.is_empty() {
            reasons.push(format!("最近刷新失败：{message}"));
        }
    }

    let health = health_factor(&liveness, provider);
    let quota_factor = quota.factor;
    let reset = reset_factor(quota.remaining_percent, quota.resets_at, now);
    let model_factor = match support {
        Some(true) => 1.0,
        Some(false) => 0.0,
        None => 0.5,
    };
    // 站点已经统计到该模型的真实调用结果时，用成功率微调“列表里有这个模型”
    // 的权重；没有样本就沿用原判断，不把缺少观测当成表现差。
    let site_factor = site.success_rate.map_or(1.0, |rate| 0.6 + 0.4 * rate);
    let entry = ProviderRecommendation {
        provider_id: provider.identity.id.clone(),
        label: {
            let label = provider.display_label();
            if label.trim().is_empty() {
                provider.identity.name.clone()
            } else {
                label
            }
        },
        base_url: provider.identity.base_url.clone(),
        score: 0,
        eligible: blockers.is_empty(),
        model: model.to_string(),
        model_supported: support,
        model_wire_name: if model.is_empty() {
            String::new()
        } else {
            provider.model_config.wire_name(model).to_string()
        },
        input_price: price.input,
        output_price: price.output,
        per_request_price: quote.per_request,
        liveness_success_rate: liveness.success_rate,
        liveness_samples: liveness.samples,
        site_success_rate: site.success_rate,
        site_avg_tps: site.avg_tps,
        site_avg_latency_ms: site.avg_latency_ms,
        quota_remaining_percent: quota.remaining_percent,
        quota_label: quota.label,
        resets_at: quota.resets_at,
        quota_updated_at: quota.updated_at,
        pricing_updated_at: quote.updated_at,
        liveness_updated_at: liveness.updated_at,
        liveness_stale: liveness.stale,
        quota_stale: quota.stale,
        pricing_stale: quote.stale,
        cooldown_until: provider
            .runtime
            .cooldown_until
            .filter(|until| *until > now.timestamp()),
        cooldown_reason: provider.runtime.cooldown_reason.clone(),
        reasons,
        blockers,
    };
    Draft {
        entry,
        health,
        quota: quota_factor,
        quota_confidence: quota.confidence,
        // 编码类 Agent 的用量以输入 token 为主，价格比较统一用输入单价；
        // 只有输出单价的账号无法与其它账号同口径比较，按未知处理。
        price: price.input,
        price_confidence: quote.confidence,
        model: model_factor * site_factor,
        reset,
    }
}

/// 把“余额/价格同步于多久以前”写成推荐行能直接读的短语。
fn quota_age_label(updated_at: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    timestamp_age_label(updated_at, now)
}

fn quote_age_label(updated_at: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    timestamp_age_label(updated_at, now)
}

fn timestamp_age_label(updated_at: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    let age = now.signed_duration_since(parse_timestamp(updated_at?)?);
    Some(if age.num_days() >= 1 {
        format!("{} 天", age.num_days())
    } else {
        "24 小时".to_string()
    })
}

fn price_reason(label: &str, kind: &str, price: f64) -> String {
    let unit = if kind == "按次" { "/次" } else { "/M" };
    if label.is_empty() {
        format!("{kind} ${price:.2}{unit}")
    } else {
        format!("{label}：{kind} ${price:.2}{unit}")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PriceSource {
    None,
    Manual,
    Site,
}

struct PriceQuote {
    price: ModelPrice,
    per_request: Option<f64>,
    source: PriceSource,
    group: String,
    updated_at: Option<String>,
    stale: bool,
    confidence: f64,
}

impl PriceQuote {
    fn empty() -> Self {
        Self {
            price: ModelPrice::default(),
            per_request: None,
            source: PriceSource::None,
            group: String::new(),
            updated_at: None,
            stale: false,
            confidence: 1.0,
        }
    }
}

/// 价格优先级：卡片手动覆盖 > 站点真实价目 > 无。models.dev 目录按模型 ID
/// 全局匹配，同一模型在所有站点上价格相同，不能用来区分中转站。
fn price_quote(provider: &Provider, model: &str, now: DateTime<Utc>) -> PriceQuote {
    if model.is_empty() {
        return PriceQuote::empty();
    }
    let (pricing_updated_at, pricing_stale, pricing_confidence) = pricing_freshness(provider, now);
    let site = provider.capabilities.pricing.model(model).or_else(|| {
        let wire = provider.model_config.wire_name(model);
        if wire == model {
            None
        } else {
            provider.capabilities.pricing.model(wire)
        }
    });
    let site_price = site.map(|price| price.token_price()).unwrap_or_default();
    let site_group = site.map(|price| price.group.clone()).unwrap_or_default();
    let site_per_request = site.and_then(|price| price.per_request);

    let over = provider.model_config.overrides.get(model);
    let manually_priced = over.is_some_and(|over| {
        [
            over.price.input,
            over.price.output,
            over.price.cache_read,
            over.price.cache_write,
        ]
        .into_iter()
        .any(|price| price.is_some())
    });
    if let Some(over) = over {
        if manually_priced {
            return PriceQuote {
                price: ModelPrice {
                    input: over.price.input.or(site_price.input),
                    output: over.price.output.or(site_price.output),
                    cache_read: over.price.cache_read.or(site_price.cache_read),
                    cache_write: over.price.cache_write.or(site_price.cache_write),
                },
                per_request: None,
                source: PriceSource::Manual,
                group: site_group,
                updated_at: None,
                stale: false,
                confidence: 1.0,
            };
        }
    }
    if site.is_some() {
        return PriceQuote {
            price: site_price,
            per_request: site_per_request,
            source: PriceSource::Site,
            group: site_group,
            updated_at: pricing_updated_at,
            stale: pricing_stale,
            confidence: pricing_confidence,
        };
    }
    PriceQuote::empty()
}

/// 站点价目的可信度：读取失败或超过一周没有更新时只作为参考价。
fn pricing_freshness(provider: &Provider, now: DateTime<Utc>) -> (Option<String>, bool, f64) {
    let pricing = &provider.capabilities.pricing;
    let updated_at = pricing
        .updated_at
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::to_string);
    let Some(raw) = updated_at.as_deref() else {
        return (None, true, 0.5);
    };
    let Some(updated) = parse_timestamp(raw) else {
        return (updated_at, true, 0.5);
    };
    let stale = pricing
        .error
        .as_deref()
        .is_some_and(|error| !error.trim().is_empty())
        || now.signed_duration_since(updated) > chrono::Duration::days(PRICE_STALE_DAYS);
    (updated_at, stale, if stale { 0.5 } else { 1.0 })
}

fn model_support(provider: &Provider, model: &str) -> Option<bool> {
    if model.is_empty() {
        return None;
    }
    let models = provider
        .model_config
        .visible(&provider.capabilities.available_models);
    if models.is_empty() {
        return None;
    }
    // 覆盖项按模型 ID 保存，而站点列表可能是实际发给上游的 wire name；
    // 两个方向都要匹配，否则配置了映射的账号会被误判为“不提供此模型”。
    let target_wire = provider.model_config.wire_name(model);
    Some(
        models.iter().any(|id| {
            id == model || id == target_wire || provider.model_config.wire_name(id) == model
        }),
    )
}

/// 站点自己统计的模型表现。它描述的是这个中转站当前的真实流量，
/// 比模型列表里“有这个模型”更接近“现在能不能顺利调用”。
#[derive(Default)]
struct SitePerformance {
    success_rate: Option<f64>,
    avg_tps: Option<f64>,
    avg_latency_ms: Option<f64>,
}

impl SitePerformance {
    fn from(metric: Option<&ProviderModelPerformance>) -> Self {
        let Some(metric) = metric else {
            return Self::default();
        };
        Self {
            success_rate: metric
                .success_rate
                .filter(|rate| rate.is_finite())
                .map(|rate| (rate / 100.0).clamp(0.0, 1.0)),
            avg_tps: metric.avg_tps.filter(|tps| tps.is_finite() && *tps > 0.0),
            avg_latency_ms: metric
                .avg_latency_ms
                .filter(|latency| latency.is_finite() && *latency > 0.0),
        }
    }
}

fn site_performance(provider: &Provider, model: &str) -> SitePerformance {
    if model.is_empty() {
        return SitePerformance::default();
    }
    let wire = provider.model_config.wire_name(model);
    let metric = provider.capabilities.performance.model(model).or_else(|| {
        if wire == model {
            None
        } else {
            provider.capabilities.performance.model(wire)
        }
    });
    SitePerformance::from(metric)
}

struct LivenessStats {
    success_rate: Option<f64>,
    samples: usize,
    blocked: bool,
    stale: bool,
    updated_at: Option<String>,
    /// 窗口内是否存在推荐模型的测活记录；没有时退回账号整体记录并说明。
    model_matched: bool,
}

fn liveness_stats(provider: &Provider, model: &str, now: DateTime<Utc>) -> LivenessStats {
    // 同一账号可能用不同模型测活。用户比较的是具体模型，优先只看该模型的
    // 记录；没有记录时才退回账号整体，并在推荐理由里写清楚这是参考值。
    // 记录里存的是实际发给上游的 wire name，配置了模型映射时要两个名字都认。
    let target_wire = if model.is_empty() {
        String::new()
    } else {
        provider.model_config.wire_name(model).to_string()
    };
    let matching = provider
        .liveness
        .records
        .iter()
        .rev()
        .filter(|record| {
            !model.is_empty()
                && (record.model.trim().eq_ignore_ascii_case(model)
                    || record.model_id.trim().eq_ignore_ascii_case(model)
                    || (!target_wire.is_empty()
                        && record.model.trim().eq_ignore_ascii_case(&target_wire)))
        })
        .take(LIVENESS_WINDOW)
        .collect::<Vec<_>>();
    let model_matched = !matching.is_empty();
    let recent = if model_matched {
        matching
    } else {
        provider
            .liveness
            .records
            .iter()
            .rev()
            .take(LIVENESS_WINDOW)
            .collect::<Vec<_>>()
    };
    if recent.is_empty() {
        return LivenessStats {
            success_rate: None,
            samples: 0,
            blocked: false,
            stale: false,
            updated_at: None,
            model_matched: false,
        };
    }
    let samples = recent.len();
    let updated_at = recent.first().map(|record| record.checked_at.clone());
    let successes = recent.iter().filter(|record| record.ok).count();
    let success_rate = successes as f64 / samples as f64;
    let latest = recent
        .first()
        .and_then(|record| parse_checked_at(&record.checked_at));
    let age = latest.map(|latest| now.signed_duration_since(latest));
    // 只有最近一次失败、连续多次失败且数据足够新，才认为账号当前不可用；
    // 陈旧的失败记录可能只代表过去的线路问题。
    let blocked = recent.iter().take(LIVENESS_FAILURE_STREAK).count() == LIVENESS_FAILURE_STREAK
        && recent
            .iter()
            .take(LIVENESS_FAILURE_STREAK)
            .all(|record| !record.ok)
        && age.is_some_and(|age| age.num_hours() < 24);
    let stale = age.is_some_and(|age| age > chrono::Duration::days(LIVENESS_STALE_DAYS));
    LivenessStats {
        success_rate: Some(success_rate),
        samples,
        blocked,
        stale,
        updated_at,
        model_matched,
    }
}

fn parse_checked_at(value: &str) -> Option<DateTime<Utc>> {
    parse_timestamp(value)
}

/// 兼容应用里并存的三种时间写法：毫秒数字串、秒数字串和 RFC3339。
fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if let Ok(number) = value.parse::<i64>() {
        // 10 位是 Unix 秒，13 位是 Unix 毫秒；阈值足够区分两者。
        return if number > 1_000_000_000_000 {
            DateTime::from_timestamp_millis(number)
        } else {
            DateTime::from_timestamp(number, 0)
        };
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn health_factor(liveness: &LivenessStats, provider: &Provider) -> f64 {
    let mut factor = match liveness.success_rate {
        Some(rate) if liveness.stale => 0.5 + (rate - 0.5) * 0.5,
        Some(rate) => rate,
        None => 0.5,
    };
    if provider
        .runtime
        .error_message
        .as_deref()
        .is_some_and(|message| !message.trim().is_empty())
    {
        factor *= 0.75;
    }
    if matches!(provider.runtime.status, ProviderStatus::Error) {
        factor *= 0.8;
    }
    factor.clamp(0.0, 1.0)
}

struct QuotaStats {
    factor: f64,
    remaining_percent: Option<f64>,
    label: String,
    resets_at: Option<i64>,
    exhausted: bool,
    exhausted_message: String,
    updated_at: Option<String>,
    stale: bool,
    confidence: f64,
}

fn quota_stats(provider: &Provider, now: DateTime<Utc>) -> QuotaStats {
    let quota = &provider.quota;
    let (updated_at, stale, confidence) = quota_freshness(provider, now);
    let stats = |factor,
                 remaining_percent,
                 label: String,
                 resets_at,
                 exhausted,
                 exhausted_message: String| QuotaStats {
        factor,
        remaining_percent,
        label,
        resets_at,
        exhausted,
        exhausted_message,
        updated_at: updated_at.clone(),
        stale,
        confidence,
    };
    if let Some(window) = quota.tightest_window() {
        let percent = window.percent().unwrap_or_default();
        let remaining = (100.0 - percent).clamp(0.0, 100.0);
        let exhausted = quota.windows_exhausted() == Some(true);
        return stats(
            remaining / 100.0,
            Some(remaining),
            window_label(window, remaining),
            window.resets_at,
            exhausted,
            "订阅额度窗口已用尽".to_string(),
        );
    }
    if quota.unlimited {
        return stats(
            1.0,
            Some(100.0),
            "额度不限".to_string(),
            None,
            false,
            String::new(),
        );
    }
    // 订阅窗口全部未设上限时，钱包余额通常只是溢出账户，不代表套餐余量。
    if !quota.windows.is_empty() {
        return stats(
            1.0,
            Some(100.0),
            "订阅窗口不限量".to_string(),
            None,
            false,
            String::new(),
        );
    }
    // 从未刷新过的账号保留了配置默认值（余额 0），不能据此判定“已用尽”；
    // 只有真正同步过余额的卡片才参与钱包余量比较。
    if quota.known && provider.automation.last_synced_at.is_some() {
        let remaining_percent = (quota.total_known && quota.available + quota.used > 0.0)
            .then(|| quota.available / (quota.available + quota.used) * 100.0)
            .map(|percent| percent.clamp(0.0, 100.0));
        let exhausted = quota.available <= 0.0;
        let factor = match remaining_percent {
            Some(percent) => percent / 100.0,
            // 只知道还有余额、不知道总额时不能和“剩余 90%”直接比较，
            // 给一个略高于未知账号的中性分，避免把正常账号排到后面。
            None if !exhausted => 0.6,
            None => 0.0,
        };
        let symbol = quota.currency_symbol.trim();
        return stats(
            factor,
            remaining_percent,
            format!("余额 {symbol}{:.2}", quota.available),
            None,
            exhausted,
            "余额已用尽".to_string(),
        );
    }
    stats(
        0.5,
        None,
        "余额未知".to_string(),
        None,
        false,
        String::new(),
    )
}

/// 余额读数的可信度：24 小时内为 1，超过一天向中性值收敛，超过两天更低。
/// 没有同步时间但已有窗口/余额时按未知处理，避免把导入数据当成刚刷新。
fn quota_freshness(provider: &Provider, now: DateTime<Utc>) -> (Option<String>, bool, f64) {
    let Some(raw) = provider
        .automation
        .last_synced_at
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    else {
        return (None, true, 0.5);
    };
    let Some(updated) = parse_timestamp(raw) else {
        return (Some(raw.to_string()), true, 0.5);
    };
    let age = now.signed_duration_since(updated);
    if age > chrono::Duration::days(QUOTA_STALE_DAYS * 2) {
        (Some(raw.to_string()), true, 0.25)
    } else if age > chrono::Duration::days(QUOTA_STALE_DAYS) {
        (Some(raw.to_string()), true, 0.5)
    } else {
        (Some(raw.to_string()), false, 1.0)
    }
}

fn window_label(window: &ProviderQuotaWindow, remaining: f64) -> String {
    let label = window.label.trim();
    if label.is_empty() {
        format!("订阅窗口剩余 {remaining:.0}%")
    } else {
        format!("{label}剩余 {remaining:.0}%")
    }
}

fn reset_factor(remaining_percent: Option<f64>, resets_at: Option<i64>, now: DateTime<Utc>) -> f64 {
    let (Some(remaining), Some(hours)) = (
        remaining_percent,
        resets_at.and_then(|resets_at| hours_until(resets_at, now)),
    ) else {
        return 0.0;
    };
    if !(0.0..=RESET_HORIZON_HOURS).contains(&hours) || remaining <= 0.0 {
        return 0.0;
    }
    // 快用完、又快重置的窗口最值得优先使用，避免额度过期浪费。
    (remaining / 100.0) * (1.0 - hours / RESET_HORIZON_HOURS)
}

fn hours_until(resets_at: i64, now: DateTime<Utc>) -> Option<f64> {
    let reset = DateTime::from_timestamp(resets_at, 0)?;
    Some((reset - now).num_minutes() as f64 / 60.0)
}

fn valid_base_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    // 这几个只在测试夹具里构造，父模块没有引入，避免重复导入。
    use crate::models::{LivenessRecord, ProviderInput, ProviderRuntime};

    fn provider(id: &str, name: &str) -> Provider {
        let mut input = ProviderInput::default();
        input.identity.name = name.to_string();
        input.identity.base_url = format!("https://{id}.example.com");
        let mut provider = Provider::from_input(input, id.to_string());
        provider.quota.known = false;
        provider.quota.total_known = false;
        provider
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn record_for(model: &str, ok: bool, minutes_ago: i64) -> LivenessRecord {
        let mut record = record(ok, minutes_ago);
        record.model = model.to_string();
        record
    }

    fn record(ok: bool, minutes_ago: i64) -> LivenessRecord {
        LivenessRecord {
            checked_at: (now().timestamp_millis() - minutes_ago * 60_000).to_string(),
            source: "test".to_string(),
            cli_kind: String::new(),
            ok,
            latency_ms: 100,
            model: "test-model".to_string(),
            model_id: String::new(),
            base_url: String::new(),
            provider_base_url: String::new(),
            prompt: String::new(),
            response_preview: String::new(),
            response_raw: String::new(),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            reasoning_output_tokens: None,
            total_tokens: None,
            total_cost_usd: None,
            message: String::new(),
            command_preview: String::new(),
        }
    }

    #[test]
    fn ranks_healthy_provider_above_exhausted_one() {
        let healthy = provider("healthy", "Healthy");
        let mut exhausted = provider("exhausted", "Exhausted");
        exhausted.quota.known = true;
        exhausted.quota.total_known = true;
        exhausted.quota.available = 0.0;
        exhausted.quota.used = 10.0;
        exhausted.automation.last_synced_at = Some("1800000000".to_string());
        let report = recommend(&[exhausted, healthy], "", now());
        assert_eq!(report.recommended_provider_id.as_deref(), Some("healthy"));
        assert!(!report.entries[1].eligible);
        assert!(report.entries[1]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("余额已用尽")));
    }

    #[test]
    fn blocks_provider_that_does_not_list_the_model() {
        let mut listed = provider("listed", "Listed");
        listed.capabilities.available_models = vec!["claude-sonnet-4-5".to_string()];
        let mut missing = provider("missing", "Missing");
        missing.capabilities.available_models = vec!["gpt-5".to_string()];
        let report = recommend(&[listed, missing], "claude-sonnet-4-5", now());
        assert_eq!(report.recommended_provider_id.as_deref(), Some("listed"));
        assert_eq!(report.entries[1].model_supported, Some(false));
        assert!(report.entries[1]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("不包含")));
    }

    #[test]
    fn unlimited_windows_do_not_fall_back_to_empty_wallet() {
        let mut provider = provider("unlimited", "Unlimited");
        provider.quota.known = true;
        provider.automation.last_synced_at = Some("1800000000".to_string());
        provider.quota.windows = vec![ProviderQuotaWindow {
            label: "月".to_string(),
            used: 0.0,
            limit: 0.0,
            resets_at: None,
            history: Vec::new(),
        }];
        let report = recommend(&[provider], "", now());
        assert!(report.entries[0].eligible);
        assert_eq!(report.entries[0].quota_label, "订阅窗口不限量");
    }

    #[test]
    fn provider_that_never_synced_is_not_declared_exhausted() {
        let mut provider = provider("fresh", "Fresh");
        provider.quota.known = true;
        provider.quota.total_known = true;
        provider.quota.available = 0.0;
        let report = recommend(&[provider], "", now());
        assert!(report.entries[0].eligible);
        assert_eq!(report.entries[0].quota_label, "余额未知");
    }

    #[test]
    fn unknown_model_list_is_not_a_blocker() {
        let mut provider = provider("unknown", "Unknown");
        provider.capabilities.available_models.clear();
        let report = recommend(&[provider], "claude-sonnet-4-5", now());
        assert_eq!(report.entries[0].model_supported, None);
        assert!(report.entries[0].eligible);
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("模型列表未同步")));
    }

    #[test]
    fn blocks_recent_failure_streak() {
        let mut provider = provider("failing", "Failing");
        provider.liveness.records = vec![
            record(true, 40),
            record(false, 30),
            record(false, 20),
            record(false, 10),
        ];
        let report = recommend(&[provider], "", now());
        assert!(!report.entries[0].eligible);
        assert!(report.entries[0]
            .blockers
            .iter()
            .any(|blocker| blocker.contains("测活均失败")));
    }

    #[test]
    fn liveness_uses_the_recommended_model_when_records_exist() {
        let mut provider = provider("multi-model", "Multi Model");
        provider.capabilities.available_models = vec!["target-model".to_string()];
        provider.liveness.records = vec![
            record_for("other-model", false, 30),
            record_for("other-model", false, 20),
            record_for("other-model", false, 10),
            record_for("target-model", true, 5),
        ];
        let report = recommend(&[provider], "target-model", now());
        let entry = &report.entries[0];
        assert!(
            entry.eligible,
            "other models' failures must not block the target model"
        );
        assert_eq!(entry.liveness_success_rate, Some(1.0));
        assert!(entry
            .reasons
            .iter()
            .any(|reason| reason.contains("该模型测活")));
    }

    #[test]
    fn liveness_records_match_the_model_wire_name() {
        let mut provider = provider("mapped-liveness", "Mapped Liveness");
        provider.capabilities.available_models = vec!["upstream-target".to_string()];
        provider.model_config.overrides.insert(
            "display-target".to_string(),
            crate::models::ProviderModelOverride {
                wire_name: "upstream-target".to_string(),
                ..Default::default()
            },
        );
        // 测活记录保存实际发给上游的 wire name；推荐按展示模型查询时
        // 也必须命中，否则会把已有记录当成“该模型没有测活”。
        provider.liveness.records = vec![record_for("upstream-target", true, 5)];
        let report = recommend(&[provider], "display-target", now());
        let entry = &report.entries[0];
        assert_eq!(entry.liveness_samples, 1);
        assert_eq!(entry.liveness_success_rate, Some(1.0));
        assert!(entry
            .reasons
            .iter()
            .any(|reason| reason.contains("该模型测活")));
    }

    #[test]
    fn balance_just_past_one_day_is_already_stale() {
        let mut provider = provider("stale-hours", "Stale Hours");
        provider.quota.known = true;
        provider.quota.total_known = true;
        provider.quota.available = 50.0;
        provider.quota.used = 50.0;
        provider.automation.last_synced_at = Some((now().timestamp() - 47 * 3_600).to_string());
        let report = recommend(&[provider], "", now());
        assert!(
            report.entries[0].quota_stale,
            "47 小时应按超过 1 天处理，不能因为 num_days 截断而继续算新鲜"
        );
    }

    #[test]
    fn site_price_just_past_seven_days_is_already_stale() {
        let mut provider = provider("stale-price-hours", "Stale Price Hours");
        provider.capabilities.available_models = vec!["model-a".to_string()];
        provider.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(1.0),
                ..crate::models::ProviderModelPrice::default()
            },
        );
        provider.capabilities.pricing.source = crate::models::ProviderPricingSource::NewApi;
        provider.capabilities.pricing.updated_at =
            Some((now().timestamp() - (7 * 86_400 + 3_600)).to_string());
        let report = recommend(&[provider], "model-a", now());
        assert!(report.entries[0].pricing_stale);
    }

    #[test]
    fn liveness_just_past_seven_days_is_already_stale() {
        let mut provider = provider("stale-liveness-hours", "Stale Liveness Hours");
        provider.liveness.records = vec![record(true, 7 * 24 * 60 + 60)];
        let report = recommend(&[provider], "", now());
        assert!(report.entries[0].liveness_stale);
    }

    #[test]
    fn model_liveness_failures_block_even_when_another_model_succeeds() {
        let mut provider = provider("multi-model", "Multi Model");
        provider.capabilities.available_models = vec!["target-model".to_string()];
        provider.liveness.records = vec![
            record_for("other-model", true, 40),
            record_for("target-model", false, 30),
            record_for("target-model", false, 20),
            record_for("target-model", false, 10),
        ];
        let report = recommend(&[provider], "target-model", now());
        assert!(!report.entries[0].eligible);
    }

    #[test]
    fn cooldown_blocks_a_provider_and_is_explained() {
        let mut provider = provider("cooling", "Cooling");
        provider.runtime.cooldown_until = Some(now().timestamp() + 3_600);
        provider.runtime.cooldown_reason = Some("凭据被拒绝".to_string());
        let report = recommend(&[provider], "", now());
        let entry = &report.entries[0];
        assert!(!entry.eligible);
        assert_eq!(entry.cooldown_until, Some(now().timestamp() + 3_600));
        assert!(entry
            .blockers
            .iter()
            .any(|blocker| blocker.contains("凭据被拒绝")));
    }

    #[test]
    fn stale_balance_is_flagged_and_downgraded() {
        let mut provider = provider("stale-balance", "Stale balance");
        provider.quota.known = true;
        provider.quota.total_known = true;
        provider.quota.available = 50.0;
        provider.quota.used = 50.0;
        provider.automation.last_synced_at = Some((now().timestamp() - 3 * 86_400).to_string());
        let expected_updated_at = (now().timestamp() - 3 * 86_400).to_string();
        let report = recommend(&[provider], "", now());
        let entry = &report.entries[0];
        assert!(entry.quota_stale);
        assert_eq!(
            entry.quota_updated_at.as_deref(),
            Some(expected_updated_at.as_str())
        );
        assert!(entry
            .reasons
            .iter()
            .any(|reason| reason.contains("余额数据已超过")));
    }

    #[test]
    fn stale_site_price_is_flagged_and_only_used_as_reference() {
        let mut provider = provider("stale-price", "Stale price");
        provider.capabilities.available_models = vec!["model-a".to_string()];
        provider.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(1.0),
                ..crate::models::ProviderModelPrice::default()
            },
        );
        provider.capabilities.pricing.source = crate::models::ProviderPricingSource::NewApi;
        provider.capabilities.pricing.updated_at =
            Some((now().timestamp() - 10 * 86_400).to_string());
        let report = recommend(&[provider], "model-a", now());
        let entry = &report.entries[0];
        assert!(entry.pricing_stale);
        assert!(entry
            .reasons
            .iter()
            .any(|reason| reason.contains("价格数据已超过")));
    }

    #[test]
    fn stale_failures_do_not_block() {
        let mut provider = provider("stale", "Stale");
        provider.liveness.records = vec![
            record(false, 60 * 24 * 10),
            record(false, 60 * 24 * 10 - 10),
            record(false, 60 * 24 * 10 - 20),
        ];
        let report = recommend(&[provider], "", now());
        assert!(report.entries[0].eligible);
        assert!(report.entries[0].liveness_stale);
        assert!(report.entries[0].liveness_updated_at.is_some());
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("超过 7 天")));
    }

    #[test]
    fn cheaper_provider_wins_when_health_is_equal() {
        let mut cheap = provider("cheap", "Cheap");
        let mut pricey = provider("pricey", "Pricey");
        for provider in [&mut cheap, &mut pricey] {
            provider.capabilities.available_models = vec!["model-a".to_string()];
        }
        cheap.model_config.overrides.insert(
            "model-a".to_string(),
            crate::models::ProviderModelOverride {
                price: ModelPrice {
                    input: Some(1.0),
                    output: Some(2.0),
                    ..ModelPrice::default()
                },
                ..Default::default()
            },
        );
        pricey.model_config.overrides.insert(
            "model-a".to_string(),
            crate::models::ProviderModelOverride {
                price: ModelPrice {
                    input: Some(8.0),
                    output: Some(16.0),
                    ..ModelPrice::default()
                },
                ..Default::default()
            },
        );
        let report = recommend(&[pricey, cheap], "model-a", now());
        assert_eq!(report.recommended_provider_id.as_deref(), Some("cheap"));
        assert!(report.entries[0].score > report.entries[1].score);
    }

    #[test]
    fn subscription_window_remaining_and_reset_are_reported() {
        let mut provider = provider("window", "Window");
        provider.quota.windows = vec![ProviderQuotaWindow {
            label: "周".to_string(),
            used: 90.0,
            limit: 100.0,
            resets_at: Some(now().timestamp() + 2 * 3600),
            history: Vec::new(),
        }];
        let report = recommend(&[provider], "", now());
        let entry = &report.entries[0];
        assert_eq!(entry.quota_remaining_percent, Some(10.0));
        assert!(entry.quota_label.contains("周剩余 10%"));
        assert!(entry
            .reasons
            .iter()
            .any(|reason| reason.contains("2 小时后重置")));
    }

    #[test]
    fn disabled_provider_is_never_recommended() {
        let healthy = provider("healthy", "Healthy");
        let mut disabled = provider("disabled", "Disabled");
        disabled.runtime = ProviderRuntime {
            enabled: false,
            status: ProviderStatus::Ok,
            error_message: None,
            consecutive_failures: 0,
            cooldown_until: None,
            cooldown_reason: None,
            credential_rejected: false,
        };
        let report = recommend(&[healthy, disabled], "", now());
        assert_eq!(report.recommended_provider_id.as_deref(), Some("healthy"));
        assert!(report.entries[1]
            .blockers
            .iter()
            .any(|blocker| blocker == "已停用"));
    }

    #[test]
    fn model_wire_name_matches_available_list() {
        let mut provider = provider("wire", "Wire");
        provider.capabilities.available_models = vec!["upstream-model-id".to_string()];
        provider.model_config.overrides.insert(
            "display-model".to_string(),
            crate::models::ProviderModelOverride {
                wire_name: "upstream-model-id".to_string(),
                ..Default::default()
            },
        );
        let report = recommend(&[provider], "display-model", now());
        assert_eq!(report.entries[0].model_supported, Some(true));
        assert_eq!(report.entries[0].model_wire_name, "upstream-model-id");
    }

    #[test]
    fn site_price_lookup_follows_the_model_wire_name() {
        let mut provider = provider("wire-price", "Wire Price");
        provider.capabilities.pricing.models.insert(
            "upstream-model-id".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(2.0),
                output: Some(4.0),
                group: "vip".to_string(),
                ..Default::default()
            },
        );
        provider.model_config.overrides.insert(
            "display-model".to_string(),
            crate::models::ProviderModelOverride {
                wire_name: "upstream-model-id".to_string(),
                ..Default::default()
            },
        );
        let report = recommend(&[provider], "display-model", now());
        assert_eq!(report.entries[0].input_price, Some(2.0));
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("站点价格（分组 vip）")));
    }

    #[test]
    fn empty_model_skips_model_filtering() {
        let mut provider = provider("any", "Any");
        provider.capabilities.available_models = vec!["model-a".to_string()];
        let report = recommend(&[provider], "", now());
        assert_eq!(report.entries[0].model_supported, None);
        assert!(report.entries[0].eligible);
        assert!(report.entries[0].model.is_empty());
    }

    #[test]
    fn site_prices_drive_the_ranking_and_are_explained() {
        let mut cheap = provider("cheap-site", "Cheap Site");
        let mut pricey = provider("pricey-site", "Pricey Site");
        for provider in [&mut cheap, &mut pricey] {
            provider.capabilities.available_models = vec!["model-a".to_string()];
        }
        cheap.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(1.0),
                output: Some(2.0),
                group: "default".to_string(),
                ..Default::default()
            },
        );
        pricey.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(8.0),
                output: Some(16.0),
                group: "default".to_string(),
                ..Default::default()
            },
        );
        let report = recommend(&[pricey, cheap], "model-a", now());
        assert_eq!(
            report.recommended_provider_id.as_deref(),
            Some("cheap-site")
        );
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("站点价格（分组 default）")));
    }

    #[test]
    fn manual_price_still_overrides_site_price() {
        let mut provider = provider("manual", "Manual");
        provider.capabilities.available_models = vec!["model-a".to_string()];
        provider.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                input: Some(5.0),
                output: Some(10.0),
                ..Default::default()
            },
        );
        provider.model_config.overrides.insert(
            "model-a".to_string(),
            crate::models::ProviderModelOverride {
                price: ModelPrice {
                    input: Some(1.0),
                    ..ModelPrice::default()
                },
                ..Default::default()
            },
        );
        let report = recommend(&[provider], "model-a", now());
        assert_eq!(report.entries[0].input_price, Some(1.0));
        assert_eq!(report.entries[0].output_price, Some(10.0));
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("卡片覆盖价格")));
    }

    #[test]
    fn per_request_site_prices_do_not_pose_as_token_prices() {
        let mut provider = provider("per-request", "Per Request");
        provider.capabilities.available_models = vec!["model-a".to_string()];
        provider.capabilities.pricing.models.insert(
            "model-a".to_string(),
            crate::models::ProviderModelPrice {
                per_request: Some(0.02),
                ..Default::default()
            },
        );
        let report = recommend(&[provider], "model-a", now());
        assert_eq!(report.entries[0].input_price, None);
        assert_eq!(report.entries[0].per_request_price, Some(0.02));
        assert!(report.entries[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("按次 $0.02/次")));
    }

    #[test]
    fn site_model_performance_breaks_ties_between_equal_accounts() {
        let mut stable = provider("stable", "Stable");
        let mut flaky = provider("flaky", "Flaky");
        for provider in [&mut stable, &mut flaky] {
            provider.capabilities.available_models = vec!["model-a".to_string()];
            provider.model_config.overrides.insert(
                "model-a".to_string(),
                crate::models::ProviderModelOverride {
                    price: ModelPrice {
                        input: Some(2.0),
                        output: Some(4.0),
                        ..ModelPrice::default()
                    },
                    ..Default::default()
                },
            );
        }
        stable.capabilities.performance.models.insert(
            "model-a".to_string(),
            ProviderModelPerformance {
                success_rate: Some(99.5),
                avg_tps: Some(48.0),
                avg_latency_ms: Some(900.0),
                ..ProviderModelPerformance::default()
            },
        );
        flaky.capabilities.performance.models.insert(
            "model-a".to_string(),
            ProviderModelPerformance {
                success_rate: Some(70.0),
                ..ProviderModelPerformance::default()
            },
        );

        let report = recommend(&[flaky, stable], "model-a", now());

        assert_eq!(report.recommended_provider_id.as_deref(), Some("stable"));
        let top = &report.entries[0];
        assert_eq!(top.site_success_rate, Some(0.995));
        assert_eq!(top.site_avg_tps, Some(48.0));
        assert_eq!(top.site_avg_latency_ms, Some(900.0));
        assert!(top
            .reasons
            .iter()
            .any(|reason| reason.contains("站点近 24 小时成功率 99.5%")));
        assert!(top.reasons.iter().any(|reason| reason.contains("48.0 t/s")));
    }

    #[test]
    fn record_checked_at_accepts_millis_and_rfc3339() {
        assert!(parse_checked_at("1800000000000").is_some());
        assert!(parse_checked_at("2027-01-15T08:00:00Z").is_some());
        assert!(parse_checked_at("").is_none());
    }
}
