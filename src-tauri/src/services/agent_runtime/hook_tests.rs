use super::{
    decoders::CodexHookDecoder,
    hook::{
        ingest_payload, ingest_payload_with_context, ingest_stdin, HookDecodeError,
        HookEventDecoder, HookIngestContext, HookSpoolLimits, HookSpoolRepository,
        NormalizedHookLifecycle, SpoolDiagnostic, DEFAULT_HOOK_PAYLOAD_MAX_BYTES,
        DEFAULT_SPOOL_MAX_AGE_MILLIS, NORMALIZED_HOOK_SCHEMA_VERSION,
    },
    reducer::AgentRuntimeEventKind,
};
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn temp_root(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("balancehub-hook-{label}-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn codex_payload(native_event: &str, observed_at: i64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "hook_event_name": native_event,
        "session_id": "session-1",
        "cwd": "/workspace",
        "transcript_path": "/workspace/session.jsonl",
        "model": "dynamic-model",
        "observed_at": observed_at,
        "prompt": "must never be persisted",
        "toolInput": {"secret": "must never be persisted"},
        "environment": {"TOKEN": "must never be persisted"}
    }))
    .unwrap()
}

#[test]
fn decoder_normalizes_metadata_and_discards_unlisted_fields() {
    let decoder = CodexHookDecoder;
    let event = decoder
        .decode(&codex_payload("UserPromptSubmit", 200), 200)
        .unwrap();
    assert_eq!(event.schema_version, NORMALIZED_HOOK_SCHEMA_VERSION);
    assert_eq!(event.agent_kind, crate::models::AgentCliKind::Codex);
    assert_eq!(event.lifecycle, NormalizedHookLifecycle::Busy);
    assert_eq!(event.model.as_deref(), Some("dynamic-model"));
    assert!(event.event_id.starts_with("hook:codex:"));
    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("must never be persisted"));
    assert!(!serialized.contains("toolInput"));
    assert!(!serialized.contains("environment"));
}

#[test]
fn codex_uses_official_snake_case_events_and_generates_stable_ids() {
    let decoder = CodexHookDecoder;
    let first = decoder.decode(&codex_payload("Stop", 321), 900).unwrap();
    let retry = decoder.decode(&codex_payload("Stop", 321), 901).unwrap();
    assert_eq!(first.lifecycle, NormalizedHookLifecycle::Idle);
    assert_eq!(first.event_id, retry.event_id);
    assert_eq!(first.received_at, 900);
    assert_eq!(retry.received_at, 901);
    assert!(decoder
        .decode(
            br#"{"schemaVersion":1,"eventId":"legacy","event":"stop"}"#,
            900
        )
        .is_err());
}

#[test]
fn ingest_context_is_the_only_balancehub_instance_association() {
    let root = temp_root("context");
    let spool = HookSpoolRepository::new(&root).unwrap();
    let context = HookIngestContext {
        runtime_scope: crate::models::AgentRuntimeScope::Wsl {
            distro_id: "Ubuntu".to_string(),
        },
        balancehub_instance_id: Some("instance-7".to_string()),
    };
    let mut payload =
        serde_json::from_slice::<serde_json::Value>(&codex_payload("SessionStart", 100)).unwrap();
    payload["balancehub_instance_id"] = serde_json::json!("spoofed");
    let payload = serde_json::to_vec(&payload).unwrap();
    assert!(
        ingest_payload_with_context(&CodexHookDecoder, &payload, 100, &spool, &context,).accepted
    );
    let event = &spool.read_batch(1, &BTreeSet::new()).unwrap().events[0].event;
    assert_eq!(event.balancehub_instance_id.as_deref(), Some("instance-7"));
    assert_eq!(event.runtime_scope, context.runtime_scope);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalized_hook_event_projects_lifecycle_and_optional_metadata() {
    let event = CodexHookDecoder
        .decode(&codex_payload("UserPromptSubmit", 200), 200)
        .unwrap();
    let runtime_events = event.into_runtime_events();
    assert_eq!(runtime_events.len(), 2);
    assert!(matches!(
        runtime_events[0].kind,
        AgentRuntimeEventKind::HookBusy
    ));
    assert!(matches!(
        runtime_events[1].kind,
        AgentRuntimeEventKind::Enrichment(_)
    ));
    assert_eq!(
        runtime_events[1].event_id,
        format!("{}:enrichment", runtime_events[0].event_id)
    );
}

#[test]
fn unsupported_schema_and_native_event_are_rejected() {
    let decoder = CodexHookDecoder;
    let unsupported = br#"{"schema_version":99,"hook_event_name":"start"}"#;
    assert!(matches!(
        decoder.decode(unsupported, 1),
        Err(HookDecodeError::UnsupportedSchema(99))
    ));
    let unknown = br#"{"schema_version":1,"hook_event_name":"new_vendor_event"}"#;
    assert!(matches!(
        decoder.decode(unknown, 1),
        Err(HookDecodeError::UnsupportedNativeEvent(_))
    ));
}

#[test]
fn spool_ack_is_the_only_path_that_removes_valid_events() {
    let root = temp_root("ack");
    let spool = HookSpoolRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    let result = ingest_payload(&decoder, &codex_payload("SessionStart", 100), 100, &spool);
    assert!(result.accepted);
    let batch = spool.read_batch(10, &BTreeSet::new()).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert!(batch.events[0].path.exists());
    spool.acknowledge(&batch).unwrap();
    assert!(!batch.events[0].path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_event_ids_are_returned_as_discardable_until_ack() {
    let root = temp_root("duplicate");
    let spool = HookSpoolRepository::new(&root).unwrap();
    let decoder = CodexHookDecoder;
    let event_id = decoder
        .decode(&codex_payload("SessionStart", 100), 100)
        .unwrap()
        .event_id;
    assert!(ingest_payload(&decoder, &codex_payload("SessionStart", 100), 100, &spool).accepted);
    assert!(ingest_payload(&decoder, &codex_payload("SessionStart", 100), 100, &spool).accepted);
    let mut known = BTreeSet::new();
    known.insert(event_id);
    let batch = spool.read_batch(10, &known).unwrap();
    assert!(batch.events.is_empty());
    assert_eq!(batch.duplicate_paths.len(), 2);
    spool.acknowledge(&batch).unwrap();
    assert!(spool
        .read_batch(10, &BTreeSet::new())
        .unwrap()
        .events
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn capacity_rejects_without_deleting_existing_events() {
    let root = temp_root("capacity");
    let limits = HookSpoolLimits {
        max_files: 1,
        max_bytes: 20 * 1024 * 1024,
        max_age_millis: DEFAULT_SPOOL_MAX_AGE_MILLIS,
        max_event_bytes: DEFAULT_HOOK_PAYLOAD_MAX_BYTES,
    };
    let spool = HookSpoolRepository::with_limits(&root, limits).unwrap();
    let decoder = CodexHookDecoder;
    assert!(ingest_payload(&decoder, &codex_payload("SessionStart", 100), 100, &spool).accepted);
    let result = ingest_payload(
        &decoder,
        &codex_payload("UserPromptSubmit", 101),
        101,
        &spool,
    );
    assert_eq!(result.diagnostic, Some(SpoolDiagnostic::CapacityFiles));
    assert_eq!(
        spool.read_batch(10, &BTreeSet::new()).unwrap().events.len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_and_unknown_files_are_quarantined_while_valid_batch_survives() {
    let root = temp_root("quarantine");
    let spool = HookSpoolRepository::new(&root).unwrap();
    fs::write(spool.incoming_dir().join("event-corrupt.json"), b"not-json").unwrap();
    fs::write(
        spool.incoming_dir().join("event-unknown.json"),
        br#"{"schemaVersion":99,"eventId":"e","event":"start"}"#,
    )
    .unwrap();
    let decoder = CodexHookDecoder;
    assert!(ingest_payload(&decoder, &codex_payload("SessionStart", 100), 100, &spool).accepted);
    let batch = spool.read_batch(10, &BTreeSet::new()).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.quarantined_paths.len(), 2);
    assert_eq!(
        fs::read_dir(root.join("hook-spool/quarantine"))
            .unwrap()
            .count(),
        2
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_ingest_respects_file_limit_and_keeps_files_parseable() {
    let root = temp_root("concurrent");
    let spool = Arc::new(
        HookSpoolRepository::with_limits(
            &root,
            HookSpoolLimits {
                max_files: 8,
                ..HookSpoolLimits::default()
            },
        )
        .unwrap(),
    );
    let mut handles = Vec::new();
    for index in 0..32 {
        let spool = Arc::clone(&spool);
        handles.push(thread::spawn(move || {
            let decoder = CodexHookDecoder;
            ingest_payload(
                &decoder,
                &codex_payload("SessionStart", 100 + index),
                100,
                &spool,
            )
        }));
    }
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.accepted).count(), 8);
    let batch = spool.read_batch(100, &BTreeSet::new()).unwrap();
    assert_eq!(batch.events.len(), 8);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symlinked_spool_root_is_rejected() {
    let root = temp_root("symlink");
    let real = root.join("real");
    let alias = root.join("alias");
    fs::create_dir_all(&real).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    #[cfg(unix)]
    assert!(matches!(
        HookSpoolRepository::new(&alias),
        Err(SpoolDiagnostic::UnsafePath)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stdin_ingest_has_a_hard_payload_limit_and_fail_open_result() {
    let root = temp_root("stdin");
    let limits = HookSpoolLimits {
        max_event_bytes: 8,
        ..HookSpoolLimits::default()
    };
    let spool = HookSpoolRepository::with_limits(&root, limits).unwrap();
    let decoder = CodexHookDecoder;
    let result = ingest_stdin(&decoder, &mut Cursor::new(b"0123456789"), 100, &spool);
    assert_eq!(result.diagnostic, Some(SpoolDiagnostic::PayloadTooLarge));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_events_are_not_expired_until_retention_window_is_reached() {
    let root = temp_root("retention");
    let limits = HookSpoolLimits {
        max_age_millis: 1,
        ..HookSpoolLimits::default()
    };
    let spool = HookSpoolRepository::with_limits(&root, limits).unwrap();
    let decoder = CodexHookDecoder;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(ingest_payload(&decoder, &codex_payload("SessionStart", now), now, &spool).accepted);
    std::thread::sleep(Duration::from_millis(2));
    assert!(
        ingest_payload(
            &decoder,
            &codex_payload("SessionStart", now + 10_000),
            now + 10_000,
            &spool
        )
        .accepted
    );
    let batch = spool.read_batch(10, &BTreeSet::new()).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(
        batch.events[0].event.event_id,
        decoder
            .decode(&codex_payload("SessionStart", now + 10_000), now + 10_000)
            .unwrap()
            .event_id
    );
    fs::remove_dir_all(root).unwrap();
}
