//! Explicit transfer from optional session integration to user-owned Hook edits.
//! Names and paths alone never establish ownership: the saved complete native
//! resource must occur exactly once in the captured document before the edit.

use super::{codex, generic, GenericAgent};
use crate::models::{AgentCliKind, AgentRuntimeScope};
use crate::services::agent_cli::config_support::parse_jsonc_document;
use crate::services::agent_cli::environment::{
    config_document::{self, ConfigDocumentFormat},
    mutation::GuardedFile,
};
use codex::ownership::ManifestFile;
use serde_json::Value;
use std::path::Path;

#[derive(Clone)]
pub(crate) struct ManagedHookCatalogChange {
    pub file: GuardedFile,
    pub replacement: Vec<u8>,
    pub note: String,
}

pub(crate) fn prepare_catalog_change(
    app_data_root: &Path,
    agent: AgentCliKind,
    source_path: &Path,
    before: &[u8],
    after: &[u8],
) -> Result<Option<ManagedHookCatalogChange>, String> {
    if before == after {
        return Ok(None);
    }
    let Some((file, manifest)) = active_manifest(app_data_root, agent, source_path)? else {
        return Ok(None);
    };
    let changed = match agent {
        AgentCliKind::Grok => standalone_owned(&manifest, before),
        _ => {
            let Some(before) = decode(agent, before) else {
                return Ok(None);
            };
            let Some(after) = decode(agent, after) else {
                return Err("Hook 变更后的配置无法验证，未解除会话接入所有权".to_owned());
            };
            match agent {
                AgentCliKind::Codex => {
                    codex::owned_resources_changed(&before, &after, &manifest.ownership)
                }
                AgentCliKind::ClaudeCode => generic::owned_resources_changed(
                    GenericAgent::ClaudeCode,
                    &before,
                    &after,
                    &manifest.ownership,
                ),
                AgentCliKind::Gemini => generic::owned_resources_changed(
                    GenericAgent::Gemini,
                    &before,
                    &after,
                    &manifest.ownership,
                ),
                AgentCliKind::Grok => unreachable!("standalone ownership handled above"),
            }
        }
    };
    changed.then(|| detached_change(file, manifest)).transpose()
}

/// Native adapters determine which rules a policy write actually affects. A
/// policy filename, node label or group index is never ownership evidence. The
/// caller guards this original definition source alongside the policy write.
pub(crate) fn prepare_catalog_policy_change(
    app_data_root: &Path,
    agent: AgentCliKind,
    source_path: &Path,
    before: &[u8],
    event: &str,
    original_group: &Value,
) -> Result<Option<ManagedHookCatalogChange>, String> {
    let Some((file, manifest)) = active_manifest(app_data_root, agent, source_path)? else {
        return Ok(None);
    };
    let Some(document) = decode(agent, before) else {
        return Ok(None);
    };
    let selected = match agent {
        AgentCliKind::Codex => {
            codex::owned_resource_selected(&document, event, original_group, &manifest.ownership)
        }
        AgentCliKind::ClaudeCode => generic::owned_resource_selected(
            GenericAgent::ClaudeCode,
            &document,
            event,
            original_group,
            &manifest.ownership,
        ),
        AgentCliKind::Gemini => generic::owned_resource_selected(
            GenericAgent::Gemini,
            &document,
            event,
            original_group,
            &manifest.ownership,
        ),
        AgentCliKind::Grok => {
            standalone_owned(&manifest, before)
                && document
                    .get("hooks")
                    .and_then(|hooks| hooks.get(event))
                    .and_then(Value::as_array)
                    .is_some_and(|groups| {
                        groups
                            .iter()
                            .filter(|group| *group == original_group)
                            .count()
                            == 1
                    })
        }
    };
    selected
        .then(|| detached_change(file, manifest))
        .transpose()
}

fn decode(agent: AgentCliKind, bytes: &[u8]) -> Option<Value> {
    if agent == AgentCliKind::Gemini {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| parse_jsonc_document(text).ok())
    } else {
        config_document::parse(bytes, ConfigDocumentFormat::Json)
    }
}

fn standalone_owned(manifest: &ManifestFile, before: &[u8]) -> bool {
    manifest.ownership.resources.len() == 1
        && manifest.ownership.resources[0].content_fingerprint
            == codex::ownership::revision_for_bytes(before)
}

/// Configuration editing guards the same ownership receipt without detaching it.
pub(crate) fn configuration_ownership_guard(
    app_data_root: &Path,
    agent: AgentCliKind,
) -> Result<GuardedFile, String> {
    let path = app_data_root
        .join("agent-hooks")
        .join(agent.key())
        .join("ownership.json");
    GuardedFile::capture_path(app_data_root, &path, 1024 * 1024)
        .map_err(|_| "会话接入所有权记录无法安全读取，请先检查该接入状态".to_owned())
}

fn active_manifest(
    app_data_root: &Path,
    agent: AgentCliKind,
    source_path: &Path,
) -> Result<Option<(GuardedFile, ManifestFile)>, String> {
    let file = configuration_ownership_guard(app_data_root, agent)?;
    let Some(bytes) = file.bytes() else {
        return Ok(None);
    };
    let Ok(manifest) = serde_json::from_slice::<ManifestFile>(bytes) else {
        // Invalid receipts already grant no ownership. They cannot reserve a
        // user's otherwise valid native rule or be overwritten by this path.
        return Ok(None);
    };
    if manifest.catalog_detached
        || manifest.schema_version != codex::MANIFEST_SCHEMA_VERSION
        || manifest.ownership.agent_kind != agent
        || manifest.ownership.runtime_scope != AgentRuntimeScope::Native
        || Path::new(&manifest.ownership.config_path) != source_path
    {
        return Ok(None);
    }
    Ok(Some((file, manifest)))
}

fn detached_change(
    file: GuardedFile,
    mut manifest: ManifestFile,
) -> Result<ManagedHookCatalogChange, String> {
    manifest.catalog_detached = true;
    manifest.ownership.enabled = false;
    let replacement = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| "无法保存会话接入所有权变更".to_owned())?;
    Ok(ManagedHookCatalogChange {
        file,
        replacement,
        note: "此规则属于 BalanceHub 会话状态接入；修改后将解除该接入的专用管理关系，其余原生规则保留，可在全局 Hook 列表继续管理。专用控制器不会自动恢复。".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::models::AgentHookHealthState;
    use crate::models::AgentHookMutation;
    #[cfg(unix)]
    use crate::services::agent_cli::environment::mutation::atomic;
    use crate::services::agent_runtime::managed_hook::CodexHookService;
    use serde_json::{json, Value};
    use std::fs;

    fn fixture() -> (tempfile::TempDir, CodexHookService) {
        let directory = tempfile::tempdir().unwrap();
        let root = &directory.path().canonicalize().unwrap();
        fs::create_dir(root.join(".codex")).unwrap();
        fs::create_dir(root.join("app-data")).unwrap();
        fs::write(root.join("app-data/balancehub"), b"helper").unwrap();
        fs::write(root.join(".codex/hooks.json"), br#"{"untouched":true}"#).unwrap();
        let service = CodexHookService::new(
            root.join(".codex/hooks.json"),
            root.join("app-data/agent-hooks/codex/ownership.json"),
            root.join("app-data/balancehub"),
            root.join("app-data"),
        );
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        (directory, service)
    }

    #[test]
    fn user_rule_changes_do_not_take_over_session_ownership() {
        let (directory, service) = fixture();
        let root = &directory.path().canonicalize().unwrap();
        let before = fs::read(&service.config_path).unwrap();
        let manifest_before = fs::read(&service.manifest_path).unwrap();
        let mut after: Value = serde_json::from_slice(&before).unwrap();
        after["hooks"]["UserDefined"] =
            json!([{ "hooks": [{"type": "command", "command": "user-created"}] }]);
        let after = serde_json::to_vec(&after).unwrap();
        assert!(prepare_catalog_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &before,
            &after,
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read(&service.manifest_path).unwrap(), manifest_before);
    }

    #[cfg(unix)]
    #[test]
    fn exact_owned_rule_customization_detaches_auxiliary_controller_without_repair() {
        let (directory, service) = fixture();
        let root = &directory.path().canonicalize().unwrap();
        let before = fs::read(&service.config_path).unwrap();
        let mut after: Value = serde_json::from_slice(&before).unwrap();
        let unrelated = after["hooks"]["Stop"].clone();
        after["hooks"]["SessionStart"][0]["hooks"][0]["command"] = json!("user-custom-command");
        let after = serde_json::to_vec(&after).unwrap();
        let change = prepare_catalog_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &before,
            &after,
        )
        .unwrap()
        .unwrap();
        assert!(change.note.contains("解除"));
        // Production applies these guarded writes under the common domains.
        fs::write(&service.config_path, &after).unwrap();
        atomic::replace(&change.file, &change.replacement, || Ok(())).unwrap();
        let inspection = service.inspect();
        assert_eq!(inspection.state, AgentHookHealthState::Conflict);
        assert!(inspection.ownership.is_none());
        assert!(inspection
            .diagnostics
            .iter()
            .any(|note| note.contains("全局 Hook 管理")));
        assert!(service.plan(AgentHookMutation::Install).conflict);
        assert!(service
            .apply(service.plan(AgentHookMutation::Install))
            .is_err());
        let current: Value =
            serde_json::from_slice(&fs::read(&service.config_path).unwrap()).unwrap();
        assert_eq!(
            current["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "user-custom-command"
        );
        assert_eq!(current["hooks"]["Stop"], unrelated);
        assert_eq!(current["untouched"], true);
    }

    #[test]
    fn names_and_wrong_application_roots_never_establish_ownership() {
        let (directory, service) = fixture();
        let root = &directory.path().canonicalize().unwrap();
        let before = fs::read(&service.config_path).unwrap();
        let mut forged: Value = serde_json::from_slice(&before).unwrap();
        forged["hooks"]["SessionStart"][0]["matcher"] = json!("user-matcher");
        let mut after = forged.clone();
        after["hooks"]["SessionStart"] = json!([]);
        assert!(prepare_catalog_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &serde_json::to_vec(&forged).unwrap(),
            &serde_json::to_vec(&after).unwrap(),
        )
        .unwrap()
        .is_none());
        fs::create_dir(root.join("app-config")).unwrap();
        assert!(prepare_catalog_change(
            &root.join("app-config"),
            AgentCliKind::Codex,
            &service.config_path,
            &before,
            &serde_json::to_vec(&after).unwrap(),
        )
        .unwrap()
        .is_none());
        assert!(service.inspect().ownership.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn native_policy_on_exact_owned_rule_transfers_ownership_without_changing_hook_bytes() {
        let (directory, service) = fixture();
        let root = &directory.path().canonicalize().unwrap();
        let before = fs::read(&service.config_path).unwrap();
        let document: Value = serde_json::from_slice(&before).unwrap();
        let group = &document["hooks"]["SessionStart"][0];
        // SetEnabled lives in Codex config.toml. The ownership decision instead
        // uses the guarded definition source and exact rule affected by policy.
        let change = prepare_catalog_policy_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &before,
            "SessionStart",
            group,
        )
        .unwrap()
        .unwrap();
        assert!(change.note.contains("会话状态接入"));
        assert!(change.note.contains("专用控制器不会自动恢复"));
        atomic::replace(&change.file, &change.replacement, || Ok(())).unwrap();
        assert_eq!(fs::read(&service.config_path).unwrap(), before);
        let inspection = service.inspect();
        assert!(!inspection.enabled);
        assert!(inspection.ownership.is_none());
        assert!(service.plan(AgentHookMutation::Install).conflict);
    }

    #[test]
    fn unrelated_ambiguous_and_stale_policy_rules_never_transfer_ownership() {
        let (directory, service) = fixture();
        let root = &directory.path().canonicalize().unwrap();
        let before = fs::read(&service.config_path).unwrap();
        let original_manifest = fs::read(&service.manifest_path).unwrap();
        let mut document: Value = serde_json::from_slice(&before).unwrap();
        let owned = document["hooks"]["SessionStart"][0].clone();
        let mut user_group = owned.clone();
        user_group["matcher"] = json!("user-specific-matcher");
        document["hooks"]["SessionStart"]
            .as_array_mut()
            .unwrap()
            .push(user_group.clone());
        for (event, group) in [("SessionStart", &user_group), ("Stop", &owned)] {
            assert!(prepare_catalog_policy_change(
                &root.join("app-data"),
                AgentCliKind::Codex,
                &service.config_path,
                &serde_json::to_vec(&document).unwrap(),
                event,
                group,
            )
            .unwrap()
            .is_none());
        }
        document["hooks"]["SessionStart"]
            .as_array_mut()
            .unwrap()
            .push(owned.clone());
        assert!(prepare_catalog_policy_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &serde_json::to_vec(&document).unwrap(),
            "SessionStart",
            &owned,
        )
        .unwrap()
        .is_none());
        document["hooks"]["SessionStart"] = json!([user_group]);
        assert!(prepare_catalog_policy_change(
            &root.join("app-data"),
            AgentCliKind::Codex,
            &service.config_path,
            &serde_json::to_vec(&document).unwrap(),
            "SessionStart",
            &owned,
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read(&service.manifest_path).unwrap(), original_manifest);
        assert!(service.inspect().ownership.is_some());
    }

    fn gemini_jsonc_fixture() -> (tempfile::TempDir, generic::GenericHookService, Vec<u8>) {
        let directory = tempfile::tempdir().unwrap();
        let root = &directory.path().canonicalize().unwrap();
        fs::create_dir(root.join(".gemini")).unwrap();
        fs::create_dir(root.join("app-data")).unwrap();
        fs::write(root.join("app-data/balancehub"), b"helper").unwrap();
        fs::write(root.join(".gemini/settings.json"), b"{}").unwrap();
        let service = generic::GenericHookService::new(
            GenericAgent::Gemini,
            root.join(".gemini/settings.json"),
            root.join("app-data/agent-hooks/gemini/ownership.json"),
            root.join("app-data/balancehub"),
            root.join("app-data"),
        );
        service
            .apply(service.plan(AgentHookMutation::Install))
            .unwrap();
        let text = fs::read_to_string(&service.config_path).unwrap();
        let before = format!("// User's configuration comment\n{}\n", text.trim_end());
        fs::write(&service.config_path, &before).unwrap();
        (directory, service, before.into_bytes())
    }

    #[test]
    fn gemini_jsonc_owned_edit_still_transfers_verified_ownership() {
        let (directory, service, before) = gemini_jsonc_fixture();
        let mut after = parse_jsonc_document(std::str::from_utf8(&before).unwrap()).unwrap();
        after["hooks"]["SessionStart"][0]["hooks"][0]["command"] = json!("user-custom-hook");
        let after = format!(
            "// User's configuration comment\n{}",
            serde_json::to_string_pretty(&after).unwrap()
        );
        let change = prepare_catalog_change(
            &directory.path().canonicalize().unwrap().join("app-data"),
            AgentCliKind::Gemini,
            &service.config_path,
            &before,
            after.as_bytes(),
        )
        .unwrap()
        .unwrap();
        let stored: ManifestFile = serde_json::from_slice(&change.replacement).unwrap();
        assert!(stored.catalog_detached);
        assert!(!stored.ownership.enabled);
        assert!(change.note.contains("会话状态接入"));
        assert_eq!(fs::read(&service.config_path).unwrap(), before);
    }

    #[test]
    fn gemini_jsonc_user_edit_preserves_independent_session_ownership() {
        let (directory, service, before) = gemini_jsonc_fixture();
        let original_manifest = fs::read(&service.manifest_path).unwrap();
        let mut after = parse_jsonc_document(std::str::from_utf8(&before).unwrap()).unwrap();
        after["hooks"]["BeforeTool"] =
            json!([{ "hooks": [{"type":"command","name":"user-rule","command":"user-command"}] }]);
        let after = format!(
            "/* Preserve comments */\n{}",
            serde_json::to_string_pretty(&after).unwrap()
        );
        assert!(prepare_catalog_change(
            &directory.path().canonicalize().unwrap().join("app-data"),
            AgentCliKind::Gemini,
            &service.config_path,
            &before,
            after.as_bytes(),
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read(&service.manifest_path).unwrap(), original_manifest);
    }

    #[test]
    fn gemini_named_policy_uses_full_owned_group_not_its_name() {
        let (directory, service, before) = gemini_jsonc_fixture();
        let document = parse_jsonc_document(std::str::from_utf8(&before).unwrap()).unwrap();
        let owned = &document["hooks"]["SessionStart"][0];
        let change = prepare_catalog_policy_change(
            &directory.path().canonicalize().unwrap().join("app-data"),
            AgentCliKind::Gemini,
            &service.config_path,
            &before,
            "SessionStart",
            owned,
        )
        .unwrap()
        .unwrap();
        let manifest: ManifestFile = serde_json::from_slice(&change.replacement).unwrap();
        assert!(manifest.catalog_detached);
        let mut unowned = owned.clone();
        unowned["hooks"][0]["command"] = json!("user-command-with-same-name");
        assert!(prepare_catalog_policy_change(
            &directory.path().canonicalize().unwrap().join("app-data"),
            AgentCliKind::Gemini,
            &service.config_path,
            &before,
            "SessionStart",
            &unowned,
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read(&service.config_path).unwrap(), before);
    }
}
