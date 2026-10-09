//! Derived native-session full-text index. Database gates are keyed by the
//! actual Agent library; transcript scanning never holds any database gate.
use super::SessionContentSearchCollector;
use crate::{
    models::{
        AgentCliKind, AppSettings, CliSessionIndexAgentStats, CliSessionIndexState,
        CliSessionIndexStatus, CliSessionMessageRole,
    },
    services::agent_cli::contracts::{
        SessionContentSearchRequest, SessionContentSearchResult, SessionHistoryAdapter,
        SessionHistoryRecord, SessionReadBudget,
    },
    util::unix_millis,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const SESSION_INDEX_UPDATED_EVENT: &str = "cli-session-index-updated";
mod records;
#[cfg(test)]
pub(super) mod test_support;
use super::record_stream as stream;

const INDEX_SCHEMA_VERSION: i64 = 5;
const MESSAGE_CHUNK_CHARS: usize = 32 * 1024;
const MESSAGE_CHUNK_OVERLAP_CHARS: usize = super::MAX_SEARCH_QUERY_CHARS;
const INDEX_SQL_BUSY_STEP: Duration = Duration::from_millis(25);

#[derive(Clone, Copy)]
struct IndexBudget<'a> {
    read: &'a SessionReadBudget,
    invalidated: &'a Arc<AtomicBool>,
}
impl IndexBudget<'_> {
    fn check(self) -> Result<(), String> {
        self.read.check()?;
        if self.invalidated.load(Ordering::Acquire) {
            return Err("会话索引设置已变化，当前读取已取消".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionIndexConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub max_size_bytes: u64,
}
struct ActiveIndex {
    directory: PathBuf,
    cancelled: Arc<AtomicBool>,
    readers: Arc<AtomicU64>,
}
#[derive(Default)]
struct IndexRegistry {
    gates: HashMap<PathBuf, Arc<Mutex<()>>>,
    active: HashMap<u64, ActiveIndex>,
}
fn registry() -> &'static Mutex<IndexRegistry> {
    static REGISTRY: OnceLock<Mutex<IndexRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(IndexRegistry::default()))
}
fn database_gate(path: &Path) -> Arc<Mutex<()>> {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .gates
        .entry(path.to_path_buf())
        .or_default()
        .clone()
}
fn cancel_directory(directory: &Path) {
    for active in registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .active
        .values()
    {
        if active.directory == directory {
            active.cancelled.store(true, Ordering::Release);
        }
    }
}

pub(crate) struct HistorySearchResult {
    pub content: SessionContentSearchResult,
    pub complete: bool,
    pub indexed_bytes: u64,
    pub source_bytes: u64,
    pub skipped_records: u64,
}

pub(crate) struct HistoryIndex {
    memory: Mutex<Option<Connection>>,
    memory_only: AtomicBool,
    config: SessionIndexConfig,
    path: PathBuf,
    token: u64,
    cancelled: Arc<AtomicBool>,
    readers: Arc<AtomicU64>,
}
struct IndexReadGuard<'a>(&'a AtomicU64);
impl Drop for IndexReadGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}
impl HistoryIndex {
    pub(crate) fn new(config: &SessionIndexConfig, kind: AgentCliKind, source_id: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let readers = Arc::new(AtomicU64::new(0));
        registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .insert(
                token,
                ActiveIndex {
                    directory: config.directory.clone(),
                    cancelled: cancelled.clone(),
                    readers: readers.clone(),
                },
            );
        Self {
            memory: Mutex::new(None),
            memory_only: AtomicBool::new(!config.enabled),
            config: config.clone(),
            path: source_database_path(config, kind, source_id),
            token,
            cancelled,
            readers,
        }
    }
    pub(crate) fn state(&self) -> CliSessionIndexState {
        if !self.config.enabled {
            CliSessionIndexState::Disabled
        } else if self.memory_only.load(Ordering::Acquire) {
            CliSessionIndexState::Fallback
        } else {
            CliSessionIndexState::Ready
        }
    }
    fn check(&self, budget: &SessionReadBudget) -> Result<(), String> {
        IndexBudget {
            read: budget,
            invalidated: &self.cancelled,
        }
        .check()
    }
    fn connection<T>(
        &self,
        budget: &SessionReadBudget,
        apply: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.memory_only.load(Ordering::Acquire) {
            let mut slot = loop {
                self.check(budget)?;
                match self.memory.try_lock() {
                    Ok(guard) => break guard,
                    Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                }
            };
            if slot.is_none() {
                let connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
                initialize_schema(&connection)?;
                *slot = Some(connection);
            }
            let connection = slot.as_mut().ok_or("会话临时索引未就绪")?;
            let read = budget.clone();
            let invalidated = self.cancelled.clone();
            connection.progress_handler(
                1000,
                Some(move || read.check().is_err() || invalidated.load(Ordering::Acquire)),
            );
            let result = apply(connection);
            self.check(budget)?;
            return result;
        }
        let gate = database_gate(&self.path);
        let _guard = loop {
            self.check(budget)?;
            match gate.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
            }
        };
        self.check(budget)?;
        let context = IndexBudget {
            read: budget,
            invalidated: &self.cancelled,
        };
        let mut connection = match open_connection_with_budget(&self.path, true, Some(context)) {
            Ok(connection) => connection,
            Err(error) => {
                context.check()?;
                // A read-only or unavailable cache directory must not turn
                // large-file search into repeated full scans. Keep a temporary
                // index for the lifetime of this query instead.
                self.memory_only.store(true, Ordering::Release);
                eprintln!("会话索引缓存暂不可用，本次查询使用临时索引：{error}");
                drop(_guard);
                return self.connection(budget, apply);
            }
        };
        let result = apply(&mut connection);
        context.check()?;
        result
    }
    pub(crate) fn search(
        &self,
        workspace: &str,
        record: &SessionHistoryRecord,
        adapter: &SessionHistoryAdapter,
        request: &SessionContentSearchRequest,
        budget: &SessionReadBudget,
    ) -> Result<HistorySearchResult, String> {
        self.readers.fetch_add(1, Ordering::AcqRel);
        let _guard = IndexReadGuard(&self.readers);
        let sources = (adapter.index)(record)?;
        let source_keys = sources
            .iter()
            .map(|source| source.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        self.connection(budget, |connection| {
            records::reconcile_sources(connection, workspace, &record.record_key, &source_keys)
        })?;
        let mut result = HistorySearchResult {
            content: SessionContentSearchResult::default(),
            complete: true,
            indexed_bytes: 0,
            source_bytes: 0,
            skipped_records: 0,
        };
        for (source, key) in sources.iter().zip(&source_keys) {
            self.check(budget)?;
            let known: Option<String> = self.connection(budget, |connection| {
                connection.query_row(
                    "SELECT checkpoint FROM session_sources WHERE workspace=?1 AND session_id=?2 AND source_key=?3",
                    params![workspace, record.record_key, key], |row| row.get(0),
                ).optional().map_err(|error| error.to_string())
            })?;
            let previous = known
                .as_deref()
                .and_then(|value| serde_json::from_str::<stream::Checkpoint>(value).ok());
            let batch =
                stream::read_batch(source, previous.as_ref(), &|| self.check(budget).is_ok())?;
            let changed = previous.as_ref() != Some(&batch.checkpoint) || batch.reset;
            if changed
                && !self.connection(budget, |connection| {
                    records::apply_batch(
                        connection,
                        workspace,
                        record,
                        key,
                        known.as_deref(),
                        &batch,
                    )
                })?
            {
                result.complete = false;
            }
            result.complete &= batch.checkpoint.complete;
            result.indexed_bytes += batch.checkpoint.offset;
            result.source_bytes += batch.checkpoint.source_bytes;
            result.skipped_records += batch.checkpoint.skipped_records;
        }
        if result.complete {
            result.content = self.connection(budget, |connection| {
                connection.execute("UPDATE sessions SET last_used_at=?3, source_bytes=?4 WHERE workspace=?1 AND session_id=?2",
                    params![workspace, record.record_key, unix_millis() as i64, result.source_bytes as i64])
                    .map_err(|error| error.to_string())?;
                search_indexed_messages(connection, workspace, &record.record_key, request)
            })?;
        }
        Ok(result)
    }
    pub(crate) fn finish(&self, budget: &SessionReadBudget) -> Result<(), String> {
        if self.memory_only.load(Ordering::Acquire) {
            return Ok(());
        }
        enforce_capacity(
            &self.config,
            IndexBudget {
                read: budget,
                invalidated: &self.cancelled,
            },
        )
    }
}
impl Drop for HistoryIndex {
    fn drop(&mut self) {
        registry()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .remove(&self.token);
    }
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

pub(crate) fn status(
    config: &SessionIndexConfig,
    max_size_mib: u64,
) -> Result<CliSessionIndexStatus, String> {
    let files = database_files(config);
    let mut agents = Vec::new();
    for &cli_kind in AgentCliKind::ALL {
        let mut size_bytes = 0;
        let mut session_count = 0;
        let mut updated_at: Option<String> = None;
        for path in files.iter().filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(cli_kind.key()))
        }) {
            size_bytes += database_disk_size(path);
            let gate = database_gate(path);
            let Ok(_guard) = gate.try_lock() else {
                continue;
            };
            if let Ok(connection) = open_connection(path, false) {
                session_count += connection
                    .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap_or_default()
                    .max(0) as usize;
                if let Ok(Some(timestamp)) =
                    connection.query_row("SELECT MAX(indexed_at) FROM sessions", [], |row| {
                        row.get::<_, Option<String>>(0)
                    })
                {
                    if updated_at
                        .as_ref()
                        .is_none_or(|current| current < &timestamp)
                    {
                        updated_at = Some(timestamp);
                    }
                }
            }
        }
        agents.push(CliSessionIndexAgentStats {
            cli_kind,
            size_bytes,
            session_count,
            updated_at,
        });
    }
    let building = registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .active
        .values()
        .any(|active| {
            active.directory == config.directory
                && !active.cancelled.load(Ordering::Acquire)
                && active.readers.load(Ordering::Acquire) > 0
        });
    Ok(CliSessionIndexStatus {
        enabled: config.enabled,
        directory: config.directory.to_string_lossy().to_string(),
        max_size_mib,
        size_bytes: agents.iter().map(|agent| agent.size_bytes).sum(),
        building,
        agents,
    })
}

pub(crate) fn clear(config: &SessionIndexConfig) -> Result<(), String> {
    cancel_directory(&config.directory);
    for path in database_files(config) {
        let gate = database_gate(&path);
        let _guard = gate.lock().unwrap_or_else(|error| error.into_inner());
        remove_database_files(&path)?;
    }
    Ok(())
}
pub(crate) fn reconfigure(app: &AppHandle, previous: &AppSettings, current: &AppSettings) {
    let (Ok(old), Ok(new)) = (config(app, previous), config(app, current)) else {
        return;
    };
    if previous.session_index_enabled == current.session_index_enabled
        && old.directory == new.directory
        && old.max_size_bytes == new.max_size_bytes
    {
        return;
    }
    cancel_directory(&old.directory);
    let app = app.clone();
    std::thread::spawn(move || {
        let result = if old.directory != new.directory || !new.enabled {
            clear(&old)
        } else {
            let budget = SessionReadBudget::new(
                Instant::now() + Duration::from_secs(30),
                Arc::new(AtomicBool::new(false)),
                u64::MAX,
            );
            enforce_capacity(
                &new,
                IndexBudget {
                    read: &budget,
                    invalidated: &budget.cancelled,
                },
            )
        };
        if let Err(error) = result {
            eprintln!("会话派生索引维护未完成：{error}");
        }
        let _ = app.emit(SESSION_INDEX_UPDATED_EVENT, ());
    });
}
fn source_database_path(config: &SessionIndexConfig, kind: AgentCliKind, source: &str) -> PathBuf {
    config.directory.join(format!(
        "{}-{}.sqlite3",
        kind.key(),
        super::workbench::hash(&[source])
    ))
}
fn database_files(config: &SessionIndexConfig) -> Vec<PathBuf> {
    database_files_with_budget(config, None).unwrap_or_default()
}
fn database_files_with_budget(
    config: &SessionIndexConfig,
    budget: Option<IndexBudget<'_>>,
) -> Result<Vec<PathBuf>, String> {
    if let Some(budget) = budget {
        budget.check()?;
    }
    let Ok(entries) = fs::read_dir(&config.directory) else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    for entry in entries {
        if let Some(budget) = budget {
            budget.check()?;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_ok_and(|ty| ty.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "sqlite3")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    AgentCliKind::ALL.iter().any(|kind| {
                        name == format!("{}.sqlite3", kind.key())
                            || name.starts_with(&format!("{}-", kind.key()))
                    })
                })
        {
            files.push(path);
        }
    }
    Ok(files)
}
fn database_disk_size(path: &Path) -> u64 {
    [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ]
    .iter()
    .map(|file| {
        fs::metadata(file)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    })
    .sum()
}
fn total_disk_size_with_budget(
    config: &SessionIndexConfig,
    budget: Option<IndexBudget<'_>>,
) -> Result<u64, String> {
    let mut total = 0u64;
    for path in database_files_with_budget(config, budget)? {
        if let Some(budget) = budget {
            budget.check()?;
        }
        total = total.saturating_add(database_disk_size(&path));
    }
    Ok(total)
}
fn remove_database_files(path: &Path) -> Result<(), String> {
    remove_database_files_with_budget(path, None)
}
fn remove_database_files_with_budget(
    path: &Path,
    budget: Option<IndexBudget<'_>>,
) -> Result<(), String> {
    for file in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if let Some(budget) = budget {
            budget.check()?;
        }
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
// Capacity is shared by all libraries, not multiplied by installation or root.
// Maintenance keeps one request budget across all libraries. Select a global
// oldest-first batch, then delete each library's batch in one transaction and
// compact it at most once. A later request can continue interrupted upkeep.
fn enforce_capacity(config: &SessionIndexConfig, budget: IndexBudget<'_>) -> Result<(), String> {
    budget.check()?;
    let total = total_disk_size_with_budget(config, Some(budget))?;
    if config.max_size_bytes == 0 || total <= config.max_size_bytes {
        return Ok(());
    }
    let maintenance = database_gate(&config.directory);
    let _maintenance = match maintenance.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err("其他查询正在维护会话索引容量，可稍后刷新；已读会话仍保留".into());
        }
    };
    let mut candidates = BinaryHeap::new();
    let mut sizes = HashMap::new();
    let mut indexed_totals = HashMap::new();
    let mut reclaimed = 0u64;
    for path in database_files_with_budget(config, Some(budget))? {
        budget.check()?;
        let gate = database_gate(&path);
        let Ok(_guard) = gate.try_lock() else {
            continue;
        };
        let size = database_disk_size(&path);
        let connection = open_connection_with_budget(&path, false, Some(budget))?;
        let mut indexed_total = 0u64;
        {
            let mut statement = connection
                .prepare("SELECT workspace, session_id, last_used_at, indexed_bytes FROM sessions WHERE NOT EXISTS (
                    SELECT 1 FROM session_sources s WHERE s.workspace=sessions.workspace AND s.session_id=sessions.session_id
                    AND json_extract(s.checkpoint, '$.complete') != 1
                )")
                .map_err(|error| error.to_string())?;
            let mut rows = statement.query([]).map_err(|error| error.to_string())?;
            while let Some(row) = rows.next().map_err(|error| error.to_string())? {
                budget.check()?;
                let workspace = row.get::<_, String>(0).map_err(|error| error.to_string())?;
                let session = row.get::<_, String>(1).map_err(|error| error.to_string())?;
                let time = row.get::<_, i64>(2).map_err(|error| error.to_string())?;
                let bytes = row
                    .get::<_, i64>(3)
                    .map_err(|error| error.to_string())?
                    .max(1) as u64;
                budget.read.record(
                    workspace
                        .len()
                        .saturating_add(session.len())
                        .saturating_add(path.as_os_str().len())
                        .saturating_add(std::mem::size_of::<(i64, u64)>()),
                )?;
                indexed_total = indexed_total.saturating_add(bytes);
                candidates.push(Reverse((time, path.clone(), workspace, session, bytes)));
            }
        }
        budget.check()?;
        let has_sessions: bool = connection
            .query_row("SELECT EXISTS(SELECT 1 FROM sessions)", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())?;
        drop(connection);
        if !has_sessions {
            // A previous bounded pass may have committed deletion before its
            // compaction budget ended. Empty derived libraries need no VACUUM.
            remove_database_files_with_budget(&path, Some(budget))?;
            reclaimed = reclaimed.saturating_add(size);
        } else {
            sizes.insert(path.clone(), size);
            indexed_totals.insert(path, indexed_total);
        }
    }
    let mut needed = total
        .saturating_sub(reclaimed)
        .saturating_sub(config.max_size_bytes);
    let mut batches = BTreeMap::<PathBuf, Vec<(String, String)>>::new();
    while needed > 0 {
        budget.check()?;
        let Some(Reverse((_, path, workspace, session, bytes))) = candidates.pop() else {
            break;
        };
        // indexed_bytes estimates each record's share of its library. Actual
        // post-compaction size is checked below; underestimation is reported
        // as deferred maintenance, never as an unlimited extra VACUUM loop.
        let estimated = (u128::from(sizes[&path]) * u128::from(bytes)
            / u128::from(indexed_totals[&path]))
        .max(1)
        .min(u128::from(u64::MAX)) as u64;
        needed = needed.saturating_sub(estimated);
        batches.entry(path).or_default().push((workspace, session));
    }
    for (path, sessions) in batches {
        budget.check()?;
        let gate = database_gate(&path);
        let Ok(_guard) = gate.try_lock() else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let mut connection = open_connection_with_budget(&path, false, Some(budget))?;
        delete_session_batch(&mut connection, &sessions, budget)?;
        budget.check()?;
        let remaining = connection
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|error| error.to_string())?;
        if remaining == 0 {
            drop(connection);
            remove_database_files_with_budget(&path, Some(budget))?;
        } else {
            budget.check()?;
            let compacted = connection.execute_batch(
                "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
            );
            budget.check()?;
            compacted.map_err(|error| format!("压缩会话索引失败：{error}"))?;
        }
    }
    budget.check()?;
    if total_disk_size_with_budget(config, Some(budget))? > config.max_size_bytes {
        return Err("会话索引容量维护尚未完成，后续查询将继续处理；已读会话仍保留".into());
    }
    Ok(())
}
fn open_connection(path: &Path, create: bool) -> Result<Connection, String> {
    open_connection_with_budget(path, create, None)
}
fn open_connection_with_budget(
    path: &Path,
    create: bool,
    budget: Option<IndexBudget<'_>>,
) -> Result<Connection, String> {
    if let Some(budget) = budget {
        budget.check()?;
    }
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
    if let Some(budget) = budget {
        budget.check()?;
        let read = budget.read.clone();
        let invalidated = budget.invalidated.clone();
        connection.progress_handler(
            512,
            Some(move || read.check().is_err() || invalidated.load(Ordering::Acquire)),
        );
    }
    connection
        .busy_timeout(budget.map_or(Duration::from_millis(750), |budget| {
            INDEX_SQL_BUSY_STEP.min(
                budget
                    .read
                    .deadline
                    .saturating_duration_since(Instant::now()),
            )
        }))
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
    if let Some(budget) = budget {
        budget.check()?;
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
                 DROP TABLE IF EXISTS session_sources;
                 DROP TABLE IF EXISTS message_positions;
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
                 content TEXT NOT NULL,
                 source_key TEXT NOT NULL DEFAULT '',
                 priority INTEGER NOT NULL DEFAULT 1,
                 message_order INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS session_sources (
                 workspace TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 source_key TEXT NOT NULL,
                 checkpoint TEXT NOT NULL,
                 PRIMARY KEY(workspace,session_id,source_key)
             );
             CREATE TABLE IF NOT EXISTS message_positions (
                 workspace TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 source_key TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 message_order INTEGER NOT NULL,
                 PRIMARY KEY(workspace,session_id,source_key,message_id)
             );
             CREATE INDEX IF NOT EXISTS messages_source_idx
                 ON messages(workspace,session_id,source_key,message_id,chunk_index);
             CREATE INDEX IF NOT EXISTS messages_session_idx
                 ON messages(workspace, session_id);
             CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                 content,
                 tokenize='trigram',
                 content='',
                 contentless_delete=1
             );
             CREATE TRIGGER IF NOT EXISTS messages_insert AFTER INSERT ON messages BEGIN
                 INSERT INTO message_fts(rowid,content) VALUES (new.row_id,new.content);
             END;
             CREATE TRIGGER IF NOT EXISTS messages_delete AFTER DELETE ON messages BEGIN
                 DELETE FROM message_fts WHERE rowid=old.row_id;
             END;",
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

fn search_indexed_messages(
    connection: &Connection,
    workspace: &str,
    session_id: &str,
    request: &SessionContentSearchRequest,
) -> Result<SessionContentSearchResult, String> {
    let mut collector = SessionContentSearchCollector::new(request);
    for term in &request.terms {
        super::workbench::check_read_budget()?;
        let content = if term.value.chars().count() >= 3 {
            let query = format!("\"{}\"", term.value.replace('"', "\"\""));
            connection.query_row("SELECT m.content FROM message_fts f JOIN messages m ON m.row_id=f.rowid WHERE m.workspace=?1 AND m.session_id=?2 AND m.priority=(SELECT MAX(priority) FROM messages WHERE workspace=?1 AND session_id=?2) AND message_fts MATCH ?3 LIMIT 1", params![workspace, session_id, query], |row| row.get::<_, String>(0)).optional()
        } else {
            connection.query_row("SELECT content FROM messages WHERE workspace=?1 AND session_id=?2 AND priority=(SELECT MAX(priority) FROM messages WHERE workspace=?1 AND session_id=?2) AND instr(lower(content), ?3)>0 LIMIT 1", params![workspace, session_id, term.value], |row| row.get::<_, String>(0)).optional()
        }.map_err(|error| format!("检索原生会话索引失败：{error}"))?;
        if let Some(content) = content {
            collector.observe(&content);
        }
    }
    Ok(collector.finish())
}

fn delete_session_batch(
    connection: &mut Connection,
    sessions: &[(String, String)],
    budget: IndexBudget<'_>,
) -> Result<(), String> {
    budget.check()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "CREATE TEMP TABLE capacity_evictions (
             workspace TEXT NOT NULL, session_id TEXT NOT NULL,
             PRIMARY KEY(workspace, session_id)
         ) WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
    {
        let mut insert = transaction
            .prepare("INSERT INTO capacity_evictions VALUES (?1, ?2)")
            .map_err(|error| error.to_string())?;
        for (workspace, session_id) in sessions {
            budget.check()?;
            insert
                .execute(params![workspace, session_id])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction
        .execute_batch(
            "DELETE FROM session_sources WHERE (workspace, session_id) IN (
             SELECT workspace, session_id FROM capacity_evictions
         );
         DELETE FROM message_positions WHERE (workspace, session_id) IN (
             SELECT workspace, session_id FROM capacity_evictions
         );
         DELETE FROM messages WHERE (workspace, session_id) IN (
             SELECT workspace, session_id FROM capacity_evictions
         );
         DELETE FROM sessions WHERE (workspace, session_id) IN (
             SELECT workspace, session_id FROM capacity_evictions
         );",
        )
        .map_err(|error| format!("批量删除会话索引失败：{error}"))?;
    budget.check()?;
    transaction.commit().map_err(|error| error.to_string())
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

fn role_key(role: CliSessionMessageRole) -> &'static str {
    match role {
        CliSessionMessageRole::User => "user",
        CliSessionMessageRole::Assistant => "assistant",
        CliSessionMessageRole::Tool => "tool",
    }
}

#[cfg(test)]
mod tests;
