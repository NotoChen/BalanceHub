//! Provenance must survive the real native inventory and global catalog paths.
use super::*;
use std::collections::BTreeSet;

fn write_skill(home: &Path, relative: &str, name: &str) {
    let directory = home.join(relative);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Synthetic provenance fixture\n---\nRead fixture files.\n"),
    )
    .unwrap();
}

fn native<'a>(
    snapshot: &'a MutationInventory,
    agent: AgentCliKind,
    category: AgentAssetCategory,
    name: &str,
) -> &'a AgentAssetRecord {
    let records = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.agent_kind == agent && asset.category == category && asset.native_id == name
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1, "fixture must find one native {name}");
    records[0]
}

fn evidence<'a>(
    snapshot: &MutationInventory,
    asset: &'a AgentAssetRecord,
) -> &'a AgentAssetProvenance {
    assert_eq!(asset.provenance.len(), 1);
    let evidence = &asset.provenance[0];
    let declaration = snapshot
        .inventory
        .declarations
        .iter()
        .find(|declaration| declaration.id == evidence.declaration_id)
        .unwrap();
    assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
    assert_eq!(declaration.source_id, evidence.source_id);
    assert_eq!(declaration.scope, evidence.scope);
    assert!(asset.resolution.contributor_ids.contains(&declaration.id));
    assert!(snapshot
        .inventory
        .sources
        .iter()
        .any(|source| source.id == evidence.source_id));
    evidence
}

#[test]
fn provenance_distinguishes_bundled_local_shared_and_config_without_author_guesses() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".codex/skills/.system/bundled-helper",
        "bundled-helper",
    );
    write_skill(
        &inspector.home,
        ".codex/skills/official-by-name-only",
        "official-by-name-only",
    );
    write_skill(
        &inspector.home,
        ".agents/skills/shared-helper",
        "shared-helper",
    );
    fs::write(
        inspector.home.join(".codex/config.toml"),
        "[mcp_servers.official-by-name-only]\ncommand = 'fixture-runner'\n",
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let codex = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.agent_kind == AgentCliKind::Codex)
        .collect::<Vec<_>>();
    assert_eq!(codex.len(), 4);
    for (category, name, scope, installation, provision, provider) in [
        (
            AgentAssetCategory::Skill,
            "bundled-helper",
            AgentAssetScope::System,
            AgentAssetInstallationOrigin::Bundled,
            AgentAssetProvision::AgentBuiltIn,
            AgentAssetProviderOrigin::Unknown,
        ),
        (
            AgentAssetCategory::Skill,
            "official-by-name-only",
            AgentAssetScope::User,
            AgentAssetInstallationOrigin::LocalFiles,
            AgentAssetProvision::Independent,
            AgentAssetProviderOrigin::Unknown,
        ),
        (
            AgentAssetCategory::Skill,
            "shared-helper",
            AgentAssetScope::User,
            AgentAssetInstallationOrigin::SharedFiles,
            AgentAssetProvision::Independent,
            AgentAssetProviderOrigin::Unknown,
        ),
        (
            AgentAssetCategory::Mcp,
            "official-by-name-only",
            AgentAssetScope::User,
            AgentAssetInstallationOrigin::ConfigEntry,
            AgentAssetProvision::Independent,
            AgentAssetProviderOrigin::Unknown,
        ),
    ] {
        let record = native(&snapshot, AgentCliKind::Codex, category, name);
        let proof = evidence(&snapshot, record);
        assert_eq!(proof.scope, scope);
        assert_eq!(proof.installation, installation);
        assert_eq!(proof.provision, provision);
        assert_eq!(proof.provider, provider);
    }
    let catalog = service.catalog(&snapshot).unwrap();
    let bundled = catalog
        .assets
        .iter()
        .find(|asset| asset.name == "bundled-helper")
        .unwrap();
    assert_eq!(
        bundled.provenance.provisions,
        [AgentAssetProvision::AgentBuiltIn]
    );
    assert_eq!(
        bundled.provenance.providers,
        [AgentAssetProviderOrigin::Unknown]
    );
}

#[test]
fn project_local_mcp_provenance_uses_logical_scope_inside_a_user_config_file() {
    let (_temporary, inspector, service) = fixture();
    let workspace = inspector.home.join("project");
    fs::create_dir(&workspace).unwrap();
    let value = serde_json::json!({
        "projects": {
            (workspace.to_string_lossy().as_ref()): {
                "hasTrustDialogAccepted": true,
                "mcpServers": { "local-proof": { "command": "fixture-runner" } }
            }
        }
    });
    fs::write(
        inspector.home.join(".claude.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let snapshot = catalog_fixture_inventory(
        &inspector.home,
        Some(&workspace),
        None,
        &fixture_definitions(),
    )
    .unwrap();
    let record = native(
        &snapshot,
        AgentCliKind::ClaudeCode,
        AgentAssetCategory::Mcp,
        "local-proof",
    );
    let proof = evidence(&snapshot, record);
    assert_eq!(record.scope, AgentAssetScope::Local);
    assert_eq!(proof.scope, AgentAssetScope::Local);
    let source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == proof.source_id)
        .unwrap();
    assert_eq!(source.scope, AgentAssetScope::User);
    assert_eq!(
        proof.installation,
        AgentAssetInstallationOrigin::ConfigEntry
    );
    assert_eq!(proof.provider, AgentAssetProviderOrigin::Unknown);
    let catalog = service.catalog(&snapshot).unwrap();
    let binding = catalog
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .find(|binding| binding.id == record.stable_id)
        .unwrap();
    assert_eq!(binding.native.provenance, record.provenance);
}

#[test]
fn configuration_overlays_do_not_replace_the_skill_definition_provenance() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".gemini/skills/local-helper",
        "local-helper",
    );
    fs::write(
        inspector.home.join(".gemini/settings.json"),
        br#"{"skills":{"disabled":["local-helper"]}}"#,
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let record = native(
        &snapshot,
        AgentCliKind::Gemini,
        AgentAssetCategory::Skill,
        "local-helper",
    );
    assert_eq!(record.effective_state, AgentAssetState::Disabled);
    assert!(snapshot
        .inventory
        .declarations
        .iter()
        .any(|declaration| declaration.role != AgentAssetDeclarationRole::Definition));
    let proof = evidence(&snapshot, record);
    assert_eq!(proof.installation, AgentAssetInstallationOrigin::LocalFiles);
    assert_eq!(proof.provider, AgentAssetProviderOrigin::Unknown);
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.name == "local-helper")
        .unwrap();
    assert_eq!(
        item.provenance.installations,
        [AgentAssetInstallationOrigin::LocalFiles]
    );
}

#[test]
fn extension_children_keep_exact_parent_and_definition_origins_without_independent_adoption() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".gemini/extensions/fixture-package/skills/child-helper",
        "child-helper",
    );
    fs::write(
        inspector.home.join(".gemini/extensions/fixture-package/gemini-extension.json"),
        br#"{"name":"fixture-package","version":"1.0.0","mcpServers":{"child-mcp":{"command":"fixture-runner"}}}"#,
    )
    .unwrap();
    fs::write(
        inspector.home.join(".gemini/extension-enablement.json"),
        br#"{"fixture-package":{"overrides":[]}}"#,
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.assets.len(), 3);
    let parent = native(
        &snapshot,
        AgentCliKind::Gemini,
        AgentAssetCategory::Extension,
        "fixture-package",
    );
    let proof = evidence(&snapshot, parent);
    assert_eq!(proof.provision, AgentAssetProvision::Independent);
    assert_eq!(
        proof.installation,
        AgentAssetInstallationOrigin::NativePackage
    );
    assert_eq!(proof.provider, AgentAssetProviderOrigin::Unknown);
    let catalog = service.catalog(&snapshot).unwrap();
    for (category, name) in [
        (AgentAssetCategory::Skill, "child-helper"),
        (AgentAssetCategory::Mcp, "child-mcp"),
    ] {
        let child = native(&snapshot, AgentCliKind::Gemini, category, name);
        assert_eq!(
            child.relationships.provided_by.as_ref(),
            Some(&parent.stable_id)
        );
        let proof = evidence(&snapshot, child);
        assert_eq!(proof.provision, AgentAssetProvision::PluginProvided);
        assert_eq!(
            proof.installation,
            AgentAssetInstallationOrigin::NativePackage
        );
        assert_eq!(proof.provider, AgentAssetProviderOrigin::Unknown);
        let item = catalog
            .assets
            .iter()
            .find(|asset| {
                asset
                    .bindings
                    .iter()
                    .any(|binding| binding.id == child.stable_id)
            })
            .unwrap();
        assert_eq!(
            item.provenance.provisions,
            [AgentAssetProvision::PluginProvided]
        );
        assert!(!item.bindings[0].can_adopt);
        assert!(service
            .adopt(
                AgentCatalogAdoptRequest {
                    binding_id: child.stable_id.clone(),
                    expected_revision: catalog.revision.clone(),
                    workspace: None,
                },
                inspector.clone()
            )
            .is_err());
    }
}

#[test]
#[cfg(unix)]
fn shared_package_and_claude_alias_remain_one_global_asset_with_mixed_installation_evidence() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".agents/skills/shared-proof",
        "shared-proof",
    );
    fs::create_dir_all(inspector.home.join(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        "../../.agents/skills/shared-proof",
        inspector.home.join(".claude/skills/claude-alias"),
    )
    .unwrap();
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    assert_eq!(catalog.assets.len(), 1);
    let item = &catalog.assets[0];
    assert_eq!(item.bindings.len(), 4);
    assert_eq!(item.variants.len(), 1);
    assert_eq!(
        item.provenance.provisions,
        [AgentAssetProvision::Independent]
    );
    assert_eq!(
        item.provenance.installations,
        [
            AgentAssetInstallationOrigin::SharedFiles,
            AgentAssetInstallationOrigin::Linked
        ]
    );
    assert_eq!(
        item.provenance.providers,
        [AgentAssetProviderOrigin::Unknown]
    );
    for binding in &item.bindings {
        let proof = evidence(&snapshot, &binding.native);
        assert_eq!(
            proof.installation,
            if binding.native.agent_kind == AgentCliKind::ClaudeCode {
                AgentAssetInstallationOrigin::Linked
            } else {
                AgentAssetInstallationOrigin::SharedFiles
            }
        );
    }
    let refreshed = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(refreshed.assets.len(), 1);
    assert_eq!(refreshed.assets[0].id, item.id);
    for binding in &item.bindings {
        let again = refreshed.assets[0]
            .bindings
            .iter()
            .find(|again| again.id == binding.id)
            .unwrap();
        assert_eq!(again.native.provenance, binding.native.provenance);
    }
}

#[test]
fn adopting_a_local_skill_changes_library_ownership_without_claiming_authorship() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".codex/skills/local-helper",
        "local-helper",
    );
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    assert_eq!(catalog.assets.len(), 1);
    let before = &catalog.assets[0];
    assert_eq!(before.ownership, AgentCatalogOwnership::Observed);
    assert!(before.bindings[0].can_adopt);
    let adopted = service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: before.bindings[0].id.clone(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    let after = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = after
        .assets
        .iter()
        .find(|asset| asset.id == adopted.asset_id)
        .unwrap();
    assert_eq!(item.id, before.id);
    assert_eq!(item.ownership, AgentCatalogOwnership::Managed);
    assert_eq!(
        item.bindings[0].native.provenance,
        before.bindings[0].native.provenance
    );
    assert_eq!(
        item.provenance.providers,
        [AgentAssetProviderOrigin::Unknown]
    );
    assert_eq!(
        item.provenance.provisions,
        [AgentAssetProvision::Independent]
    );
}

#[test]
fn unapplied_library_definition_does_not_invent_native_provenance() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("unapplied-fixture")).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert!(item.bindings.is_empty());
    assert_eq!(item.ownership, AgentCatalogOwnership::Managed);
    assert!(item.provenance.provisions.is_empty());
    assert!(item.provenance.installations.is_empty());
    assert!(item.provenance.providers.is_empty());
}

#[test]
fn replacement_payloads_and_provenance_follow_the_native_anchor_regardless_of_declaration_order() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home,
        ".gemini/extensions/fixture-package/skills/shared-name",
        "shared-name",
    );
    write_skill(&inspector.home, ".gemini/skills/shared-name", "shared-name");
    fs::write(
        inspector.home.join(".gemini/skills/shared-name/SKILL.md"),
        "---\nname: shared-name\ndescription: User fixture\n---\nCurrent user edition.\n",
    )
    .unwrap();
    fs::write(inspector.home.join(".gemini/extensions/fixture-package/gemini-extension.json"),
        br#"{"name":"fixture-package","version":"1.0.0","mcpServers":{"shared-name":{"command":"extension-runner"}}}"#).unwrap();
    fs::write(
        inspector.home.join(".gemini/settings.json"),
        br#"{"mcpServers":{"shared-name":{"command":"user-runner"}}}"#,
    )
    .unwrap();
    fs::write(
        inspector.home.join(".gemini/extension-enablement.json"),
        br#"{"fixture-package":{"overrides":[]}}"#,
    )
    .unwrap();
    let mut snapshot = inspector.inspect().unwrap();
    assert_eq!(snapshot.inventory.assets.len(), 5);
    // Public evidence ordering has no native precedence meaning. Put all
    // overwritten package Definitions first to make this regression deterministic.
    let winner_sources = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .map(|asset| asset.inspection_source_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(winner_sources.len(), 2);
    snapshot
        .inventory
        .declarations
        .sort_by_key(|declaration| winner_sources.contains(&declaration.source_id));
    for category in [AgentAssetCategory::Skill, AgentAssetCategory::Mcp] {
        let rows = snapshot
            .inventory
            .assets
            .iter()
            .filter(|asset| asset.category == category)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        let winner = rows
            .iter()
            .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
            .unwrap();
        let loser = rows
            .iter()
            .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .unwrap();
        let source = projection::definition_source(&snapshot, winner).unwrap();
        assert_eq!(source.id, winner.inspection_source_id);
        let proof = evidence(&snapshot, winner);
        assert_eq!(proof.provision, AgentAssetProvision::Independent);
        assert_eq!(proof.provider, AgentAssetProviderOrigin::Unknown);
        assert_eq!(
            evidence(&snapshot, loser).provision,
            AgentAssetProvision::PluginProvided
        );
        assert_ne!(
            crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                .physical_key(winner),
            crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                .physical_key(loser)
        );
        match projection::observe_payload(&snapshot, winner).unwrap() {
            definition::DefinitionPayload::Skill(files) => {
                assert!(String::from_utf8_lossy(&files["SKILL.md"].bytes)
                    .contains("Current user edition"))
            }
            definition::DefinitionPayload::Mcp(mcp) => {
                assert_eq!(mcp.command.as_deref(), Some("user-runner"))
            }
            definition::DefinitionPayload::Hook(_) => {
                panic!("fixture contains only MCP and Skill definitions")
            }
        }
    }
    let catalog = service.catalog(&snapshot).unwrap();
    assert_eq!(catalog.assets.len(), 5);
    for item in &catalog.assets {
        assert_eq!(item.bindings.len(), 1);
        let binding = &item.bindings[0];
        if binding.native.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner {
            assert_eq!(
                item.provenance.provisions,
                [AgentAssetProvision::Independent]
            );
            assert!(binding.can_adopt);
        }
    }
}

#[test]
fn merged_native_mcp_keeps_all_origins_without_single_file_equivalence_or_adoption() {
    let (_temporary, inspector, service) = fixture();
    let workspace = inspector.home.join("project");
    fs::create_dir_all(workspace.join(".codex")).unwrap();
    let mut config = toml::Table::new();
    config.insert(
        "projects".to_owned(),
        toml::Value::Table(toml::Table::from_iter([(
            workspace.to_string_lossy().into_owned(),
            toml::Value::Table(toml::Table::from_iter([(
                "trust_level".to_owned(),
                toml::Value::String("trusted".to_owned()),
            )])),
        )])),
    );
    config.insert(
        "mcp_servers".to_owned(),
        toml::Value::Table(toml::Table::from_iter([(
            "merged-fixture".to_owned(),
            toml::Value::Table(toml::Table::from_iter([(
                "command".to_owned(),
                toml::Value::String("fixture-runner".to_owned()),
            )])),
        )])),
    );
    fs::write(
        inspector.home.join(".codex/config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(
        workspace.join(".codex/config.toml"),
        "[mcp_servers.merged-fixture]\nargs=['workspace-argument']\n",
    )
    .unwrap();
    let snapshot = catalog_fixture_inventory(
        &inspector.home,
        Some(&workspace),
        None,
        &fixture_definitions(),
    )
    .unwrap();
    let record = native(
        &snapshot,
        AgentCliKind::Codex,
        AgentAssetCategory::Mcp,
        "merged-fixture",
    );
    assert_eq!(
        record.resolution.relation,
        AgentAssetResolutionRelation::Merged
    );
    assert_eq!(record.provenance.len(), 2);
    assert_eq!(
        record
            .provenance
            .iter()
            .map(|proof| proof.scope)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([AgentAssetScope::User, AgentAssetScope::Workspace])
    );
    assert!(snapshot
        .inventory
        .sources
        .iter()
        .any(|source| source.id == record.inspection_source_id));
    assert!(projection::definition_source(&snapshot, record).is_none());
    assert!(
        crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
            .physical_key(record)
            .is_none()
    );
    assert!(projection::observe_payload(&snapshot, record).is_err());
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| {
            asset
                .bindings
                .iter()
                .any(|binding| binding.id == record.stable_id)
        })
        .unwrap();
    assert_eq!(item.bindings.len(), 1);
    assert!(!item.bindings[0].can_adopt);
    assert!(item.bindings[0].reason.is_some());
}
