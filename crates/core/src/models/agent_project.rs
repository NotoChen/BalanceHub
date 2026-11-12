use serde::{Deserialize, Serialize};

use super::{AgentCliKind, SessionUsageSummary};

fn default_project_period() -> String {
    "7d".to_string()
}

fn default_include_usage() -> bool {
    true
}

/// 项目视图：按原生会话的工作目录聚合跨 Agent 会话与周期用量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentProjectReportRequest {
    pub consumer_id: String,
    pub request_id: u64,
    /// 用量统计周期，与站点用量趋势和工作台用量对账共用同一组周期名。
    #[serde(default = "default_project_period")]
    pub period: String,
    /// 是否解码周期内的会话正文用于估算消耗；关闭时只返回元数据。
    #[serde(default = "default_include_usage")]
    pub include_usage: bool,
}

impl Default for AgentProjectReportRequest {
    fn default() -> Self {
        Self {
            consumer_id: String::new(),
            request_id: 0,
            period: default_project_period(),
            include_usage: true,
        }
    }
}

/// 项目视图的后台读取阶段。扫描按原生来源计数，解码按候选会话计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentProjectProgressPhase {
    Scanning,
    Decoding,
    Finalizing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectProgress {
    pub phase: AgentProjectProgressPhase,
    pub completed: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectAgent {
    pub cli_kind: AgentCliKind,
    pub sessions: u64,
    pub last_activity: Option<String>,
}

/// 该项目最近一次通过 BalanceHub 启动时使用的账号。归属只来自启动记录，
/// 不从目录、模型或其他 Agent 的配置推断。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectAccount {
    pub provider_id: String,
    pub provider_name: String,
    pub account_label: String,
    pub cli_kind: AgentCliKind,
    pub sessions: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProject {
    /// 与历史会话筛选共用的工作空间 ID；同一路径可能由多个来源登记。
    pub workspace_ids: Vec<String>,
    pub path: String,
    /// 该项目下扫描到的全部原生会话数，不受周期限制。
    pub sessions: u64,
    pub undated_sessions: u64,
    pub last_activity: Option<String>,
    pub agents: Vec<AgentProjectAgent>,
    pub accounts: Vec<AgentProjectAccount>,
    /// 本周期内成功解码并计入消耗的会话数。
    pub usage_sessions: u64,
    pub usage: SessionUsageSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProjectReport {
    pub period: String,
    pub generated_at: String,
    pub window_start: String,
    pub window_end: String,
    /// 本次扫描到的全部原生会话数（含缺少时间的会话）。
    pub sessions: u64,
    pub undated_sessions: u64,
    /// 本周期内成功解码的会话数；未启用消耗统计时为 0。
    pub decoded_sessions: u64,
    pub skipped_sessions: u64,
    pub partial: bool,
    pub truncated: bool,
    pub projects: Vec<AgentProject>,
    pub diagnostics: Vec<String>,
    pub account_retention_days: u32,
    pub pricing_error: Option<String>,
}
