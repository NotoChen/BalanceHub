use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
    AccessToken,
    Session,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    #[default]
    NewApi,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderQuotaScope {
    #[default]
    Account,
    Token,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    #[default]
    System,
    NoProxy,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LivenessCliKind {
    #[default]
    Codex,
    ClaudeCode,
}

/// 测活方式：本地 CLI 调用，或直接 HTTP 调用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LivenessMethod {
    #[default]
    Cli,
    Http,
}

/// HTTP 测活使用的协议：OpenAI Chat Completions / OpenAI Responses / Anthropic Messages。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LivenessHttpProtocol {
    #[default]
    OpenaiChat,
    OpenaiResponses,
    Anthropic,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LivenessIntervalMode {
    #[default]
    Fixed,
    Random,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LivenessPromptMode {
    Fixed,
    #[default]
    Random,
    RoundRobin,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderProxyMode {
    #[default]
    Inherit,
    System,
    NoProxy,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderNotificationMode {
    #[default]
    Inherit,
    Custom,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderStatus {
    Ok,
    Warning,
    Error,
    Syncing,
}
