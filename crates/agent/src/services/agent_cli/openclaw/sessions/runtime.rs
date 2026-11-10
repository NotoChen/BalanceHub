//! OpenClaw runtime session database.
//!
//! OpenClaw 2026.x keeps session windows in `session_windows`/`session_nodes`
//! and the Pi-format transcript events in `transcript_events` below
//! `<state>/agents/<agent>/agent/openclaw-agent.sqlite`. Large events are
//! stored zstd-compressed (`event_zstd`); the SQL projection returns them as
//! hex and the decoder expands them before handing each event to the Pi
//! parser.

use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::{
            contracts::{
                SessionHistoryReadMode, SessionHistoryRecord, SessionHistoryScan, SessionReadBudget,
                SessionTranscriptFormat, SessionTranscriptMutation, SessionTranscriptSource,
                SqliteTranscript,
            },
            pi_family,
        },
        cli_sessions::{clean_text, first_non_empty, timestamp_from_value},
    },
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const METADATA_SOURCE: &str = "openclawRuntimeDatabase";
const PARSER_VERSION: u32 = 1;
const MAX_SESSIONS: usize = 100_000;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);
const SUBAGENT_REASON: &str = "OpenClaw 子代理会话不能单独续聊";
const EMPTY_REASON: &str = "会话还没有可续聊的消息";

const SUMMARY_QUERY: &str = "SELECT w.session_id, w.session_key, \
        w.created_at, w.updated_at, w.model, \
        coalesce(nullif(trim(n.display_name), ''), nullif(trim(n.label), ''), '') AS title, \
        n.parent_session_key, n.spawned_by, \
        (SELECT json_extract(e.event_json, '$.cwd') FROM transcript_events e \
            WHERE e.session_id = w.session_id AND e.seq = 0 \
              AND e.event_json IS NOT NULL) AS cwd, \
        (SELECT count(*) FROM transcript_events e \
            WHERE e.session_id = w.session_id AND e.message_role = 'user' \
              AND e.event_json IS NOT NULL) AS user_messages, \
        (SELECT e.event_json FROM transcript_events e \
            WHERE e.session_id = w.session_id AND e.message_role = 'user' \
              AND e.event_json IS NOT NULL ORDER BY e.seq LIMIT 1) AS first_user \
    FROM session_windows w \
    JOIN session_nodes n \
      ON n.session_key = w.session_key AND n.current_session_id = w.session_id \
    ORDER BY w.updated_at DESC, w.session_id";

const METADATA_QUERY: &str = "SELECT w.session_id, w.session_key, \
        w.created_at, w.updated_at, w.model, \
        coalesce(nullif(trim(n.display_name), ''), nullif(trim(n.label), ''), '') AS title, \
        n.parent_session_key, n.spawned_by, \
        NULL AS cwd, 0 AS user_messages, NULL AS first_user \
    FROM session_windows w \
    JOIN session_nodes n \
      ON n.session_key = w.session_key AND n.current_session_id = w.session_id \
    ORDER BY w.updated_at DESC, w.session_id";

const TRANSCRIPT_QUERY: &str = "SELECT seq AS row_key, \
        CASE WHEN event_json IS NOT NULL THEN event_json ELSE hex(event_zstd) END AS payload \
    FROM transcript_events \
    WHERE session_id = ?3 AND seq > ?1 \
      AND (event_json IS NOT NULL OR event_zstd IS NOT NULL) \
    ORDER BY seq LIMIT ?2";

pub(super) fn database_paths(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root.join("agents")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| {
            entry
                .path()
                .join("agent")
                .join("openclaw-agent.sqlite")
        })
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
}

pub(super) fn scan_database(
    kind: AgentCliKind,
    path: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
    result: &mut SessionHistoryScan,
    runtime_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let connection = open(path)?;
    if !table_exists(&connection, "session_windows")
        || !table_exists(&connection, "session_nodes")
    {
        return Ok(());
    }
    let query = if table_exists(&connection, "transcript_events") {
        SUMMARY_QUERY
    } else {
        METADATA_QUERY
    };
    let mut statement = connection
        .prepare(query)
        .map_err(|error| format!("读取 OpenClaw 会话列表失败：{error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("读取 OpenClaw 会话列表失败：{error}"))?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut read = 0usize;
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("读取 OpenClaw 会话列表失败：{error}"))?
    {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("OpenClaw 会话数量超过上限，已跳过部分记录");
            break;
        }
        read += 1;
        let session_id: String = row
            .get("session_id")
            .map_err(|error| format!("读取 OpenClaw 会话标识失败：{error}"))?;
        let session_key: String = row
            .get("session_key")
            .map_err(|error| format!("读取 OpenClaw 会话键失败：{error}"))?;
        let first_user = row.get::<_, Option<String>>("first_user").ok().flatten();
        if let Some(first_user) = &first_user {
            budget.record(first_user.len())?;
        }
        let workdir = row
            .get::<_, Option<String>>("cwd")
            .ok()
            .flatten()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let parent = row
            .get::<_, Option<String>>("parent_session_key")
            .ok()
            .flatten();
        let spawned_by = row.get::<_, Option<String>>("spawned_by").ok().flatten();
        runtime_ids.insert(session_id.clone());
        let record = match mode {
            SessionHistoryReadMode::Identities => identity_record(
                kind,
                path,
                &session_id,
                &session_key,
                &workdir,
            ),
            SessionHistoryReadMode::Summaries => summary_record(
                kind,
                path,
                SummaryFacts {
                    session_id: &session_id,
                    session_key: &session_key,
                    workdir: &workdir,
                    title: row.get::<_, Option<String>>("title").ok().flatten(),
                    model: row.get::<_, Option<String>>("model").ok().flatten(),
                    created_at: row.get::<_, Option<i64>>("created_at").ok().flatten(),
                    updated_at: row.get::<_, Option<i64>>("updated_at").ok().flatten(),
                    first_user: first_user.as_deref(),
                    parent,
                    spawned_by,
                    user_messages: row
                        .get::<_, i64>("user_messages")
                        .map(|value| value > 0)
                        .unwrap_or(false),
                },
            ),
        };
        result.workspace(&workdir).records.push(record);
    }
    Ok(())
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    session_id: &str,
    session_key: &str,
    workdir: &Path,
) -> SessionHistoryRecord {
    SessionHistoryRecord {
        summary: CliSessionSummary {
            id: session_key.to_string(),
            title: String::new(),
            preview: None,
            model: None,
            models: Vec::new(),
            cli_kind: kind,
            created_at: None,
            updated_at: None,
            workdir: workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume: false,
            metadata_source: METADATA_SOURCE.into(),
        },
        locator: path.to_path_buf(),
        record_key: session_id.to_string(),
        role: AgentSessionRole::Unknown,
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason: None,
    }
}

struct SummaryFacts<'a> {
    session_id: &'a str,
    session_key: &'a str,
    workdir: &'a Path,
    title: Option<String>,
    model: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    first_user: Option<&'a str>,
    parent: Option<String>,
    spawned_by: Option<String>,
    user_messages: bool,
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    facts: SummaryFacts<'_>,
) -> SessionHistoryRecord {
    let preview = facts
        .first_user
        .and_then(first_user_text)
        .and_then(|value| clean_text(value, 240));
    let title = first_non_empty([
        facts
            .title
            .and_then(|value| clean_text(value, 100)),
        preview.clone().and_then(|value| clean_text(value, 100)),
    ]);
    let model = facts
        .model
        .and_then(|value| clean_text(value, 100));
    let subagent = facts.parent.is_some() || facts.spawned_by.is_some();
    let can_resume = !subagent && facts.user_messages;
    let resume_reason = subagent
        .then(|| SUBAGENT_REASON.to_string())
        .or_else(|| (!facts.user_messages).then(|| EMPTY_REASON.to_string()));
    SessionHistoryRecord {
        summary: CliSessionSummary {
            id: facts.session_key.to_string(),
            title,
            preview,
            models: model.clone().into_iter().collect(),
            model,
            cli_kind: kind,
            created_at: facts
                .created_at
                .and_then(|value| timestamp_from_value(Some(value), true)),
            updated_at: facts
                .updated_at
                .and_then(|value| timestamp_from_value(Some(value), true)),
            workdir: facts.workdir.to_string_lossy().into_owned(),
            cli_version: None,
            archived: false,
            can_resume,
            metadata_source: METADATA_SOURCE.into(),
        },
        locator: path.to_path_buf(),
        record_key: facts.session_id.to_string(),
        role: if subagent {
            AgentSessionRole::Subagent
        } else {
            AgentSessionRole::Main
        },
        parent_native_id: facts.parent,
        resume_reason,
        content_unavailable_reason: None,
    }
}

pub(super) fn transcript(
    record: &SessionHistoryRecord,
) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("OpenClaw 会话数据库不存在".into());
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
        decode,
    }])
}

pub(super) fn decode(
    path: &Path,
    sequence: u64,
    payload: &[u8],
    state: &mut Value,
) -> Vec<SessionTranscriptMutation> {
    if payload.first() == Some(&b'{') {
        return pi_family::sessions::decode_entry(path, sequence, payload, state);
    }
    let Ok(encoded) = std::str::from_utf8(payload) else {
        return Vec::new();
    };
    let Some(compressed) = decode_hex(encoded) else {
        return Vec::new();
    };
    let Ok(decoded) = zstd::stream::decode_all(compressed.as_slice()) else {
        return Vec::new();
    };
    pi_family::sessions::decode_entry(path, sequence, &decoded, state)
}

fn first_user_text(value: &str) -> Option<String> {
    let entry: Value = serde_json::from_str(value).ok()?;
    pi_family::sessions::message_text(entry.get("message")?)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let (pairs, _) = value.as_bytes().as_chunks::<2>();
    for pair in pairs {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    Some(bytes)
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 OpenClaw 会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .ok()
        .and_then(|mut statement| {
            statement
                .query_row([table], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some()
}
