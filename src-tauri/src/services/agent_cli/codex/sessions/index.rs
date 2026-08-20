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
const MAX_ROLLOUT_FALLBACK_DIRECTORIES: usize = 2_000;

pub(super) struct CodexSessionRecord {
    pub(super) summary: CliSessionSummary,
    pub(super) rollout_path: String,
}

pub(super) fn state_databases(codex_home: &Path) -> Result<Vec<PathBuf>, String> {
    let mut databases = fs::read_dir(codex_home)
        .map_err(|err| format!("读取 Codex 状态目录失败：{}：{err}", codex_home.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(STATE_DB_PREFIX) && name.ends_with(".sqlite")
                    })
        })
        .collect::<Vec<_>>();
    databases.sort_by(|left, right| right.cmp(left));
    Ok(databases)
}

pub(super) fn read_session_titles(
    codex_home: &Path,
) -> Result<HashMap<String, String>, String> {
    let path = codex_home.join(SESSION_INDEX_FILE);
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let file = fs::File::open(&path)
        .map_err(|err| format!("打开 Codex 会话命名索引失败：{}：{err}", path.display()))?;
    let mut titles = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = value
            .get("id")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let Some(title) = value
            .get("thread_name")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            continue;
        };
        titles.insert(id.to_string(), title.to_string());
    }
    Ok(titles)
}

pub(super) fn read_database(
    cli_kind: AgentCliKind,
    path: &Path,
    workdir: &str,
    canonical_workdir: &str,
    limit: usize,
) -> Result<Vec<CliSessionSummary>, String> {
    let connection = open_database(path)?;
    let columns = thread_columns(&connection)?;
    if !columns.contains("id") || !columns.contains("cwd") {
        return Err(format!(
            "Codex 状态数据库缺少 threads 会话索引：{}",
            path.display()
        ));
    }

    let optional = |name: &str| optional_column(&columns, name);
    let created_column = timestamp_column(&columns, "created_at_ms", "created_at");
    let updated_column = timestamp_column(&columns, "updated_at_ms", "updated_at");
    let order_column = ["updated_at_ms", "updated_at", "created_at_ms", "created_at"]
        .into_iter()
        .find(|column| columns.contains(*column))
        .unwrap_or("rowid");
    let sql = format!(
        "SELECT id, cwd, {name}, {title}, {preview}, {first_user_message}, {model}, {created}, {updated}, {archived}, {cli_version} FROM threads WHERE cwd = ?1 OR cwd = ?2 ORDER BY {order_column} DESC LIMIT ?3",
        name = optional("name"),
        title = optional("title"),
        preview = optional("preview"),
        first_user_message = optional("first_user_message"),
        model = optional("model"),
        created = created_column,
        updated = updated_column,
        archived = optional("archived"),
        cli_version = optional("cli_version"),
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| format!("读取 Codex 会话索引失败：{err}"))?;
    let rows = statement
        .query_map((workdir, canonical_workdir, limit as i64), |row| {
            row_to_summary(
                row,
                cli_kind,
                created_column == "created_at_ms",
                updated_column == "updated_at_ms",
            )
        })
        .map_err(|err| format!("查询 Codex 历史会话失败：{err}"))?;
    rows.map(|row| row.map_err(|err| format!("解析 Codex 历史会话失败：{err}")))
        .collect()
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
        .query_row(
            &sql,
            (session_id, workdir, canonical_workdir),
            |row| {
                Ok(CodexSessionRecord {
                    summary: row_to_summary(
                        row,
                        cli_kind,
                        created_column == "created_at_ms",
                        updated_column == "updated_at_ms",
                    )?,
                    rollout_path: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                })
            },
        )
        .optional()
        .map_err(|err| format!("查询 Codex 会话详情失败：{err}"))
}

pub(super) fn resolve_rollout_path(codex_home: &Path, indexed_path: &str) -> Option<PathBuf> {
    let indexed = PathBuf::from(indexed_path.trim());
    if indexed.is_file() {
        return Some(indexed);
    }
    let file_name = indexed.file_name()?;
    for root in [
        codex_home.join("archived_sessions"),
        codex_home.join("sessions"),
    ] {
        let direct = root.join(file_name);
        if direct.is_file() {
            return Some(direct);
        }
        if let Some(found) = find_rollout_by_name(&root, file_name) {
            return Some(found);
        }
    }
    None
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| format!("打开 Codex 状态数据库失败：{}：{err}", path.display()))?;
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

fn find_rollout_by_name(root: &Path, file_name: &std::ffi::OsStr) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let mut directories = vec![root.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(directory) = directories.pop() {
        if scanned >= MAX_ROLLOUT_FALLBACK_DIRECTORIES {
            break;
        }
        scanned += 1;
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_file() && path.file_name() == Some(file_name) {
                return Some(path);
            }
            if file_type.is_dir()
                && directories.len().saturating_add(scanned)
                    < MAX_ROLLOUT_FALLBACK_DIRECTORIES
            {
                directories.push(path);
            }
        }
    }
    None
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
