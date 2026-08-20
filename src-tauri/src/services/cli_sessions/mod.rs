use crate::{
    models::{
        AgentCliKind, AppSettings, CliSessionDetail, CliSessionIndexState, CliSessionMessage,
        CliSessionSearchResponse, CliSessionSearchResult, CliSessionSummary,
    },
    services::agent_cli::{self, contracts::SessionReadLimits},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::AppHandle;

mod index;
mod io;
mod search;
#[cfg(test)]
mod tests;

pub(crate) use index::{
    clear as clear_index, config as index_config, reconfigure as reconfigure_index,
    schedule_build as schedule_index_build, search as search_index, status as index_status,
    BuildRequest as SessionIndexBuildRequest, BuildScheduleState as SessionIndexBuildScheduleState,
};
pub(crate) use io::{
    compact_json, json_record_may_match, json_text, read_json_lines_limited,
    scan_json_lines_matching, scan_json_records, scan_json_records_background,
    session_index_source_fingerprint,
};
pub(crate) use search::{
    combine_content_search_results, truncate_text, SessionContentSearchCollector,
};
pub(crate) use search::{SearchAccumulator, SearchQuery};

const MAX_SESSIONS: usize = 100;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_SEARCH_QUERY_CHARS: usize = 200;
const MAX_SEARCH_TERMS: usize = 8;
const SUMMARY_CACHE_TTL: Duration = Duration::from_secs(30);
const SUMMARY_CACHE_RETENTION: Duration = Duration::from_secs(10 * 60);
const MAX_SUMMARY_CACHE_ENTRIES: usize = 64;
const DETAIL_READ_LIMITS: SessionReadLimits = SessionReadLimits {
    max_file_bytes: 32 * 1024 * 1024,
    max_messages: 800,
    max_total_chars: 4 * 1024 * 1024,
    max_message_chars: 32 * 1024,
};

pub(crate) struct SearchOptions<'a> {
    pub query: &'a str,
    pub limit: usize,
    pub force_refresh: bool,
}

pub fn search(
    app: &AppHandle,
    settings: &AppSettings,
    cli_kind: AgentCliKind,
    workdir: &Path,
    options: SearchOptions<'_>,
    is_current: impl Fn() -> bool,
) -> Result<CliSessionSearchResponse, String> {
    let adapter = session_adapter(cli_kind, workdir)?;
    let sessions = load_session_summaries(adapter, cli_kind, workdir, options.force_refresh)?;
    let query = SearchQuery::new(options.query)?;
    let result_limit = options.limit.clamp(1, MAX_SEARCH_RESULTS);
    let config = index_config(app, settings)?;
    if config.enabled && adapter.supports_index() {
        let outcome = match search_index(
            cli_kind,
            workdir,
            &sessions,
            &query,
            result_limit,
            &config,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                let results = cold_search(
                    adapter,
                    cli_kind,
                    workdir,
                    &sessions,
                    &query,
                    result_limit,
                    &is_current,
                )?;
                let schedule_state = schedule_index_build(
                    app.clone(),
                    SessionIndexBuildRequest {
                        cli_kind,
                        workdir: workdir.to_path_buf(),
                        sessions,
                        adapter,
                        config,
                        reset_database: true,
                    },
                );
                return Ok(CliSessionSearchResponse {
                    results,
                    index_state: CliSessionIndexState::Fallback,
                    index_message: Some(match schedule_state {
                        SessionIndexBuildScheduleState::Scheduled
                        | SessionIndexBuildScheduleState::Active => {
                            format!("索引暂不可用，已降级为受控扫描并在后台重建：{error}")
                        }
                        SessionIndexBuildScheduleState::CoolingDown => format!(
                            "索引暂不可用，已降级为受控扫描；后台重建将在后续搜索中低频重试：{error}"
                        ),
                        SessionIndexBuildScheduleState::Skipped => {
                            format!("索引暂不可用，已降级为受控扫描：{error}")
                        }
                    }),
                });
            }
        };
        let schedule_state = schedule_index_build(
            app.clone(),
            SessionIndexBuildRequest {
                cli_kind,
                workdir: workdir.to_path_buf(),
                sessions,
                adapter,
                config,
                reset_database: false,
            },
        );
        return Ok(CliSessionSearchResponse {
            results: outcome.results,
            index_state: outcome.state,
            index_message: if outcome.state == CliSessionIndexState::Building
                && schedule_state == SessionIndexBuildScheduleState::CoolingDown
            {
                Some("部分会话暂不可读，将在后续搜索中低频重试".to_string())
            } else {
                outcome.message
            },
        });
    }

    let results = cold_search(
        adapter,
        cli_kind,
        workdir,
        &sessions,
        &query,
        result_limit,
        &is_current,
    )?;
    Ok(CliSessionSearchResponse {
        results,
        index_state: if config.enabled {
            CliSessionIndexState::Fallback
        } else {
            CliSessionIndexState::Disabled
        },
        index_message: Some(if config.enabled {
            "当前 Agent 不支持增量索引，已使用受控扫描".to_string()
        } else {
            "会话索引已关闭，搜索会直接扫描可见对话正文".to_string()
        }),
    })
}

#[derive(Clone)]
struct SummaryCacheEntry {
    loaded_at: Instant,
    sessions: Vec<CliSessionSummary>,
}

fn summary_cache() -> &'static Mutex<HashMap<String, SummaryCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SummaryCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn summary_scan_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

fn load_session_summaries(
    adapter: &'static agent_cli::contracts::SessionAdapter,
    cli_kind: AgentCliKind,
    workdir: &Path,
    force_refresh: bool,
) -> Result<Vec<CliSessionSummary>, String> {
    let key = summary_cache_key(cli_kind, workdir);
    let requested_at = Instant::now();
    if !force_refresh {
        if let Some(sessions) = fresh_summary_cache(&key, requested_at) {
            return Ok(sessions);
        }
    }

    let _scan_guard = summary_scan_gate()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = summary_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        if entry.loaded_at >= requested_at
            || (!force_refresh && entry.loaded_at.elapsed() < SUMMARY_CACHE_TTL)
        {
            return Ok(entry.sessions);
        }
    }

    let mut sessions = adapter.list(cli_kind, workdir)?;
    normalize_sessions(&mut sessions);
    let mut cache = summary_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cache.retain(|_, entry| entry.loaded_at.elapsed() < SUMMARY_CACHE_RETENTION);
    if cache.len() >= MAX_SUMMARY_CACHE_ENTRIES {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.loaded_at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest_key);
        }
    }
    cache.insert(
        key,
        SummaryCacheEntry {
            loaded_at: Instant::now(),
            sessions: sessions.clone(),
        },
    );
    Ok(sessions)
}

fn fresh_summary_cache(key: &str, now: Instant) -> Option<Vec<CliSessionSummary>> {
    summary_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(key)
        .filter(|entry| {
            entry.loaded_at >= now
                || now
                    .checked_duration_since(entry.loaded_at)
                    .is_some_and(|elapsed| elapsed < SUMMARY_CACHE_TTL)
        })
        .map(|entry| entry.sessions.clone())
}

fn summary_cache_key(cli_kind: AgentCliKind, workdir: &Path) -> String {
    let mut normalized = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        normalized.make_ascii_lowercase();
    }
    format!("{}:{normalized}", cli_kind.key())
}

fn cold_search(
    adapter: &'static agent_cli::contracts::SessionAdapter,
    cli_kind: AgentCliKind,
    workdir: &Path,
    sessions: &[CliSessionSummary],
    query: &SearchQuery,
    result_limit: usize,
    is_current: &impl Fn() -> bool,
) -> Result<Vec<CliSessionSearchResult>, String> {
    if query.is_empty() {
        return Ok(sessions
            .iter()
            .take(result_limit)
            .cloned()
            .map(|session| CliSessionSearchResult { session })
            .collect());
    }
    if !adapter.supports_search() {
        return Err(format!(
            "{} 当前不支持检索会话正文",
            agent_cli::definition(cli_kind).label
        ));
    }

    let mut results = Vec::new();
    let mut detail_attempts = 0usize;
    let mut readable_details = 0usize;
    let mut last_detail_error = None;
    for session in sessions {
        if !is_current() {
            return Err("会话检索已被新的搜索替换".to_string());
        }
        let mut matched = SearchAccumulator::new(query);
        matched.observe(&session.title);
        matched.observe(&session.id);
        if let Some(preview) = session.preview.as_deref() {
            matched.observe(preview);
        }
        if let Some(model) = session.model.as_deref() {
            matched.observe(model);
        }
        for model in &session.models {
            matched.observe(model);
        }
        matched.observe(&session.workdir);

        if !matched.complete() {
            detail_attempts += 1;
            let request = matched.content_request();
            match adapter.search(cli_kind, workdir, &session.id, &request, &is_current) {
                Ok(content) => {
                    if !is_current() {
                        return Err("会话检索已被新的搜索替换".to_string());
                    }
                    readable_details += 1;
                    matched.merge_content(content);
                }
                Err(error) => last_detail_error = Some(error),
            }
        }

        if matched.complete() {
            results.push(CliSessionSearchResult {
                session: session.clone(),
            });
            if results.len() >= result_limit {
                break;
            }
        }
    }

    if results.is_empty() && detail_attempts > 0 && readable_details == 0 {
        if let Some(error) = last_detail_error {
            return Err(format!("会话摘要可读取，但正文检索失败：{error}"));
        }
    }
    Ok(results)
}

pub fn detail(
    cli_kind: AgentCliKind,
    workdir: &Path,
    session_id: &str,
) -> Result<CliSessionDetail, String> {
    let adapter = session_adapter(cli_kind, workdir)?;
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("会话 ID 不能为空".to_string());
    }
    if session_id.chars().count() > 512 {
        return Err("会话 ID 过长".to_string());
    }
    if session_id.chars().any(char::is_control) {
        return Err("会话 ID 不能包含控制字符".to_string());
    }
    if session_id
        .chars()
        .any(|character| matches!(character, '/' | '\\'))
    {
        return Err("会话 ID 不能包含路径分隔符".to_string());
    }
    adapter.detail(cli_kind, workdir, session_id, DETAIL_READ_LIMITS)
}

fn session_adapter(
    cli_kind: AgentCliKind,
    workdir: &Path,
) -> Result<&'static agent_cli::contracts::SessionAdapter, String> {
    let definition = agent_cli::definition(cli_kind);
    let adapter = definition
        .sessions()
        .ok_or_else(|| format!("{} 当前不支持读取历史会话", definition.label))?;
    if !workdir.is_dir() {
        return Err("工作目录不存在，无法读取历史会话".to_string());
    }
    Ok(adapter)
}

fn normalize_sessions(sessions: &mut Vec<CliSessionSummary>) {
    sessions.sort_by(|left, right| {
        session_sort_key(right.updated_at.as_deref())
            .cmp(&session_sort_key(left.updated_at.as_deref()))
            .then_with(|| left.id.cmp(&right.id))
    });
    // CLI 状态目录会在进程启动后、用户尚未发送任何消息时留下空壳记录。
    // 这些记录没有可展示内容，也不能为用户提供有效的恢复目标；只在
    // BalanceHub 的读取结果中过滤，不修改 CLI 自己维护的原始索引。
    sessions.retain(|session| !is_empty_shell(session));
    sessions.truncate(MAX_SESSIONS);
}

fn is_empty_shell(session: &CliSessionSummary) -> bool {
    session.title == "未命名会话" && session.preview.is_none()
}

pub(crate) fn session_sort_key(value: Option<&str>) -> i64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or_default()
}

pub(crate) fn clean_text(value: impl AsRef<str>, limit: usize) -> Option<String> {
    let value = value
        .as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if value.is_empty() {
        return None;
    }
    let mut text = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        text.push_str("...");
    }
    Some(text)
}

pub(crate) fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> String {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "未命名会话".to_string())
}

pub(crate) fn timestamp_from_value(value: Option<i64>, milliseconds: bool) -> Option<String> {
    let value = value?;
    let millis = if milliseconds {
        value
    } else if value.abs() < 100_000_000_000 {
        value.saturating_mul(1000)
    } else {
        value
    };
    chrono::DateTime::from_timestamp_millis(millis).map(|date| date.to_rfc3339())
}

pub(crate) fn normalize_timestamp(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(number) = value.parse::<i64>() {
        return timestamp_from_value(Some(number), false);
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.to_rfc3339())
}

pub(crate) fn timestamp_from_unix(value: Option<i64>) -> Option<String> {
    timestamp_from_value(value, false)
}

pub(crate) struct SessionMessageCollector {
    limits: SessionReadLimits,
    messages: Vec<CliSessionMessage>,
    total_chars: usize,
    truncated: bool,
    omitted_message_count: usize,
}

impl SessionMessageCollector {
    pub(crate) fn new(limits: SessionReadLimits) -> Self {
        Self {
            limits,
            messages: Vec::new(),
            total_chars: 0,
            truncated: false,
            omitted_message_count: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        id: impl Into<String>,
        role: crate::models::CliSessionMessageRole,
        content: impl AsRef<str>,
        timestamp: Option<String>,
        model: Option<String>,
        tool_name: Option<String>,
    ) {
        let content = content.as_ref().trim();
        if content.is_empty() {
            return;
        }
        if self.messages.len() >= self.limits.max_messages
            || self.total_chars >= self.limits.max_total_chars
        {
            self.truncated = true;
            self.omitted_message_count = self.omitted_message_count.saturating_add(1);
            return;
        }

        let remaining = self.limits.max_total_chars - self.total_chars;
        let allowed = self.limits.max_message_chars.min(remaining);
        let (content, was_truncated) = truncate_text(content, allowed);
        if content.is_empty() {
            self.truncated = true;
            self.omitted_message_count = self.omitted_message_count.saturating_add(1);
            return;
        }
        self.total_chars = self.total_chars.saturating_add(content.chars().count());
        self.truncated |= was_truncated;
        self.messages.push(CliSessionMessage {
            id: id.into(),
            role,
            content,
            timestamp,
            model: model.and_then(|value| clean_text(value, 120)),
            tool_name: tool_name.and_then(|value| clean_text(value, 120)),
        });
    }

    pub(crate) fn finish(
        mut self,
        source_truncated: bool,
    ) -> (Vec<CliSessionMessage>, bool, usize) {
        self.truncated |= source_truncated;
        (self.messages, self.truncated, self.omitted_message_count)
    }
}
