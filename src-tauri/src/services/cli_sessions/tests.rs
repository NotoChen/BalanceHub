use super::{
    clean_text, first_non_empty, is_empty_shell, normalize_timestamp, timestamp_from_value,
};
use crate::models::{AgentCliKind, CliSessionSummary};

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
fn empty_shell_sessions_are_filtered_without_touching_source_data() {
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
    assert!(is_empty_shell(&empty));
    assert!(!is_empty_shell(&named));
}
