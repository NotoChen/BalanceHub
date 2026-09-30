use super::{
    tests::{fixture, mcp_request, FixtureInspector},
    *,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Weak},
};

fn planned(
    service: &CatalogService,
    inspector: Arc<dyn MutationInspector>,
    source: AgentCatalogPlanSource,
    agent: AgentCliKind,
) -> AgentCatalogPlan {
    let (catalog, _) = service.read_catalog(&inspector.inspect().unwrap()).unwrap();
    let options = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: source.clone(),
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: Vec::new(),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(options.token.is_none());
    assert!(options.plan_id.is_none());
    let target = options
        .targets
        .iter()
        .find(|target| target.agent_kind == agent && target.available)
        .unwrap_or_else(|| panic!("no available target: {:?}", options.targets));
    service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source,
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: vec![target.target_id.clone()],
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector,
        )
        .unwrap()
}

fn start(service: &CatalogService, plan: &AgentCatalogPlan) -> AgentCatalogOperation {
    service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.clone().expect("concrete plan"),
                asset_id: plan.asset_id.clone(),
                action: plan.action,
            },
        )
        .unwrap()
}

fn skill(home: &Path, text: &str) -> PathBuf {
    let root = home.join(".codex/skills/bh-transfer");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: bh-transfer\ndescription: synthetic catalog flow fixture\n---\n{text}\n"
        ),
    )
    .unwrap();
    fs::create_dir(root.join("resources")).unwrap();
    fs::write(
        root.join("resources/data.txt"),
        "complete synthetic resource",
    )
    .unwrap();
    root
}

#[test]
fn unconfirmed_draft_and_cancel_before_worker_create_no_library_or_native_files() {
    let (temp, inspector, service) = fixture();
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-draft")),
        },
        AgentCliKind::ClaudeCode,
    );
    assert!(!temp.path().join("library").exists());
    assert_eq!(plan.definition_change.as_ref().unwrap().after_version, 1);
    let operation = start(&service, &plan);
    assert_eq!(Some(operation.plan_id.as_str()), plan.plan_id.as_deref());
    assert_eq!(
        operation.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Pending
    );
    assert!(!temp.path().join("library").exists());
    service.cancel("window", &operation.id).unwrap();
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Unchanged
    );
    assert!(completed
        .targets
        .iter()
        .all(|target| target.outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit)));
    assert!(!temp.path().join("library").exists());
    assert!(!inspector.home.join(".claude.json").exists());
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
    assert!(service.operations.lock().unwrap().is_empty());
    assert_eq!(service.operations("window").len(), 1);
}

#[test]
fn confirmed_new_draft_keeps_plan_identity_and_saves_before_native_application() {
    let (temp, inspector, service) = fixture();
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-confirmed")),
        },
        AgentCliKind::ClaudeCode,
    );
    let operation = start(&service, &plan);
    assert!(!temp.path().join("library").exists());
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    let definition = completed.definition_change.as_ref().unwrap();
    assert_eq!(definition.state, AgentCatalogDefinitionChangeState::Saved);
    assert_eq!(definition.version, Some(1));
    assert_eq!(completed.plan_id, plan.plan_id.unwrap());
    assert_eq!(completed.asset_id, plan.asset_id);
    assert_eq!(service.definition(&completed.asset_id).unwrap().version, 1);
    assert!(
        completed
            .targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified)),
        "{completed:?}"
    );
    assert!(service.operations("another-window").is_empty());
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
}

#[test]
fn native_source_two_previews_without_a_library_keep_identity_and_do_not_adopt() {
    let (temp, inspector, service) = fixture();
    skill(&inspector.home, "native-only source");
    let snapshot = inspector.inspect().unwrap();
    let (first, _) = service.read_catalog(&snapshot).unwrap();
    let source = first
        .assets
        .iter()
        .find(|asset| asset.name == "bh-transfer")
        .unwrap();
    let id = source.id.clone();
    let binding_id = source.bindings[0].id.clone();
    let (second, _) = service.read_catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(second.assets[0].id, id);
    let source = AgentCatalogPlanSource::NativeBinding {
        asset_id: id.clone(),
        binding_id,
    };
    let plan = planned(
        &service,
        inspector.clone(),
        source,
        AgentCliKind::ClaudeCode,
    );
    assert_eq!(plan.asset_id, id);
    assert_eq!(
        plan.definition_change.as_ref().unwrap().kind,
        AgentCatalogDefinitionChangeKind::Adopt
    );
    assert!(!temp.path().join("library").exists());
    assert!(!inspector.home.join(".claude/skills/bh-transfer").exists());
    let operation = start(&service, &plan);
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Saved
    );
    let library = service.repository.read_snapshot().unwrap().library;
    assert_eq!(library.entries[&id].versions.len(), 1);
    assert_eq!(
        fs::read_to_string(
            inspector
                .home
                .join(".claude/skills/bh-transfer/resources/data.txt")
        )
        .unwrap(),
        "complete synthetic resource"
    );
    assert!(service.unpersisted_observations.lock().unwrap().is_empty());
}

#[test]
fn changing_a_resource_after_native_preview_does_not_save_or_apply_the_old_package() {
    let (temp, inspector, service) = fixture();
    let source_path = skill(&inspector.home, "initial source");
    let (catalog, _) = service.read_catalog(&inspector.inspect().unwrap()).unwrap();
    let source = &catalog.assets[0];
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::NativeBinding {
            asset_id: source.id.clone(),
            binding_id: source.bindings[0].id.clone(),
        },
        AgentCliKind::ClaudeCode,
    );
    fs::write(source_path.join("resources/data.txt"), "later native edit").unwrap();
    let operation = start(&service, &plan);
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Unchanged
    );
    assert!(completed
        .targets
        .iter()
        .all(|target| target.outcome == Some(AgentAssetOperationOutcome::UnchangedConflict)));
    assert!(!temp.path().join("library").exists());
    assert!(!inspector.home.join(".claude/skills/bh-transfer").exists());
    assert_eq!(
        fs::read_to_string(source_path.join("resources/data.txt")).unwrap(),
        "later native edit"
    );
}

#[test]
fn a_stale_library_rejects_source_save_without_overwriting_newer_content() {
    let (_temp, inspector, service) = fixture();
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-stale")),
        },
        AgentCliKind::ClaudeCode,
    );
    let independent = service.save(mcp_request("bh-independent")).unwrap();
    let before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    let operation = start(&service, &plan);
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Unchanged
    );
    assert_eq!(
        service
            .repository
            .read_snapshot()
            .unwrap()
            .guard
            .bytes()
            .unwrap(),
        before
    );
    assert!(service.definition(&independent.asset_id).is_ok());
    assert!(service.definition(&plan.asset_id).is_err());
    assert!(!inspector.home.join(".claude.json").exists());
}

#[test]
fn a_failure_after_replacement_reports_the_saved_version_and_stops_native_writes() {
    let (_temp, inspector, service) = fixture();
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-readback")),
        },
        AgentCliKind::ClaudeCode,
    );
    service.repository.inject_failure_after_save();
    let operation = start(&service, &plan);
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Saved
    );
    assert_eq!(service.definition(&plan.asset_id).unwrap().version, 1);
    assert!(completed
        .targets
        .iter()
        .all(|target| target.outcome == Some(AgentAssetOperationOutcome::UnchangedConflict)));
    assert!(!inspector.home.join(".claude.json").exists());
}

struct CancelAfterSaveInspector {
    inner: Arc<FixtureInspector>,
    library_path: PathBuf,
    service: Weak<CatalogService>,
    operation_id: Mutex<Option<String>>,
}
impl MutationInspector for CancelAfterSaveInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let snapshot = self.inner.inspect()?;
        if self.library_path.exists() {
            if let Some(id) = self.operation_id.lock().unwrap().take() {
                self.service
                    .upgrade()
                    .unwrap()
                    .cancel("window", &id)
                    .unwrap();
            }
        }
        Ok(snapshot)
    }
    fn home(&self) -> &Path {
        &self.inner.home
    }
    fn workspace(&self) -> Option<&Path> {
        None
    }
    fn settings(&self) -> &AppSettings {
        &self.inner.settings
    }
}

#[test]
fn cancellation_after_definition_commit_retains_it_and_cancels_unwritten_targets() {
    let (temp, inner, service) = fixture();
    let service = Arc::new(service);
    let inspector = Arc::new(CancelAfterSaveInspector {
        inner: inner.clone(),
        library_path: temp.path().join("library/library.json"),
        service: Arc::downgrade(&service),
        operation_id: Mutex::new(None),
    });
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-after-save")),
        },
        AgentCliKind::ClaudeCode,
    );
    let operation = start(&service, &plan);
    *inspector.operation_id.lock().unwrap() = Some(operation.id.clone());
    service.run_operation(&operation.id);
    let completed = service.operation("window", &operation.id).unwrap();
    assert_eq!(
        completed.definition_change.unwrap().state,
        AgentCatalogDefinitionChangeState::Saved
    );
    assert_eq!(service.definition(&plan.asset_id).unwrap().version, 1);
    assert!(
        completed
            .targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit)),
        "{:?}",
        completed.targets
    );
    assert!(!inner.home.join(".claude.json").exists());
}

#[test]
fn cache_budget_actor_cleanup_and_all_action_choices_keep_the_read_boundary() {
    let (temp, inspector, service) = fixture();
    let full = planning::reserve(&service.preview_bytes, 64 * 1024 * 1024).unwrap();
    assert!(planning::reserve(&service.preview_bytes, 1).is_err());
    drop(full);
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::Draft {
            definition: Box::new(mcp_request("bh-budget")),
        },
        AgentCliKind::ClaudeCode,
    );
    assert!(service.preview_bytes.load(Ordering::Acquire) > 0);
    service.remove_actor("window");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
    assert!(service
        .start(
            "window",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: plan.asset_id,
                action: plan.action
            }
        )
        .is_err());
    assert!(!temp.path().join("library").exists());
    skill(&inspector.home, "choices without writes");
    let (catalog, _) = service.read_catalog(&inspector.inspect().unwrap()).unwrap();
    let item = &catalog.assets[0];
    for action in [
        AgentCatalogAction::Enable,
        AgentCatalogAction::Disable,
        AgentCatalogAction::RemoveBinding,
    ] {
        let options = service
            .plan(
                "window",
                AgentCatalogPlanRequest {
                    source: AgentCatalogPlanSource::Catalog {
                        asset_id: item.id.clone(),
                        expected_version: None,
                    },
                    action,
                    target_ids: Vec::new(),
                    expected_revision: catalog.revision.clone(),
                    workspace: None,
                },
                inspector.clone(),
            )
            .unwrap();
        assert!(options.token.is_none());
        assert!(options.plan_id.is_none());
        assert_eq!(options.targets.len(), item.bindings.len());
        assert!(options
            .targets
            .iter()
            .all(|target| target.target_kind == AgentCatalogTargetKind::Binding));
    }
    assert!(!temp.path().join("library").exists());
}

#[test]
fn small_definition_previews_charge_large_target_copies_and_release_on_window_cleanup() {
    let (_temp, inspector, service) = fixture();
    let native_path = inspector.home.join(".claude.json");
    let native_before = serde_json::to_vec(&serde_json::json!({
        "mcpServers": {},
        "unrelated": "x".repeat(480 * 1024),
    }))
    .unwrap();
    fs::write(&native_path, &native_before).unwrap();
    let saved = service.save(mcp_request("bh-large-target")).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = AgentCatalogPlanSource::Catalog {
        asset_id: saved.asset_id,
        expected_version: Some(saved.version),
    };
    let options = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: source.clone(),
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: Vec::new(),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    let target = options
        .targets
        .iter()
        .find(|target| target.agent_kind == AgentCliKind::ClaudeCode && target.available)
        .unwrap_or_else(|| panic!("large native target unavailable: {:?}", options.targets));
    let request = AgentCatalogPlanRequest {
        source,
        action: AgentCatalogAction::ApplyDefinition,
        target_ids: vec![target.target_id.clone()],
        expected_revision: catalog.revision,
        workspace: None,
    };
    let library_before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
    let mut issued = 0;
    let mut rejected = None;
    for _ in 0..128 {
        let before = service.preview_bytes.load(Ordering::Acquire);
        match service.plan("window", request.clone(), inspector.clone()) {
            Ok(plan) => {
                assert!(plan.token.is_some());
                issued += 1;
                // The original GuardedFile and its replacement are separate
                // retained buffers, even though the definition itself is tiny.
                assert!(
                    service.preview_bytes.load(Ordering::Acquire)
                        >= before + native_before.len() * 2
                );
            }
            Err(error) => {
                assert_eq!(service.preview_bytes.load(Ordering::Acquire), before);
                rejected = Some(error);
                break;
            }
        }
        assert!(service.preview_bytes.load(Ordering::Acquire) <= 64 * 1024 * 1024);
    }
    let error = rejected.expect("target buffers must reach the byte cap before 128 plans");
    assert!(error.contains("64 MiB"), "{error}");
    assert!(issued > 1 && issued < 128);
    assert_eq!(fs::read(&native_path).unwrap(), native_before);
    assert_eq!(
        service
            .repository
            .read_snapshot()
            .unwrap()
            .guard
            .bytes()
            .unwrap(),
        library_before
    );
    service.remove_actor("window");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
    assert!(service
        .plan("window", request, inspector)
        .unwrap()
        .token
        .is_some());
    service.remove_actor("window");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
}

#[test]
fn hook_receipt_targets_are_unique_in_catalog_choices_and_agent_panel() {
    use super::hook_tests;

    let (_temp, inspector, service) = fixture();
    let native_path = inspector.home.join(".claude/settings.json");
    hook_tests::write_json(
        &native_path,
        &serde_json::json!({"hooks": {"PreToolUse": [{
            "matcher": "Bash", "hooks": [{
                "type": "command", "command": "printf bh-target-identity"
            }]
        }]}}),
    );
    let (_, binding) = hook_tests::binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf bh-target-identity",
    );
    let saved = hook_tests::adopt(&service, inspector.clone(), &binding.id);
    let saved = hook_tests::update_command(
        &service,
        &saved,
        AgentCliKind::ClaudeCode,
        "printf bh-target-updated",
    );
    let target_id =
        hook_tests::primary_target(&service, inspector.as_ref(), AgentCliKind::ClaudeCode);
    hook_tests::assert_verified(&hook_tests::run(
        &service,
        hook_tests::plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target_id.clone()],
        ),
    ));
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    assert_eq!(
        catalog
            .targets
            .iter()
            .map(|target| &target.id)
            .collect::<BTreeSet<_>>()
            .len(),
        catalog.targets.len()
    );
    let native_before = fs::read(&native_path).unwrap();
    let options = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: saved.asset_id.clone(),
                    expected_version: Some(saved.version),
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: Vec::new(),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert_eq!(
        options
            .targets
            .iter()
            .map(|target| &target.target_id)
            .collect::<BTreeSet<_>>()
            .len(),
        options.targets.len()
    );
    assert_eq!(
        options
            .targets
            .iter()
            .filter(|target| target.target_id == target_id)
            .count(),
        1
    );
    let panel = service
        .agent_panel(
            AgentCatalogAgentPanelRequest {
                asset_id: saved.asset_id.clone(),
                agent_kind: Some(AgentCliKind::ClaudeCode),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector,
            &catalog,
            &snapshot,
            &service
                .begin_read("window", "panel", "panel-fixture")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        panel
            .entries
            .iter()
            .map(|entry| &entry.target_id)
            .collect::<BTreeSet<_>>()
            .len(),
        panel.entries.len()
    );
    assert_eq!(panel.entries.len(), 1);
    assert_eq!(
        panel.entries[0].target_kind,
        AgentCatalogTargetKind::Binding
    );
    assert!(!panel.entries[0]
        .actions
        .iter()
        .any(|action| action.action == AgentCatalogAction::ApplyDefinition));
    assert_eq!(fs::read(&native_path).unwrap(), native_before);

    // A pending receipt must win over a duplicate available destination or
    // receipt, regardless of entry traversal order.
    let library = service.repository.read_snapshot().unwrap().library;
    let receipts = &library.entries[&saved.asset_id].receipts;
    assert_eq!(receipts.len(), 1);
    let (receipt_id, receipt) = receipts.first_key_value().unwrap();
    assert_ne!(
        receipt_id, &target_id,
        "native destinations and recovery receipts have distinct IDs"
    );
    assert_eq!(
        options
            .targets
            .iter()
            .filter(|target| &target.target_id == receipt_id)
            .count(),
        1
    );
    for pending_first in [true, false] {
        let mut library = repository::Library::default();
        for (index, pending) in [pending_first, !pending_first].into_iter().enumerate() {
            let mut receipt = receipt.clone();
            if pending {
                receipt.hook.as_mut().unwrap().pending = Some(hook_receipts::HookIntent {
                    id: "bh-pending-target".to_owned(),
                    before: None,
                    after: None,
                    files: Vec::new(),
                });
            }
            let mut entry =
                repository::Entry::new("bh-target".to_owned(), AgentAssetCategory::Hook);
            entry.receipts.insert(receipt_id.clone(), receipt);
            library.entries.insert(index.to_string(), entry);
        }
        // Start with real catalog rows, including the original receipt target,
        // then offer its pending and active records again using that exact ID.
        let mut targets = catalog.targets.clone();
        hook_targets::append_receipt_targets(&mut targets, &library, &snapshot);
        let matches = targets
            .iter()
            .filter(|target| &target.id == receipt_id)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        assert!(!matches[0].available);
        assert!(matches[0].reason.as_deref().unwrap().contains("对账"));
    }
}

#[test]
fn agent_panel_preserves_native_unknown_diagnostics_when_the_complete_package_is_readable() {
    let (_temp, inspector, service) = fixture();
    let directory = inspector.home.join(".claude/skills/bh-header-budget");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("SKILL.md"), format!("---\nname: bh-header-budget\ndescription: valid complete header\n{}---\nReadable full package\n", "# bounded native header fixture\n".repeat(70))).unwrap();
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.name == "bh-header-budget")
        .unwrap();
    let binding = &item.bindings[0];
    assert!(binding.reason.is_none());
    assert_eq!(binding.native.effective_state, AgentAssetState::Unknown);
    let source = projection::definition_source(&snapshot, &binding.native).unwrap();
    assert!(source.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::FrontmatterLines,
            ..
        }
    )));
    let panel = service
        .agent_panel(
            AgentCatalogAgentPanelRequest {
                asset_id: item.id.clone(),
                agent_kind: Some(AgentCliKind::ClaudeCode),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector,
            &catalog,
            &snapshot,
            &service
                .begin_read("window", "panel", "panel-fixture")
                .unwrap(),
        )
        .unwrap();
    let row = panel
        .entries
        .iter()
        .find(|entry| entry.target_id == binding.id)
        .unwrap();
    assert_eq!(row.state_label, "状态待核对");
    assert_eq!(row.diagnostics, source.diagnostics);
    assert!(binding
        .native
        .diagnostics
        .iter()
        .all(|diagnostic| row.diagnostics.contains(diagnostic)));
    assert!(row
        .actions
        .iter()
        .filter(|action| action.action == AgentCatalogAction::Enable)
        .all(|action| !action.available));
}

#[test]
fn installed_skill_panel_combines_updates_with_the_exact_binding_and_explains_removal() {
    let (_temp, inspector, service) = fixture();
    skill(&inspector.home, "synthetic cross-Agent panel source");
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.name == "bh-transfer")
        .unwrap();
    let plan = planned(
        &service,
        inspector.clone(),
        AgentCatalogPlanSource::NativeBinding {
            asset_id: item.id.clone(),
            binding_id: item.bindings[0].id.clone(),
        },
        AgentCliKind::ClaudeCode,
    );
    let destination_id = plan.targets[0].target_id.clone();
    let operation = start(&service, &plan);
    service.run_operation(&operation.id);
    assert!(service
        .operation("window", &operation.id)
        .unwrap()
        .targets
        .iter()
        .all(|target| { target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified) }));
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.id == plan.asset_id)
        .unwrap();
    let binding = item
        .bindings
        .iter()
        .find(|binding| binding.native.agent_kind == AgentCliKind::ClaudeCode)
        .unwrap();
    let native_path = inspector.home.join(".claude/skills/bh-transfer/SKILL.md");
    let native_before = fs::read(&native_path).unwrap();
    let library_before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    let panel = service
        .agent_panel(
            AgentCatalogAgentPanelRequest {
                asset_id: item.id.clone(),
                agent_kind: Some(AgentCliKind::ClaudeCode),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector.clone(),
            &catalog,
            &snapshot,
            &service
                .begin_read("window", "panel", "panel-fixture")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(panel.entries.len(), 1, "{:?}", panel.entries);
    let row = &panel.entries[0];
    assert_eq!(row.target_id, binding.id);
    assert_eq!(row.target_kind, AgentCatalogTargetKind::Binding);
    assert_eq!(row.state_label, "已启用");
    assert!(
        !row.actions
            .iter()
            .any(|action| action.action == AgentCatalogAction::ApplyDefinition),
        "identical content needs no update action"
    );
    let remove = row
        .actions
        .iter()
        .find(|action| action.action == AgentCatalogAction::RemoveBinding)
        .unwrap();
    assert!(remove.available, "{:?}", remove.reason);
    assert_eq!(remove.target_ids, vec![binding.id.clone()]);
    let update_plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: item.id.clone(),
                    expected_version: item.version,
                },
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: vec![destination_id],
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(update_plan.token.is_some());
    let remove_plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: item.id.clone(),
                    expected_version: item.version,
                },
                action: AgentCatalogAction::RemoveBinding,
                target_ids: remove.target_ids.clone(),
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector,
        )
        .unwrap();
    assert!(remove_plan.token.is_some());
    assert_eq!(remove_plan.targets[0].reason, remove.reason);
    assert_eq!(fs::read(native_path).unwrap(), native_before);
    assert_eq!(
        service
            .repository
            .read_snapshot()
            .unwrap()
            .guard
            .bytes()
            .unwrap(),
        library_before
    );
}

#[test]
fn skill_panel_keeps_a_distinct_physical_destination_in_the_same_scope() {
    let (_temp, inspector, service) = fixture();
    skill(&inspector.home, "local user directory");
    let shared_root = inspector.home.join(".agents/skills");
    fs::create_dir_all(&shared_root).unwrap();
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|asset| asset.name == "bh-transfer")
        .unwrap();
    let binding = item
        .bindings
        .iter()
        .find(|binding| binding.native.agent_kind == AgentCliKind::Codex)
        .unwrap();
    let local_root = inspector.home.join(".codex/skills");
    let target_at = |path: &Path| {
        catalog
            .targets
            .iter()
            .find(|target| {
                target.agent_kind == AgentCliKind::Codex
                    && target.categories == [AgentAssetCategory::Skill]
                    && projection::resolve_target(&snapshot, &target.id)
                        .is_ok_and(|(_, source, _)| Path::new(&source.path) == path)
            })
            .unwrap()
            .id
            .clone()
    };
    let local_target = target_at(&local_root);
    let shared_target = target_at(&shared_root);
    let panel = service
        .agent_panel(
            AgentCatalogAgentPanelRequest {
                asset_id: item.id.clone(),
                agent_kind: Some(AgentCliKind::Codex),
                expected_revision: catalog.revision.clone(),
                workspace: None,
            },
            inspector,
            &catalog,
            &snapshot,
            &service
                .begin_read("window", "panel", "panel-fixture")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(panel.entries.len(), 2, "{:?}", panel.entries);
    let existing = panel
        .entries
        .iter()
        .find(|entry| entry.target_id == binding.id)
        .unwrap();
    assert_eq!(existing.target_kind, AgentCatalogTargetKind::Binding);
    assert!(!existing.actions.iter().any(|action| action.action
        == AgentCatalogAction::ApplyDefinition
        && action.target_ids.contains(&local_target)));
    let destination = panel
        .entries
        .iter()
        .find(|entry| entry.target_id == shared_target)
        .unwrap();
    assert_eq!(destination.target_kind, AgentCatalogTargetKind::Destination);
    assert_eq!(destination.context_id, existing.context_id);
    assert_eq!(destination.scope, existing.scope);
    assert_eq!(
        destination.path.as_deref().map(Path::new),
        Some(shared_root.as_path())
    );
    assert_eq!(destination.actions[0].target_ids, vec![shared_target]);
    assert!(
        destination.actions[0].available,
        "{:?}",
        destination.actions[0].reason
    );
    assert!(fs::read_dir(shared_root).unwrap().next().is_none());
}
