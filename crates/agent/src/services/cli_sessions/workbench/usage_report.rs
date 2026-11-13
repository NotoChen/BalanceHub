//! 本地会话用量对账：按原生会话的最近更新时间划定窗口，解码用量后按
//! 账号、项目和模型汇总。这里只生成“本机会话日志的估算”，站点账单由
//! API 层另行读取并对比；实际费用以中转站账单为准。
//!
//! 账号归属来自 BalanceHub 的启动记录：只有经过本应用启动（或继续）的
//! 历史会话才带有原生身份，未经过本应用的会话一律归入“未关联账号”，
//! 不根据工作目录或模型猜测账号。

use super::{
    budget::HistoryReadCache,
    emit_progress, hash,
    scope::{self, history_adapter},
    state, with_read_budget, with_source_root,
    workspace_scan::scan_workspaces,
    AgentSessionActor, ProjectProgressReporter,
};
use crate::{
    models::{
        AgentCliKind, AgentProjectProgressPhase, AgentRuntimeScope, AgentSessionSource,
        AgentUsageAccount, AgentUsageProject, AgentUsageReport, Provider, SessionTokenCounts,
        SessionUsageSummary,
    },
    services::{
        agent_cli::contracts::{
            SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan, SessionHistorySource,
            SessionReadBudget,
        },
        cli_sessions::transcript::Transcript,
    },
};
use chrono::{DateTime, Local};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

/// 单次对账最多解码的会话数。只统计最近活跃的会话，避免一次打开就
/// 顺序解析全部历史；命中上限时报告会明确标记为截断。
const MAX_REPORT_SESSIONS: usize = 300;
/// 单次对账一轮读取（元数据扫描或正文解码）的总字节预算；调用方预算更小时以调用方为准。
const REPORT_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) struct CollectedUsageReport {
    period: String,
    window_start: DateTime<Local>,
    window_end: DateTime<Local>,
    scanned: u64,
    undated: u64,
    skipped: u64,
    truncated: bool,
    partial: bool,
    diagnostics: Vec<String>,
    model_ids: BTreeSet<String>,
    facts: Vec<UsageFact>,
}

impl CollectedUsageReport {
    /// 已解码会话涉及的模型 ID，用于在计价前补齐 models.dev 价格资料。
    pub(crate) fn model_ids(&self) -> Vec<String> {
        self.model_ids.iter().cloned().collect()
    }
}

struct UsageCandidate {
    source_id: String,
    workspace_id: String,
    source_identity: String,
    native_id: String,
    cli_kind: AgentCliKind,
    runtime_scope: AgentRuntimeScope,
    project: String,
    updated_at_ms: i64,
    record: SessionHistoryRecord,
}

struct UsageFact {
    project: String,
    source_identity: String,
    native_id: String,
    cli_kind: AgentCliKind,
    runtime_scope: AgentRuntimeScope,
    usage: SessionUsageSummary,
}

#[derive(Default)]
struct SourceScanOutcome {
    usage: SourceUsageOutcome,
    diagnostics: Vec<String>,
    error: Option<String>,
}

/// 单来源的用量候选累加器。项目视图在同一次元数据遍历中调用 `observe`，
/// 用量对账在自己的扫描线程中调用同一方法，窗口和跳过规则只保留一份。
#[derive(Default)]
pub(crate) struct SourceUsageOutcome {
    candidates: Vec<UsageCandidate>,
    scanned: u64,
    undated: u64,
    skipped: u64,
    partial: bool,
}

impl SourceUsageOutcome {
    pub(crate) fn observe(
        &mut self,
        source: &AgentSessionSource,
        workspace_id: &str,
        workspace_workdir: &Path,
        record: SessionHistoryRecord,
        window_start_ms: i64,
    ) {
        let timestamp = record
            .summary
            .updated_at
            .as_deref()
            .or(record.summary.created_at.as_deref());
        let Some(updated_at_ms) = timestamp.and_then(parse_timestamp_ms) else {
            self.undated = self.undated.saturating_add(1);
            return;
        };
        if updated_at_ms < window_start_ms {
            return;
        }
        self.scanned = self.scanned.saturating_add(1);
        if record.content_unavailable_reason.is_some() {
            self.skipped = self.skipped.saturating_add(1);
            return;
        }
        let project = if record.summary.workdir.trim().is_empty() {
            workspace_workdir.to_string_lossy().into_owned()
        } else {
            record.summary.workdir.clone()
        };
        self.candidates.push(UsageCandidate {
            source_id: source.id.clone(),
            source_identity: hash(&["native", &source.id, workspace_id, &record.record_key]),
            workspace_id: workspace_id.to_string(),
            native_id: record.summary.id.clone(),
            cli_kind: source.agent_kind,
            runtime_scope: source.runtime_scope.clone(),
            project,
            updated_at_ms,
            record,
        });
    }

    pub(crate) fn failed() -> Self {
        Self {
            partial: true,
            ..Self::default()
        }
    }

    pub(crate) fn with_partial(mut self, partial: bool) -> Self {
        self.partial |= partial;
        self
    }
}

#[derive(Default)]
struct SourceDecodeOutcome {
    facts: Vec<UsageFact>,
    skipped: u64,
    missing_records: u64,
    failed_reads: u64,
    partial: bool,
    truncated: bool,
}

struct DecodeProgress<'a> {
    total: u64,
    processed: &'a AtomicU64,
    reporter: Option<ProjectProgressReporter>,
}

impl SourceDecodeOutcome {
    fn diagnostics(&self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        if self.missing_records > 0 {
            diagnostics.push(format!(
                "有 {} 条损坏或异常超长记录无法读取，相关会话的用量不完整",
                self.missing_records
            ));
        }
        if self.failed_reads > 0 {
            diagnostics.push(format!(
                "有 {} 个会话的正文无法读取，未计入用量",
                self.failed_reads
            ));
        }
        diagnostics
    }
}

/// 一次元数据扫描得到的用量候选。项目视图复用同一次扫描的项目聚合与
/// 候选列表，只在第二阶段解码正文，避免为了用量再遍历一遍历史目录。
pub(crate) struct CollectedUsageCandidates {
    period: String,
    window_start: DateTime<Local>,
    window_end: DateTime<Local>,
    scanned: u64,
    undated: u64,
    skipped: u64,
    truncated: bool,
    partial: bool,
    diagnostics: Vec<String>,
    candidates: Vec<UsageCandidate>,
    sources: HistorySourceList,
}

impl CollectedUsageCandidates {
    fn empty(
        period: String,
        window_start: DateTime<Local>,
        window_end: DateTime<Local>,
        sources: HistorySourceList,
        diagnostics: Vec<String>,
    ) -> Self {
        Self {
            period,
            window_start,
            window_end,
            scanned: 0,
            undated: 0,
            skipped: 0,
            truncated: false,
            partial: false,
            diagnostics,
            candidates: Vec::new(),
            sources,
        }
    }

    /// 解码候选会话正文并汇总。进度按已处理候选计数，扫描阶段已经完成，
    /// 这里不再触碰历史目录。
    pub(crate) fn decode(
        self,
        actor: AgentSessionActor<'_>,
        budget: &SessionReadBudget,
        reporter: Option<ProjectProgressReporter>,
    ) -> Result<CollectedUsageReport, String> {
        let Self {
            period,
            window_start,
            window_end,
            scanned,
            undated,
            mut skipped,
            mut truncated,
            mut partial,
            mut diagnostics,
            candidates,
            sources,
        } = self;
        let total = candidates.len() as u64;
        emit_progress(
            &reporter,
            AgentProjectProgressPhase::Decoding,
            0,
            total,
            format!("解码 {total} 个会话的用量"),
        );
        let per_source_bytes =
            (budget.max_bytes.min(REPORT_BYTES) / sources.len().max(1) as u64).max(1);
        let mut by_source = BTreeMap::<String, Vec<UsageCandidate>>::new();
        for candidate in candidates {
            by_source
                .entry(candidate.source_id.clone())
                .or_default()
                .push(candidate);
        }
        let processed = AtomicU64::new(0);
        let decoded = std::thread::scope(|threads| {
            let handles = by_source
                .into_iter()
                .filter_map(|(source_id, candidates)| {
                    let (source, config) = sources
                        .iter()
                        .find(|(source, _)| source.id == source_id)?
                        .clone();
                    let cache = state::source_resources(actor, &source).cache;
                    let decode_budget = SessionReadBudget::new(
                        budget.deadline,
                        budget.cancelled.clone(),
                        per_source_bytes,
                    );
                    let progress = DecodeProgress {
                        total,
                        processed: &processed,
                        reporter: reporter.clone(),
                    };
                    Some(threads.spawn(move || {
                        decode_usage_source(
                            &source,
                            &config,
                            &cache,
                            &decode_budget,
                            candidates,
                            progress,
                        )
                    }))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| "会话用量解码线程异常".to_string())
                })
                .collect::<Result<Vec<_>, String>>()
        })?;

        let mut facts = Vec::new();
        let mut model_ids = BTreeSet::new();
        for outcome in decoded {
            skipped = skipped.saturating_add(outcome.skipped);
            partial |= outcome.partial;
            truncated |= outcome.truncated;
            diagnostics.extend(outcome.diagnostics());
            for fact in outcome.facts {
                model_ids.extend(
                    fact.usage
                        .models
                        .iter()
                        .filter_map(|model| model.model.clone()),
                );
                facts.push(fact);
            }
        }
        if budget.cancelled.load(Ordering::Acquire) {
            partial = true;
        }
        diagnostics = dedupe(diagnostics);
        Ok(CollectedUsageReport {
            period,
            window_start,
            window_end,
            scanned,
            undated,
            skipped,
            truncated,
            partial,
            diagnostics,
            model_ids,
            facts,
        })
    }
}

/// 合并各来源的候选并应用单次上限。项目视图和用量对账共用这一份裁剪规则。
pub(super) fn build_usage_candidates(
    sources: HistorySourceList,
    period: &str,
    window_start: DateTime<Local>,
    window_end: DateTime<Local>,
    outcomes: Vec<SourceUsageOutcome>,
    mut diagnostics: Vec<String>,
) -> CollectedUsageCandidates {
    let mut scanned = 0u64;
    let mut undated = 0u64;
    let mut skipped = 0u64;
    let mut partial = false;
    let mut candidates = Vec::new();
    for outcome in outcomes {
        scanned = scanned.saturating_add(outcome.scanned);
        undated = undated.saturating_add(outcome.undated);
        skipped = skipped.saturating_add(outcome.skipped);
        partial |= outcome.partial;
        candidates.extend(outcome.candidates);
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.updated_at_ms));
    let mut truncated = false;
    if candidates.len() > MAX_REPORT_SESSIONS {
        skipped = skipped.saturating_add((candidates.len() - MAX_REPORT_SESSIONS) as u64);
        candidates.truncate(MAX_REPORT_SESSIONS);
        truncated = true;
        diagnostics.push(format!("单次最多统计最近 {MAX_REPORT_SESSIONS} 个会话"));
    }
    CollectedUsageCandidates {
        period: period.to_string(),
        window_start,
        window_end,
        scanned,
        undated,
        skipped,
        truncated,
        partial,
        diagnostics,
        candidates,
        sources,
    }
}

pub(super) struct Attribution {
    pub(super) provider_id: String,
    pub(super) provider_name: String,
    pub(super) account_label: String,
    pub(super) cli_kind: AgentCliKind,
}

pub(super) type AttributionKey = (String, String, String, AgentCliKind);

/// 启动记录按时间倒序；同一会话被继续过多次时保留最近一次启动的账号。
/// 返回的布尔值表示记录是否触及条数上限，调用方据此提示归属可能缺失。
pub(super) fn attribution_index() -> (HashMap<AttributionKey, Attribution>, bool) {
    let instances = crate::services::cli_runtime::runtime_instances();
    let mut attribution = HashMap::<AttributionKey, Attribution>::new();
    for instance in &instances {
        let Some(provider_id) = instance.provider_id.as_deref() else {
            continue;
        };
        let Some(identity) = instance.native_session.as_ref() else {
            continue;
        };
        attribution
            .entry((
                identity.runtime_scope.key(),
                identity.source_identity.clone(),
                identity.native_session_id.clone(),
                instance.cli_kind,
            ))
            .or_insert_with(|| Attribution {
                provider_id: provider_id.to_string(),
                provider_name: instance.provider_name.clone().unwrap_or_default(),
                account_label: instance.account_label.clone(),
                cli_kind: instance.cli_kind,
            });
    }
    (
        attribution,
        instances.len() >= crate::services::cli_runtime::MAX_ACTIVE_INSTANCES,
    )
}

struct AccountBucket {
    provider_name: String,
    sessions: u64,
    usage: SessionUsageSummary,
}

struct ProjectBucket {
    sessions: u64,
    usage: SessionUsageSummary,
}

/// 校验对外公开的统计周期。API 在发起网络读取前先用它拒绝无效参数。
pub(crate) fn validate_usage_period(period: &str) -> Result<(), String> {
    crate::models::AGENT_USAGE_PERIODS
        .contains(&period)
        .then_some(())
        .ok_or_else(|| "用量周期仅支持今日、7 天或 30 天".to_string())
}

pub(super) fn usage_window_start(
    period: &str,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, String> {
    validate_usage_period(period)?;
    Ok(match period {
        // 与站点“今日”口径一致：本地零点到现在，而不是固定 24 小时。
        "24h" => now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| start.and_local_timezone(Local).earliest())
            .unwrap_or(now),
        "7d" => now - chrono::Duration::days(7),
        _ => now - chrono::Duration::days(30),
    })
}

/// 一个原生来源和它对应的历史读取配置。项目视图与用量统计共用同一份
/// 来源发现结果，避免各自解释来源可用性。
pub(super) type HistorySourceList = Vec<(AgentSessionSource, SessionHistorySource)>;

pub(super) fn history_sources(
    actor: AgentSessionActor<'_>,
) -> Result<(HistorySourceList, Vec<String>), String> {
    let scope = super::scope::build_scope(actor, None)?;
    let mut diagnostics = Vec::new();
    let mut sources = Vec::new();
    for source in &scope.public.sources {
        if !source.available {
            continue;
        }
        if let AgentRuntimeScope::Wsl { distro_id } = &source.runtime_scope {
            if let Err(error) = crate::services::wsl::require_running(distro_id) {
                diagnostics.push(format!("{}：{error}", source.label));
                continue;
            }
        }
        let Some(config) = scope.sources.get(&source.id).cloned() else {
            continue;
        };
        sources.push((source.clone(), config));
    }
    Ok((sources, diagnostics))
}

/// 读取一个原生来源的会话元数据。用量对账与项目视图共用同一次来源读取，
/// 避免各自维护一套预算、缓存和 WSL 根路径处理。
pub(super) fn load_history_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    cache: &HistoryReadCache,
    budget: &SessionReadBudget,
) -> Result<SessionHistoryScan, String> {
    let adapter = history_adapter(source.agent_kind)?;
    with_read_budget(budget, || {
        super::budget::with_history_cache(cache, || {
            with_source_root(&config.config_root, || {
                scan_workspaces(
                    adapter,
                    source.agent_kind,
                    &config.config_root,
                    &[],
                    budget,
                    SessionHistoryReadMode::Summaries,
                )
            })
        })
    })
}

/// 扫描所有可用的原生来源，解码窗口内最近活跃的会话并汇总用量。
/// 扫描/解码预算耗尽、损坏记录和单次上限都会写入诊断，不把部分结果
/// 当作错误丢弃。
pub(crate) fn collect_usage_report(
    actor: AgentSessionActor<'_>,
    period: &str,
    budget: &SessionReadBudget,
) -> Result<CollectedUsageReport, String> {
    scan_usage_candidates(actor, period, budget)?.decode(actor, budget, None)
}

/// 只扫描元数据并生成用量候选；项目视图把这一步和自己的项目聚合合并，
/// 避免为了用量再遍历一遍历史目录。
pub(super) fn scan_usage_candidates(
    actor: AgentSessionActor<'_>,
    period: &str,
    budget: &SessionReadBudget,
) -> Result<CollectedUsageCandidates, String> {
    let window_end = Local::now();
    let window_start = usage_window_start(period, window_end)?;
    let window_start_ms = window_start.timestamp_millis();
    let (sources, mut diagnostics) = history_sources(actor)?;
    if sources.is_empty() {
        diagnostics.push("没有可用的原生会话来源".into());
        return Ok(CollectedUsageCandidates::empty(
            period.to_string(),
            window_start,
            window_end,
            sources,
            diagnostics,
        ));
    }
    let per_source_bytes =
        (budget.max_bytes.min(REPORT_BYTES) / sources.len().max(1) as u64).max(1);

    // 第一轮只扫描元数据，先得到窗口内的候选会话；解码留给第二轮，
    // 这样单次上限可以按最近更新时间统一裁剪，而不是按来源平分。
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
                threads.spawn(move || {
                    scan_usage_source(&source, &config, &cache, &source_budget, window_start_ms)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "会话用量扫描线程异常".to_string())
            })
            .collect::<Result<Vec<_>, String>>()
    })?;

    let mut usage_outcomes = Vec::with_capacity(outcomes.len());
    for (index, outcome) in outcomes.into_iter().enumerate() {
        let SourceScanOutcome {
            usage,
            diagnostics: source_diagnostics,
            error,
        } = outcome;
        if let Some(error) = error {
            diagnostics.push(format!("{}：{error}", sources[index].0.label));
        }
        diagnostics.extend(source_diagnostics);
        usage_outcomes.push(usage);
    }
    Ok(build_usage_candidates(
        sources,
        period,
        window_start,
        window_end,
        usage_outcomes,
        diagnostics,
    ))
}

fn scan_usage_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    cache: &HistoryReadCache,
    budget: &SessionReadBudget,
    window_start_ms: i64,
) -> SourceScanOutcome {
    let mut outcome = SourceScanOutcome::default();
    let loaded = match load_history_source(source, config, cache, budget) {
        Ok(loaded) => loaded,
        Err(error) => {
            outcome.error = Some(error);
            outcome.usage = SourceUsageOutcome::failed();
            return outcome;
        }
    };
    outcome.diagnostics.extend(loaded.diagnostics);
    for workspace in loaded.workspaces {
        if !workspace.complete {
            outcome.usage.partial = true;
        }
        outcome.diagnostics.extend(workspace.diagnostics);
        let workspace_id = scope::workspace_for_path(&workspace.workdir).id;
        for record in workspace.records {
            outcome.usage.observe(
                source,
                &workspace_id,
                &workspace.workdir,
                record,
                window_start_ms,
            );
        }
    }
    outcome
}

fn decode_usage_source(
    source: &AgentSessionSource,
    config: &SessionHistorySource,
    cache: &HistoryReadCache,
    budget: &SessionReadBudget,
    candidates: Vec<UsageCandidate>,
    progress: DecodeProgress<'_>,
) -> SourceDecodeOutcome {
    let mut outcome = SourceDecodeOutcome::default();
    let adapter = match history_adapter(source.agent_kind) {
        Ok(adapter) => adapter,
        Err(_) => {
            outcome.skipped = candidates.len() as u64;
            return outcome;
        }
    };
    with_read_budget(budget, || {
        super::budget::with_history_cache(cache, || {
            with_source_root(&config.config_root, || {
                for candidate in candidates {
                    let done = progress.processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if done == progress.total || done.is_multiple_of(10) {
                        emit_progress(
                            &progress.reporter,
                            AgentProjectProgressPhase::Decoding,
                            done,
                            progress.total,
                            format!("已处理 {done}/{} 个会话", progress.total),
                        );
                    }
                    if budget.check().is_err() {
                        // 预算耗尽后其余候选按跳过处理；报告保留已解码部分。
                        outcome.skipped = outcome.skipped.saturating_add(1);
                        outcome.truncated = true;
                        outcome.partial = true;
                        continue;
                    }
                    match Transcript::load(
                        &candidate.workspace_id,
                        &candidate.record.record_key,
                        &candidate.record,
                        adapter,
                        budget,
                    ) {
                        Ok(transcript) => {
                            let mut usage = transcript.usage();
                            usage.partial |= transcript.truncated
                                || transcript.window.is_some()
                                || !transcript.complete
                                || transcript.skipped_records > 0;
                            outcome.missing_records = outcome
                                .missing_records
                                .saturating_add(transcript.skipped_records);
                            outcome.partial |= usage.partial;
                            outcome.truncated |= transcript.truncated;
                            outcome.facts.push(UsageFact {
                                project: candidate.project,
                                source_identity: candidate.source_identity,
                                native_id: candidate.native_id,
                                cli_kind: candidate.cli_kind,
                                runtime_scope: candidate.runtime_scope,
                                usage,
                            });
                        }
                        Err(_) if budget.check().is_err() => {
                            outcome.skipped = outcome.skipped.saturating_add(1);
                            outcome.truncated = true;
                            outcome.partial = true;
                        }
                        Err(_) => {
                            outcome.skipped = outcome.skipped.saturating_add(1);
                            outcome.failed_reads = outcome.failed_reads.saturating_add(1);
                        }
                    }
                }
            })
        })
    });
    outcome
}

/// 计价并按账号、项目、模型汇总。站点账单由调用方在返回后追加，
/// 这样网络读取失败不会丢掉本地结果。
pub(crate) fn finalize_usage_report(
    collected: CollectedUsageReport,
    providers: &[Provider],
    catalog: Option<&balancehub_core::services::model_catalog::ModelCatalogService>,
    mut pricing_error: Option<String>,
) -> AgentUsageReport {
    let CollectedUsageReport {
        period,
        window_start,
        window_end,
        scanned,
        undated,
        skipped,
        truncated,
        mut partial,
        mut diagnostics,
        facts,
        ..
    } = collected;
    let (attribution, account_capped) = attribution_index();
    if account_capped {
        diagnostics.push("启动记录只保留最近一段时间的条数，较早会话的账号归属可能缺失".into());
    }
    let providers_by_id = providers
        .iter()
        .map(|provider| (provider.identity.id.as_str(), provider))
        .collect::<HashMap<_, _>>();
    let mut totals = SessionUsageSummary::default();
    let mut accounts = BTreeMap::<(Option<String>, String, AgentCliKind), AccountBucket>::new();
    let mut projects = BTreeMap::<String, ProjectBucket>::new();
    let counted = facts.len() as u64;
    for fact in facts {
        let matched = attribution.get(&(
            fact.runtime_scope.key(),
            fact.source_identity.clone(),
            fact.native_id.clone(),
            fact.cli_kind,
        ));
        let provider = matched.and_then(|attribution| {
            providers_by_id
                .get(attribution.provider_id.as_str())
                .copied()
        });
        let mut usage = fact.usage;
        if let Some(catalog) = catalog {
            match catalog.estimate_with(
                usage.clone(),
                provider.map(|provider| &provider.model_config),
                provider.map(|provider| &provider.capabilities.pricing),
            ) {
                Ok(priced) => usage = priced,
                Err(error) => {
                    pricing_error.get_or_insert(error);
                }
            }
        }
        if let Some(provider) = provider {
            // 站点用量按卡片声明的归并名统计；本地也换成同一名称，模型行才能对齐。
            for model in &mut usage.models {
                if let Some(id) = model.model.as_deref() {
                    let alias = provider.model_config.usage_alias(id);
                    if alias != id {
                        model.model = Some(alias.to_string());
                    }
                }
            }
        }
        let (provider_name, account_key) = match matched {
            Some(attribution) => {
                let provider_name = providers_by_id
                    .get(attribution.provider_id.as_str())
                    .map(|provider| provider.display_label())
                    .filter(|label| !label.trim().is_empty())
                    .unwrap_or_else(|| attribution.provider_name.clone());
                (
                    provider_name,
                    (
                        Some(attribution.provider_id.clone()),
                        attribution.account_label.clone(),
                        attribution.cli_kind,
                    ),
                )
            }
            None => (String::new(), (None, String::new(), fact.cli_kind)),
        };
        let account = accounts
            .entry(account_key)
            .or_insert_with(|| AccountBucket {
                provider_name,
                sessions: 0,
                usage: SessionUsageSummary::default(),
            });
        account.sessions = account.sessions.saturating_add(1);
        account.usage.merge(&usage);
        let project = projects
            .entry(if fact.project.trim().is_empty() {
                "未记录项目".to_string()
            } else {
                fact.project.clone()
            })
            .or_insert_with(|| ProjectBucket {
                sessions: 0,
                usage: SessionUsageSummary::default(),
            });
        project.sessions = project.sessions.saturating_add(1);
        project.usage.merge(&usage);
        totals.merge(&usage);
    }
    sort_models(&mut totals);
    partial |= totals.partial;
    let mut accounts = accounts
        .into_iter()
        .map(
            |((provider_id, account_label, cli_kind), bucket)| AgentUsageAccount {
                provider_id,
                provider_name: bucket.provider_name,
                account_label,
                cli_kind,
                sessions: bucket.sessions,
                usage: bucket.usage,
            },
        )
        .collect::<Vec<_>>();
    accounts.sort_by(|left, right| {
        display_tokens(&right.usage.tokens)
            .cmp(&display_tokens(&left.usage.tokens))
            .then_with(|| left.provider_name.cmp(&right.provider_name))
            .then_with(|| left.account_label.cmp(&right.account_label))
    });
    let mut projects = projects
        .into_iter()
        .map(|(path, bucket)| AgentUsageProject {
            path,
            sessions: bucket.sessions,
            usage: bucket.usage,
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        display_tokens(&right.usage.tokens)
            .cmp(&display_tokens(&left.usage.tokens))
            .then_with(|| left.path.cmp(&right.path))
    });
    diagnostics = dedupe(diagnostics);
    AgentUsageReport {
        period,
        generated_at: Local::now().to_rfc3339(),
        window_start: window_start.to_rfc3339(),
        window_end: window_end.to_rfc3339(),
        scanned_sessions: scanned,
        counted_sessions: counted,
        skipped_sessions: skipped,
        undated_sessions: undated,
        partial,
        truncated,
        totals,
        accounts,
        projects,
        sites: Vec::new(),
        diagnostics,
        account_retention_days: crate::services::cli_runtime::EXITED_INSTANCE_RETENTION_DAYS,
        pricing_error,
    }
}

fn sort_models(usage: &mut SessionUsageSummary) {
    usage.models.sort_by(|left, right| {
        display_tokens(&right.tokens)
            .cmp(&display_tokens(&left.tokens))
            .then_with(|| left.model.cmp(&right.model))
    });
}

/// 输入已包含缓存 token，输出已包含推理 token；两者相加即模型用量排行
/// 与站点 token 口径最接近的可比数字。
fn display_tokens(tokens: &SessionTokenCounts) -> u64 {
    tokens
        .input_tokens
        .unwrap_or_default()
        .saturating_add(tokens.output_tokens.unwrap_or_default())
}

pub(super) fn parse_timestamp_ms(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
}

pub(super) fn dedupe(messages: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    messages
        .into_iter()
        .filter(|message| !message.trim().is_empty() && seen.insert(message.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentRuntimeScope, CliSessionSummary, SessionModelUsage};
    use std::path::PathBuf;

    fn history_record(
        id: &str,
        workdir: &str,
        updated_at: Option<&str>,
        content_unavailable: bool,
    ) -> SessionHistoryRecord {
        SessionHistoryRecord {
            summary: CliSessionSummary {
                id: id.into(),
                title: String::new(),
                preview: None,
                model: None,
                models: Vec::new(),
                cli_kind: AgentCliKind::ClaudeCode,
                created_at: None,
                updated_at: updated_at.map(str::to_string),
                workdir: workdir.into(),
                cli_version: None,
                archived: false,
                can_resume: false,
                metadata_source: "test".into(),
            },
            locator: PathBuf::from("/tmp/session"),
            record_key: id.into(),
            role: crate::models::AgentSessionRole::Main,
            parent_native_id: None,
            resume_reason: None,
            content_unavailable_reason: content_unavailable.then(|| "unreadable".to_string()),
        }
    }

    fn history_source() -> AgentSessionSource {
        AgentSessionSource {
            id: "source".into(),
            agent_kind: AgentCliKind::ClaudeCode,
            config_root: String::new(),
            available: true,
            runtime_scope: AgentRuntimeScope::Native,
            label: "Claude".into(),
            resume_cli: None,
        }
    }

    fn tokens(input: u64, output: u64) -> SessionTokenCounts {
        SessionTokenCounts {
            input_tokens: Some(input),
            output_tokens: Some(output),
            ..SessionTokenCounts::default()
        }
    }

    fn fact(project: &str, model: &str, input: u64, output: u64) -> UsageFact {
        UsageFact {
            project: project.into(),
            source_identity: format!("source-{project}"),
            native_id: format!("session-{project}"),
            cli_kind: AgentCliKind::Codex,
            runtime_scope: AgentRuntimeScope::Native,
            usage: SessionUsageSummary {
                tokens: tokens(input, output),
                models: vec![SessionModelUsage {
                    model: Some(model.into()),
                    tokens: tokens(input, output),
                    records: 1,
                    estimated_cost_microusd: None,
                }],
                records: 1,
                ..SessionUsageSummary::default()
            },
        }
    }

    fn collected(facts: Vec<UsageFact>) -> CollectedUsageReport {
        let now = Local::now();
        CollectedUsageReport {
            period: "7d".into(),
            window_start: now - chrono::Duration::days(7),
            window_end: now,
            scanned: facts.len() as u64,
            undated: 0,
            skipped: 0,
            truncated: false,
            partial: false,
            diagnostics: Vec::new(),
            model_ids: BTreeSet::new(),
            facts,
        }
    }

    #[test]
    fn usage_observation_keeps_window_skip_and_project_fallback_rules() {
        let source = history_source();
        let window_start_ms = parse_timestamp_ms("2027-01-15T07:00:00+08:00").unwrap();
        let mut outcome = SourceUsageOutcome::default();
        outcome.observe(
            &source,
            "workspace",
            Path::new("/fallback"),
            history_record("undated", "", None, false),
            window_start_ms,
        );
        outcome.observe(
            &source,
            "workspace",
            Path::new("/fallback"),
            history_record("old", "", Some("2027-01-14T07:00:00+08:00"), false),
            window_start_ms,
        );
        outcome.observe(
            &source,
            "workspace",
            Path::new("/fallback"),
            history_record("candidate", "", Some("2027-01-15T08:00:00+08:00"), false),
            window_start_ms,
        );
        outcome.observe(
            &source,
            "workspace",
            Path::new("/fallback"),
            history_record(
                "unavailable",
                "/work/alpha",
                Some("2027-01-15T08:00:00+08:00"),
                true,
            ),
            window_start_ms,
        );

        assert_eq!(outcome.undated, 1);
        assert_eq!(outcome.scanned, 2);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(outcome.candidates[0].project, "/fallback");
        assert_eq!(outcome.candidates[0].native_id, "candidate");
    }

    #[test]
    fn usage_candidate_selection_keeps_newest_and_marks_truncation() {
        let mut outcome = SourceUsageOutcome::default();
        for index in 0..(MAX_REPORT_SESSIONS + 3) {
            let id = format!("session-{index}");
            outcome.candidates.push(UsageCandidate {
                source_id: "source".into(),
                source_identity: format!("identity-{index}"),
                workspace_id: "workspace".into(),
                native_id: id.clone(),
                cli_kind: AgentCliKind::ClaudeCode,
                runtime_scope: AgentRuntimeScope::Native,
                project: "/work/alpha".into(),
                updated_at_ms: index as i64,
                record: history_record(&id, "/work/alpha", None, false),
            });
        }
        let now = Local::now();
        let built = build_usage_candidates(
            Vec::new(),
            "7d",
            now - chrono::Duration::days(7),
            now,
            vec![outcome],
            Vec::new(),
        );

        assert!(built.truncated);
        assert_eq!(built.candidates.len(), MAX_REPORT_SESSIONS);
        assert_eq!(built.skipped, 3);
        assert!(built
            .diagnostics
            .iter()
            .any(|message| message.contains("300")));
        assert_eq!(built.candidates[0].native_id, "session-302");
    }

    #[test]
    fn today_starts_at_local_midnight() {
        let now = Local::now();
        let start = usage_window_start("24h", now).unwrap();
        assert_eq!(start.date_naive(), now.date_naive());
        assert_eq!(start.time(), chrono::NaiveTime::MIN);
    }

    #[test]
    fn periods_roll_back_from_now() {
        let now = Local::now();
        assert_eq!(
            usage_window_start("7d", now).unwrap(),
            now - chrono::Duration::days(7)
        );
        assert_eq!(
            usage_window_start("30d", now).unwrap(),
            now - chrono::Duration::days(30)
        );
        assert!(usage_window_start("90d", now).is_err());
    }

    #[test]
    fn finalize_groups_by_project_and_unattributed_account() {
        let report = finalize_usage_report(
            collected(vec![
                fact("/work/alpha", "model-a", 100, 10),
                fact("/work/alpha", "model-a", 50, 5),
                fact("/work/beta", "model-b", 20, 2),
            ]),
            &[],
            None,
            None,
        );
        assert_eq!(report.counted_sessions, 3);
        assert_eq!(report.totals.tokens.input_tokens, Some(170));
        assert_eq!(report.accounts.len(), 1);
        assert_eq!(report.accounts[0].provider_id, None);
        assert_eq!(report.accounts[0].sessions, 3);
        assert_eq!(report.projects.len(), 2);
        assert_eq!(report.projects[0].path, "/work/alpha");
        assert_eq!(report.projects[0].sessions, 2);
        assert_eq!(report.totals.models.len(), 2);
        assert_eq!(report.totals.models[0].model.as_deref(), Some("model-a"));
    }

    #[test]
    fn finalize_keeps_partial_and_truncated_flags() {
        let mut inputs = collected(vec![fact("/work/alpha", "model-a", 10, 1)]);
        inputs.partial = true;
        inputs.truncated = true;
        inputs.diagnostics = vec!["同一诊断".into(), "同一诊断".into(), String::new()];
        let report = finalize_usage_report(inputs, &[], None, Some("价格资料不可用".into()));
        assert!(report.partial);
        assert!(report.truncated);
        assert_eq!(report.diagnostics, vec!["同一诊断".to_string()]);
        assert_eq!(report.pricing_error.as_deref(), Some("价格资料不可用"));
    }
}
