use super::{clean_text, first_non_empty, timestamp_from_value};
use crate::models::{CliSessionMetadataSource, CliSessionSummary, LivenessCliKind};
use rusqlite::{Connection, OpenFlags, Row};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const STATE_DB_PREFIX: &str = "state_";
const SESSION_INDEX_FILE: &str = "session_index.jsonl";

pub(super) fn list(workdir: &Path, limit: usize) -> Result<Vec<CliSessionSummary>, String> {
    let codex_home = codex_home()?;
    // The official resume picker keeps explicit session names separately from
    // the SQLite thread metadata. Read it independently so a missing or stale
    // index never prevents the database-backed history from loading.
    let session_titles = read_session_titles(&codex_home).unwrap_or_default();
    let mut databases = fs::read_dir(&codex_home)
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
    if databases.is_empty() {
        return Err(format!("未找到 Codex 状态数据库：{}", codex_home.display()));
    }

    let workdir = workdir.to_string_lossy().to_string();
    let canonical_workdir = Path::new(&workdir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&workdir))
        .to_string_lossy()
        .to_string();
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let mut last_error = None;
    let mut readable_database = false;
    for database in databases {
        match read_database(&database, &workdir, &canonical_workdir, limit) {
            Ok(items) => {
                readable_database = true;
                for item in items {
                    if seen.insert(item.id.clone()) {
                        sessions.push(item);
                    }
                }
            }
            Err(err) => last_error = Some(err),
        }
    }
    if !readable_database {
        if let Some(err) = last_error {
            return Err(err);
        }
    }
    for session in &mut sessions {
        if let Some(title) = session_titles
            .get(&session.id)
            .and_then(|value| clean_text(value, 100))
        {
            session.title = title;
        }
    }
    Ok(sessions)
}

fn read_session_titles(codex_home: &Path) -> Result<HashMap<String, String>, String> {
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

fn read_database(
    path: &Path,
    workdir: &str,
    canonical_workdir: &str,
    limit: usize,
) -> Result<Vec<CliSessionSummary>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| format!("打开 Codex 状态数据库失败：{}：{err}", path.display()))?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(|err| format!("设置 Codex 状态数据库只读模式失败：{err}"))?;

    let columns = connection
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map_err(|err| format!("读取 Codex 会话表结构失败：{err}"))?
        .into_iter()
        .collect::<HashSet<_>>();
    if !columns.contains("id") || !columns.contains("cwd") {
        return Err(format!(
            "Codex 状态数据库缺少 threads 会话索引：{}",
            path.display()
        ));
    }

    let optional = |name: &str| {
        if columns.contains(name) {
            name.to_string()
        } else {
            format!("NULL AS {name}")
        }
    };
    let created_column = if columns.contains("created_at_ms") {
        "created_at_ms"
    } else if columns.contains("created_at") {
        "created_at"
    } else {
        "NULL"
    };
    let updated_column = if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else if columns.contains("updated_at") {
        "updated_at"
    } else {
        "NULL"
    };
    let created_milliseconds = created_column == "created_at_ms";
    let updated_milliseconds = updated_column == "updated_at_ms";
    let order_column = if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else if columns.contains("updated_at") {
        "updated_at"
    } else if columns.contains("created_at_ms") {
        "created_at_ms"
    } else if columns.contains("created_at") {
        "created_at"
    } else {
        "rowid"
    };
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
        order_column = order_column,
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|err| format!("读取 Codex 会话索引失败：{err}"))?;
    let rows = statement
        .query_map((workdir, canonical_workdir, limit as i64), |row| {
            row_to_summary(row, created_milliseconds, updated_milliseconds)
        })
        .map_err(|err| format!("查询 Codex 历史会话失败：{err}"))?;
    rows.map(|row| row.map_err(|err| format!("解析 Codex 历史会话失败：{err}")))
        .collect()
}

fn row_to_summary(
    row: &Row<'_>,
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
        cli_kind: LivenessCliKind::Codex,
        created_at: created,
        updated_at: updated,
        workdir,
        cli_version: cli_version.and_then(|value| clean_text(value, 50)),
        archived,
        can_resume: !archived,
        metadata_source: CliSessionMetadataSource::CodexStateDb,
    })
}

fn codex_home() -> Result<PathBuf, String> {
    crate::services::cli_paths::codex_home()
        .ok_or_else(|| "无法定位用户目录，无法读取 Codex 历史会话".to_string())
}

#[cfg(test)]
mod tests {
    use super::{codex_home, read_database, read_session_titles};
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn default_home_uses_user_home() {
        assert!(codex_home().is_ok());
    }

    #[test]
    fn state_database_maps_title_preview_model_and_dates() {
        let path = std::env::temp_dir().join(format!(
            "balancehub-cli-session-codex-test-{}.sqlite",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (id TEXT, cwd TEXT, name TEXT, title TEXT, preview TEXT, first_user_message TEXT, model TEXT, created_at_ms INTEGER, updated_at_ms INTEGER, archived INTEGER, cli_version TEXT); INSERT INTO threads VALUES ('session-1', '/tmp/project', '', '显式标题', '摘要', '首条请求', 'gpt-5.5', 1786000000000, 1786000005000, 0, '0.146.0');",
            )
            .unwrap();
        drop(connection);
        let sessions = read_database(&path, "/tmp/project", "/tmp/project", 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "显式标题");
        assert_eq!(sessions[0].preview.as_deref(), Some("摘要"));
        assert_eq!(sessions[0].model.as_deref(), Some("gpt-5.5"));
        assert!(sessions[0].created_at.is_some());
        assert_eq!(sessions[0].cli_version.as_deref(), Some("0.146.0"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn official_session_name_overrides_database_title() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-cli-session-index-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("session_index.jsonl"),
            concat!(
                "{\"id\":\"session-1\",\"thread_name\":\"BalanceHub\"}\n",
                "not-json\n",
                "{\"id\":\"session-2\",\"thread_name\":\"  \"}\n",
                "{\"session_id\":\"session-3\",\"name\":\"命名会话\"}\n",
            ),
        )
        .unwrap();

        let titles = read_session_titles(&directory).unwrap();
        assert_eq!(
            titles.get("session-1").map(String::as_str),
            Some("BalanceHub")
        );
        assert_eq!(
            titles.get("session-3").map(String::as_str),
            Some("命名会话")
        );
        assert!(!titles.contains_key("session-2"));
        fs::remove_dir_all(directory).unwrap();
    }
}
