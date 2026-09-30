use super::*;
use crate::{
    models::{
        AgentAssetAccess, AgentAssetCategory, AgentAssetLimits, AgentAssetScope, AgentAssetSource,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetLogicalOrigin, AgentAssetSnapshot, AgentAssetSourcePathPolicy,
            AgentAssetSourceSpec,
        },
        environment::{
            mutation::GuardedFile,
            run::AgentInventoryRun,
            snapshot::{snapshot_revision_of, RealSnapshotPort, SnapshotPort, SnapshotRequest},
        },
    },
};
use std::{collections::BTreeMap, fs, os::unix::fs::symlink};

const MARKDOWN: &str = "---\nname: fixture\ndescription: A fixture\n---\nPrivate fixture body\n";

struct SkillLinkFixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    manifest: PathBuf,
    shared: PathBuf,
    target: PathBuf,
    link: PathBuf,
}

impl SkillLinkFixture {
    fn new(absolute: bool) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        let manifest = home.join(".claude/skills");
        let shared = home.join(".agents/skills");
        let target = shared.join("physical-package");
        fs::create_dir_all(&manifest).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::create_dir(shared.join("same-byte-package")).unwrap();
        fs::write(target.join("SKILL.md"), MARKDOWN).unwrap();
        fs::write(shared.join("same-byte-package/SKILL.md"), MARKDOWN).unwrap();
        let link = manifest.join("declared-alias");
        symlink(
            if absolute {
                target.clone()
            } else {
                PathBuf::from("../../.agents/skills/physical-package")
            },
            &link,
        )
        .unwrap();
        Self {
            _temporary: temporary,
            home,
            manifest,
            shared,
            target,
            link,
        }
    }

    fn manifest_spec(&self) -> AgentAssetSourceSpec {
        AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
                shared_root: self.shared.clone(),
            },
            native_source_key: "skills".to_owned(),
            label: "Claude Skills".to_owned(),
            scope: AgentAssetScope::User,
            path: self.manifest.clone(),
            allowed_root: self.home.join(".claude"),
            precedence: 10,
            writable: true,
            sensitive: true,
            source_kind: AgentAssetSourceKind::Directory,
            categories: vec![AgentAssetCategory::Skill],
            allowed_logical_origins: vec![AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            }],
        }
    }

    fn capture_manifest(&self, port: &RealSnapshotPort) -> VerifiedPathAnchor {
        let snapshot = port.snapshot(
            SnapshotRequest {
                source: &self.manifest_spec(),
                source_id: "source:manifest",
                trusted_roots: &[&self.home],
            },
            &mut AgentInventoryRun::new(AgentAssetLimits::DEFAULT),
        );
        assert!(matches!(
            &snapshot,
            AgentAssetSnapshot::DirectoryManifest { complete: true, .. }
        ));
        port.access_anchor(&snapshot_revision_of(&snapshot))
            .unwrap()
    }

    fn capture(&self) -> (AgentAssetSourceSpec, VerifiedPathAnchor) {
        let port = RealSnapshotPort::default();
        let manifest = self.capture_manifest(&port);
        let spec = AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: AgentAssetSourcePathPolicy::ReadonlySkillLink {
                shared_root: self.shared.clone(),
                manifest_path: self.manifest.clone(),
                manifest_revision: manifest.revision().clone(),
                entry_name: "declared-alias".to_owned(),
            },
            native_source_key: "skill-manifest:skills:declared-alias".to_owned(),
            path: self.link.join("SKILL.md"),
            source_kind: AgentAssetSourceKind::File,
            writable: false,
            ..self.manifest_spec()
        };
        let snapshot = port.snapshot(
            SnapshotRequest {
                source: &spec,
                source_id: "source:link",
                trusted_roots: &[&self.home],
            },
            &mut AgentInventoryRun::new(AgentAssetLimits::DEFAULT),
        );
        let AgentAssetSnapshot::File { bytes, revision } = snapshot else {
            panic!("expected complete linked Skill snapshot: {snapshot:?}");
        };
        assert_eq!(bytes, MARKDOWN.as_bytes());
        let anchor = port.access_anchor(&revision).unwrap();
        (spec, anchor)
    }

    fn replace_link(&self, target: &Path) {
        // Keep the prior inode alive so the fixture cannot accidentally reuse it.
        fs::rename(&self.link, self.manifest.join("previous-link")).unwrap();
        symlink(target, &self.link).unwrap();
    }

    fn source(&self, spec: &AgentAssetSourceSpec, anchor: &VerifiedPathAnchor) -> AgentAssetSource {
        AgentAssetSource {
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            id: "source:link".to_owned(),
            context_id: "context:claude".to_owned(),
            label: spec.label.clone(),
            scope: spec.scope,
            environment_id: "environment:native".to_owned(),
            workspace_id: None,
            path: spec.path.to_string_lossy().into_owned(),
            allowed_root: spec.allowed_root.to_string_lossy().into_owned(),
            precedence: spec.precedence,
            writable: spec.writable,
            sensitive: spec.sensitive,
            source_kind: spec.source_kind,
            categories: spec.categories.clone(),
            revision: anchor.revision().clone(),
            diagnostics: Vec::new(),
            access: AgentAssetAccess::default(),
            actions: Vec::new(),
        }
    }
}

#[test]
fn readonly_skill_link_preserves_declared_path_and_binds_the_actual_file() {
    for absolute in [false, true] {
        let fixture = SkillLinkFixture::new(absolute);
        let (spec, anchor) = fixture.capture();
        assert!(anchor.is_readonly_reference());
        assert_eq!(anchor.display_path(), spec.path);
        assert_eq!(anchor.allowed_root(), spec.allowed_root);
        assert!(!anchor.revision().is_symlink);
        let guard = reopen_verified_path(&anchor).unwrap();
        assert_eq!(guard.display_path(), spec.path);
        assert_eq!(
            guard.verified_read_path(),
            (
                fixture.target.join("SKILL.md").as_path(),
                fixture.shared.as_path()
            )
        );
        assert!(anchor.matches_bytes(&guard.read_file_bounded(4096).unwrap()));
        assert!(guard.parent_handle().is_none());
        assert!(guard.revalidate_identity().is_err());
        assert!(reopen_verified_directory_identity(&anchor).is_err());
        assert!(matches!(
            inspect_verified_path(
                &[&fixture.home],
                &spec.allowed_root,
                &spec.path,
                AgentAssetSourceKind::File
            ),
            Err(VerifiedPathError::SymlinkRejected)
        ));
    }
}

#[test]
fn readonly_skill_link_rejects_parent_traversal_after_a_named_or_missing_component() {
    for middle in ["pivot", "missing"] {
        let fixture = SkillLinkFixture::new(false);
        let outside = fixture.home.join("outside");
        fs::create_dir_all(outside.join("dir")).unwrap();
        fs::create_dir(outside.join("physical-package")).unwrap();
        fs::write(
            outside.join("physical-package/SKILL.md"),
            "outside fixture content",
        )
        .unwrap();
        symlink(outside.join("dir"), fixture.shared.join("pivot")).unwrap();
        fixture.replace_link(&PathBuf::from(format!(
            "../../.agents/skills/{middle}/../physical-package"
        )));
        if middle == "pivot" {
            assert_eq!(
                fs::read(fixture.link.join("SKILL.md")).unwrap(),
                b"outside fixture content"
            );
        } else {
            assert!(fs::read(fixture.link.join("SKILL.md")).is_err());
        }
        let manifest = fixture.capture_manifest(&RealSnapshotPort::default());
        assert!(matches!(
            inspect_readonly_skill_link(
                &[&fixture.home],
                &manifest,
                "declared-alias",
                &fixture.shared
            ),
            Err(VerifiedPathError::OutsideAllowedRoot)
        ));
    }
    let fixture = SkillLinkFixture::new(false);
    assert!(normalize_skill_link_target(
        &fixture.manifest,
        &fixture.target.join("../physical-package")
    )
    .is_err());
    assert!(normalize_skill_link_target(&fixture.manifest, Path::new("/../tmp")).is_err());
    assert_eq!(
        normalize_skill_link_target(
            &fixture.manifest,
            Path::new("./../../.agents/skills/physical-package")
        )
        .unwrap(),
        fixture.target
    );
}

#[derive(Debug, Clone, Copy)]
enum Replacement {
    LinkTarget,
    LinkObject,
    TargetDirectory,
    TargetFile,
    Content,
}

impl Replacement {
    fn apply(self, fixture: &SkillLinkFixture) {
        match self {
            Self::LinkTarget => fixture.replace_link(&fixture.shared.join("same-byte-package")),
            Self::LinkObject => fixture.replace_link(&fs::read_link(&fixture.link).unwrap()),
            Self::TargetDirectory => {
                fs::rename(&fixture.target, fixture.shared.join("previous-package")).unwrap();
                fs::create_dir(&fixture.target).unwrap();
                fs::write(fixture.target.join("SKILL.md"), MARKDOWN).unwrap();
            }
            Self::TargetFile => {
                fs::rename(
                    fixture.target.join("SKILL.md"),
                    fixture.target.join("previous.md"),
                )
                .unwrap();
                fs::write(fixture.target.join("SKILL.md"), MARKDOWN).unwrap();
            }
            Self::Content => {
                fs::write(
                    fixture.target.join("SKILL.md"),
                    MARKDOWN.replace("Private", "Changed"),
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn readonly_skill_link_old_anchor_rejects_link_target_and_content_replacement() {
    for change in [
        Replacement::LinkTarget,
        Replacement::LinkObject,
        Replacement::TargetDirectory,
        Replacement::TargetFile,
        Replacement::Content,
    ] {
        let fixture = SkillLinkFixture::new(false);
        let (_, anchor) = fixture.capture();
        let guard = reopen_verified_path(&anchor).unwrap();
        change.apply(&fixture);
        assert!(guard.revalidate().is_err(), "retained guard: {change:?}");
        assert!(
            reopen_verified_path(&anchor).is_err(),
            "reopened guard: {change:?}"
        );
        let fresh_manifest = fixture.capture_manifest(&RealSnapshotPort::default());
        let fresh = inspect_readonly_skill_link(
            &[&fixture.home],
            &fresh_manifest,
            "declared-alias",
            &fixture.shared,
        )
        .unwrap();
        let bytes = fresh.read_file_bounded(4096).unwrap();
        if !matches!(change, Replacement::Content) {
            assert!(
                anchor.matches_bytes(&bytes),
                "same bytes do not authorize old evidence"
            );
        }
    }
}

#[test]
fn readonly_skill_link_checks_link_evidence_independently_of_manifest_membership() {
    for target_changed in [false, true] {
        let fixture = SkillLinkFixture::new(false);
        let (_, mut anchor) = fixture.capture();
        fixture.replace_link(&if target_changed {
            fixture.shared.join("same-byte-package")
        } else {
            fs::read_link(&fixture.link).unwrap()
        });
        // Refresh only the parent stamp. The link's saved inode/raw target
        // evidence must still reject the old reference on its own.
        *anchor.readonly_skill_link.as_mut().unwrap().manifest =
            fixture.capture_manifest(&RealSnapshotPort::default());
        assert!(matches!(
            reopen_verified_path(&anchor),
            Err(VerifiedPathError::SourceChanged)
        ));
    }
}

#[test]
fn readonly_skill_link_read_revalidates_both_sides_after_reading() {
    for change in [
        Replacement::LinkTarget,
        Replacement::LinkObject,
        Replacement::TargetDirectory,
        Replacement::TargetFile,
        Replacement::Content,
    ] {
        let fixture = SkillLinkFixture::new(false);
        let (_, anchor) = fixture.capture();
        let guard = reopen_verified_path(&anchor).unwrap();
        assert!(
            guard
                .read_file_bounded_then(4096, || change.apply(&fixture))
                .is_err(),
            "{change:?}"
        );
    }
}

#[test]
fn readonly_skill_link_anchor_cannot_become_write_authority_by_rewriting_public_fields() {
    let fixture = SkillLinkFixture::new(false);
    let (spec, anchor) = fixture.capture();
    let original_link = fs::read_link(&fixture.link).unwrap();
    let mut source = fixture.source(&spec, &anchor);
    let anchors = BTreeMap::from([(source.id.clone(), anchor)]);
    assert!(GuardedFile::capture(&source, &anchors).is_err());
    source.writable = true;
    source.path = fixture
        .target
        .join("SKILL.md")
        .to_string_lossy()
        .into_owned();
    source.allowed_root = fixture.shared.to_string_lossy().into_owned();
    assert!(GuardedFile::capture(&source, &anchors).is_err());
    source.revision.is_missing = true;
    source.path = fixture.target.join("new.md").to_string_lossy().into_owned();
    assert!(GuardedFile::capture(&source, &anchors).is_err());
    assert!(!fixture.target.join("new.md").exists());
    assert_eq!(fs::read_link(&fixture.link).unwrap(), original_link);
    assert_eq!(
        fs::read(fixture.target.join("SKILL.md")).unwrap(),
        MARKDOWN.as_bytes()
    );
}
