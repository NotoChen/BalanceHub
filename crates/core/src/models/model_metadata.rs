use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    /// USD / 百万 token；来源由所在模型的 `price_source` 指明。
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelPriceSource {
    #[default]
    None,
    /// models.dev 的公开参考价。
    Catalog,
    /// 中转站价目接口返回的真实价格。
    Site,
    /// 卡片上手动覆盖的价格。
    Manual,
}

/// 站点计费形态；决定模型广场按 token、按次还是按张展示价格。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelBillingMode {
    #[default]
    Token,
    PerRequest,
    Image,
    /// 视频生成按次计费。
    Video,
}

/// 长上下文阶梯的计价基准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelLongContextBasis {
    /// 整单请求按命中的档位计价。
    WholeRequest,
    /// 只有超出前一档的 token 按新档计价。
    Marginal,
}

/// 站点返回的一档价格：长上下文阶梯，或图片计费的 1K / 2K / 4K 档位。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceTier {
    /// 站点给的可读档位名，如 `≤272K`、`2K`；可能为空。
    pub label: String,
    pub min_tokens: u64,
    pub max_tokens: Option<u64>,
    /// 档位内的 token 单价（USD / 百万 token）。
    pub price: ModelPrice,
    /// 按次或按张的档位单价（USD / 次、USD / 张）。
    pub per_request: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTimePricingPeriod {
    pub start: String,
    pub end: String,
    pub multiplier: f64,
}

/// 分时倍率；仅包含倍率不为 1 的时段。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTimePricing {
    pub timezone: String,
    pub weekdays_only: bool,
    pub periods: Vec<ModelTimePricingPeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadata {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: Option<u64>,
    pub output_limit: Option<u64>,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub price: ModelPrice,
    /// 站点按次计费单价（USD / 次）。有值时该模型不参与 token 价格估算，
    /// `price` 也不再保留目录参考价。
    #[serde(default)]
    pub per_request: Option<f64>,
    /// 价格来源；UI 据此区分公开参考价、站点实价和卡片覆盖价。
    #[serde(default)]
    pub price_source: ModelPriceSource,
    /// 站点价格采用的分组；非站点价格时为空。
    #[serde(default)]
    pub price_group: String,
    /// 站点计费形态；没有站点价格时为 Token。
    #[serde(default)]
    pub billing: ModelBillingMode,
    /// 官方参考价（USD / 百万 token）；站点没有提供时为空。
    #[serde(default)]
    pub official_price: Option<ModelPrice>,
    /// 长上下文阶梯或图片分辨率档位；站点没有多档定价时为空。
    #[serde(default)]
    pub price_tiers: Vec<ModelPriceTier>,
    /// 多档定价的计价基准；只有长上下文阶梯会给出。
    #[serde(default)]
    pub long_context_basis: Option<ModelLongContextBasis>,
    /// 分时倍率；站点没有配置时为空。
    #[serde(default)]
    pub time_pricing: Option<ModelTimePricing>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadataSnapshot {
    pub models: Vec<ModelMetadata>,
    pub updated_at: Option<String>,
    pub stale: bool,
    pub error: Option<String>,
}
