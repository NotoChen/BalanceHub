use super::*;
use crate::services::agent_cli::contracts::SessionReadBudget;
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
        "balancehub-droid-session-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sessions/-work-proj")).unwrap();
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
    let path = root.join(format!("sessions/-work-proj/{id}.jsonl"));
    let mut file = fs::File::create(path).unwrap();
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(line).unwrap()).unwrap();
    }
}

fn session_lines() -> Vec<Value> {
    vec![
        json!({
            "type": "session_start",
            "version": 2,
            "id": "sess-1",
            "title": "Fix parser",
            "cwd": "/work/proj",
            "parent": null
        }),
        json!({
            "type": "message",
            "id": "m1",
            "parentId": null,
            "timestamp": "2026-11-09T10:00:00Z",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }
        }),
        json!({
            "type": "message",
            "id": "m2",
            "parentId": "m1",
            "timestamp": "2026-11-09T10:00:05Z",
            "message": {
                "role": "assistant",
                "modelId": "claude-sonnet-4-5",
                "content": [
                    { "type": "thinking", "thinking": "Let me look." },
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
        json!({
            "type": "message",
            "id": "m3",
            "parentId": "m2",
            "timestamp": "2026-11-09T10:00:06Z",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call-1",
                    "content": [{ "type": "text", "text": "file body" }]
                }]
            }
        }),
        json!({
            "type": "compaction_state",
            "id": "compact-1",
            "timestamp": "2026-11-09T10:01:00Z",
            "summaryText": "Compacted early turns",
            "removedMessageIds": ["m1"]
        }),
    ]
}

#[test]
fn scan_reads_session_start_and_latest_model() {
    let root = test_root("scan");
    write_session(&root, "sess-1", &session_lines());
    let scan = (super::HISTORY.scan)(
        AgentCliKind::Droid,
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
    assert_eq!(summary.title, "Fix parser");
    assert_eq!(summary.preview.as_deref(), Some("hello"));
    assert_eq!(summary.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(summary.workdir, "/work/proj");
    assert_eq!(summary.created_at.as_deref(), Some("2026-11-09T10:00:00+00:00"));
    assert_eq!(summary.updated_at.as_deref(), Some("2026-11-09T10:00:06+00:00"));
    assert!(summary.can_resume);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn decoder_reads_session_token_totals_from_settings() {
    let root = test_root("usage");
    write_session(&root, "sess-1", &session_lines());
    let path = root.join("sessions/-work-proj/sess-1.jsonl");
    fs::write(
        root.join("sessions/-work-proj/sess-1.settings.json"),
        serde_json::to_string(&json!({
            "model": "claude-sonnet-4-5",
            "tokenUsage": {
                "inputTokens": 100,
                "outputTokens": 20,
                "cacheReadTokens": 10,
                "cacheCreationTokens": 5,
                "thinkingTokens": 3
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let line = serde_json::to_vec(&session_lines()[1]).unwrap();
    let mutations = super::decoder::decode(&path, 1, &line, &mut Value::Null);
    let usage = mutations
        .iter()
        .find_map(|mutation| match mutation {
            crate::services::agent_cli::contracts::SessionTranscriptMutation::Usage(record) => {
                Some(record)
            }
            _ => None,
        })
        .expect("usage record");
    assert_eq!(usage.tokens.input_tokens, Some(115));
    assert_eq!(usage.tokens.output_tokens, Some(20));
    assert_eq!(usage.tokens.cache_read_tokens, Some(10));
    assert_eq!(usage.tokens.cache_write_tokens, Some(5));
    assert_eq!(usage.tokens.reasoning_tokens, Some(3));
    assert_eq!(usage.model.as_deref(), Some("claude-sonnet-4-5"));
    assert!(usage.cumulative);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn decoder_emits_blocks_tools_and_compaction() {
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
    assert!(ids.contains(&"droid-m1:0".to_string()));
    assert!(ids.contains(&"droid-m2:0".to_string()));
    assert!(ids.contains(&"droid-m2:1".to_string()));
    assert!(ids.contains(&"droid-call-call-1".to_string()));
    assert!(ids.contains(&"droid-result-call-1".to_string()));
    assert!(ids.contains(&"droid-compaction-compact-1".to_string()));
    let _ = fs::remove_dir_all(root);
}
