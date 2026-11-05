use super::{test_support::read_indexed_messages, *};
use crate::services::agent_cli::contracts::{SessionSearchTerm, SessionTranscriptFormat};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

fn decode(_: &Path, _: u64, bytes: &[u8], _: &mut Value) -> Vec<SessionTranscriptMutation> {
    let value: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    let priority = value["priority"].as_i64().unwrap_or(0);
    let message = || SessionTranscriptMessage {
        id: text("id"),
        role: match value["role"].as_str().unwrap_or_default() {
            "assistant" => CliSessionMessageRole::Assistant,
            "tool" => CliSessionMessageRole::Tool,
            _ => CliSessionMessageRole::User,
        },
        content: text("content"),
        metadata: serde_json::from_value(
            value.get("metadata").cloned().unwrap_or_else(|| json!({})),
        )
        .unwrap_or_default(),
    };
    match value["op"].as_str().unwrap_or_default() {
        "put" => vec![SessionTranscriptMutation::Put {
            message: message(),
            priority,
        }],
        "append" => vec![SessionTranscriptMutation::Append {
            message: message(),
            priority,
        }],
        "clear" => vec![SessionTranscriptMutation::Clear],
        "rewind" => vec![SessionTranscriptMutation::Rewind(text("id"))],
        "removeGroup" => vec![SessionTranscriptMutation::RemoveGroup(text("id"))],
        "usage" => vec![SessionTranscriptMutation::Usage(SessionUsageRecord {
            id: text("id"),
            message_id: value["message"].as_str().map(str::to_string),
            model: value["model"].as_str().map(str::to_string),
            tokens: SessionTokenCounts {
                input_tokens: value["input"].as_u64(),
                output_tokens: value["output"].as_u64(),
                ..Default::default()
            },
            cumulative: false,
            uncertain: false,
        })],
        _ => Vec::new(),
    }
}

fn source(path: &Path) -> SessionTranscriptSource {
    SessionTranscriptSource {
        path: path.to_path_buf(),
        parser_version: 1,
        format: SessionTranscriptFormat::JsonLines,
        decode,
    }
}

fn fixture(name: &str, lines: &[Value]) -> (tempfile::TempDir, SessionTranscriptSource) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(format!("{name}.jsonl"));
    fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    (root, source(&path))
}

fn read(sources: &[SessionTranscriptSource]) -> Vec<SessionTranscriptMessage> {
    read_indexed_messages(sources)
}

fn budget(bytes: u64) -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
        bytes,
    )
}

fn summary() -> CliSessionSummary {
    CliSessionSummary {
        id: "session".into(),
        title: "fixture".into(),
        preview: None,
        model: None,
        models: Vec::new(),
        cli_kind: AgentCliKind::Codex,
        created_at: None,
        updated_at: None,
        workdir: String::new(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: "test".into(),
    }
}

fn page_request() -> AgentSessionDetailRequest {
    AgentSessionDetailRequest {
        consumer_id: "test".into(),
        request_id: 1,
        scope_revision: "scope".into(),
        session_ref: "ref".into(),
        cursor: None,
        anchor: None,
        query: String::new(),
        match_position: None,
    }
}

#[test]
fn every_source_is_merged_in_order_and_hidden_groups_stay_hidden() {
    let (_root, source) = fixture(
        "merge",
        &[
            json!({"op":"put", "id":"m1", "content":"first", "priority":0}),
            json!({"op":"put", "id":"m2", "content":"hidden fallback", "priority":-1}),
            json!({"op":"put", "id":"m3", "content":"second", "priority":0}),
            json!({"op":"put", "id":"m4", "content":"hidden negative", "priority":-2}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["m1", "m2", "m3"]
    );
}

#[test]
fn append_does_not_replace_and_rewind_removes_later_messages() {
    let (_root, source) = fixture(
        "rewind",
        &[
            json!({"op":"put", "id":"m1", "content":"one"}),
            json!({"op":"append", "id":"m1", "content":" two"}),
            json!({"op":"put", "id":"m2", "content":"second"}),
            json!({"op":"rewind", "id":"m2"}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "one two");
}

#[test]
fn appending_after_a_rewind_keeps_the_message_and_grows_it() {
    let (_root, source) = fixture(
        "append-after-rewind",
        &[
            json!({"op":"put", "id":"m1", "content":"one"}),
            json!({"op":"put", "id":"m2", "content":"second"}),
            json!({"op":"rewind", "id":"m2"}),
            json!({"op":"append", "id":"m1", "content":" more"}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "one more");
}

#[test]
fn rewinding_an_unknown_id_clears_the_visible_transcript() {
    let (_root, source) = fixture(
        "rewind-unknown",
        &[
            json!({"op":"put", "id":"m1", "content":"one"}),
            json!({"op":"rewind", "id":"missing"}),
        ],
    );
    assert!(read(&[source]).is_empty());
}

#[test]
fn removing_a_tool_group_drops_its_children_and_usage() {
    let (_root, source) = fixture(
        "group",
        &[
            json!({"op":"put", "id":"call", "content":"call"}),
            json!({"op":"put", "id":"call:result", "content":"result"}),
            json!({"op":"usage", "id":"u1", "message":"call", "input":10}),
            json!({"op":"removeGroup", "id":"call"}),
            json!({"op":"put", "id":"later", "content":"later"}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "later");
}

#[test]
fn a_replacement_with_a_new_priority_hides_the_older_visible_group() {
    let (_root, source) = fixture(
        "priority",
        &[
            json!({"op":"put", "id":"m1", "content":"old", "priority":0}),
            json!({"op":"put", "id":"m2", "content":"new", "priority":3}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "new");
}

#[test]
fn clear_removes_previous_messages_and_usage() {
    let (_root, source) = fixture(
        "clear",
        &[
            json!({"op":"put", "id":"m1", "content":"one"}),
            json!({"op":"usage", "id":"u1", "message":"m1", "input":10}),
            json!({"op":"clear"}),
            json!({"op":"put", "id":"m2", "content":"two"}),
        ],
    );
    let messages = read(&[source]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "m2");
}

#[test]
fn search_is_case_insensitive_and_skips_images() {
    let (_root, source) = fixture(
        "search",
        &[
            json!({"op":"put", "id":"m1", "content":"BalanceHub 会话检索"}),
            json!({"op":"put", "id":"m2", "content":"needle in an image", "metadata":{"kind":"image"}}),
        ],
    );
    let transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    let request = SessionContentSearchRequest {
        terms: vec![SessionSearchTerm {
            index: 0,
            value: "balancehub".into(),
        }],
    };
    let result = transcript.search(&request);
    assert_eq!(result.matched_term_indexes, [0]);
    assert_eq!(
        result.hit.unwrap().message_id,
        transcript.messages[0].public_id
    );
    let image = SessionContentSearchRequest {
        terms: vec![SessionSearchTerm {
            index: 0,
            value: "needle".into(),
        }],
    };
    assert!(!transcript.search(&image).has_content);
}

#[test]
fn page_cursors_walk_every_message_without_gaps() {
    let lines = (0..100)
        .map(|index| json!({"op":"put", "id":format!("m{index}"), "content":format!("message {index}")}))
        .collect::<Vec<_>>();
    let (_root, source) = fixture("pages", &lines);
    let transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    let mut request = page_request();
    let mut seen = Vec::new();
    loop {
        let page = transcript
            .read_page(summary(), &request, false, &budget(u64::MAX))
            .unwrap();
        assert_eq!(page.page.total_messages, 100);
        seen.extend(page.messages.into_iter().map(|message| message.id));
        match page.page.next_cursor {
            Some(cursor) => request.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen.len(), 100);
    assert!(seen.iter().all(|id| !id.is_empty()));
}

#[test]
fn content_expansion_checks_revision_and_content_version() {
    let (_root, source) = fixture(
        "content",
        &[json!({"op":"put", "id":"m1", "content":"x".repeat(200_000)})],
    );
    let transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    let message = &transcript.messages[0];
    let request = AgentSessionMessageRequest {
        consumer_id: "test".into(),
        request_id: 1,
        scope_revision: "scope".into(),
        session_ref: "ref".into(),
        revision: transcript.revision.clone(),
        message_id: message.public_id.clone(),
        content_version: message.content_version,
        offset: 0,
    };
    let content = transcript
        .read_content(&request, &budget(u64::MAX))
        .unwrap();
    assert_eq!(content.content.chars().count(), CONTENT_PAGE_CHARS);
    assert!(content.next_offset.is_some());

    let mut wrong_version = request.clone();
    wrong_version.content_version += 1;
    assert!(transcript
        .read_content(&wrong_version, &budget(u64::MAX))
        .is_err());
    let mut wrong_revision = request;
    wrong_revision.revision = "stale".into();
    assert!(transcript
        .read_content(&wrong_revision, &budget(u64::MAX))
        .is_err());
}

#[test]
fn an_exhausted_budget_returns_a_partial_transcript() {
    let (_root, source) = fixture(
        "budget",
        &[json!({"op":"put", "id":"m1", "content":"some content"})],
    );
    let transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(1)).unwrap();
    assert!(!transcript.complete);
    assert!(transcript.messages.is_empty());
}
