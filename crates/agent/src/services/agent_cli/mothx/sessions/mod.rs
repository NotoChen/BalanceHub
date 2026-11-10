//! MothX SQLite session history.
//!
//! MothX keeps every session in one `sessions.db`. The `sessions` table holds
//! the header and the append-only `entries` table holds messages, model
//! changes, compactions and metadata. Entry `data` is the complete native
//! entry JSON, including its message payload.

mod decoder;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode,
            SessionHistoryRecord, SessionHistoryScan, SessionHistorySource, SessionReadBudget,
            SessionTranscriptFormat, SessionTranscriptSource, SqliteTranscript,
        },
        cli_sessions::{clean_text, first_non_empty, normalize_timestamp},
        cli_paths::user_home,
    },
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};

const PARSER_VERSION: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SESSIONS: usize = 100_000;

const TRANSCRIPT_QUERY: &str = "SELECT seq AS row_key, data AS payload FROM entries \
WHERE session_id = ?3 AND seq > ?1 AND type IN (\
'message', 'content_override', 'compaction', 'branch_summary', \
'session_info', 'custom_message', 'custom', 'model_change'\
) ORDER BY seq LIMIT ?2";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("MothX 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("MOTHX_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let root = config::config_dir_for_home(
        &user_home().ok_or_else(|| "无法定位用户目录".to_string())?,
    );
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

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("MothX 会话数据库不存在".into());
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

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut result = SessionHistoryScan::new(&[]);
    // `sessionDir` is independent of `MOTHX_DIR`; a user who moved the session
    // store must still see every native session in BalanceHub.
    let session_dir = config::session_dir_for_root(root).unwrap_or_else(|| root.join("sessions"));
    let path = session_dir.join("sessions.db");
    if !path.is_file() {
        return Ok(result);
    }
    if let Err(error) = scan_database(kind, &path, workdirs, budget, mode, &mut result) {
        result.failed(error);
    }
    Ok(result)
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
    if !columns.contains("id") || !columns.contains("cwd") {
        return Err("MothX 会话数据库缺少 sessions 表".into());
    }
    let column = |name: &str| {
        if columns.contains(name) {
            name.to_owned()
        } else {
            format!("NULL AS {name}")
        }
    };
    let want_previews = matches!(mode, SessionHistoryReadMode::Summaries);
    let sql = format!(
        "SELECT s.id, {}, {}, {}, COALESCE((\
             SELECT MAX(e.timestamp) FROM entries e WHERE e.session_id = s.id\
         ), {}) AS last_active_at, \
         (SELECT e.data FROM entries e WHERE e.session_id = s.id AND e.type = 'session_info' \
          ORDER BY e.seq DESC LIMIT 1) AS info, \
         (SELECT e.data FROM entries e WHERE e.session_id = s.id AND e.type = 'model_change' \
          ORDER BY e.seq DESC LIMIT 1) AS model_change, \
         (SELECT e.data FROM entries e WHERE e.session_id = s.id AND e.type = 'message' \
          ORDER BY e.seq ASC LIMIT 1) AS first_message \
         FROM sessions s ORDER BY last_active_at DESC, s.id ASC",
        column("cwd"),
        column("parent_session"),
        column("timestamp"),
        if columns.contains("timestamp") {
            "s.timestamp".to_owned()
        } else {
            "NULL".to_owned()
        },
    );
    let mut statement = connection.prepare(&sql).map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut read = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("MothX 会话数量超过上限，已跳过部分记录");
            return Ok(());
        }
        read += 1;
        let id: String = row.get(0).map_err(|error| error.to_string())?;
        let cwd = row.get::<_, Option<String>>(1).ok().flatten();
        let parent = row
            .get::<_, Option<String>>(2)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());
        let created_at = normalize_timestamp(row.get::<_, Option<String>>(3).ok().flatten().as_deref());
        let updated_at = normalize_timestamp(row.get::<_, Option<String>>(4).ok().flatten().as_deref());
        let info = row.get::<_, Option<String>>(5).ok().flatten();
        let model_change = row.get::<_, Option<String>>(6).ok().flatten();
        let first_message = want_previews
            .then(|| row.get::<_, Option<String>>(7).ok().flatten())
            .flatten();
        let name = info.as_deref().and_then(session_name);
        let model = model_change.as_deref().and_then(model_from_change);
        let preview = first_message.as_deref().and_then(user_text);
        let workdir = cwd
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let preview = preview.and_then(|value| clean_text(value, 240));
        let title = first_non_empty([
            name.and_then(|value| clean_text(value, 100)),
            preview.clone().and_then(|value| clean_text(value, 100)),
        ]);
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
                    models: model.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
                    cli_kind: kind,
                    created_at,
                    updated_at,
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived: false,
                    can_resume: true,
                    metadata_source: "mothxDatabase".into(),
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
    .map_err(|error| format!("打开 MothX 会话数据库失败：{error}"))?;
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

fn session_name(data: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn model_from_change(data: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    value
        .get("modelId")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn user_text(data: &str) -> Option<String> {
    let entry = serde_json::from_str::<Value>(data).ok()?;
    let message = entry.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("user")
        || message.get("systemInjected").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    message_text(message)
}

fn message_text(message: &Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(content) = message.get("content").and_then(Value::as_str) {
        if !content.trim().is_empty() {
            parts.push(content.to_owned());
        }
    }
    if let Some(contents) = message.get("contents").and_then(Value::as_array) {
        for block in contents {
            if let Some(text) = block
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
            {
                parts.push(text.to_owned());
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_cli::contracts::{SessionHistoryReadMode, SessionReadBudget};
    use std::{
        fs,
        sync::{
            atomic::AtomicBool,
            Arc,
        },
        time::{Duration, Instant},
    };

    fn budget() -> SessionReadBudget {
        SessionReadBudget::new(
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            32 * 1024 * 1024,
        )
    }

    #[test]
    fn scan_honors_the_configured_session_dir() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-mothx-session-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = root.join("moved-sessions");
        fs::create_dir_all(&store).unwrap();
        {
            let connection = Connection::open(store.join("sessions.db")).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE sessions (
                        id TEXT PRIMARY KEY,
                        cwd TEXT,
                        parent_session TEXT,
                        timestamp TEXT
                    );
                    CREATE TABLE entries (
                        seq INTEGER PRIMARY KEY,
                        session_id TEXT NOT NULL,
                        timestamp TEXT,
                        type TEXT,
                        data TEXT
                    );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO sessions (id, cwd, timestamp) VALUES ('sess-1', '/work/proj', '2026-01-01T00:00:00Z')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO entries (seq, session_id, timestamp, type, data) VALUES
                     (1, 'sess-1', '2026-01-01T00:00:01Z', 'session_info', '{\"name\":\"会话标题\"}'),
                     (2, 'sess-1', '2026-01-01T00:00:02Z', 'message', '{\"message\":{\"role\":\"user\",\"content\":\"请检查构建\"}}')",
                    [],
                )
                .unwrap();
        }
        fs::write(
            root.join("settings.json"),
            serde_json::json!({ "sessionDir": store.to_string_lossy() }).to_string(),
        )
        .unwrap();

        let scan = (super::HISTORY.scan)(
            AgentCliKind::MothX,
            &root,
            &[],
            &budget(),
            SessionHistoryReadMode::Summaries,
        )
        .unwrap();
        let summaries = scan
            .workspaces
            .into_iter()
            .flat_map(|workspace| workspace.records)
            .map(|record| record.summary)
            .collect::<Vec<_>>();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "sess-1");
        assert_eq!(summaries[0].title, "会话标题");
        assert_eq!(summaries[0].preview.as_deref(), Some("请检查构建"));
        assert!(summaries[0].can_resume);
        let _ = fs::remove_dir_all(root);
    }
}
