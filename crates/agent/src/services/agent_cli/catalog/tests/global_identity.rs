//! Exercise the real inventory and catalog boundary with isolated native data.
use super::super::{
    hook_tests::{assert_verified, plan, run},
    *,
};
use super::{fixture, mcp_request};
use crate::services::agent_cli::environment::config_document::{self, ConfigDocumentFormat};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, os::unix::fs::PermissionsExt, path::Path};

fn skill(root: &Path, name: &str, resource: &[u8], executable: bool) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Complete identity fixture\n---\nRead the bundled resource.\n"),
    )
    .unwrap();
    let script = root.join("scripts/run.sh");
    fs::write(&script, resource).unwrap();
    fs::set_permissions(
        script,
        fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
    )
    .unwrap();
}

fn mcp_pair(home: &Path, codex: Value, gemini: Value) {
    fs::write(
        home.join(".codex/config.toml"),
        config_document::serialize(
            &json!({"mcp_servers":{"shared":codex}}),
            ConfigDocumentFormat::Toml,
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        home.join(".gemini/settings.json"),
        serde_json::to_vec(&json!({"mcpServers":{"shared":gemini}})).unwrap(),
    )
    .unwrap();
}

fn rows(catalog: &AgentAssetCatalog, category: AgentAssetCategory) -> Vec<&AgentCatalogAsset> {
    catalog
        .assets
        .iter()
        .filter(|asset| asset.category == category)
        .collect()
}

fn native_bindings(catalog: &AgentAssetCatalog) -> BTreeMap<String, Value> {
    catalog
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .map(|binding| {
            // The catalog adds file removal capabilities; read capabilities
            // and the embedded native record must retain native semantics.
            for action in &binding.native.actions {
                if matches!(
                    action.action,
                    AgentAssetActionKind::Inspect
                        | AgentAssetActionKind::Preview
                        | AgentAssetActionKind::Open
                        | AgentAssetActionKind::Reveal
                ) {
                    assert!(binding.actions.iter().any(|candidate| serde_json::to_value(
                        candidate
                    )
                    .unwrap()
                        == serde_json::to_value(action).unwrap()));
                }
            }
            (
                binding.id.clone(),
                serde_json::to_value(&binding.native).unwrap(),
            )
        })
        .collect()
}

fn agent_observation(
    asset: &AgentCatalogAsset,
    kind: AgentCliKind,
) -> &AgentCatalogAgentObservation {
    assert_eq!(
        asset.application.observations.len(),
        AgentCliKind::ALL.len()
    );
    assert_eq!(
        asset
            .application
            .observations
            .iter()
            .map(|observation| observation.agent_kind)
            .collect::<BTreeSet<_>>(),
        AgentCliKind::ALL.iter().copied().collect::<BTreeSet<_>>()
    );
    asset
        .application
        .observations
        .iter()
        .find(|observation| observation.agent_kind == kind)
        .unwrap()
}

#[test]
fn unbound_mcp_requires_readable_native_absence_instead_of_an_empty_binding_list() {
    for (claude, expected) in [
        (None, AgentCatalogObservationState::Missing),
        (Some("{}"), AgentCatalogObservationState::Missing),
        (
            Some(r#"{"mcpServers":{}}"#),
            AgentCatalogObservationState::Missing,
        ),
        (Some("{broken"), AgentCatalogObservationState::Unknown),
        (
            Some(r#"{"mcpServers":[]}"#),
            AgentCatalogObservationState::Unknown,
        ),
    ] {
        let (_temporary, inspector, service) = fixture();
        fs::write(
            inspector.home.join(".codex/config.toml"),
            "[mcp_servers.shared]\ncommand = 'fixture-runner'\n",
        )
        .unwrap();
        if let Some(contents) = claude {
            fs::write(inspector.home.join(".claude.json"), contents).unwrap();
        }
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let item = rows(&catalog, AgentAssetCategory::Mcp)[0];
        assert!(item.application.available);
        assert_eq!(item.bindings.len(), 1);
        assert_eq!(
            agent_observation(item, AgentCliKind::Codex).state,
            AgentCatalogObservationState::Observed
        );
        let observation = agent_observation(item, AgentCliKind::ClaudeCode);
        assert_eq!(observation.state, expected, "Claude source: {claude:?}");
        assert_eq!(
            observation.reason.is_some(),
            expected == AgentCatalogObservationState::Unknown
        );
    }
}

#[test]
fn unbound_mcp_stays_unknown_when_a_captured_source_loses_its_anchor_or_changes() {
    for lose_anchor in [true, false] {
        let (_temporary, inspector, service) = fixture();
        fs::write(
            inspector.home.join(".codex/config.toml"),
            "[mcp_servers.shared]\ncommand = 'fixture-runner'\n",
        )
        .unwrap();
        let claude = inspector.home.join(".claude.json");
        fs::write(&claude, "{}").unwrap();
        let mut snapshot = inspector.inspect().unwrap();
        let source = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| Path::new(&source.path) == claude)
            .unwrap();
        assert!(!source.revision.is_missing);
        if lose_anchor {
            assert!(snapshot.source_anchors.remove(&source.id).is_some());
        } else {
            fs::write(
                &claude,
                r#"{"mcpServers":{"shared":{"command":"changed"}}}"#,
            )
            .unwrap();
        }
        let catalog = service.catalog(&snapshot).unwrap();
        let item = rows(&catalog, AgentAssetCategory::Mcp)[0];
        let observation = agent_observation(item, AgentCliKind::ClaudeCode);
        assert_eq!(observation.state, AgentCatalogObservationState::Unknown);
        assert!(observation.reason.is_some());
        assert!(item.application.available);
    }
}

#[test]
fn a_foreign_same_name_is_unknown_while_an_exact_disabled_binding_is_observed() {
    let (_temporary, inspector, service) = fixture();
    fs::write(
        inspector.home.join(".codex/config.toml"),
        "[mcp_servers.shared]\ncommand = 'codex-runner'\nenabled = false\n",
    )
    .unwrap();
    fs::write(
        inspector.home.join(".claude.json"),
        r#"{"mcpServers":{"shared":{"command":"claude-runner"}}}"#,
    )
    .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let items = rows(&catalog, AgentAssetCategory::Mcp);
    assert_eq!(items.len(), 2);
    let codex = items
        .into_iter()
        .find(|item| item.bindings[0].native.agent_kind == AgentCliKind::Codex)
        .unwrap();
    assert!(matches!(
        &codex.bindings[0].native.details,
        AgentAssetDetails::Mcp {
            declared_state: AgentAssetDeclaredState::Disabled,
            ..
        }
    ));
    assert_eq!(
        agent_observation(codex, AgentCliKind::Codex).state,
        AgentCatalogObservationState::Observed
    );
    let claude = agent_observation(codex, AgentCliKind::ClaudeCode);
    assert_eq!(claude.state, AgentCatalogObservationState::Unknown);
    assert!(claude.reason.as_deref().unwrap().contains("同名"));
}

#[test]
fn incomplete_untyped_native_entrypoints_do_not_prove_a_skill_is_unconfigured() {
    for blocked in [true, false] {
        let (_temporary, inspector, service) = fixture();
        skill(
            &inspector.home.join(".codex/skills/shared"),
            "shared",
            b"fixture resource",
            false,
        );
        let bundled = inspector.home.join(".grok/bundled");
        if blocked {
            fs::write(&bundled, "not a native directory").unwrap();
        } else {
            fs::create_dir_all(bundled.join("skills")).unwrap();
        }
        let snapshot = inspector.inspect().unwrap();
        assert!(snapshot
            .inventory
            .sources
            .iter()
            .any(|source| { Path::new(&source.path) == bundled && source.categories.is_empty() }));
        assert!(snapshot
            .inventory
            .diagnostics
            .iter()
            .chain(
                snapshot
                    .inventory
                    .sources
                    .iter()
                    .flat_map(|source| &source.diagnostics)
            )
            .any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::DiscoveryIncomplete {
                    agent_kind: AgentCliKind::Grok,
                    category: AgentAssetCategory::Skill,
                    reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable
                        | AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
                }
            )));
        let catalog = service.catalog(&snapshot).unwrap();
        let item = rows(&catalog, AgentAssetCategory::Skill)[0];
        let grok = agent_observation(item, AgentCliKind::Grok);
        assert_eq!(grok.state, AgentCatalogObservationState::Unknown);
        assert!(grok.reason.is_some());
        assert_eq!(
            agent_observation(item, AgentCliKind::ClaudeCode).state,
            AgentCatalogObservationState::Missing,
            "Grok discovery gaps must not erase independent Claude absence evidence"
        );
    }
}

#[test]
fn independent_complete_skill_aliases_merge_existing_ids_and_survive_drift_order_and_restart() {
    let (temporary, inspector, service) = fixture();
    let codex = inspector.home.join(".codex/skills/shared");
    let claude = inspector.home.join(".claude/skills/shared");
    for root in [&codex, &claude] {
        skill(root, "shared", b"#!/bin/sh\nprintf fixture\n", true);
    }
    let snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.assets.len(), 2);
    let expected = snapshot
        .inventory
        .assets
        .iter()
        .map(|asset| {
            (
                asset.stable_id.clone(),
                serde_json::to_value(asset).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    // Seed separate prior observations. Old private fingerprints alone must not
    // join an absent source, while this scan's complete pair must join them.
    let mut prior_ids = BTreeSet::new();
    for kind in [AgentCliKind::Codex, AgentCliKind::ClaudeCode] {
        let mut partial = inspector.inspect().unwrap();
        partial
            .inventory
            .assets
            .retain(|asset| asset.agent_kind == kind);
        let catalog = service.catalog(&partial).unwrap();
        let single = rows(&catalog, AgentAssetCategory::Skill);
        assert_eq!(single.len(), 1);
        prior_ids.insert(single[0].id.clone());
    }
    assert_eq!(prior_ids.len(), 2);
    let catalog = service.catalog(&snapshot).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Skill);
    assert_eq!(assets.len(), 1);
    let asset = assets[0];
    let id = asset.id.clone();
    assert!(prior_ids.contains(&id));
    assert_eq!(asset.bindings.len(), 2);
    assert_eq!(asset.variants.len(), 1);
    assert!(asset.version.is_none() && asset.application.available);
    assert!(asset.candidate_ids.is_empty());
    assert_eq!(native_bindings(&catalog), expected);
    let selected = asset.application.source_binding_id.clone();
    assert_eq!(
        selected.as_ref(),
        asset.bindings.iter().map(|binding| &binding.id).min()
    );
    service
        .repository
        .transact(|library| {
            assert_eq!(library.entries.len(), 1);
            let entry = &library.entries[&id];
            assert_eq!(entry.aliases.len(), 2);
            assert!(entry.versions.is_empty() && entry.receipts.is_empty());
            Ok(())
        })
        .unwrap();

    let mut reversed = snapshot;
    reversed.inventory.assets.reverse();
    reversed.inventory.declarations.reverse();
    let again = service.catalog(&reversed).unwrap();
    assert_eq!(again.revision, catalog.revision);
    assert_eq!(again.assets[0].id, id);
    assert_eq!(again.assets[0].application.source_binding_id, selected);
    assert_eq!(native_bindings(&again), expected);

    fs::write(codex.join("scripts/run.sh"), b"#!/bin/sh\nprintf changed\n").unwrap();
    let changed = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let changed = rows(&changed, AgentAssetCategory::Skill)[0];
    assert_eq!(changed.id, id);
    assert_eq!(changed.bindings.len(), 2);
    assert_eq!(changed.variants.len(), 2);
    assert!(!changed.application.available);
    assert!(changed.application.source_binding_id.is_none());
    assert!(changed
        .application
        .reason
        .as_deref()
        .unwrap()
        .contains("多个不同定义"));
    drop(service);
    let restarted = CatalogService::new(
        temporary.path().canonicalize().unwrap().join("library"),
        Arc::new(MutationService::default()),
    );
    let mut reversed = inspector.inspect().unwrap();
    reversed.inventory.assets.reverse();
    let catalog = restarted.catalog(&reversed).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Skill);
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].id, id);
    assert_eq!(assets[0].variants.len(), 2);
    assert_eq!(
        assets[0]
            .bindings
            .iter()
            .map(|binding| &binding.id)
            .collect::<BTreeSet<_>>(),
        expected.keys().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        fs::read(claude.join("scripts/run.sh")).unwrap(),
        b"#!/bin/sh\nprintf fixture\n"
    );
}

#[test]
fn equal_manifests_do_not_hide_resource_file_set_or_executable_differences() {
    for difference in ["resource", "mode", "extra"] {
        let (_temporary, inspector, service) = fixture();
        let codex = inspector.home.join(".codex/skills/shared");
        let claude = inspector.home.join(".claude/skills/shared");
        skill(&codex, "shared", b"original resource", true);
        skill(&claude, "shared", b"original resource", true);
        match difference {
            "resource" => fs::write(claude.join("scripts/run.sh"), b"different resource").unwrap(),
            "mode" => fs::set_permissions(
                claude.join("scripts/run.sh"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
            _ => fs::write(claude.join("reference.txt"), b"extra resource").unwrap(),
        }
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let assets = rows(&catalog, AgentAssetCategory::Skill);
        assert_eq!(assets.len(), 2, "{difference}");
        for asset in assets {
            assert_eq!(asset.bindings.len(), 1);
            assert_eq!(asset.candidate_ids.len(), 1);
            assert!(asset.application.available);
            assert_eq!(
                asset.application.source_binding_id.as_deref(),
                Some(asset.bindings[0].id.as_str())
            );
        }
    }
}

#[test]
fn equal_payloads_with_distinct_native_names_remain_separate_assets() {
    let (_temporary, inspector, service) = fixture();
    for directory in ["left", "right"] {
        skill(
            &inspector.home.join(".claude/skills").join(directory),
            "same-manifest",
            b"resource",
            false,
        );
    }
    let snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.assets.len(), 2);
    assert_eq!(
        snapshot
            .inventory
            .assets
            .iter()
            .map(|asset| &asset.native_id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    let catalog = service.catalog(&snapshot).unwrap();
    assert_eq!(rows(&catalog, AgentAssetCategory::Skill).len(), 2);
}

#[test]
fn incomplete_current_packages_do_not_merge_or_reuse_a_previous_complete_fingerprint() {
    let (_temporary, inspector, service) = fixture();
    let codex = inspector.home.join(".codex/skills/shared");
    let claude = inspector.home.join(".claude/skills/shared");
    skill(&codex, "shared", b"resource", false);
    skill(&claude, "shared", b"resource", false);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let id = rows(&catalog, AgentAssetCategory::Skill)[0].id.clone();
    std::os::unix::fs::symlink(codex.join("scripts/run.sh"), claude.join("unreadable-link"))
        .unwrap();
    // This third complete copy must not join a now-incomplete old group using
    // its persisted fingerprint; the existing pair still keeps its identity.
    skill(
        &inspector.home.join(".gemini/skills/shared"),
        "shared",
        b"resource",
        false,
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Skill);
    assert_eq!(assets.len(), 2);
    let old = assets.iter().find(|asset| asset.id == id).unwrap();
    assert_eq!(old.bindings.len(), 2);
    assert!(!old.application.available);
    let incomplete = old
        .bindings
        .iter()
        .find(|binding| binding.native.agent_kind == AgentCliKind::ClaudeCode)
        .unwrap();
    assert!(!incomplete.can_adopt);
    assert!(incomplete.variant_id.is_none());
    assert_eq!(incomplete.drift, AgentCatalogDrift::Unknown);
    let new = assets.iter().find(|asset| asset.id != id).unwrap();
    assert_eq!(new.bindings.len(), 1);
    assert!(new.application.available);
}

#[test]
fn private_json_toml_equivalence_keeps_native_state_and_secrets_out_of_public_ids() {
    let (_temporary, inspector, service) = fixture();
    mcp_pair(
        &inspector.home,
        json!({"command":"runner","args":["--port","3000"],"env":{"TOKEN":"private-equivalence-fixture"},"enabled":false}),
        json!({"command":"runner","args":["--port","3000"],"env":{"TOKEN":"private-equivalence-fixture"}}),
    );
    let snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.assets.len(), 2);
    let expected = snapshot
        .inventory
        .assets
        .iter()
        .map(|asset| {
            (
                asset.stable_id.clone(),
                serde_json::to_value(asset).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let catalog = service.catalog(&snapshot).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Mcp);
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].bindings.len(), 2);
    assert_eq!(assets[0].variants.len(), 1);
    assert!(assets[0].application.available && assets[0].version.is_none());
    assert_eq!(native_bindings(&catalog), expected);
    let public = serde_json::to_string(&catalog).unwrap();
    assert!(!public.contains("private-equivalence-fixture"));
    for asset in &snapshot.inventory.assets {
        let private = projection::observe_payload(&snapshot, asset)
            .unwrap()
            .fingerprint();
        assert!(!public.contains(&private));
    }
    assert!(!public.contains("\"canApply\""));
    assert!(public.contains("\"sourceBindingId\""));
}

#[test]
fn different_private_credentials_never_merge_by_redacted_content() {
    let (_temporary, inspector, service) = fixture();
    let left = json!({"command":"runner","env":{"TOKEN":"private-left"}});
    let mut right = left.clone();
    right["env"]["TOKEN"] = json!("private-right");
    mcp_pair(&inspector.home, left, right);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Mcp);
    assert_eq!(assets.len(), 2);
    assert!(assets
        .iter()
        .all(|asset| asset.bindings.len() == 1 && asset.candidate_ids.len() == 1));
    let public = serde_json::to_string(&catalog).unwrap();
    assert!(!public.contains("private-left") && !public.contains("private-right"));
}

#[test]
fn equivalent_native_content_does_not_merge_independent_managed_versions_or_receipts() {
    let (_temporary, inspector, service) = fixture();
    mcp_pair(
        &inspector.home,
        json!({"command":"left-runner"}),
        json!({"command":"right-runner"}),
    );
    let mut saved_ids = BTreeSet::new();
    for kind in [AgentCliKind::Codex, AgentCliKind::Gemini] {
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let binding = catalog
            .assets
            .iter()
            .flat_map(|asset| &asset.bindings)
            .find(|binding| binding.native.agent_kind == kind)
            .unwrap();
        let saved = service
            .adopt(
                AgentCatalogAdoptRequest {
                    binding_id: binding.id.clone(),
                    expected_revision: catalog.revision,
                    workspace: None,
                },
                inspector.clone(),
            )
            .unwrap();
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let target = catalog
            .targets
            .iter()
            .find(|target| {
                target.agent_kind == kind && target.categories == [AgentAssetCategory::Mcp]
            })
            .unwrap();
        assert_verified(&run(
            &service,
            plan(
                &service,
                inspector.clone(),
                &saved.asset_id,
                AgentCatalogAction::ApplyDefinition,
                vec![target.id.clone()],
            ),
        ));
        saved_ids.insert(saved.asset_id);
    }
    assert_eq!(saved_ids.len(), 2);
    let histories = service
        .repository
        .transact(|library| {
            Ok(saved_ids
                .iter()
                .map(|id| {
                    let entry = &library.entries[id];
                    assert_eq!(entry.versions.len(), 1);
                    assert_eq!(entry.receipts.len(), 1);
                    (
                        id.clone(),
                        serde_json::to_value((&entry.versions, &entry.receipts)).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>())
        })
        .unwrap();
    mcp_pair(
        &inspector.home,
        json!({"command":"right-runner"}),
        json!({"command":"right-runner"}),
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let assets = rows(&catalog, AgentAssetCategory::Mcp);
    assert_eq!(assets.len(), 2);
    assert_eq!(
        assets
            .iter()
            .map(|asset| asset.id.clone())
            .collect::<BTreeSet<_>>(),
        saved_ids
    );
    assert!(assets.iter().all(|asset| asset.version == Some(1)
        && asset.bindings.len() == 1
        && asset.candidate_ids.len() == 1));
    service
        .repository
        .transact(|library| {
            for id in &saved_ids {
                let entry = &library.entries[id];
                assert_eq!(
                    serde_json::to_value((&entry.versions, &entry.receipts)).unwrap(),
                    histories[id]
                );
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn observed_skill_uses_backend_source_then_stored_version_for_the_exact_unbound_target_plan() {
    let (_temporary, inspector, service) = fixture();
    skill(
        &inspector.home.join(".codex/skills/shared"),
        "shared",
        b"#!/bin/sh\nprintf fixture\n",
        true,
    );
    let destination = inspector.home.join(".claude/skills/shared");
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = rows(&catalog, AgentAssetCategory::Skill)[0];
    assert!(item.application.available && item.version.is_none());
    let source = item.application.source_binding_id.clone().unwrap();
    assert_eq!(item.bindings.len(), 1);
    assert_eq!(item.bindings[0].native.agent_kind, AgentCliKind::Codex);
    let target = item
        .application
        .targets
        .iter()
        .find(|target| target.agent_kind == AgentCliKind::ClaudeCode)
        .unwrap();
    assert!(target.available);
    let target_id = target.id.clone();
    assert!(
        service
            .plan(
                "fixture",
                AgentCatalogPlanRequest {
                    source: AgentCatalogPlanSource::Catalog {
                        asset_id: item.id.clone(),
                        expected_version: None,
                    },
                    action: AgentCatalogAction::ApplyDefinition,
                    target_ids: vec![target_id.clone()],
                    expected_revision: catalog.revision.clone(),
                    workspace: None,
                },
                inspector.clone()
            )
            .is_err(),
        "a catalog source needs a stored version; native installation must select its exact binding"
    );
    assert!(!destination.exists());
    let saved = service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: source,
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(item.version, Some(1));
    assert!(item.application.available && item.application.source_binding_id.is_none());
    assert!(item
        .application
        .targets
        .iter()
        .any(|target| target.id == target_id && target.available));
    let plan = plan(
        &service,
        inspector.clone(),
        &saved.asset_id,
        AgentCatalogAction::ApplyDefinition,
        vec![target_id],
    );
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].agent_kind, AgentCliKind::ClaudeCode);
    assert!(plan.targets[0].available);
    assert!(
        !destination.exists(),
        "planning never installs a native package"
    );
    assert_verified(&run(&service, plan));
    assert_eq!(
        fs::read(destination.join("scripts/run.sh")).unwrap(),
        b"#!/bin/sh\nprintf fixture\n"
    );
    assert_ne!(
        fs::metadata(destination.join("scripts/run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(item.bindings.len(), 2);
    for binding in &item.bindings {
        let native = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == binding.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&binding.native).unwrap(),
            serde_json::to_value(native).unwrap()
        );
    }
    fs::remove_dir_all(&destination).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(item.bindings.len(), 1);
    assert_eq!(item.unresolved_targets.len(), 1);
    assert_eq!(
        item.unresolved_targets[0].state,
        AgentCatalogUnresolvedState::Missing
    );
    assert_eq!(
        agent_observation(item, AgentCliKind::ClaudeCode).state,
        AgentCatalogObservationState::Observed,
        "retained exact target history takes precedence over absence-only UI"
    );
}

#[test]
fn application_targets_validate_sse_cwd_and_hook_variants() {
    let (_temporary, inspector, service) = fixture();
    for (name, sse) in [("sse-fixture", true), ("cwd-fixture", false)] {
        let mut request = mcp_request(name);
        let input = request.mcp.as_mut().unwrap();
        if sse {
            input.transport = Some(AgentMcpTransport::Sse);
            input.command = None;
            input.args.clear();
            input.environment.clear();
            input.url = Some("https://example.invalid/sse".to_owned());
        } else {
            input.cwd = Some("/fixture/workspace".to_owned());
        }
        let saved = service.save(request).unwrap();
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let item = catalog
            .assets
            .iter()
            .find(|asset| asset.id == saved.asset_id)
            .unwrap();
        assert!(item.application.available && item.application.source_binding_id.is_none());
        for target in &item.application.targets {
            let supported = if sse {
                matches!(
                    target.agent_kind,
                    AgentCliKind::ClaudeCode | AgentCliKind::Gemini | AgentCliKind::Grok
                )
            } else {
                target.agent_kind != AgentCliKind::ClaudeCode
            };
            assert_eq!(target.available, supported, "{name}: {target:?}");
        }
    }
    let saved = service.save(AgentCatalogSaveRequest {
        asset_id: None, expected_version: None, name: "explicit-hook".to_owned(), category: AgentAssetCategory::Hook,
        mcp: None, skill_markdown: None,
        hook: Some(AgentCatalogHookInput { variants: vec![AgentCatalogHookVariantInput {
            agent_kind: AgentCliKind::ClaudeCode, event: "PreToolUse".to_owned(),
            group_json: json!({"matcher":"Bash","hooks":[{"type":"command","command":"printf fixture"}]}).to_string(),
        }] }),
    }).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert!(item.application.available);
    assert!(item
        .application
        .targets
        .iter()
        .any(|target| target.agent_kind == AgentCliKind::ClaudeCode && target.available));
    for target in item
        .application
        .targets
        .iter()
        .filter(|target| target.agent_kind != AgentCliKind::ClaudeCode)
    {
        assert!(!target.available);
        assert!(target.reason.as_deref().unwrap().contains("原生 Hook 变体"));
    }
}
