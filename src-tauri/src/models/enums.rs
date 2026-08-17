use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::enum_variant_names)]
pub enum ProviderProtocol {
    #[default]
    NewApi,
    Sub2Api,
    /// OpenAI-compatible gateway with no known account-management protocol.
    Api,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
    AccessToken,
    Session,
    Password,
}

/// 主凭据的来源/获取方式，与「用什么凭据」(AuthMode) 正交。账号密码、OAuth 都是
/// 产出主凭据的来源，而非与 Cookie/Token 并列的认证方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthSource {
    /// 手动粘贴现成凭据（Cookie / 访问令牌 / API Key）。
    #[default]
    Manual,
    /// 账号密码登录 → 产出主凭据（NewAPI: 会话 Cookie；Sub2API: Access + Refresh Token）。
    Password,
    /// OAuth 授权 → 同样产出主凭据（预留，UI 暂未开放）。
    Oauth,
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

macro_rules! define_agent_cli_kinds {
    (
        $default_variant:ident => { key: $default_key:literal, module: $default_module:ident }
        $(, $variant:ident => { key: $key:literal, module: $module:ident })*
        $(,)?
    ) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        pub enum AgentCliKind {
            #[default]
            #[serde(rename = $default_key)]
            $default_variant,
            $(
                #[serde(rename = $key)]
                $variant,
            )*
        }

        impl AgentCliKind {
            pub const ALL: &'static [Self] = &[
                Self::$default_variant,
                $(Self::$variant,)*
            ];

            pub const fn key(self) -> &'static str {
                match self {
                    Self::$default_variant => $default_key,
                    $(Self::$variant => $key,)*
                }
            }
        }
    };
}

// 后端身份和服务模块注册共用一份目录；新增内置 Agent 只改 agent_cli_catalog.rs。
crate::agent_cli_catalog::for_each_agent_cli!(define_agent_cli_kinds);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemporaryCliTerminalKind {
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
}

// 默认终端随目标平台变化，不能使用单一的 `#[default]` 枚举项。
#[allow(clippy::derivable_impls)]
impl Default for TemporaryCliTerminalKind {
    fn default() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::CommandPrompt
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::Terminal
        }
    }
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
