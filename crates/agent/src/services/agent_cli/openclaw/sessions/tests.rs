use super::{runtime, scan};
use crate::{
    models::AgentCliKind,
    services::agent_cli::contracts::{SessionHistoryReadMode, SessionReadBudget},
};
use rusqlite::{params, Connection};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAIN_ID: &str = "11111111-1111-4111-8111-111111111111";
const SUB_ID: &str = "22222222-2222-4222-8222-222222222222";

fn temp_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "balancehub-openclaw-sessions-{}-{stamp}",
        std::process::id()
    ))
}

fn budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        8 * 1024 * 1024,
    )
}

fn workdir() -> String {
    std::env::temp_dir()
        .join("openclaw-work")
        .to_string_lossy()
        .into_owned()
}

fn database(root: &Path) -> PathBuf {
    let directory = root.join("agents/main/agent");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("openclaw-agent.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session_nodes (
                session_key TEXT PRIMARY KEY,
                current_session_id TEXT NOT NULL,
                entry_json TEXT NOT NULL,
                display_name TEXT,
                label TEXT,
                parent_session_key TEXT,
                spawned_by TEXT
            );
            CREATE TABLE session_windows (
                session_id TEXT PRIMARY KEY,
                session_key TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                model TEXT
            );
            CREATE TABLE transcript_events (
                session_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                event_json TEXT,
                event_zstd BLOB,
                message_role TEXT,
                PRIMARY KEY (session_id, seq)
            );",
        )
        .unwrap();
    for (key, id, parent) in [
        ("agent:main:main", MAIN_ID, None),
        ("agent:main:sub", SUB_ID, Some("agent:main:main")),
    ] {
        connection
            .execute(
                "INSERT INTO session_nodes
                 (session_key, current_session_id, entry_json, parent_session_key)
                 VALUES (?1, ?2, '{}', ?3)",
                params![key, id, parent],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_windows
                 (session_id, session_key, created_at, updated_at, model)
                 VALUES (?1, ?2, 1700000000000, 1700000001000, 'gpt-test')",
                params![id, key],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO transcript_events (session_id, seq, event_json, message_role)
                 VALUES (?1, 0, ?2, NULL)",
                params![
                    id,
                    serde_json::json!({
                        "type": "session",
                        "id": id,
                        "cwd": workdir(),
                    })
                    .to_string()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO transcript_events (session_id, seq, event_json, message_role)
                 VALUES (?1, 1, ?2, 'user')",
                params![
                    id,
                    format!(
                        r#"{{"type":"message","message":{{"role":"user","content":"hello {key}"}}}}"#
                    )
                ],
            )
            .unwrap();
    }
    path
}

#[test]
fn sqlite_scan_reads_session_windows_and_skips_subagent_resume() {
    let root = temp_root();
    database(&root);
    let result = scan(
        AgentCliKind::OpenClaw,
        &root,
        &[],
        &budget(),
        SessionHistoryReadMode::Summaries,
    )
    .unwrap();
    let records: Vec<_> = result
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.records.iter())
        .collect();
    assert_eq!(records.len(), 2);
    let main = records
        .iter()
        .find(|record| record.summary.id == "agent:main:main")
        .unwrap();
    assert_eq!(main.record_key, MAIN_ID);
    assert_eq!(main.summary.preview.as_deref(), Some("hello agent:main:main"));
    assert_eq!(main.summary.workdir, workdir());
    assert!(main.summary.can_resume);
    assert!(main.resume_reason.is_none());
    let sub = records
        .iter()
        .find(|record| record.summary.id == "agent:main:sub")
        .unwrap();
    assert!(!sub.summary.can_resume);
    assert!(sub
        .resume_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("子代理")));
}

#[test]
fn sqlite_transcript_targets_the_session_row_and_decodes_compressed_events() {
    let root = temp_root();
    let path = database(&root);
    let result = scan(
        AgentCliKind::OpenClaw,
        &root,
        &[],
        &budget(),
        SessionHistoryReadMode::Summaries,
    )
    .unwrap();
    let record = result
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.records.iter())
        .find(|record| record.summary.id == "agent:main:main")
        .unwrap();
    let sources = runtime::transcript(record).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path, path);
    let sqlite = sources[0].sqlite.as_ref().unwrap();
    assert_eq!(sqlite.scope, MAIN_ID);

    let event = br#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"compressed"}]}}"#;
    let compressed = zstd::stream::encode_all(&event[..], 1).unwrap();
    let hex: String = compressed.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut state = serde_json::Value::Null;
    let mutations = runtime::decode(
        Path::new("openclaw-agent.sqlite"),
        0,
        hex.as_bytes(),
        &mut state,
    );
    assert!(!mutations.is_empty());
}
