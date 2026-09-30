use super::*;
use crate::services::agent_cli::environment::{
    access_registry::{
        AgentAssetAccessRegistry, AgentAssetAccessRequest, AgentAssetAccessTargetKind,
        AgentSourceAccessEvidence,
    },
    mutation::GuardedFile,
    open_asset,
    preview::AgentSourcePreviewPolicy,
    read_asset,
};
use std::{cell::Cell, os::unix::fs::symlink};

const PRIVATE_BODY: &str = "API_KEY=private-linked-skill-fixture\nDo not execute fixture content\n";
const SCRIPT: &str = "print('private-package-resource-fixture')\n";

fn write_skill(path: &Path, name: &str) {
    fs::create_dir_all(path.join("scripts")).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Fixture\n---\n{PRIVATE_BODY}"),
    )
    .unwrap();
    fs::write(path.join("scripts/run.py"), SCRIPT).unwrap();
}

fn link_skill(home: &Path, alias: &str, target: &str) -> PathBuf {
    let manifest = home.join(".claude/skills");
    fs::create_dir_all(&manifest).unwrap();
    let link = manifest.join(alias);
    symlink(format!("../../.agents/skills/{target}"), &link).unwrap();
    link
}

fn claude_skill<'a>(snapshot: &'a MutationInventory, alias: &str) -> &'a AgentAssetRecord {
    snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.agent_kind == AgentCliKind::ClaudeCode
                && asset.category == AgentAssetCategory::Skill
                && asset.native_id == alias
        })
        .unwrap()
}

#[test]
fn readonly_skill_links_close_all_33_catalog_bindings_and_shared_package_fingerprints() {
    let (_temporary, inspector, service) = fixture();
    for index in 0..2 {
        write_skill(
            &inspector.home.join(format!(".claude/skills/local-{index}")),
            &format!("local-{index}"),
        );
    }
    fs::create_dir(inspector.home.join(".claude/skills/no-manifest")).unwrap();
    for index in 0..31 {
        let target = format!("shared-{index:02}");
        write_skill(
            &inspector.home.join(".agents/skills").join(&target),
            &target,
        );
        link_skill(
            &inspector.home,
            &format!("claude-alias-{index:02}"),
            &target,
        );
    }
    let snapshot = inspector.inspect().unwrap();
    let catalog = service.catalog(&snapshot).unwrap();
    let claude_assets = catalog
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Skill
                && asset
                    .bindings
                    .iter()
                    .any(|binding| binding.native.agent_kind == AgentCliKind::ClaudeCode)
        })
        .collect::<Vec<_>>();
    assert_eq!(claude_assets.len(), 33);
    for asset in claude_assets {
        assert_eq!(asset.variants.len(), 1);
        assert!(asset.variants[0].complete);
        let claude = asset
            .bindings
            .iter()
            .find(|binding| binding.native.agent_kind == AgentCliKind::ClaudeCode)
            .unwrap();
        assert!(claude.can_adopt);
        assert!(claude.reason.is_none());
        assert!(claude.variant_id.is_some());
        let source = projection::definition_source(&snapshot, &claude.native).unwrap();
        let payload = projection::observe_payload(&snapshot, &claude.native).unwrap();
        let definition::DefinitionPayload::Skill(files) = payload else {
            panic!("expected Skill package")
        };
        assert_eq!(files.len(), 2);
        assert_eq!(files["scripts/run.py"].bytes, SCRIPT.as_bytes());
        if claude.native.native_id.starts_with("claude-alias-") {
            assert!(!source.writable);
            assert_eq!(
                Path::new(&source.path),
                inspector
                    .home
                    .join(".claude/skills")
                    .join(&claude.native.native_id)
                    .join("SKILL.md")
            );
            let codex = asset
                .bindings
                .iter()
                .find(|binding| binding.native.agent_kind == AgentCliKind::Codex)
                .expect("same physical package has a Codex native binding");
            assert_eq!(claude.variant_id, codex.variant_id);
            let physical =
                crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                    .physical_key(&claude.native);
            assert!(physical.is_some());
            assert_eq!(
                physical,
                crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                    .physical_key(&codex.native)
            );
            assert!(GuardedFile::capture(source, &snapshot.source_anchors).is_err());
            assert!(claude
                .native
                .actions
                .iter()
                .filter(|action| matches!(
                    action.action,
                    AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
                ))
                .all(|action| !action.available));
        }
    }
    let public = serde_json::to_string(&catalog).unwrap();
    assert!(!public.contains("private-linked-skill-fixture"));
    assert!(!public.contains("private-package-resource-fixture"));
    assert!(!public.contains("Do not execute fixture content"));
    let again = service.catalog(&inspector.inspect().unwrap()).unwrap();
    assert_eq!(catalog.revision, again.revision);
}

#[test]
fn readonly_skill_link_aliases_keep_native_identities_and_share_full_resource_variants() {
    let (_temporary, inspector, service) = fixture();
    let target = inspector.home.join(".agents/skills/physical-package");
    write_skill(&target, "physical-package");
    let link = link_skill(&inspector.home, "first-alias", "physical-package");
    link_skill(&inspector.home, "second-alias", "physical-package");
    let snapshot = inspector.inspect().unwrap();
    let first = claude_skill(&snapshot, "first-alias");
    let second = claude_skill(&snapshot, "second-alias");
    assert_ne!(first.stable_id, second.stable_id);
    assert_ne!(first.inspection_source_id, second.inspection_source_id);
    let before = projection::observe_payload(&snapshot, first)
        .unwrap()
        .fingerprint();
    let catalog = service.catalog(&snapshot).unwrap();
    let shared = catalog
        .assets
        .iter()
        .find(|asset| {
            asset
                .bindings
                .iter()
                .any(|binding| binding.id == first.stable_id)
        })
        .unwrap();
    assert!(shared
        .bindings
        .iter()
        .any(|binding| binding.id == second.stable_id));
    assert!(shared
        .bindings
        .iter()
        .any(|binding| binding.native.agent_kind == AgentCliKind::Codex));
    assert_eq!(shared.variants.len(), 1);
    assert!(package::read_package(&link, &inspector.home.join(".claude")).is_err());

    fs::write(target.join("scripts/run.py"), "print('changed-resource')\n").unwrap();
    let refreshed = inspector.inspect().unwrap();
    let after = projection::observe_payload(&refreshed, claude_skill(&refreshed, "first-alias"))
        .unwrap()
        .fingerprint();
    assert_ne!(
        before, after,
        "package fingerprint includes resources beyond SKILL.md"
    );
    let updated = service.catalog(&refreshed).unwrap();
    let shared_updated = updated
        .assets
        .iter()
        .find(|asset| asset.id == shared.id)
        .unwrap();
    assert_eq!(shared_updated.variants.len(), 1);
    assert_ne!(shared.variants[0].id, shared_updated.variants[0].id);

    symlink(
        target.join("scripts/run.py"),
        target.join("linked-resource.py"),
    )
    .unwrap();
    let with_resource_link = inspector.inspect().unwrap();
    assert!(projection::observe_payload(
        &with_resource_link,
        claude_skill(&with_resource_link, "first-alias")
    )
    .is_err());
}

#[test]
fn readonly_skill_link_retarget_rejects_old_catalog_preview_and_open_evidence() {
    for change in [
        "retarget",
        "replace link",
        "replace target",
        "change content",
    ] {
        let (_temporary, inspector, service) = fixture();
        let target = inspector.home.join(".agents/skills/first-package");
        let alternate = inspector.home.join(".agents/skills/second-package");
        write_skill(&target, "same-content");
        write_skill(&alternate, "same-content");
        let link = link_skill(&inspector.home, "claude-alias", "first-package");
        let mut snapshot = inspector.inspect().unwrap();
        let registry = AgentAssetAccessRegistry::default();
        let evidence = snapshot
            .source_anchors
            .iter()
            .map(|(source_id, anchor)| AgentSourceAccessEvidence {
                source_id: source_id.clone(),
                anchor: anchor.clone(),
                policy: AgentSourcePreviewPolicy::metadata_only(),
            })
            .collect();
        registry
            .publish("linked-skill-fixture", &mut snapshot.inventory, evidence)
            .unwrap();
        let asset = claude_skill(&snapshot, "claude-alias");
        projection::definition_source(&snapshot, asset).unwrap();
        let AgentAssetAccess::Ready { access_id } = &asset.access else {
            panic!("linked Skill must have verified opaque access")
        };
        let request = AgentAssetAccessRequest {
            actor: "linked-skill-fixture",
            environment_id: &snapshot.inventory.environment.id,
            workspace: None,
            target_id: &asset.stable_id,
            access_id,
            target_kind: AgentAssetAccessTargetKind::Asset,
        };
        let preview = read_asset(&registry, request).unwrap();
        assert_eq!(preview.path, link.join("SKILL.md").to_string_lossy());
        assert!(preview.content.is_none());
        let risks = asset
            .actions
            .iter()
            .find(|action| action.action == AgentAssetActionKind::Open)
            .unwrap()
            .risks
            .clone();
        let opened = Cell::new(0);
        open_asset(
            &registry,
            request,
            AgentAssetOpenTarget::Asset,
            &risks,
            |path| {
                assert_eq!(path, link.join("SKILL.md"));
                opened.set(opened.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        let before = projection::observe_payload(&snapshot, asset)
            .unwrap()
            .fingerprint();
        assert!(
            crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                .physical_key(asset)
                .is_some()
        );
        service.catalog(&snapshot).unwrap();
        match change {
            "retarget" | "replace link" => {
                let raw_target = fs::read_link(&link).unwrap();
                fs::rename(&link, inspector.home.join("previous-link")).unwrap();
                symlink(
                    if change == "retarget" {
                        alternate.clone()
                    } else {
                        raw_target
                    },
                    &link,
                )
                .unwrap();
            }
            "replace target" => {
                fs::rename(&target, inspector.home.join("previous-package")).unwrap();
                write_skill(&target, "same-content");
            }
            "change content" => {
                fs::write(target.join("SKILL.md"), "---\nname: same-content\ndescription: Fixture\n---\nchanged-private-fixture-body\n").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            projection::observe_payload(&snapshot, asset).is_err(),
            "{change}"
        );
        assert!(
            crate::services::agent_cli::catalog::observation::DefinitionReader::new(&snapshot)
                .physical_key(asset)
                .is_none(),
            "{change}"
        );
        assert!(read_asset(&registry, request).is_err(), "{change}");
        assert!(
            open_asset(
                &registry,
                request,
                AgentAssetOpenTarget::Asset,
                &risks,
                |_| {
                    opened.set(opened.get() + 1);
                    Ok(())
                }
            )
            .is_err(),
            "{change}"
        );
        assert_eq!(
            opened.get(),
            1,
            "stale evidence must not invoke external opener"
        );
        let refreshed = inspector.inspect().unwrap();
        let after =
            projection::observe_payload(&refreshed, claude_skill(&refreshed, "claude-alias"))
                .unwrap()
                .fingerprint();
        if change != "change content" {
            assert_eq!(
                before, after,
                "identical content still invalidates the previous access evidence"
            );
        }
    }
}
