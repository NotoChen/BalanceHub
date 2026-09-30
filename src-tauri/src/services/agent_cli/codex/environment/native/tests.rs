mod hooks;
mod mcp_formats;
mod support;

use super::*;
use crate::models::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentIndex, AgentAssetAssessmentRequest, AgentAssetAssessmentResult,
    AgentAssetAssessmentSubject, AgentAssetAssessmentTarget, AgentAssetDeclaredStateProofDraft,
    AgentAssetEffectiveStateProofDraft, AgentAssetNativePayload, AgentAssetProjectedDraft,
    AgentAssetResolveRequest, AgentAssetSourceSpec, AgentResolveOutput, CodexAssetPayload,
    ParsedAgentAsset,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use support::*;

const PRIVATE_DESCRIPTION: &str = "codex-private-description-sentinel";
const PRIVATE_BODY: &str = "codex-private-body-sentinel";
const PRIVATE_COMMAND: &str = "codex-private-command-never-execute";
const PRIVATE_TOKEN: &str = "codex-private-token-sentinel";

#[test]
fn real_codex_skills_need_valid_documents_and_follow_ordered_user_rules() {
    let fixture = Fixture::new("skills");
    let user_path = fixture.home.join(".codex/skills/one/SKILL.md");
    skill(&user_path, "Same Name");
    skill(
        &fixture.workspace.join(".agents/skills/two/SKILL.md"),
        "Same Name",
    );
    skill(
        &fixture.home.join(".codex/skills/.system/internal/SKILL.md"),
        "internal",
    );
    write(
        &fixture.workspace.join(".codex/skills/repaired/SKILL.md"),
        format!("---\ndescription: Use: {PRIVATE_DESCRIPTION}\n---\n{PRIVATE_BODY}"),
    );
    fs::create_dir_all(fixture.home.join(".codex/skills/directory-only")).unwrap();
    write(
        &fixture.home.join(".codex/skills/invalid/SKILL.md"),
        "---\nname: invalid\ndescription: ''\n---\n",
    );
    write(
        &fixture.workspace.join(".codex/config.toml"),
        "[[skills.config]]\nname='Same Name'\nenabled=false\n",
    );
    let path = toml::Value::String(user_path.to_string_lossy().into_owned()).to_string();
    let path_false = format!("[[skills.config]]\npath={path}\nenabled=false\n");
    let path_true = format!("[[skills.config]]\npath={path}\nenabled=true\n");
    let name_false = "[[skills.config]]\nname=' Same Name '\nenabled=false\n";
    let name_true = "[[skills.config]]\nname='Same Name'\nenabled=true\n";
    let cases = [
        (String::new(), [AgentAssetState::Enabled; 2]),
        (
            format!("{path_false}{name_true}"),
            [AgentAssetState::Enabled; 2],
        ),
        (
            format!("{name_false}{path_true}"),
            [AgentAssetState::Enabled, AgentAssetState::Disabled],
        ),
        (
            format!("{path_false}{path_true}"),
            [AgentAssetState::Enabled; 2],
        ),
        (
            format!("{name_false}[[skills.config]]\nname='Same Name'\npath={path}\nenabled=true\n"),
            [AgentAssetState::Disabled; 2],
        ),
        (
            "[[skills.config]]\nname='Same Name'\nenabled='invalid'\n".to_owned(),
            [AgentAssetState::Enabled; 2],
        ),
    ];
    let mut ids = None;
    for (config, expected) in cases {
        let run = fixture.run::<false>(&config);
        assert_eq!(run.inventory.assets.len(), 4);
        assert_eq!(run.rows(AgentAssetCategory::Skill).len(), 4);
        let mut duplicates = run
            .rows(AgentAssetCategory::Skill)
            .into_iter()
            .filter(|row| row.native_id == "Same Name")
            .collect::<Vec<_>>();
        duplicates.sort_by_key(|row| row.scope);
        assert_eq!(duplicates.len(), 2);
        let user = duplicates
            .iter()
            .find(|row| row.scope == AgentAssetScope::User)
            .unwrap();
        let workspace = duplicates
            .iter()
            .find(|row| row.scope == AgentAssetScope::Workspace)
            .unwrap();
        for (row, expected) in [(*user, expected[0]), (*workspace, expected[1])] {
            assert_eq!(row.declared_state, expected);
            assert_eq!(row.effective_state, expected);
            assert_eq!(
                row.resolution.relation,
                AgentAssetResolutionRelation::Additive
            );
            assert_eq!(
                Path::new(row.path.as_deref().unwrap()).file_name().unwrap(),
                "SKILL.md"
            );
            assert!(!row.is_directory);
        }
        let current = vec![user.stable_id.clone(), workspace.stable_id.clone()];
        if let Some(previous) = &ids {
            assert_eq!(previous, &current);
        } else {
            ids = Some(current);
        }
        assert_eq!(
            run.row(AgentAssetCategory::Skill, "internal").scope,
            AgentAssetScope::System
        );
        assert_eq!(
            run.row(AgentAssetCategory::Skill, "repaired").scope,
            AgentAssetScope::Workspace
        );
    }
}

#[test]
fn real_codex_plugin_parent_toggle_preserves_child_declared_state_and_owners() {
    let fixture = Fixture::new("parent");
    let plugin = fixture.plugin("configured-id@fixture", "local", "namespace");
    skill(&plugin.join("skills/writer/SKILL.md"), "writer");
    write(
        &plugin.join(".mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "server": {"command": PRIVATE_COMMAND},
        }))
        .unwrap(),
    );
    let mut identity = None;
    for (config, parent_state) in [
        (
            "[plugins.'configured-id@fixture']\n",
            AgentAssetState::Enabled,
        ),
        (
            "[plugins.'configured-id@fixture']\nenabled=false\n",
            AgentAssetState::Disabled,
        ),
    ] {
        let run = fixture.run::<false>(config);
        assert_eq!(run.inventory.assets.len(), 3);
        let parent = run.row(AgentAssetCategory::Plugin, "configured-id@fixture");
        assert_eq!(parent.effective_state, parent_state);
        assert!(matches!(
            parent.details,
            AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                ..
            }
        ));
        let skill = run.row(AgentAssetCategory::Skill, "namespace:writer");
        let mcp = run.row(AgentAssetCategory::Mcp, "server");
        for child in [skill, mcp] {
            assert_eq!(child.declared_state, AgentAssetState::Enabled);
            assert_eq!(child.effective_state, parent_state);
            assert_eq!(
                child.relationships.provided_by.as_deref(),
                Some(parent.stable_id.as_str())
            );
            assert!(matches!(
                run.draft(child.category, &child.native_id)
                    .state_proof
                    .effective,
                AgentAssetEffectiveStateProofDraft::ParentGate { .. }
            ));
        }
        assert!(skill.relationships.action_owner.is_none());
        assert_eq!(
            mcp.relationships.action_owner.as_deref(),
            Some(parent.stable_id.as_str())
        );
        let current = [
            parent.stable_id.clone(),
            skill.stable_id.clone(),
            mcp.stable_id.clone(),
        ];
        if let Some(previous) = &identity {
            assert_eq!(previous, &current);
        } else {
            identity = Some(current);
        }
    }
    let run = fixture.run::<false>("[plugins.'configured-id@fixture']\n[plugins.'configured-id@fixture'.mcp_servers.server]\nenabled=false\n");
    assert_eq!(run.inventory.assets.len(), 3);
    assert_eq!(
        run.row(AgentAssetCategory::Mcp, "server").declared_state,
        AgentAssetState::Disabled
    );
    let mcp = run.row(AgentAssetCategory::Mcp, "server");
    let policy = run
        .capture
        .declarations
        .iter()
        .find(|asset| asset.declaration_key == "plugins.mcp:configured-id@fixture:server")
        .unwrap();
    assert_eq!(policy.role, AgentAssetDeclarationRole::PolicyOverlay);
    assert_eq!(policy.resolution_group_key, policy.declaration_key);
    assert_eq!(mcp.effective_state, AgentAssetState::Disabled);
    assert!(!mcp
        .resolution
        .contributor_ids
        .contains(&policy.declaration_id));
    assert!(run
        .inventory
        .declarations
        .iter()
        .any(|declaration| declaration.id == policy.declaration_id));
    assert!(matches!(
        run.draft(AgentAssetCategory::Mcp, "server")
            .state_proof
            .declared,
        AgentAssetDeclaredStateProofDraft::Policy { .. }
    ));
}

#[test]
fn real_codex_plugin_cache_binding_ignores_orphans_and_uses_local_then_semver() {
    let fixture = Fixture::new("versions");
    for (version, namespace) in [
        ("local", "local-space"),
        ("2.0.0", "old-space"),
        ("10.0.0", "new-space"),
    ] {
        let root = fixture.plugin("selected@fixture", version, namespace);
        skill(&root.join("skills/task/SKILL.md"), "task");
    }
    let orphan = fixture.plugin("orphan@fixture", "local", "orphan");
    skill(&orphan.join("skills/task/SKILL.md"), "task");
    let broken = fixture.plugin("broken@fixture", "local", "broken");
    write(&broken.join(".codex-plugin/plugin.json"), "{ malformed");
    let config =
        "[plugins.'selected@fixture']\n[plugins.'missing@fixture']\n[plugins.'broken@fixture']\n";
    let run = fixture.run::<false>(config);
    assert_eq!(run.inventory.assets.len(), 4);
    assert_eq!(run.rows(AgentAssetCategory::Plugin).len(), 3);
    assert_eq!(run.rows(AgentAssetCategory::Skill).len(), 1);
    assert_eq!(
        run.row(AgentAssetCategory::Skill, "local-space:task")
            .effective_state,
        AgentAssetState::Enabled
    );
    assert!(matches!(
        run.row(AgentAssetCategory::Plugin, "missing@fixture")
            .details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::NotInstalled,
            ..
        }
    ));
    assert!(matches!(
        run.row(AgentAssetCategory::Plugin, "broken@fixture")
            .details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::Unknown,
            ..
        }
    ));
    let missing = run.row(AgentAssetCategory::Plugin, "missing@fixture");
    assert_eq!(missing.declared_state, AgentAssetState::Enabled);
    assert_eq!(missing.effective_state, AgentAssetState::NotInstalled);
    let broken = run.row(AgentAssetCategory::Plugin, "broken@fixture");
    assert_eq!(broken.declared_state, AgentAssetState::Enabled);
    assert_eq!(broken.effective_state, AgentAssetState::Unknown);
    for (row, reason) in [
        (
            missing,
            AgentAssetActionUnavailableReason::AssetNotInstalled,
        ),
        (
            broken,
            AgentAssetActionUnavailableReason::AssetInstallationUnknown,
        ),
    ] {
        for action in row.actions.iter().filter(|action| {
            matches!(
                action.action,
                AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
            )
        }) {
            assert!(!action.available);
            assert_eq!(action.reason, Some(reason));
        }
    }
    assert!(!run
        .inventory
        .assets
        .iter()
        .any(|row| row.native_id.contains("orphan")));
    let selected = run.row(AgentAssetCategory::Plugin, "selected@fixture");
    let local_manifest = run
        .inventory
        .sources
        .iter()
        .find(|source| {
            source
                .path
                .ends_with("selected/local/.codex-plugin/plugin.json")
        })
        .unwrap();
    assert!(selected.source_ids.contains(&local_manifest.id));
    fs::remove_dir_all(
        fixture
            .home
            .join(".codex/plugins/cache/fixture/selected/local"),
    )
    .unwrap();
    let updated = fixture.run::<false>(config);
    assert_eq!(updated.inventory.assets.len(), 4);
    let next = updated.row(AgentAssetCategory::Plugin, "selected@fixture");
    assert_ne!(next.stable_id, selected.stable_id);
    assert_eq!(next.context_id, selected.context_id);
    assert_eq!(next.inspection_source_id, selected.inspection_source_id);
    let next_manifest = updated
        .inventory
        .sources
        .iter()
        .find(|source| {
            source
                .path
                .ends_with("selected/10.0.0/.codex-plugin/plugin.json")
        })
        .unwrap();
    assert!(!next.source_ids.contains(&local_manifest.id));
    assert!(next.source_ids.contains(&next_manifest.id));
    assert_eq!(
        updated
            .row(AgentAssetCategory::Skill, "new-space:task")
            .effective_state,
        AgentAssetState::Enabled
    );
    let repeated = fixture.run::<false>(config);
    assert_eq!(updated.normalized_rows(), repeated.normalized_rows());
}

#[test]
fn real_codex_mcp_catalog_precedence_keeps_only_the_winners_parent() {
    let fixture = Fixture::new("catalog");
    for id in ["a@fixture", "z@fixture"] {
        let root = fixture.plugin(id, "local", id.split('@').next().unwrap());
        write(
            &root.join(".mcp.json"),
            serde_json::to_vec(&serde_json::json!({
                "mcpServers": {"same": {"command": PRIVATE_COMMAND}},
            }))
            .unwrap(),
        );
    }
    let config = format!("[plugins.'a@fixture']\n[plugins.'a@fixture'.mcp_servers.same]\nenabled=false\n[plugins.'z@fixture']\n[mcp_servers.same]\ncommand='{PRIVATE_COMMAND}'\n");
    let run = fixture.run::<false>(&config);
    assert_eq!(run.inventory.assets.len(), 5);
    assert_eq!(run.rows(AgentAssetCategory::Mcp).len(), 3);
    let winner = run.row(AgentAssetCategory::Mcp, "same");
    assert_eq!(
        winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);
    assert!(winner.relationships.provided_by.is_none());
    assert!(winner.relationships.action_owner.is_none());
    for loser in run
        .rows(AgentAssetCategory::Mcp)
        .into_iter()
        .filter(|row| row.stable_id != winner.stable_id)
    {
        assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
        assert_eq!(
            loser.resolution.relation,
            AgentAssetResolutionRelation::Replaced
        );
        assert_eq!(
            loser.resolution.winner_id.as_deref(),
            Some(winner.stable_id.as_str())
        );
        assert!(loser.relationships.provided_by.is_some());
        let parent = run.row(AgentAssetCategory::Plugin, "a@fixture");
        if loser.relationships.provided_by.as_deref() == Some(parent.stable_id.as_str()) {
            assert_eq!(loser.declared_state, AgentAssetState::Disabled);
            let policy = run
                .capture
                .declarations
                .iter()
                .find(|asset| asset.declaration_key == "plugins.mcp:a@fixture:same")
                .unwrap();
            assert!(!loser
                .resolution
                .contributor_ids
                .contains(&policy.declaration_id));
        }
    }
    let reversed = fixture.run::<true>(&config);
    assert_eq!(run.normalized_rows(), reversed.normalized_rows());
    let plugin_winner =
        fixture.run::<false>("[plugins.'a@fixture']\nenabled=false\n[plugins.'z@fixture']\n");
    assert_eq!(plugin_winner.inventory.assets.len(), 4);
    let winner = plugin_winner.row(AgentAssetCategory::Mcp, "same");
    let parent = plugin_winner.row(AgentAssetCategory::Plugin, "z@fixture");
    assert_eq!(
        winner.relationships.provided_by.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);
}

#[test]
fn independent_codex_assessment_rejects_incomplete_policy_batches_and_forged_parent_bindings() {
    let fixture = Fixture::new("assessment");
    let root = fixture.plugin("parent@fixture", "local", "ns");
    skill(&root.join("skills/child/SKILL.md"), "child");
    write(
        &root.join(".mcp.json"),
        format!("{{\"server\":{{\"command\":\"{PRIVATE_COMMAND}\"}}}}"),
    );
    let run = fixture.run::<false>("[plugins.'parent@fixture']\n[plugins.'parent@fixture'.mcp_servers.server]\nenabled=false\n");
    assert_eq!(run.inventory.assets.len(), 3);
    let raw = &run.capture.declarations;
    for key in ["plugins:root", "plugins.mcp:parent@fixture:server"] {
        let interrupted = raw
            .iter()
            .filter(|asset| asset.declaration_key != key)
            .cloned()
            .collect::<Vec<_>>();
        assert!(matches!(
            run.assess(&interrupted, AgentAssetCategory::Plugin, "parent@fixture"),
            AgentAssetAssessmentResult::Unsupported(_)
        ));
        assert!(matches!(
            run.assess(&interrupted, AgentAssetCategory::Mcp, "server"),
            AgentAssetAssessmentResult::Unsupported(_)
        ));
    }
    let mut no_rules = raw.clone();
    no_rules.retain(|asset| asset.declaration_key != "skills.config");
    assert!(matches!(
        run.assess(&no_rules, AgentAssetCategory::Skill, "ns:child"),
        AgentAssetAssessmentResult::Unsupported(_)
    ));
    for corrupt in 0..4 {
        let mut changed = raw.clone();
        let child = changed
            .iter_mut()
            .find(|asset| asset.native_id == "ns:child")
            .unwrap();
        match corrupt {
            0 => child.provided_by = None,
            1 => child.action_owner = child.provided_by.clone(),
            2 => {
                if let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillDefinition {
                    plugin_id,
                    ..
                }) = &mut child.native_payload
                {
                    *plugin_id = None;
                }
            }
            _ => {
                if let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillDefinition {
                    canonical_path,
                    ..
                }) = &mut child.native_payload
                {
                    *canonical_path = fixture.home.join("unrelated/SKILL.md");
                }
            }
        }
        assert!(matches!(
            run.assess(&changed, AgentAssetCategory::Skill, "ns:child"),
            AgentAssetAssessmentResult::Unsupported(_)
        ));
    }
    let mut incomplete_versions = raw.clone();
    for asset in &mut incomplete_versions {
        if let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginVersions {
            complete,
            ..
        }) = &mut asset.native_payload
        {
            *complete = false;
        }
    }
    assert!(matches!(
        run.assess(&incomplete_versions, AgentAssetCategory::Skill, "ns:child"),
        AgentAssetAssessmentResult::Unsupported(_)
    ));
    assert!(matches!(
        run.assess(&incomplete_versions, AgentAssetCategory::Mcp, "server"),
        AgentAssetAssessmentResult::Unsupported(_)
    ));
}

#[test]
fn unknown_codex_parent_does_not_overwrite_a_known_disabled_child() {
    let fixture = Fixture::new("unknown-parent");
    let root = fixture.plugin("parent@fixture", "local", "ns");
    skill(&root.join("skills/child/SKILL.md"), "child");
    write(
        &root.join(".mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "server": {"command": PRIVATE_COMMAND, "enabled": false},
        }))
        .unwrap(),
    );
    let run = fixture.run::<false>("[[skills.config]]\nname='ns:child'\nenabled=false\n[plugins.'parent@fixture']\nenabled='invalid'\n");
    assert_eq!(run.inventory.assets.len(), 3);
    assert_eq!(
        run.row(AgentAssetCategory::Plugin, "parent@fixture")
            .effective_state,
        AgentAssetState::Unknown
    );
    for (category, name) in [
        (AgentAssetCategory::Skill, "ns:child"),
        (AgentAssetCategory::Mcp, "server"),
    ] {
        let row = run.row(category, name);
        assert_eq!(row.declared_state, AgentAssetState::Disabled);
        assert_eq!(row.effective_state, AgentAssetState::Disabled);
        assert!(matches!(
            run.draft(category, name).state_proof.effective,
            AgentAssetEffectiveStateProofDraft::Intrinsic
        ));
    }
}

#[test]
fn unavailable_codex_user_config_never_proves_an_enabled_skill() {
    let fixture = Fixture::new("invalid-config");
    skill(&fixture.home.join(".codex/skills/child/SKILL.md"), "child");
    let run = fixture.run_with_workspace::<false>("[[ malformed", false);
    assert_eq!(run.inventory.assets.len(), 1);
    let row = run.row(AgentAssetCategory::Skill, "child");
    assert_eq!(row.declared_state, AgentAssetState::Enabled);
    assert_eq!(row.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        run.draft(AgentAssetCategory::Skill, "child")
            .state_proof
            .effective,
        AgentAssetEffectiveStateProofDraft::Terminal { .. }
    ));
}

#[test]
fn codex_discovery_without_a_snapshot_reader_never_reads_config_out_of_band() {
    use crate::services::agent_cli::contracts::{
        AgentDiagnosticEmission, AgentDiagnosticOutput, AgentSourceDiscoveryRequest,
        InitialSourceOutput,
    };
    use std::ops::ControlFlow;
    let fixture = Fixture::new("no-reader");
    fixture.plugin("private-cache@fixture", "local", "private-cache");
    write(
        &fixture.home.join(".codex/config.toml"),
        "[plugins.'private-cache@fixture']\n",
    );
    #[derive(Default)]
    struct Output {
        keys: Vec<String>,
    }
    impl AgentDiagnosticOutput for Output {
        fn has_regular_capacity(&self) -> bool {
            true
        }
        fn emit_diagnostic(&mut self, _: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            AgentDiagnosticEmission::Accepted
        }
    }
    impl InitialSourceOutput for Output {
        fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
            self.keys.push(source.native_source_key);
            ControlFlow::Continue(())
        }
    }
    let context = AgentConfigurationContext {
        id: "codex-reader-fixture".to_owned(),
        environment_id: "local".to_owned(),
        agent_kind: AgentCliKind::Codex,
        config_root: fixture.home.join(".codex").to_string_lossy().into_owned(),
        profile: "default".to_owned(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: 2,
        schema_facts: Default::default(),
        compatible_installation_ids: Vec::new(),
    };
    let mut output = Output::default();
    sources::discover_sources(
        AgentSourceDiscoveryRequest {
            installations: &[],
            context: &context,
            home: &fixture.home,
            workspace: None,
        },
        &mut output,
    );
    assert!(output.keys.iter().any(|key| key == "config"));
    assert!(!output
        .keys
        .iter()
        .any(|key| key.starts_with(PLUGIN_BASE_PREFIX)));
}
