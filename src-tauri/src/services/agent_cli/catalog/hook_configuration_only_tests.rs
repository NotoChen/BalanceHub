//! Ordinary user Hooks remain manageable when no Agent executable is discovered.
//! Native certification is separate from these isolated file-operation regressions.
use super::{
    hook_tests::{
        adopt, assert_verified, binding_for, plan, primary_target, run, update_command, write_json,
    },
    tests::{fixture, FixtureInspector},
    *,
};
use crate::services::agent_cli::{
    self,
    environment::config_document::{self, ConfigDocumentFormat},
};
use serde_json::{json, Value};
use std::{fs, path::Path};

const CODEX_CONFIG: &str = "[features]\nhooks = true # retain native feature\n";
const GROK_CONFIG: &str = "[auth]\npreferred_method = \"api_key\" # retain auth\n";
const GROK_POLICY: &str = "# retain unrelated policy\nglobal/unrelated:stop[0].hooks[0]\n";

struct ExpectedFiles {
    document: Value,
    codex_policy: Value,
    grok_policy: String,
}

fn assert_native_files(
    inspector: &FixtureInspector,
    kind: AgentCliKind,
    path: &Path,
    expected: &ExpectedFiles,
    selected: Option<(&str, AgentAssetDeclaredState)>,
    neighbor: &str,
    execution_markers: &[PathBuf],
) {
    let actual: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        actual, expected.document,
        "complete native document: {kind:?}"
    );
    match kind {
        AgentCliKind::Codex => {
            let bytes = fs::read(inspector.home.join(".codex/config.toml")).unwrap();
            assert_eq!(
                config_document::parse(&bytes, ConfigDocumentFormat::Toml),
                Some(expected.codex_policy.clone())
            );
            assert!(std::str::from_utf8(&bytes)
                .unwrap()
                .contains("hooks = true # retain native feature"));
        }
        AgentCliKind::Grok => {
            assert_eq!(
                fs::read(inspector.home.join(".grok/config.toml")).unwrap(),
                GROK_CONFIG.as_bytes()
            );
            assert_eq!(
                fs::read_to_string(inspector.home.join(".grok/disabled-hooks")).unwrap(),
                expected.grok_policy
            );
        }
        AgentCliKind::ClaudeCode | AgentCliKind::Gemini => (),
    }
    for marker in execution_markers {
        assert!(
            !marker.exists(),
            "configuration CRUD must never execute a Hook"
        );
    }
    let snapshot = inspector.inspect().unwrap();
    assert!(snapshot.inventory.installations.is_empty());
    assert!(snapshot
        .inventory
        .contexts
        .iter()
        .all(|context| context.compatible_installation_ids.is_empty()));
    let source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == path)
        .unwrap();
    let adapter = agent_cli::definition(kind)
        .environment
        .hook_adapter()
        .unwrap();
    let rules = (adapter.inspect_source)(&snapshot, &source.id).unwrap();
    assert_eq!(rules.len(), 1 + usize::from(selected.is_some()));
    let neighbor_rule = rules
        .iter()
        .find(|rule| rule.definition.group["hooks"][0]["command"] == neighbor)
        .unwrap();
    assert_eq!(neighbor_rule.enabled, AgentAssetDeclaredState::Enabled);
    if let Some((command, state)) = selected {
        let rule = rules
            .iter()
            .find(|rule| rule.definition.group["hooks"][0]["command"] == command)
            .unwrap();
        assert_eq!(rule.enabled, state);
    }
    let assets = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.agent_kind == kind
                && asset.category == AgentAssetCategory::Hook
                && asset.inspection_source_id == source.id
        })
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), rules.len());
    assert!(assets.iter().all(|asset| matches!(
        asset.details,
        AgentAssetDetails::Hook {
            managed: false,
            rule_count: Some(1),
            ..
        }
    )));
}

fn execute(
    service: &CatalogService,
    inspector: &Arc<FixtureInspector>,
    saved: &AgentCatalogDefinition,
    action: AgentCatalogAction,
    target_id: String,
    watched: &[PathBuf],
) {
    let before = watched
        .iter()
        .map(|path| fs::read(path).ok())
        .collect::<Vec<_>>();
    let confirmation = plan(
        service,
        inspector.clone(),
        &saved.asset_id,
        action,
        vec![target_id],
    );
    for (path, bytes) in watched.iter().zip(before) {
        assert_eq!(
            fs::read(path).ok(),
            bytes,
            "planning must preserve {}",
            path.display()
        );
    }
    assert_verified(&run(service, confirmation));
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn assert_production_configuration_only_qualification(
    inspector: &FixtureInspector,
    kind: AgentCliKind,
) {
    let snapshot = inspector.inspect().unwrap();
    assert!(snapshot.inventory.installations.is_empty());
    let context = snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.agent_kind == kind)
        .unwrap();
    assert!(context.compatible_installation_ids.is_empty());
    let adapter = agent_cli::definition(kind)
        .environment
        .hook_adapter()
        .unwrap();
    // Configuration uses the native schema even without an executable.
    adapter
        .qualify(context, native::hooks::HookNativeCapability::Configuration)
        .unwrap();
    if adapter.switch_mode == native::hooks::HookNativeSwitchMode::Native {
        adapter
            .qualify(context, native::hooks::HookNativeCapability::NativeSwitch)
            .unwrap();
    }
}

fn file_only_lifecycle(kind: AgentCliKind, adopt_existing: bool) {
    let (temporary, inspector, service) = fixture();

    assert!(!inspector.discover_installations);
    let fixture: Value = serde_json::from_str(include_str!("native/hook-fixture.json")).unwrap();
    let (directory, event_key, filename) = match kind {
        AgentCliKind::ClaudeCode => (".claude", "claudeEvent", "settings.json"),
        AgentCliKind::Codex => (".codex", "codexEvent", "hooks.json"),
        AgentCliKind::Gemini => (".gemini", "geminiEvent", "settings.json"),
        AgentCliKind::Grok => (".grok", "grokEvent", "hooks/hooks.json"),
    };
    let event = fixture[event_key].as_str().unwrap();
    let matcher = if kind == AgentCliKind::Codex {
        ".*"
    } else {
        "*"
    };
    let path = inspector.home.join(directory).join(filename);
    let markers = ["executed-original", "executed-update", "executed-neighbor"]
        .map(|name| temporary.path().join(name));
    let commands = markers
        .each_ref()
        .map(|marker| format!("/usr/bin/touch '{}'", marker.display()));
    let [command, updated, neighbor] = &commands;
    let original_group = json!({"matcher":matcher,"hooks":[{"type":"command","command":command}]});
    let mut expected = ExpectedFiles {
        document: json!({"description":"ordinary user Hook fixture","hooks":{(event):[
            {"matcher":matcher,"hooks":[{"type":"command","command":neighbor}]}
        ]}}),
        codex_policy: json!({"features":{"hooks":true}}),
        grok_policy: GROK_POLICY.to_owned(),
    };
    if kind == AgentCliKind::Gemini {
        expected.document["hooksConfig"] =
            json!({"enabled":true,"disabled":["unrelated-user-hook"]});
    }
    if adopt_existing {
        expected.document["hooks"][event]
            .as_array_mut()
            .unwrap()
            .push(original_group.clone());
    }
    write_json(&path, &expected.document);
    if kind == AgentCliKind::Codex {
        fs::write(inspector.home.join(".codex/config.toml"), CODEX_CONFIG).unwrap();
    }
    if kind == AgentCliKind::Grok {
        fs::write(inspector.home.join(".grok/config.toml"), GROK_CONFIG).unwrap();
        fs::write(inspector.home.join(".grok/disabled-hooks"), GROK_POLICY).unwrap();
    }
    let watched = [
        path.clone(),
        inspector.home.join(".codex/config.toml"),
        inspector.home.join(".grok/disabled-hooks"),
    ];
    native::hook_evidence::with_isolated_fixture(temporary.path(), || {
        assert_native_files(
            &inspector,
            kind,
            &path,
            &expected,
            adopt_existing.then_some((command.as_str(), AgentAssetDeclaredState::Enabled)),
            neighbor,
            &markers,
        );
        let before_save = fs::read(&path).unwrap();
        let saved = if adopt_existing {
            let (_, binding) = binding_for(&service, inspector.as_ref(), kind, command);
            adopt(&service, inspector.clone(), &binding.id)
        } else {
            service
                .save(AgentCatalogSaveRequest {
                    asset_id: None,
                    expected_version: None,
                    name: "ordinary-file-only-hook".to_owned(),
                    category: AgentAssetCategory::Hook,
                    mcp: None,
                    skill_markdown: None,
                    hook: Some(AgentCatalogHookInput {
                        variants: vec![AgentCatalogHookVariantInput {
                            agent_kind: kind,
                            event: event.to_owned(),
                            group_json: original_group.to_string(),
                        }],
                    }),
                })
                .unwrap()
        };
        assert_eq!(
            fs::read(&path).unwrap(),
            before_save,
            "adoption/save must not write native configuration"
        );
        assert_eq!(saved.version, 1);
        assert_eq!(saved.hook.as_ref().unwrap().variants.len(), 1);
        if !adopt_existing {
            execute(
                &service,
                &inspector,
                &saved,
                AgentCatalogAction::ApplyDefinition,
                primary_target(&service, inspector.as_ref(), kind),
                &watched,
            );
            expected.document["hooks"][event]
                .as_array_mut()
                .unwrap()
                .push(original_group.clone());
        }
        assert_native_files(
            &inspector,
            kind,
            &path,
            &expected,
            Some((command, AgentAssetDeclaredState::Enabled)),
            neighbor,
            &markers,
        );
        let before_save = fs::read(&path).unwrap();
        let saved = update_command(&service, &saved, kind, updated);
        assert_eq!(fs::read(&path).unwrap(), before_save);
        execute(
            &service,
            &inspector,
            &saved,
            AgentCatalogAction::ApplyDefinition,
            primary_target(&service, inspector.as_ref(), kind),
            &watched,
        );
        expected.document["hooks"][event][1]["hooks"][0]["command"] = json!(updated);
        assert_native_files(
            &inspector,
            kind,
            &path,
            &expected,
            Some((updated, AgentAssetDeclaredState::Enabled)),
            neighbor,
            &markers,
        );
        let (item, binding) = binding_for(&service, inspector.as_ref(), kind, updated);
        assert_eq!(item.id, saved.asset_id);
        assert_eq!(binding.applied_version, Some(2));
        assert_eq!(binding.drift, AgentCatalogDrift::InSync);
        execute(
            &service,
            &inspector,
            &saved,
            AgentCatalogAction::Disable,
            binding.id,
            &watched,
        );
        let codex_key = format!("{}:pre_tool_use:1:0", path.display());
        match kind {
            AgentCliKind::ClaudeCode => expected.document["hooks"][event][1]["hooks"] = json!([]),
            AgentCliKind::Codex => {
                expected.codex_policy["hooks"] =
                    json!({"state":{(codex_key.clone()):{"enabled":false}}})
            }
            AgentCliKind::Gemini => {
                expected.document["hooksConfig"]["disabled"] =
                    json!(["unrelated-user-hook", updated])
            }
            AgentCliKind::Grok => expected
                .grok_policy
                .push_str("global/hooks:pre_tool_use[1].hooks[0]\n"),
        }
        let suspended = kind == AgentCliKind::ClaudeCode;
        assert_native_files(
            &inspector,
            kind,
            &path,
            &expected,
            (!suspended).then_some((updated.as_str(), AgentAssetDeclaredState::Disabled)),
            neighbor,
            &markers,
        );
        let enable_id = if suspended {
            let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
            let item = catalog
                .assets
                .iter()
                .find(|item| item.id == saved.asset_id)
                .unwrap();
            assert!(item.bindings.is_empty());
            assert_eq!(item.unresolved_targets.len(), 1);
            assert_eq!(
                item.unresolved_targets[0].state,
                AgentCatalogUnresolvedState::Suspended
            );
            item.unresolved_targets[0].target_id.clone()
        } else {
            binding_for(&service, inspector.as_ref(), kind, updated)
                .1
                .id
        };
        execute(
            &service,
            &inspector,
            &saved,
            AgentCatalogAction::Enable,
            enable_id,
            &watched,
        );
        match kind {
            AgentCliKind::ClaudeCode => {
                expected.document["hooks"][event][1]["hooks"] =
                    json!([{"type":"command","command":updated}])
            }
            AgentCliKind::Codex => {
                expected.codex_policy["hooks"]["state"][&codex_key]["enabled"] = json!(true)
            }
            AgentCliKind::Gemini => {
                expected.document["hooksConfig"]["disabled"] = json!(["unrelated-user-hook"])
            }
            AgentCliKind::Grok => expected.grok_policy = GROK_POLICY.to_owned(),
        }
        assert_native_files(
            &inspector,
            kind,
            &path,
            &expected,
            Some((updated, AgentAssetDeclaredState::Enabled)),
            neighbor,
            &markers,
        );
        let (_, binding) = binding_for(&service, inspector.as_ref(), kind, updated);
        execute(
            &service,
            &inspector,
            &saved,
            AgentCatalogAction::RemoveBinding,
            binding.id,
            &watched,
        );
        expected.document["hooks"][event][1]["hooks"] = json!([]);
        if kind == AgentCliKind::Codex {
            expected.codex_policy["hooks"]["state"] = json!({});
        }
        assert_native_files(&inspector, kind, &path, &expected, None, neighbor, &markers);
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let item = catalog
            .assets
            .iter()
            .find(|item| item.id == saved.asset_id)
            .unwrap();
        assert!(item.bindings.is_empty() && item.unresolved_targets.is_empty());
        assert_eq!(service.definition(&saved.asset_id).unwrap().version, 2);
    })
    .unwrap();
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert_production_configuration_only_qualification(&inspector, kind);
}

#[test]
fn claude_file_only_hook_adoption_and_creation_complete_native_lifecycle() {
    for existing in [true, false] {
        file_only_lifecycle(AgentCliKind::ClaudeCode, existing);
    }
}

#[test]
fn codex_file_only_hook_adoption_and_creation_complete_native_lifecycle() {
    for existing in [true, false] {
        file_only_lifecycle(AgentCliKind::Codex, existing);
    }
}

#[test]
fn gemini_file_only_hook_adoption_and_creation_complete_native_lifecycle() {
    for existing in [true, false] {
        file_only_lifecycle(AgentCliKind::Gemini, existing);
    }
}

#[test]
fn grok_file_only_hook_adoption_and_creation_complete_native_lifecycle() {
    for existing in [true, false] {
        file_only_lifecycle(AgentCliKind::Grok, existing);
    }
}
