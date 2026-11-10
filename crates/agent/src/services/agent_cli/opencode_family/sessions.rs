//! Session history shared by OpenCode and its forks.
//!
//! These CLIs keep sessions in SQLite (`session`, `message`, `part`) below
//! their data directory. One `part` row is one visible fragment; tool parts
//! carry both the request and the result, so the decoder emits a call row and
//! a result row that share the native call id.

use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan,
            SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
            SessionTranscriptSource, SqliteTranscript,
        },
        cli_paths::{configured_path, user_home},
        cli_sessions::{clean_text, first_non_empty, timestamp_from_value},
    },
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};

mod decoder;

const PARSER_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SESSIONS: usize = 100_000;

pub(crate) struct SessionSpec {
    pub data_dir: fn() -> Option<PathBuf>,
    pub db_name: &'static str,
    pub db_env: &'static str,
    pub metadata_source: &'static str,
    pub resume_reason: Option<&'static str>,
}

pub(crate) const TRANSCRIPT_QUERY: &str = "SELECT p.rowid AS row_key, json_object(\
    'part_id', p.id, 'message_id', p.message_id, 'session_id', p.session_id, \
    'part_time', p.time_created, 'message_time', m.time_created, \
    'part', json(p.data), 'message', json(m.data)\
) AS payload FROM part p JOIN message m ON m.id = p.message_id \
WHERE p.session_id = ?3 AND p.rowid > ?1 ORDER BY p.rowid LIMIT ?2";

/// XDG-style data directory used by OpenCode and its direct forks.
pub(crate) fn xdg_data_dir_for_home(home: &Path, app: &str) -> PathBuf {
    if let Some(root) = configured_path("XDG_DATA_HOME").filter(|path| path.is_absolute()) {
        return root.join(app);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = configured_path("LOCALAPPDATA").filter(|path| path.is_absolute()) {
            return local.join(app);
        }
        return home.join("AppData/Local").join(app);
    }
    #[cfg(not(target_os = "windows"))]
    {
        home.join(".local/share").join(app)
    }
}

pub(crate) fn app_data_dir(app: &'static str) -> Option<PathBuf> {
    Some(xdg_data_dir_for_home(&user_home()?, app))
}

pub(crate) fn database_path(spec: &SessionSpec) -> Option<PathBuf> {
    let root = (spec.data_dir)()?;
    let configured = std::env::var_os(spec.db_env)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    match configured {
        Some(path) if path.is_absolute() => Some(path),
        Some(path) => Some(root.join(path)),
        None => Some(root.join(spec.db_name)),
    }
}

pub(crate) fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("原生数据目录必须是绝对路径");
    }
    Ok(EnvironmentPatch::default())
}

pub(crate) fn source(spec: &'static SessionSpec) -> Result<SessionHistorySource, String> {
    let path = database_path(spec).ok_or_else(|| "无法定位会话数据库目录".to_string())?;
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "会话数据库目录无效".to_string())?;
    let (launch_environment, resume_reason) = match launch_environment(&root) {
        Ok(environment) => (environment, spec.resume_reason.map(str::to_owned)),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment,
        resume_reason,
    })
}

pub(crate) fn scan(
    spec: &'static SessionSpec,
    kind: AgentCliKind,
    _root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    let Some(path) = database_path(spec) else {
        return Ok(result);
    };
    if !path.is_file() {
        return Ok(result);
    }
    if let Err(error) = scan_database(spec, kind, &path, workdirs, budget, mode, &mut result) {
        result.failed(error);
    }
    Ok(result)
}

pub(crate) fn transcript(
    record: &SessionHistoryRecord,
) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("会话数据库不存在".into());
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

struct Preview {
    text: String,
    model: Option<String>,
}

fn scan_database(
    spec: &SessionSpec,
    kind: AgentCliKind,
    path: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    result: &mut SessionHistoryScan,
) -> Result<(), String> {
    let connection = open(path)?;
    let columns = table_columns(&connection, "session")?;
    if !columns.contains("id") || !columns.contains("directory") {
        return Err("会话数据库缺少 session 表".into());
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
        "SELECT id, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM session \
         ORDER BY time_updated DESC, id ASC",
        column("title"),
        column("directory"),
        column("parent_id"),
        column("time_created"),
        column("time_updated"),
        column("time_archived"),
        column("model"),
        column("version"),
        column("slug"),
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut read = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("会话数量超过上限，已跳过部分记录");
            return Ok(());
        }
        read += 1;
        let id: String = row.get(0).map_err(|error| error.to_string())?;
        let title = row
            .get::<_, Option<String>>(1)
            .ok()
            .flatten()
            .unwrap_or_default();
        let directory = row.get::<_, Option<String>>(2).ok().flatten();
        let parent = row
            .get::<_, Option<String>>(3)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());
        let created_at = timestamp_from_value(row.get::<_, Option<i64>>(4).ok().flatten(), true);
        let updated_at = timestamp_from_value(row.get::<_, Option<i64>>(5).ok().flatten(), true);
        let archived = row.get::<_, Option<i64>>(6).ok().flatten().is_some();
        let model = row
            .get::<_, Option<String>>(7)
            .ok()
            .flatten()
            .and_then(|value| model_from_json(&value));
        let cli_version = row
            .get::<_, Option<String>>(8)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());
        let saved = previews.get(&id);
        let model = model.or_else(|| saved.and_then(|preview| preview.model.clone()));
        let workdir = directory
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let preview = saved
            .map(|preview| preview.text.clone())
            .and_then(|value| clean_text(value, 240));
        let title = if is_placeholder_title(&title) {
            first_non_empty([
                preview.clone().and_then(|value| clean_text(value, 100)),
                clean_text(title, 100),
            ])
        } else {
            first_non_empty([
                clean_text(title, 100),
                preview.clone().and_then(|value| clean_text(value, 100)),
            ])
        };
        let record = match mode {
            SessionHistoryReadMode::Identities => {
                SessionHistoryRecord::identity(kind, id, &workdir, path.to_path_buf())
            }
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: id.clone(),
                    title,
                    preview,
                    model: model.clone(),
                    models: model
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    cli_kind: kind,
                    created_at,
                    updated_at,
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version,
                    archived,
                    can_resume: spec.resume_reason.is_none(),
                    metadata_source: spec.metadata_source.into(),
                },
                locator: path.to_path_buf(),
                record_key: id,
                role: if parent.is_some() {
                    AgentSessionRole::Subagent
                } else {
                    AgentSessionRole::Main
                },
                parent_native_id: parent,
                resume_reason: spec.resume_reason.map(Into::into),
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
    .map_err(|error| format!("打开会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_columns(connection: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
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
) -> Result<HashMap<String, Preview>, String> {
    let mut statement = connection
        .prepare(
            "SELECT m.session_id, m.data, p.data FROM message m \
             JOIN part p ON p.message_id = m.id \
             WHERE m.id = (\
                 SELECT m2.id FROM message m2 \
                 WHERE m2.session_id = m.session_id \
                   AND json_extract(m2.data, '$.role') = 'user' \
                 ORDER BY m2.time_created, m2.id LIMIT 1\
             ) ORDER BY m.session_id, p.id",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let mut previews: HashMap<String, Preview> = HashMap::new();
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        let session_id: String = row.get(0).map_err(|error| error.to_string())?;
        let message: String = row.get(1).map_err(|error| error.to_string())?;
        let part: String = row.get(2).map_err(|error| error.to_string())?;
        let entry = previews.entry(session_id).or_insert_with(|| Preview {
            text: String::new(),
            model: model_from_json(&message),
        });
        if !entry.text.is_empty() {
            continue;
        }
        if let Some(text) = part_text(&part) {
            entry.text = text;
        }
    }
    Ok(previews)
}

fn part_text(part: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(part).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    value
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn model_from_json(value: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(value).ok()?;
    value
        .get("model")
        .and_then(|model| {
            model
                .get("modelID")
                .or_else(|| model.get("id"))
                .and_then(Value::as_str)
        })
        .or_else(|| value.get("modelID").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_placeholder_title(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.is_empty()
        || value.starts_with("new session")
        || value == "untitled"
        || value == "new chat"
}
