//! 项目视图：按原生会话的工作目录聚合跨 Agent 的会话数、Agent 分布、
//! 启动记录中的账号和最近活动，并可选附加周期用量。
//!
//! 这里只扫描会话元数据；周期用量复用 `usage_report` 的两阶段读取，
//! 不维护第二套解码或计价实现。项目路径来自会话自身记录的工作目录，
//! 不要求事先登记工作空间。

use super::{
    emit_progress, hash, scope, state,
    usage_report::{
        attribution_index, build_usage_candidates, dedupe, history_sources, load_history_source,
        parse_timestamp_ms, usage_window_start, AttributionKey, CollectedUsageCandidates,
        SourceUsageOutcome,
    },
    AgentSessionActor, ProjectProgressReporter,
};
use crate::{
    models::{
        AgentCliKind, AgentProject, AgentProjectAccount, AgentProjectAgent,
        AgentProjectProgressPhase, AgentProjectReport, AgentUsageProject, AgentUsageReport,
        Provider,
    },
    services::agent_cli::contracts::{SessionHistorySource, SessionReadBudget},
};
use chrono::{DateTime, Local};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) struct CollectedProjectReport {
    period: String,
    window_start: DateTime<Local>,
    window_end: DateTime<Local>,
    sessions: u64,
    undated: u64,
    projects: Vec<ProjectAggregate>,
    partial: bool,
    diagnostics: Vec<String>,
    usage_candidates: Option<CollectedUsageCandidates>,
}

impl CollectedProjectReport {
    /// 取出同一次扫描产生的用量候选。项目视图随后单独解码正文，
    /// 失败时仍可只返回项目元数据。
    pub(crate) fn take_usage_candidates(&mut self) -> Option<CollectedUsageCandidates> {
        self.usage_candidates.take()
    }
}

struct ProjectAggregate {
    workspace_ids: BTreeSet<String>,
    path: String,
    sessions: u64,
    undated: u64,
    last_activity_ms: Option<i64>,
    agents: BTreeMap<AgentCliKind, AgentAggregate>,
    accounts: BTreeMap<(String, String, AgentCliKind), AccountAggregate>,
}

impl ProjectAggregate {
    fn new(workspace_id: String, path: String) -> Self {
        Self {
            workspace_ids: BTreeSet::from([workspace_id]),
            path,
            sessions: 0,
            undated: 0,
            last_activity_ms: None,
            agents: BTreeMap::new(),
            accounts: BTreeMap::new(),
        }
    }

    fn merge(&mut self, other: Self) {
        self.workspace_ids.extend(other.workspace_ids);
        self.sessions = self.sessions.saturating_add(other.sessions);
        self.undated = self.undated.saturating_add(other.undated);
        self.last_activity_ms = max_option(self.last_activity_ms, other.last_activity_ms);
        for (kind, agent) in other.agents {
            self.agents.entry(kind).or_default().merge(agent);
        }
        for (key, account) in other.accounts {
            let entry = self
                .accounts
                .entry(key)
                .or_insert_with(|| AccountAggregate {
                    provider_name: account.provider_name.clone(),
                    sessions: 0,
                });
            entry.sessions = entry.sessions.saturating_add(account.sessions);
        }
    }
}

#[derive(Default)]
struct AgentAggregate {
    sessions: u64,
    last_activity_ms: Option<i64>,
}

impl AgentAggregate {
    fn merge(&mut self, other: Self) {
        self.sessions = self.sessions.saturating_add(other.sessions);
        self.last_activity_ms = max_option(self.last_activity_ms, other.last_activity_ms);
    }
}

struct AccountAggregate {
    provider_name: String,
    sessions: u64,
}

#[derive(Default)]
struct ProjectSourceOutcome {
    projects: Vec<ProjectAggregate>,
    usage: Option<SourceUsageOutcome>,
    partial: bool,
    diagnostics: Vec<String>,
    error: Option<String>,
}

/// 扫描所有可用的原生来源，按项目工作目录聚合全部会话的元数据。
/// 启用用量统计时，同一次遍历还会收集周期内的用量候选；正文解码由
/// 调用方在第二阶段单独执行。
pub(crate) fn collect_project_report(
    actor: AgentSessionActor<'_>,
    period: &str,
    budget: &SessionReadBudget,
    include_usage: bool,
    reporter: Option<ProjectProgressReporter>,
) -> Result<CollectedProjectReport, String> {
    let window_end = Local::now();
    let window_start = usage_window_start(period, window_end)?;
    let window_start_ms = window_start.timestamp_millis();
    let (sources, mut diagnostics) = history_sources(actor)?;
    if sources.is_empty() {
        diagnostics.push("没有可用的原生会话来源".into());
        emit_progress(
            &reporter,
            AgentProjectProgressPhase::Scanning,
            0,
            0,
            "没有可用的原生会话来源",
        );
        return Ok(CollectedProjectReport {
            period: period.to_string(),
            window_start,
            window_end,
            sessions: 0,
            undated: 0,
            projects: Vec::new(),
            partial: false,
            diagnostics,
            usage_candidates: None,
        });
    }
    let (attribution, account_capped) = attribution_index();
    if account_capped {
        diagnostics.push("启动记录只保留最近一段时间的条数，较早会话的账号归属可能缺失".into());
    }
    let total_sources = sources.len() as u64;
    emit_progress(
        &reporter,
        AgentProjectProgressPhase::Scanning,
        0,
        total_sources,
        format!("扫描 {total_sources} 个原生来源"),
    );
    let scanned_sources = AtomicU64::new(0);
    let per_source_bytes = (budget.max_bytes / sources.len().max(1) as u64).max(1);
    let outcomes = std::thread::scope(|threads| {
        let handles = sources
            .iter()
            .map(|(source, config)| {
                let source = source.clone();
                let config = config.clone();
                let cache = state::source_resources(actor, &source).cache;
                let source_budget = SessionReadBudget::new(
                    budget.deadline,
                    budget.cancelled.clone(),
                    per_source_bytes,
                );
                let attribution = &attribution;
                let reporter = reporter.clone();
                let scanned_sources = &scanned_sources;
                threads.spawn(move || {
                    let outcome = collect_project_source(
                        &source,
                        &config,
                        &cache,
                        &source_budget,
                        attribution,
                        window_start_ms,
                        include_usage,
                    );
                    let done = scanned_sources.fetch_add(1, Ordering::Relaxed) + 1;
                    emit_progress(
                        &reporter,
                        AgentProjectProgressPhase::Scanning,
                        done,
                        total_sources,
                        format!("已扫描 {done}/{total_sources} 个来源"),
                    );
                    outcome
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().map_err(|_| "项目扫描线程异常".to_string()))
            .collect::<Result<Vec<_>, String>>()
    })?;

    let mut sessions = 0u64;
    let mut undated = 0u64;
    let mut partial = false;
    let mut usage_outcomes = Vec::with_capacity(outcomes.len());
    let mut by_path = BTreeMap::<String, ProjectAggregate>::new();
    for (index, outcome) in outcomes.into_iter().enumerate() {
        sessions = sessions.saturating_add(
            outcome
                .projects
                .iter()
                .map(|project| project.sessions)
                .sum::<u64>(),
        );
        undated = undated.saturating_add(
            outcome
                .projects
                .iter()
                .map(|project| project.undated)
                .sum::<u64>(),
        );
        partial |= outcome.partial;
        if let Some(error) = outcome.error {
            diagnostics.push(format!("{}：{error}", sources[index].0.label));
        }
        diagnostics.extend(outcome.diagnostics);
        if let Some(usage) = outcome.usage {
            usage_outcomes.push(usage);
        }
        for project in outcome.projects {
            match by_path.get_mut(&project.path) {
                Some(existing) => existing.merge(project),
                None => {
                    let path = project.path.clone();
                    by_path.insert(path, project);
                }
            }
        }
    }
    if budget.cancelled.load(Ordering::Acquire) {
        partial = true;
    }
    let mut projects = by_path.into_values().collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        right
            .last_activity_ms
            .cmp(&left.last_activity_ms)
            .then_with(|| left.path.cmp(&right.path))
    });
    let usage_candidates = include_usage.then(|| {
        // 扫描诊断已经进入项目报告，用量候选只补自己的截断说明，
        // 避免同一来源错误在两份报告里重复出现。
        build_usage_candidates(
            sources,
            period,
            window_start,
            window_end,
            usage_outcomes,
            Vec::new(),
        )
    });
    Ok(CollectedProjectReport {
        period: period.to_string(),
        window_start,
        window_end,
        sessions,
        undated,
        projects,
        partial,
        diagnostics: dedupe(diagnostics),
        usage_candidates,
    })
}

fn collect_project_source(
    source: &crate::models::AgentSessionSource,
    config: &SessionHistorySource,
    cache: &super::budget::HistoryReadCache,
    budget: &SessionReadBudget,
    attribution: &HashMap<AttributionKey, super::usage_report::Attribution>,
    window_start_ms: i64,
    include_usage: bool,
) -> ProjectSourceOutcome {
    let mut outcome = ProjectSourceOutcome::default();
    let loaded = match load_history_source(source, config, cache, budget) {
        Ok(loaded) => loaded,
        Err(error) => {
            outcome.error = Some(error);
            outcome.partial = true;
            outcome.usage = include_usage.then(SourceUsageOutcome::failed);
            return outcome;
        }
    };
    outcome.diagnostics.extend(loaded.diagnostics);
    let mut usage = include_usage.then(SourceUsageOutcome::default);
    let mut by_path = BTreeMap::<String, ProjectAggregate>::new();
    for workspace in loaded.workspaces {
        if !workspace.complete {
            outcome.partial = true;
        }
        outcome.diagnostics.extend(workspace.diagnostics);
        let workspace_id = scope::workspace_for_path(&workspace.workdir).id;
        for record in workspace.records {
            let project_path = if record.summary.workdir.trim().is_empty() {
                workspace.workdir.to_string_lossy().into_owned()
            } else {
                record.summary.workdir.clone()
            };
            let project = by_path
                .entry(project_path.clone())
                .or_insert_with(|| ProjectAggregate::new(workspace_id.clone(), project_path));
            project.sessions = project.sessions.saturating_add(1);
            let timestamp = record
                .summary
                .updated_at
                .as_deref()
                .or(record.summary.created_at.as_deref());
            let updated_at_ms = timestamp.and_then(parse_timestamp_ms);
            match updated_at_ms {
                Some(updated_at_ms) => {
                    project.last_activity_ms =
                        max_option(project.last_activity_ms, Some(updated_at_ms));
                    let agent = project.agents.entry(source.agent_kind).or_default();
                    agent.sessions = agent.sessions.saturating_add(1);
                    agent.last_activity_ms =
                        max_option(agent.last_activity_ms, Some(updated_at_ms));
                }
                None => {
                    project.undated = project.undated.saturating_add(1);
                    project
                        .agents
                        .entry(source.agent_kind)
                        .or_default()
                        .sessions += 1;
                }
            }
            let identity = hash(&["native", &source.id, &workspace_id, &record.record_key]);
            let key = (
                source.runtime_scope.key(),
                identity,
                record.summary.id.clone(),
                source.agent_kind,
            );
            if let Some(attribution) = attribution.get(&key) {
                let account = project
                    .accounts
                    .entry((
                        attribution.provider_id.clone(),
                        attribution.account_label.clone(),
                        attribution.cli_kind,
                    ))
                    .or_insert_with(|| AccountAggregate {
                        provider_name: attribution.provider_name.clone(),
                        sessions: 0,
                    });
                account.sessions = account.sessions.saturating_add(1);
            }
            if let Some(usage) = usage.as_mut() {
                usage.observe(
                    source,
                    &workspace_id,
                    &workspace.workdir,
                    record,
                    window_start_ms,
                );
            }
        }
    }
    outcome.projects = by_path.into_values().collect();
    outcome.usage = usage.map(|usage| usage.with_partial(outcome.partial));
    outcome
}

/// 把周期用量附加到项目元数据上。用量读取失败不会丢掉项目列表，
/// 只写入诊断并保持用量为空。
pub(crate) fn finalize_project_report(
    collected: CollectedProjectReport,
    providers: &[Provider],
    usage: Option<AgentUsageReport>,
    usage_error: Option<String>,
) -> AgentProjectReport {
    let CollectedProjectReport {
        period,
        window_start,
        window_end,
        sessions,
        undated,
        projects,
        mut partial,
        mut diagnostics,
        usage_candidates: _,
    } = collected;
    if let Some(error) = usage_error {
        diagnostics.push(error);
    }
    let providers_by_id = providers
        .iter()
        .map(|provider| (provider.identity.id.as_str(), provider))
        .collect::<HashMap<_, _>>();
    let mut usage_by_path = HashMap::<String, AgentUsageProject>::new();
    let mut decoded_sessions = 0;
    let mut skipped_sessions = 0;
    let mut truncated = false;
    let mut pricing_error = None;
    if let Some(usage) = usage {
        partial |= usage.partial;
        truncated = usage.truncated;
        decoded_sessions = usage.counted_sessions;
        skipped_sessions = usage.skipped_sessions;
        pricing_error = usage.pricing_error;
        diagnostics.extend(
            usage
                .diagnostics
                .into_iter()
                .map(|item| format!("用量：{item}")),
        );
        for project in usage.projects {
            usage_by_path.insert(project.path.clone(), project);
        }
    }
    let known_paths = projects
        .iter()
        .map(|project| project.path.clone())
        .collect::<HashSet<_>>();
    let missing_usage_projects = usage_by_path
        .keys()
        .filter(|path| !known_paths.contains(*path))
        .count();
    if missing_usage_projects > 0 {
        diagnostics.push(format!(
            "有 {missing_usage_projects} 个用量项目未出现在会话扫描结果中"
        ));
    }
    let mut projects = projects
        .into_iter()
        .map(|project| {
            let usage_project = usage_by_path.remove(&project.path);
            let agents = project
                .agents
                .into_iter()
                .map(|(cli_kind, agent)| AgentProjectAgent {
                    cli_kind,
                    sessions: agent.sessions,
                    last_activity: format_timestamp(agent.last_activity_ms),
                })
                .collect::<Vec<_>>();
            let mut agents = agents;
            agents.sort_by(|left, right| {
                right
                    .sessions
                    .cmp(&left.sessions)
                    .then_with(|| left.cli_kind.key().cmp(right.cli_kind.key()))
            });
            let mut accounts = project
                .accounts
                .into_iter()
                .map(
                    |((provider_id, account_label, cli_kind), account)| AgentProjectAccount {
                        provider_name: providers_by_id
                            .get(provider_id.as_str())
                            .map(|provider| provider.display_label())
                            .filter(|label| !label.trim().is_empty())
                            .unwrap_or(account.provider_name),
                        provider_id,
                        account_label,
                        cli_kind,
                        sessions: account.sessions,
                    },
                )
                .collect::<Vec<_>>();
            accounts.sort_by(|left, right| {
                right
                    .sessions
                    .cmp(&left.sessions)
                    .then_with(|| left.provider_name.cmp(&right.provider_name))
                    .then_with(|| left.account_label.cmp(&right.account_label))
            });
            AgentProject {
                workspace_ids: project.workspace_ids.into_iter().collect(),
                path: project.path,
                sessions: project.sessions,
                undated_sessions: project.undated,
                last_activity: format_timestamp(project.last_activity_ms),
                agents,
                accounts,
                usage_sessions: usage_project.as_ref().map(|p| p.sessions).unwrap_or(0),
                usage: usage_project
                    .map(|project| project.usage)
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    // 用量路径与元数据路径理论上一致；若来源在两次读取间变化，仍保留
    // 元数据排序，不把用量项目单独插到列表里。
    projects.sort_by(|left, right| {
        right
            .last_activity
            .cmp(&left.last_activity)
            .then_with(|| left.path.cmp(&right.path))
    });
    AgentProjectReport {
        period,
        generated_at: Local::now().to_rfc3339(),
        window_start: window_start.to_rfc3339(),
        window_end: window_end.to_rfc3339(),
        sessions,
        undated_sessions: undated,
        decoded_sessions,
        skipped_sessions,
        partial,
        truncated,
        projects,
        diagnostics: dedupe(diagnostics),
        account_retention_days: crate::services::cli_runtime::EXITED_INSTANCE_RETENTION_DAYS,
        pricing_error,
    }
}

fn max_option(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

fn format_timestamp(milliseconds: Option<i64>) -> Option<String> {
    milliseconds
        .and_then(DateTime::from_timestamp_millis)
        .map(|date| date.with_timezone(&Local).to_rfc3339())
}
