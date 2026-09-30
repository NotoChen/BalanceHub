use super::{
    tests::{fixture, FixtureInspector},
    *,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

fn skill(home: &Path, agent_dir: &str, body: &str) -> PathBuf {
    let path = home.join(agent_dir).join("skills/bh-relation");
    fs::create_dir_all(&path).unwrap();
    write_skill(&path, body);
    path
}
fn write_skill(path: &Path, body: &str) {
    fs::write(
        path.join("SKILL.md"),
        format!(
            "---\nname: bh-relation\ndescription: synthetic relationship fixture\n---\n{body}\n"
        ),
    )
    .unwrap();
}
fn read(service: &CatalogService, inspector: &FixtureInspector) -> AgentAssetCatalog {
    service
        .read_catalog(&inspector.inspect().unwrap())
        .unwrap()
        .0
}
fn for_agent(catalog: &AgentAssetCatalog, agent: AgentCliKind) -> &AgentCatalogAsset {
    catalog
        .assets
        .iter()
        .find(|asset| {
            asset
                .bindings
                .iter()
                .any(|binding| binding.native.agent_kind == agent)
        })
        .unwrap()
}
fn preview(
    service: &CatalogService,
    inspector: Arc<FixtureInspector>,
    intent: AgentCatalogRelationIntent,
) -> AgentCatalogRelationPreview {
    let catalog = read(service, &inspector);
    service
        .preview_relation(
            "window",
            AgentCatalogRelationPreviewRequest {
                intent,
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector,
        )
        .unwrap()
}
fn commit(
    service: &CatalogService,
    preview: &AgentCatalogRelationPreview,
) -> AgentCatalogRelationCommitResult {
    assert!(preview.available, "{:?}", preview.reason);
    service
        .commit_relation(
            "window",
            AgentCatalogRelationCommitRequest {
                plan_token: preview.token.clone().expect("mutation token"),
                relation_key: preview.relation_key.clone(),
                action: preview.action,
            },
        )
        .unwrap()
}
fn ids(catalog: &AgentAssetCatalog) -> (String, String) {
    (
        for_agent(catalog, AgentCliKind::Codex).id.clone(),
        for_agent(catalog, AgentCliKind::ClaudeCode).id.clone(),
    )
}
fn merge_intent(left: &str, right: &str) -> AgentCatalogRelationIntent {
    AgentCatalogRelationIntent::Merge {
        destination_asset_id: left.to_owned(),
        source_asset_id: right.to_owned(),
    }
}

#[test]
fn compare_and_merge_preview_are_read_only_and_tokens_remain_actor_bound() {
    let (temp, inspector, service) = fixture();
    let left_path = skill(&inspector.home, ".codex", "codex variant");
    let right_path = skill(&inspector.home, ".claude", "claude variant");
    let (left, right) = ids(&read(&service, &inspector));
    let comparison = preview(
        &service,
        inspector.clone(),
        AgentCatalogRelationIntent::Compare {
            left_asset_id: left.clone(),
            right_asset_id: right.clone(),
        },
    );
    assert!(comparison.token.is_none());
    assert_eq!(
        comparison.equality,
        AgentCatalogComparisonEquality::Different
    );
    assert_eq!(
        comparison
            .capabilities
            .iter()
            .filter(|capability| matches!(
                capability.intent,
                AgentCatalogRelationIntent::Merge { .. }
            ) && capability.available)
            .count(),
        2
    );
    let selected = preview(&service, inspector.clone(), merge_intent(&left, &right));
    assert!(selected.token.is_some());
    assert!(!temp.path().join("library").exists());
    let left_bytes = fs::read(left_path.join("SKILL.md")).unwrap();
    let right_bytes = fs::read(right_path.join("SKILL.md")).unwrap();
    let request = AgentCatalogRelationCommitRequest {
        plan_token: selected.token.clone().unwrap(),
        relation_key: selected.relation_key.clone(),
        action: selected.action,
    };
    assert!(service.commit_relation("other", request.clone()).is_err());
    let result = service.commit_relation("window", request.clone()).unwrap();
    assert!(result.association_id.is_some());
    assert!(service.commit_relation("window", request).is_err());
    assert_eq!(fs::read(left_path.join("SKILL.md")).unwrap(), left_bytes);
    assert_eq!(fs::read(right_path.join("SKILL.md")).unwrap(), right_bytes);
    let merged = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(merged.assets.len(), 1);
    assert_eq!(merged.assets[0].bindings.len(), 2);
    assert_eq!(merged.assets[0].version, None);
    assert_eq!(merged.assets[0].manual_associations.len(), 1);
    assert!(merged.assets[0].manual_associations[0].can_detach);
}

#[test]
fn detach_moves_current_sources_and_preserves_later_native_edits_and_shared_versions() {
    let (_temp, inspector, service) = fixture();
    let left_path = skill(&inspector.home, ".codex", "destination native");
    let right_path = skill(&inspector.home, ".claude", "source native");
    let (left, right) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    let merged = commit(
        &service,
        &preview(&service, inspector.clone(), merge_intent(&left, &right)),
    );
    let saved = service.save(AgentCatalogSaveRequest { asset_id: Some(left.clone()), expected_version: None, name: "bh-relation".to_owned(), category: AgentAssetCategory::Skill,
        mcp: None, hook: None, skill_markdown: Some("---\nname: bh-relation\ndescription: shared version remains\n---\nExplicit shared definition\n".to_owned()) }).unwrap();
    write_skill(&right_path, "later native edit must survive detach");
    service.catalog(&inspector.inspect().unwrap()).unwrap();
    let before_left = fs::read(left_path.join("SKILL.md")).unwrap();
    let before_right = fs::read(right_path.join("SKILL.md")).unwrap();
    let detached = preview(
        &service,
        inspector.clone(),
        AgentCatalogRelationIntent::Detach {
            association_id: merged.association_id.unwrap(),
        },
    );
    commit(&service, &detached);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(catalog.assets.len(), 2);
    assert_eq!(for_agent(&catalog, AgentCliKind::Codex).id, left);
    assert_eq!(for_agent(&catalog, AgentCliKind::ClaudeCode).id, right);
    assert_eq!(service.definition(&saved.asset_id).unwrap().version, 1);
    assert!(service.definition(&right).is_err());
    assert!(for_agent(&catalog, AgentCliKind::Codex)
        .manual_associations
        .is_empty());
    assert!(for_agent(&catalog, AgentCliKind::Codex)
        .separated_asset_ids
        .contains(&right));
    assert_eq!(fs::read(left_path.join("SKILL.md")).unwrap(), before_left);
    assert_eq!(fs::read(right_path.join("SKILL.md")).unwrap(), before_right);
}

#[test]
fn detach_comparison_and_commit_include_aliases_added_to_the_current_physical_group() {
    let (_temp, inspector, service) = fixture();
    let source_path = skill(&inspector.home, ".agents", "current shared source");
    let destination_path = skill(&inspector.home, ".claude", "independent destination");
    let (source_id, destination_id) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    let association_id = commit(
        &service,
        &preview(
            &service,
            inspector.clone(),
            merge_intent(&destination_id, &source_id),
        ),
    )
    .association_id
    .unwrap();
    let alias = inspector.home.join(".claude/skills/bh-added-alias");
    std::os::unix::fs::symlink("../../.agents/skills/bh-relation", &alias).unwrap();
    let current = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let merged = current
        .assets
        .iter()
        .find(|asset| asset.id == destination_id)
        .unwrap();
    let alias_binding = merged
        .bindings
        .iter()
        .find(|binding| {
            binding.native.agent_kind == AgentCliKind::ClaudeCode
                && binding.native.native_id == "bh-added-alias"
        })
        .expect("new physical alias must join the current logical asset")
        .id
        .clone();
    let source_before = fs::read(source_path.join("SKILL.md")).unwrap();
    let destination_before = fs::read(destination_path.join("SKILL.md")).unwrap();
    let detached = preview(
        &service,
        inspector.clone(),
        AgentCatalogRelationIntent::Detach { association_id },
    );
    assert!(detached.available, "{:?}", detached.reason);
    let restored = detached
        .sides
        .iter()
        .find(|side| side.asset_id == source_id)
        .unwrap();
    let compared_bindings = restored
        .bindings
        .iter()
        .map(|binding| binding.binding_id.clone())
        .collect::<BTreeSet<_>>();
    assert!(compared_bindings.contains(&alias_binding));
    assert_eq!(
        compared_bindings,
        detached
            .affected_binding_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
    );
    commit(&service, &detached);
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let restored = catalog
        .assets
        .iter()
        .find(|asset| asset.id == source_id)
        .unwrap();
    let destination = catalog
        .assets
        .iter()
        .find(|asset| asset.id == destination_id)
        .unwrap();
    assert_eq!(
        restored
            .bindings
            .iter()
            .map(|binding| binding.id.clone())
            .collect::<BTreeSet<_>>(),
        compared_bindings
    );
    assert!(!destination
        .bindings
        .iter()
        .any(|binding| binding.id == alias_binding));
    assert!(restored.separated_asset_ids.contains(&destination_id));
    assert_eq!(
        fs::read(source_path.join("SKILL.md")).unwrap(),
        source_before
    );
    assert_eq!(fs::read(alias.join("SKILL.md")).unwrap(), source_before);
    assert_eq!(
        fs::read(destination_path.join("SKILL.md")).unwrap(),
        destination_before
    );
    assert_eq!(
        fs::read_link(alias).unwrap(),
        Path::new("../../.agents/skills/bh-relation")
    );
}

#[test]
fn ignored_pairs_stay_separate_through_a_third_equal_asset_and_restore_only_the_hint() {
    let (_temp, inspector, service) = fixture();
    let codex_path = skill(&inspector.home, ".codex", "a");
    let claude_path = skill(&inspector.home, ".claude", "b");
    let gemini_path = skill(&inspector.home, ".gemini", "c");
    let (left, right) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    commit(
        &service,
        &preview(
            &service,
            inspector.clone(),
            AgentCatalogRelationIntent::KeepSeparate {
                left_asset_id: left,
                right_asset_id: right,
                hide_candidate: true,
            },
        ),
    );
    for path in [&codex_path, &claude_path, &gemini_path] {
        write_skill(path, "now fully equal");
    }
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let (left, right) = ids(&catalog);
    assert_ne!(left, right);
    assert_eq!(catalog.assets.len(), 2);
    assert!(!for_agent(&catalog, AgentCliKind::Codex)
        .candidate_ids
        .contains(&right));
    assert!(for_agent(&catalog, AgentCliKind::Codex)
        .separated_asset_ids
        .contains(&right));
    commit(
        &service,
        &preview(
            &service,
            inspector.clone(),
            AgentCatalogRelationIntent::RestoreHint {
                left_asset_id: left.clone(),
                right_asset_id: right.clone(),
            },
        ),
    );
    let restored = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(restored.assets.len(), 2);
    assert!(for_agent(&restored, AgentCliKind::Codex)
        .candidate_ids
        .contains(&right));
    assert!(for_agent(&restored, AgentCliKind::Codex)
        .separated_asset_ids
        .contains(&right));
    assert_eq!(
        ids(&service.catalog(&inspector.inspect().unwrap()).unwrap()),
        (left, right)
    );
}

#[test]
fn current_shared_physical_sources_invalidate_an_impossible_separation() {
    let (_temp, inspector, service) = fixture();
    let shared = skill(&inspector.home, ".agents", "global physical source");
    let claude = skill(&inspector.home, ".claude", "independent source");
    let (left, right) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    commit(
        &service,
        &preview(
            &service,
            inspector.clone(),
            AgentCatalogRelationIntent::KeepSeparate {
                left_asset_id: left,
                right_asset_id: right,
                hide_candidate: true,
            },
        ),
    );
    fs::remove_dir_all(&claude).unwrap();
    std::os::unix::fs::symlink("../../.agents/skills/bh-relation", &claude).unwrap();
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(catalog.assets.len(), 1);
    assert!(catalog.assets[0].bindings.len() >= 2);
    assert!(catalog.assets[0].separated_asset_ids.is_empty());
    assert!(catalog
        .diagnostics
        .iter()
        .any(|message| message.contains("分开约束已失效")));
    assert!(fs::read_to_string(shared.join("SKILL.md"))
        .unwrap()
        .contains("global physical source"));
}

#[test]
fn detach_rejects_a_new_shared_physical_group_instead_of_splitting_aliases() {
    let (_temp, inspector, service) = fixture();
    skill(&inspector.home, ".agents", "global physical source");
    let claude = skill(&inspector.home, ".claude", "independent source");
    let (left, right) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    let association = commit(
        &service,
        &preview(&service, inspector.clone(), merge_intent(&left, &right)),
    )
    .association_id
    .unwrap();
    fs::remove_dir_all(&claude).unwrap();
    std::os::unix::fs::symlink("../../.agents/skills/bh-relation", &claude).unwrap();
    let current = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    let detached = preview(
        &service,
        inspector.clone(),
        AgentCatalogRelationIntent::Detach {
            association_id: association,
        },
    );
    assert!(!detached.available);
    assert!(detached.token.is_none());
    assert!(detached.reason.unwrap().contains("共享物理"));
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
    assert_eq!(current.assets.len(), 1);
}

#[test]
fn independent_managed_history_and_missing_detach_sources_have_explicit_reasons() {
    let (_temp, inspector, service) = fixture();
    skill(&inspector.home, ".codex", "a");
    let source_path = skill(&inspector.home, ".claude", "b");
    let (left, right) = ids(&service.catalog(&inspector.inspect().unwrap()).unwrap());
    service
        .save(AgentCatalogSaveRequest {
            asset_id: Some(right.clone()),
            expected_version: None,
            name: "bh-relation".to_owned(),
            category: AgentAssetCategory::Skill,
            mcp: None,
            hook: None,
            skill_markdown: Some(
                "---\nname: bh-relation\ndescription: independent history\n---\nretained version\n"
                    .to_owned(),
            ),
        })
        .unwrap();
    let prohibited = preview(&service, inspector.clone(), merge_intent(&left, &right));
    assert!(!prohibited.available);
    assert!(prohibited.token.is_none());
    assert!(prohibited.reason.unwrap().contains("独立共享版本"));
    let allowed = preview(&service, inspector.clone(), merge_intent(&right, &left));
    let association = commit(&service, &allowed).association_id.unwrap();
    // The original source of this direction is Codex. Remove only that fixture.
    fs::remove_file(inspector.home.join(".codex/skills/bh-relation/SKILL.md")).unwrap();
    service.catalog(&inspector.inspect().unwrap()).unwrap();
    let detached = preview(
        &service,
        inspector.clone(),
        AgentCatalogRelationIntent::Detach {
            association_id: association,
        },
    );
    assert!(!detached.available);
    assert!(detached.token.is_none());
    assert!(detached.reason.is_some());
    assert_eq!(service.definition(&right).unwrap().version, 1);
    assert!(source_path.join("SKILL.md").exists());
}

#[test]
fn a_relation_token_rechecks_overlapping_background_admission_without_writing() {
    let (_temp, inspector, service) = fixture();
    skill(&inspector.home, ".codex", "a");
    skill(&inspector.home, ".claude", "b");
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let (left, right) = ids(&catalog);
    let relation = preview(&service, inspector.clone(), merge_intent(&left, &right));
    let item = for_agent(&catalog, AgentCliKind::Codex);
    let source = AgentCatalogPlanSource::NativeBinding {
        asset_id: left.clone(),
        binding_id: item.bindings[0].id.clone(),
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
        .find(|target| target.available && target.agent_kind == AgentCliKind::Gemini)
        .unwrap();
    let plan = service
        .plan(
            "window",
            AgentCatalogPlanRequest {
                source,
                action: AgentCatalogAction::ApplyDefinition,
                target_ids: vec![target.target_id.clone()],
                expected_revision: catalog.revision,
                workspace: None,
            },
            inspector.clone(),
        )
        .unwrap();
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
    let before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    let result = service.commit_relation(
        "window",
        AgentCatalogRelationCommitRequest {
            plan_token: relation.token.unwrap(),
            relation_key: relation.relation_key,
            action: relation.action,
        },
    );
    assert!(result.unwrap_err().contains("后台任务"));
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
    service.cancel("window", &operation.id).unwrap();
    service.run_operation(&operation.id);
    assert!(preview(&service, inspector, merge_intent(&left, &right)).available);
}

#[test]
fn closing_relationship_previews_discards_private_buffers_without_creating_a_library() {
    let (temp, inspector, service) = fixture();
    skill(&inspector.home, ".codex", "a");
    skill(&inspector.home, ".claude", "b");
    let (left, right) = ids(&read(&service, &inspector));
    let planned = preview(&service, inspector, merge_intent(&left, &right));
    assert!(service.preview_bytes.load(Ordering::Acquire) > 0);
    service.remove_actor("window");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
    assert!(!temp.path().join("library").exists());
    assert!(service
        .commit_relation(
            "window",
            AgentCatalogRelationCommitRequest {
                plan_token: planned.token.unwrap(),
                relation_key: planned.relation_key,
                action: planned.action
            }
        )
        .is_err());
}

#[test]
fn relationship_previews_charge_retained_inspector_settings_and_release_the_budget() {
    let (_temp, mut inspector, service) = fixture();
    let secret = format!("bh-private-notification-fixture-{}", "x".repeat(256 * 1024));
    Arc::get_mut(&mut inspector)
        .unwrap()
        .settings
        .notification_channels[0]
        .secret = secret.clone();
    skill(&inspector.home, ".codex", "a");
    skill(&inspector.home, ".claude", "b");
    let catalog = service.catalog(&inspector.inspect().unwrap()).unwrap();
    let (left, right) = ids(&catalog);
    let before = service
        .repository
        .read_snapshot()
        .unwrap()
        .guard
        .bytes()
        .unwrap()
        .to_vec();
    let settings_bytes = serde_json::to_vec(inspector.settings()).unwrap().len();
    let reserved = planning::reserve(
        &service.preview_bytes,
        64 * 1024 * 1024 - settings_bytes / 2,
    )
    .unwrap();
    let occupied = service.preview_bytes.load(Ordering::Acquire);
    let request = AgentCatalogRelationPreviewRequest {
        intent: merge_intent(&left, &right),
        expected_revision: catalog.revision,
        workspace: None,
    };
    let error = service
        .preview_relation("window", request.clone(), inspector.clone())
        .unwrap_err();
    assert!(error.contains("64 MiB"), "{error}");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), occupied);
    drop(reserved);
    let relation = service
        .preview_relation("window", request, inspector)
        .unwrap();
    assert!(relation.token.is_some());
    assert!(service.preview_bytes.load(Ordering::Acquire) >= settings_bytes);
    assert!(!serde_json::to_string(&relation).unwrap().contains(&secret));
    service.remove_actor("window");
    assert_eq!(service.preview_bytes.load(Ordering::Acquire), 0);
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
}
