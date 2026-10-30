use serde::{Deserialize, Serialize};

macro_rules! define_provider_protocols {
    (
        $default_variant:ident => { key: $default_key:literal, module: $default_module:ident }
        $(, $variant:ident => { key: $key:literal, module: $module:ident })*
        $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[allow(clippy::enum_variant_names)]
        pub enum ProviderProtocol {
            #[default]
            #[serde(rename = $default_key)]
            $default_variant,
            $(
                #[serde(rename = $key)]
                $variant,
            )*
        }

        impl ProviderProtocol {
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

// 后端持久化身份与运行时协议注册表共用一份目录。
crate::provider_protocol_catalog::for_each_provider_protocol!(define_provider_protocols);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
    AccessToken,
    Session,
    Password,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCheckInMethod {
    #[default]
    Auto,
    Standard,
    SessionSignIn,
    FreshLogin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderTurnstileMode {
    #[default]
    Auto,
    Always,
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

/// 主窗口关闭按钮的行为。默认隐藏到托盘：后台刷新、签到和测活依赖进程存活，
/// 直接退出会让这些自动化静默停摆。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowCloseAction {
    #[default]
    HideToTray,
    Quit,
}

/// 单击托盘图标的行为。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrayClickAction {
    #[default]
    ShowWindow,
    ToggleWindow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

impl LogLevel {
    pub fn as_filter(self) -> log::LevelFilter {
        match self {
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
        }
    }
}

/// 通知事件类别。既用于「逐类开关」，也作为合并窗口的去重键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationEvent {
    RefreshFailure,
    LivenessFailure,
    CheckIn,
    /// 用户手动触发（含设置里的「发送测试」）：不参与开关与合并。
    Manual,
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

macro_rules! define_terminal_kinds {
    (
        $(
            $variant:ident => { key: $key:literal, label: $label:literal }
        ),+
        $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum TemporaryCliTerminalKind {
            $(
                #[serde(rename = $key)]
                $variant,
            )+
        }

        impl TemporaryCliTerminalKind {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }
    };
}

crate::terminal_catalog::for_each_terminal!(define_terminal_kinds);

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
