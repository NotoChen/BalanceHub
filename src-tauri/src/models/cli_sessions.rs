use serde::Serialize;

use super::AgentCliKind;

/// 只读历史会话索引项。不会把完整对话、工具输出或凭据带入 IPC。
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CliSessionMessageRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionMessage {
    pub id: String,
    pub role: CliSessionMessageRole,
    pub content: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionDetail {
    pub session: CliSessionSummary,
    pub messages: Vec<CliSessionMessage>,
    pub truncated: bool,
    pub omitted_message_count: usize,
    /// Agent 自己维护的正文来源标识，仅用于诊断显示。
    pub content_source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CliSessionIndexState {
    Ready,
    Disabled,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionIndexAgentStats {
    pub cli_kind: AgentCliKind,
    pub size_bytes: u64,
    pub session_count: usize,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionIndexStatus {
    pub enabled: bool,
    pub directory: String,
    #[serde(rename = "maxSizeMiB")]
    pub max_size_mib: u64,
    pub size_bytes: u64,
    pub building: bool,
    pub agents: Vec<CliSessionIndexAgentStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_status_keeps_the_frontend_mib_field_name() {
        let status = CliSessionIndexStatus {
            enabled: true,
            directory: "/tmp/index".to_string(),
            max_size_mib: 64,
            size_bytes: 1_024,
            building: false,
            agents: Vec::new(),
        };
        let value = serde_json::to_value(status).expect("index status should serialize");
        assert_eq!(value.get("maxSizeMiB"), Some(&serde_json::json!(64)));
        assert!(value.get("maxSizeMib").is_none());
    }
}
