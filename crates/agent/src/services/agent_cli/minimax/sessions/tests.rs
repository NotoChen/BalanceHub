use super::*;
use crate::{
    models::SessionToolStatus,
    services::agent_cli::contracts::{
        SessionHistoryReadMode, SessionReadBudget, SessionTranscriptMutation,
    },
};
use serde_json::json;
use std::{
    fs,
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{Duration, Instant},
};

fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "balancehub-minimax-session-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("v2/sqlite")).unwrap();
    root
}

fn budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        32 * 1024 * 1024,
    )
}

fn write_database(root: &Path, sessions: &[Value], messages: &[Value]) {
    let connection = Connection::open(root.join("v2/sqlite/runtime-state.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE local_runtime_sessions (
                session_id TEXT PRIMARY KEY,
                record_json TEXT,
                visibility TEXT,
                session_kind TEXT,
                updated_at_ms INTEGER,
                created_at_ms INTEGER,
                workspace_dir TEXT,
                title TEXT,
                archived INTEGER,
                parent_session_id TEXT,
                extra_data_json TEXT
            );
            CREATE TABLE local_runtime_message_rows (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT,
                data_json TEXT NOT NULL
            );
            CREATE TABLE local_runtime_messages (
                session_id TEXT PRIMARY KEY,
                display_messages_json TEXT
            );",
        )
        .unwrap();
    for session in sessions {
        connection
            .execute(
                "INSERT INTO local_runtime_sessions
                 (session_id, record_json, visibility, session_kind, updated_at_ms,
                  created_at_ms, workspace_dir, title, archived, parent_session_id,
                  extra_data_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    session["session_id"].as_str().unwrap(),
                    session.get("record_json").map(Value::to_string),
                    session.get("visibility").and_then(Value::as_str),
                    session.get("session_kind").and_then(Value::as_str),
                    session.get("updated_at_ms").and_then(Value::as_i64),
                    session.get("created_at_ms").and_then(Value::as_i64),
                    session.get("workspace_dir").and_then(Value::as_str),
                    session.get("title").and_then(Value::as_str),
                    session.get("archived").and_then(Value::as_i64),
                    session.get("parent_session_id").and_then(Value::as_str),
                    session.get("extra_data_json").map(Value::to_string),
                ],
            )
            .unwrap();
    }
    for message in messages {
        connection
            .execute(
                "INSERT INTO local_runtime_message_rows (id, session_id, role, data_json)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    message["id"].as_i64().unwrap(),
                    message["session_id"].as_str().unwrap(),
                    message["role"].as_str().unwrap(),
                    message["data"].to_string(),
                ],
            )
            .unwrap();
    }
}

fn user_row(id: i64, session_id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "session_id": session_id,
        "role": "user",
        "data": {
            "msg_id": format!("u{id}"),
            "role": "user",
            "msg_content": text,
            "timestamp": 1_791_000_000_000i64,
            "turnId": "turn-1"
        }
    })
}

fn summaries(root: &Path) -> Vec<CliSessionSummary> {
    let scan = (super::HISTORY.scan)(
        AgentCliKind::MiniMaxCode,
        root,
        &[],
        &budget(),
        SessionHistoryReadMode::Summaries,
    )
    .unwrap();
    scan.workspaces
        .into_iter()
        .flat_map(|workspace| workspace.records)
        .map(|record| record.summary)
        .collect()
}

#[test]
fn scan_lists_visible_sessions_and_skips_hidden_peek_and_cron() {
    let root = test_root("scan");
    write_database(
        &root,
        &[
            json!({
                "session_id": "sess-1",
                "record_json": {
                    "title": "会话标题",
                    "lastAssistantText": "模型最终回答",
                    "workspaceDir": "/work/proj"
                },
                "updated_at_ms": 1_791_000_000_000i64,
                "created_at_ms": 1_790_999_000_000i64,
                "extra_data_json": { "effectiveModel": "minimax/MiniMax-M2" }
            }),
            json!({
                "session_id": "sess-hidden",
                "visibility": "hidden",
                "workspace_dir": "/work/proj"
            }),
            json!({
                "session_id": "sess-peek",
                "session_kind": "peek",
                "workspace_dir": "/work/proj"
            }),
            json!({
                "session_id": "sess-cron",
                "session_kind": "cron",
                "workspace_dir": "/work/proj"
            }),
            json!({
                "session_id": "sess-sub",
                "workspace_dir": "/work/proj",
                "parent_session_id": "sess-1"
            }),
        ],
        &[user_row(1, "sess-1", "请检查构建")],
    );
    let summaries = summaries(&root);
    assert_eq!(summaries.len(), 2);
    let summary = summaries
        .iter()
        .find(|summary| summary.id == "sess-1")
        .unwrap();
    assert_eq!(summary.title, "会话标题");
    assert_eq!(summary.preview.as_deref(), Some("请检查构建"));
    assert_eq!(summary.model.as_deref(), Some("MiniMax-M2"));
    assert_eq!(summary.workdir, "/work/proj");
    assert_eq!(summary.metadata_source, METADATA_SOURCE);
    assert!(summary.can_resume);
    let created_at = chrono::DateTime::from_timestamp_millis(1_790_999_000_000)
        .unwrap()
        .to_rfc3339();
    assert_eq!(summary.created_at.as_deref(), Some(created_at.as_str()));
    let subagent = summaries
        .iter()
        .find(|summary| summary.id == "sess-sub")
        .unwrap();
    assert_eq!(subagent.title, "proj");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scan_identity_mode_only_reads_ids_and_workdirs() {
    let root = test_root("identities");
    write_database(
        &root,
        &[json!({
            "session_id": "sess-1",
            "workspace_dir": "/work/proj",
            "updated_at_ms": 1_791_000_000_000i64
        })],
        &[],
    );
    let scan = (super::HISTORY.scan)(
        AgentCliKind::MiniMaxCode,
        &root,
        &[],
        &budget(),
        SessionHistoryReadMode::Identities,
    )
    .unwrap();
    let records = scan
        .workspaces
        .into_iter()
        .flat_map(|workspace| workspace.records)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_key, "sess-1");
    assert_eq!(records[0].summary.id, "sess-1");
    assert_eq!(records[0].summary.workdir, "/work/proj");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn decoder_emits_text_thinking_tools_and_usage() {
    let line = json!({
        "msg_id": "a1",
        "role": "assistant",
        "thinking_content": "先看文件",
        "msg_content": "构建通过",
        "turnId": "turn-1",
        "context_usage_telemetry": { "model": "MiniMax-M2" },
        "usage": {
            "input_tokens": 120,
            "output_tokens": 30,
            "cache_read_input_tokens": 40,
            "reasoning_tokens": 5
        },
        "tool_calls": [{
            "tool_call_id": "call_1",
            "tool_name": "Bash",
            "tool_call_args": "{\"command\":\"npm run build\"}",
            "tool_call_status": 2,
            "tool_call_result_data": {
                "content": [{ "type": "text", "text": "build ok" }],
                "details": { "processOutput": { "exitCode": 0 } }
            }
        }]
    });
    let mutations = super::decoder::decode(
        Path::new("unused"),
        0,
        line.to_string().as_bytes(),
        &mut Value::Null,
    );
    let messages = mutations
        .into_iter()
        .filter_map(|mutation| match mutation {
            SessionTranscriptMutation::Put { message, .. } => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();
    let ids = messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "minimax-a1:thinking",
            "minimax-a1",
            "minimax-call-call_1",
            "minimax-result-call_1",
        ]
    );
    assert_eq!(messages[1].content, "构建通过");
    assert_eq!(messages[2].metadata.tool_name.as_deref(), Some("Bash"));
    assert_eq!(
        messages[2].metadata.status,
        Some(SessionToolStatus::Succeeded)
    );
    assert_eq!(messages[3].content, "build ok");
    assert_eq!(messages[3].metadata.exit_code, Some(0));
}

#[test]
fn transcript_query_falls_back_to_legacy_display_messages() {
    let root = test_root("legacy");
    let connection = Connection::open(root.join("v2/sqlite/runtime-state.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE local_runtime_messages (
                session_id TEXT PRIMARY KEY,
                display_messages_json TEXT
            );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO local_runtime_messages (session_id, display_messages_json)
             VALUES ('sess-1', ?1)",
            [json!([
                { "role": "user", "msg_content": "旧版消息" },
                { "role": "assistant", "msg_content": "旧版回答" }
            ])
            .to_string()],
        )
        .unwrap();
    let path = root.join("v2/sqlite/runtime-state.sqlite");
    assert_eq!(
        transcript_query(&path, "sess-1"),
        Some(LEGACY_TRANSCRIPT_QUERY)
    );
    // 读取端以 0 作为初始 offset 并跳过 `row_key <= offset`；旧版数组下标
    // 必须平移到 1 基，否则第一条消息会被当成已消费。
    let mut statement = connection.prepare(LEGACY_TRANSCRIPT_QUERY).unwrap();
    let rows = statement
        .query_map(rusqlite::params![0_i64, 2000_i64, "sess-1"], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 1);
    assert!(rows[0].1.contains("旧版消息"));
    assert_eq!(rows[1].0, 2);
    let keys = statement
        .query_map(rusqlite::params![1_i64, 2000_i64, "sess-1"], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(keys, vec![2]);
    drop(statement);
    drop(connection);
    let _ = fs::remove_dir_all(root);
}
