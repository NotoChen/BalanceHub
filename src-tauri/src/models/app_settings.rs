use serde::{Deserialize, Serialize};

use super::enums::{
    LivenessCliKind, LivenessIntervalMode, LivenessPromptMode, ProxyMode, TemporaryCliTerminalKind,
    ThemeMode,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub onboarding_completed: bool,
    pub refresh_interval: u64,
    pub launch_at_login: bool,
    /// 仅对携带 `--silent-start` 参数的自启动生效；手动启动始终弹出主窗口。
    #[serde(default)]
    pub launch_at_login_minimized: bool,
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_true")]
    pub auto_refresh_enabled: bool,
    #[serde(default = "default_true")]
    pub auto_check_in_enabled: bool,
    #[serde(default = "default_check_in_time")]
    pub check_in_time: String,
    #[serde(default = "default_true")]
    pub notification_enabled: bool,
    #[serde(default = "default_notification_channels")]
    pub notification_channels: Vec<NotificationChannel>,
    #[serde(default = "default_glass_transparency")]
    pub glass_transparency: u8,
    #[serde(default)]
    pub liveness_cli_kind: LivenessCliKind,
    #[serde(default)]
    pub codex_cli_path: String,
    #[serde(default)]
    pub claude_cli_path: String,
    #[serde(default)]
    pub temporary_cli_terminal_kind: TemporaryCliTerminalKind,
    #[serde(default)]
    pub temporary_cli_terminal_command: String,
    #[serde(default)]
    pub liveness_enabled: bool,
    #[serde(default = "default_codex_model")]
    pub liveness_model: String,
    #[serde(default)]
    pub liveness_interval_mode: LivenessIntervalMode,
    #[serde(default = "default_liveness_interval")]
    pub liveness_interval: u64,
    #[serde(default = "default_liveness_random_min_interval")]
    pub liveness_random_min_interval: u64,
    #[serde(default = "default_liveness_interval")]
    pub liveness_random_max_interval: u64,
    #[serde(default = "default_liveness_timeout")]
    pub liveness_timeout: u64,
    #[serde(default)]
    pub liveness_prompt_mode: LivenessPromptMode,
    #[serde(default = "default_liveness_prompt")]
    pub liveness_fixed_prompt: String,
    #[serde(default = "default_liveness_prompt_library")]
    pub liveness_prompt_library: Vec<String>,
    #[serde(default = "default_liveness_placeholder_pools")]
    pub liveness_placeholder_pools: Vec<LivenessPlaceholderPool>,
    #[serde(default = "default_liveness_number_min")]
    pub liveness_number_min: u64,
    #[serde(default = "default_liveness_number_max")]
    pub liveness_number_max: u64,
    /// 自动测活会消耗真实额度。全 App 一次性授权：首次开启任意自动测活（全局或单站）时
    /// 弹窗确认一次并记录于此；之后不再逐站/逐次询问。为空表示尚未授权。
    #[serde(default)]
    pub liveness_consent_accepted_at: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            onboarding_completed: false,
            refresh_interval: 300,
            launch_at_login: false,
            launch_at_login_minimized: false,
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            theme_mode: ThemeMode::System,
            auto_refresh_enabled: true,
            auto_check_in_enabled: true,
            check_in_time: default_check_in_time(),
            notification_enabled: true,
            notification_channels: default_notification_channels(),
            glass_transparency: default_glass_transparency(),
            liveness_cli_kind: LivenessCliKind::Codex,
            codex_cli_path: String::new(),
            claude_cli_path: String::new(),
            temporary_cli_terminal_kind: TemporaryCliTerminalKind::Auto,
            temporary_cli_terminal_command: String::new(),
            liveness_enabled: false,
            liveness_model: default_codex_model(),
            liveness_interval_mode: LivenessIntervalMode::Fixed,
            liveness_interval: default_liveness_interval(),
            liveness_random_min_interval: default_liveness_random_min_interval(),
            liveness_random_max_interval: default_liveness_interval(),
            liveness_timeout: default_liveness_timeout(),
            liveness_prompt_mode: LivenessPromptMode::Random,
            liveness_fixed_prompt: default_liveness_prompt(),
            liveness_prompt_library: default_liveness_prompt_library(),
            liveness_placeholder_pools: default_liveness_placeholder_pools(),
            liveness_number_min: default_liveness_number_min(),
            liveness_number_max: default_liveness_number_max(),
            liveness_consent_accepted_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: NotificationChannelKind,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum NotificationChannelKind {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "dingtalk")]
    DingTalk,
    #[serde(rename = "wecom")]
    WeCom,
    #[serde(rename = "feishu")]
    Feishu,
    #[serde(rename = "slack")]
    Slack,
    #[serde(rename = "generic")]
    Generic,
}

fn default_notification_channels() -> Vec<NotificationChannel> {
    vec![NotificationChannel {
        id: "system".to_string(),
        name: "系统通知".to_string(),
        kind: NotificationChannelKind::System,
        url: String::new(),
        secret: String::new(),
        enabled: true,
    }]
}

pub(crate) fn default_glass_transparency() -> u8 {
    58
}

pub(crate) fn default_codex_model() -> String {
    "gpt-5.5".to_string()
}

pub(crate) fn default_liveness_interval() -> u64 {
    5 * 60
}

pub(crate) fn default_liveness_random_min_interval() -> u64 {
    default_liveness_interval()
}

pub(crate) fn default_liveness_timeout() -> u64 {
    75
}

fn default_liveness_prompt() -> String {
    "Explain: ls -la".to_string()
}

fn default_liveness_prompt_library() -> Vec<String> {
    vec![
        "Explain: {cmd}".to_string(),
        "Convert to camelCase: {snake}".to_string(),
        "Is this valid JSON: {json}".to_string(),
        "Normalize path: {path}".to_string(),
        "Fix typo: {typo}".to_string(),
        "Rename variable: {var}".to_string(),
        "Sum: {a}+{b}".to_string(),
        "Choose smaller: {a} or {b}".to_string(),
        "Make this concise: {sentence}".to_string(),
        "Classify log level: {log}".to_string(),
        "Title case: {phrase}".to_string(),
        "Make slug: {phrase}".to_string(),
        "Is this path absolute: {path}".to_string(),
    ]
}

fn default_liveness_number_min() -> u64 {
    2
}

fn default_liveness_number_max() -> u64 {
    97
}

pub(crate) fn default_liveness_placeholder_pools() -> Vec<LivenessPlaceholderPool> {
    vec![
        placeholder_pool(
            "word",
            &[
                "folder", "record", "index", "config", "window", "button", "result", "summary",
            ],
        ),
        placeholder_pool(
            "cmd",
            &[
                "ls -la",
                "git status",
                "npm test",
                "node -v",
                "pwd",
                "cat README.md",
                "cargo check",
                "date",
            ],
        ),
        placeholder_pool(
            "phrase",
            &[
                "daily notes",
                "release plan",
                "window title",
                "build result",
                "local draft",
                "error summary",
            ],
        ),
        placeholder_pool(
            "snake",
            &[
                "file_name",
                "total_count",
                "last_seen",
                "item_index",
                "window_title",
                "retry_delay",
                "created_at",
            ],
        ),
        placeholder_pool(
            "var",
            &[
                "tmpValue",
                "rawText",
                "nextItem",
                "userName",
                "filePath",
                "totalCount",
                "retryCount",
            ],
        ),
        placeholder_pool(
            "path",
            &[
                "/tmp/../var/log",
                "./src/../README.md",
                "./notes//today.md",
                "~/Downloads/../.config",
                "/usr/local/../bin",
            ],
        ),
        placeholder_pool(
            "json",
            &[
                "{\"ok\":true}",
                "{\"name\":\"demo\"}",
                "{\"items\":[\"a\",\"b\"]}",
                "{\"enabled\":false}",
                "{\"limit\":3}",
            ],
        ),
        placeholder_pool("status", &["200", "201", "204", "301", "400", "404", "418"]),
        placeholder_pool(
            "typo",
            &[
                "recieve",
                "adress",
                "teh",
                "occured",
                "seperate",
                "enviroment",
            ],
        ),
        placeholder_pool(
            "sentence",
            &[
                "The file was saved after the last edit.",
                "This setting controls how often data refreshes.",
                "The window title should stay short.",
                "The table row needs a clearer label.",
                "The command finished without output.",
            ],
        ),
        placeholder_pool(
            "log",
            &[
                "WARN retry after timeout",
                "ERROR file not found",
                "INFO build completed",
                "DEBUG cache hit",
                "WARN missing config",
            ],
        ),
        placeholder_pool("port", &["22", "80", "443", "3000", "5432", "6379", "8080"]),
    ]
}

fn placeholder_pool(key: &str, values: &[&str]) -> LivenessPlaceholderPool {
    LivenessPlaceholderPool {
        key: key.to_string(),
        values: values.iter().map(|value| value.to_string()).collect(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessPlaceholderPool {
    pub key: String,
    pub values: Vec<String>,
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_check_in_time() -> String {
    "00:00".to_string()
}
