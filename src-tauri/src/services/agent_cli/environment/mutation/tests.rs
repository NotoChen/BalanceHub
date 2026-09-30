use super::test_support::TestInspector;
#[cfg(unix)]
use super::{atomic, files::conflict};
use crate::models::*;
use std::fs;

#[cfg(unix)]
#[test]
fn atomic_replace_preserves_mode_unknown_values_and_noop_bytes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    fs::set_permissions(inspector.config(), fs::Permissions::from_mode(0o640)).unwrap();
    let source = inspector.file();
    let inode = fs::metadata(inspector.config()).unwrap().ino();
    assert_eq!(
        atomic::replace(&source, source.bytes().unwrap(), || panic!(
            "no-op crossed commit boundary"
        ))
        .unwrap(),
        atomic::AtomicWriteResult::Unchanged
    );
    assert_eq!(fs::metadata(inspector.config()).unwrap().ino(), inode);
    let replacement = b"enabled = false\nfixture_secret = 'private-fixture-value'\n";
    assert_eq!(
        atomic::replace(&source, replacement, || Ok(())).unwrap(),
        atomic::AtomicWriteResult::Replaced
    );
    assert_eq!(fs::read(inspector.config()).unwrap(), replacement);
    assert_eq!(
        fs::metadata(inspector.config())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert!(fs::read_dir(&inspector.root).unwrap().all(|item| !item
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".balancehub-")));
}

#[cfg(unix)]
#[test]
fn canceled_write_and_replaced_source_never_overwrite_current_content() {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let source = inspector.file();
    let before = fs::read(inspector.config()).unwrap();
    assert!(atomic::replace(&source, b"other", || Err(conflict())).is_err());
    assert_eq!(fs::read(inspector.config()).unwrap(), before);
    let external = inspector.root.join("external");
    fs::write(&external, "untouched").unwrap();
    fs::remove_file(inspector.config()).unwrap();
    symlink(&external, inspector.config()).unwrap();
    assert!(atomic::replace(&source, b"other", || Ok(())).is_err());
    assert_eq!(fs::read_to_string(external).unwrap(), "untouched");
}

#[cfg(unix)]
#[test]
fn missing_creation_is_private_and_does_not_clobber_a_racing_creator() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    fs::remove_file(inspector.config()).unwrap();
    let absent = inspector.file();
    assert_eq!(
        atomic::replace(&absent, b"new", || Ok(())).unwrap(),
        atomic::AtomicWriteResult::Replaced
    );
    assert_eq!(
        fs::metadata(inspector.config())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    fs::remove_file(inspector.config()).unwrap();
    let absent = inspector.file();
    let error = atomic::replace(&absent, b"ours", || {
        fs::write(inspector.config(), "theirs").unwrap();
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.kind, AgentAssetMutationErrorKind::SourceConflict);
    assert_eq!(fs::read_to_string(inspector.config()).unwrap(), "theirs");
}

#[cfg(unix)]
#[test]
fn missing_nested_file_rejects_a_new_intermediate_symlink() {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let mut snapshot = inspector.snapshot();
    let missing_parent = inspector.root.join("not-yet-present");
    let source = &mut snapshot.inventory.sources[0];
    source.path = missing_parent
        .join("nested.toml")
        .to_string_lossy()
        .into_owned();
    source.revision = super::test_support::revision(None);
    let guarded = super::GuardedFile::capture(source, &snapshot.source_anchors).unwrap();
    let external = inspector.root.join("other-location");
    fs::create_dir(&external).unwrap();
    symlink(external, missing_parent).unwrap();
    assert_eq!(
        guarded.revalidate().unwrap_err().kind,
        AgentAssetMutationErrorKind::SourceConflict
    );
}

#[cfg(unix)]
#[test]
fn missing_parent_reanchor_preserves_domain_and_allows_atomic_creation() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let path = inspector.root.join("new/hooks/hooks.json");
    let mut file = super::GuardedFile::capture_path(&inspector.root, &path, 4096).unwrap();
    let source_id = file.source_id.clone();
    let domain = file.domain();
    file.revalidate().unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert_eq!(
        file.revalidate().unwrap_err().kind,
        AgentAssetMutationErrorKind::SourceConflict
    );
    file.reanchor_after_parent_creation(path.parent().unwrap())
        .unwrap();
    assert_eq!(file.source_id, source_id);
    assert_eq!(file.domain(), domain);
    assert_eq!(file.bytes(), None);
    file.revalidate().unwrap();
    assert_eq!(
        atomic::replace(&file, b"{\"hooks\":{}}", || Ok(())).unwrap(),
        atomic::AtomicWriteResult::Replaced
    );
    assert_eq!(fs::read(path).unwrap(), b"{\"hooks\":{}}");
}

#[cfg(unix)]
#[test]
fn missing_sibling_reanchor_only_advances_along_the_ensured_chain() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let path = inspector.root.join("new/other/config.json");
    let ensured = inspector.root.join("new/hooks");
    let mut file = super::GuardedFile::capture_path(&inspector.root, &path, 4096).unwrap();
    fs::create_dir_all(&ensured).unwrap();
    file.reanchor_after_parent_creation(&ensured).unwrap();
    file.revalidate().unwrap();
    // The operation ensured new/hooks, not the sibling new/other.
    fs::create_dir(path.parent().unwrap()).unwrap();
    assert_eq!(
        file.reanchor_after_parent_creation(&ensured)
            .and_then(|()| file.revalidate())
            .unwrap_err()
            .kind,
        AgentAssetMutationErrorKind::SourceConflict
    );
}

#[cfg(unix)]
#[test]
fn missing_parent_reanchor_rejects_changed_ancestor_leaf_and_links() {
    use std::os::unix::fs::symlink;
    for change in ["ancestor", "leaf", "link", "other-branch"] {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = TestInspector::new(temporary.path());
        let retained = inspector.root.join("retained");
        fs::create_dir(&retained).unwrap();
        let path = retained.join("new/hooks/config.json");
        let mut file = super::GuardedFile::capture_path(&inspector.root, &path, 4096).unwrap();
        let domain = file.domain();
        match change {
            "ancestor" => {
                fs::rename(&retained, inspector.root.join("old-retained")).unwrap();
                fs::create_dir_all(path.parent().unwrap()).unwrap();
            }
            "leaf" => {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "external").unwrap();
            }
            "link" => {
                let outside = inspector.root.join("outside");
                fs::create_dir_all(outside.join("hooks")).unwrap();
                symlink(outside, retained.join("new")).unwrap();
            }
            "other-branch" => {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::create_dir_all(retained.join("different/hooks")).unwrap();
            }
            _ => unreachable!(),
        }
        let ensured = if change == "other-branch" {
            retained.join("different/hooks")
        } else {
            path.parent().unwrap().to_path_buf()
        };
        assert_eq!(
            file.reanchor_after_parent_creation(&ensured)
                .and_then(|()| file.revalidate())
                .unwrap_err()
                .kind,
            AgentAssetMutationErrorKind::SourceConflict,
            "{change}"
        );
        assert_eq!(file.domain(), domain);
        if change == "leaf" {
            assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        }
    }
}

#[test]
fn prepared_mutation_rejects_changed_or_new_directory_sources() {
    use super::super::verified_path::inspect_verified_path;
    use super::{MutationInspector, MutationPreparation};
    use sha2::{Digest, Sha256};
    for existed in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = TestInspector::new(temporary.path());
        let path = inspector.root.join("asset-directory");
        if existed {
            fs::create_dir(&path).unwrap();
            fs::File::open(&path)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH)
                .unwrap();
        }
        let mut snapshot = inspector.snapshot();
        let mut source = snapshot.inventory.sources[0].clone();
        source.id = "directory-source".into();
        source.path = path.to_string_lossy().into_owned();
        source.source_kind = AgentAssetSourceKind::Directory;
        source.revision = super::test_support::revision(None);
        source.revision.is_directory = true;
        if existed {
            let guard = inspect_verified_path(
                &[&inspector.root],
                &inspector.root,
                &path,
                AgentAssetSourceKind::Directory,
            )
            .unwrap();
            let mut hash = Sha256::new();
            guard.update_revision(&mut hash);
            source.revision.identity = format!("{:x}", hash.finalize());
            source.revision.is_missing = false;
            snapshot
                .source_anchors
                .insert(source.id.clone(), guard.anchor(&source.revision, None));
        }
        snapshot.inventory.sources.push(source);
        let mechanism = super::test_support::mechanism(AgentAssetActionKind::Disable);
        let prepared = inspector
            .prepare(MutationPreparation {
                inventory: &snapshot.inventory,
                source_anchors: &snapshot.source_anchors,
                context: &snapshot.inventory.contexts[0],
                asset: &snapshot.inventory.assets[0],
                installation: &snapshot.inventory.installations[0],
                mechanism: &mechanism,
                action: AgentAssetActionKind::Disable,
                home: &inspector.root,
                workspace: None,
            })
            .unwrap();
        prepared.revalidate().unwrap();
        fs::create_dir_all(path.join("new-plugin-version")).unwrap();
        assert_eq!(
            prepared.revalidate().unwrap_err().kind,
            AgentAssetMutationErrorKind::SourceConflict
        );
    }
}

#[test]
fn stored_inventory_anchor_cannot_be_relabelled_as_current_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let snapshot = inspector.snapshot();
    fs::write(inspector.config(), "enabled = false\n").unwrap();
    assert!(
        super::GuardedFile::capture(&snapshot.inventory.sources[0], &snapshot.source_anchors)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn hard_links_share_the_same_mutation_domain() {
    use super::super::verified_path::inspect_verified_path;
    let temporary = tempfile::tempdir().unwrap();
    let inspector = TestInspector::new(temporary.path());
    let file = inspector.file();
    let other = inspector.root.join("hard-link.toml");
    fs::hard_link(inspector.config(), &other).unwrap();
    let mut snapshot = inspector.snapshot();
    let source = &mut snapshot.inventory.sources[0];
    source.path = other.to_string_lossy().into_owned();
    let guard = inspect_verified_path(
        &[&inspector.root],
        &inspector.root,
        &other,
        AgentAssetSourceKind::File,
    )
    .unwrap();
    let bytes = guard.read_file_bounded(4096).unwrap();
    snapshot.source_anchors.insert(
        source.id.clone(),
        guard.anchor(&source.revision, Some(&bytes)),
    );
    let alias = super::GuardedFile::capture(source, &snapshot.source_anchors).unwrap();
    assert_eq!(file.domain(), alias.domain());
}

#[test]
fn parent_enable_verifies_children_without_overriding_their_own_disabled_state() {
    let mut inventory = super::test_support::inventory();
    inventory.assets[0].category = AgentAssetCategory::Plugin;
    let mut child = inventory.assets[0].clone();
    child.stable_id = "child".into();
    child.category = AgentAssetCategory::Skill;
    child.relationships.provided_by = Some("asset".into());
    child.effective_state = AgentAssetState::Disabled;
    inventory.assets.push(child);
    let affected = vec!["asset".into(), "child".into()];
    assert!(super::MutationVerification {
        inventory: &inventory,
        asset_id: "asset",
        action: AgentAssetActionKind::Enable,
        affected_asset_ids: &affected
    }
    .parent_state_matches());
    inventory.assets[0].effective_state = AgentAssetState::Disabled;
    inventory.assets[1].effective_state = AgentAssetState::Enabled;
    assert!(!super::MutationVerification {
        inventory: &inventory,
        asset_id: "asset",
        action: AgentAssetActionKind::Disable,
        affected_asset_ids: &affected
    }
    .parent_state_matches());
}
