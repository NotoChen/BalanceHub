use crate::{
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT},
    models::{
        AgentCliKind, AppSettings, CliSessionIndexAgentStats, CliSessionIndexState,
        CliSessionIndexStatus, CliSessionMessageRole, CliSessionSearchResult, CliSessionSummary,
    },
    services::agent_cli::contracts::{SessionAdapter, SessionIndexLoadResult, SessionIndexMessage},
    util::unix_millis,
};
use rusqlite::{params, Connection, OpenFlags};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock, RwLock,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

use super::{SearchAccumulator, SearchQuery, SessionContentSearchCollector};

pub(crate) const SESSION_INDEX_UPDATED_EVENT: &str = "cli-session-index-updated";
const INDEX_TASK_KIND: &str = "sessionIndex";
const INDEX_SCHEMA_VERSION: i64 = 3;
const MESSAGE_CHUNK_CHARS: usize = 32 * 1024;
const MESSAGE_CHUNK_OVERLAP_CHARS: usize = 96;
const READY_REFRESH_COOLDOWN: Duration = Duration::from_secs(60);
const FAILED_REFRESH_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const MAINTENANCE_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CAPACITY_EVICTION_BATCH: usize = 16;

#[derive(Debug, Clone)]
pub(crate) struct SessionIndexConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub max_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedSearchOutcome {
    pub results: Vec<CliSessionSearchResult>,
    pub state: CliSessionIndexState,
    pub message: Option<String>,
}

#[derive(Clone)]
pub(crate) struct BuildRequest {
    pub cli_kind: AgentCliKind,
    pub workdir: PathBuf,
    pub sessions: Vec<CliSessionSummary>,
    pub adapter: &'static SessionAdapter,
    pub config: SessionIndexConfig,
    pub reset_database: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildScheduleState {
    Scheduled,
    Active,
    CoolingDown,
    Skipped,
}

struct QueuedBuild {
    app: AppHandle,
    key: String,
    input_fingerprint: u64,
    request: BuildRequest,
    cancelled: Arc<AtomicBool>,
}

enum MaintenanceKind {
    Clear,
    EnforceCapacity,
}

struct QueuedMaintenance {
    app: Option<AppHandle>,
    key: String,
    config: SessionIndexConfig,
    kind: MaintenanceKind,
    completion: Option<mpsc::Sender<Result<(), String>>>,
}

enum QueueItem {
    Build(QueuedBuild),
    Maintenance(QueuedMaintenance),
}

struct RunningTask {
    key: String,
    directory: PathBuf,
    is_build: bool,
    cancelled: Option<Arc<AtomicBool>>,
}

struct BuildAttempt {
    finished_at: Instant,
    input_fingerprint: u64,
    cooldown: Duration,
}

struct BuildStats {
    updated: usize,
    failed: usize,
}

enum BuildOutcome {
    Completed(BuildStats),
    Cancelled,
}

#[derive(Default)]
struct BuildRegistry {
    queue: VecDeque<QueueItem>,
    queued_builds: HashSet<String>,
    queued_maintenance: HashSet<String>,
    running: Option<RunningTask>,
    worker_active: bool,
    last_attempts: HashMap<String, BuildAttempt>,
}

fn build_registry() -> &'static Mutex<BuildRegistry> {
    static REGISTRY: OnceLock<Mutex<BuildRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BuildRegistry::default()))
}

fn index_file_gate() -> &'static RwLock<()> {
    static GATE: OnceLock<RwLock<()>> = OnceLock::new();
    GATE.get_or_init(|| RwLock::new(()))
}

pub(crate) fn config(
    app: &AppHandle,
    settings: &AppSettings,
) -> Result<SessionIndexConfig, String> {
    let directory = if settings.session_index_directory.trim().is_empty() {
        app.path()
            .app_cache_dir()
            .map_err(|error| format!("获取应用缓存目录失败: {error}"))?
            .join("session-index")
    } else {
        PathBuf::from(settings.session_index_directory.trim()).join("session-index")
    };
    Ok(SessionIndexConfig {
        enabled: settings.session_index_enabled,
        directory,
        max_size_bytes: settings
            .session_index_max_size_mib
            .saturating_mul(1024 * 1024),
    })
}

pub(crate) fn search(
    cli_kind: AgentCliKind,
    workdir: &Path,
    sessions: &[CliSessionSummary],
    query: &SearchQuery,
    limit: usize,
    config: &SessionIndexConfig,
) -> Result<IndexedSearchOutcome, String> {
    if !config.enabled {
        return Ok(IndexedSearchOutcome {
            results: summary_search(sessions, query, limit),
            state: CliSessionIndexState::Disabled,
            message: Some("会话索引已在设置中关闭".to_string()),
        });
    }
    let _file_guard = index_file_gate()
        .read()
        .unwrap_or_else(|error| error.into_inner());
    let path = database_path(config, cli_kind);
    if !path.is_file() {
        return Ok(IndexedSearchOutcome {
            results: summary_search(sessions, query, limit),
            state: CliSessionIndexState::Building,
            message: Some("正在后台建立会话索引；已有摘要仍可立即查看".to_string()),
        });
    }
    let connection = open_connection(&path, false)?;
    let workspace = workspace_key(workdir);
    let indexed_ids = indexed_session_ids(&connection, &workspace)?;
    let mut indexed_content = if query.is_empty() {
        HashMap::new()
    } else {
        search_indexed_workspace(&connection, &workspace, &query.content_request())?
    };
    let mut results = Vec::new();
    for session in sessions {
        let mut matched = SearchAccumulator::new(query);
        observe_summary(&mut matched, session);
        if !matched.complete() && indexed_ids.contains(&session.id) {
            if let Some(content) = indexed_content.remove(&session.id) {
                matched.merge_content(content);
            }
        }
        if matched.complete() {
            results.push(CliSessionSearchResult {
                session: session.clone(),
            });
            if results.len() >= limit {
                break;
            }
        }
    }

    let fully_indexed = sessions
        .iter()
        .all(|session| indexed_ids.contains(&session.id));
    Ok(IndexedSearchOutcome {
        results,
        state: if fully_indexed {
            CliSessionIndexState::Ready
        } else {
            CliSessionIndexState::Building
        },
        message: (!fully_indexed)
            .then(|| "索引正在增量更新，结果会在后台完成后自动刷新".to_string()),
    })
}

fn summary_search(
    sessions: &[CliSessionSummary],
    query: &SearchQuery,
    limit: usize,
) -> Vec<CliSessionSearchResult> {
    let mut results = Vec::new();
    for session in sessions {
        let mut matched = SearchAccumulator::new(query);
        observe_summary(&mut matched, session);
        if matched.complete() {
            results.push(CliSessionSearchResult {
                session: session.clone(),
            });
            if results.len() >= limit {
                break;
            }
        }
    }
    results
}

pub(crate) fn schedule_build(app: AppHandle, request: BuildRequest) -> BuildScheduleState {
    if !request.config.enabled || !request.adapter.supports_index() {
        return BuildScheduleState::Skipped;
    }
    let key = build_key(
        request.cli_kind,
        &request.workdir,
        &request.config.directory,
    );
    let input_fingerprint = build_input_fingerprint(&request.sessions);
    let mut start_worker = false;
    {
        let mut registry = build_registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if registry.queued_builds.contains(&key)
            || registry
                .running
                .as_ref()
                .is_some_and(|running| running.key == key)
        {
            return BuildScheduleState::Active;
        }
        if registry.last_attempts.get(&key).is_some_and(|attempt| {
            attempt.input_fingerprint == input_fingerprint
                && attempt.finished_at.elapsed() < attempt.cooldown
        }) {
            return BuildScheduleState::CoolingDown;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        registry.queue.push_back(QueueItem::Build(QueuedBuild {
            app,
            key: key.clone(),
            input_fingerprint,
            request,
            cancelled,
        }));
        registry.queued_builds.insert(key);
        if !registry.worker_active {
            registry.worker_active = true;
            start_worker = true;
        }
    }
    if start_worker {
        start_queue_worker();
    }
    BuildScheduleState::Scheduled
}

fn start_queue_worker() {
    tauri::async_runtime::spawn(async {
        run_queue_worker().await;
    });
}

async fn run_queue_worker() {
    loop {
        let item = {
            let mut registry = build_registry()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(item) = registry.queue.pop_front() else {
                registry.worker_active = false;
                return;
            };
            item
        };
        match item {
            QueueItem::Build(item) => run_build_item(item).await,
            QueueItem::Maintenance(item) => run_maintenance_item(item).await,
        }
    }
}

async fn run_build_item(item: QueuedBuild) {
    let QueuedBuild {
        app,
        key,
        input_fingerprint,
        request,
        cancelled,
    } = item;
    {
        let mut registry = build_registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        registry.queued_builds.remove(&key);
        registry.running = Some(RunningTask {
            key: key.clone(),
            directory: request.config.directory.clone(),
            is_build: true,
            cancelled: Some(cancelled.clone()),
        });
    }

    let cli_kind = request.cli_kind;
    let task_id = format!("session-index-{}", cli_kind.key());
    let started_at = unix_millis() as u64;
    emit_task(
        &app,
        &task_id,
        "running",
        format!("准备索引 {} 个会话", request.sessions.len()),
        Some(0.0),
        started_at,
        None,
    );
    let task_app = app.clone();
    let task_request = request;
    let task_id_for_build = task_id.clone();
    let task_cancelled = cancelled.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        build(
            &task_app,
            &task_id_for_build,
            started_at,
            &task_request,
            &task_cancelled,
        )
    })
    .await
    .map_err(|error| format!("会话索引任务异常: {error}"))
    .and_then(|result| result);

    match result {
        Ok(BuildOutcome::Cancelled) => {
            emit_task(
                &app,
                &task_id,
                "success",
                "会话索引任务已取消",
                None,
                started_at,
                None,
            );
        }
        Ok(BuildOutcome::Completed(stats)) => {
            let cooldown = if stats.failed > 0 {
                FAILED_REFRESH_COOLDOWN
            } else {
                READY_REFRESH_COOLDOWN
            };
            build_registry()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .last_attempts
                .insert(
                    key.clone(),
                    BuildAttempt {
                        finished_at: Instant::now(),
                        input_fingerprint,
                        cooldown,
                    },
                );
            let detail = if stats.failed > 0 {
                format!(
                    "已更新 {} 个会话，跳过 {} 个暂不可读会话；稍后自动重试",
                    stats.updated, stats.failed
                )
            } else if stats.updated > 0 {
                format!("已更新 {} 个会话索引", stats.updated)
            } else {
                "会话索引已是最新".to_string()
            };
            emit_task(
                &app,
                &task_id,
                "success",
                detail,
                Some(1.0),
                started_at,
                None,
            );
            if stats.updated > 0 {
                let _ = app.emit(SESSION_INDEX_UPDATED_EVENT, cli_kind.key());
            }
        }
        Err(error) => {
            build_registry()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .last_attempts
                .insert(
                    key.clone(),
                    BuildAttempt {
                        finished_at: Instant::now(),
                        input_fingerprint,
                        cooldown: FAILED_REFRESH_COOLDOWN,
                    },
                );
            emit_task(
                &app,
                &task_id,
                "failed",
                "会话索引更新失败",
                None,
                started_at,
                Some(error),
            );
        }
    }
    let mut registry = build_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if registry
        .running
        .as_ref()
        .is_some_and(|running| running.key == key)
    {
        registry.running = None;
    }
}

async fn run_maintenance_item(item: QueuedMaintenance) {
    let QueuedMaintenance {
        app,
        key,
        config,
        kind,
        completion,
    } = item;
    let app_for_event = app.clone();
    let directory = config.directory.clone();
    let started_at = unix_millis() as u64;
    if let Some(app) = app_for_event.as_ref() {
        emit_task(
            app,
            &format!("session-index-maintenance-{}", stable_path_hash(&directory)),
            "running",
            match kind {
                MaintenanceKind::Clear => "正在清理旧的会话索引",
                MaintenanceKind::EnforceCapacity => "正在按容量上限整理会话索引",
            },
            None,
            started_at,
            None,
        );
    }
    {
        let mut registry = build_registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        registry.queued_maintenance.remove(&key);
        registry.running = Some(RunningTask {
            key: key.clone(),
            directory: config.directory.clone(),
            is_build: false,
            cancelled: None,
        });
    }
    let result = tauri::async_runtime::spawn_blocking(move || match kind {
        MaintenanceKind::Clear => {
            let _file_guard = index_file_gate()
                .write()
                .unwrap_or_else(|error| error.into_inner());
            clear_files(&config)
        }
        MaintenanceKind::EnforceCapacity => {
            let _file_guard = index_file_gate()
                .read()
                .unwrap_or_else(|error| error.into_inner());
            enforce_capacity(&config)
        }
    })
    .await
    .map_err(|error| format!("会话索引维护任务异常: {error}"))
    .and_then(|result| result);
    if let Some(sender) = completion {
        let _ = sender.send(result.clone());
    }
    if let Some(app) = app_for_event {
        let task_id = format!("session-index-maintenance-{}", stable_path_hash(&directory));
        match result {
            Ok(()) => emit_task(
                &app,
                &task_id,
                "success",
                "会话索引维护完成",
                Some(1.0),
                started_at,
                None,
            ),
            Err(error) => emit_task(
                &app,
                &task_id,
                "failed",
                "会话索引维护失败",
                None,
                started_at,
                Some(error),
            ),
        }
    }
    let mut registry = build_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if registry
        .running
        .as_ref()
        .is_some_and(|running| running.key == key)
    {
        registry.running = None;
    }
}

pub(crate) fn status(
    config: &SessionIndexConfig,
    max_size_mib: u64,
) -> Result<CliSessionIndexStatus, String> {
    let _file_guard = index_file_gate()
        .read()
        .unwrap_or_else(|error| error.into_inner());
    let mut agents = Vec::new();
    let mut size_bytes = 0u64;
    for &cli_kind in AgentCliKind::ALL {
        let path = database_path(config, cli_kind);
        let size = database_disk_size(&path);
        size_bytes = size_bytes.saturating_add(size);
        let (session_count, updated_at) = if path.is_file() {
            match open_connection(&path, false) {
                Ok(connection) => {
                    let count = connection
                        .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap_or_default()
                        .max(0) as usize;
                    let timestamp = connection
                        .query_row("SELECT MAX(indexed_at) FROM sessions", [], |row| {
                            row.get::<_, Option<String>>(0)
                        })
                        .unwrap_or(None);
                    (count, timestamp)
                }
                Err(_) => (0, None),
            }
        } else {
            (0, None)
        };
        agents.push(CliSessionIndexAgentStats {
            cli_kind,
            size_bytes: size,
            session_count,
            updated_at,
        });
    }
    let registry = build_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let building = registry
        .running
        .as_ref()
        .is_some_and(|running| running.is_build)
        || !registry.queued_builds.is_empty();
    Ok(CliSessionIndexStatus {
        enabled: config.enabled,
        directory: config.directory.to_string_lossy().to_string(),
        max_size_mib,
        size_bytes,
        building,
        agents,
    })
}

pub(crate) fn clear(config: &SessionIndexConfig) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    enqueue_clear(None, config.clone(), Some(sender));
    receiver
        .recv_timeout(MAINTENANCE_WAIT_TIMEOUT)
        .map_err(|_| "会话索引仍在停止，请稍后再试".to_string())?
}

fn enqueue_clear(
    app: Option<AppHandle>,
    config: SessionIndexConfig,
    completion: Option<mpsc::Sender<Result<(), String>>>,
) {
    let key = unique_maintenance_key("clear", &config.directory);
    let mut start_worker = false;
    {
        let mut registry = build_registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cancel_directory_locked(&mut registry, &config.directory);
        registry.queued_maintenance.insert(key.clone());
        registry
            .queue
            .push_front(QueueItem::Maintenance(QueuedMaintenance {
                app,
                key,
                config,
                kind: MaintenanceKind::Clear,
                completion,
            }));
        if !registry.worker_active {
            registry.worker_active = true;
            start_worker = true;
        }
    }
    if start_worker {
        start_queue_worker();
    }
}

pub(crate) fn reconfigure(app: &AppHandle, previous: &AppSettings, current: &AppSettings) {
    let Ok(previous_config) = config(app, previous) else {
        return;
    };
    let Ok(current_config) = config(app, current) else {
        return;
    };
    if previous_config.directory != current_config.directory {
        enqueue_clear(Some(app.clone()), previous_config.clone(), None);
    }
    if previous_config.directory == current_config.directory
        && current_config.enabled
        && previous_config.max_size_bytes != current_config.max_size_bytes
    {
        enqueue_capacity_maintenance(Some(app.clone()), current_config.clone());
    }
    if previous_config.enabled && !current_config.enabled {
        cancel_directory(&current_config.directory);
    }
}

fn enqueue_capacity_maintenance(app: Option<AppHandle>, config: SessionIndexConfig) {
    let key = maintenance_key("capacity", &config.directory);
    let mut start_worker = false;
    {
        let mut registry = build_registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !registry.queued_maintenance.insert(key.clone()) {
            return;
        }
        registry
            .queue
            .push_back(QueueItem::Maintenance(QueuedMaintenance {
                app,
                key,
                config,
                kind: MaintenanceKind::EnforceCapacity,
                completion: None,
            }));
        if !registry.worker_active {
            registry.worker_active = true;
            start_worker = true;
        }
    }
    if start_worker {
        start_queue_worker();
    }
}

fn cancel_directory(directory: &Path) {
    let mut registry = build_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cancel_directory_locked(&mut registry, directory);
}

fn cancel_directory_locked(registry: &mut BuildRegistry, directory: &Path) {
    let mut retained = VecDeque::with_capacity(registry.queue.len());
    while let Some(item) = registry.queue.pop_front() {
        match item {
            QueueItem::Build(build) if build.request.config.directory == directory => {
                build.cancelled.store(true, Ordering::Relaxed);
                registry.queued_builds.remove(&build.key);
            }
            other => retained.push_back(other),
        }
    }
    registry.queue = retained;
    if let Some(running) = registry.running.as_ref() {
        if running.directory == directory {
            if let Some(cancelled) = &running.cancelled {
                cancelled.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn clear_files(config: &SessionIndexConfig) -> Result<(), String> {
    if !config.directory.exists() {
        return Ok(());
    }
    for &cli_kind in AgentCliKind::ALL {
        let path = database_path(config, cli_kind);
        for candidate in [
            path.clone(),
            path.with_extension("sqlite3-wal"),
            path.with_extension("sqlite3-shm"),
        ] {
            if candidate.is_file() {
                fs::remove_file(&candidate).map_err(|error| {
                    format!("清理会话索引失败({}): {error}", candidate.display())
                })?;
            }
        }
    }
    let _ = fs::remove_dir(&config.directory);
    Ok(())
}

fn clear_database_files(config: &SessionIndexConfig, cli_kind: AgentCliKind) -> Result<(), String> {
    let path = database_path(config, cli_kind);
    for candidate in [
        path.clone(),
        path.with_extension("sqlite3-wal"),
        path.with_extension("sqlite3-shm"),
    ] {
        if candidate.is_file() {
            fs::remove_file(&candidate)
                .map_err(|error| format!("重建会话索引失败({}): {error}", candidate.display()))?;
        }
    }
    Ok(())
}

fn build(
    app: &AppHandle,
    task_id: &str,
    started_at: u64,
    request: &BuildRequest,
    cancelled: &AtomicBool,
) -> Result<BuildOutcome, String> {
    if request.reset_database {
        let _file_guard = index_file_gate()
            .write()
            .unwrap_or_else(|error| error.into_inner());
        build_inner(app, task_id, started_at, request, cancelled)
    } else {
        let _file_guard = index_file_gate()
            .read()
            .unwrap_or_else(|error| error.into_inner());
        build_inner(app, task_id, started_at, request, cancelled)
    }
}

fn build_inner(
    app: &AppHandle,
    task_id: &str,
    started_at: u64,
    request: &BuildRequest,
    cancelled: &AtomicBool,
) -> Result<BuildOutcome, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(BuildOutcome::Cancelled);
    }
    if request.reset_database {
        clear_database_files(&request.config, request.cli_kind)?;
    }
    fs::create_dir_all(&request.config.directory).map_err(|error| {
        format!(
            "创建会话索引目录失败({}): {error}",
            request.config.directory.display()
        )
    })?;
    let path = database_path(&request.config, request.cli_kind);
    let mut connection = open_connection(&path, true)?;
    let workspace = workspace_key(&request.workdir);
    let active_ids = request
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<HashSet<_>>();
    let existing = existing_fingerprints(&connection, &workspace)?;
    let total = request.sessions.len().max(1);
    let mut updated = 0usize;
    let mut failed = 0usize;
    for (position, session) in request.sessions.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(BuildOutcome::Cancelled);
        }
        let known = existing.get(&session.id).map(String::as_str);
        match request.adapter.index(
            request.cli_kind,
            &request.workdir,
            &session.id,
            known,
            &|| !cancelled.load(Ordering::Relaxed),
        ) {
            Ok(SessionIndexLoadResult::Unchanged { .. }) => {
                touch_session(&connection, &workspace, session)?;
            }
            Ok(SessionIndexLoadResult::Updated {
                fingerprint,
                source_bytes,
                messages,
            }) => {
                replace_session(
                    &mut connection,
                    &workspace,
                    session,
                    &fingerprint,
                    source_bytes,
                    messages,
                )?;
                updated = updated.saturating_add(1);
            }
            Err(error) if cancelled.load(Ordering::Relaxed) || is_cancelled_error(&error) => {
                return Ok(BuildOutcome::Cancelled);
            }
            Err(_) => {
                failed = failed.saturating_add(1);
                continue;
            }
        }
        if position % 3 == 0 || position + 1 == request.sessions.len() {
            emit_task(
                app,
                task_id,
                "running",
                format!(
                    "已处理 {} / {} 个会话",
                    position + 1,
                    request.sessions.len()
                ),
                Some((position + 1) as f32 / total as f32),
                started_at,
                None,
            );
            std::thread::yield_now();
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(BuildOutcome::Cancelled);
    }
    remove_missing_sessions(&connection, &workspace, &active_ids)?;
    enforce_capacity(&request.config)?;
    Ok(BuildOutcome::Completed(BuildStats { updated, failed }))
}

fn open_connection(path: &Path, create: bool) -> Result<Connection, String> {
    if create {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建会话索引目录失败({}): {error}", parent.display()))?;
        }
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_FULL_MUTEX
        | if create {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let mut connection = Connection::open_with_flags(path, flags)
        .map_err(|error| format!("打开会话索引失败({}): {error}", path.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_millis(750))
        .map_err(|error| format!("配置会话索引超时失败: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA temp_store=MEMORY;
             PRAGMA cache_size=-2048;",
        )
        .map_err(|error| format!("初始化会话索引连接失败: {error}"))?;
    if create {
        ensure_schema(&mut connection)?;
    }
    Ok(connection)
}

fn ensure_schema(connection: &mut Connection) -> Result<(), String> {
    let stored_version = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse::<i64>().ok());
    if stored_version.is_some() && stored_version != Some(INDEX_SCHEMA_VERSION) {
        connection
            .execute_batch(
                "DROP TABLE IF EXISTS message_fts;
                 DROP TABLE IF EXISTS messages;
                 DROP TABLE IF EXISTS sessions;
                 DROP TABLE IF EXISTS metadata;",
            )
            .map_err(|error| format!("重建过期会话索引失败: {error}"))?;
    }
    initialize_schema(connection)
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                 workspace TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 preview TEXT,
                 model TEXT,
                 models_json TEXT NOT NULL,
                 workdir TEXT NOT NULL,
                 created_at TEXT,
                 updated_at TEXT,
                 fingerprint TEXT NOT NULL,
                 source_bytes INTEGER NOT NULL,
                 indexed_bytes INTEGER NOT NULL,
                 indexed_at TEXT NOT NULL,
                 last_used_at INTEGER NOT NULL,
                 PRIMARY KEY (workspace, session_id)
             );
             CREATE TABLE IF NOT EXISTS messages (
                 row_id INTEGER PRIMARY KEY,
                 workspace TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 role TEXT NOT NULL,
                 chunk_index INTEGER NOT NULL,
                 content TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_session_idx
                 ON messages(workspace, session_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                 content,
                 tokenize='trigram',
                 content='',
                 contentless_delete=1
             );",
        )
        .map_err(|error| format!("创建会话索引结构失败: {error}"))?;
    connection
        .execute(
            "INSERT OR REPLACE INTO metadata(key, value) VALUES ('schema_version', ?1)",
            [INDEX_SCHEMA_VERSION.to_string()],
        )
        .map_err(|error| format!("写入会话索引版本失败: {error}"))?;
    Ok(())
}

fn replace_session(
    connection: &mut Connection,
    workspace: &str,
    session: &CliSessionSummary,
    fingerprint: &str,
    source_bytes: u64,
    messages: Vec<SessionIndexMessage>,
) -> Result<(), String> {
    let indexed_bytes = session
        .title
        .len()
        .saturating_add(session.preview.as_deref().map(str::len).unwrap_or_default())
        .saturating_add(session.model.as_deref().map(str::len).unwrap_or_default())
        .saturating_add(session.models.iter().map(String::len).sum::<usize>())
        .saturating_add(
            messages
                .iter()
                .map(|message| message.id.len().saturating_add(message.content.len()))
                .sum::<usize>(),
        );
    let transaction = connection
        .transaction()
        .map_err(|error| format!("开始会话索引事务失败: {error}"))?;
    let old_ids = {
        let mut statement = transaction
            .prepare("SELECT row_id FROM messages WHERE workspace=?1 AND session_id=?2")
            .map_err(|error| format!("读取旧会话索引失败: {error}"))?;
        let rows = statement
            .query_map(params![workspace, session.id], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("查询旧会话索引失败: {error}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };
    for row_id in old_ids {
        transaction
            .execute("DELETE FROM message_fts WHERE rowid=?1", [row_id])
            .map_err(|error| format!("删除旧全文索引失败: {error}"))?;
    }
    transaction
        .execute(
            "DELETE FROM messages WHERE workspace=?1 AND session_id=?2",
            params![workspace, session.id],
        )
        .map_err(|error| format!("删除旧会话消息失败: {error}"))?;
    transaction
        .execute(
            "INSERT OR REPLACE INTO sessions(
                 workspace, session_id, title, preview, model, models_json, workdir,
                 created_at, updated_at, fingerprint, source_bytes, indexed_bytes, indexed_at,
                 last_used_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                workspace,
                session.id,
                session.title,
                session.preview,
                session.model,
                serde_json::to_string(&session.models).unwrap_or_else(|_| "[]".to_string()),
                session.workdir,
                session.created_at,
                session.updated_at,
                fingerprint,
                source_bytes as i64,
                indexed_bytes as i64,
                chrono::Utc::now().to_rfc3339(),
                unix_millis() as i64,
            ],
        )
        .map_err(|error| format!("写入会话索引摘要失败: {error}"))?;
    for message in messages {
        for (chunk_index, content) in chunk_message(&message.content).into_iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO messages(workspace, session_id, message_id, role, chunk_index, content)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        workspace,
                        session.id,
                        message.id,
                        role_key(message.role),
                        chunk_index as i64,
                        content,
                    ],
                )
                .map_err(|error| format!("写入会话消息索引失败: {error}"))?;
            let row_id = transaction.last_insert_rowid();
            transaction
                .execute(
                    "INSERT INTO message_fts(rowid, content) VALUES (?1, ?2)",
                    params![row_id, content],
                )
                .map_err(|error| format!("写入全文索引失败: {error}"))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| format!("提交会话索引失败: {error}"))
}

fn search_indexed_workspace(
    connection: &Connection,
    workspace: &str,
    request: &crate::services::agent_cli::contracts::SessionContentSearchRequest,
) -> Result<
    HashMap<String, crate::services::agent_cli::contracts::SessionContentSearchResult>,
    String,
> {
    let mut collectors = HashMap::<String, SessionContentSearchCollector<'_>>::new();
    for term in &request.terms {
        let rows = if term.value.chars().count() >= 3 {
            let query = format!("\"{}\"", term.value.replace('"', "\"\""));
            let mut statement = connection
                .prepare(
                    "SELECT session_id, content
                     FROM (
                         SELECT m.session_id, m.content,
                                ROW_NUMBER() OVER (
                                    PARTITION BY m.session_id ORDER BY m.row_id
                                ) AS result_rank
                         FROM message_fts f
                         JOIN messages m ON m.row_id=f.rowid
                         WHERE m.workspace=?1 AND message_fts MATCH ?2
                     )
                     WHERE result_rank = 1",
                )
                .map_err(|error| format!("准备工作区会话全文检索失败: {error}"))?;
            let rows = statement
                .query_map(params![workspace, query], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| format!("检索工作区会话全文失败: {error}"))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            rows
        } else {
            let mut statement = connection
                .prepare(
                    "SELECT session_id, content
                     FROM (
                         SELECT m.session_id, m.content,
                                ROW_NUMBER() OVER (
                                    PARTITION BY m.session_id ORDER BY m.row_id
                                ) AS result_rank
                         FROM messages m
                         WHERE m.workspace=?1 AND instr(lower(m.content), ?2) > 0
                     )
                     WHERE result_rank = 1",
                )
                .map_err(|error| format!("准备工作区短词检索失败: {error}"))?;
            let rows = statement
                .query_map(params![workspace, term.value], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| format!("检索工作区短词内容失败: {error}"))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            rows
        };
        for (session_id, content) in rows {
            collectors
                .entry(session_id)
                .or_insert_with(|| SessionContentSearchCollector::new(request))
                .observe(&content);
        }
    }
    Ok(collectors
        .into_iter()
        .map(|(session_id, collector)| (session_id, collector.finish()))
        .collect())
}

#[cfg(test)]
fn search_indexed_messages(
    connection: &Connection,
    workspace: &str,
    session_id: &str,
    request: &crate::services::agent_cli::contracts::SessionContentSearchRequest,
) -> Result<crate::services::agent_cli::contracts::SessionContentSearchResult, String> {
    Ok(search_indexed_workspace(connection, workspace, request)?
        .remove(session_id)
        .unwrap_or_default())
}

fn observe_summary(accumulator: &mut SearchAccumulator<'_>, session: &CliSessionSummary) {
    accumulator.observe(&session.title);
    accumulator.observe(&session.id);
    if let Some(preview) = session.preview.as_deref() {
        accumulator.observe(preview);
    }
    if let Some(model) = session.model.as_deref() {
        accumulator.observe(model);
    }
    for model in &session.models {
        accumulator.observe(model);
    }
    accumulator.observe(&session.workdir);
}

fn existing_fingerprints(
    connection: &Connection,
    workspace: &str,
) -> Result<HashMap<String, String>, String> {
    let mut statement = connection
        .prepare("SELECT session_id, fingerprint FROM sessions WHERE workspace=?1")
        .map_err(|error| format!("准备读取索引指纹失败: {error}"))?;
    let rows = statement
        .query_map([workspace], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("读取索引指纹失败: {error}"))?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn indexed_session_ids(
    connection: &Connection,
    workspace: &str,
) -> Result<HashSet<String>, String> {
    Ok(existing_fingerprints(connection, workspace)?
        .into_keys()
        .collect())
}

fn touch_session(
    connection: &Connection,
    workspace: &str,
    session: &CliSessionSummary,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE sessions SET last_used_at=?1, title=?2, preview=?3, model=?4,
             models_json=?5, updated_at=?6 WHERE workspace=?7 AND session_id=?8",
            params![
                unix_millis() as i64,
                session.title,
                session.preview,
                session.model,
                serde_json::to_string(&session.models).unwrap_or_else(|_| "[]".to_string()),
                session.updated_at,
                workspace,
                session.id,
            ],
        )
        .map_err(|error| format!("更新会话索引摘要失败: {error}"))?;
    Ok(())
}

fn remove_missing_sessions(
    connection: &Connection,
    workspace: &str,
    active_ids: &HashSet<String>,
) -> Result<(), String> {
    let existing = indexed_session_ids(connection, workspace)?;
    for session_id in existing.difference(active_ids) {
        delete_session(connection, workspace, session_id)?;
    }
    Ok(())
}

fn delete_session(
    connection: &Connection,
    workspace: &str,
    session_id: &str,
) -> Result<(), String> {
    let row_ids = {
        let mut statement = connection
            .prepare("SELECT row_id FROM messages WHERE workspace=?1 AND session_id=?2")
            .map_err(|error| format!("读取待删除会话索引失败: {error}"))?;
        let rows = statement
            .query_map(params![workspace, session_id], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("查询待删除会话索引失败: {error}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };
    for row_id in row_ids {
        connection
            .execute("DELETE FROM message_fts WHERE rowid=?1", [row_id])
            .map_err(|error| format!("删除全文索引记录失败: {error}"))?;
    }
    connection
        .execute(
            "DELETE FROM messages WHERE workspace=?1 AND session_id=?2",
            params![workspace, session_id],
        )
        .map_err(|error| format!("删除会话索引消息失败: {error}"))?;
    connection
        .execute(
            "DELETE FROM sessions WHERE workspace=?1 AND session_id=?2",
            params![workspace, session_id],
        )
        .map_err(|error| format!("删除会话索引摘要失败: {error}"))?;
    Ok(())
}

fn enforce_capacity(config: &SessionIndexConfig) -> Result<(), String> {
    if config.max_size_bytes == 0 {
        return Ok(());
    }
    let mut measured_total = total_disk_size(config);
    if measured_total <= config.max_size_bytes {
        return Ok(());
    }
    let target = config.max_size_bytes;
    let mut candidates = Vec::new();
    for &cli_kind in AgentCliKind::ALL {
        let path = database_path(config, cli_kind);
        if !path.is_file() {
            continue;
        }
        let connection = open_connection(&path, false)?;
        let mut statement = connection
            .prepare(
                "SELECT workspace, session_id, last_used_at
                 FROM sessions ORDER BY last_used_at ASC",
            )
            .map_err(|error| format!("准备索引淘汰查询失败: {error}"))?;
        candidates.extend(
            statement
                .query_map([], |row| {
                    Ok((
                        cli_kind,
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(|error| format!("读取索引淘汰候选失败: {error}"))?
                .filter_map(Result::ok),
        );
    }
    candidates.sort_by_key(|candidate| candidate.3);
    let mut next_candidate = 0usize;
    let mut batch_size = 1usize;
    while measured_total > target && next_candidate < candidates.len() {
        let previous_total = measured_total;
        let batch_start = next_candidate;
        let batch_end = next_candidate
            .saturating_add(batch_size)
            .min(candidates.len());
        let mut touched = HashSet::new();
        let mut connections = HashMap::new();
        while next_candidate < batch_end {
            let (cli_kind, workspace, session_id, _) = &candidates[next_candidate];
            if !connections.contains_key(cli_kind) {
                connections.insert(
                    *cli_kind,
                    open_connection(&database_path(config, *cli_kind), false)?,
                );
            }
            let connection = connections
                .get(cli_kind)
                .ok_or_else(|| "会话索引容量整理连接丢失".to_string())?;
            delete_session(connection, workspace, session_id)?;
            touched.insert(*cli_kind);
            next_candidate += 1;
        }
        drop(connections);
        compact_databases(config, &touched)?;
        measured_total = total_disk_size(config);
        let deleted_count = next_candidate.saturating_sub(batch_start);
        let measured_freed = previous_total.saturating_sub(measured_total);
        batch_size = if measured_freed == 0 || deleted_count == 0 {
            1
        } else {
            let average_freed = measured_freed / deleted_count as u64;
            let remaining = measured_total.saturating_sub(target);
            remaining
                .saturating_add(average_freed.saturating_sub(1))
                .checked_div(average_freed.max(1))
                .unwrap_or(1)
                .clamp(1, MAX_CAPACITY_EVICTION_BATCH as u64) as usize
        };
    }

    if measured_total > config.max_size_bytes {
        remove_empty_databases(config)?;
    }
    Ok(())
}

fn compact_databases(
    config: &SessionIndexConfig,
    cli_kinds: &HashSet<AgentCliKind>,
) -> Result<(), String> {
    for &cli_kind in cli_kinds {
        let path = database_path(config, cli_kind);
        if path.is_file() {
            let connection = open_connection(&path, false)?;
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
                .map_err(|error| format!("压缩会话索引失败({}): {error}", path.display()))?;
        }
    }
    Ok(())
}

fn remove_empty_databases(config: &SessionIndexConfig) -> Result<(), String> {
    for &cli_kind in AgentCliKind::ALL {
        let path = database_path(config, cli_kind);
        if !path.is_file() {
            continue;
        }
        let connection = open_connection(&path, false)?;
        let session_count = connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| format!("读取空会话索引失败({}): {error}", path.display()))?;
        drop(connection);
        if session_count == 0 {
            clear_database_files(config, cli_kind)?;
        }
    }
    Ok(())
}

fn chunk_message(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start_byte = 0usize;
    while start_byte < content.len() {
        let remaining = &content[start_byte..];
        let end_byte = start_byte
            + remaining
                .char_indices()
                .nth(MESSAGE_CHUNK_CHARS)
                .map(|(index, _)| index)
                .unwrap_or(remaining.len());
        let chunk = &content[start_byte..end_byte];
        chunks.push(chunk);
        if end_byte == content.len() {
            break;
        }
        let overlap_start = chunk
            .char_indices()
            .rev()
            .nth(MESSAGE_CHUNK_OVERLAP_CHARS.saturating_sub(1))
            .map(|(index, _)| index)
            .unwrap_or_default();
        let next_start_byte = start_byte.saturating_add(overlap_start);
        if next_start_byte <= start_byte {
            break;
        }
        start_byte = next_start_byte;
    }
    chunks
}

fn workspace_key(path: &Path) -> String {
    let mut value = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        value.make_ascii_lowercase();
    }
    value
}

fn build_key(cli_kind: AgentCliKind, workdir: &Path, directory: &Path) -> String {
    format!(
        "{}:{}:{}",
        cli_kind.key(),
        stable_path_hash(directory),
        workspace_key(workdir)
    )
}

fn build_input_fingerprint(sessions: &[CliSessionSummary]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for session in sessions {
        session.id.hash(&mut hasher);
        session.updated_at.hash(&mut hasher);
        session.title.hash(&mut hasher);
        session.preview.hash(&mut hasher);
        session.model.hash(&mut hasher);
    }
    hasher.finish()
}

fn maintenance_key(kind: &str, directory: &Path) -> String {
    format!("{kind}:{}", stable_path_hash(directory))
}

fn unique_maintenance_key(kind: &str, directory: &Path) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}:{}",
        maintenance_key(kind, directory),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn stable_path_hash(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    workspace_key(path).hash(&mut hasher);
    hasher.finish()
}

fn is_cancelled_error(error: &str) -> bool {
    error.contains("会话检索已被新的搜索替换")
}

fn database_path(config: &SessionIndexConfig, cli_kind: AgentCliKind) -> PathBuf {
    config.directory.join(format!("{}.sqlite3", cli_kind.key()))
}

fn database_disk_size(path: &Path) -> u64 {
    [
        path.to_path_buf(),
        path.with_extension("sqlite3-wal"),
        path.with_extension("sqlite3-shm"),
    ]
    .into_iter()
    .filter_map(|candidate| candidate.metadata().ok().map(|metadata| metadata.len()))
    .sum()
}

fn total_disk_size(config: &SessionIndexConfig) -> u64 {
    AgentCliKind::ALL
        .iter()
        .map(|&cli_kind| database_disk_size(&database_path(config, cli_kind)))
        .sum()
}

fn role_key(role: CliSessionMessageRole) -> &'static str {
    match role {
        CliSessionMessageRole::User => "user",
        CliSessionMessageRole::Assistant => "assistant",
        CliSessionMessageRole::Tool => "tool",
    }
}

fn emit_task(
    app: &AppHandle,
    task_id: &str,
    status: &str,
    detail: impl Into<String>,
    progress: Option<f32>,
    started_at: u64,
    error: Option<String>,
) {
    let _ = app.emit(
        BACKGROUND_TASK_EVENT,
        BackgroundTaskEvent {
            task_id: task_id.to_string(),
            kind: INDEX_TASK_KIND.to_string(),
            status: status.to_string(),
            title: "更新会话索引".to_string(),
            detail: detail.into(),
            progress,
            started_at,
            finished_at: (status != "running").then(|| unix_millis() as u64),
            error,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CliSessionMessageRole;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-session-index-{name}-{}",
            std::process::id()
        ))
    }

    fn summary(id: &str, workdir: &Path) -> CliSessionSummary {
        CliSessionSummary {
            id: id.to_string(),
            title: "会话索引测试".to_string(),
            preview: Some("只保存可见正文".to_string()),
            model: Some("model-test".to_string()),
            models: vec!["model-test".to_string()],
            cli_kind: AgentCliKind::Codex,
            created_at: None,
            updated_at: Some("2026-08-19T08:00:00Z".to_string()),
            workdir: workdir.to_string_lossy().to_string(),
            cli_version: None,
            archived: false,
            can_resume: true,
            metadata_source: "test".to_string(),
        }
    }

    #[test]
    fn trigram_index_finds_chinese_substrings_and_short_terms() {
        let root = test_root("search");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let database = root.join("codex.sqlite3");
        let mut connection = open_connection(&database, true).unwrap();
        let workdir = root.join("workspace");
        fs::create_dir_all(&workdir).unwrap();
        let session = summary("session-1", &workdir);
        replace_session(
            &mut connection,
            &workspace_key(&workdir),
            &session,
            "fingerprint",
            128,
            vec![SessionIndexMessage {
                id: "message-1".to_string(),
                role: CliSessionMessageRole::Assistant,
                content: "BalanceHub 会话全文检索已经完成".to_string(),
            }],
        )
        .unwrap();

        for term in ["全文检索", "会话"] {
            let request = crate::services::agent_cli::contracts::SessionContentSearchRequest {
                terms: vec![crate::services::agent_cli::contracts::SessionSearchTerm {
                    index: 0,
                    value: term.to_string(),
                }],
            };
            let result = search_indexed_messages(
                &connection,
                &workspace_key(&workdir),
                &session.id,
                &request,
            )
            .unwrap();
            assert_eq!(result.matched_term_indexes, vec![0]);
        }
        connection
            .close()
            .map_err(|(_, error)| error)
            .expect("the SQLite test connection should close before cleanup");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn message_chunking_preserves_cross_boundary_search_text() {
        let content = format!(
            "{}跨块关键字{}",
            "x".repeat(MESSAGE_CHUNK_CHARS - 4),
            "y".repeat(20)
        );
        let chunks = chunk_message(&content);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().any(|chunk| chunk.contains("跨块关键字")));
    }

    #[test]
    fn message_chunking_uses_utf8_boundaries() {
        let content = format!("{}尾部", "会".repeat(MESSAGE_CHUNK_CHARS + 8));
        let chunks = chunk_message(&content);
        assert_eq!(chunks[0].chars().count(), MESSAGE_CHUNK_CHARS);
        assert!(chunks[1].starts_with('会'));
        assert!(chunks[1].ends_with("尾部"));
    }

    #[test]
    fn capacity_is_shared_across_agent_databases() {
        let root = test_root("capacity");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let config = SessionIndexConfig {
            enabled: true,
            directory: root.clone(),
            max_size_bytes: 1,
        };
        for cli_kind in [AgentCliKind::Codex, AgentCliKind::ClaudeCode] {
            let database = database_path(&config, cli_kind);
            let mut connection = open_connection(&database, true).unwrap();
            let workdir = root.join(cli_kind.key());
            fs::create_dir_all(&workdir).unwrap();
            let mut session = summary(cli_kind.key(), &workdir);
            session.cli_kind = cli_kind;
            replace_session(
                &mut connection,
                &workspace_key(&workdir),
                &session,
                "fingerprint",
                64,
                vec![SessionIndexMessage {
                    id: "message".to_string(),
                    role: CliSessionMessageRole::User,
                    content: "需要被容量约束管理的正文".repeat(200),
                }],
            )
            .unwrap();
        }
        enforce_capacity(&config).unwrap();
        let remaining = AgentCliKind::ALL
            .iter()
            .filter_map(|&kind| {
                let path = database_path(&config, kind);
                path.is_file().then(|| {
                    open_connection(&path, false)
                        .unwrap()
                        .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap_or_default()
                })
            })
            .sum::<i64>();
        assert_eq!(remaining, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_capacity_keeps_valid_indexes_when_partial_eviction_is_enough() {
        let root = test_root("capacity-partial");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut config = SessionIndexConfig {
            enabled: true,
            directory: root.clone(),
            max_size_bytes: u64::MAX,
        };

        for &cli_kind in AgentCliKind::ALL {
            let database = database_path(&config, cli_kind);
            let mut connection = open_connection(&database, true).unwrap();
            let workdir = root.join(cli_kind.key());
            fs::create_dir_all(&workdir).unwrap();
            let mut session = summary(cli_kind.key(), &workdir);
            session.cli_kind = cli_kind;
            let content = (0..2_000)
                .map(|index| format!("{}-{index:04x}", cli_kind.key()))
                .collect::<Vec<_>>()
                .join(" ");
            replace_session(
                &mut connection,
                &workspace_key(&workdir),
                &session,
                "fingerprint",
                content.len() as u64,
                vec![SessionIndexMessage {
                    id: "message".to_string(),
                    role: CliSessionMessageRole::Assistant,
                    content,
                }],
            )
            .unwrap();
        }

        let total_before = total_disk_size(&config);
        let largest_database = AgentCliKind::ALL
            .iter()
            .map(|&kind| database_disk_size(&database_path(&config, kind)))
            .max()
            .unwrap();
        config.max_size_bytes = total_before.saturating_sub(largest_database / 3);

        enforce_capacity(&config).unwrap();

        let remaining = AgentCliKind::ALL
            .iter()
            .filter_map(|&kind| {
                let path = database_path(&config, kind);
                path.is_file().then(|| {
                    open_connection(&path, false)
                        .unwrap()
                        .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap_or_default()
                })
            })
            .sum::<i64>();
        assert!(remaining > 0, "partial pressure must not erase every index");
        assert!(remaining < AgentCliKind::ALL.len() as i64);
        assert!(total_disk_size(&config) <= config.max_size_bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelling_a_directory_marks_its_running_build_only() {
        let target = test_root("cancel-target");
        let other = test_root("cancel-other");
        let target_cancelled = Arc::new(AtomicBool::new(false));
        let mut registry = BuildRegistry {
            running: Some(RunningTask {
                key: "target".to_string(),
                directory: target.clone(),
                is_build: true,
                cancelled: Some(target_cancelled.clone()),
            }),
            ..BuildRegistry::default()
        };
        cancel_directory_locked(&mut registry, &target);
        assert!(target_cancelled.load(Ordering::Relaxed));

        let other_cancelled = Arc::new(AtomicBool::new(false));
        registry.running = Some(RunningTask {
            key: "other".to_string(),
            directory: other,
            is_build: true,
            cancelled: Some(other_cancelled.clone()),
        });
        cancel_directory_locked(&mut registry, &target);
        assert!(!other_cancelled.load(Ordering::Relaxed));
    }
}
