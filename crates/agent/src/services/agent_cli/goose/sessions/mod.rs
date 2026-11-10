//! Goose SQLite session history.
//!
//! Goose keeps `<data>/sessions/sessions.db` with one row per Session and one
//! row per Message. Message `content_json` is an MCP-shaped content-block
//! array, decoded by the shared SQLite transcript reader.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource, SqliteTranscript,
    },
    services::cli_sessions::{clean_text, first_non_empty, normalize_timestamp},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};

const PARSER_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SESSIONS: usize = 100_000;

const TRANSCRIPT_QUERY: &str = "SELECT rowid AS row_key, json_object(\
    'id', message_id, 'row_id', id, 'role', role, 'content', content_json, \
    'timestamp', created_timestamp, 'metadata', metadata_json\
) AS payload FROM messages \
WHERE session_id = ?3 AND rowid > ?1 ORDER BY rowid LIMIT ?2";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Goose 原生目录必须是绝对路径");
    }
    let _ = root;
    let mut environment = EnvironmentPatch::default();
    // Goose reads `GOOSE_PATH_ROOT` itself, so only carry an explicit root
    // through; the platform default has no matching synthetic root.
    if let Some(root) = std::env::var_os("GOOSE_PATH_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        environment.set_path("GOOSE_PATH_ROOT", &root);
    }
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let path = config::sessions_db_path().ok_or_else(|| "无法定位 Goose 数据目录".to_string())?;
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Goose 会话目录无效".to_string())?;
    let (launch_environment, resume_reason) = match launch_environment(&root) {
        Ok(environment) => (environment, None),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment,
        resume_reason,
    })
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let path = root.join("sessions.db");
    if !path.is_file() {
        return Ok(result);
    }
    if let Err(error) = scan_database(kind, &path, workdirs, budget, mode, &mut result) {
        result.failed(error);
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Goose 会话数据库不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: false,
        sqlite: Some(SqliteTranscript {
            query: TRANSCRIPT_QUERY,
            scope: record.record_key.clone(),
        }),
        decode: decoder::decode,
    }])
}

fn scan_database(
    kind: AgentCliKind,
    path: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    result: &mut SessionHistoryScan,
) -> Result<(), String> {
    let connection = open(path)?;
    let columns = table_columns(&connection, "sessions")?;
    if !columns.contains("id") || !columns.contains("working_dir") {
        return Err("Goose 会话数据库缺少 sessions 表".into());
    }
    let column = |name: &str| {
        if columns.contains(name) {
            name.to_owned()
        } else {
            format!("NULL AS {name}")
        }
    };
    let previews = if matches!(mode, SessionHistoryReadMode::Summaries) {
        read_previews(&connection, budget).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let sql = format!(
        "SELECT id, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM sessions ORDER BY updated_at DESC, id ASC",
        column("name"),
        column("description"),
        column("working_dir"),
        column("created_at"),
        column("updated_at"),
        column("provider_name"),
        column("model_config_json"),
        column("archived_at"),
        column("parent_session_id"),
    );
    let mut statement = connection.prepare(&sql).map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut read = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("Goose 会话数量超过上限，已跳过部分记录");
            return Ok(());
        }
        read += 1;
        let id: String = row.get(0).map_err(|error| error.to_string())?;
        let name: Option<String> = row.get(1).ok();
        let description: Option<String> = row.get(2).ok();
        let workdir = row
            .get::<_, Option<String>>(3)
            .ok()
            .flatten()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let created_at = sqlite_time(row.get::<_, Option<String>>(4).ok().flatten());
        let updated_at = sqlite_time(row.get::<_, Option<String>>(5).ok().flatten());
        let provider = row
            .get::<_, Option<String>>(6)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());
        let model = row
            .get::<_, Option<String>>(7)
            .ok()
            .flatten()
            .and_then(|value| model_from_config(&value))
            .or(provider.clone());
        let archived = row
            .get::<_, Option<String>>(8)
            .ok()
            .flatten()
            .is_some_and(|value| !value.trim().is_empty());
        let parent = row
            .get::<_, Option<String>>(9)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());
        let preview = previews.get(&id).cloned();
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                SessionHistoryRecord::identity(kind, id.clone(), &workdir, path.to_path_buf())
            }
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: id.clone(),
                    title: first_non_empty([
                        name.filter(|value| !is_placeholder_name(value))
                            .and_then(|value| clean_text(value, 100)),
                        preview.clone().and_then(|value| clean_text(value, 100)),
                        description.and_then(|value| clean_text(value, 100)),
                    ]),
                    preview: preview.and_then(|value| clean_text(value, 240)),
                    model: model.clone(),
                    models: model.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
                    cli_kind: kind,
                    created_at,
                    updated_at,
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived,
                    can_resume: true,
                    metadata_source: "gooseDatabase".into(),
                },
                locator: path.to_path_buf(),
                record_key: id,
                role: if parent.is_some() {
                    AgentSessionRole::Subagent
                } else {
                    AgentSessionRole::Main
                },
                parent_native_id: parent,
                resume_reason: None,
                content_unavailable_reason: None,
            },
        };
        result.workspace(&workdir).records.push(record);
    }
    Ok(())
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 Goose 会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_columns(
    connection: &Connection,
    table: &str,
) -> Result<BTreeSet<String>, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(columns)
}

fn read_previews(
    connection: &Connection,
    budget: &SessionReadBudget,
) -> Result<HashMap<String, String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT m.session_id, m.content_json FROM messages m \
             WHERE m.role = 'user' AND m.rowid = (\
                 SELECT MIN(rowid) FROM messages \
                 WHERE session_id = m.session_id AND role = 'user'\
             )",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut previews = HashMap::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        let session_id: String = row.get(0).map_err(|error| error.to_string())?;
        let Ok(content) = row.get::<_, String>(1) else {
            continue;
        };
        let Some(text) = content_text(&content) else {
            continue;
        };
        previews.entry(session_id).or_insert(text);
    }
    Ok(previews)
}

fn content_text(content: &str) -> Option<String> {
    let content = serde_json::from_str::<Value>(content).ok()?;
    let text = content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn model_from_config(value: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(value).ok()?;
    ["model_name", "model", "name"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_placeholder_name(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "new session" | "untitled" | "new chat"
    )
}

fn sqlite_time(value: Option<String>) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(normalized) = normalize_timestamp(Some(value)) {
        return Some(normalized);
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(time) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Some(time.and_utc().to_rfc3339());
        }
    }
    None
}
