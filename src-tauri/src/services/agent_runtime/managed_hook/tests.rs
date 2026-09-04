use super::CodexHookService;
use crate::models::{
    AgentCliKind, AgentHookChangeKind, AgentHookHealthState, AgentHookMutation, AgentRuntimeScope,
};
use crate::services::agent_runtime::{
    decoders::CodexHookDecoder,
    hook::{ingest_payload, HookSpoolRepository},
    repository::AgentRuntimeRepository,
};
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = env::temp_dir().join(format!("balancehub-managed-codex-{label}-{nonce}"));
    fs::create_dir_all(path.join(".codex")).unwrap();
    fs::create_dir_all(path.join("app")).unwrap();
    path
}

fn service(root: &Path) -> CodexHookService {
    CodexHookService::new(
        root.join(".codex/hooks.json"),
        root.join("app/ownership.json"),
        root.join("app/balancehub"),
        root.join("app"),
    )
}

#[test]
fn install_preserves_unknown_fields_and_is_idempotent() {
    let root = temp_root("install");
    let service = service(&root);
    fs::write(root.join(".codex/hooks.json"), br#"{"custom":{"keep":true},"hooks":{"Other":[{"matcher":"*","hooks":[{"type":"command","command":"user"}]}]}}"#).unwrap();
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();
    let value: Value =
        serde_json::from_slice(&fs::read(root.join(".codex/hooks.json")).unwrap()).unwrap();
    assert_eq!(value["custom"]["keep"], true);
    assert_eq!(value["hooks"]["Other"][0]["hooks"][0]["command"], "user");
    let second_plan = service.plan(AgentHookMutation::Install);
    assert!(second_plan
        .changes
        .iter()
        .all(|change| change.kind == AgentHookChangeKind::Keep));
    service.apply(second_plan).unwrap();
    assert!(service.inspect().enabled);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn drift_and_revision_conflict_never_write() {
    let root = temp_root("conflict");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    fs::write(root.join(".codex/hooks.json"), b"{}\n").unwrap();
    let plan = service.plan(AgentHookMutation::Install);
    fs::write(root.join(".codex/hooks.json"), b"{\"user\":true}\n").unwrap();
    assert!(service.apply(plan).is_err());
    assert_eq!(
        fs::read(root.join(".codex/hooks.json")).unwrap(),
        b"{\"user\":true}\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn forged_plan_target_or_changes_never_write() {
    let root = temp_root("forged-plan");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    let path = root.join(".codex/hooks.json");
    fs::write(&path, b"{}\n").unwrap();
    let original = fs::read(&path).unwrap();

    let mut wrong_agent = service.plan(AgentHookMutation::Install);
    wrong_agent.agent_kind = AgentCliKind::ClaudeCode;
    assert!(service.apply(wrong_agent).is_err());

    let mut wrong_scope = service.plan(AgentHookMutation::Install);
    wrong_scope.runtime_scope = AgentRuntimeScope::Wsl {
        distro_id: "Ubuntu".to_string(),
    };
    assert!(service.apply(wrong_scope).is_err());

    let mut wrong_path = service.plan(AgentHookMutation::Install);
    wrong_path.config_path = root.join("other.json").to_string_lossy().into_owned();
    assert!(service.apply(wrong_path).is_err());

    let mut changed_plan = service.plan(AgentHookMutation::Install);
    changed_plan.changes[0].fingerprint = "forged".to_string();
    assert!(service.apply(changed_plan).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn disable_keeps_manifest_and_remove_requires_matching_fingerprint() {
    let root = temp_root("lifecycle");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    fs::write(root.join(".codex/hooks.json"), b"{}\n").unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();
    service
        .apply(service.plan(AgentHookMutation::Disable))
        .unwrap();
    assert!(root.join("app/ownership.json").exists());
    assert!(!service.inspect().enabled);
    service
        .apply(service.plan(AgentHookMutation::Enable))
        .unwrap();
    let path = root.join(".codex/hooks.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["hooks"]["SessionStart"][0]["hooks"][0]["command"] =
        Value::String("user changed".to_string());
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(service.plan(AgentHookMutation::Remove).conflict);
    assert!(service
        .apply(service.plan(AgentHookMutation::Remove))
        .is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn matcher_group_drift_is_a_conflict_and_is_never_removed() {
    let root = temp_root("matcher-drift");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    fs::write(root.join(".codex/hooks.json"), b"{}\n").unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();

    let path = root.join(".codex/hooks.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["hooks"]["SessionStart"][0]["matcher"] = Value::String("startup".to_string());
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let before_remove = fs::read(&path).unwrap();

    let plan = service.plan(AgentHookMutation::Remove);
    assert!(plan.conflict);
    assert!(service.apply(plan).is_err());
    assert_eq!(fs::read(&path).unwrap(), before_remove);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_fixture_event_is_required_before_healthy() {
    let root = temp_root("verify");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    fs::write(root.join(".codex/hooks.json"), b"{}\n").unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();
    assert_eq!(
        service.inspect().state,
        AgentHookHealthState::InstalledUnverified
    );
    let spool = HookSpoolRepository::new(root.join("app")).unwrap();
    let payload =
        serde_json::json!({"hook_event_name":"SessionStart","session_id":"s","cwd":"/tmp"});
    let event_time = now_millis();
    assert!(
        ingest_payload(
            &CodexHookDecoder,
            &serde_json::to_vec(&payload).unwrap(),
            event_time,
            &spool,
        )
        .accepted
    );
    assert_eq!(service.inspect().state, AgentHookHealthState::Healthy);

    // Runtime refresh acknowledges incoming records after committing the
    // durable projection. Health must continue to observe the consumed event.
    AgentRuntimeRepository::new(root.join("app"))
        .unwrap()
        .refresh_with_launch_snapshots_at(&[], event_time + 1)
        .unwrap();
    let inspection = service.inspect();
    assert_eq!(inspection.state, AgentHookHealthState::Healthy);
    assert_eq!(inspection.last_event_at, Some(event_time));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_disabled_hook_does_not_rewrite_user_bytes() {
    let root = temp_root("disabled-remove");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    let original = br#"{"user":true}
"#;
    fs::write(root.join(".codex/hooks.json"), original).unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();
    service
        .apply(service.plan(AgentHookMutation::Disable))
        .unwrap();
    let before_remove = fs::read(root.join(".codex/hooks.json")).unwrap();
    service
        .apply(service.plan(AgentHookMutation::Remove))
        .unwrap();
    assert_eq!(
        fs::read(root.join(".codex/hooks.json")).unwrap(),
        before_remove
    );
    assert!(!root.join("app/ownership.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_enabled_config_is_a_conflict() {
    let root = temp_root("missing-config");
    let service = service(&root);
    fs::write(root.join("app/balancehub"), b"helper").unwrap();
    fs::write(root.join(".codex/hooks.json"), b"{}\n").unwrap();
    service
        .apply(service.plan(AgentHookMutation::Install))
        .unwrap();
    fs::remove_file(root.join(".codex/hooks.json")).unwrap();
    let plan = service.plan(AgentHookMutation::Remove);
    assert!(plan.conflict);
    assert!(service.apply(plan).is_err());
    assert!(root.join("app/ownership.json").exists());
    fs::remove_dir_all(root).unwrap();
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
