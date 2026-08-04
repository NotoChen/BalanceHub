use serde::{Deserialize, Serialize};

use super::LivenessCliKind;

/// CLI 本地历史会话的展示元数据。
///
/// 只暴露启动恢复所需的信息，不把对话正文、工具输出或凭据带进 IPC。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSessionSummary {
    pub id: String,
    pub title: String,
    pub model: Option<String>,
    pub cli_kind: LivenessCliKind,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}
