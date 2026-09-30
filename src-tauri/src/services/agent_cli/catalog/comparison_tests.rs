use super::*;
use crate::services::agent_cli::{
    catalog::{
        tests::{fixture, mcp_request},
        CatalogService,
    },
    environment::mutation::MutationInspector,
};
use std::{fs, path::Path};

fn compare_saved(
    service: &CatalogService,
    snapshot: &MutationInventory,
    left: &str,
    right: &str,
) -> (
    Vec<AgentCatalogComparisonSide>,
    AgentCatalogComparisonEquality,
    Vec<AgentCatalogDifference>,
) {
    let (catalog, library) = service.read_catalog(snapshot).unwrap();
    compare(
        snapshot,
        (
            catalog
                .assets
                .iter()
                .find(|asset| asset.id == left)
                .unwrap(),
            &library.library.entries[left],
        ),
        (
            catalog
                .assets
                .iter()
                .find(|asset| asset.id == right)
                .unwrap(),
            &library.library.entries[right],
        ),
    )
}

fn write_skill(root: &Path, name: &str, script: &[u8]) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Comparison fixture\n---\nUse the bundled script.\n"
        ),
    )
    .unwrap();
    fs::write(root.join("scripts/run.sh"), script).unwrap();
}

#[test]
fn comparison_of_native_sources_never_creates_a_missing_library() {
    let (temporary, inspector, service) = fixture();
    let left = inspector.home.join(".claude/skills/compare");
    let right = inspector.home.join(".codex/skills/compare");
    write_skill(&left, "compare", b"one\n");
    write_skill(&right, "compare", b"two\n");
    let snapshot = inspector.inspect().unwrap();
    let (catalog, library) = service.read_catalog(&snapshot).unwrap();
    let assets = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill && asset.name == "compare")
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 2);
    assert!(!temporary.path().join("library").exists());
    let (_, equality, _) = compare(
        &snapshot,
        (assets[0], &library.library.entries[&assets[0].id]),
        (assets[1], &library.library.entries[&assets[1].id]),
    );
    assert_eq!(equality, AgentCatalogComparisonEquality::Different);
    assert!(!temporary.path().join("library").exists());
    assert_eq!(fs::read(left.join("scripts/run.sh")).unwrap(), b"one\n");
    assert_eq!(fs::read(right.join("scripts/run.sh")).unwrap(), b"two\n");
}

#[cfg(unix)]
#[test]
fn native_skill_comparison_includes_resources_permissions_and_all_source_metadata() {
    use std::os::unix::fs::PermissionsExt;
    for different_bytes in [false, true] {
        let (temporary, inspector, service) = fixture();
        let left_root = inspector.home.join(".codex/skills/compare");
        let right_root = inspector.home.join(".claude/skills/compare");
        write_skill(&left_root, "compare", b"fixture-private-script-left\n");
        write_skill(
            &right_root,
            "compare",
            if different_bytes {
                b"fixture-private-script-right\n"
            } else {
                b"fixture-private-script-left\n"
            },
        );
        fs::set_permissions(
            left_root.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        fs::set_permissions(
            right_root.join("scripts/run.sh"),
            fs::Permissions::from_mode(if different_bytes { 0o644 } else { 0o755 }),
        )
        .unwrap();
        let snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let assets = catalog
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill && asset.name == "compare")
            .collect::<Vec<_>>();
        assert_eq!(assets.len(), 2);
        let library = service.repository.read_snapshot().unwrap();
        let before = fs::read(temporary.path().join("library/library.json")).unwrap();
        let native_before = fs::read(right_root.join("scripts/run.sh")).unwrap();
        let (sides, equality, differences) = compare(
            &snapshot,
            (assets[0], &library.library.entries[&assets[0].id]),
            (assets[1], &library.library.entries[&assets[1].id]),
        );
        assert_eq!(equality, AgentCatalogComparisonEquality::Different);
        let difference = differences
            .iter()
            .find(|difference| difference.path == "scripts/run.sh")
            .unwrap();
        assert!(difference
            .reason
            .as_deref()
            .unwrap()
            .contains(if different_bytes {
                "内容不同"
            } else {
                "执行权限不同"
            }));
        for (side, asset) in sides.iter().zip(assets) {
            assert!(side.complete);
            assert_eq!(side.bindings.len(), asset.bindings.len());
            for (public, original) in side.bindings.iter().zip(&asset.bindings) {
                assert_eq!(public.binding_id, original.id);
                assert_eq!(public.context_id, original.native.context_id);
                assert_eq!(public.agent_kind, original.native.agent_kind);
                assert_eq!(public.scope, original.native.scope);
                assert_eq!(public.provenance, original.native.provenance);
                assert!(public.path.is_some());
                let script = public
                    .documents
                    .iter()
                    .find(|document| document.path.as_deref() == Some("scripts/run.sh"))
                    .unwrap();
                assert!(script
                    .content
                    .as_deref()
                    .is_some_and(|text| text.starts_with("fixture-private-script")));
                assert!(script.reason.as_deref().unwrap().contains("字节"));
            }
        }
        let public = serde_json::to_string(&sides).unwrap();
        assert!(public.contains("fixture-private-script"));
        assert_eq!(
            fs::read(temporary.path().join("library/library.json")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(right_root.join("scripts/run.sh")).unwrap(),
            native_before
        );
    }
}

#[test]
fn changed_or_unanchored_native_source_is_unknown_without_historical_fingerprint_fallback() {
    for missing_anchor in [false, true] {
        let (_temporary, inspector, service) = fixture();
        let root = inspector.home.join(".claude/skills/compare");
        write_skill(&root, "compare", b"fixture-script\n");
        let mut snapshot = inspector.inspect().unwrap();
        let catalog = service.catalog(&snapshot).unwrap();
        let asset = catalog
            .assets
            .iter()
            .find(|asset| asset.category == AgentAssetCategory::Skill && asset.name == "compare")
            .unwrap();
        let library = service.repository.read_snapshot().unwrap();
        if missing_anchor {
            snapshot.source_anchors.clear();
        } else {
            fs::write(
                root.join("SKILL.md"),
                "---\nname: compare\ndescription: Changed externally\n---\nChanged\n",
            )
            .unwrap();
        }
        let entry = &library.library.entries[&asset.id];
        let (sides, equality, differences) = compare(&snapshot, (asset, entry), (asset, entry));
        assert_eq!(equality, AgentCatalogComparisonEquality::Unknown);
        assert!(!sides[0].complete);
        assert!(sides[0].bindings[0].reason.is_some());
        assert!(sides[0].bindings[0].path.is_some());
        assert_eq!(differences[0].kind, AgentCatalogDifferenceKind::Unknown);
    }
}

#[test]
fn read_budget_exhaustion_is_explicit_and_does_not_turn_an_empty_observation_into_equality() {
    let (_temporary, inspector, service) = fixture();
    write_skill(
        &inspector.home.join(".claude/skills/compare"),
        "compare",
        b"fixture\n",
    );
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Skill)
        .unwrap();
    let library = service.repository.read_snapshot().unwrap();
    let current = side(
        &mut DefinitionReader::new(&snapshot),
        (asset, &library.library.entries[&asset.id]),
        Instant::now() + Duration::from_secs(10),
        &mut 0,
    );
    assert!(!current.public.complete);
    assert!(current.definitions.is_empty());
    assert!(current.public.bindings[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("64 MiB"));

    let saved = service.save(mcp_request("managed-budget")).unwrap();
    let (catalog, library) = service.read_catalog(&snapshot).unwrap();
    let asset = catalog
        .assets
        .iter()
        .find(|asset| asset.id == saved.asset_id)
        .unwrap();
    let current = side(
        &mut DefinitionReader::new(&snapshot),
        (asset, &library.library.entries[&asset.id]),
        Instant::now() + Duration::from_secs(10),
        &mut 0,
    );
    assert!(!current.public.complete);
    assert!(current.definitions.is_empty());
    assert!(current.public.reason.as_deref().unwrap().contains("64 MiB"));
}

#[test]
fn preview_limits_count_json_escapes_and_preserve_utf8() {
    let (_temporary, inspector, service) = fixture();
    let markdown = format!("---\nname: compare\ndescription: Comparison fixture\n---\n{}\nAPI_KEY=fixture-private-after-cutoff\n", "中文\\\"\n".repeat(18000));
    let saved = service
        .save(AgentCatalogSaveRequest {
            asset_id: None,
            expected_version: None,
            name: "compare".to_owned(),
            category: AgentAssetCategory::Skill,
            mcp: None,
            hook: None,
            skill_markdown: Some(markdown),
        })
        .unwrap();
    let (mut sides, equality, mut differences) = compare_saved(
        &service,
        &inspector.inspect().unwrap(),
        &saved.asset_id,
        &saved.asset_id,
    );
    assert_eq!(equality, AgentCatalogComparisonEquality::Equal);
    let document = &sides[0].definition[0];
    assert!(document.truncated);
    assert!(
        serde_json::to_vec(document.content.as_ref().unwrap())
            .unwrap()
            .len()
            <= DOCUMENT_BYTES
    );
    assert!(!document
        .content
        .as_ref()
        .unwrap()
        .contains("fixture-private-after-cutoff"));
    // Many independently visible sources exercise the aggregate wire budget.
    sides = vec![sides[0].clone(); 16];
    bound_public_output(&mut sides, &mut differences);
    assert!(serde_json::to_vec(&(&sides, &differences)).unwrap().len() <= PUBLIC_BYTES);
    for side in &sides {
        for document in &side.definition {
            if let Some(content) = &document.content {
                assert!(std::str::from_utf8(content.as_bytes()).is_ok());
                assert!(serde_json::to_vec(content).unwrap().len() <= DOCUMENT_BYTES);
            }
        }
    }
}
