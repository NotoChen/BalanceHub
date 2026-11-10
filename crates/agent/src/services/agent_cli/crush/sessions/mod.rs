//! Crush SQLite session history.
//!
//! Crush keeps one database per project (`<project>/.crush/crush.db`) and a
//! global `projects.json` registry. Sessions are read from every registered
//! project plus the current workspace candidates.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::agent_cli::contracts::{
        EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
        SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
        SessionTranscriptSource, SqliteTranscript,
    },
    services::cli_sessions::{clean_text, first_non_empty, timestamp_from_unix},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const PARSER_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SESSIONS: usize = 100_000;

// Columns present since Crush's initial schema, so a current database always
// satisfies the transcript query.
const TRANSCRIPT_QUERY: &str = "SELECT rowid AS row_key, json_object(\
    'id', id, 'role', role, 'parts', parts, 'model', model, \
    'created_at', created_at, 'updated_at', updated_at\
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
        return Err("Crush 原生目录必须是绝对路径");
    }
    Ok(EnvironmentPatch::default())
}

fn source() -> Result<SessionHistorySource, String> {
    // Project databases live next to their workspaces rather than below one
    // global state folder; the user's home bounds every registered project the
    // reader is allowed to open.
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = home.canonicalize().unwrap_or(home);
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment: EnvironmentPatch::default(),
        resume_reason: None,
    })
}

fn scan(
    kind: AgentCliKind,
    _root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let databases = match databases(workdirs) {
        Ok(databases) => databases,
        Err(error) => {
            result.failed(error);
            return Ok(result);
        }
    };
    for database in databases {
        if let Err(error) = budget.check() {
            result.failed(error);
            break;
        }
        if let Err(error) = scan_database(kind, &database, budget, mode, &mut result) {
            result.failed(error);
        }
    }
    Ok(result)
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Crush 会话数据库不存在".into());
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

struct Database {
    path: PathBuf,
    workdir: PathBuf,
}

fn databases(workdirs: &[PathBuf]) -> Result<Vec<Database>, String> {
    let mut found: Vec<Database> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut add = |path: PathBuf, workdir: PathBuf| {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(canonical) || !path.is_file() {
            return;
        }
        found.push(Database { path, workdir });
    };
    for workdir in workdirs {
        add(
            workdir.join(".crush/crush.db"),
            workdir.to_path_buf(),
        );
    }
    let Some(global) = config::global_data_dir() else {
        return Ok(found);
    };
    let registry = global.join("projects.json");
    let Ok(text) = fs::read_to_string(&registry) else {
        return Ok(found);
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Ok(found);
    };
    for project in value
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(data_dir) = project.get("data_dir").and_then(Value::as_str) else {
            continue;
        };
        let workdir = project
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_default();
        add(PathBuf::from(data_dir).join("crush.db"), workdir);
    }
    Ok(found)
}

fn scan_database(
    kind: AgentCliKind,
    database: &Database,
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    result: &mut SessionHistoryScan,
) -> Result<(), String> {
    let connection = open(&database.path)?;
    let summaries = if matches!(mode, SessionHistoryReadMode::Summaries) {
        read_previews(&connection, budget).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    let mut statement = connection
        .prepare(
            "SELECT id, parent_session_id, title, created_at, updated_at \
             FROM sessions ORDER BY updated_at DESC, id ASC",
        )
        .map_err(|error| format!("读取 Crush 会话失败：{error}"))?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut read = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("Crush 会话数量超过上限，已跳过部分记录");
            return Ok(());
        }
        read += 1;
        let id: String = row.get(0).map_err(|error| error.to_string())?;
        let parent: Option<String> = row.get(1).ok();
        let title: Option<String> = row.get(2).ok();
        let created_at: Option<i64> = row.get(3).ok();
        let updated_at: Option<i64> = row.get(4).ok();
        let preview = summaries.get(&id).cloned();
        let workdir = database.workdir.clone();
        let record = match mode {
            SessionHistoryReadMode::Identities => SessionHistoryRecord::identity(
                kind,
                id.clone(),
                &workdir,
                database.path.clone(),
            ),
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: id.clone(),
                    title: first_non_empty([
                        title.and_then(|value| clean_text(value, 100)),
                        preview.clone().and_then(|value| clean_text(value, 100)),
                    ]),
                    preview: preview.and_then(|value| clean_text(value, 240)),
                    model: None,
                    models: Vec::new(),
                    cli_kind: kind,
                    created_at: timestamp_from_unix(created_at.filter(|value| *value > 0)),
                    updated_at: timestamp_from_unix(updated_at.filter(|value| *value > 0)),
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived: false,
                    can_resume: true,
                    metadata_source: "crushDatabase".into(),
                },
                locator: database.path.clone(),
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
    .map_err(|error| format!("打开 Crush 会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn read_previews(
    connection: &Connection,
    budget: &SessionReadBudget,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT m.session_id, m.parts FROM messages m \
             WHERE m.role = 'user' AND m.rowid = (\
                 SELECT MIN(rowid) FROM messages \
                 WHERE session_id = m.session_id AND role = 'user'\
             )",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut previews = std::collections::HashMap::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        let session_id: String = row.get(0).map_err(|error| error.to_string())?;
        let Ok(parts) = row.get::<_, String>(1) else {
            continue;
        };
        let Some(text) = parts_text(&parts) else {
            continue;
        };
        previews.entry(session_id).or_insert(text);
    }
    Ok(previews)
}

fn parts_text(parts: &str) -> Option<String> {
    let parts = serde_json::from_str::<Value>(parts).ok()?;
    let text = parts
        .as_array()?
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) != Some("text") {
                return None;
            }
            let data = part.get("data")?;
            if data.get("hidden").and_then(Value::as_bool) == Some(true)
                || data
                    .get("context")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            {
                return None;
            }
            data.get("text")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
