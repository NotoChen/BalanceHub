//! Regressions for the workbench independent review. These exercise the
//! catalog service with private fixture data and the real inventory pipeline.
use super::{
    definition::DefinitionPayload,
    tests::{fixture, fixture_definitions},
    *,
};
use std::{collections::BTreeSet, fs, path::Path};

fn write_skill(path: &Path, name: &str) {
    fs::create_dir_all(path.join("scripts")).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Fixture package\n---\nRead sources\n"),
    )
    .unwrap();
    fs::write(path.join("scripts/run.py"), "print('shared-resource')\n").unwrap();
}

#[cfg(unix)]
fn adopted_skill(
    inspector: Arc<tests::FixtureInspector>,
    service: &CatalogService,
) -> AgentCatalogDefinition {
    write_skill(
        &inspector.home.join(".claude/skills/shared-package"),
        "shared-package",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let bindings = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .flat_map(|asset| &asset.bindings)
        .collect::<Vec<_>>();
    assert_eq!(
        bindings.len(),
        1,
        "the native fixture must observe the complete source package"
    );
    assert_eq!(bindings[0].native.agent_kind, AgentCliKind::ClaudeCode);
    let saved = service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: bindings[0].id.clone(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector,
        )
        .unwrap();
    assert_eq!(saved.files.len(), 2);
    saved
}

#[cfg(unix)]
fn plan_for_agent(
    service: &CatalogService,
    inspector: Arc<dyn MutationInspector>,
    saved: &AgentCatalogDefinition,
    agent: AgentCliKind,
    expected_targets: usize,
) -> AgentCatalogPlan {
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let targets = catalog
        .targets
        .iter()
        .filter(|target| {
            target.agent_kind == agent
                && target.scope == AgentAssetScope::User
                && target.categories.contains(&saved.category)
        })
        .map(|target| target.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), expected_targets);
    let workspace = inspector
        .workspace()
        .map(|path| path.to_string_lossy().into_owned());
    service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: saved.asset_id.clone(),
                    expected_version: Some(saved.version),
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: targets,
                expected_revision: catalog.revision,
                workspace,
            },
            inspector,
        )
        .unwrap()
}

#[cfg(unix)]
fn run_plan(service: &CatalogService, plan: AgentCatalogPlan) -> AgentCatalogOperation {
    let operation = service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: plan.asset_id,
                action: plan.action,
            },
        )
        .unwrap();
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert_eq!(done.phase, AgentAssetOperationPhase::Completed);
    done
}

#[test]
#[cfg(unix)]
fn distribution_does_not_take_over_an_unowned_directory_without_a_skill_manifest() {
    let (_temporary, inspector, service) = fixture();
    let saved = adopted_skill(inspector.clone(), &service);
    let destination = inspector.home.join(".codex/skills/shared-package");
    fs::create_dir_all(destination.join("scripts")).unwrap();
    fs::write(
        destination.join("scripts/run.py"),
        "print('local-unowned-resource')\n",
    )
    .unwrap();
    let plan = plan_for_agent(&service, inspector, &saved, AgentCliKind::Codex, 1);
    assert!(!plan.targets[0].available);
    assert!(plan.targets[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("未收录"));
    assert!(plan.token.is_none() && plan.plan_id.is_none());
    assert!(service.operations("window").is_empty());
    assert!(!destination.join("SKILL.md").exists());
    assert_eq!(
        fs::read_to_string(destination.join("scripts/run.py")).unwrap(),
        "print('local-unowned-resource')\n"
    );
}

#[test]
#[cfg(unix)]
fn an_exact_receipt_restores_a_missing_skill_manifest_and_preserves_its_resource() {
    let (_temporary, inspector, service) = fixture();
    let saved = adopted_skill(inspector.clone(), &service);
    let plan = plan_for_agent(&service, inspector.clone(), &saved, AgentCliKind::Codex, 1);
    assert!(plan.targets[0].available, "{:?}", plan.targets);
    let done = run_plan(&service, plan);
    assert_eq!(
        done.targets[0].outcome,
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );
    let destination = inspector.home.join(".codex/skills/shared-package");
    let manifest = fs::read(destination.join("SKILL.md")).unwrap();
    let resource = fs::read(destination.join("scripts/run.py")).unwrap();
    fs::remove_file(destination.join("SKILL.md")).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert!(asset
        .bindings
        .iter()
        .all(|binding| binding.native.agent_kind != AgentCliKind::Codex));
    assert!(asset.unresolved_targets.iter().any(|target| {
        target.agent_kind == AgentCliKind::Codex && target.drift == AgentCatalogDrift::Missing
    }));
    let restore = plan_for_agent(&service, inspector.clone(), &saved, AgentCliKind::Codex, 1);
    assert!(restore.targets[0].available, "{:?}", restore.targets);
    let done = run_plan(&service, restore);
    assert_eq!(
        done.targets[0].outcome,
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );
    assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), manifest);
    assert_eq!(
        fs::read(destination.join("scripts/run.py")).unwrap(),
        resource
    );
    let restored = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = restored
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert!(asset.unresolved_targets.is_empty());
    assert!(asset.bindings.iter().any(|binding| {
        binding.native.agent_kind == AgentCliKind::Codex
            && binding.drift == AgentCatalogDrift::InSync
    }));
}

#[test]
fn merging_source_moves_the_complete_current_physical_skill_group() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home.join(".agents/skills/shared-review"),
        "Shared Review",
    );
    write_skill(
        &inspector.home.join(".claude/skills/independent"),
        "independent",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let skills = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skills.len(), 2);
    let shared = skills
        .iter()
        .find(|asset| asset.bindings.len() >= 2)
        .unwrap();
    let destination = skills
        .iter()
        .find(|asset| asset.bindings.len() == 1)
        .unwrap();
    let destination_id = destination.id.clone();
    let old_id = shared.id.clone();
    let expected = skills
        .iter()
        .flat_map(|asset| asset.bindings.iter().map(|binding| binding.id.clone()))
        .collect::<BTreeSet<_>>();
    assert!(
        shared
            .bindings
            .iter()
            .map(|binding| &binding.native.native_id)
            .collect::<BTreeSet<_>>()
            .len()
            >= 2
    );
    let relation = service
        .preview_relation(
            "window",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: destination_id.clone(),
                    source_asset_id: shared.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    assert_eq!(
        relation
            .affected_binding_ids
            .iter()
            .collect::<BTreeSet<_>>(),
        shared
            .bindings
            .iter()
            .map(|binding| &binding.id)
            .collect::<BTreeSet<_>>()
    );
    service
        .commit_relation(
            "window",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    for _ in 0..2 {
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let skills = catalog
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
            .collect::<Vec<_>>();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, destination_id);
        assert_ne!(skills[0].id, old_id);
        assert_eq!(
            skills[0]
                .bindings
                .iter()
                .map(|binding| binding.id.clone())
                .collect::<BTreeSet<_>>(),
            expected
        );
    }
}

#[test]
fn a_stale_physical_observation_cannot_join_a_new_package_at_another_path() {
    let (_temporary, inspector, service) = fixture();
    let old = inspector.home.join(".claude/skills/old-package");
    write_skill(&old, "same-content");
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let old_rows = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(old_rows.len(), 1);
    let old_id = old_rows[0].id.clone();
    fs::remove_dir_all(old).unwrap();
    write_skill(
        &inspector.home.join(".claude/skills/new-package"),
        "same-content",
    );
    let snapshot = inspector.inspect().unwrap();
    let rows = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    let current_key =
        crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
            .physical_key(rows[0]);
    assert!(current_key.is_some());
    // Deterministically simulate device/inode reuse in persisted history;
    // the new package and its current observation are still real inventory.
    service
        .repository
        .transact(|library| {
            let entry = library.entries.get_mut(&old_id).unwrap();
            assert!(!entry.aliases.contains_key(&rows[0].stable_id));
            for observation in entry.aliases.values_mut() {
                observation.physical_key.clone_from(&current_key);
            }
            Ok(())
        })
        .unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let current = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 1);
    assert_ne!(current[0].id, old_id);
    assert_eq!(current[0].bindings[0].id, rows[0].stable_id);
}

#[test]
#[cfg(unix)]
fn conflicting_managed_physical_owners_remain_visible_with_versions_and_blocked_plans() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home.join(".agents/skills/shared-review"),
        "shared-review",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let shared = catalog
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Skill)
        .unwrap();
    assert!(shared.bindings.len() >= 3);
    let moved_binding = shared.bindings[1].id.clone();
    let new_binding = shared.bindings[2].id.clone();
    let first = service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: shared.bindings[0].id.clone(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    let second = service.save(AgentCatalogSaveRequest { hook: None,
        asset_id: None, expected_version: None, name: "other-definition".into(),
        category: AgentAssetCategory::Skill, mcp: None,
        skill_markdown: Some("---\nname: other-definition\ndescription: Other definition\n---\nKeep this version\n".into()),
    }).unwrap();
    service
        .repository
        .transact(|library| {
            // A newly discovered third Agent has no persisted alias yet.
            // Its real current native record must inherit the conflict now.
            for entry in library.entries.values_mut() {
                entry.aliases.remove(&new_binding);
            }
            let alias = library
                .entries
                .get_mut(&first.asset_id)
                .unwrap()
                .aliases
                .remove(&moved_binding)
                .unwrap();
            library
                .entries
                .get_mut(&second.asset_id)
                .unwrap()
                .aliases
                .insert(moved_binding, alias);
            Ok(())
        })
        .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert!(catalog
        .diagnostics
        .iter()
        .any(|message| message.contains("归属冲突")));
    let new_member = catalog
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .find(|binding| binding.id == new_binding)
        .unwrap();
    assert!(!new_member.can_adopt);
    assert_eq!(new_member.drift, AgentCatalogDrift::Unknown);
    assert!(new_member.reason.as_deref().unwrap().contains("归属冲突"));
    for saved in [&first, &second] {
        let asset = catalog
            .assets
            .iter()
            .find(|asset| asset.id == saved.asset_id)
            .unwrap();
        assert_eq!(asset.version, Some(1));
        assert!(!asset.bindings.is_empty());
        assert!(!asset.application.available);
        assert!(asset
            .bindings
            .iter()
            .all(|binding| !binding.can_adopt && binding.drift == AgentCatalogDrift::Unknown));
        assert_eq!(
            service.definition(&saved.asset_id).unwrap().skill_markdown,
            saved.skill_markdown
        );
        // This fixture has both native and shared User Skill destinations.
        // Neither may escape the physical ownership conflict.
        let plan = plan_for_agent(&service, inspector.clone(), saved, AgentCliKind::Codex, 2);
        assert!(plan.targets.iter().all(|target| {
            !target.available
                && target
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("归属冲突"))
        }));
    }
}

struct WorkspaceInspector {
    home: PathBuf,
    workspace: PathBuf,
    settings: AppSettings,
}

impl MutationInspector for WorkspaceInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        crate::services::agent_cli::environment::mutation::native_test_support::catalog_fixture_inventory(
            &self.home, Some(&self.workspace), None, &fixture_definitions(),
        ).map_err(|_| AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed))
    }
    fn home(&self) -> &Path {
        &self.home
    }
    fn workspace(&self) -> Option<&Path> {
        Some(&self.workspace)
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
}

#[test]
fn claude_user_and_local_mcp_adoption_reads_the_exact_native_declaration() {
    let (temporary, original, service) = fixture();
    let workspace = temporary.path().canonicalize().unwrap().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let inspector = Arc::new(WorkspaceInspector {
        home: original.home.clone(),
        workspace: workspace.clone(),
        settings: AppSettings::default(),
    });
    let account_path = inspector.home.join(".claude.json");
    fs::write(&account_path, serde_json::to_vec(&serde_json::json!({
        "mcpServers": { "same": { "command": "user-runner", "env": { "API_KEY": "user-fixture-secret" } } },
        "projects": { workspace.to_string_lossy().into_owned(): {
            "hasTrustDialogAccepted": true,
            "mcpServers": { "same": { "command": "local-runner", "env": { "API_KEY": "local-fixture-secret" } } }
        } }
    })).unwrap()).unwrap();
    let snapshot = inspector.inspect().unwrap();
    let assets = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.agent_kind == AgentCliKind::ClaudeCode
                && asset.category == AgentAssetCategory::Mcp
                && asset.native_id == "same"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        assets.len(),
        2,
        "actual inventory must preserve both User and Local definitions"
    );
    for (scope, key, command, credential) in [
        (
            AgentAssetScope::User,
            "user:same",
            "user-runner",
            "user-fixture-secret",
        ),
        (
            AgentAssetScope::Local,
            "project:same",
            "local-runner",
            "local-fixture-secret",
        ),
    ] {
        let asset = assets.iter().find(|asset| asset.scope == scope).unwrap();
        let context = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| context.id == asset.context_id)
            .unwrap();
        assert_eq!(context.trust_context, AgentTrustState::Trusted);
        let declaration = snapshot
            .inventory
            .declarations
            .iter()
            .find(|declaration| {
                asset.represented_declaration_ids.contains(&declaration.id)
                    && declaration.role == AgentAssetDeclarationRole::Definition
                    && declaration.scope == scope
                    && declaration.declaration_key == key
            })
            .unwrap();
        let source = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| source.id == declaration.source_id)
            .unwrap();
        assert_eq!(Path::new(&source.path), account_path);
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let rows = catalog
            .assets
            .iter()
            .filter(|row| row.category == AgentAssetCategory::Mcp && row.name == "same")
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        let saved = service
            .adopt(
                AgentCatalogAdoptRequest {
                    binding_id: asset.stable_id.clone(),
                    expected_revision: catalog.revision,
                    workspace: Some(workspace.to_string_lossy().into_owned()),
                },
                inspector.clone(),
            )
            .unwrap();
        let DefinitionPayload::Mcp(private) = service
            .repository
            .current(&saved.asset_id)
            .unwrap()
            .1
            .payload
        else {
            panic!()
        };
        assert_eq!(private.command.as_deref(), Some(command));
        assert_eq!(private.environment["API_KEY"], credential);
        let public = serde_json::to_string(&saved).unwrap();
        assert!(public.contains(credential));
    }
    // A missing exact project node never falls back to the same-name User MCP.
    let local = assets
        .iter()
        .find(|asset| asset.scope == AgentAssetScope::Local)
        .unwrap();
    let mut missing_context = inspector.inspect().unwrap();
    missing_context
        .inventory
        .contexts
        .iter_mut()
        .find(|context| context.id == local.context_id)
        .unwrap()
        .workspace_id = Some(workspace.join("absent").to_string_lossy().into_owned());
    assert!(projection::observe_payload(&missing_context, local).is_err());
}
