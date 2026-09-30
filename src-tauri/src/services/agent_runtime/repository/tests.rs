use super::*;
use crate::models::{
    AgentCliKind, AgentRuntimeOrigin, AgentRuntimeScope, AgentRuntimeState,
    TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
};
use crate::services::agent_runtime::decoders::CodexHookDecoder;
use crate::services::agent_runtime::hook::{
    ingest_payload, ingest_payload_with_context, HookEventDecoder, HookIngestContext,
    HookSpoolLimits, NormalizedHookLifecycle,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

trait RepositoryTestAccess {
    fn projection_path(&self) -> &Path;
    fn spool(&self) -> &super::super::hook::HookSpoolRepository;
    fn refresh_at(
        &self,
        observed_at: i64,
    ) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError>;
}

impl RepositoryTestAccess for AgentRuntimeRepository {
    fn projection_path(&self) -> &Path {
        &self.projection_path
    }

    fn spool(&self) -> &super::super::hook::HookSpoolRepository {
        &self.spool
    }

    fn refresh_at(
        &self,
        observed_at: i64,
    ) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError> {
        self.refresh_with_launch_snapshots_at(&[], observed_at)
    }
}

fn next_nonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!(
        "{millis}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("balancehub-runtime-{label}-{}", next_nonce()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn payload(event: &str, observed_at: i64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "hook_event_name": event,
        "session_id": "session-1",
        "cwd": "/workspace",
        "model": "model-1",
        "observed_at": observed_at,
        "prompt": "must not be persisted"
    }))
    .unwrap()
}

#[test]
fn refresh_commits_projection_before_ack_and_is_idempotent() {
    let root = temp_root("ack");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    let first = repository.refresh_at(20).unwrap();
    assert_eq!(first.sessions.len(), 1);
    assert_eq!(first.sessions[0].state, AgentRuntimeState::Idle);
    assert!(repository
        .spool()
        .read_batch(10, &BTreeSet::new())
        .unwrap()
        .events
        .is_empty());
    let before_bytes = fs::read(repository.projection_path()).unwrap();
    let before_modified = fs::metadata(repository.projection_path())
        .unwrap()
        .modified()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let second = repository.refresh_at(30).unwrap();
    assert_eq!(second.sessions, first.sessions);
    assert_eq!(second.revision, first.revision);
    assert_eq!(
        fs::read(repository.projection_path()).unwrap(),
        before_bytes
    );
    assert_eq!(
        fs::metadata(repository.projection_path())
            .unwrap()
            .modified()
            .unwrap(),
        before_modified
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_projection_write_leaves_incoming_event() {
    let root = temp_root("failure");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    fs::write(repository.projection_path(), b"not-json").unwrap();
    assert_eq!(
        repository.refresh_at(20),
        Err(AgentRuntimeRepositoryError::CorruptProjection)
    );
    assert_eq!(
        repository
            .spool()
            .read_batch(10, &BTreeSet::new())
            .unwrap()
            .events
            .len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_events_in_one_batch_are_acknowledged_after_commit() {
    let root = temp_root("duplicates");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            11,
            repository.spool()
        )
        .accepted
    );
    let snapshot = repository.refresh_at(20).unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert!(repository
        .spool()
        .read_batch(10, &BTreeSet::new())
        .unwrap()
        .events
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn persisted_history_rebuilds_projection_after_restart() {
    let root = temp_root("restart");
    let first = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("UserPromptSubmit", 10),
            10,
            first.spool()
        )
        .accepted
    );
    let expected = first.refresh_at(10).unwrap();
    drop(first);
    let restarted = AgentRuntimeRepository::new(&root).unwrap();
    assert_eq!(restarted.snapshot().unwrap().sessions, expected.sessions);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_external_runtime_becomes_unknown_without_inventing_an_exit() {
    let root = temp_root("external-stale");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    let active = repository.refresh_at(10).unwrap();
    assert_eq!(active.sessions[0].state, AgentRuntimeState::Idle);

    let stale = repository
        .refresh_at(10 + EXTERNAL_RUNTIME_STALE_AFTER_MILLIS + 1)
        .unwrap();
    assert_eq!(stale.sessions[0].state, AgentRuntimeState::Unknown);
    assert_eq!(stale.sessions[0].last_activity_at, Some(10));
    assert_eq!(stale.sessions[0].ended_at, None);

    let resumed_at = 10 + EXTERNAL_RUNTIME_STALE_AFTER_MILLIS + 2;
    assert!(
        ingest_payload(
            &decoder,
            &payload("UserPromptSubmit", resumed_at),
            resumed_at,
            repository.spool()
        )
        .accepted
    );
    let resumed = repository.refresh_at(resumed_at).unwrap();
    assert_eq!(resumed.sessions[0].state, AgentRuntimeState::Busy);
    assert_eq!(resumed.sessions[0].last_activity_at, Some(resumed_at));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sensitive_payload_fields_never_enter_projection_file() {
    let root = temp_root("privacy");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    repository.refresh_at(10).unwrap();
    let bytes = fs::read(repository.projection_path()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("must not be persisted"));
    assert!(!text.contains("prompt"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn enrichment_from_an_older_hook_cannot_commit_after_a_newer_hook() {
    let root = temp_root("stale-enrichment");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload(
            &decoder,
            &payload("SessionStart", 10),
            10,
            repository.spool()
        )
        .accepted
    );
    let old_outcome = repository
        .refresh_with_launch_snapshots_outcome_at(&[], 10)
        .unwrap();
    let old_request = old_outcome.enrichment_requests.into_iter().next().unwrap();

    assert!(
        ingest_payload(
            &decoder,
            &payload("UserPromptSubmit", 20),
            20,
            repository.spool()
        )
        .accepted
    );
    let current_outcome = repository
        .refresh_with_launch_snapshots_outcome_at(&[], 20)
        .unwrap();
    let current_request = current_outcome.enrichment_requests[0].clone();
    let current = current_outcome.snapshot;
    let stale = repository
        .append_enrichment_if_current(
            &old_request,
            super::super::reducer::RuntimeEnrichment {
                source: super::super::reducer::RuntimeEnrichmentSource::SessionAdapter,
                title: Some("stale title".to_string()),
                model: None,
                workdir: None,
                source_last_activity_at: None,
                source_revision: Some("stale-revision".to_string()),
            },
        )
        .unwrap();
    assert_eq!(stale.revision, current.revision);
    assert_eq!(stale.sessions[0].title, current.sessions[0].title);

    let updated = repository
        .append_enrichment_if_current(
            &current_request,
            super::super::reducer::RuntimeEnrichment {
                source: super::super::reducer::RuntimeEnrichmentSource::SessionAdapter,
                title: Some("current title".to_string()),
                model: None,
                workdir: None,
                source_last_activity_at: None,
                source_revision: Some("current-revision".to_string()),
            },
        )
        .unwrap();
    assert_eq!(updated.revision, current.revision + 1);
    assert_eq!(updated.sessions[0].title.as_deref(), Some("current title"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn launch_snapshots_and_hooks_share_the_reducer_projection() {
    let root = temp_root("launch");
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let instance = TemporaryCliInstance {
        id: "instance-1".to_string(),
        native_session: None,
        provider_id: Some("provider-1".to_string()),
        provider_name: Some("Provider".to_string()),
        session_title: "Title".to_string(),
        account_label: "Account".to_string(),
        api_key_local_id: Some("key-1".to_string()),
        cli_kind: AgentCliKind::Codex,
        workdir: "/workspace".to_string(),
        terminal_kind: TemporaryCliTerminalKind::default(),
        terminal_name: TemporaryCliTerminalKind::default().label().to_string(),
        terminal_locator: None,
        started_at: "10".to_string(),
        ended_at: None,
        pid: Some(42),
        status: TemporaryCliInstanceStatus::Running,
        exit_code: None,
        can_activate: false,
    };
    let decoder = CodexHookDecoder;
    assert!(
        ingest_payload_with_context(
            &decoder,
            &payload("UserPromptSubmit", 20),
            20,
            repository.spool(),
            &HookIngestContext {
                runtime_scope: AgentRuntimeScope::Native,
                balancehub_instance_id: Some("instance-1".to_string()),
            }
        )
        .accepted
    );
    let snapshot = repository
        .refresh_with_launch_snapshots_at(&[instance], 20)
        .unwrap();
    let session = &snapshot.sessions[0];
    assert_eq!(session.origin, AgentRuntimeOrigin::BalancehubLaunch);
    assert_eq!(
        session.balancehub_instance_id.as_deref(),
        Some("instance-1")
    );
    assert_eq!(session.runtime_scope, AgentRuntimeScope::Native);
    assert_eq!(session.state, AgentRuntimeState::Busy);
    assert_eq!(
        session.process.as_ref().map(|process| process.pid),
        Some(42)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_hook_event_is_quarantined_without_blocking_refresh() {
    let root = temp_root("quarantine");
    let limits = HookSpoolLimits {
        max_files: 10,
        max_bytes: 1024 * 1024,
        max_age_millis: 0,
        max_event_bytes: 1024,
    };
    let spool = HookSpoolRepository::with_limits(&root, limits).unwrap();
    let corrupt = spool.incoming_dir().join("event-corrupt.json");
    fs::write(&corrupt, b"not-json").unwrap();
    let repository = AgentRuntimeRepository::new(&root).unwrap();
    let snapshot = repository.refresh_at(20).unwrap();
    assert!(snapshot.sessions.is_empty());
    assert!(!corrupt.exists());
    assert!(root
        .join("hook-spool/quarantine/event-corrupt.json")
        .exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalized_event_contract_drops_unlisted_lifecycle_payload() {
    let event = CodexHookDecoder.decode(&payload("Stop", 10), 10).unwrap();
    assert_eq!(event.lifecycle, NormalizedHookLifecycle::Idle);
    let runtime_events = event.into_runtime_events();
    assert!(runtime_events.iter().all(|event| {
        !serde_json::to_string(event)
            .unwrap()
            .contains("must not be persisted")
    }));
}
