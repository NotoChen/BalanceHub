use super::*;
use crate::models::AgentAssetLimits;
use crate::services::agent_cli::environment::{
    access_registry::test_support::AccessFixture,
    run::AgentInventoryRun,
    snapshot::{snapshot_revision_of, RealSnapshotPort, SnapshotPort, SnapshotRequest},
};
use std::fs;

fn capture(fixture: &AccessFixture, path: &Path) -> VerifiedPathAnchor {
    let mut spec = fixture.source_spec();
    spec.path = path.to_path_buf();
    let port = RealSnapshotPort::default();
    let snapshot = port.snapshot(
        SnapshotRequest {
            source: &spec,
            source_id: "source:test",
            trusted_roots: &[&fixture.root],
        },
        &mut AgentInventoryRun::new(AgentAssetLimits::DEFAULT),
    );
    port.access_anchor(&snapshot_revision_of(&snapshot))
        .expect("snapshot captured object evidence")
}

#[test]
fn exact_revision_binds_source_object_even_when_bytes_are_identical() {
    let fixture = AccessFixture::new("object-revision");
    let before = capture(&fixture, &fixture.file);
    let bytes = fs::read(&fixture.file).unwrap();
    fs::rename(&fixture.file, fixture.root.join("old.toml")).unwrap();
    fs::write(&fixture.file, &bytes).unwrap();
    let after = capture(&fixture, &fixture.file);
    assert!(before.matches_bytes(&bytes));
    assert!(after.matches_bytes(&bytes));
    assert_ne!(before.revision().identity, after.revision().identity);
    assert!(matches!(
        reopen_verified_path(&before),
        Err(VerifiedPathError::SourceChanged)
    ));
}

#[test]
fn an_intermediate_component_replacement_cannot_reauthorize_the_same_file_name() {
    let fixture = AccessFixture::new("component-revision");
    let nested = fixture.root.join(".codex/inside");
    fs::create_dir(&nested).unwrap();
    let path = nested.join("config.toml");
    fs::write(&path, "enabled=true").unwrap();
    let anchor = capture(&fixture, &path);
    fs::rename(&nested, fixture.root.join(".codex/old-inside")).unwrap();
    fs::create_dir(&nested).unwrap();
    fs::write(&path, "enabled=true").unwrap();
    assert!(matches!(
        reopen_verified_path(&anchor),
        Err(VerifiedPathError::SourceChanged)
    ));
}

#[test]
fn complete_same_bytes_allowed_root_replacement_changes_file_revision() {
    let fixture = AccessFixture::new("root-revision");
    let before = capture(&fixture, &fixture.file);
    let bytes = fs::read(&fixture.file).unwrap();
    fs::rename(fixture.root.join(".codex"), fixture.root.join("old-root")).unwrap();
    fs::create_dir(fixture.root.join(".codex")).unwrap();
    fs::write(&fixture.file, &bytes).unwrap();
    let after = capture(&fixture, &fixture.file);
    assert_ne!(before.revision().identity, after.revision().identity);
    assert!(matches!(
        reopen_verified_path(&before),
        Err(VerifiedPathError::RootChanged)
    ));
}

#[cfg(unix)]
#[test]
fn read_after_validation_rejects_in_place_mutation_and_same_bytes_replacement() {
    for replace in [false, true] {
        let fixture = AccessFixture::new("read-race");
        let anchor = capture(&fixture, &fixture.file);
        let guard = reopen_verified_path(&anchor).unwrap();
        let original = fs::read(&fixture.file).unwrap();
        let result = guard.read_file_bounded_then(4096, || {
            if replace {
                fs::rename(&fixture.file, fixture.root.join("old.toml")).unwrap();
                fs::write(&fixture.file, &original).unwrap();
            } else {
                fs::write(&fixture.file, vec![b'x'; original.len()]).unwrap();
            }
        });
        assert!(matches!(result, Err(VerifiedPathError::SourceChanged)));
    }
}

#[cfg(unix)]
#[test]
fn an_aba_tree_swap_never_changes_bytes_read_from_the_retained_file() {
    let fixture = AccessFixture::new("aba");
    let anchor = capture(&fixture, &fixture.file);
    let original = fs::read(&fixture.file).unwrap();
    let guard = reopen_verified_path(&anchor).unwrap();
    let result = guard.read_file_bounded_then(4096, || {
        let root = fixture.root.join(".codex");
        let saved = fixture.root.join("saved");
        fs::rename(&root, &saved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(&fixture.file, "temporary-tree-secret").unwrap();
        fs::remove_file(&fixture.file).unwrap();
        fs::remove_dir(&root).unwrap();
        fs::rename(&saved, &root).unwrap();
    });
    // No claim is made that restored historical events are all detected.
    if let Ok(bytes) = result {
        assert_eq!(bytes, original);
        assert!(!String::from_utf8_lossy(&bytes).contains("temporary-tree-secret"));
    }
}

#[test]
fn bounded_file_reads_never_return_partial_documents() {
    let fixture = AccessFixture::new("bounded-read");
    let anchor = capture(&fixture, &fixture.file);
    let guard = reopen_verified_path(&anchor).unwrap();
    assert!(matches!(
        guard.read_file_bounded(1),
        Err(VerifiedPathError::TooLarge)
    ));
    assert!(anchor.matches_bytes(&guard.read_file_bounded(4096).unwrap()));
}

#[cfg(unix)]
#[test]
fn directory_identity_mode_allows_own_sibling_changes_but_keeps_root_binding() {
    let fixture = AccessFixture::new("directory-identity");
    let root = fixture.root.join(".codex");
    let guard = inspect_verified_path(
        &[&fixture.root],
        &root,
        &root,
        AgentAssetSourceKind::Directory,
    )
    .unwrap();
    let revision = AgentAssetRevision {
        identity: "directory:test".to_string(),
        observed_at: String::new(),
        size_bytes: Some(0),
        is_missing: false,
        is_directory: true,
        is_symlink: false,
    };
    let anchor = guard.anchor(&revision, None);
    fs::write(root.join("own-temporary-file"), "safe").unwrap();
    assert!(guard.revalidate_identity().is_ok());
    assert!(matches!(
        guard.revalidate(),
        Err(VerifiedPathError::SourceChanged)
    ));
    drop(guard);
    assert!(reopen_verified_directory_identity(&anchor).is_ok());
    fs::rename(&root, fixture.root.join("old")).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(matches!(
        reopen_verified_directory_identity(&anchor),
        Err(VerifiedPathError::RootChanged)
    ));
}

#[cfg(windows)]
#[test]
fn final_file_reparse_points_are_rejected_and_no_delete_share_is_retained() {
    use std::os::windows::fs::symlink_file;
    let fixture = AccessFixture::new("windows-file");
    let anchor = capture(&fixture, &fixture.file);
    let guard = reopen_verified_path(&anchor).unwrap();
    assert!(fs::rename(&fixture.file, fixture.root.join("held.toml")).is_err());
    drop(guard);
    let actual = fixture.root.join("actual.toml");
    fs::rename(&fixture.file, &actual).unwrap();
    symlink_file(&actual, &fixture.file)
        .expect("native Windows fixture requires symlink permission");
    assert!(matches!(
        reopen_verified_path(&anchor),
        Err(VerifiedPathError::SymlinkRejected)
    ));
}
