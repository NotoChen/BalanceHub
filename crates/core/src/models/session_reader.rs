use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionMessageKind {
    #[default]
    Text,
    Thinking,
    ToolCall,
    ToolResult,
    Image,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionToolStatus {
    Succeeded,
    Failed,
    Running,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionToolKind {
    Shell,
    Read,
    Search,
    Edit,
    Write,
    Mcp,
    Agent,
    Other,
}

/// Small native facts stored separately from chunked, searchable content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionMessageMetadata {
    pub kind: SessionMessageKind,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
    pub tool_kind: Option<SessionToolKind>,
    pub turn_id: Option<String>,
    pub group_id: Option<String>,
    pub call_id: Option<String>,
    pub status: Option<SessionToolStatus>,
    pub title: Option<String>,
    pub exit_code: Option<i32>,
    pub injected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub message_id: String,
    pub offset: usize,
    pub snippet: String,
    pub terms: Vec<String>,
    pub kind: SessionMessageKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReadPage {
    pub revision: String,
    pub previous_cursor: Option<String>,
    pub next_cursor: Option<String>,
    pub total_messages: usize,
    pub start: usize,
    pub indexing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFindState {
    pub query: String,
    pub total: usize,
    pub position: Option<usize>,
    pub active_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurnSummary {
    pub id: String,
    pub message_id: String,
    pub title: String,
    pub timestamp: Option<String>,
    pub steps: usize,
    pub failures: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileChange {
    pub path: String,
    pub before: String,
    pub after: String,
    pub patch: Option<String>,
    pub before_known: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessageRequest {
    pub consumer_id: String,
    pub request_id: u64,
    pub scope_revision: String,
    pub session_ref: String,
    pub revision: String,
    pub message_id: String,
    pub content_version: u64,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionMessageContent {
    pub message_id: String,
    pub content: String,
    pub offset: usize,
    pub total_chars: usize,
    pub next_offset: Option<usize>,
    pub changes: Vec<SessionFileChange>,
}
