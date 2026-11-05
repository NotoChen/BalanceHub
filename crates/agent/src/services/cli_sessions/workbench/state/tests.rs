use super::*;
use crate::{
    models::CliSessionMessageRole,
    services::agent_cli::contracts::{
        SessionTranscriptFormat, SessionTranscriptMessage, SessionTranscriptMutation,
        SessionTranscriptSource,
    },
};
use serde_json::Value;
use std::{fs, path::Path};

fn scope(revision: &str, explicit_workdir: Option<&str>) -> ScopeState {
    ScopeState {
        public: AgentSessionScope {
            revision: revision.into(),
            workspaces: Vec::new(),
            sources: Vec::new(),
        },
        explicit_workdir: explicit_workdir.map(str::to_owned),
        sources: HashMap::new(),
    }
}

fn snapshot(actor: &str, consumer: &str, revision: &str) -> Snapshot {
    Snapshot {
        actor: actor.into(),
        consumer: consumer.into(),
        scope_revision: revision.into(),
        query_key: "query".into(),
        rows: Vec::new(),
        parent_updates: HashMap::new(),
        states: Vec::new(),
        workspaces: Vec::new(),
        complete: true,
        progress: QueryProgress::default(),
        used_at: Instant::now(),
    }
}

#[test]
fn a_visible_list_keeps_its_scope_and_cursor_after_a_long_reading_pause() {
    let mut registry = Registry::default();
    registry.register_scope("window".into(), scope("scope", None));
    registry.store_snapshot("page".into(), snapshot("window", "history", "scope"));
    registry.snapshots.get_mut("page").unwrap().used_at =
        Instant::now() - Duration::from_secs(2 * 60 * 60);
    registry.prune();
    assert!(registry
        .scopes
        .contains_key(&("window".into(), "scope".into())));
    assert!(registry.snapshots.contains_key("page"));

    registry.register_scope("window".into(), scope("scope", None));
    assert!(registry.snapshots.contains_key("page"));
}

#[test]
fn a_changed_scope_retires_its_old_cursor_without_invalidating_other_contexts() {
    let mut registry = Registry::default();
    registry.register_scope("window".into(), scope("old", None));
    registry.register_scope("window".into(), scope("explicit", Some("/project")));
    registry.register_scope("other-window".into(), scope("old", None));
    registry.store_snapshot("history".into(), snapshot("window", "history", "old"));
    registry.store_snapshot("picker".into(), snapshot("window", "picker", "explicit"));
    registry.store_snapshot("other".into(), snapshot("other-window", "history", "old"));

    registry.register_scope("window".into(), scope("new", None));
    assert!(!registry
        .scopes
        .contains_key(&("window".into(), "old".into())));
    assert!(!registry.snapshots.contains_key("history"));
    assert!(registry.snapshots.contains_key("picker"));
    assert!(registry.snapshots.contains_key("other"));
}

#[test]
fn new_searches_replace_only_their_own_previous_snapshot() {
    let mut registry = Registry::default();
    registry.register_scope("window".into(), scope("scope", None));
    registry.register_scope("other-window".into(), scope("scope", None));
    registry.store_snapshot("old".into(), snapshot("window", "history", "scope"));
    registry.store_snapshot("picker".into(), snapshot("window", "picker", "scope"));
    registry.store_snapshot("other".into(), snapshot("other-window", "history", "scope"));
    for index in 0..40 {
        registry.store_snapshot(
            format!("query-{index}"),
            snapshot("window", "history", "scope"),
        );
    }
    assert_eq!(registry.snapshots.len(), 3);
    assert!(!registry.snapshots.contains_key("old"));
    assert!(registry.snapshots.contains_key("query-39"));
    assert!(registry.snapshots.contains_key("picker"));
    assert!(registry.snapshots.contains_key("other"));
}

#[test]
fn snapshot_pressure_reclaims_the_least_recently_used_consumer() {
    let mut registry = Registry::default();
    registry.register_scope("window".into(), scope("scope", None));
    for index in 0..32 {
        registry.store_snapshot(
            format!("page-{index}"),
            snapshot("window", &format!("consumer-{index}"), "scope"),
        );
    }
    registry.snapshots.get_mut("page-17").unwrap().used_at =
        Instant::now() - Duration::from_secs(2 * 60 * 60);
    registry.store_snapshot("latest".into(), snapshot("window", "latest", "scope"));
    assert_eq!(registry.snapshots.len(), 32);
    assert!(!registry.snapshots.contains_key("page-17"));
    assert!(registry.snapshots.contains_key("latest"));
}

fn decode(_: &Path, _: u64, bytes: &[u8], _: &mut Value) -> Vec<SessionTranscriptMutation> {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    vec![SessionTranscriptMutation::Put {
        message: SessionTranscriptMessage {
            id: "message".into(),
            role: CliSessionMessageRole::User,
            content: value["content"].as_str().unwrap_or_default().into(),
            metadata: Default::default(),
        },
        priority: 0,
    }]
}

fn transcript(root: &Path, session: &str) -> Arc<Transcript> {
    let path = root.join(format!("{session}.jsonl"));
    fs::write(&path, format!("{{\"content\":\"{session}\"}}\n")).unwrap();
    let source = SessionTranscriptSource {
        path,
        parser_version: 1,
        format: SessionTranscriptFormat::JsonLines,
        decode,
    };
    let budget = SessionReadBudget::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
        u64::MAX,
    );
    Arc::new(Transcript::from_sources("workspace", session, &[source], &budget).unwrap())
}

#[test]
fn transcript_cache_keeps_recent_windows_and_finds_them_by_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = TranscriptCacheData {
        entries: lru::LruCache::unbounded(),
        weights: HashMap::new(),
        bytes: 0,
    };
    let revisions = (0..10)
        .map(|index| {
            let transcript = transcript(root.path(), &format!("session-{index}"));
            let revision = transcript.revision().to_string();
            cache.put(format!("key-{index}"), transcript);
            revision
        })
        .collect::<Vec<_>>();
    assert_eq!(cache.entries.len(), CACHE_ENTRIES);
    assert_eq!(cache.weights.len(), CACHE_ENTRIES);
    assert_eq!(cache.bytes, cache.weights.values().sum::<usize>());
    assert!(cache.find_by_revision(&revisions[0]).is_none());
    assert!(cache.find_by_revision(&revisions[9]).is_some());
}
