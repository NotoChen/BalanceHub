use super::{
    clean_text, first_non_empty, normalize_timestamp, read_json_lines_limited,
    scan_json_lines_matching, scan_json_records, timestamp_from_value, SearchAccumulator,
    SearchQuery,
};
use crate::models::{AgentCliKind, CliSessionSummary};
use crate::services::agent_cli::contracts::SessionContentSearchRequest;
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn text_is_compacted_and_bounded() {
    assert_eq!(
        clean_text("  hello\n world  ", 20).as_deref(),
        Some("hello world")
    );
    assert_eq!(clean_text("abcdef", 3).as_deref(), Some("abc..."));
    assert_eq!(clean_text("  ", 20), None);
}

#[test]
fn title_fallback_uses_first_non_empty_value() {
    assert_eq!(
        first_non_empty([None, Some(String::new()), Some("title".to_string())]),
        "title"
    );
    assert_eq!(first_non_empty([None]), "未命名会话");
}

#[test]
fn timestamps_accept_seconds_milliseconds_and_rfc3339() {
    assert!(timestamp_from_value(Some(1_725_000_000), false).is_some());
    assert!(timestamp_from_value(Some(1_725_000_000_000), true).is_some());
    assert_eq!(
        normalize_timestamp(Some("2026-08-06T08:00:00Z")),
        Some("2026-08-06T08:00:00+00:00".to_string())
    );
}

#[test]
fn native_identity_does_not_depend_on_a_display_title() {
    let empty = CliSessionSummary {
        id: "empty".to_string(),
        title: "未命名会话".to_string(),
        preview: None,
        model: None,
        models: Vec::new(),
        cli_kind: AgentCliKind::Codex,
        created_at: None,
        updated_at: None,
        workdir: "/tmp/project".to_string(),
        cli_version: None,
        archived: false,
        can_resume: true,
        metadata_source: "testSource".to_string(),
    };
    let named = CliSessionSummary {
        id: "named".to_string(),
        title: "BalanceHub".to_string(),
        ..empty.clone()
    };
    let records = [empty, named]
        .into_iter()
        .map(|summary| {
            let mut record = crate::services::agent_cli::contracts::SessionHistoryRecord::identity(
                summary.cli_kind,
                summary.id.clone(),
                std::path::Path::new(&summary.workdir),
                std::path::PathBuf::new(),
            );
            record.summary = summary;
            record
        })
        .collect();
    assert_eq!(super::normalize_records(records).len(), 2);
}

#[test]
fn multi_term_search_requires_every_term_across_fields() {
    let query = SearchQuery::new("BalanceHub proxy").unwrap();
    let mut matched = SearchAccumulator::new(&query);
    matched.observe("BalanceHub 会话检索");
    assert!(!matched.complete());
    matched.observe("修复 proxy 链路");
    assert!(matched.complete());
}

#[test]
fn large_jsonl_reads_the_beginning_and_latest_complete_records() {
    let path = std::env::temp_dir().join(format!(
        "balancehub-cli-session-window-test-{}.jsonl",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let content = (0..12)
        .map(|index| format!(r#"{{"id":{index},"text":"{}"}}"#, "x".repeat(24)))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, format!("{content}\n")).unwrap();

    let mut ids = Vec::new();
    let truncated =
        read_json_lines_limited(&path, 180, "读取测试会话", |_sequence, value| {
            if let Some(id) = value.get("id").and_then(serde_json::Value::as_i64) {
                ids.push(id);
            }
        })
        .unwrap();
    assert!(truncated);
    assert!(ids.contains(&0));
    assert!(ids.contains(&11));
    assert!(!ids.contains(&6));
    fs::remove_file(path).unwrap();
}

#[test]
fn streaming_jsonl_scan_reaches_middle_records_without_loading_the_file() {
    let path = std::env::temp_dir().join(format!(
        "balancehub-cli-session-stream-test-{}.jsonl",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let content = (0..2_000)
        .map(|index| {
            let text = if index == 1_237 {
                "middle-only-keyword".to_string()
            } else {
                "x".repeat(1_024)
            };
            serde_json::json!({"id": index, "text": text}).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, format!("{content}\n")).unwrap();

    let mut found = None;
    scan_json_lines_matching(
        &path,
        "检索测试会话",
        &SessionContentSearchRequest { terms: Vec::new() },
        &|| true,
        |_sequence, value| {
            if value.get("text").and_then(serde_json::Value::as_str) == Some("middle-only-keyword")
            {
                found = value.get("id").and_then(serde_json::Value::as_i64);
                true
            } else {
                false
            }
        },
    )
    .unwrap();
    assert_eq!(found, Some(1_237));
    fs::remove_file(path).unwrap();
}

#[test]
fn streaming_jsonl_scan_honors_cancellation() {
    let path = std::env::temp_dir().join(format!(
        "balancehub-cli-session-cancel-test-{}.jsonl",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    fs::write(&path, "{}\n{}\n").unwrap();
    let error = scan_json_lines_matching(
        &path,
        "检索测试会话",
        &SessionContentSearchRequest { terms: Vec::new() },
        &|| false,
        |_sequence, _value| false,
    )
    .unwrap_err();
    assert!(error.contains("新的搜索"));
    fs::remove_file(path).unwrap();
}

#[test]
fn background_jsonl_scan_checks_cancellation_at_the_throttle_boundary() {
    let path = std::env::temp_dir().join(format!(
        "balancehub-cli-session-background-cancel-test-{}.jsonl",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    fs::write(&path, format!("{}\n", "x".repeat(3 * 1024 * 1024))).unwrap();
    let cancellation_checks = AtomicUsize::new(0);
    let error = scan_json_records(
        &path,
        "索引测试会话",
        &|| cancellation_checks.fetch_add(1, Ordering::Relaxed) == 0,
        |_sequence, _line| false,
    )
    .unwrap_err();
    assert!(error.contains("新的搜索"));
    fs::remove_file(path).unwrap();
}

#[test]
fn streaming_jsonl_scan_skips_oversized_records_and_keeps_searching() {
    let path = std::env::temp_dir().join(format!(
        "balancehub-cli-session-oversized-test-{}.jsonl",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    let oversized = serde_json::json!({"text": "x".repeat(33 * 1024 * 1024)}).to_string();
    fs::write(
        &path,
        format!("{oversized}\n{{\"text\":\"after-oversized-keyword\"}}\n"),
    )
    .unwrap();
    let request = SessionContentSearchRequest {
        terms: vec![crate::services::agent_cli::contracts::SessionSearchTerm {
            index: 0,
            value: "after-oversized-keyword".to_string(),
        }],
    };
    let mut found = false;
    scan_json_lines_matching(
        &path,
        "检索测试会话",
        &request,
        &|| true,
        |_sequence, value| {
            found = value.get("text").and_then(serde_json::Value::as_str)
                == Some("after-oversized-keyword");
            found
        },
    )
    .unwrap();
    assert!(found);
    fs::remove_file(path).unwrap();
}
