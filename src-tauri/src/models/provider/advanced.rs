//! 自定义接入的高级配置：请求头、API Key 发送方式、余额接口声明和模型目录覆盖。
//!
//! 这些配置都只描述「这个中转站怎么被读取」，不改变 BalanceHub 的请求路径：
//! 应用仍然只做协议级查询和 Agent 配置写入，不代理模型请求。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::normalize::{normalize_model_config, normalize_quota_spec, normalize_request_config};
use super::ProviderInput;
use crate::limits;
use crate::models::provider_domain::endpoints::{
    validate_endpoint_overrides, ProviderEndpointOverride,
};

/// 一张中转站卡片对上游的请求约定。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderRequestConfig {
    /// 额外请求头，按填写顺序附加到该中转站的每个请求。
    pub headers: Vec<ProviderHeader>,
    /// API Key 的发送方式；Bearer 是默认值。
    pub auth_scheme: ProviderAuthScheme,
    /// 按接口改写的路径、查询参数和请求头；未声明的接口沿用协议默认值。
    pub endpoints: Vec<ProviderEndpointOverride>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderHeader {
    pub name: String,
    pub value: String,
}

impl ProviderHeader {
    pub(crate) fn is_usable(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderAuthScheme {
    /// `Authorization: Bearer <key>`。
    #[default]
    Bearer,
    /// `X-Api-Key: <key>`。
    XApiKey,
    /// 不自动附加凭据，完全由自定义请求头提供。
    None,
}

/// 通用 API Key 中转站的余额读取声明。表达式求值结果按 `per_unit` 换算后
/// 直接写入 `ProviderQuota`，卡片和窗口条不需要知道数据来自哪个接口。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderQuotaSpec {
    /// 余额接口地址；支持绝对地址和以中转站地址为基准的相对路径。
    pub url: String,
    pub method: ProviderQuotaMethod,
    /// 访问令牌；非空时以 `Authorization: Bearer` 发送。
    pub token: String,
    pub headers: Vec<ProviderHeader>,
    /// 钱包剩余额度表达式。
    pub balance: String,
    /// 钱包总额表达式；与余额一起用来计算已用比例。
    pub total: String,
    /// 订阅窗口；每项按自己的表达式读取。
    pub windows: Vec<ProviderQuotaWindowSpec>,
    /// 表达式数值的换算单位；0 表示不换算，表达式数值直接作为展示金额。
    pub per_unit: f64,
    /// 表达式数值的货币符号；空表示沿用卡片默认值。
    pub currency_symbol: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderQuotaMethod {
    #[default]
    Get,
    Post,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderQuotaWindowSpec {
    pub label: String,
    pub used: String,
    pub limit: String,
    pub resets_at: String,
}

impl ProviderQuotaSpec {
    pub(crate) fn is_configured(&self) -> bool {
        !self.url.trim().is_empty()
    }
}

/// 模型目录的展示与元数据覆盖。`available_models` 始终保存站点返回的原始
/// 列表，覆盖层只决定「展示哪些」和「怎么读」。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelConfig {
    /// 模型列表地址；空表示使用协议默认地址。只对通用 OpenAI 兼容协议生效。
    pub models_url: String,
    /// 只展示匹配这些 glob / `re:` 模式的模型；空表示全部展示。
    pub include: Vec<String>,
    /// 隐藏匹配这些模式的模型；优先级高于 include。
    pub exclude: Vec<String>,
    /// 借用 models.dev 中该供应商的元数据补全缺失值；空表示按模型 ID 自动匹配。
    pub catalog: String,
    /// 按模型 ID 覆盖的元数据。
    pub overrides: BTreeMap<String, ProviderModelOverride>,
}

impl ProviderModelConfig {
    pub(crate) fn allows(&self, model: &str) -> bool {
        if self.overrides.get(model).is_some_and(|over| over.hidden) {
            return false;
        }
        if self
            .exclude
            .iter()
            .any(|pattern| model_matches(pattern, model))
        {
            return false;
        }
        self.include.is_empty()
            || self
                .include
                .iter()
                .any(|pattern| model_matches(pattern, model))
    }

    pub(crate) fn visible<'a>(&'a self, models: &'a [String]) -> Vec<String> {
        models
            .iter()
            .filter(|model| self.allows(model))
            .cloned()
            .collect()
    }

    pub(crate) fn wire_name<'a>(&'a self, model: &'a str) -> &'a str {
        self.overrides
            .get(model)
            .map(|over| over.wire_name.trim())
            .filter(|wire| !wire.is_empty())
            .unwrap_or(model)
    }

    /// 用量排行归并到的模型 ID；未声明时保持原样。
    pub(crate) fn usage_alias<'a>(&'a self, model: &'a str) -> &'a str {
        self.overrides
            .get(model)
            .map(|over| over.alias.trim())
            .filter(|alias| !alias.is_empty())
            .unwrap_or(model)
    }

    pub(crate) fn reasoning_levels(&self, model: &str) -> &[String] {
        self.overrides
            .get(model)
            .map(|over| over.reasoning_levels.as_slice())
            .unwrap_or_default()
    }

    /// `false` 表示该模型不接受推理字段；`None` 表示未声明。
    pub(crate) fn sends_reasoning(&self, model: &str) -> Option<bool> {
        self.overrides
            .get(model)
            .and_then(|over| over.sends_reasoning)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModelOverride {
    /// Agent 模型选择器里显示的名称；空表示使用模型 ID。
    pub display_name: String,
    /// 实际发给上游的模型 ID；空表示与模型 ID 相同。
    pub wire_name: String,
    /// 用量统计归并到的模型 ID；空表示不归并。
    pub alias: String,
    pub context_window: Option<u64>,
    pub output_limit: Option<u64>,
    pub price: crate::models::ModelPrice,
    /// 原生支持的推理档位；空表示未声明。
    pub reasoning_levels: Vec<String>,
    /// `false` 表示该模型不接受推理字段；`None` 表示未声明。
    pub sends_reasoning: Option<bool>,
    /// 从模型选择器隐藏。
    pub hidden: bool,
}

impl ProviderInput {
    /// 把输入里的高级配置规范化后写回草稿：保存和再次打开时看到的是同一份值。
    pub(crate) fn normalize_advanced(&mut self) {
        self.request = normalize_request_config(self.request.clone(), self.identity.protocol);
        self.quota_spec = normalize_quota_spec(self.quota_spec.clone());
        self.model_config = normalize_model_config(self.model_config.clone());
    }

    /// 高级配置在保存前统一校验：这些值直接决定请求能不能发出去，静默丢弃
    /// 只会让问题延后到同步失败时才暴露。
    pub(crate) fn validate_advanced(&self) -> Result<(), String> {
        validate_headers(&self.request.headers, "请求头")?;
        validate_endpoint_overrides(self.identity.protocol, &self.request.endpoints)?;
        validate_quota_spec(&self.quota_spec)?;
        validate_model_config(&self.model_config)?;
        Ok(())
    }
}

pub(crate) fn validate_headers(headers: &[ProviderHeader], label: &str) -> Result<(), String> {
    if headers.len() > limits::MAX_PROVIDER_HEADERS {
        return Err(format!(
            "{label}数量超过上限（{} 条）",
            limits::MAX_PROVIDER_HEADERS
        ));
    }
    for header in headers {
        let name = header.name.trim();
        if name.is_empty() {
            continue;
        }
        if name.chars().count() > limits::MAX_HEADER_NAME_CHARS {
            return Err(format!("{label}名称过长：{name}"));
        }
        reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("{label}「{name}」不是合法的 HTTP 名称"))?;
        if header.value.chars().count() > limits::MAX_HEADER_VALUE_CHARS {
            return Err(format!("{label}「{name}」的值过长"));
        }
        reqwest::header::HeaderValue::from_str(header.value.trim())
            .map_err(|_| format!("{label}「{name}」的值包含无法发送的字符"))?;
    }
    Ok(())
}

fn validate_url(value: &str, label: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        let url = reqwest::Url::parse(value).map_err(|_| format!("{label}地址无效"))?;
        if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(format!("{label}必须是完整地址，凭据请填写在认证栏或令牌栏"));
        }
        return Ok(());
    }
    if value.contains("://") || value.starts_with("//") {
        return Err(format!("{label}只支持 HTTP / HTTPS 地址或相对路径"));
    }
    Ok(())
}

fn validate_expression(value: &str, label: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if value.chars().count() > limits::MAX_QUOTA_EXPRESSION_CHARS {
        return Err(format!("{label}过长"));
    }
    crate::quota_expression::validate_expression(value).map_err(|error| format!("{label}：{error}"))
}

fn validate_quota_spec(spec: &ProviderQuotaSpec) -> Result<(), String> {
    if !spec.is_configured()
        && spec.balance.trim().is_empty()
        && spec.total.trim().is_empty()
        && spec.windows.is_empty()
    {
        return Ok(());
    }
    if !spec.is_configured() {
        return Err("填写余额表达式前请先填写余额接口地址".to_string());
    }
    validate_url(&spec.url, "余额接口")?;
    validate_headers(&spec.headers, "余额接口请求头")?;
    if spec.token.chars().count() > 2048 {
        return Err("余额接口令牌过长".to_string());
    }
    if !(spec.per_unit.is_finite() && spec.per_unit >= 0.0) {
        return Err("余额换算单位必须是大于等于 0 的数字".to_string());
    }
    if spec.windows.len() > limits::MAX_QUOTA_WINDOWS {
        return Err(format!(
            "余额窗口数量超过上限（{} 个）",
            limits::MAX_QUOTA_WINDOWS
        ));
    }
    validate_expression(&spec.balance, "余额表达式")?;
    validate_expression(&spec.total, "总额表达式")?;
    for (index, window) in spec.windows.iter().enumerate() {
        let label = if window.label.trim().is_empty() {
            format!("窗口 {}", index + 1)
        } else {
            window.label.trim().to_string()
        };
        validate_expression(&window.used, &format!("{label} 已用表达式"))?;
        validate_expression(&window.limit, &format!("{label} 上限表达式"))?;
        validate_expression(&window.resets_at, &format!("{label} 重置时间表达式"))?;
        if window.used.trim().is_empty() && window.limit.trim().is_empty() {
            return Err(format!("{label} 至少需要填写已用或上限表达式"));
        }
    }
    Ok(())
}

fn validate_pattern(pattern: &str, label: &str) -> Result<(), String> {
    let pattern = pattern.trim();
    if pattern.chars().count() > limits::MAX_MODEL_PATTERN_CHARS {
        return Err(format!("{label}模式过长：{pattern}"));
    }
    if let Some(regex) = pattern.strip_prefix("re:") {
        regex::RegexBuilder::new(regex)
            .case_insensitive(true)
            .build()
            .map_err(|_| format!("{label}正则无效：{pattern}"))?;
    }
    Ok(())
}

fn validate_model_config(config: &ProviderModelConfig) -> Result<(), String> {
    validate_url(&config.models_url, "模型列表接口")?;
    for pattern in config.include.iter() {
        validate_pattern(pattern, "展示模型")?;
    }
    for pattern in config.exclude.iter() {
        validate_pattern(pattern, "隐藏模型")?;
    }
    if config.catalog.chars().count() > 128 {
        return Err("models.dev 供应商名称过长".to_string());
    }
    if config.overrides.len() > limits::MAX_MODELS_PER_PROVIDER {
        return Err(format!(
            "模型覆盖数量超过上限（{} 个）",
            limits::MAX_MODELS_PER_PROVIDER
        ));
    }
    for (model, over) in &config.overrides {
        if model.trim().is_empty() {
            return Err("模型覆盖的模型 ID 不能为空".to_string());
        }
        if model.chars().count() > limits::MAX_MODEL_OVERRIDE_CHARS {
            return Err(format!("模型 ID 过长：{model}"));
        }
        for value in [&over.display_name, &over.wire_name, &over.alias] {
            if value.chars().count() > limits::MAX_MODEL_OVERRIDE_CHARS {
                return Err(format!("模型「{model}」的覆盖文本过长"));
            }
        }
        for (label, value) in [
            ("上下文窗口", over.context_window),
            ("输出上限", over.output_limit),
        ] {
            if value == Some(0) {
                return Err(format!("模型「{model}」的{label}必须大于 0"));
            }
        }
        for (label, value) in [
            ("输入价格", over.price.input),
            ("输出价格", over.price.output),
            ("缓存读取价格", over.price.cache_read),
            ("缓存写入价格", over.price.cache_write),
        ] {
            if value.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("模型「{model}」的{label}必须是非负数字"));
            }
        }
        if over.reasoning_levels.len() > limits::MAX_REASONING_LEVELS {
            return Err(format!("模型「{model}」的推理档位过多"));
        }
    }
    Ok(())
}

/// 大小写不敏感的 glob 匹配：`*` 任意长度、`?` 单个字符；以 `re:` 开头时按
/// 正则处理（忽略大小写）。非法正则按不匹配处理，不会让一次保存失败。
pub(crate) fn model_matches(pattern: &str, model: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if let Some(regex) = pattern.strip_prefix("re:") {
        return regex::RegexBuilder::new(regex)
            .case_insensitive(true)
            .build()
            .map(|regex| regex.is_match(model))
            .unwrap_or(false);
    }
    glob_matches(&pattern.to_lowercase(), &model.to_lowercase())
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ProviderModelConfig {
        ProviderModelConfig {
            include: vec!["gpt-*".into(), "re:^claude-".into()],
            exclude: vec!["*-mini".into()],
            overrides: BTreeMap::from([(
                "gpt-5.5".to_string(),
                ProviderModelOverride {
                    display_name: "GPT 5.5".into(),
                    wire_name: "gpt-5.5-2026".into(),
                    alias: "gpt-5.5".into(),
                    hidden: false,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn glob_and_regex_patterns_decide_visibility_together() {
        let config = config();
        assert!(config.allows("gpt-5.5"));
        assert!(config.allows("CLAUDE-opus"));
        assert!(!config.allows("gpt-5.5-mini"));
        assert!(!config.allows("deepseek-v4"));
        assert!(model_matches("GPT-*", "gpt-5"));
        assert!(model_matches("re:^claude-", "Claude-Opus"));
        assert!(!model_matches("re:[", "anything"));
    }

    #[test]
    fn an_empty_include_list_shows_everything_but_still_honours_excludes_and_hidden() {
        let mut config = ProviderModelConfig {
            exclude: vec!["*embedding*".into()],
            ..Default::default()
        };
        config.overrides.insert(
            "gpt-4o".into(),
            ProviderModelOverride {
                hidden: true,
                ..Default::default()
            },
        );
        assert!(config.allows("gpt-5"));
        assert!(!config.allows("text-embedding-3"));
        assert!(!config.allows("gpt-4o"));
    }

    #[test]
    fn wire_names_and_usage_aliases_fall_back_to_the_model_id() {
        let config = config();
        assert_eq!(config.wire_name("gpt-5.5"), "gpt-5.5-2026");
        assert_eq!(config.wire_name("o5"), "o5");
        assert_eq!(config.usage_alias("gpt-5.5"), "gpt-5.5");
        assert_eq!(config.usage_alias("o5"), "o5");
    }

    #[test]
    fn reasoning_declarations_are_optional() {
        let mut config = ProviderModelConfig::default();
        assert!(config.reasoning_levels("gpt-5").is_empty());
        assert_eq!(config.sends_reasoning("gpt-5"), None);
        config.overrides.insert(
            "gpt-5".into(),
            ProviderModelOverride {
                reasoning_levels: vec!["low".into(), "high".into()],
                sends_reasoning: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(config.reasoning_levels("gpt-5"), ["low", "high"]);
        assert_eq!(config.sends_reasoning("gpt-5"), Some(false));
    }
}
