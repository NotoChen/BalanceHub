//! Real Grok discovery, parser, independent assessment, projection and native
//! reader fixtures. Every filesystem path belongs to the parent temp fixture.
use super::*;
use crate::models::{AgentAssetNativeRef, AgentAssetPolicyReference};
use crate::services::agent_cli::{
    catalog::native::hooks::{HookNativeCapability, HookNativeEdit, NativeHookAdapter},
    contracts::{
        AgentAssetAssessmentRequest, AgentAssetAssessmentResult, AgentAssetAssessmentSubject,
        AgentAssetAssessmentTarget, AgentAssetNativePayload,
    },
    grok::GrokHookPayload,
};
use serde_json::Value;

fn adapter() -> &'static NativeHookAdapter {
    definition(AgentCliKind::Grok)
        .environment()
        .hook_adapter()
        .unwrap()
}

fn hook_document() -> Value {
    json!({
        "retainedRoot": {"private": "fixture-private-root"},
        "hooks": {"SessionStart": [{
            "matcher": "*", "retainedGroup": "fixture-private-group",
            "hooks": [
                {"type":"command", "command":"${GROK_PLUGIN_ROOT}/fixture-private-command", "timeout": 5, "env":{"FIXTURE_TOKEN":"fixture-private-secret"}},
                {"type":"http", "url":"https://example.invalid/fixture-private-url", "timeout": 2}
            ]
        }]}
    })
}

fn one_hook() -> Value {
    json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-private-command"}]}]}})
}

fn write_json(fixture: &Fixture, path: &Path, value: &Value) {
    fixture.write(path, serde_json::to_vec(value).unwrap());
}

fn count(snapshot: &MutationInventory) -> Option<u32> {
    snapshot
        .inventory
        .hook_rule_counts
        .iter()
        .find(|count| count.agent_kind == AgentCliKind::Grok)
        .unwrap()
        .rule_count
}

#[test]
fn grok_plugin_hook_entrypoints_preserve_exact_native_ids_parent_and_privacy() {
    for (mode, layout, stem) in [
        ("default", "plugin.json", "hooks"),
        ("default", ".grok-plugin/plugin.json", "hooks"),
        ("default", ".claude-plugin/plugin.json", "hooks"),
        ("custom", "plugin.json", "native-events"),
        ("inline", ".grok-plugin/plugin.json", "plugin"),
        ("convention", "", "hooks"),
    ] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "pack");
        let document = hook_document();
        let source_path = match mode {
            "inline" => {
                write_json(
                    &fixture,
                    &package.join(layout),
                    &json!({"name":"pack", "hooks": document}),
                );
                package.join(layout)
            }
            "custom" => {
                write_json(
                    &fixture,
                    &package.join(layout),
                    &json!({"name":"pack", "hooks":"private/native-events.json"}),
                );
                package.join("private/native-events.json")
            }
            _ => {
                if mode != "convention" {
                    write_json(&fixture, &package.join(layout), &json!({"name":"pack"}));
                }
                package.join("hooks/hooks.json")
            }
        };
        if mode != "inline" {
            write_json(&fixture, &source_path, &document);
        }
        if matches!(mode, "custom" | "inline") {
            write_json(&fixture, &package.join("hooks/hooks.json"), &one_hook());
        }
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, 3);
        assert_eq!(count(&snapshot), Some(2), "{mode} {layout}");
        let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
        let mut hooks = rows(&snapshot, AgentAssetCategory::Hook);
        hooks.sort_by(|left, right| left.native_id.cmp(&right.native_id));
        assert_eq!(hooks.len(), 2);
        for (index, hook) in hooks.iter().enumerate() {
            assert_eq!(
                hook.native_id,
                format!("plugin/pack/{stem}:session_start[0].hooks[{index}]")
            );
            assert_eq!(
                hook.relationships.provided_by.as_ref(),
                Some(&parent.stable_id)
            );
            assert_eq!(
                hook.relationships.action_owner,
                hook.relationships.provided_by
            );
            assert_eq!(hook.effective_state, AgentAssetState::Unknown);
            assert!(!hook.writable);
            assert_eq!(hook.path.as_deref(), source_path.to_str());
            let native = (adapter().read_hook)(&snapshot, hook).unwrap();
            assert_eq!(native.definition.event, "SessionStart");
            assert_eq!(
                native.definition.group["retainedGroup"],
                "fixture-private-group"
            );
            assert_eq!(
                native.anchor.original_group["hooks"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                native.definition.group["hooks"].as_array().unwrap().len(),
                1
            );
            assert_eq!(native.enabled, AgentAssetDeclaredState::Enabled);
            assert!(!(adapter().hook_targets)(
                &snapshot.inventory,
                &snapshot.inventory.contexts[0]
            )
            .iter()
            .any(|target| target.source_id == native.source_id));
        }
        if matches!(mode, "custom" | "inline") {
            assert!(!reads.contains(&package.join("hooks/hooks.json")));
        }
        let serialized = serde_json::to_string(&snapshot.inventory).unwrap();
        assert!(!serialized.contains("fixture-private-"));
        assert!(!serialized.contains("${GROK_PLUGIN_ROOT}"));
        let catalog = CatalogService::new(
            fixture.root.join("library"),
            Arc::new(MutationService::default()),
        )
        .catalog(&snapshot)
        .unwrap();
        let bindings = catalog
            .assets
            .iter()
            .flat_map(|asset| &asset.bindings)
            .filter(|binding| binding.native.category == AgentAssetCategory::Hook)
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().all(|binding| !binding.can_adopt));
    }
}

#[test]
fn grok_plugin_hook_and_mcp_colocation_reuse_one_verified_source() {
    for inline in [false, true] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "pack");
        let servers = json!({"fixture-server":{"command":"fixture-private-mcp"}});
        let source_path = if inline {
            fixture.manifest(
                &package,
                json!({"name":"pack", "hooks":hook_document(), "mcpServers":servers}),
            );
            package.join("plugin.json")
        } else {
            fixture.manifest(&package, json!({"name":"pack", "hooks":".mcp.json"}));
            let mut document = hook_document();
            document["mcpServers"] = servers;
            write_json(&fixture, &package.join(".mcp.json"), &document);
            package.join(".mcp.json")
        };
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, 4);
        assert_eq!(count(&snapshot), Some(2));
        let mcp = selected(&snapshot, AgentAssetCategory::Mcp, "fixture-server");
        let hooks = rows(&snapshot, AgentAssetCategory::Hook);
        assert_eq!(hooks.len(), 2);
        assert!(hooks
            .iter()
            .all(|hook| hook.inspection_source_id == mcp.inspection_source_id));
        assert_eq!(reads.iter().filter(|path| **path == source_path).count(), 1);
        assert_eq!(
            (adapter().inspect_source)(&snapshot, &mcp.inspection_source_id)
                .unwrap()
                .len(),
            2
        );
    }
}

#[test]
fn grok_plugin_hook_policy_switch_keeps_parent_unknown_and_never_writes_package() {
    let fixture = Fixture::new();
    fixture.record_workspace_trust(false);
    let package = fixture.package(true, "pack");
    let manifest_path = package.join(".grok-plugin/plugin.json");
    write_json(&fixture, &manifest_path, &json!({"name":"pack"}));
    let hook_path = package.join("hooks/hooks.json");
    write_json(&fixture, &hook_path, &one_hook());
    let original_manifest = fs::read(&manifest_path).unwrap();
    let original_hook = fs::read(&hook_path).unwrap();
    let name = "plugin/pack/hooks:session_start[0].hooks[0]";
    let policy_path = fixture.home.join(".grok/disabled-hooks");
    fixture.write(
        &policy_path,
        b"# retain policy comment\nglobal/neighbor:session_start[0].hooks[0]\n",
    );
    for enabled in [false, true] {
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, 2);
        assert_eq!(
            snapshot.inventory.contexts[0].trust_context,
            AgentTrustState::Unknown
        );
        assert!(reads.contains(&fixture.home.join(".grok/trusted_folders.toml")));
        let asset = selected(&snapshot, AgentAssetCategory::Hook, name);
        assert_eq!(
            asset.effective_state,
            if enabled {
                AgentAssetState::Disabled
            } else {
                AgentAssetState::Unknown
            }
        );
        assert_eq!(asset.trust_state, AgentTrustState::Unknown);
        let native = (adapter().read_hook)(&snapshot, asset).unwrap();
        assert_eq!(
            native.enabled,
            if enabled {
                AgentAssetDeclaredState::Disabled
            } else {
                AgentAssetDeclaredState::Enabled
            }
        );
        let target = (adapter().hook_targets)(&snapshot.inventory, &snapshot.inventory.contexts[0])
            .into_iter()
            .find(|target| target.role == "config")
            .unwrap();
        let prepared = (adapter().prepare_hook_edits)(
            &snapshot,
            &target,
            &[HookNativeEdit::SetEnabled {
                original: native,
                enabled,
            }],
        )
        .unwrap();
        assert_eq!(prepared.writes.len(), 1);
        let policy = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| Path::new(&source.path) == policy_path)
            .unwrap();
        assert_eq!(prepared.writes[0].source_id, policy.id);
        assert!(prepared
            .required_capabilities
            .contains(&HookNativeCapability::Configuration));
        assert!(prepared
            .required_capabilities
            .contains(&HookNativeCapability::NativeSwitch));
        let dependencies = prepared
            .read_source_ids
            .iter()
            .map(|id| {
                PathBuf::from(
                    &snapshot
                        .inventory
                        .sources
                        .iter()
                        .find(|source| source.id == *id)
                        .unwrap()
                        .path,
                )
            })
            .collect::<BTreeSet<_>>();
        for path in [
            fixture.home.join(".grok/plugins"),
            fixture.workspace.join(".grok/plugins"),
            package.clone(),
            package.join("plugin.json"),
            manifest_path.clone(),
            hook_path.clone(),
            policy_path.clone(),
        ] {
            assert!(dependencies.contains(&path), "missing dependency: {path:?}");
        }
        let final_policy = String::from_utf8(prepared.writes[0].bytes.clone()).unwrap();
        assert!(final_policy
            .contains("# retain policy comment\nglobal/neighbor:session_start[0].hooks[0]\n"));
        assert_eq!(final_policy.lines().any(|line| line == name), !enabled);
        fixture.write(&policy_path, &prepared.writes[0].bytes);
    }
    let (snapshot, _) = fixture.scan();
    let asset = selected(&snapshot, AgentAssetCategory::Hook, name);
    assert_eq!(asset.declared_state, AgentAssetState::Enabled);
    assert_eq!(asset.effective_state, AgentAssetState::Unknown);
    assert_eq!(fs::read(manifest_path).unwrap(), original_manifest);
    assert_eq!(fs::read(hook_path).unwrap(), original_hook);
    assert!(adapter()
        .qualify(
            &snapshot.inventory.contexts[0],
            HookNativeCapability::NativeSwitch
        )
        .is_ok());
}

#[test]
fn grok_plugin_hook_loser_inline_definitions_are_suppressed_and_workspace_config_is_not_loaded() {
    let fixture = Fixture::new();
    let loser = fixture.package(false, "loser");
    let mut loser_hooks = one_hook();
    loser_hooks["hooks"]["Stop"] =
        json!([{"hooks":[{"type":"command","command":"fixture-private-loser"}]}]);
    fixture.manifest(&loser, json!({"name":"pack", "hooks":loser_hooks}));
    let winner = fixture.package(true, "winner");
    fixture.manifest(&winner, json!({"name":"pack", "hooks":one_hook()}));
    fixture.write(
        fixture.workspace.join(".grok/config.toml"),
        b"[[hooks.pre_tool_use]]\nhooks=[{type='command',command='fixture-private-not-loaded'}]\n",
    );
    let (snapshot, _) = fixture.scan();
    assert_admitted(&snapshot, 3);
    let hooks = rows(&snapshot, AgentAssetCategory::Hook);
    assert_eq!(hooks.len(), 1);
    assert_eq!(
        hooks[0].native_id,
        "plugin/pack/plugin:session_start[0].hooks[0]"
    );
    let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
    assert_eq!(
        hooks[0].relationships.provided_by.as_ref(),
        Some(&parent.stable_id)
    );
    let suppressed = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Hook
                && matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::ParentNotSelected
                    }
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(suppressed.len(), 2);
    assert!(suppressed
        .iter()
        .any(|declaration| declaration.native_id.ends_with("stop[0].hooks[0]")));
    let loser_source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == loser.join("plugin.json"))
        .unwrap();
    assert!((adapter().inspect_source)(&snapshot, &loser_source.id).is_err());
    assert!((adapter().read_hook)(&snapshot, hooks[0]).is_ok());
}

#[test]
fn grok_plugin_hook_missing_invalid_and_escaping_sources_keep_healthy_neighbors() {
    for mode in ["missing", "invalid", "aliases", "escape"] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "pack");
        let hook_path = package.join("private/hooks.json");
        fixture.manifest(&package, json!({"name":"pack", "hooks": if mode == "escape" { "../outside.json" } else { "private/hooks.json" }}));
        match mode {
            "invalid" => fixture.write(&hook_path, b"{"),
            "aliases" => write_json(
                &fixture,
                &hook_path,
                &json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"a"}]}],"session_start":[{"hooks":[{"type":"command","command":"b"}]}]}}),
            ),
            "escape" => write_json(
                &fixture,
                &fixture.home.join(".grok/plugins/outside.json"),
                &one_hook(),
            ),
            _ => (),
        }
        write_json(
            &fixture,
            &fixture.home.join(".grok/hooks/healthy.json"),
            &one_hook(),
        );
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, 2);
        assert_eq!(rows(&snapshot, AgentAssetCategory::Hook).len(), 1);
        selected(
            &snapshot,
            AgentAssetCategory::Hook,
            "global/healthy:session_start[0].hooks[0]",
        );
        assert_eq!(
            count(&snapshot),
            if mode == "missing" { Some(1) } else { None }
        );
        assert!(!reads.contains(&fixture.home.join(".grok/plugins/outside.json")));
    }
}

#[cfg(unix)]
#[test]
fn grok_plugin_hook_symlink_is_not_followed_or_counted_as_zero() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(&package, json!({"name":"pack"}));
    let outside = fixture.root.join("outside/hooks.json");
    write_json(&fixture, &outside, &one_hook());
    fs::create_dir_all(package.join("hooks")).unwrap();
    symlink(&outside, package.join("hooks/hooks.json")).unwrap();
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 1);
    assert_eq!(count(&snapshot), None);
    assert!(!reads.contains(&outside));
    assert!(rows(&snapshot, AgentAssetCategory::Hook).is_empty());
}

#[test]
fn grok_plugin_hook_reader_rejects_stale_selection_policy_and_forged_parent() {
    for attack in [
        "hook",
        "manifest",
        "higher-manifest",
        "competitor",
        "policy",
        "parent",
        "origin",
    ] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "pack");
        write_json(
            &fixture,
            &package.join(".grok-plugin/plugin.json"),
            &json!({"name":"pack"}),
        );
        let hook_path = package.join("hooks/hooks.json");
        write_json(&fixture, &hook_path, &one_hook());
        let other = fixture.package(false, "other");
        fixture.manifest(&other, json!({"name":"other"}));
        let policy = fixture.home.join(".grok/disabled-hooks");
        fixture.write(&policy, b"# baseline\n");
        let (mut snapshot, _) = fixture.scan();
        assert_admitted(&snapshot, 3);
        let hook = selected(
            &snapshot,
            AgentAssetCategory::Hook,
            "plugin/pack/hooks:session_start[0].hooks[0]",
        )
        .clone();
        assert!((adapter().read_hook)(&snapshot, &hook).is_ok());
        match attack {
            "hook" => write_json(&fixture, &hook_path, &hook_document()),
            "manifest" => write_json(
                &fixture,
                &package.join(".grok-plugin/plugin.json"),
                &json!({"name":"pack", "hooks":"other.json"}),
            ),
            "higher-manifest" => fixture.manifest(&package, json!({"name":"pack"})),
            "competitor" => fixture.manifest(&other, json!({"name":"pack"})),
            "policy" => fixture.write(&policy, b"plugin/pack/hooks:session_start[0].hooks[0]\n"),
            "parent" => {
                let parent_id = selected(&snapshot, AgentAssetCategory::Plugin, "other")
                    .stable_id
                    .clone();
                let asset = snapshot
                    .inventory
                    .assets
                    .iter_mut()
                    .find(|asset| asset.stable_id == hook.stable_id)
                    .unwrap();
                asset.relationships.provided_by = Some(parent_id.clone());
                asset.relationships.action_owner = Some(parent_id);
            }
            "origin" => {
                snapshot
                    .inventory
                    .sources
                    .iter_mut()
                    .find(|source| source.id == hook.inspection_source_id)
                    .unwrap()
                    .origin = AgentAssetInstallationOrigin::LocalFiles
            }
            _ => unreachable!(),
        }
        assert!((adapter().read_hook)(&snapshot, &hook).is_err(), "{attack}");
    }
}

fn verify_hook_members(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    let captured = super::super::grok_proof::native_resolve(request);
    let target = AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Hook,
        resolution_group_key: "plugin/pack/plugin:session_start[0].hooks[0]".to_owned(),
        exact_native_id: "plugin/pack/plugin:session_start[0].hooks[0]".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let targets = [target.clone()];
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let assessor = definition(AgentCliKind::Grok)
        .environment()
        .state_assessor();
    let baseline = assessor(AgentAssetAssessmentRequest {
        context: request.context,
        targets: &targets,
        declarations: request.declarations,
        sources: &sources,
    });
    assert!(matches!(
        baseline.get(&target),
        Some(AgentAssetAssessmentResult::Assessed { .. })
    ));
    for attack in ["missing", "duplicate", "parent", "probe", "payload"] {
        let mut declarations = request.declarations.to_vec();
        let index = declarations
            .iter()
            .position(|asset| asset.native_id == target.exact_native_id)
            .unwrap();
        match attack {
            "missing" => {
                declarations.remove(index);
            }
            "duplicate" => declarations.push(declarations[index].clone()),
            "parent" => {
                declarations[index].provided_by = Some(AgentAssetNativeRef {
                    category: AgentAssetCategory::Plugin,
                    native_id: "other".to_owned(),
                    qualifier: Some("plugin:other".to_owned()),
                });
                declarations[index].action_owner = declarations[index].provided_by.clone();
            }
            "probe" => declarations.retain(|asset| asset.declaration_key != "package-directory"),
            "payload" => {
                declarations[index].native_payload =
                    AgentAssetNativePayload::GrokHook(GrokHookPayload::Definition {
                        name: target.exact_native_id.clone(),
                        managed: false,
                    })
            }
            _ => unreachable!(),
        }
        let assessment = assessor(AgentAssetAssessmentRequest {
            context: request.context,
            targets: &targets,
            declarations: &declarations,
            sources: &sources,
        });
        assert!(
            matches!(
                assessment.get(&target),
                Some(AgentAssetAssessmentResult::Unsupported(_))
            ),
            "{attack}"
        );
    }
    super::super::grok_proof::emit_captured(captured, output);
}

#[test]
fn grok_plugin_hook_independent_assessment_rejects_missing_duplicate_and_forged_members() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(&package, json!({"name":"pack", "hooks":hook_document()}));
    let (snapshot, _) =
        fixture.scan_with_resolver(AgentAssetLimits::DEFAULT, false, Some(verify_hook_members));
    assert_admitted(&snapshot, 3);
    assert_eq!(count(&snapshot), Some(2));
}

#[test]
fn grok_plugin_hook_invalid_disabled_policy_preserves_known_definition_count() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(&package, json!({"name":"pack", "hooks":one_hook()}));
    fixture.write(fixture.home.join(".grok/disabled-hooks"), [0xff]);
    let (snapshot, _) = fixture.scan();
    assert_admitted(&snapshot, 2);
    assert_eq!(count(&snapshot), Some(1));
    let hook = selected(
        &snapshot,
        AgentAssetCategory::Hook,
        "plugin/pack/plugin:session_start[0].hooks[0]",
    );
    assert_eq!(hook.declared_state, AgentAssetState::Enabled);
    assert_eq!(hook.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        hook.resolution.control_source,
        Some(AgentAssetPolicyReference::Declaration { .. })
    ));
    assert!((adapter().read_hook)(&snapshot, hook).is_err());
}
