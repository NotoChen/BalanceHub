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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelPrice {
    /// USD / 百万 token。
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// 按次计费的单价（USD / 次）。有值时该模型不再按 token 估算。
    pub per_request: Option<f64>,
    /// 该价格对应的站点分组；空表示站点没有返回分组信息。
    pub group: String,
}

impl ProviderModelPrice {
    pub fn has_token_price(&self) -> bool {
        [self.input, self.output, self.cache_read, self.cache_write]
            .into_iter()
            .any(|price| price.is_some())
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
    ) {
        self.models = models;
        self.groups = groups;
        self.group = group;
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
        );
        snapshot
    }

    #[test]
    fn transient_error_keeps_the_last_successful_prices() {
        let mut snapshot = loaded();
        snapshot.mark_error("网络超时".to_string());
        assert!(snapshot.model("model-a").is_some());
        assert_eq!(snapshot.source, ProviderPricingSource::NewApi);
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
