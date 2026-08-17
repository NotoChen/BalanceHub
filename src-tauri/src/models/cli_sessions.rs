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
