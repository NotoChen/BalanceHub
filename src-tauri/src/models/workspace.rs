use serde::{Deserialize, Serialize};

use super::LivenessCliKind;

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
