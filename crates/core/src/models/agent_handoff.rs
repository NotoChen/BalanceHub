use serde::{Deserialize, Serialize};

use super::AgentCliKind;

/// 从一个原生会话生成跨 Agent 交接文件。读取只使用已有会话引用，
/// 不改写原始会话记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentHandoffRequest {
    pub consumer_id: String,
    pub request_id: u64,
    pub scope_revision: String,
    pub session_ref: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHandoff {
    /// 交接文件的绝对路径。文件位于应用数据目录，不写入项目或原生会话目录。
    pub path: String,
    /// 完整 Markdown 正文，可直接粘贴到目标 Agent 的新会话。
    pub content: String,
    pub session_title: String,
    pub session_id: String,
    pub workdir: String,
    pub cli_kind: AgentCliKind,
    pub model: Option<String>,
    /// 实际写入交接文件的消息数。
    pub messages: u64,
    /// 会话只读取到部分片段，或单条消息被截断。
    pub truncated: bool,
    pub generated_at: String,
}
