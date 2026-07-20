use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
    AccessToken,
    Session,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemporaryCliTerminalKind {
    #[default]
    Auto,
    SystemDefault,
    Terminal,
    #[serde(rename = "iTerm2")]
    ITerm2,
    Warp,
    #[serde(rename = "wezTerm")]
    WezTerm,
    Ghostty,
    Kitty,
    Alacritty,
    Kaku,
    #[serde(rename = "windowsTerminal")]
    WindowsTerminal,
    CommandPrompt,
    #[serde(rename = "powerShell")]
    PowerShell,
    Custom,
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
