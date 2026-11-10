use super::*;
use crate::services::agent_cli::contracts::SessionReadBudget;
use serde_json::json;
use std::{
    io::Write,
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::{Duration, Instant},
};

fn test_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "balancehub-qoder-session-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("projects/-work-proj")).unwrap();
    root
}

fn budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
        32 * 1024 * 1024,
    )
}

fn write_session(root: &Path, id: &str, lines: &[Value]) {
    let path = root.join(format!("projects/-work-proj/{id}.jsonl"));
    let mut file = fs::File::create(path).unwrap();
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
    }
}

fn session_lines() -> Vec<Value> {
    vec![
        json!({
            "type": "user",
            "sessionId": "sess-1",
            "uuid": "u1",
            "cwd": "/work/proj",
            "timestamp": "2026-11-09T10:00:00Z",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }
        }),
        json!({
            "type": "assistant",
            "sessionId": "sess-1",
            "uuid": "u2",
            "cwd": "/work/proj",
            "timestamp": "2026-11-09T10:00:05Z",
            "message": {
                "role": "assistant",
                "model": "qwen3-coder",
                "content": [
                    { "type": "thinking", "thinking": "Look." },
                    { "type": "text", "text": "Reading it." },
                    {
                        "type": "tool_use",
                        "id": "call-1",
                        "name": "Read",
                        "input": { "file_path": "README.md" }
                    }
                ]
            }
        }),
    ]
}

#[test]
fn scan_reads_claude_compatible_jsonl() {
    let root = test_root("scan");
    write_session(&root, "sess-1", &session_lines());
    let scan = (super::HISTORY.scan)(
        AgentCliKind::Qoder,
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
    let summary = &summaries[0];
    assert_eq!(summary.id, "sess-1");
    assert_eq!(summary.preview.as_deref(), Some("hello"));
    assert_eq!(summary.model.as_deref(), Some("qwen3-coder"));
    assert_eq!(summary.workdir, "/work/proj");
    assert_eq!(summary.metadata_source, "qoderTranscript");
    assert!(summary.can_resume);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn decoder_reuses_claude_blocks_with_qoder_ids() {
    let root = test_root("decoder");
    let mut ids = Vec::new();
    for (index, line) in session_lines().into_iter().enumerate() {
        let encoded = serde_json::to_vec(&line).unwrap();
        let mutations = super::decoder::decode(
            &root,
            index as u64,
            &encoded,
            &mut Value::Null,
        );
        for mutation in mutations {
            if let crate::services::agent_cli::contracts::SessionTranscriptMutation::Put {
                message,
                ..
            } = mutation
            {
                ids.push(message.id);
            }
        }
    }
    assert!(ids.contains(&"qoder-u1-0".to_string()));
    assert!(ids.contains(&"qoder-u2-1".to_string()));
    assert!(ids.iter().any(|id| id.starts_with("qoder-u2-2")));
    let _ = fs::remove_dir_all(root);
}
