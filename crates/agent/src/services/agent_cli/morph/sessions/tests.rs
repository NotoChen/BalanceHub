use super::*;
use crate::services::agent_cli::contracts::SessionReadBudget;
use flate2::{write::DeflateEncoder, Compression};
use serde_json::json;
use std::{
    fs,
    io::Write,
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{Duration, Instant},
};

fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "balancehub-morph-session-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("journal")).unwrap();
    root
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn compressed(payload: &Value) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(serde_json::to_string(payload).unwrap().as_bytes())
        .unwrap();
    encoder.finish().unwrap()
}

struct Event<'a> {
    event_type: &'a str,
    task_id: &'a str,
    topic_id: &'a str,
    payload: Value,
}

fn write_journal(root: &Path, events: &[Event<'_>]) {
    let connection = Connection::open(root.join("journal/journal.db")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE journal_events (
                seq INTEGER PRIMARY KEY,
                id TEXT NOT NULL,
                time TEXT NOT NULL,
                at_ms INTEGER NOT NULL,
                domain TEXT NOT NULL,
                type TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                trace_id TEXT NOT NULL DEFAULT '',
                runtime TEXT NOT NULL DEFAULT '',
                target TEXT NOT NULL DEFAULT '',
                topic_id TEXT NOT NULL DEFAULT '',
                task_id TEXT NOT NULL DEFAULT '',
                audience TEXT NOT NULL DEFAULT '',
                text TEXT NOT NULL DEFAULT '',
                payload BLOB NOT NULL
            );",
        )
        .unwrap();
    for (index, event) in events.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO journal_events
                 (seq, id, time, at_ms, domain, type, schema_version, topic_id, task_id, payload)
                 VALUES (?1, ?2, ?3, ?4, 'task', ?5, 1, ?6, ?7, ?8)",
                rusqlite::params![
                    index as i64 + 1,
                    format!("evt_{index}"),
                    "2026-11-09T10:00:00Z",
                    1_791_000_000_000i64 + index as i64,
                    event.event_type,
                    event.topic_id,
                    event.task_id,
                    compressed(&event.payload),
                ],
            )
            .unwrap();
    }
}

fn budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        32 * 1024 * 1024,
    )
}

fn task_payload(topic: &str, status: &str, output: Option<&str>) -> Value {
    json!({
        "at": "2026-11-09T10:00:02Z",
        "target": "console",
        "task": {
            "id": "task_1",
            "status": status,
            "task": "Hello Morph",
            "model": "gpt-test",
            "created_at": "2026-11-09T10:00:00Z",
            "finished_at": "2026-11-09T10:00:02Z",
            "topic_id": topic,
            "result": output.map(|output| json!({
                "final": { "output": output, "reasoning": "Read the message." },
                "trace": { "entries": [
                    {
                        "seq": 1,
                        "at": "2026-11-09T10:00:01Z",
                        "event": {
                            "kind": "tool_start",
                            "run_id": "run_1",
                            "tool_name": "read_file",
                            "activity_id": "act_1",
                            "args": { "path": "README.md" }
                        }
                    },
                    {
                        "seq": 2,
                        "at": "2026-11-09T10:00:02Z",
                        "event": {
                            "kind": "tool_done",
                            "run_id": "run_1",
                            "tool_name": "read_file",
                            "activity_id": "act_1",
                            "status": "completed",
                            "text": "read ok"
                        }
                    }
                ]}
            })),
        },
        "topic": {
            "id": topic,
            "title": "Greeting",
            "created_at": "2026-11-09T09:59:59Z",
            "updated_at": "2026-11-09T10:00:02Z"
        }
    })
}

fn summaries(root: &Path) -> Vec<CliSessionSummary> {
    let scan = (super::HISTORY.scan)(
        AgentCliKind::Morph,
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
fn scan_lists_topics_and_skips_internal_or_deleted_ones() {
    let root = test_root("scan");
    write_journal(
        &root,
        &[
            Event {
                event_type: "task_upsert",
                task_id: "task_1",
                topic_id: "topic_1",
                payload: task_payload("topic_1", "done", Some("Hi")),
            },
            Event {
                event_type: "task_upsert",
                task_id: "task_2",
                topic_id: "_cron_daily",
                payload: task_payload("_cron_daily", "done", Some("no")),
            },
            Event {
                event_type: "task_upsert",
                task_id: "task_3",
                topic_id: "topic_deleted",
                payload: task_payload("topic_deleted", "done", Some("old")),
            },
            Event {
                event_type: "topic_deleted",
                task_id: "",
                topic_id: "topic_deleted",
                payload: json!({
                    "at": "2026-11-09T10:00:03Z",
                    "topic": {
                        "id": "topic_deleted",
                        "title": "Old",
                        "created_at": "2026-11-09T09:00:00Z",
                        "updated_at": "2026-11-09T10:00:03Z",
                        "deleted_at": "2026-11-09T10:00:03Z"
                    }
                }),
            },
        ],
    );
    let summaries = summaries(&root);
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.id, "topic_1");
    assert_eq!(summary.title, "Greeting");
    assert_eq!(summary.preview.as_deref(), Some("Hello Morph"));
    assert_eq!(summary.model.as_deref(), Some("gpt-test"));
    assert!(!summary.can_resume);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn decoder_emits_user_trace_and_answer() {
    let root = test_root("decoder");
    let payload = task_payload("topic_1", "done", Some("Hi"));
    let encoded = hex_encode(&compressed(&payload));
    let mutations = super::decoder::decode(
        &root,
        0,
        encoded.as_bytes(),
        &mut Value::Null,
    );
    let ids = mutations
        .iter()
        .filter_map(|mutation| match mutation {
            crate::services::agent_cli::contracts::SessionTranscriptMutation::Put {
                message,
                ..
            } => Some(message.id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(ids.contains(&"morph-task_1:user"));
    assert!(ids.contains(&"morph-task_1:answer-thinking"));
    assert!(ids.contains(&"morph-task_1:answer"));
    assert!(ids.contains(&"morph-call-act_1"));
    assert!(ids.contains(&"morph-result-act_1"));
    let _ = fs::remove_dir_all(root);
}
