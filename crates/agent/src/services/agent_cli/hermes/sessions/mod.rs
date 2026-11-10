//! Hermes Agent SQLite session history.
//!
//! The active profile keeps `state.db` with a `sessions` table and one
//! `messages` row per turn. Message rows are read through the shared SQLite
//! transcript batch reader; summaries query the same tables once.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource, SqliteTranscript,
    },
    services::cli_sessions::{
        clean_text, first_non_empty, timestamp_from_unix,
    },
};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

const PARSER_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SESSIONS: usize = 100_000;

const TRANSCRIPT_QUERY: &str = "SELECT id AS row_key, json_object(\
    'id', id, 'role', role, 'content', content, 'tool_call_id', tool_call_id, \
    'tool_calls', tool_calls, 'tool_name', tool_name, 'timestamp', timestamp, \
    'reasoning', reasoning, 'display_kind', display_kind, 'compacted', compacted\
) AS payload FROM messages \
WHERE session_id = ?3 AND active = 1 AND id > ?1 ORDER BY id LIMIT ?2";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Hermes 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("HERMES_HOME", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::active_data_dir().ok_or_else(|| "无法定位 Hermes 数据目录".to_string())?;
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

fn database_path(root: &Path) -> PathBuf {
    root.join("state.db")
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let path = database_path(root);
    if !path.is_file() {
        return Ok(result);
    }
    let sessions = match read_sessions(&path, budget) {
        Ok(sessions) => sessions,
        Err(error) => {
            result.failed(error);
            return Ok(result);
        }
    };
    if sessions.len() > MAX_SESSIONS {
        result.failed("Hermes 会话数量超过上限，已跳过部分记录");
    }
    let previews = if matches!(mode, SessionHistoryReadMode::Summaries) {
        read_first_user_messages(&path, budget).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    for session in sessions.into_iter().take(MAX_SESSIONS) {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        let workdir = session
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let preview = previews
            .get(&session.id)
            .and_then(|value| clean_text(value.clone(), 240));
        let title = first_non_empty([
            session.title.and_then(|value| clean_text(value, 100)),
            session.display_name.and_then(|value| clean_text(value, 100)),
            preview.clone().and_then(|value| clean_text(value, 100)),
        ]);
        let record = match mode {
            SessionHistoryReadMode::Identities => SessionHistoryRecord::identity(
                kind,
                session.id.clone(),
                &workdir,
                path.clone(),
            ),
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: session.id.clone(),
                    title,
                    preview,
                    model: session.model.clone(),
                    models: session.model.into_iter().collect(),
                    cli_kind: kind,
                    created_at: timestamp_from_unix(session.started_at),
                    updated_at: timestamp_from_unix(session.last_activity_at)
                        .or_else(|| timestamp_from_unix(session.ended_at)),
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived: session.archived,
                    can_resume: true,
                    metadata_source: "hermesState".into(),
                },
                locator: path.clone(),
                record_key: session.id.clone(),
                role: if session.parent_session_id.is_some() {
                    AgentSessionRole::Subagent
                } else {
                    AgentSessionRole::Main
                },
                parent_native_id: session.parent_session_id,
                resume_reason: None,
                content_unavailable_reason: None,
            },
        };
        result.workspace(&workdir).records.push(record);
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Hermes 会话数据库不存在".into());
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

struct SessionRow {
    id: String,
    title: Option<String>,
    display_name: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    parent_session_id: Option<String>,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    last_activity_at: Option<i64>,
    archived: bool,
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 Hermes 会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_columns(
    connection: &Connection,
    table: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
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

fn read_sessions(
    path: &Path,
    budget: &SessionReadBudget,
) -> Result<Vec<SessionRow>, String> {
    let connection = open(path)?;
    let columns = table_columns(&connection, "sessions")?;
    if !columns.contains("id") {
        return Err("Hermes 会话数据库缺少 sessions 表".into());
    }
    let column = |name: &str| {
        if columns.contains(name) {
            name.to_owned()
        } else {
            format!("NULL AS {name}")
        }
    };
    let sql = format!(
        "SELECT id, {}, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM sessions ORDER BY id ASC",
        column("title"),
        column("display_name"),
        column("model"),
        column("cwd"),
        column("parent_session_id"),
        column("started_at"),
        column("ended_at"),
        column("last_activity_at"),
        column("archived"),
        column("message_count"),
    );
    let mut statement = connection.prepare(&sql).map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut sessions = Vec::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        let id: String = row.get(0).map_err(|error| error.to_string())?;
        sessions.push(SessionRow {
            id,
            title: row.get(1).ok(),
            display_name: row.get(2).ok(),
            model: row.get(3).ok(),
            cwd: row.get(4).ok(),
            parent_session_id: row
                .get::<_, Option<String>>(5)
                .ok()
                .flatten()
                .filter(|value| !value.trim().is_empty()),
            started_at: row.get(6).ok().map(|value: f64| value as i64).filter(|value| *value > 0),
            ended_at: row.get(7).ok().map(|value: f64| value as i64).filter(|value| *value > 0),
            last_activity_at: row.get(8).ok().map(|value: f64| value as i64).filter(|value| *value > 0),
            archived: row.get::<_, Option<i64>>(9).ok().flatten().is_some_and(|value| value != 0),
        });
    }
    Ok(sessions)
}

fn read_first_user_messages(
    path: &Path,
    budget: &SessionReadBudget,
) -> Result<HashMap<String, String>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare(
            "SELECT m.session_id, m.content FROM messages m \
             WHERE m.role = 'user' AND m.active = 1 AND m.id = (\
                 SELECT MIN(id) FROM messages \
                 WHERE session_id = m.session_id AND role = 'user' AND active = 1\
             )",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut previews = HashMap::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        let session_id: String = row.get(0).map_err(|error| error.to_string())?;
        if let Ok(content) = row.get::<_, String>(1) {
            previews.entry(session_id).or_insert(content);
        }
    }
    Ok(previews)
}
