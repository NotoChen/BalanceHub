use super::{
    tests::{fixture, mcp_request},
    *,
};
use std::fs;

fn apply_request(plan: AgentCatalogPlan) -> AgentCatalogApplyRequest {
    AgentCatalogApplyRequest {
        plan_token: plan.token.unwrap(),
        asset_id: plan.asset_id,
        action: plan.action,
    }
}

fn distribution_plan(
    service: &CatalogService,
    inspector: Arc<dyn MutationInspector>,
    id: &str,
    category: AgentAssetCategory,
) -> AgentCatalogPlan {
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let version = catalog
        .assets
        .iter()
        .find(|asset| asset.id == id)
        .unwrap()
        .version;
    let targets = catalog
        .targets
        .iter()
        .filter(|target| target.categories.contains(&category))
        .map(|target| target.id.clone())
        .collect();
    service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: id.to_owned(),
                    expected_version: version,
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: targets,
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector,
        )
        .unwrap()
}

#[test]
fn canceled_catalog_batch_leaves_all_native_targets_untouched_and_rejects_wrong_actor() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("cancel-batch")).unwrap();
    let plan = distribution_plan(&service, inspector.clone(), &saved.asset_id, saved.category);
    assert_eq!(plan.targets.len(), 4);
    assert!(plan.targets.iter().all(|target| target.available));
    let request = apply_request(plan);
    assert!(service.start("other-window", &request).is_err());
    let operation = service.start("window", &request).unwrap();
    assert!(service.cancel("other-window", &operation.id).is_err());
    assert!(!service.cancel("window", &operation.id).unwrap().can_cancel);
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert_eq!(done.phase, AgentAssetOperationPhase::Completed);
    assert!(done
        .targets
        .iter()
        .all(|target| target.outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit)));
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert!(catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap()
        .bindings
        .is_empty());
    assert!(!inspector.home.join(".codex/config.toml").exists());
    assert!(!inspector.home.join(".grok/config.toml").exists());
    assert!(!inspector.home.join(".gemini/settings.json").exists());
}

#[test]
fn catalog_partial_conflict_preserves_other_outcomes_and_tracks_missing_unknown_receipts() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("partial-batch")).unwrap();
    let plan = distribution_plan(&service, inspector.clone(), &saved.asset_id, saved.category);
    let codex = plan
        .targets
        .iter()
        .find(|target| target.agent_kind == AgentCliKind::Codex)
        .unwrap()
        .target_id
        .clone();
    let external = "# External writer wins\n[mcp_servers.external]\ncommand = 'keep-external'\n";
    fs::write(inspector.home.join(".codex/config.toml"), external).unwrap();
    let operation = service.start("window", &apply_request(plan)).unwrap();
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert_eq!(done.targets.len(), 4);
    for target in &done.targets {
        assert_eq!(
            target.outcome,
            Some(if target.target_id == codex {
                AgentAssetOperationOutcome::UnchangedConflict
            } else {
                AgentAssetOperationOutcome::AppliedVerified
            }),
            "{:?}",
            done.targets
        );
    }
    assert_eq!(
        fs::read_to_string(inspector.home.join(".codex/config.toml")).unwrap(),
        external
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let applied = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(applied.bindings.len(), 3);
    assert!(applied
        .bindings
        .iter()
        .all(|binding| binding.drift == AgentCatalogDrift::InSync));

    fs::write(inspector.home.join(".gemini/settings.json"), "{}\n").unwrap();
    fs::write(inspector.home.join(".grok/config.toml"), "invalid = [\n").unwrap();
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(asset.bindings.len(), 1);
    assert_eq!(asset.unresolved_targets.len(), 2);
    assert!(asset
        .unresolved_targets
        .iter()
        .any(|target| target.agent_kind == AgentCliKind::Gemini
            && target.drift == AgentCatalogDrift::Missing));
    assert!(asset
        .unresolved_targets
        .iter()
        .any(|target| target.agent_kind == AgentCliKind::Grok
            && target.drift == AgentCatalogDrift::Unknown));
    assert!(asset
        .unresolved_targets
        .iter()
        .all(|target| target.applied_version == 1 && target.scope == AgentAssetScope::User));

    // Leaving a scanned context does not erase its durable receipt or invent a
    // currently installed native row. The stored context/scope remains visible.
    let mut absent = inspector.inspect().unwrap();
    absent
        .inventory
        .contexts
        .retain(|context| context.agent_kind != AgentCliKind::Gemini);
    let catalog = service.catalog(&absent).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert!(asset
        .unresolved_targets
        .iter()
        .any(|target| target.agent_kind == AgentCliKind::Gemini
            && target.drift == AgentCatalogDrift::Unknown));
}

#[test]
fn missing_skill_manifest_is_reported_without_fabricating_an_installed_binding() {
    let (_temporary, inspector, service) = fixture();
    let saved = service
        .save(AgentCatalogSaveRequest {
            hook: None,
            asset_id: None,
            expected_version: None,
            name: "shared-skill".into(),
            category: AgentAssetCategory::Skill,
            mcp: None,
            skill_markdown: Some(
                "---\nname: shared-skill\ndescription: Test complete package\n---\nRead sources\n"
                    .into(),
            ),
        })
        .unwrap();
    let plan = distribution_plan(&service, inspector.clone(), &saved.asset_id, saved.category);
    assert_eq!(plan.targets.len(), 4);
    assert!(plan.targets.iter().all(|target| target.available));
    let operation = service.start("window", &apply_request(plan)).unwrap();
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert!(
        done.targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified)),
        "{:?}",
        done.targets
    );
    fs::remove_file(inspector.home.join(".codex/skills/shared-skill/SKILL.md")).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    assert_eq!(asset.bindings.len(), 3);
    assert!(asset
        .unresolved_targets
        .iter()
        .any(|target| target.agent_kind == AgentCliKind::Codex
            && target.drift == AgentCatalogDrift::Missing));
}

#[test]
fn library_version_change_after_plan_rejects_every_uncommitted_target() {
    let (_temporary, inspector, service) = fixture();
    let saved = service.save(mcp_request("version-race")).unwrap();
    let plan = distribution_plan(&service, inspector.clone(), &saved.asset_id, saved.category);
    let mut updated = mcp_request("version-race");
    updated.asset_id = Some(saved.asset_id.clone());
    updated.expected_version = Some(1);
    updated.mcp.as_mut().unwrap().args.push("--quiet".into());
    service.save(updated).unwrap();
    let operation = service.start("window", &apply_request(plan)).unwrap();
    service.run_operation(&operation.id);
    let done = service.operation("window", &operation.id).unwrap();
    assert!(done
        .targets
        .iter()
        .all(|target| target.outcome == Some(AgentAssetOperationOutcome::UnchangedConflict)));
    assert!(!inspector.home.join(".codex/config.toml").exists());
}
