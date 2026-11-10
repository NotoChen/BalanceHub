use super::{test_support::read_indexed_messages, *};
use crate::services::agent_cli::contracts::{SessionSearchTerm, SessionTranscriptFormat};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
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
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
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

/// Windowed transcripts decode one byte range at a time. The transcript here
/// is a small fixture with window metadata attached, so the cursor branches
/// can be exercised without writing a multi-megabyte session.
#[test]
fn windowed_pages_open_at_the_newest_messages_and_cross_window_boundaries() {
    let lines = (0..100)
        .map(|index| {
            json!({"op":"put", "id":format!("m{index}"), "content":format!("message {index}")})
        })
        .collect::<Vec<_>>();
    let (_root, source) = fixture("window-pages", &lines);
    let mut transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    let content = transcript.revision.clone();

    let newest = format!("{content}@10-500");
    transcript.window = Some(SourceWindow {
        start: 10,
        end: 500,
        file_bytes: 1000,
    });
    transcript.revision = newest.clone();
    let page = transcript
        .read_page(summary(), &page_request(), false, &budget(u64::MAX))
        .unwrap();
    // The page opens on the newest messages inside the window, not its start.
    assert_eq!(page.page.start, 20);
    assert_eq!(page.messages.len(), 80);
    assert_eq!(
        page.page.window,
        Some(SessionReadWindow {
            start: 10,
            end: 500,
            file_bytes: 1000,
            index: 1,
            total: 1,
            has_earlier: true,
            has_later: true,
        })
    );
    assert!(page
        .page
        .previous_cursor
        .as_deref()
        .is_some_and(|cursor| cursor.starts_with(&format!("{newest}:"))));
    assert_eq!(page.page.next_cursor, Some(format!("{newest}:later:500")));

    // Walking back loads the previous window and lands on its last page.
    transcript.window = Some(SourceWindow {
        start: 0,
        end: 10,
        file_bytes: 1000,
    });
    transcript.revision = format!("{content}@0-10");
    let mut request = page_request();
    request.cursor = Some(format!("{newest}:earlier:10"));
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 20);
    assert_eq!(page.messages.len(), 80);
    assert_eq!(
        page.page.next_cursor,
        Some(format!("{content}@0-10:later:10"))
    );

    // Walking forward loads the next window and lands on its first page.
    transcript.window = Some(SourceWindow {
        start: 500,
        end: 1000,
        file_bytes: 1000,
    });
    transcript.revision = format!("{content}@500-1000");
    request.cursor = Some(format!("{newest}:later:500"));
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 0);
    assert_eq!(page.messages.len(), 80);
    assert_eq!(
        page.page.previous_cursor,
        Some(format!("{content}@500-1000:earlier:500"))
    );

    // A bare revision keeps this window and applies a new search focus, so
    // re-searching or cycling matches does not jump back to the newest window.
    request.cursor = Some(format!("{content}@500-1000"));
    request.query = "90".into();
    request.match_position = Some(0);
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 82);
    assert_eq!(page.page.window.unwrap().start, 500);
    assert_eq!(page.messages[0].content, "message 82");
    assert_eq!(
        window_request_for_cursor(request.cursor.as_deref()).unwrap(),
        stream::WindowRequest::Range {
            start: 500,
            end: 1000
        }
    );
    request.query.clear();
    request.match_position = None;

    // "回到最新" jumps back to the end of the newest window.
    request.cursor = Some(format!("{newest}:latest"));
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 20);

    // A cursor for an adjacent window only works at that exact boundary.
    request.cursor = Some(format!("{newest}:later:501"));
    assert!(transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .is_err());
    // "latest" is relative to the file: a revision that changed after the
    // cursor was issued still opens the newest window instead of failing.
    request.cursor = Some("stale@500-1000:latest".into());
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 20);

    // "earliest" opens the first page of the first window.
    transcript.window = Some(SourceWindow {
        start: 0,
        end: 10,
        file_bytes: 1000,
    });
    transcript.revision = format!("{content}@0-10");
    request.cursor = Some(format!("{content}@10-500:earliest"));
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 0);
    assert_eq!(page.messages.len(), 80);
    assert_eq!(page.page.window.unwrap().start, 0);
}

#[test]
fn refreshing_after_an_append_does_not_reject_the_stale_revision() {
    let (_root, source) = fixture(
        "refresh-append",
        &[json!({"op":"put", "id":"m1", "content":"first"})],
    );
    let initial = Transcript::from_sources(
        "workspace",
        "session",
        std::slice::from_ref(&source),
        &budget(u64::MAX),
    )
    .unwrap();
    let stale = initial.revision.clone();

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&source.path)
        .unwrap();
    file.write_all(b"\n{\"op\":\"put\",\"id\":\"m2\",\"content\":\"second\"}")
        .unwrap();
    drop(file);

    let refreshed =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    assert_ne!(refreshed.revision, stale);
    let mut request = page_request();
    request.cursor = Some(stale);
    let page = refreshed
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert!(page
        .messages
        .iter()
        .any(|message| message.content == "second"));
    assert_eq!(page.page.revision, refreshed.revision);
}

#[test]
fn an_earlier_window_keeps_its_revision_when_the_file_grows() {
    let (_root, source) = fixture(
        "stable-window-revision",
        &[
            json!({"op":"put", "id":"m1", "content":"first"}),
            json!({"op":"put", "id":"m2", "content":"second"}),
        ],
    );
    let initial_bytes = fs::metadata(&source.path).unwrap().len();
    let initial = Transcript::from_sources_with_window(
        "workspace",
        "session",
        std::slice::from_ref(&source),
        stream::WindowRequest::Range {
            start: 0,
            end: initial_bytes,
        },
        &budget(u64::MAX),
    )
    .unwrap();
    let revision = initial.revision.clone();
    assert!(revision.contains('@'));

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&source.path)
        .unwrap();
    file.write_all(b"\n{\"op\":\"put\",\"id\":\"m3\",\"content\":\"third\"}")
        .unwrap();
    drop(file);
    let grown_bytes = fs::metadata(&source.path).unwrap().len();

    let unchanged = Transcript::from_sources_with_window(
        "workspace",
        "session",
        std::slice::from_ref(&source),
        stream::WindowRequest::Range {
            start: 0,
            end: initial_bytes,
        },
        &budget(u64::MAX),
    )
    .unwrap();
    assert_eq!(unchanged.revision, revision);
    assert_eq!(unchanged.window.unwrap().file_bytes, grown_bytes);

    let changed = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source],
        stream::WindowRequest::Range {
            start: 0,
            end: grown_bytes,
        },
        &budget(u64::MAX),
    )
    .unwrap();
    assert_ne!(changed.revision, revision);
}

#[test]
fn latest_cursor_still_opens_the_newest_window_after_a_revision_change() {
    let lines = (0..100)
        .map(|index| {
            json!({"op":"put", "id":format!("m{index}"), "content":format!("message {index}")})
        })
        .collect::<Vec<_>>();
    let (_root, source) = fixture("latest-after-change", &lines);
    let mut transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    let content = transcript.revision.clone();
    transcript.window = Some(SourceWindow {
        start: 500,
        end: 1000,
        file_bytes: 1000,
    });
    transcript.revision = format!("{content}@500-1000");

    let mut request = page_request();
    request.cursor = Some(format!("{content}@10-500:latest"));
    let page = transcript
        .read_page(summary(), &request, false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.page.start, 20);
    assert_eq!(page.messages.len(), 80);
}

#[test]
fn the_first_window_starts_at_the_file_head() {
    let (_root, source) = fixture(
        "first-window",
        &[
            json!({"op":"put", "id":"m1", "content":"first"}),
            json!({"op":"put", "id":"m2", "content":"second"}),
        ],
    );
    let transcript = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source],
        stream::WindowRequest::First,
        &budget(u64::MAX),
    )
    .unwrap();
    let window = transcript
        .window
        .expect("an explicit first request opens a window");
    assert_eq!(window.start, 0);
    assert_eq!(transcript.messages.len(), 2);
}

#[test]
fn window_pages_report_an_estimated_fragment_position() {
    let (_root, source) = fixture(
        "fragment-position",
        &[json!({"op":"put", "id":"m1", "content":"first"})],
    );
    let mut transcript =
        Transcript::from_sources("workspace", "session", &[source], &budget(u64::MAX)).unwrap();
    transcript.window = Some(SourceWindow {
        start: TRANSCRIPT_WINDOW_BYTES + 10,
        end: TRANSCRIPT_WINDOW_BYTES * 2,
        file_bytes: TRANSCRIPT_WINDOW_BYTES * 3,
    });
    transcript.revision = format!(
        "window@{}-{}",
        transcript.window.unwrap().start,
        transcript.window.unwrap().end
    );
    let page = transcript
        .read_page(summary(), &page_request(), false, &budget(u64::MAX))
        .unwrap();
    let window = page.page.window.expect("window metadata");
    assert_eq!(window.index, 2);
    assert_eq!(window.total, 3);
}

#[test]
fn sources_that_need_prior_state_do_not_open_windows() {
    let (_root, mut source) = fixture(
        "unsafe-window",
        &[json!({"op":"put", "id":"m1", "content":"first"})],
    );
    source.window_safe = false;
    let result = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source],
        stream::WindowRequest::Range { start: 0, end: 10 },
        &budget(u64::MAX),
    );
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "该会话格式不支持分段读取");
}

#[test]
fn oversized_single_source_jsonl_opens_only_the_latest_window() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(b"{\"op\":\"put\",\"id\":\"head\",\"content\":\"head\"}\n")
        .unwrap();
    // Sparse padding plus a real trailing record: the reader must not decode
    // the padding as messages, but still open a bounded window near the end.
    file.set_len(WINDOWED_TRANSCRIPT_BYTES + 1024).unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(b"\n{\"op\":\"put\",\"id\":\"tail\",\"content\":\"tail\"}\n")
        .unwrap();
    drop(file);

    let transcript = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source(&path)],
        stream::WindowRequest::Latest,
        &budget(u64::MAX),
    )
    .unwrap();
    assert!(!transcript.complete);
    assert!(!transcript.truncated);
    let window = transcript.window.expect("large JSONL opens a window");
    assert!(window.start > 0);
    assert!(window.end <= window.file_bytes);
    assert!(transcript.revision.contains('@'));
    assert_eq!(
        transcript
            .messages
            .iter()
            .filter(|message| message.content == "tail")
            .count(),
        1
    );
    assert!(transcript
        .messages
        .iter()
        .all(|message| message.content != "head"));
    let tail_id = transcript.messages[0].public_id.clone();
    assert!(transcript.has_message(&tail_id));
    assert!(!transcript.has_message("missing"));

    let page = transcript
        .read_page(summary(), &page_request(), false, &budget(u64::MAX))
        .unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.page.total_messages, 1);
    assert!(page.page.window.is_some());
    assert!(
        page.usage.partial,
        "a window's usage total only covers its own slice"
    );
    assert_eq!(
        page.page.previous_cursor,
        Some(format!("{}:earlier:{}", transcript.revision, window.start))
    );
    assert_eq!(page.page.next_cursor, None);
}

#[test]
fn a_window_revision_is_reproduced_by_reopening_its_byte_range() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("reopen.jsonl");
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(b"{\"op\":\"put\",\"id\":\"head\",\"content\":\"head\"}\n")
        .unwrap();
    file.set_len(WINDOWED_TRANSCRIPT_BYTES + 1024).unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(b"\n{\"op\":\"put\",\"id\":\"tail\",\"content\":\"tail\"}\n")
        .unwrap();
    drop(file);

    let latest = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source(&path)],
        stream::WindowRequest::Latest,
        &budget(u64::MAX),
    )
    .unwrap();
    let window = latest.window.expect("large JSONL opens a window");
    let reopened = Transcript::from_sources_with_window(
        "workspace",
        "session",
        &[source(&path)],
        stream::WindowRequest::Range {
            start: window.start,
            end: window.end,
        },
        &budget(u64::MAX),
    )
    .unwrap();
    assert_eq!(reopened.revision, latest.revision);
}

#[test]
fn window_requests_are_recovered_from_cursors_and_revisions() {
    assert_eq!(
        window_request_for_cursor(None).unwrap(),
        stream::WindowRequest::Latest
    );
    assert_eq!(
        window_request_for_cursor(Some("hash:latest")).unwrap(),
        stream::WindowRequest::Latest
    );
    assert_eq!(
        window_request_for_cursor(Some("hash@10-20:latest")).unwrap(),
        stream::WindowRequest::Latest
    );
    assert_eq!(
        window_request_for_cursor(Some("hash:earliest")).unwrap(),
        stream::WindowRequest::First
    );
    assert_eq!(
        window_request_for_cursor(Some("hash:earlier:42")).unwrap(),
        stream::WindowRequest::Before(42)
    );
    assert_eq!(
        window_request_for_cursor(Some("hash:later:42")).unwrap(),
        stream::WindowRequest::After(42)
    );
    assert_eq!(
        window_request_for_cursor(Some("hash@10-20:0:80")).unwrap(),
        stream::WindowRequest::Range { start: 10, end: 20 }
    );
    assert_eq!(
        window_request_for_cursor(Some("plain")).unwrap(),
        stream::WindowRequest::Default
    );
    assert!(window_request_for_cursor(Some("hash:earlier:")).is_err());
    assert!(window_request_for_cursor(Some("hash:latest:1")).is_err());
    assert_eq!(
        window_request_for_revision("hash@10-20").unwrap(),
        stream::WindowRequest::Range { start: 10, end: 20 }
    );
    assert_eq!(
        window_request_for_revision("plain").unwrap(),
        stream::WindowRequest::Default
    );
    assert!(window_request_for_revision("hash@bad").is_err());
    assert_eq!(
        same_transcript_cursor_revision(Some("hash@10-20:0:80")),
        Some("hash@10-20")
    );
    assert_eq!(
        same_transcript_cursor_revision(Some("hash@10-20:80")),
        Some("hash@10-20")
    );
    assert_eq!(
        same_transcript_cursor_revision(Some("hash@10-20")),
        Some("hash@10-20")
    );
    assert_eq!(
        same_transcript_cursor_revision(Some("plain")),
        Some("plain")
    );
    assert_eq!(
        same_transcript_cursor_revision(Some("hash@10-20:earlier:10")),
        None
    );
    assert_eq!(same_transcript_cursor_revision(None), None);
}
