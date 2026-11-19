//! 站点真实价格快照。
//!
//! 推荐、本地用量对账和模型详情都从这里取价，不再用 models.dev 的全局参考价
//! 冒充某个中转站的实际收费。价格统一换算为 USD / 百万 token；按次计费的模型
//! 单独记 `per_request`，不与 token 单价混排。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::models::ModelPrice;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderPricingSource {
    #[default]
    None,
    NewApi,
    Sub2Api,
}

/// 站点的计费形态。图片计费的 token 单价仍按 token 口径保存，分辨率档位价
/// 放在 `tiers` 的 `per_request` 里。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderBillingMode {
    #[default]
    Token,
    PerRequest,
    Image,
    /// 视频生成按次计费，可配置独立倍率。
    Video,
}

/// 站点给出的官方参考价（USD / 百万 token）。Sub2API 用它对照官方目录价，
/// 据此看出当前分组实际是折扣还是溢价。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderOfficialPrice {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl ProviderOfficialPrice {
    pub fn has_price(&self) -> bool {
        [self.input, self.output, self.cache_read, self.cache_write]
            .into_iter()
            .any(|price| price.is_some())
    }
}

/// 长上下文阶梯的计价基准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderLongContextBasis {
    /// 整单请求按命中的档位计价。
    WholeRequest,
    /// 只有超出前一档的 token 按新档计价。
    Marginal,
}

/// 站点返回的一档价格：长上下文阶梯，或图片计费的 1K / 2K / 4K 档位。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderPricingTier {
    /// 输入 token 的档位起点。
    pub min_tokens: u64,
    /// 档位终点；最后一档为空。
    pub max_tokens: Option<u64>,
    /// 站点给的可读档位名，如 `≤272K` / `2K`；可能为空。
    pub label: String,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// 按次或按张计费的档位单价（USD / 次、USD / 张）。
    pub per_request: Option<f64>,
}

impl ProviderPricingTier {
    pub fn has_price(&self) -> bool {
        self.input.is_some()
            || self.output.is_some()
            || self.cache_read.is_some()
            || self.cache_write.is_some()
            || self.per_request.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderTimePricingPeriod {
    /// 站点配置时区的 HH:MM。
    pub start: String,
    pub end: String,
    pub multiplier: f64,
}

/// 分时倍率：落在时段内的请求整单乘倍率。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderTimePricing {
    pub timezone: String,
    /// 仅周一至周五生效；周末整天按标准价。
    pub weekdays_only: bool,
    pub periods: Vec<ProviderTimePricingPeriod>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelPrice {
    pub billing: ProviderBillingMode,
    /// USD / 百万 token。
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// 按次计费的单价（USD / 次）。有值时该模型不再按 token 估算。
    pub per_request: Option<f64>,
    /// 该价格对应的站点分组；空表示站点没有返回分组信息。
    pub group: String,
    /// 站点声明的上下文窗口（token）。比 models.dev 更贴近这个中转站的实际配置。
    pub context_window: Option<u64>,
    /// 站点声明的最大输出 token。
    pub max_output_tokens: Option<u64>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    /// 站点声明的能力标签，如 `reasoning`、`function_calling`。
    pub capabilities: Vec<String>,
    /// 官方参考价；站点没有提供时为空。
    pub official: Option<ProviderOfficialPrice>,
    /// 长上下文阶梯或图片分辨率档位；站点没有多档定价时为空。
    pub tiers: Vec<ProviderPricingTier>,
    /// 多档定价的计价基准；只有长上下文阶梯会给出。
    pub long_context_basis: Option<ProviderLongContextBasis>,
    /// 分时倍率；站点没有配置时为空。
    pub time_pricing: Option<ProviderTimePricing>,
}

impl ProviderModelPrice {
    pub fn has_token_price(&self) -> bool {
        [self.input, self.output, self.cache_read, self.cache_write]
            .into_iter()
            .any(|price| price.is_some())
    }

    /// 站点对该模型给出了任何可展示的价格。
    pub fn has_any_price(&self) -> bool {
        self.has_token_price() || self.per_request.is_some() || !self.tiers.is_empty()
    }

    pub fn token_price(&self) -> ModelPrice {
        ModelPrice {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

/// 当前价格所采用分组的计费口径。价格已经按生效倍率换算；这里保留倍率与
/// 高峰时段，供 UI 说明为什么账单可能高于列表价。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderGroupPricing {
    pub rate_multiplier: Option<f64>,
    pub user_rate_multiplier: Option<f64>,
    pub peak_rate_enabled: bool,
    /// 高峰时段起止（站点配置时区的 HH:MM）。
    pub peak_start: String,
    pub peak_end: String,
    pub peak_rate_multiplier: Option<f64>,
    /// 分组是否启用了长上下文阶梯计费；站点没有给出时为 None。
    pub long_context_pricing_enabled: Option<bool>,
}

/// 一张卡片最近一次成功读取的站点价目。
///
/// 读取失败时保留上一次成功的模型价格，只更新 `error`；推荐和对账因此不会
/// 因为一次网络抖动突然失去价格依据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderPricingSnapshot {
    pub models: BTreeMap<String, ProviderModelPrice>,
    /// 站点可选分组及倍率；用于说明价格按哪个分组计算。
    pub groups: BTreeMap<String, f64>,
    /// 本次价格采用的分组。
    pub group: String,
    /// 分组的倍率、高峰时段与阶梯开关。
    pub group_pricing: ProviderGroupPricing,
    pub source: ProviderPricingSource,
    /// unix 毫秒字符串，与模型列表状态保持一致。
    pub updated_at: Option<String>,
    pub error: Option<String>,
}

impl ProviderPricingSnapshot {
    pub fn model(&self, model: &str) -> Option<&ProviderModelPrice> {
        self.models.get(model)
    }

    /// 成功读取空价目也代表价格能力可用，但推荐不应把空快照当作价格来源。
    pub fn mark_loaded(
        &mut self,
        source: ProviderPricingSource,
        models: BTreeMap<String, ProviderModelPrice>,
        groups: BTreeMap<String, f64>,
        group: String,
        group_pricing: ProviderGroupPricing,
    ) {
        self.models = models;
        self.groups = groups;
        self.group = group;
        self.group_pricing = group_pricing;
        self.source = source;
        self.updated_at = Some(crate::util::unix_millis().to_string());
        self.error = None;
    }

    pub fn mark_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// 站点明确不再提供价目接口时清空旧价格，避免推荐继续使用已经失效的
    /// 站点实价；网络或认证错误仍走 `mark_error`，保留上一次成功读数。
    pub fn mark_unsupported(&mut self, error: String) {
        self.models.clear();
        self.groups.clear();
        self.group.clear();
        self.group_pricing = ProviderGroupPricing::default();
        self.source = ProviderPricingSource::None;
        self.updated_at = None;
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> ProviderPricingSnapshot {
        let mut snapshot = ProviderPricingSnapshot::default();
        snapshot.mark_loaded(
            ProviderPricingSource::NewApi,
            BTreeMap::from([(
                "model-a".to_string(),
                ProviderModelPrice {
                    input: Some(1.0),
                    ..ProviderModelPrice::default()
                },
            )]),
            BTreeMap::from([("vip".to_string(), 1.5)]),
            "vip".to_string(),
            ProviderGroupPricing {
                rate_multiplier: Some(1.5),
                ..ProviderGroupPricing::default()
            },
        );
        snapshot
    }

    #[test]
    fn transient_error_keeps_the_last_successful_prices() {
        let mut snapshot = loaded();
        snapshot.mark_error("网络超时".to_string());
        assert!(snapshot.model("model-a").is_some());
        assert_eq!(snapshot.source, ProviderPricingSource::NewApi);
        assert_eq!(snapshot.group_pricing.rate_multiplier, Some(1.5));
        assert_eq!(snapshot.error.as_deref(), Some("网络超时"));
    }

    #[test]
    fn confirmed_unsupported_pricing_clears_stale_site_prices() {
        let mut snapshot = loaded();
        snapshot.mark_unsupported("站点未提供价目接口".to_string());
        assert!(snapshot.models.is_empty());
        assert_eq!(snapshot.source, ProviderPricingSource::None);
        assert!(snapshot.updated_at.is_none());
        assert_eq!(snapshot.error.as_deref(), Some("站点未提供价目接口"));
    }
}
