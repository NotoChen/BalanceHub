use super::*;
use crate::{
    models::AgentCliKind,
    services::agent_cli::contracts::{SessionHistoryReadMode, SessionReadBudget},
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::AtomicBool,
    },
    time::{Duration, Instant},
};

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-atomcode-session-{name}-{}",
        std::process::id()
    ))
}

fn budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        32 * 1024 * 1024,
    )
}

fn write_session(root: &std::path::Path, events: Option<&str>) {
    let directory = root.join("sessions/project-hash");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("session-1.index"),
        json!({
            "v": 1,
            "id": "session-1",
            "name": "修复登录",
            "user_renamed": false,
            "ai_named": false,
            "owner": "native",
            "working_dir": root.join("workspace").to_string_lossy(),
            "created_at": 1_794_217_416_512i64,
            "updated_at": 1_794_217_436_000i64,
            "turn_count": 1,
            "message_count": 3,
        })
        .to_string(),
    )
    .unwrap();
    if let Some(events) = events {
        fs::write(directory.join("session-1.events"), events).unwrap();
    }
}

fn events() -> String {
    [
        json!({"header":{"version":15,"id":"session-1","cwd":"/tmp/workspace"}}),
        json!({"seq":2,"at":1_794_217_416_702i64,"event":{"kind":"user_message","turn":1,"text":"修复登录"}}),
        json!({"seq":3,"at":1_794_217_416_703i64,"event":{"kind":"assistant_message","turn":1,"round":1,"text":"","tool_calls":[{"id":"call_1","name":"bash","arguments":"{\"command\":\"echo hi\"}"}],"meta":{"provider_model":"gpt-test"}}}),
        json!({"seq":4,"at":1_794_217_416_704i64,"event":{"kind":"tool_result_logged","turn":1,"round":1,"call_id":"call_1","content":"hi","is_error":false}}),
        json!({"seq":5,"at":1_794_217_416_705i64,"event":{"kind":"assistant_message","turn":1,"round":2,"text":"已修复","meta":{"provider_model":"gpt-test"}}}),
        json!({"seq":6,"at":1_794_217_416_706i64,"event":{"kind":"turn_end","turn":1,"stop":"Stopped"}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n")
}

fn scan_summaries(root: &std::path::Path) -> SessionHistoryScan {
    let budget = budget();
    let scan = crate::services::cli_sessions::workbench::with_read_budget(&budget, || {
        super::scan(
            AgentCliKind::AtomCode,
            root,
            &[root.join("workspace")],
            &budget,
            SessionHistoryReadMode::Summaries,
        )
    })
    .unwrap();
    assert_eq!(scan.workspaces.len(), 1);
    scan
}

#[test]
fn index_and_events_drive_the_session_summary() {
    let root = test_root("summary");
    let _ = fs::remove_dir_all(&root);
    write_session(&root, Some(&events()));
    let scan = scan_summaries(&root);
    assert!(scan.diagnostics.is_empty());
    let records = &scan.workspaces[0].records;
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.record_key, "session-1");
    assert_eq!(record.summary.title, "修复登录");
    assert_eq!(record.summary.preview.as_deref(), Some("修复登录"));
    assert!(record.summary.can_resume);
    assert!(record.resume_reason.is_none());
    assert!(record.content_unavailable_reason.is_none());
    assert!(record.summary.created_at.is_some());
    assert!(record.summary.updated_at.is_some());
    assert!(record.locator.ends_with("session-1.events"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn events_decode_into_messages_and_tool_rows() {
    let root = test_root("decode");
    let _ = fs::remove_dir_all(&root);
    write_session(&root, Some(&events()));
    let scan = scan_summaries(&root);
    let record = &scan.workspaces[0].records[0];
    let sources = transcript(record).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].format, SessionTranscriptFormat::Lines);
    let mut rows = Vec::new();
    for (sequence, line) in events().lines().enumerate() {
        rows.extend((sources[0].decode)(
            &sources[0].path,
            sequence as u64,
            line.as_bytes(),
            &mut Value::Null,
        ));
    }
    let messages: Vec<_> = rows
        .into_iter()
        .map(|mutation| match mutation {
            crate::services::agent_cli::contracts::SessionTranscriptMutation::Put { message, .. } => {
                message
            }
            _ => panic!("unexpected mutation"),        })
        .collect();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, crate::models::CliSessionMessageRole::User);
    assert_eq!(messages[0].content, "修复登录");
    assert_eq!(
        messages[1].metadata.kind,
        crate::models::SessionMessageKind::ToolCall
    );
    assert_eq!(messages[1].metadata.tool_name.as_deref(), Some("bash"));
    assert_eq!(
        messages[2].metadata.kind,
        crate::models::SessionMessageKind::ToolResult
    );
    assert_eq!(
        messages[2].metadata.status,
        Some(crate::models::SessionToolStatus::Succeeded)
    );
    assert_eq!(messages[3].content, "已修复");
    assert_eq!(messages[3].metadata.model.as_deref(), Some("gpt-test"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_event_log_keeps_the_session_listable_but_unreadable() {
    let root = test_root("missing-events");
    let _ = fs::remove_dir_all(&root);
    write_session(&root, None);
    let scan = scan_summaries(&root);
    let record = &scan.workspaces[0].records[0];
    assert!(record.summary.can_resume);
    assert!(record.content_unavailable_reason.is_some());
    assert!(transcript(record).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn identity_scan_reads_only_the_index() {
    let root = test_root("identity");
    let _ = fs::remove_dir_all(&root);
    write_session(&root, Some(&events()));
    let budget = budget();
    let scan = crate::services::cli_sessions::workbench::with_read_budget(&budget, || {
        super::scan(
            AgentCliKind::AtomCode,
            &root,
            &[root.join("workspace")],
            &budget,
            SessionHistoryReadMode::Identities,
        )
    })
    .unwrap();
    let record = &scan.workspaces[0].records[0];
    assert_eq!(record.summary.id, "session-1");
    assert!(record.locator.ends_with("session-1.events"));
    let _ = fs::remove_dir_all(&root);
}
