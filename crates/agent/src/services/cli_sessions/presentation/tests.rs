use super::*;
use crate::services::agent_cli::{self, contracts::SessionHistoryRecord};
use serde_json::json;

#[test]
fn every_native_decoder_preserves_failed_tool_status_and_exit_code() {
    for &kind in AgentCliKind::ALL {
        let root = tempfile::tempdir().unwrap();
        let output = json!({"exit_code":2,"output":"fixture command failed"});
        let (filename, lines) = match kind {
            AgentCliKind::Codex => (
                "session.jsonl",
                vec![
                    json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"call-1","arguments":"{\"cmd\":\"fixture-command\"}"}}),
                    json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":output.to_string()}}),
                ],
            ),
            AgentCliKind::ClaudeCode => (
                "session.jsonl",
                vec![
                    json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"call-1","name":"Bash","input":{"command":"fixture-command"}}]}}),
                    json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call-1","content":output.to_string()}]}}),
                ],
            ),
            AgentCliKind::Gemini => (
                "session.jsonl",
                vec![
                    json!({"id":"message-1","type":"gemini","content":"checking","toolCalls":[{"id":"call-1","name":"run_shell_command","args":{"command":"fixture-command"},"result":output}]}),
                ],
            ),
            AgentCliKind::Grok => (
                "chat_history.jsonl",
                vec![
                    json!({"type":"assistant","content":[{"type":"tool_call","id":"call-1","name":"run_shell_command","input":{"command":"fixture-command"},"result":output}]}),
                ],
            ),
        };
        std::fs::write(
            root.path().join(filename),
            lines
                .iter()
                .map(|line| line.to_string() + "\n")
                .collect::<String>(),
        )
        .unwrap();
        let locator = root.path().join(if kind == AgentCliKind::Grok {
            "summary.json"
        } else {
            filename
        });
        let record = SessionHistoryRecord::identity(kind, "fixture".into(), root.path(), locator);
        let sources = (agent_cli::definition(kind)
            .sessions()
            .unwrap()
            .history()
            .index)(&record)
        .unwrap();
        let messages = crate::services::cli_sessions::read_indexed_messages(&sources);
        let tools: Vec<_> = messages
            .iter()
            .filter(|message| {
                matches!(
                    message.metadata.kind,
                    SessionMessageKind::ToolCall | SessionMessageKind::ToolResult
                )
            })
            .collect();
        assert_eq!(tools.len(), 2, "{kind:?}");
        for message in tools {
            assert_eq!(
                message.metadata.status,
                Some(SessionToolStatus::Failed),
                "{kind:?}"
            );
            assert_eq!(message.metadata.exit_code, Some(2), "{kind:?}");
            assert_eq!(
                message.metadata.tool_kind,
                Some(SessionToolKind::Shell),
                "{kind:?}"
            );
        }
    }
}

#[test]
fn tool_status_uses_explicit_facts_and_keeps_unknown_results_unknown() {
    for value in [
        json!("finished reading"),
        json!({"output":"done"}),
        json!({"error":false}),
    ] {
        assert_eq!(tool_result(&value, None).status, None);
    }
    assert_eq!(
        tool_result(
            &json!({"status":"completed","result":{"isError":true}}),
            None
        )
        .status,
        Some(SessionToolStatus::Failed)
    );
    assert_eq!(
        tool_result(&json!({"status":"cancelled"}), None).status,
        Some(SessionToolStatus::Cancelled)
    );
    assert_eq!(
        tool_result(&json!({"status":"pending"}), None).status,
        Some(SessionToolStatus::Running)
    );
    assert_eq!(
        tool_result(&json!("Process exited with code 0\nOutput:\nfixture"), None).status,
        Some(SessionToolStatus::Succeeded)
    );
}

#[test]
fn native_patch_events_preserve_add_delete_and_update_contents() {
    let metadata = tool_call("apply_patch", &Value::Null, None);
    let content = json!({"changes":{
        "added.rs":{"type":"add","content":"new\n"},
        "deleted.rs":{"type":"delete","content":"old\n"},
        "updated.rs":{"type":"update","unified_diff":"@@ -3 +3 @@\n-old\n+new\n"}
    }})
    .to_string();
    let changes = file_changes(&metadata, &content);
    assert_eq!(changes.len(), 3);
    let added = changes
        .iter()
        .find(|change| change.path == "added.rs")
        .unwrap();
    assert!(added.before_known && added.before.is_empty());
    assert_eq!(added.after, "new\n");
    let deleted = changes
        .iter()
        .find(|change| change.path == "deleted.rs")
        .unwrap();
    assert!(deleted.before_known && deleted.after.is_empty());
    assert_eq!(deleted.before, "old\n");
    assert_eq!(
        changes
            .iter()
            .find(|change| change.path == "updated.rs")
            .unwrap()
            .patch
            .as_deref(),
        Some("@@ -3 +3 @@\n-old\n+new\n")
    );
}

#[test]
fn edit_and_write_records_do_not_invent_missing_original_content() {
    let edit = tool_call("Edit", &Value::Null, None);
    let changes = file_changes(
        &edit,
        &json!({"file_path":"file.rs","old_string":"before","new_string":"after"}).to_string(),
    );
    assert_eq!(changes[0].before, "before");
    assert_eq!(changes[0].after, "after");
    assert!(changes[0].before_known);
    let write = tool_call("write_file", &Value::Null, None);
    let changes = file_changes(
        &write,
        &json!({"file_path":"file.rs","content":"written"}).to_string(),
    );
    assert_eq!(changes[0].after, "written");
    assert!(!changes[0].before_known);
}
