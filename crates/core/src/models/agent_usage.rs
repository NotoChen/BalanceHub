use serde::{Deserialize, Serialize};

use super::{AgentCliKind, SessionUsageSummary};

/// 本地会话用量对账的统计周期，与站点用量趋势共用同一组周期名。
pub const AGENT_USAGE_PERIODS: &[&str] = &["24h", "7d", "30d"];

pub fn default_agent_usage_period() -> String {
    AGENT_USAGE_PERIODS[0].to_string()
}

fn default_include_site_usage() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentUsageReportRequest {
    pub consumer_id: String,
    pub request_id: u64,
    /// 今日 / 7 天 / 30 天。
    #[serde(default = "default_agent_usage_period")]
    pub period: String,
    /// 是否读取站点账单用于对比；站点失败只影响对比区，不影响本机统计。
    #[serde(default = "default_include_site_usage")]
    pub include_site_usage: bool,
}

impl Default for AgentUsageReportRequest {
    fn default() -> Self {
        Self {
            consumer_id: String::new(),
            request_id: 0,
            period: default_agent_usage_period(),
            include_site_usage: true,
        }
    }
}

/// 本机会话日志的用量估算。所有金额都是参考估算，实际费用以中转站账单为准。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageReport {
    pub period: String,
    pub generated_at: String,
    /// 统计窗口按本地时区；今日是本地零点到现在，7/30 天为往前滚动的天数。
    pub window_start: String,
    pub window_end: String,
    /// 窗口内扫描到的会话总数（按会话最近更新时间归属）。
    pub scanned_sessions: u64,
    /// 成功解码并计入汇总的会话数。
    pub counted_sessions: u64,
    /// 因读取预算、解码失败或单次会话上限未计入的会话数。
    pub skipped_sessions: u64,
    /// 缺少时间信息、无法判断是否落在窗口内的会话数。
    pub undated_sessions: u64,
    /// 有会话只读取到部分正文/用量，或原生来源扫描本身不完整。
    pub partial: bool,
    /// 命中单次统计的会话上限。
    pub truncated: bool,
    pub totals: SessionUsageSummary,
    pub accounts: Vec<AgentUsageAccount>,
    pub projects: Vec<AgentUsageProject>,
    /// `totals.models` 已包含模型维度，这里不再重复。
    pub sites: Vec<AgentUsageSite>,
    pub diagnostics: Vec<String>,
    /// 账号归属来自 BalanceHub 的启动记录；记录按保留策略清理。
    pub account_retention_days: u32,
    /// 模型价格目录读取失败时的说明；费用估算仍会按已有缓存给出。
    pub pricing_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageAccount {
    pub provider_id: Option<String>,
    pub provider_name: String,
    /// 启动时记录的非敏感账号展示快照；空表示未记录。
    pub account_label: String,
    pub cli_kind: AgentCliKind,
    pub sessions: u64,
    pub usage: SessionUsageSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageProject {
    pub path: String,
    pub sessions: u64,
    pub usage: SessionUsageSummary,
}

/// 一个中转站的站点账单，与本地归属到该站的 token 放在一起对比。
/// 站点统计包含该账号的全部流量（其他机器、其他客户端），本地只覆盖
/// 本机原生会话日志，因此两者不要求相等。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSite {
    pub provider_id: String,
    pub provider_name: String,
    /// 本地统计中归属该中转站的 token 总量（输入含缓存 + 输出）。
    pub local_tokens: u64,
    pub error: Option<String>,
    pub usage: Option<AgentUsageSiteSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSiteSummary {
    /// 站点统计窗口内的请求数。
    pub request_count: i64,
    /// 站点账单的 token 合计，来自站点返回的模型统计。
    pub token_used: u64,
}
