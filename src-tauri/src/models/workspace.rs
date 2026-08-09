use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::{LivenessCliKind, TemporaryCliTerminalKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemporaryCliSessionMode {
    #[default]
    New,
    History,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub path: String,
    #[serde(default)]
    pub use_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliPreference {
    pub provider_id: String,
    #[serde(default)]
    pub cli_kind: LivenessCliKind,
    #[serde(default)]
    pub api_key_token_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub workspace_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliLaunchInput {
    pub provider_id: String,
    pub cli_kind: LivenessCliKind,
    pub workdir: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_key_token_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub session_mode: TemporaryCliSessionMode,
    #[serde(default)]
    pub session_name: String,
    /// 历史模式下选中的会话 ID。
    #[serde(default)]
    pub resume_id: String,
    pub terminal_kind: TemporaryCliTerminalKind,
}

/// 启动前展示给用户确认的实际 CLI 参数。该结构只在当前确认弹窗内存中流转，
/// 不写入应用配置、运行记录或日志。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCliLaunchPreview {
    pub provider_name: String,
    pub cli_kind: LivenessCliKind,
    pub cli_path: String,
    pub args: Vec<String>,
    pub command: String,
    pub terminal_kind: TemporaryCliTerminalKind,
    pub terminal_name: String,
    pub workdir: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub session_mode: TemporaryCliSessionMode,
    pub session_name: String,
    pub resume_id: String,
    pub environment: BTreeMap<String, String>,
    pub settings_path: Option<String>,
    pub settings_content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectoryEntry {
    pub name: String,
    pub path: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDirectoryListing {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub home_path: String,
    pub entries: Vec<WorkspaceDirectoryEntry>,
}
