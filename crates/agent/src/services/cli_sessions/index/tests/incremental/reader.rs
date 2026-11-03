use super::*;
use crate::models::{AgentSessionDetailRequest, AgentSessionMessageRequest, CliSessionDetail};

fn index_all(index: &HistoryIndex, record: &SessionHistoryRecord) {
    for pass in 0..1000 {
        assert!(pass < 999, "the fixture must finish indexing");
        let budget = read_budget(u64::MAX);
        let progress = crate::services::cli_sessions::workbench::with_read_budget(&budget, || {
            index.search(
                "workspace",
                record,
                adapter(record.summary.cli_kind),
                &SessionContentSearchRequest { terms: vec![] },
                &budget,
            )
        })
        .unwrap();
        if progress.complete {
            return;
        }
    }
}
fn page_request() -> AgentSessionDetailRequest {
    AgentSessionDetailRequest {
        consumer_id: "reader-test".into(),
        request_id: 1,
        scope_revision: "scope".into(),
        session_ref: "ref".into(),
        cursor: None,
        anchor: None,
        query: String::new(),
        match_position: None,
    }
}
fn page(
    index: &HistoryIndex,
    record: &SessionHistoryRecord,
    request: &AgentSessionDetailRequest,
) -> CliSessionDetail {
    let budget = read_budget(u64::MAX);
    crate::services::cli_sessions::workbench::with_read_budget(&budget, || {
        index.read_page(
            "workspace",
            &record.record_key,
            record.summary.clone(),
            request,
            false,
            &budget,
        )
    })
    .unwrap()
}

#[test]
fn every_message_after_the_former_whole_session_limit_is_reachable_by_cursor() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    fs::write(
        &record.locator,
        (0..1005)
            .map(|number| codex_message(&format!("第 {number} 条记录")))
            .collect::<String>(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let mut request = page_request();
    let mut contents = Vec::new();
    let mut ids = std::collections::HashSet::new();
    loop {
        let detail = page(&index, &record, &request);
        assert_eq!(detail.page.total_messages, 1005);
        assert_eq!(detail.page.start, contents.len());
        assert!(detail.messages.len() <= 80);
        assert!(detail
            .messages
            .iter()
            .all(|message| ids.insert(message.id.clone())));
        contents.extend(detail.messages.into_iter().map(|message| message.content));
        if detail.page.next_cursor.is_none() {
            break;
        }
        request.cursor = detail.page.next_cursor;
    }
    assert_eq!(
        contents,
        (0..1005)
            .map(|number| format!("第 {number} 条记录"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn previous_pages_respect_content_budgets_and_never_skip_intervening_messages() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    fs::write(
        &record.locator,
        (0..100)
            .map(|index| codex_message(&format!("{index}:{}", "字".repeat(10_000))))
            .collect::<String>(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let mut request = page_request();
    let first = page(&index, &record, &request);
    assert_eq!(first.messages.len(), 32);
    request.cursor = first.page.next_cursor;
    let second = page(&index, &record, &request);
    request.cursor = second.page.next_cursor;
    let third = page(&index, &record, &request);
    request.cursor = third.page.previous_cursor;
    let previous = page(&index, &record, &request);
    assert_eq!(previous.page.start, second.page.start);
    assert_eq!(
        previous
            .messages
            .iter()
            .map(|message| &message.id)
            .collect::<Vec<_>>(),
        second
            .messages
            .iter()
            .map(|message| &message.id)
            .collect::<Vec<_>>()
    );
    request.cursor = previous.page.next_cursor;
    let next = page(&index, &record, &request);
    assert_eq!(next.page.start, third.page.start);
}

#[test]
fn unicode_message_search_can_focus_the_tail_and_chunks_reassemble_without_loss() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    let content = format!("{}末尾定位", "界🦀".repeat(150_000));
    fs::write(&record.locator, codex_message(&content)).unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let mut request = page_request();
    request.query = "末尾定位".into();
    let detail = page(&index, &record, &request);
    let message = &detail.messages[0];
    assert!(message.content_offset > 290_000);
    assert!(message.content.contains("末尾定位"));
    assert_eq!(message.content_length, content.chars().count());
    let mut offset = 0;
    let mut reconstructed = String::new();
    loop {
        let request = AgentSessionMessageRequest {
            consumer_id: "message".into(),
            request_id: 1,
            scope_revision: "scope".into(),
            session_ref: "ref".into(),
            revision: detail.page.revision.clone(),
            message_id: message.id.clone(),
            content_version: message.content_version,
            offset,
        };
        let chunk = index
            .read_content(
                "workspace",
                &record.record_key,
                &request,
                &read_budget(u64::MAX),
            )
            .unwrap();
        assert_eq!(chunk.offset, offset);
        assert!(chunk.content.chars().count() <= 128 * 1024);
        reconstructed.push_str(&chunk.content);
        match chunk.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(reconstructed, content);
}

#[test]
fn search_navigation_reaches_later_pages_and_outline_focus_does_not_invent_a_match_position() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    fs::write(
        &record.locator,
        (0..500)
            .map(|number| {
                codex_message(&format!(
                    "问题 {number} {}",
                    if number == 5 || number == 450 {
                        "needle"
                    } else {
                        "正文"
                    }
                ))
            })
            .collect::<String>(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let mut request = page_request();
    request.query = "needle".into();
    request.match_position = Some(1);
    let detail = page(&index, &record, &request);
    assert!(detail.page.start > 400);
    assert_eq!(detail.find.total, 2);
    assert_eq!(detail.find.position, Some(1));
    let focused = detail
        .messages
        .iter()
        .find(|message| Some(&message.id) == detail.find.active_message_id.as_ref())
        .unwrap();
    assert!(focused.content.contains("450 needle"));
    request.match_position = None;
    request.anchor = Some(detail.outline[200].message_id.clone());
    let outline = page(&index, &record, &request);
    assert_eq!(outline.find.position, None);
    assert_eq!(outline.find.active_message_id, request.anchor);
}

#[test]
fn list_search_reads_examples_per_term_instead_of_every_matching_message() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    fs::write(
        &record.locator,
        (0..600)
            .map(|number| codex_message(&format!("needle 第 {number} 条")))
            .collect::<String>(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let request = request("needle");
    let hits = index
        .connection(&read_budget(u64::MAX), |connection| {
            super::super::super::reader::search_hits(connection, "workspace", None, &request)
        })
        .unwrap();
    assert_eq!(hits[&record.record_key].len(), 1);
    let detail_hits = index
        .connection(&read_budget(u64::MAX), |connection| {
            super::super::super::reader::search_hits(
                connection,
                "workspace",
                Some(&record.record_key),
                &request,
            )
        })
        .unwrap();
    assert_eq!(detail_hits[&record.record_key].len(), 600);
}

#[test]
fn message_versions_change_during_indexing_even_when_the_source_revision_does_not() {
    let (_root, config, record) = fixture(AgentCliKind::Gemini);
    let initial = "相同前缀".repeat(2500) + "最初版本";
    let updated = "相同前缀".repeat(2500) + "更新版本";
    fs::write(
        &record.locator,
        [
            json!({"id":"user","type":"user","content":initial}),
            json!({"id":"user","type":"user","content":updated}),
        ]
        .iter()
        .map(|value| value.to_string() + "\n")
        .collect::<String>(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Gemini, "source");
    let source = (adapter(AgentCliKind::Gemini).index)(&record)
        .unwrap()
        .remove(0);
    let key = source.path.to_string_lossy();
    let first = stream::read_batch_with_limit(&source, None, &|| true, 1).unwrap();
    index
        .connection(&read_budget(u64::MAX), |connection| {
            records::apply_batch(connection, "workspace", &record, &key, None, &first)
        })
        .unwrap();
    let before = page(&index, &record, &page_request());
    let second =
        stream::read_batch_with_limit(&source, Some(&first.checkpoint), &|| true, 1).unwrap();
    let expected = serde_json::to_string(&first.checkpoint).unwrap();
    index
        .connection(&read_budget(u64::MAX), |connection| {
            records::apply_batch(
                connection,
                "workspace",
                &record,
                &key,
                Some(&expected),
                &second,
            )
        })
        .unwrap();
    let after = page(&index, &record, &page_request());
    assert_eq!(before.page.revision, after.page.revision);
    assert_eq!(before.messages[0].id, after.messages[0].id);
    assert_ne!(
        before.messages[0].content_version,
        after.messages[0].content_version
    );
    let old = AgentSessionMessageRequest {
        consumer_id: "message".into(),
        request_id: 1,
        scope_revision: "scope".into(),
        session_ref: "ref".into(),
        revision: before.page.revision,
        message_id: before.messages[0].id.clone(),
        content_version: before.messages[0].content_version,
        offset: 0,
    };
    assert!(index
        .read_content(
            "workspace",
            &record.record_key,
            &old,
            &read_budget(u64::MAX)
        )
        .unwrap_err()
        .contains("内容已更新"));
}

#[test]
fn compressed_and_plain_codex_transcripts_produce_the_same_readable_messages() {
    let (_root, config, mut record) = fixture(AgentCliKind::Codex);
    let text = codex_message("压缩会话的完整正文") + &codex_message("第二条 needle");
    fs::write(&record.locator, &text).unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    index_all(&index, &record);
    let plain = page(&index, &record, &page_request());
    let compressed = record.locator.with_extension("jsonl.zst");
    fs::write(
        &compressed,
        zstd::stream::encode_all(text.as_bytes(), 1).unwrap(),
    )
    .unwrap();
    fs::remove_file(&record.locator).unwrap();
    record.locator = compressed;
    index_all(&index, &record);
    let decoded = page(&index, &record, &page_request());
    assert_eq!(
        plain
            .messages
            .iter()
            .map(|message| &message.content)
            .collect::<Vec<_>>(),
        decoded
            .messages
            .iter()
            .map(|message| &message.content)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        decoded.messages.len(),
        2,
        "the removed plain source is not duplicated"
    );
    assert_eq!(
        query(&index, &record, "needle", &read_budget(0))
            .content
            .matched_term_indexes,
        [0]
    );
}

#[test]
fn gemini_replacing_tool_blocks_removes_old_children_and_keeps_conversation_order() {
    let (_root, config, record) = fixture(AgentCliKind::Gemini);
    fs::write(&record.locator, [
        json!({"id":"first","type":"user","content":"问题"}),
        json!({"id":"answer","type":"gemini","content":"旧回答","toolCalls":[{"id":"old","name":"read_file","args":{"path":"obsolete"},"result":"obsolete output"}]}),
        json!({"id":"second","type":"user","content":"第二轮"}),
        json!({"id":"answer","type":"gemini","content":"最终回答","toolCalls":[{"id":"new","name":"write_file","args":{"path":"new.txt","content":"新内容"},"result":"done"}]}),
    ].iter().map(|value| value.to_string() + "\n").collect::<String>()).unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Gemini, "source");
    index_all(&index, &record);
    let detail = page(&index, &record, &page_request());
    assert_eq!(detail.messages.len(), 5);
    assert_eq!(detail.messages.last().unwrap().content, "第二轮");
    assert!(!detail
        .messages
        .iter()
        .any(|message| message.content.contains("obsolete")));
    assert!(query(&index, &record, "obsolete", &read_budget(0))
        .content
        .matched_term_indexes
        .is_empty());
}
