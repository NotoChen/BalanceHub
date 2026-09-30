use super::{
    tests::{fixture, FixtureInspector},
    *,
};
use crate::services::agent_cli;
use serde_json::{json, Value};
use std::{fs, path::Path};

pub(super) fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn claude_hooks(inspector: &FixtureInspector) -> std::path::PathBuf {
    let path = inspector.home.join(".claude/settings.json");
    write_json(
        &path,
        &json!({"unrelated":{"keep":true},"hooks":{"PreToolUse":[{
            "matcher":"Bash", "hooks":[
                {"type":"command","command":"printf selected-user-hook"},
                {"type":"command","command":"printf keep-neighbor"},
                {"type":"command","command":"printf other-user-hook"}
            ]
        }]}}),
    );
    path
}

pub(super) fn binding_for(
    service: &CatalogService,
    inspector: &dyn MutationInspector,
    kind: AgentCliKind,
    command: &str,
) -> (AgentCatalogAsset, AgentCatalogBinding) {
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let adapter = agent_cli::definition(kind)
        .environment
        .hook_adapter()
        .unwrap();
    for item in &catalog.assets {
        for binding in &item.bindings {
            if binding.native.agent_kind != kind
                || binding.native.category != AgentAssetCategory::Hook
            {
                continue;
            }
            if (adapter.read_hook)(&snapshot, &binding.native)
                .is_ok_and(|rule| rule.definition.group["hooks"][0]["command"] == command)
            {
                return (item.clone(), binding.clone());
            }
        }
    }
    panic!("Expected native fixture Hook is absent: {kind:?} {command}");
}

pub(super) fn plan(
    service: &CatalogService,
    inspector: Arc<dyn MutationInspector>,
    id: &str,
    action: AgentCatalogAction,
    target_ids: Vec<String>,
) -> AgentCatalogPlan {
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog.assets.iter().find(|item| item.id == id).unwrap();
    service
        .plan(
            "fixture",
            AgentCatalogPlanRequest {
                source: AgentCatalogPlanSource::Catalog {
                    asset_id: id.to_owned(),
                    expected_version: item.version,
                },
                action,
                target_ids,
                expected_revision: catalog.revision,
                workspace: inspector
                    .workspace()
                    .map(|path| path.to_string_lossy().into_owned()),
            },
            inspector,
        )
        .unwrap()
}

pub(super) fn run(service: &CatalogService, plan: AgentCatalogPlan) -> AgentCatalogOperation {
    assert!(
        plan.targets.iter().all(|target| target.available),
        "{:?}",
        plan.targets
    );
    let operation = service
        .start(
            "fixture",
            &AgentCatalogApplyRequest {
                plan_token: plan.token.unwrap(),
                asset_id: plan.asset_id,
                action: plan.action,
            },
        )
        .unwrap();
    service.run_operation(&operation.id);
    service.operation("fixture", &operation.id).unwrap()
}

pub(super) fn assert_verified(operation: &AgentCatalogOperation) {
    assert_eq!(operation.phase, AgentAssetOperationPhase::Completed);
    assert!(
        operation
            .targets
            .iter()
            .all(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedVerified)),
        "{:?}",
        operation.targets
    );
}

pub(super) fn primary_target(
    service: &CatalogService,
    inspector: &dyn MutationInspector,
    kind: AgentCliKind,
) -> String {
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    catalog
        .targets
        .iter()
        .find(|target| {
            target.agent_kind == kind
                && target.scope == AgentAssetScope::User
                && target.categories == [AgentAssetCategory::Hook]
                && hook_targets::resolve(&snapshot, &target.id, None).is_ok_and(|resolved| {
                    resolved.binding.is_none()
                        && if matches!(kind, AgentCliKind::Codex | AgentCliKind::Grok) {
                            resolved.source.path.ends_with("hooks.json")
                        } else {
                            true
                        }
                })
        })
        .expect("A primary native Hook destination must exist")
        .id
        .clone()
}

pub(super) fn adopt(
    service: &CatalogService,
    inspector: Arc<dyn MutationInspector>,
    binding_id: &str,
) -> AgentCatalogDefinition {
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    service
        .adopt(
            AgentCatalogAdoptRequest {
                binding_id: binding_id.to_owned(),
                expected_revision: catalog.revision,
                workspace: inspector
                    .workspace()
                    .map(|path| path.to_string_lossy().into_owned()),
            },
            inspector,
        )
        .unwrap()
}

pub(super) fn edit_request(saved: &AgentCatalogDefinition) -> AgentCatalogSaveRequest {
    AgentCatalogSaveRequest {
        asset_id: Some(saved.asset_id.clone()),
        expected_version: Some(saved.version),
        name: saved.name.clone(),
        category: saved.category,
        mcp: saved.mcp.clone(),
        hook: saved.hook.clone(),
        skill_markdown: saved.skill_markdown.clone(),
    }
}

pub(super) fn update_command(
    service: &CatalogService,
    saved: &AgentCatalogDefinition,
    kind: AgentCliKind,
    command: &str,
) -> AgentCatalogDefinition {
    let mut request = edit_request(saved);
    let variant = request
        .hook
        .as_mut()
        .unwrap()
        .variants
        .iter_mut()
        .find(|variant| variant.agent_kind == kind)
        .unwrap();
    let mut group: Value = serde_json::from_str(&variant.group_json).unwrap();
    group["hooks"][0]["command"] = json!(command);
    variant.group_json = serde_json::to_string(&group).unwrap();
    service.save(request).unwrap()
}

#[test]
fn arbitrary_user_hook_adopts_updates_and_removes_only_selected_native_handler() {
    let (_temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let before: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let (_, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert!(binding
        .actions
        .iter()
        .any(|action| action.action == AgentAssetActionKind::Disable && action.available));
    assert!(binding
        .actions
        .iter()
        .any(|action| action.action == AgentAssetActionKind::Remove && action.available));
    let saved = adopt(&service, inspector.clone(), &binding.id);
    let saved = update_command(
        &service,
        &saved,
        AgentCliKind::ClaudeCode,
        "printf edited-user-hook",
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        serde_json::to_vec_pretty(&before).unwrap(),
        "library save must not mutate native configuration"
    );
    let target = primary_target(&service, inspector.as_ref(), AgentCliKind::ClaudeCode);
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target],
        ),
    ));
    let (item, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf edited-user-hook",
    );
    assert_eq!(item.id, saved.asset_id);
    assert_eq!(binding.drift, AgentCatalogDrift::InSync);
    let after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(after["unrelated"], before["unrelated"]);
    assert_eq!(
        after["hooks"]["PreToolUse"][0]["hooks"][1],
        before["hooks"]["PreToolUse"][0]["hooks"][1]
    );
    assert_eq!(
        after["hooks"]["PreToolUse"][0]["hooks"][2],
        before["hooks"]["PreToolUse"][0]["hooks"][2]
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::RemoveBinding,
            vec![binding.id],
        ),
    ));
    let final_value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        final_value["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(service.definition(&saved.asset_id).unwrap().version, 2);
}

#[test]
fn suspended_hook_survives_restart_and_version_update_without_reactivation() {
    let (temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let (_, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let saved = adopt(&service, inspector.clone(), &binding.id);
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::Disable,
            vec![binding.id],
        ),
    ));
    let paused = fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&paused).contains("printf selected-user-hook"));
    drop(service);
    let service = CatalogService::new(
        temporary.path().canonicalize().unwrap().join("library"),
        Arc::new(MutationService::default()),
    );

    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap();
    assert!(item.bindings.is_empty());
    assert_eq!(item.unresolved_targets.len(), 1);
    let suspended = &item.unresolved_targets[0];
    assert_eq!(suspended.state, AgentCatalogUnresolvedState::Suspended);
    let receipt_id = suspended.target_id.clone();
    assert!(catalog
        .targets
        .iter()
        .any(|target| target.id == receipt_id && target.available));
    let saved = update_command(
        &service,
        &saved,
        AgentCliKind::ClaudeCode,
        "printf paused-version-two",
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![receipt_id.clone()],
        ),
    ));
    assert_eq!(
        fs::read(&path).unwrap(),
        paused,
        "applying a new retained version must keep the native Hook absent"
    );
    let mut outside: Value = serde_json::from_slice(&paused).unwrap();
    outside["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({"type":"command","command":"printf inserted-neighbor"}),
        );
    write_json(&path, &outside);
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::Enable,
            vec![receipt_id],
        ),
    ));
    let (item, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf paused-version-two",
    );
    assert_eq!(item.id, saved.asset_id);
    assert_eq!(binding.applied_version, Some(2));
    assert!(item.unresolved_targets.is_empty());
    let final_value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let commands = final_value["hooks"]["PreToolUse"][0]["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hook| hook["command"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(commands.len(), 4);
    assert!(
        commands.contains(&"printf inserted-neighbor")
            && commands.contains(&"printf keep-neighbor")
            && commands.contains(&"printf other-user-hook")
    );
}

#[test]
fn hook_receipt_reidentifies_reordered_content_and_refuses_ambiguous_old_slots() {
    let (_temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let (_, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let old_id = binding.id.clone();
    let saved = adopt(&service, inspector.clone(), &binding.id);
    let target = primary_target(&service, inspector.as_ref(), AgentCliKind::ClaudeCode);
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target],
        ),
    ));
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .rotate_left(1);
    write_json(&path, &value);
    let (same, moved) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert_eq!(same.id, saved.asset_id);
    assert_ne!(moved.id, old_id);
    let (neighbor, _) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf keep-neighbor",
    );
    assert_ne!(neighbor.id, saved.asset_id);
    let duplicate = value["hooks"]["PreToolUse"][0]["hooks"][2].clone();
    value["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    write_json(&path, &value);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let managed = catalog
        .assets
        .iter()
        .find(|item| item.id == saved.asset_id)
        .unwrap();
    assert!(managed.bindings.is_empty());
    assert_eq!(
        managed.unresolved_targets[0].state,
        AgentCatalogUnresolvedState::Unknown
    );
    let rejected = plan(
        &service,
        inspector.clone(),
        &saved.asset_id,
        AgentCatalogAction::ApplyDefinition,
        vec![managed.unresolved_targets[0].target_id.clone()],
    );
    assert!(!rejected.targets[0].available);
    assert!(rejected.token.is_none() && rejected.plan_id.is_none());
    assert_eq!(
        fs::read(path).unwrap(),
        serde_json::to_vec_pretty(&value).unwrap()
    );
}

fn pending(bytes: &[u8]) -> bool {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    value["entries"].as_object().unwrap().values().any(|entry| {
        entry["receipts"]
            .as_object()
            .unwrap()
            .values()
            .any(|receipt| receipt["hook"]["pending"].is_object())
    })
}

#[test]
fn intent_persistence_failure_never_removes_native_hook_and_final_failure_reconciles_after_restart()
{
    for fail_intent in [true, false] {
        let (temporary, inspector, service) = fixture();
        let path = claude_hooks(&inspector);
        let before = fs::read(&path).unwrap();
        let (_, binding) = binding_for(
            &service,
            inspector.as_ref(),
            AgentCliKind::ClaudeCode,
            "printf selected-user-hook",
        );
        let saved = adopt(&service, inspector.clone(), &binding.id);
        let confirmation = plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::Disable,
            vec![binding.id],
        );
        if fail_intent {
            service
                .repository
                .inject_persistence_fault(|before, after| !pending(before) && pending(after));
        } else {
            service
                .repository
                .inject_persistence_fault(|before, after| pending(before) && !pending(after));
        }
        let done = run(&service, confirmation);
        if fail_intent {
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(!pending(
                &fs::read(temporary.path().join("library/library.json")).unwrap()
            ));
            assert_eq!(
                done.targets[0].outcome,
                Some(AgentAssetOperationOutcome::UnchangedConflict)
            );
        } else {
            assert_eq!(
                done.targets[0].outcome,
                Some(AgentAssetOperationOutcome::AppliedUnverified)
            );
            assert!(pending(
                &fs::read(temporary.path().join("library/library.json")).unwrap()
            ));
            let after = fs::read(&path).unwrap();
            drop(service);
            let restarted = CatalogService::new(
                temporary.path().canonicalize().unwrap().join("library"),
                Arc::new(MutationService::default()),
            );
            let catalog = restarted.catalog(&inspector.inspect().unwrap()).unwrap();
            let item = catalog
                .assets
                .iter()
                .find(|item| item.id == saved.asset_id)
                .unwrap();
            assert!(item.bindings.is_empty());
            assert_eq!(
                item.unresolved_targets[0].state,
                AgentCatalogUnresolvedState::Suspended
            );
            assert_eq!(
                fs::read(&path).unwrap(),
                after,
                "reconciliation must never replay a native write"
            );
        }
    }
}

#[test]
fn hook_display_names_round_trip_and_invalid_edits_leave_the_library_unchanged() {
    let (temporary, _, service) = fixture();
    let mut saved_definitions = Vec::new();
    for name in [
        "工具前检查  · PreToolUse / Bash".to_owned(),
        "x".repeat(128),
        "钩".repeat(42),
        format!("{}ab", "钩".repeat(42)),
    ] {
        let saved = service
            .save(AgentCatalogSaveRequest {
                asset_id: None,
                expected_version: None,
                name: name.clone(),
                category: AgentAssetCategory::Hook,
                mcp: None,
                skill_markdown: None,
                hook: Some(AgentCatalogHookInput {
                    variants: vec![AgentCatalogHookVariantInput {
                        agent_kind: AgentCliKind::ClaudeCode,
                        event: "PreToolUse".to_owned(),
                        group_json: json!({"matcher":"Bash","hooks":[{"type":"command","command":"printf name-fixture"}]}).to_string(),
                    }],
                }),
            })
            .unwrap();
        assert_eq!(saved.name, name);
        let round_trip = service.save(edit_request(&saved)).unwrap();
        assert_eq!(round_trip.name, name);
        assert_eq!(round_trip.version, saved.version);
        saved_definitions.push(round_trip);
    }
    let library = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("library/library.json");
    let before = fs::read(&library).unwrap();
    for name in [
        "".to_owned(),
        " ".to_owned(),
        " leading".to_owned(),
        "trailing ".to_owned(),
        "\u{a0}leading".to_owned(),
        "trailing\u{3000}".to_owned(),
        "line\nbreak".to_owned(),
        "tab\tname".to_owned(),
        "null\0name".to_owned(),
        "control\u{7f}name".to_owned(),
        "x".repeat(129),
        "钩".repeat(43),
    ] {
        let mut request = edit_request(&saved_definitions[0]);
        request.name = name;
        let error = service.save(request).unwrap_err();
        assert!(error.contains("Hook 名称"), "{error}");
    }
    assert_eq!(fs::read(&library).unwrap(), before);
    drop(service);
    let restarted = CatalogService::new(
        library.parent().unwrap().to_path_buf(),
        Arc::new(MutationService::default()),
    );
    for saved in saved_definitions {
        let loaded = restarted.definition(&saved.asset_id).unwrap();
        assert_eq!(loaded.name, saved.name);
        assert_eq!(loaded.version, saved.version);
    }
}

#[test]
fn skill_and_mcp_names_keep_native_identifier_restrictions() {
    let (temporary, _, service) = fixture();
    for category in [AgentAssetCategory::Skill, AgentAssetCategory::Mcp] {
        let request = |name: &str| {
            let mut request = super::tests::mcp_request(name);
            if category == AgentAssetCategory::Skill {
                request.category = category;
                request.mcp = None;
                request.skill_markdown = Some(
                    "---\nname: native.tool-v1_2\ndescription: Name validation fixture\n---\nRead fixture sources.\n".to_owned(),
                );
            }
            request
        };
        service.save(request("native.tool-v1_2")).unwrap();
        let library = temporary.path().join("library/library.json");
        let before = fs::read(&library).unwrap();
        for name in [
            "工具前检查 · PreToolUse / Bash".to_owned(),
            "two words".to_owned(),
            "a/b".to_owned(),
            ".".to_owned(),
            "..".to_owned(),
            "x".repeat(129),
        ] {
            let error = service.save(request(&name)).unwrap_err();
            assert!(error.contains("资产名称"), "{category:?}: {error}");
        }
        assert_eq!(fs::read(library).unwrap(), before);
    }
}

#[test]
fn stale_hook_plan_and_canceled_background_operation_preserve_native_configuration() {
    let (_temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let (item, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::Disable,
        vec![binding.id.clone()],
    );
    let mut external: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    external["unrelated"]["external"] = json!(true);
    write_json(&path, &external);
    let done = run(&service, confirmation);
    assert_eq!(
        done.targets[0].outcome,
        Some(AgentAssetOperationOutcome::UnchangedConflict)
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        serde_json::to_vec_pretty(&external).unwrap()
    );
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::Disable,
        vec![binding.id],
    );
    let request = AgentCatalogApplyRequest {
        plan_token: confirmation.token.unwrap(),
        asset_id: confirmation.asset_id,
        action: confirmation.action,
    };
    assert!(service.start("another-window", &request).is_err());
    let operation = service.start("fixture", &request).unwrap();
    assert!(service.start("fixture", &request).is_err());
    service.cancel("fixture", &operation.id).unwrap();
    service.run_operation(&operation.id);
    assert_eq!(
        service.operation("fixture", &operation.id).unwrap().targets[0].outcome,
        Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
    );
    assert_eq!(
        fs::read(path).unwrap(),
        serde_json::to_vec_pretty(&external).unwrap()
    );
}

#[test]
fn hook_variants_merge_by_agent_and_other_agent_versions_do_not_create_false_drift() {
    let (_temporary, inspector, service) = fixture();
    claude_hooks(&inspector);
    let gemini_path = inspector.home.join(".gemini/settings.json");
    write_json(
        &gemini_path,
        &json!({"hooks":{"BeforeTool":[{"matcher":"*","hooks":[{"type":"command","command":"printf gemini-user-hook"}]}]}}),
    );
    let (claude, claude_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let (_, gemini_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Gemini,
        "printf gemini-user-hook",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == gemini_binding.id)
        })
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: claude.id.clone(),
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let first = adopt(&service, inspector.clone(), &claude_binding.id);
    let saved = adopt(&service, inspector.clone(), &gemini_binding.id);
    assert_eq!(saved.asset_id, first.asset_id);
    assert_eq!(saved.hook.as_ref().unwrap().variants.len(), 2);
    assert!(saved
        .hook
        .as_ref()
        .unwrap()
        .variants
        .iter()
        .any(|variant| variant.agent_kind == AgentCliKind::ClaudeCode
            && variant.group_json.contains("selected-user-hook")));
    let targets = [AgentCliKind::ClaudeCode, AgentCliKind::Gemini]
        .into_iter()
        .map(|kind| primary_target(&service, inspector.as_ref(), kind))
        .collect();
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &saved.asset_id,
            AgentCatalogAction::ApplyDefinition,
            targets,
        ),
    ));
    let before_gemini = fs::read(&gemini_path).unwrap();
    let updated = update_command(
        &service,
        &saved,
        AgentCliKind::ClaudeCode,
        "printf claude-user-hook-v3",
    );
    let (item, gemini) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::Gemini,
        "printf gemini-user-hook",
    );
    assert_eq!(item.id, saved.asset_id);
    assert_eq!(gemini.applied_version, Some(2));
    assert_eq!(gemini.drift, AgentCatalogDrift::InSync);
    let target = primary_target(&service, inspector.as_ref(), AgentCliKind::ClaudeCode);
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &updated.asset_id,
            AgentCatalogAction::ApplyDefinition,
            vec![target],
        ),
    ));
    assert_eq!(fs::read(gemini_path).unwrap(), before_gemini);
}

#[test]
fn same_source_first_and_third_handlers_suspend_and_restore_as_one_composed_change() {
    let (_temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let (first, first_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let (_, third_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf other-user-hook",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == third_binding.id)
        })
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: first.id.clone(),
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &first.id,
            AgentCatalogAction::Disable,
            vec![first_binding.id, third_binding.id],
        ),
    ));
    let paused: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        paused["hooks"]["PreToolUse"][0]["hooks"],
        json!([{"type":"command","command":"printf keep-neighbor"}])
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == first.id)
        .unwrap();
    assert_eq!(item.unresolved_targets.len(), 2);
    assert!(item
        .unresolved_targets
        .iter()
        .all(|target| target.state == AgentCatalogUnresolvedState::Suspended));
    let targets = item
        .unresolved_targets
        .iter()
        .map(|target| target.target_id.clone())
        .collect();
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &first.id,
            AgentCatalogAction::Enable,
            targets,
        ),
    ));
    let restored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let commands = restored["hooks"]["PreToolUse"][0]["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hook| hook["command"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        [
            "printf selected-user-hook",
            "printf keep-neighbor",
            "printf other-user-hook"
        ]
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == first.id)
        .unwrap();
    assert_eq!(item.bindings.len(), 2);
    assert!(item.unresolved_targets.is_empty());
}

#[test]
fn already_restored_hook_is_not_inserted_twice_and_pending_mixed_files_are_never_replayed() {
    let (temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let before = fs::read(&path).unwrap();
    let (item, binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &item.id,
            AgentCatalogAction::Disable,
            vec![binding.id],
        ),
    ));
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let receipt_id = catalog
        .assets
        .iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap()
        .unresolved_targets[0]
        .target_id
        .clone();
    fs::write(&path, &before).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let restored = catalog
        .assets
        .iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap();
    assert_eq!(restored.bindings.len(), 1);
    assert!(restored.unresolved_targets.is_empty());
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "refresh recognizes the external restore without inserting a second rule"
    );
    let receipt = service
        .repository
        .transact(|library| Ok(library.entries[&item.id].receipts[&receipt_id].clone()))
        .unwrap();
    let source_id = receipt.hook.as_ref().unwrap().rule.source_id.clone();
    let allowed_root = inspector
        .inspect()
        .unwrap()
        .inventory
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .unwrap()
        .allowed_root
        .clone();
    let auxiliary_root = temporary.path().canonicalize().unwrap();
    let auxiliary = auxiliary_root.join("auxiliary.json");
    fs::write(&auxiliary, b"after-auxiliary").unwrap();
    service
        .repository
        .transact(|library| {
            let entry = library.entries.get_mut(&item.id).unwrap();
            let mut pending_receipt = receipt.clone();
            pending_receipt.hook.as_mut().unwrap().pending = Some(hook_receipts::HookIntent {
                id: "interrupted-fixture".into(),
                before: Some(Box::new(receipt.clone())),
                after: None,
                files: vec![
                    hook_receipts::HookFileEvidence {
                        source_id: Some(source_id),
                        root: allowed_root,
                        path: path.to_string_lossy().into_owned(),
                        before: Some(digest(&before)),
                        after: digest(b"uncommitted-native"),
                    },
                    hook_receipts::HookFileEvidence {
                        source_id: None,
                        root: auxiliary_root.to_string_lossy().into_owned(),
                        path: auxiliary.to_string_lossy().into_owned(),
                        before: Some(digest(b"before-auxiliary")),
                        after: digest(b"after-auxiliary"),
                    },
                ],
            });
            entry.receipts.insert(receipt_id.clone(), pending_receipt);
            Ok(())
        })
        .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let unresolved = catalog
        .assets
        .iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap();
    assert!(unresolved
        .unresolved_targets
        .iter()
        .any(|target| target.state == AgentCatalogUnresolvedState::Unknown));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&auxiliary).unwrap(), b"after-auxiliary");
    fs::write(&auxiliary, b"before-auxiliary").unwrap();
    service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert!(service
        .repository
        .transact(|library| Ok(library.entries[&item.id].receipts[&receipt_id]
            .hook
            .as_ref()
            .unwrap()
            .pending
            .is_none()))
        .unwrap());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn cancel_after_first_native_file_preserves_the_other_target_and_independent_recovery() {
    use super::target_regressions::{trust_workspace, WorkspaceInspector};
    let (_temporary, base, service) = fixture();
    let user_path = claude_hooks(&base);
    let workspace = base.home.join("project");
    let project_path = workspace.join(".claude/settings.json");
    write_json(
        &project_path,
        &json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"printf project-user-hook"}]}]}}),
    );
    trust_workspace(&base.home, &workspace);
    let before_user = fs::read(&user_path).unwrap();
    let before_project = fs::read(&project_path).unwrap();
    let inspector = Arc::new(WorkspaceInspector {
        home: base.home.clone(),
        workspace,
        settings: base.settings.clone(),
        discover_installations: false,
    });
    let service = Arc::new(service);
    let (item, user_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    let (_, project_binding) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf project-user-hook",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| {
            item.bindings
                .iter()
                .any(|binding| binding.id == project_binding.id)
        })
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: item.id.clone(),
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: inspector
                    .workspace()
                    .map(|path| path.to_string_lossy().into_owned()),
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let confirmation = plan(
        &service,
        inspector.clone(),
        &item.id,
        AgentCatalogAction::Disable,
        vec![user_binding.id, project_binding.id],
    );
    assert!(confirmation.targets.iter().all(|target| target.available));
    let operation = service
        .start(
            "fixture",
            &AgentCatalogApplyRequest {
                plan_token: confirmation.token.unwrap(),
                asset_id: confirmation.asset_id,
                action: confirmation.action,
            },
        )
        .unwrap();
    let weak = Arc::downgrade(&service);
    let operation_id = operation.id.clone();
    *service.after_hook_write.lock().unwrap() = Some(Box::new(move || {
        weak.upgrade()
            .unwrap()
            .cancel("fixture", &operation_id)
            .unwrap();
    }));
    service.run_operation(&operation.id);
    let done = service.operation("fixture", &operation.id).unwrap();
    assert_eq!(
        done.targets
            .iter()
            .filter(
                |target| target.outcome == Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
            )
            .count(),
        1
    );
    assert_eq!(
        done.targets
            .iter()
            .filter(|target| target.outcome == Some(AgentAssetOperationOutcome::AppliedUnverified))
            .count(),
        1
    );
    assert_ne!(
        fs::read(&user_path).unwrap() == before_user,
        fs::read(&project_path).unwrap() == before_project,
        "exactly one native file may commit"
    );
    let user_after = fs::read(&user_path).unwrap();
    let project_after = fs::read(&project_path).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let current = catalog
        .assets
        .iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap();
    assert_eq!(
        current
            .unresolved_targets
            .iter()
            .filter(|target| target.state == AgentCatalogUnresolvedState::Suspended)
            .count(),
        1
    );
    assert!(current
        .unresolved_targets
        .iter()
        .all(|target| target.state != AgentCatalogUnresolvedState::Unknown));
    assert_eq!(fs::read(user_path).unwrap(), user_after);
    assert_eq!(fs::read(project_path).unwrap(), project_after);
}

#[test]
fn unrelated_association_preserves_unadopted_suspended_hook_and_its_restore_action() {
    let (_temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let (suspended, selected) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert!(suspended.version.is_none());
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &suspended.id,
            AgentCatalogAction::Disable,
            vec![selected.id],
        ),
    ));
    let (destination, _) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf keep-neighbor",
    );
    let (_, other) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf other-user-hook",
    );
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let source = catalog
        .assets
        .iter()
        .find(|item| item.bindings.iter().any(|binding| binding.id == other.id))
        .unwrap();
    let relation = service
        .preview_relation(
            "fixture",
            AgentCatalogRelationPreviewRequest {
                intent: AgentCatalogRelationIntent::Merge {
                    destination_asset_id: destination.id,
                    source_asset_id: source.id.clone(),
                },
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
    assert!(relation.available, "{:?}", relation.reason);
    service
        .commit_relation(
            "fixture",
            AgentCatalogRelationCommitRequest {
                plan_token: relation.token.unwrap(),
                relation_key: relation.relation_key,
                action: relation.action,
            },
        )
        .unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let item = catalog
        .assets
        .iter()
        .find(|item| item.id == suspended.id)
        .expect("unselected recovery-only entries must survive unrelated association");
    assert_eq!(item.ownership, AgentCatalogOwnership::Observed);
    assert!(item.version.is_none() && item.bindings.is_empty());
    assert_eq!(item.unresolved_targets.len(), 1);
    let retained = &item.unresolved_targets[0];
    assert_eq!(retained.state, AgentCatalogUnresolvedState::Suspended);
    assert!(retained
        .actions
        .iter()
        .any(|action| action.action == AgentAssetActionKind::Enable && action.available));
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &item.id,
            AgentCatalogAction::Enable,
            vec![retained.target_id.clone()],
        ),
    ));
    let (restored, _) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert_eq!(restored.id, suspended.id);
    let document: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        document["hooks"]["PreToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn externally_restored_suspended_hook_rebinds_on_refresh_and_restart_without_native_writes() {
    let (temporary, inspector, service) = fixture();
    let path = claude_hooks(&inspector);
    let mut restored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let (item, selected) = binding_for(
        &service,
        inspector.as_ref(),
        AgentCliKind::ClaudeCode,
        "printf selected-user-hook",
    );
    assert_verified(&run(
        &service,
        plan(
            &service,
            inspector.clone(),
            &item.id,
            AgentCatalogAction::Disable,
            vec![selected.id.clone()],
        ),
    ));
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let receipt_id = catalog
        .assets
        .iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap()
        .unresolved_targets[0]
        .target_id
        .clone();
    // The external writer restores the same complete definition at another
    // array index. The old slot now contains an unrelated neighboring rule.
    restored["hooks"]["PreToolUse"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .rotate_left(1);
    write_json(&path, &restored);
    let native_after = fs::read(&path).unwrap();
    drop(service);
    for _ in 0..2 {
        let restarted = CatalogService::new(
            temporary.path().canonicalize().unwrap().join("library"),
            Arc::new(MutationService::default()),
        );
        for _ in 0..2 {
            let catalog = restarted.catalog(&inspector.inspect().unwrap()).unwrap();
            let current = catalog
                .assets
                .iter()
                .find(|candidate| candidate.id == item.id)
                .expect("external restoration keeps the original global asset");
            assert_eq!(current.bindings.len(), 1);
            assert!(current.unresolved_targets.is_empty());
            assert_eq!(current.ownership, AgentCatalogOwnership::Observed);
            assert!(current.version.is_none());
            assert_ne!(current.bindings[0].id, selected.id);
            assert_eq!(
                catalog
                    .assets
                    .iter()
                    .flat_map(|asset| &asset.bindings)
                    .filter(|binding| {
                        binding.native.agent_kind == AgentCliKind::ClaudeCode
                            && binding.native.category == AgentAssetCategory::Hook
                    })
                    .count(),
                3,
                "each native rule must have only one global binding"
            );
            let receipt = restarted
                .repository
                .transact(|library| Ok(library.entries[&item.id].receipts[&receipt_id].clone()))
                .unwrap();
            let hook = receipt.hook.unwrap();
            assert!(hook.state == hook_receipts::HookBindingState::Active);
            assert!(hook.pending.is_none());
            assert_eq!(
                hook.rule.native_asset_id.as_deref(),
                Some(current.bindings[0].id.as_str())
            );
            assert_eq!(fs::read(&path).unwrap(), native_after);
        }
    }
}

#[test]
fn suspended_hook_does_not_claim_similar_duplicate_or_unreadable_external_rules() {
    for scenario in ["similar", "duplicate", "unreadable"] {
        let (_temporary, inspector, service) = fixture();
        let path = claude_hooks(&inspector);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let (item, selected) = binding_for(
            &service,
            inspector.as_ref(),
            AgentCliKind::ClaudeCode,
            "printf selected-user-hook",
        );
        assert_verified(&run(
            &service,
            plan(
                &service,
                inspector.clone(),
                &item.id,
                AgentCatalogAction::Disable,
                vec![selected.id],
            ),
        ));
        match scenario {
            "similar" => {
                document["hooks"]["PreToolUse"][0]["matcher"] = json!("Read");
                write_json(&path, &document);
            }
            "duplicate" => {
                let selected = document["hooks"]["PreToolUse"][0]["hooks"][0].clone();
                document["hooks"]["PreToolUse"][0]["hooks"]
                    .as_array_mut()
                    .unwrap()
                    .push(selected);
                write_json(&path, &document);
            }
            "unreadable" => fs::write(&path, b"{ incomplete native configuration").unwrap(),
            _ => unreachable!(),
        }
        let native_after = fs::read(&path).unwrap();
        let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
        let current = catalog
            .assets
            .iter()
            .find(|candidate| candidate.id == item.id)
            .expect("unresolved external changes must preserve the recovery record");
        assert!(current.bindings.is_empty(), "{scenario}");
        assert_eq!(current.unresolved_targets.len(), 1);
        let retained = &current.unresolved_targets[0];
        assert_eq!(
            retained.state,
            if scenario == "similar" {
                AgentCatalogUnresolvedState::Suspended
            } else {
                AgentCatalogUnresolvedState::Unknown
            },
            "{scenario}"
        );
        if retained.state == AgentCatalogUnresolvedState::Unknown {
            assert!(retained.actions.is_empty());
        }
        let receipt = service
            .repository
            .transact(|library| Ok(library.entries[&item.id].receipts[&retained.target_id].clone()))
            .unwrap();
        let hook = receipt.hook.unwrap();
        assert!(hook.state == hook_receipts::HookBindingState::Suspended);
        assert!(hook.rule.native_asset_id.is_none());
        assert_eq!(hook.desired.group["matcher"], "Bash");
        assert_eq!(fs::read(&path).unwrap(), native_after);
    }
}
