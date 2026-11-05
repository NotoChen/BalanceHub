use serde::{Deserialize, Serialize};

use super::AgentCliKind;

/// 只读历史会话条目。不会把完整对话、工具输出或凭据带入 IPC。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionSummary {
    pub id: String,
    pub title: String,
    pub preview: Option<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub cli_kind: AgentCliKind,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub workdir: String,
    pub cli_version: Option<String>,
    pub archived: bool,
    pub can_resume: bool,
    /// Agent 自己声明的诊断来源标识。它不是跨 Agent 的业务枚举，新增 Agent
    /// 不需要修改公共模型或前端联合类型。
    pub metadata_source: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CliSessionMessageRole {
    #[default]
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionMessage {
    pub id: String,
    pub role: CliSessionMessageRole,
    pub content: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
    pub metadata: super::SessionMessageMetadata,
    pub content_version: u64,
    pub content_length: usize,
    pub content_offset: usize,
    pub has_more_content: bool,
    pub changes: Vec<super::SessionFileChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionDetail {
    pub session: CliSessionSummary,
    pub usage: super::SessionUsageSummary,
    pub messages: Vec<CliSessionMessage>,
    /// 当前阅读页的位置与可继续读取的游标。
    pub page: super::SessionReadPage,
    pub outline: Vec<super::SessionTurnSummary>,
    pub find: super::SessionFindState,
}
