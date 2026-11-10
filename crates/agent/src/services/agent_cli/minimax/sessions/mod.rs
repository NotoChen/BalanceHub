//! MiniMax Code native session history.
//!
//! v2 keeps session metadata in `<data>/v2/sqlite/runtime-state.sqlite`
//! (`local_runtime_sessions.record_json`) and the display conversation in
//! `local_runtime_message_rows.data_json`. Installs that never ran the row
//! backfill keep the display array in `local_runtime_messages`.
//! `canonicalTextRange` and the display projection already exclude injected
//! system reminders, so both sources read as the conversation users saw.

mod decoder;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::contracts::{
            EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistoryScan, SessionHistorySource, SessionReadBudget, SessionTranscriptFormat,
            SessionTranscriptSource, SqliteTranscript,
        },
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

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "minimaxRuntimeDatabase";
const MAX_SESSIONS: usize = 100_000;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

const TRANSCRIPT_QUERY: &str = "SELECT id AS row_key, data_json AS payload \
    FROM local_runtime_message_rows \
    WHERE session_id = ?3 AND id > ?1 ORDER BY id LIMIT ?2";

// json_each 的数组下标从 0 开始，而读取端以 0 作为“还没消费任何行”的初始
// offset 并把 `row_key <= offset` 视为已读；这里把下标平移成 1 基，避免第一条
// 消息在第一轮就被当成已消费而丢掉。
const LEGACY_TRANSCRIPT_QUERY: &str = "SELECT je.key + 1 AS row_key, je.value AS payload \
    FROM local_runtime_messages m, json_each(m.display_messages_json) je \
    WHERE m.session_id = ?3 AND je.key + 1 > ?1 ORDER BY je.key LIMIT ?2";

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("MiniMax Code 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("MINIMAX_DATA_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = config::data_dir_for_home(&home);
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
    root.join("v2/sqlite/runtime-state.sqlite")
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
        return Err("MiniMax Code 会话数据库不存在".into());
    }
    let query = transcript_query(&record.locator, &record.record_key)
        .ok_or_else(|| "MiniMax Code 会话内容已不可用".to_string())?;
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: false,
        sqlite: Some(SqliteTranscript {
            query,
            scope: record.record_key.clone(),
        }),
        decode: decoder::decode,
    }])
}

fn transcript_query(path: &Path, session_id: &str) -> Option<&'static str> {
    let connection = open(path).ok()?;
    if table_exists(&connection, "local_runtime_message_rows")
        && session_has_rows(&connection, "local_runtime_message_rows", session_id)
    {
        return Some(TRANSCRIPT_QUERY);
    }
    if table_exists(&connection, "local_runtime_messages")
        && legacy_blob_exists(&connection, session_id)
    {
        return Some(LEGACY_TRANSCRIPT_QUERY);
    }
    None
}

fn session_has_rows(connection: &Connection, table: &str, session_id: &str) -> bool {
    connection
        .prepare(&format!(
            "SELECT 1 FROM {table} WHERE session_id = ? LIMIT 1"
        ))
        .ok()
        .and_then(|mut statement| {
            statement
                .query_row([session_id], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some()
}

fn legacy_blob_exists(connection: &Connection, session_id: &str) -> bool {
    connection
        .prepare(
            "SELECT 1 FROM local_runtime_messages \
             WHERE session_id = ? AND display_messages_json IS NOT NULL LIMIT 1",
        )
        .ok()
        .and_then(|mut statement| {
            statement
                .query_row([session_id], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some()
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
    if !table_exists(&connection, "local_runtime_sessions") {
        return Err("MiniMax Code 会话数据库缺少 local_runtime_sessions 表".into());
    }
    let previews = if matches!(mode, SessionHistoryReadMode::Summaries) {
        read_previews(&connection, budget).unwrap_or_default()
    } else {
        HashMap::new()
    };
    // The columnar projection is what the CLI lists from; `record_json` can lag
    // behind after archive/visibility updates, so columns win where present.
    let mut statement = connection
        .prepare("SELECT * FROM local_runtime_sessions ORDER BY updated_at_ms DESC, session_id ASC")
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    let fallback_workdir = (workdirs.len() == 1).then(|| workdirs[0].as_path());
    let mut read = 0usize;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        budget.check()?;
        if read >= MAX_SESSIONS {
            result.failed("MiniMax Code 会话数量超过上限，已跳过部分记录");
            return Ok(());
        }
        read += 1;
        let id: String = row.get("session_id").map_err(|error| error.to_string())?;
        let record = row
            .get::<_, Option<String>>("record_json")
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or(Value::Null);
        let visibility = column_text(row, "visibility").or_else(|| text(&record, "visibility"));
        if visibility.as_deref() == Some("hidden") {
            continue;
        }
        if matches!(
            column_text(row, "session_kind").as_deref(),
            Some("peek" | "cron")
        ) {
            continue;
        }
        let updated_column = row.get::<_, Option<i64>>("updated_at_ms").ok().flatten();
        let workdir = column_text(row, "workspace_dir")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                text(&record, "workspaceDir")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
            })
            .or_else(|| fallback_workdir.map(Path::to_path_buf))
            .unwrap_or_default();
        let preview = previews
            .get(&id)
            .cloned()
            .or_else(|| text(&record, "lastAssistantText"))
            .and_then(|value| clean_text(value, 240));
        let model = row
            .get::<_, Option<String>>("extra_data_json")
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .and_then(|value| {
                value
                    .get("effectiveModel")
                    .and_then(Value::as_str)
                    .and_then(model_label)
            })
            .or_else(|| {
                record
                    .pointer("/model/modelId")
                    .or_else(|| record.pointer("/model/model_id"))
                    .and_then(Value::as_str)
                    .and_then(model_label)
            });
        let parent = column_text(row, "parent_session_id")
            .or_else(|| text(&record, "parentSessionId"));
        let record_mode = match mode {
            SessionHistoryReadMode::Identities => {
                SessionHistoryRecord::identity(kind, id.clone(), &workdir, path.to_path_buf())
            }
            SessionHistoryReadMode::Summaries => SessionHistoryRecord {
                summary: CliSessionSummary {
                    id: id.clone(),
                    title: first_non_empty([
                        column_text(row, "title")
                            .or_else(|| text(&record, "title"))
                            .and_then(|value| clean_text(value, 100)),
                        preview.clone().and_then(|value| clean_text(value, 100)),
                        workdir
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .and_then(|value| clean_text(value, 100)),
                    ]),
                    preview,
                    model: model.clone(),
                    models: model.into_iter().collect::<BTreeSet<_>>().into_iter().collect(),
                    cli_kind: kind,
                    created_at: row
                        .get::<_, Option<i64>>("created_at_ms")
                        .ok()
                        .flatten()
                        .or_else(|| record.get("createdAtMs").and_then(Value::as_i64))
                        .and_then(|value| timestamp_from_value(Some(value), true)),
                    updated_at: updated_column
                        .or_else(|| record.get("updatedAtMs").and_then(Value::as_i64))
                        .and_then(|value| timestamp_from_value(Some(value), true)),
                    workdir: workdir.to_string_lossy().into_owned(),
                    cli_version: None,
                    archived: row
                        .get::<_, Option<i64>>("archived")
                        .ok()
                        .flatten()
                        .map(|value| value != 0)
                        .or_else(|| record.get("archived").and_then(Value::as_bool))
                        .unwrap_or(false),
                    can_resume: true,
                    metadata_source: METADATA_SOURCE.into(),
                },
                locator: path.to_path_buf(),
                record_key: id.clone(),
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
        result.workspace(&workdir).records.push(record_mode);
    }
    Ok(())
}

fn read_previews(
    connection: &Connection,
    budget: &SessionReadBudget,
) -> Result<HashMap<String, String>, String> {
    let mut previews = HashMap::new();
    let rows_exist = table_exists(connection, "local_runtime_message_rows");
    if rows_exist {
        let mut statement = connection
            .prepare(
                "SELECT r.session_id, r.data_json \
                 FROM local_runtime_message_rows r \
                 JOIN (SELECT session_id, MIN(id) AS id FROM local_runtime_message_rows \
                       WHERE role = 'user' GROUP BY session_id) first \
                   ON first.id = r.id",
            )
            .map_err(|error| error.to_string())?;
        let mut rows = statement.query([]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            budget.check()?;
            let Ok(session_id) = row.get::<_, String>(0) else {
                continue;
            };
            let Ok(data) = row.get::<_, String>(1) else {
                continue;
            };
            let Some(content) = serde_json::from_str::<Value>(&data)
                .ok()
                .and_then(|value| text(&value, "msg_content"))
            else {
                continue;
            };
            previews.entry(session_id).or_insert(content);
        }
    }
    if table_exists(connection, "local_runtime_messages") {
        let filter = if rows_exist {
            " AND NOT EXISTS (SELECT 1 FROM local_runtime_message_rows x \
             WHERE x.session_id = m.session_id)"
        } else {
            ""
        };
        let mut statement = connection
            .prepare(&format!(
                "SELECT m.session_id, je.value FROM local_runtime_messages m, \
                     json_each(m.display_messages_json) je \
                 WHERE json_extract(je.value, '$.role') = 'user' \
                 AND coalesce(json_extract(je.value, '$.msg_content'), '') <> ''{filter}"
            ))
            .map_err(|error| error.to_string())?;
        let mut rows = statement.query([]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            budget.check()?;
            let Ok(session_id) = row.get::<_, String>(0) else {
                continue;
            };
            if previews.contains_key(&session_id) {
                continue;
            }
            let Ok(data) = row.get::<_, String>(1) else {
                continue;
            };
            let Some(content) = serde_json::from_str::<Value>(&data)
                .ok()
                .and_then(|value| text(&value, "msg_content"))
            else {
                continue;
            };
            previews.insert(session_id, content);
        }
    }
    Ok(previews)
}

fn column_text(row: &rusqlite::Row<'_>, column: &str) -> Option<String> {
    row.get::<_, Option<String>>(column)
        .ok()
        .flatten()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn model_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let label = trimmed.rsplit('/').next().unwrap_or(trimmed).trim();
    (!label.is_empty()).then(|| label.to_owned())
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开 MiniMax Code 会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
        .ok()
        .and_then(|mut statement| {
            statement
                .query_row([table], |row| row.get::<_, i64>(0))
                .ok()
        })
        .is_some()
}
