use crate::{
    models::{AgentCliKind, CliSessionSummary},
    services::cli_sessions::{clean_text, first_non_empty, timestamp_from_value},
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const STATE_DB_PREFIX: &str = "state_";
const SESSION_INDEX_FILE: &str = "session_index.jsonl";

pub(super) struct CodexSessionRecord {
    pub(super) summary: CliSessionSummary,
    pub(super) rollout_path: String,
}

pub(super) fn state_databases(codex_home: &Path) -> Result<Vec<PathBuf>, String> {
    let mut databases = Vec::new();
    let entries =
        fs::read_dir(codex_home).map_err(|error| format!("读取 Codex 状态目录失败：{error}"))?;
    for entry in entries {
        crate::services::cli_sessions::workbench::check_read_budget()?;
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(STATE_DB_PREFIX) && name.ends_with(".sqlite"))
        {
            databases.push(path);
        }
    }
    databases.sort_by(|left, right| right.cmp(left));
    Ok(databases)
}

pub(super) fn read_session_titles(codex_home: &Path) -> Result<HashMap<String, String>, String> {
    let path = codex_home.join(SESSION_INDEX_FILE);
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let mut titles = HashMap::new();
    crate::services::cli_sessions::scan_json_records(
        &path,
        "读取 Codex 会话命名索引",
        &|| true,
        |_sequence, line| {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                return false;
            };
            if let (Some(id), Some(title)) = (
                value
                    .get("id")
                    .or_else(|| value.get("session_id"))
                    .and_then(Value::as_str),
                value
                    .get("thread_name")
                    .or_else(|| value.get("name"))
                    .and_then(Value::as_str),
            ) {
                if !id.trim().is_empty() && !title.trim().is_empty() {
                    titles.insert(id.trim().into(), title.trim().into());
                }
            }
            false
        },
    )?;
    Ok(titles)
}

pub(super) fn read_session_title(
    codex_home: &Path,
    session_id: &str,
    max_bytes: usize,
) -> Option<String> {
    let path = codex_home.join(SESSION_INDEX_FILE);
    let file = crate::services::cli_sessions::workbench::open_session_file(path).ok()?;
    let mut consumed = 0usize;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        consumed = consumed.saturating_add(line.len());
        if consumed > max_bytes {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = value
            .get("id")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            continue;
        };
        if id != session_id {
            continue;
        }
        return value
            .get("thread_name")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string);
    }
    None
}

/// Streams all selected native workspace rows from one open database. SQL has
/// no history-count LIMIT; request cancellation interrupts both SQLite and rows.
pub(super) fn read_database_history(
    cli_kind: AgentCliKind,
    path: &Path,
    workdirs: &[PathBuf],
    budget: &crate::services::agent_cli::contracts::SessionReadBudget,
    mut observe: impl FnMut(CodexSessionRecord, Option<String>, Option<String>),
) -> Result<(), String> {
    let connection = open_database(path)?;
    let interrupt = connection.get_interrupt_handle();
    let (stop, receiver) = std::sync::mpsc::channel::<()>();
    let watched = budget.clone();
    let watcher = std::thread::spawn(move || {
        while receiver.recv_timeout(std::time::Duration::from_millis(20))
            == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        {
            if watched.check().is_err() {
                interrupt.interrupt();
                break;
            }
        }
    });
    let result = (|| {
        let columns = thread_columns(&connection)?;
        if !columns.contains("id") || !columns.contains("cwd") {
            return Err("Codex 数据库缺少原生 threads 索引".into());
        }
        let optional = |name: &str| optional_column(&columns, name);
        let created = timestamp_column(&columns, "created_at_ms", "created_at");
        let updated = timestamp_column(&columns, "updated_at_ms", "updated_at");
        let mut paths = Vec::new();
        for workdir in workdirs {
            paths.push(workdir.to_string_lossy().to_string());
            if let Ok(canonical) = workdir.canonicalize() {
                paths.push(canonical.to_string_lossy().to_string());
            }
        }
        paths.sort();
        paths.dedup();
        // Batch parameters avoid SQLite's variable limit without changing the
        // requested directory set or re-opening its state database per project.
        for paths in paths.chunks(200) {
            budget.check()?;
            let placeholders = vec!["?"; paths.len()].join(",");
            let sql = format!("SELECT id, cwd, {name}, {title}, {preview}, {first_user_message}, {model}, {created}, {updated}, {archived}, {cli_version}, {rollout_path}, {source}, {parent_thread_id} FROM threads WHERE cwd IN ({placeholders}) ORDER BY {updated} DESC, id ASC", name=optional("name"), title=optional("title"), preview=optional("preview"), first_user_message=optional("first_user_message"), model=optional("model"), archived=optional("archived"), cli_version=optional("cli_version"), rollout_path=optional("rollout_path"), source=optional("source"), parent_thread_id=optional("parent_thread_id"));
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| error.to_string())?;
            let mut rows = statement
                .query(rusqlite::params_from_iter(paths))
                .map_err(|error| error.to_string())?;
            while let Some(row) = rows.next().map_err(|error| error.to_string())? {
                budget.check()?;
                let record = CodexSessionRecord {
                    summary: row_to_summary(
                        row,
                        cli_kind,
                        created == "created_at_ms",
                        updated == "updated_at_ms",
                    )
                    .map_err(|error| error.to_string())?,
                    rollout_path: row
                        .get::<_, Option<String>>(11)
                        .map_err(|error| error.to_string())?
                        .unwrap_or_default(),
                };
                observe(
                    record,
                    row.get(12).map_err(|error| error.to_string())?,
                    row.get(13).map_err(|error| error.to_string())?,
                );
            }
        }
        Ok(())
    })();
    let _ = stop.send(());
    let _ = watcher.join();
    result
}

pub(super) fn read_database_session(
    cli_kind: AgentCliKind,
    path: &Path,
    workdir: &str,
    canonical_workdir: &str,
    session_id: &str,
) -> Result<Option<CodexSessionRecord>, String> {
    let connection = open_database(path)?;
    let columns = thread_columns(&connection)?;
    if !columns.contains("id") || !columns.contains("cwd") {
        return Ok(None);
    }

    let optional = |name: &str| optional_column(&columns, name);
    let created_column = timestamp_column(&columns, "created_at_ms", "created_at");
    let updated_column = timestamp_column(&columns, "updated_at_ms", "updated_at");
    let sql = format!(
        "SELECT id, cwd, {name}, {title}, {preview}, {first_user_message}, {model}, {created}, {updated}, {archived}, {cli_version}, {rollout_path} FROM threads WHERE id = ?1 AND (cwd = ?2 OR cwd = ?3) LIMIT 1",
        name = optional("name"),
        title = optional("title"),
        preview = optional("preview"),
        first_user_message = optional("first_user_message"),
        model = optional("model"),
        created = created_column,
        updated = updated_column,
        archived = optional("archived"),
        cli_version = optional("cli_version"),
        rollout_path = optional("rollout_path"),
    );
    connection
        .query_row(&sql, (session_id, workdir, canonical_workdir), |row| {
            Ok(CodexSessionRecord {
                summary: row_to_summary(
                    row,
                    cli_kind,
                    created_column == "created_at_ms",
                    updated_column == "updated_at_ms",
                )?,
                rollout_path: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
            })
        })
        .optional()
        .map_err(|err| format!("查询 Codex 会话详情失败：{err}"))
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| format!("打开 Codex 状态数据库失败：{}：{err}", path.display()))?;
    connection
        .busy_timeout(std::time::Duration::from_millis(100))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|err| format!("设置 Codex 状态数据库只读模式失败：{err}"))?;
    Ok(connection)
}

fn thread_columns(connection: &Connection) -> Result<HashSet<String>, String> {
    connection
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map(|columns| columns.into_iter().collect())
        .map_err(|err| format!("读取 Codex 会话表结构失败：{err}"))
}

fn optional_column(columns: &HashSet<String>, name: &str) -> String {
    if columns.contains(name) {
        name.to_string()
    } else {
        format!("NULL AS {name}")
    }
}

fn timestamp_column<'a>(
    columns: &HashSet<String>,
    milliseconds: &'a str,
    seconds: &'a str,
) -> &'a str {
    if columns.contains(milliseconds) {
        milliseconds
    } else if columns.contains(seconds) {
        seconds
    } else {
        "NULL"
    }
}

fn row_to_summary(
    row: &Row<'_>,
    cli_kind: AgentCliKind,
    created_milliseconds: bool,
    updated_milliseconds: bool,
) -> rusqlite::Result<CliSessionSummary> {
    let id: String = row.get(0)?;
    let workdir: String = row.get(1)?;
    let name: Option<String> = row.get(2)?;
    let title: Option<String> = row.get(3)?;
    let preview: Option<String> = row.get(4)?;
    let first_user_message: Option<String> = row.get(5)?;
    let model: Option<String> = row.get(6)?;
    let created = timestamp_from_value(row.get(7)?, created_milliseconds);
    let updated = timestamp_from_value(row.get(8)?, updated_milliseconds);
    let archived = row.get::<_, Option<i64>>(9)?.unwrap_or_default() != 0;
    let cli_version: Option<String> = row.get(10)?;
    let preview = preview.and_then(|value| clean_text(value, 240));
    let title = first_non_empty([
        name.and_then(|value| clean_text(value, 100)),
        title.and_then(|value| clean_text(value, 100)),
        preview.clone().and_then(|value| clean_text(value, 100)),
        first_user_message.and_then(|value| clean_text(value, 100)),
    ]);
    let model = model.and_then(|value| clean_text(value, 120));
    let models = model.clone().into_iter().collect();
    Ok(CliSessionSummary {
        id,
        title,
        preview,
        model,
        models,
        cli_kind,
        created_at: created,
        updated_at: updated,
        workdir,
        cli_version: cli_version.and_then(|value| clean_text(value, 50)),
        archived,
        can_resume: !archived,
        metadata_source: "codexStateDb".to_string(),
    })
}
