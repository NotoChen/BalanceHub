use super::*;
use crate::services::agent_cli::{self, contracts::SessionSearchTerm};
use serde_json::json;
use std::io::Write;

fn fixture(kind: AgentCliKind) -> (tempfile::TempDir, SessionIndexConfig, SessionHistoryRecord) {
    let root = tempfile::tempdir().unwrap();
    let config = SessionIndexConfig {
        enabled: true,
        directory: root.path().join("index"),
        max_size_bytes: 64 * 1024 * 1024,
    };
    let record = SessionHistoryRecord::identity(
        kind,
        "native".into(),
        root.path(),
        root.path().join(if kind == AgentCliKind::Grok {
            "summary.json"
        } else {
            "session.jsonl"
        }),
    );
    (root, config, record)
}
fn adapter(kind: AgentCliKind) -> &'static SessionHistoryAdapter {
    agent_cli::definition(kind).sessions().unwrap().history()
}
fn request(term: &str) -> SessionContentSearchRequest {
    SessionContentSearchRequest {
        terms: vec![SessionSearchTerm {
            index: 0,
            value: term.to_lowercase(),
        }],
    }
}
fn query(
    index: &HistoryIndex,
    record: &SessionHistoryRecord,
    term: &str,
    budget: &SessionReadBudget,
) -> HistorySearchResult {
    crate::services::cli_sessions::workbench::with_read_budget(budget, || {
        index.search(
            "workspace",
            record,
            adapter(record.summary.cli_kind),
            &request(term),
            budget,
        )
    })
    .unwrap()
}
fn codex_message(text: &str) -> String {
    json!({"type":"event_msg", "payload":{"type":"user_message", "message":text}}).to_string()
        + "\n"
}

#[test]
fn persistent_checkpoints_survive_recreated_readers_and_append_without_reindexing_the_prefix() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    let mut file = fs::File::create(&record.locator).unwrap();
    let progress = json!({"type":"progress", "padding":"x".repeat(32 * 1024)}).to_string();
    for _ in 0..640 {
        writeln!(file, "{progress}").unwrap();
    }
    file.write_all(codex_message("tail needle").as_bytes())
        .unwrap();
    drop(file);
    let mut previous_offset = 0;
    let mut complete = false;
    for pass in 0..100 {
        let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
        let result = query(&index, &record, "tail needle", &read_budget(u64::MAX));
        if pass == 0 {
            assert!(!result.complete);
            assert!(result.content.matched_term_indexes.is_empty());
        }
        assert!(result.indexed_bytes > previous_offset || result.complete);
        previous_offset = result.indexed_bytes;
        if result.complete {
            assert_eq!(result.content.matched_term_indexes, [0]);
            complete = true;
            break;
        }
    }
    assert!(complete);
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    let cached_budget = read_budget(0);
    assert_eq!(
        query(&index, &record, "tail needle", &cached_budget)
            .content
            .matched_term_indexes,
        [0]
    );
    assert_eq!(cached_budget.bytes.load(Ordering::Relaxed), 0);
    fs::OpenOptions::new()
        .append(true)
        .open(&record.locator)
        .unwrap()
        .write_all(codex_message("new appended needle").as_bytes())
        .unwrap();
    let appended_budget = read_budget(64 * 1024);
    assert_eq!(
        query(&index, &record, "appended needle", &appended_budget)
            .content
            .matched_term_indexes,
        [0]
    );
    assert!(appended_budget.bytes.load(Ordering::Relaxed) < 32 * 1024);
    let count: i64 = index
        .connection(&read_budget(u64::MAX), |connection| {
            connection
                .query_row("SELECT COUNT(*) FROM message_positions", [], |row| {
                    row.get(0)
                })
                .map_err(|error| error.to_string())
        })
        .unwrap();
    assert_eq!(count, 2, "appends cannot duplicate the old indexed message");
}

#[test]
fn checkpoint_compare_and_swap_prevents_duplicate_streamed_chunks() {
    let (_root, _config, record) = fixture(AgentCliKind::Grok);
    let path = record.locator.parent().unwrap().join("updates.jsonl");
    fs::write(
        &path,
        ["Balance", "Hub"]
            .map(|text| {
                json!({"params":{"update":{"sessionUpdate":"agent_message_chunk", "content":text}}})
                    .to_string()
                    + "\n"
            })
            .concat(),
    )
    .unwrap();
    let sources = (adapter(AgentCliKind::Grok).index)(&record).unwrap();
    let source = &sources[0];
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    let key = path.to_string_lossy();
    let first = stream::read_batch_with_limit(source, None, &|| true, 1).unwrap();
    assert!(
        records::apply_batch(&mut connection, "workspace", &record, &key, None, &first).unwrap()
    );
    let known = serde_json::to_string(&first.checkpoint).unwrap();
    let a = stream::read_batch_with_limit(source, Some(&first.checkpoint), &|| true, 1).unwrap();
    let b = stream::read_batch_with_limit(source, Some(&first.checkpoint), &|| true, 1).unwrap();
    assert!(records::apply_batch(
        &mut connection,
        "workspace",
        &record,
        &key,
        Some(&known),
        &a
    )
    .unwrap());
    assert!(!records::apply_batch(
        &mut connection,
        "workspace",
        &record,
        &key,
        Some(&known),
        &b
    )
    .unwrap());
    let content: String = connection
        .query_row("SELECT content FROM messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(content, "BalanceHub");
    assert_eq!(
        search_indexed_messages(
            &connection,
            "workspace",
            &record.record_key,
            &request("BalanceHub")
        )
        .unwrap()
        .matched_term_indexes,
        [0]
    );
}

#[test]
fn removed_native_sources_drop_their_text_and_reveal_the_remaining_fallback() {
    let (_root, config, record) = fixture(AgentCliKind::Grok);
    let directory = record.locator.parent().unwrap();
    let updates = directory.join("updates.jsonl");
    fs::write(&updates, json!({"params":{"update":{"sessionUpdate":"agent_message_chunk", "content":"primary marker"}}}).to_string()).unwrap();
    fs::write(
        directory.join("chat_history.jsonl"),
        json!({"type":"assistant", "content":"fallback marker"}).to_string(),
    )
    .unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Grok, "source");
    assert_eq!(
        query(&index, &record, "primary", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    assert!(query(&index, &record, "fallback", &read_budget(u64::MAX))
        .content
        .matched_term_indexes
        .is_empty());
    fs::remove_file(updates).unwrap();
    assert_eq!(
        query(&index, &record, "fallback", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    assert!(query(&index, &record, "primary", &read_budget(u64::MAX))
        .content
        .matched_term_indexes
        .is_empty());
}

#[test]
fn unavailable_disk_cache_uses_a_reusable_memory_index() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    fs::write(&config.directory, "not a directory").unwrap();
    fs::write(&record.locator, codex_message("memory needle")).unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    assert_eq!(
        query(&index, &record, "needle", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    assert_eq!(index.state(), CliSessionIndexState::Fallback);
    let reused = read_budget(0);
    assert_eq!(
        query(&index, &record, "needle", &reused)
            .content
            .matched_term_indexes,
        [0]
    );
    assert_eq!(reused.bytes.load(Ordering::Relaxed), 0);
    assert!(config.directory.is_file());
}

#[test]
fn source_rewrites_remove_old_matches_without_touching_another_session() {
    let (_root, config, record) = fixture(AgentCliKind::Codex);
    let mut other = record.clone();
    other.record_key = "other".into();
    other.locator = record.locator.with_file_name("other.jsonl");
    fs::write(&record.locator, codex_message("obsolete marker")).unwrap();
    fs::write(&other.locator, codex_message("other marker")).unwrap();
    let index = HistoryIndex::new(&config, AgentCliKind::Codex, "source");
    assert_eq!(
        query(&index, &record, "obsolete", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    assert_eq!(
        query(&index, &other, "other", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    fs::write(
        &record.locator,
        codex_message("replacement with another size"),
    )
    .unwrap();
    assert!(query(&index, &record, "obsolete", &read_budget(u64::MAX))
        .content
        .matched_term_indexes
        .is_empty());
    assert_eq!(
        query(&index, &record, "replacement", &read_budget(u64::MAX))
            .content
            .matched_term_indexes,
        [0]
    );
    assert_eq!(
        query(&index, &other, "other", &read_budget(0))
            .content
            .matched_term_indexes,
        [0]
    );
}
