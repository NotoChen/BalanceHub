use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    /// USD per million tokens; catalog estimates, never a relay's actual bill.
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
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadataSnapshot {
    pub models: Vec<ModelMetadata>,
    pub updated_at: Option<String>,
    pub stale: bool,
    pub error: Option<String>,
}
