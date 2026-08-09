use serde::Serialize;

use super::LivenessCliKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CliSessionMetadataSource {
    CodexStateDb,
    ClaudeTranscript,
}

/// 只读历史会话索引项。不会把完整对话、工具输出或凭据带入 IPC。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionSummary {
    pub id: String,
    pub title: String,
    pub preview: Option<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub cli_kind: LivenessCliKind,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub workdir: String,
    pub cli_version: Option<String>,
    pub archived: bool,
    pub can_resume: bool,
    pub metadata_source: CliSessionMetadataSource,
}
