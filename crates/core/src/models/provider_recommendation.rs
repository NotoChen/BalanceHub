use serde::{Deserialize, Serialize};

/// 一个中转站针对某个模型的推荐结果。
///
/// `score` 只用于同一份报告内的排序，不是可以跨设备比较的绝对评分；
/// 用户真正需要的是 `reasons` / `blockers` 里能读懂的依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecommendation {
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub score: u8,
    pub eligible: bool,
    /// 本次排序使用的模型；空字符串表示不按模型筛选。
    pub model: String,
    /// `None` 表示站点模型列表尚未同步，无法判断。
    pub model_supported: Option<bool>,
    pub model_wire_name: String,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    /// 最近测活窗口内的成功率（0..1）；没有记录时为 None。
    pub liveness_success_rate: Option<f64>,
    pub liveness_samples: usize,
    /// 额度剩余比例（0..100）；无法换算时为 None。
    pub quota_remaining_percent: Option<f64>,
    /// 卡片上展示的额度摘要，已经包含货币符号或窗口名称。
    pub quota_label: String,
    pub resets_at: Option<i64>,
    pub reasons: Vec<String>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecommendationReport {
    pub model: String,
    pub generated_at: String,
    /// 排序后第一个可用账号；没有可用账号时为 None。
    pub recommended_provider_id: Option<String>,
    pub entries: Vec<ProviderRecommendation>,
}
