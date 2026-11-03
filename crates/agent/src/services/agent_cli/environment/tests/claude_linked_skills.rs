//! Real filesystem coverage for Claude's explicitly linked user Skills.
use super::super::{snapshot::revision_for_missing, verified_path::VerifiedPathAnchor};
use super::*;
use crate::models::AgentEnvironmentInventory;
use std::os::unix::fs::symlink;

struct IsolatedClaudeSnapshots<'a> {
    home: &'a Path,
    real: RealSnapshotPort,
    max_attempts: usize,
    trace: Mutex<SnapshotTrace>,
}

#[derive(Debug, Default)]
struct SnapshotTrace {
    attempted_paths: Vec<PathBuf>,
    file_paths: BTreeSet<PathBuf>,
}

impl SnapshotPort for IsolatedClaudeSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let path = request.source.path.clone();
        {
            let mut trace = self.trace.lock().unwrap();
            assert!(trace.attempted_paths.len() < self.max_attempts);
            trace.attempted_paths.push(path.clone());
        }
        let snapshot = if !path.starts_with(self.home) {
            // Production discovery also offers these system-wide sources. Do
            // not read a developer's managed configuration in this fixture.
            assert!(matches!(
                request.source.native_source_key.as_str(),
                "managed-settings" | "managed-mcp"
            ));
            AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            }
        } else {
            self.real.snapshot(request, run)
        };
        if matches!(&snapshot, AgentAssetSnapshot::File { .. }) {
            self.trace.lock().unwrap().file_paths.insert(path);
        }
        snapshot
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

struct ExpectedSkill {
    label: String,
    declaration_path: PathBuf,
    content_path: PathBuf,
    content: String,
    relative_link_target: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
struct SkillIdentity {
    asset: String,
    declaration: String,
    source: String,
    revision: String,
}

fn write_skill(directory: &Path, label: &str) -> String {
    fs::create_dir_all(directory).unwrap();
    let content = format!("---\nname: {label}\n---\n\nSynthetic Skill fixture.\n");
    fs::write(directory.join("SKILL.md"), &content).unwrap();
    content
}

fn inventory(home: &Path) -> AgentEnvironmentInventory {
    inventory_at_config_root(home, &home.join(".claude"), AgentAssetLimits::DEFAULT).0
}

fn inventory_at_config_root(
    home: &Path,
    config_root: &Path,
    limits: AgentAssetLimits,
) -> (AgentEnvironmentInventory, SnapshotTrace) {
    let _config_root = ClaudeConfigRootOverrideGuard::new(config_root.to_path_buf());
    let template = definition(AgentCliKind::ClaudeCode);
    let definitions = [AgentCliDefinition {
        kind: template.kind,
        label: template.label,
        executable: template.executable,
        session_name_hint: template.session_name_hint,
        additional_env_keys: template.additional_env_keys,
        home_scan: template.home_scan,
        invalid_path_reason: template.invalid_path_reason,
        require_version_substring: template.require_version_substring,
        endpoint: template.endpoint,
        temporary_launch: template.temporary_launch,
        sessions: template.sessions,
        liveness: template.liveness,
        default_config: template.default_config,
        environment: claude_test_adapter(),
        ..*template
    }];
    let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.270",
        AgentInstallationChannel::Stable,
    )]);
    let snapshots = IsolatedClaudeSnapshots {
        home,
        real: RealSnapshotPort::default(),
        max_attempts: limits.sources_per_context,
        trace: Mutex::new(SnapshotTrace::default()),
    };
    let inventory = build_inventory_with(
        InventoryInput {
            home,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    (inventory, snapshots.trace.into_inner().unwrap())
}

fn assert_skills(
    inventory: &AgentEnvironmentInventory,
    expected: &BTreeMap<String, ExpectedSkill>,
    home: &Path,
) -> BTreeMap<String, SkillIdentity> {
    let assets = inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    let declarations = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    let manifests = inventory
        .sources
        .iter()
        .filter(|source| {
            source.categories.contains(&AgentAssetCategory::Skill)
                && source.source_kind == AgentAssetSourceKind::File
                && !source.revision.is_missing
        })
        .collect::<Vec<_>>();

    // Establish exact, nonzero fixture coverage before any rescan comparison.
    assert_eq!(assets.len(), 33, "{:#?}", inventory.diagnostics);
    assert_eq!(declarations.len(), 33);
    assert_eq!(manifests.len(), 33);
    let expected_names = expected.keys().map(String::as_str).collect::<BTreeSet<_>>();
    assert_eq!(expected_names.len(), 33);
    assert_eq!(
        assets
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        expected_names
    );
    assert_eq!(
        declarations
            .iter()
            .map(|declaration| declaration.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        expected_names
    );
    assert_eq!(inventory.contexts.len(), 1);
    let context = &inventory.contexts[0];
    assert!(context.id.starts_with("context:"));
    assert_eq!(context.agent_kind, AgentCliKind::ClaudeCode);
    assert_eq!(Path::new(&context.config_root), home.join(".claude"));
    assert_eq!(context.workspace_id, None);

    let sources_by_id = inventory
        .sources
        .iter()
        .map(|source| (source.id.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(sources_by_id.len(), inventory.sources.len());
    assert!(sources_by_id.keys().all(|id| id.starts_with("source:")));
    assert!(inventory
        .sources
        .iter()
        .all(|source| { !Path::new(&source.path).starts_with(home.join(".agents/skills")) }));

    let mut identities = BTreeMap::new();
    for asset in assets {
        let fixture = &expected[&asset.native_id];
        let declaration = declarations
            .iter()
            .find(|declaration| declaration.native_id == asset.native_id)
            .unwrap();
        let source = sources_by_id[declaration.source_id.as_str()];
        assert!(asset.stable_id.starts_with("asset:"));
        assert!(declaration.id.starts_with("declaration:"));
        assert_eq!(asset.agent_kind, AgentCliKind::ClaudeCode);
        assert_eq!(asset.context_id, context.id);
        assert_eq!(declaration.context_id, context.id);
        assert_eq!(source.context_id, context.id);
        assert_eq!(asset.environment_id, inventory.environment.id);
        assert_eq!(source.environment_id, inventory.environment.id);
        assert_eq!(asset.scope, AgentAssetScope::User);
        assert_eq!(declaration.scope, AgentAssetScope::User);
        assert_eq!(source.scope, AgentAssetScope::User);
        assert_eq!(asset.workspace_id, None);
        assert_eq!(source.workspace_id, None);
        assert_eq!(asset.label, fixture.label);
        assert_eq!(declaration.label, fixture.label);
        assert_eq!(declaration.declaration_key, asset.native_id);
        assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
        assert_eq!(
            declaration.presence,
            crate::models::AgentAssetPresence::Present
        );
        assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Enabled);
        assert_eq!(asset.declared_state, AgentAssetState::Enabled);
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert_eq!(asset.source_ids, vec![source.id.clone()]);
        assert_eq!(asset.inspection_source_id, source.id);
        assert_eq!(
            asset.represented_declaration_ids,
            vec![declaration.id.clone()]
        );
        assert_eq!(
            asset.resolution.contributor_ids,
            vec![declaration.id.clone()]
        );
        assert_eq!(Path::new(&source.path), fixture.declaration_path);
        assert_eq!(
            asset.path.as_deref().map(Path::new),
            Some(fixture.declaration_path.as_path())
        );
        assert_eq!(source.source_kind, AgentAssetSourceKind::File);
        assert!(!source.revision.is_missing);
        assert!(!source.revision.identity.is_empty());
        assert_eq!(
            source.revision.size_bytes,
            Some(fixture.content.len() as u64)
        );
        assert_eq!(asset.revision.identity, source.revision.identity);
        assert_eq!(
            declaration.evidence.revision.identity,
            source.revision.identity
        );
        if fixture.relative_link_target.is_some() {
            assert!(!source.writable, "linked source must remain read-only");
            assert!(!asset.writable, "linked asset must remain read-only");
        }
        assert!(source.diagnostics.is_empty(), "{:?}", source.diagnostics);
        assert!(
            declaration.diagnostics.is_empty(),
            "{:?}",
            declaration.diagnostics
        );
        assert!(asset.diagnostics.is_empty(), "{:?}", asset.diagnostics);
        assert!(asset.resolution.diagnostics.is_empty());
        identities.insert(
            asset.native_id.clone(),
            SkillIdentity {
                asset: asset.stable_id.clone(),
                declaration: declaration.id.clone(),
                source: source.id.clone(),
                revision: source.revision.identity.clone(),
            },
        );
    }
    assert_eq!(
        identities
            .values()
            .map(|id| &id.asset)
            .collect::<BTreeSet<_>>()
            .len(),
        33
    );
    assert_eq!(
        identities
            .values()
            .map(|id| &id.declaration)
            .collect::<BTreeSet<_>>()
            .len(),
        33
    );
    assert_eq!(
        identities
            .values()
            .map(|id| &id.source)
            .collect::<BTreeSet<_>>()
            .len(),
        33
    );
    identities
}

#[test]
fn claude_inventory_preserves_two_local_and_thirty_one_linked_skills() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().canonicalize().unwrap();
    let claude_skills = home.join(".claude/skills");
    let shared_skills = home.join(".agents/skills");
    fs::create_dir_all(&claude_skills).unwrap();
    fs::create_dir_all(&shared_skills).unwrap();
    let mut expected = BTreeMap::new();
    for index in 1..=2 {
        let name = format!("local-{index:02}");
        let directory = claude_skills.join(&name);
        let label = format!("Synthetic local {index:02}");
        let content = write_skill(&directory, &label);
        expected.insert(
            name,
            ExpectedSkill {
                label,
                declaration_path: directory.join("SKILL.md"),
                content_path: directory.join("SKILL.md"),
                content,
                relative_link_target: None,
            },
        );
    }
    for index in 1..=31 {
        let alias = format!("alias-{index:02}");
        let target_name = format!("shared-{index:02}");
        let directory = shared_skills.join(&target_name);
        let label = format!("Synthetic shared {index:02}");
        let content = write_skill(&directory, &label);
        let relative_target = PathBuf::from("../../.agents/skills").join(&target_name);
        symlink(&relative_target, claude_skills.join(&alias)).unwrap();
        expected.insert(
            alias.clone(),
            ExpectedSkill {
                label,
                declaration_path: claude_skills.join(alias).join("SKILL.md"),
                content_path: directory.join("SKILL.md"),
                content,
                relative_link_target: Some(relative_target),
            },
        );
    }
    fs::create_dir(claude_skills.join("not-a-skill")).unwrap();
    let unreferenced = shared_skills.join("unreferenced");
    let unreferenced_content = write_skill(&unreferenced, "Synthetic unreferenced");
    assert_eq!(
        expected
            .values()
            .filter(|skill| skill.relative_link_target.is_none())
            .count(),
        2
    );
    assert_eq!(
        expected
            .values()
            .filter(|skill| skill.relative_link_target.is_some())
            .count(),
        31
    );
    assert_eq!(fs::read_dir(&claude_skills).unwrap().count(), 34);
    assert_eq!(fs::read_dir(&shared_skills).unwrap().count(), 32);

    let first = inventory(&home);
    let first_ids = assert_skills(&first, &expected, &home);
    let second = inventory(&home);
    let second_ids = assert_skills(&second, &expected, &home);
    assert_eq!(first_ids, second_ids);
    assert_eq!(first.contexts[0].id, second.contexts[0].id);

    // The scan must neither rewrite ordinary files nor replace a link with a
    // materialized directory. Only fixture-owned paths are inspected here.
    for fixture in expected.values() {
        assert_eq!(
            fs::read_to_string(&fixture.content_path).unwrap(),
            fixture.content
        );
        let directory = fixture.declaration_path.parent().unwrap();
        if let Some(relative_target) = &fixture.relative_link_target {
            assert_eq!(fs::read_link(directory).unwrap(), *relative_target);
        } else {
            assert!(fs::symlink_metadata(directory).unwrap().is_dir());
        }
    }
    assert!(!claude_skills.join("not-a-skill/SKILL.md").exists());
    assert_eq!(
        fs::read_to_string(unreferenced.join("SKILL.md")).unwrap(),
        unreferenced_content
    );
}

fn assert_single_readonly_skill(
    inventory: &AgentEnvironmentInventory,
    config_root: &Path,
    alias: &str,
    label: &str,
) {
    let assets = inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    let declarations = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(assets.len(), 1, "{:#?}", inventory.diagnostics);
    assert_eq!(declarations.len(), 1);
    assert_eq!(inventory.contexts.len(), 1);
    let context = &inventory.contexts[0];
    let asset = assets[0];
    let declaration = declarations[0];
    let source = inventory
        .sources
        .iter()
        .find(|source| source.id == declaration.source_id)
        .unwrap();
    let path = config_root.join("skills").join(alias).join("SKILL.md");
    assert_eq!(Path::new(&context.config_root), config_root);
    assert_eq!(asset.context_id, context.id);
    assert_eq!(declaration.context_id, context.id);
    assert_eq!(source.context_id, context.id);
    assert_eq!(asset.native_id, alias);
    assert_eq!(declaration.native_id, alias);
    assert_eq!(declaration.declaration_key, alias);
    assert_eq!(asset.label, label);
    assert_eq!(declaration.label, label);
    assert!(asset.stable_id.starts_with("asset:"));
    assert!(declaration.id.starts_with("declaration:"));
    assert!(source.id.starts_with("source:"));
    assert_eq!(asset.scope, AgentAssetScope::User);
    assert_eq!(declaration.scope, AgentAssetScope::User);
    assert_eq!(source.scope, AgentAssetScope::User);
    assert_eq!(asset.declared_state, AgentAssetState::Enabled);
    assert_eq!(asset.effective_state, AgentAssetState::Enabled);
    assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Enabled);
    assert_eq!(asset.source_ids, vec![source.id.clone()]);
    assert_eq!(asset.inspection_source_id, source.id);
    assert_eq!(
        asset.represented_declaration_ids,
        vec![declaration.id.clone()]
    );
    assert_eq!(Path::new(&source.path), path);
    assert_eq!(asset.path.as_deref().map(Path::new), Some(path.as_path()));
    assert_eq!(source.source_kind, AgentAssetSourceKind::File);
    assert!(!source.revision.is_missing);
    assert!(!source.writable);
    assert!(!asset.writable);
    assert!(source.diagnostics.is_empty(), "{:?}", source.diagnostics);
    assert!(
        declaration.diagnostics.is_empty(),
        "{:?}",
        declaration.diagnostics
    );
    assert!(asset.diagnostics.is_empty(), "{:?}", asset.diagnostics);
}

#[derive(Debug, PartialEq, Eq)]
enum FixtureObject {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

fn fixture_objects(root: &Path) -> BTreeMap<PathBuf, FixtureObject> {
    let mut objects = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let object = if metadata.is_symlink() {
                FixtureObject::Link(fs::read_link(&path).unwrap())
            } else if metadata.is_dir() {
                pending.push(path.clone());
                FixtureObject::Directory
            } else {
                assert!(metadata.is_file());
                FixtureObject::File(fs::read(&path).unwrap())
            };
            // Enumerate only the small synthetic tree, never follow a link.
            assert!(objects.len() < 128);
            objects.insert(path.strip_prefix(root).unwrap().to_path_buf(), object);
        }
    }
    objects
}

#[test]
fn claude_linked_skill_custom_config_root_uses_explicit_native_home() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().canonicalize().unwrap();
    let config_root = home.join("profiles/claude/custom");
    let skills = config_root.join("skills");
    fs::create_dir_all(&skills).unwrap();
    let shared_target = home.join(".agents/skills/custom-target");
    write_skill(&shared_target, "Synthetic explicit native home");
    let alias = "custom-alias";
    symlink(
        "../../../../.agents/skills/custom-target",
        skills.join(alias),
    )
    .unwrap();
    let before = fixture_objects(&home);

    let (inventory, trace) =
        inventory_at_config_root(&home, &config_root, AgentAssetLimits::DEFAULT);
    assert_single_readonly_skill(
        &inventory,
        &config_root,
        alias,
        "Synthetic explicit native home",
    );
    assert_eq!(
        trace.file_paths,
        BTreeSet::from([skills.join(alias).join("SKILL.md")])
    );
    assert!(inventory
        .sources
        .iter()
        .all(|source| { !Path::new(&source.path).starts_with(home.join(".agents/skills")) }));
    assert_eq!(fixture_objects(&home), before);
}

#[test]
fn claude_linked_skill_rejects_unsafe_targets_without_hiding_valid_skill() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("native-home");
    let config_root = home.join(".claude");
    let skills = config_root.join("skills");
    let shared = home.join(".agents/skills");
    fs::create_dir_all(&skills).unwrap();
    write_skill(&shared.join("accepted-target"), "Synthetic allowed target");
    let valid_alias = "valid-alias";
    symlink(
        "../../.agents/skills/accepted-target",
        skills.join(valid_alias),
    )
    .unwrap();

    write_skill(
        &home.join("outside-shared/target"),
        "Synthetic outside shared",
    );
    write_skill(&root.join("outside-home/target"), "Synthetic outside home");
    write_skill(&shared.join("nested/child"), "Synthetic nested target");
    symlink("chain-second", shared.join("chain-first")).unwrap();
    symlink("accepted-target", shared.join("chain-second")).unwrap();
    symlink("cycle-second", shared.join("cycle-first")).unwrap();
    symlink("cycle-first", shared.join("cycle-second")).unwrap();
    symlink("accepted-target", shared.join("linked-directory")).unwrap();
    fs::create_dir(shared.join("linked-manifest")).unwrap();
    symlink(
        "../accepted-target/SKILL.md",
        shared.join("linked-manifest/SKILL.md"),
    )
    .unwrap();
    fs::write(
        shared.join("file-target"),
        b"Synthetic non-directory target",
    )
    .unwrap();
    fs::create_dir_all(shared.join("directory-manifest/SKILL.md")).unwrap();

    let rejected = [
        ("outside-shared", "../../outside-shared/target"),
        ("outside-home", "../../../outside-home/target"),
        ("nested-target", "../../.agents/skills/nested/child"),
        ("chained-target", "../../.agents/skills/chain-first"),
        ("cyclic-target", "../../.agents/skills/cycle-first"),
        ("dangling-target", "../../.agents/skills/missing-target"),
        ("linked-directory", "../../.agents/skills/linked-directory"),
        ("linked-manifest", "../../.agents/skills/linked-manifest"),
        ("file-target", "../../.agents/skills/file-target"),
        (
            "directory-manifest",
            "../../.agents/skills/directory-manifest",
        ),
    ];
    for (name, target) in rejected {
        symlink(target, skills.join(format!("denied-{name}"))).unwrap();
    }
    assert_eq!(fs::read_dir(&skills).unwrap().count(), 11);
    let before = fixture_objects(&root);
    let limits = AgentAssetLimits {
        sources_per_context: 32,
        first_level_entries: 32,
        bytes_per_source: 4_096,
        bytes_per_refresh: 16_384,
        ..AgentAssetLimits::DEFAULT
    };

    let (inventory, trace) = inventory_at_config_root(&home, &config_root, limits);
    // The valid alias sorts after every denied alias. Rejections must not stop
    // discovery before the remaining, independently authorized Skill.
    assert_single_readonly_skill(
        &inventory,
        &config_root,
        valid_alias,
        "Synthetic allowed target",
    );
    assert_eq!(
        trace.file_paths,
        BTreeSet::from([skills.join(valid_alias).join("SKILL.md")])
    );
    assert!(trace.attempted_paths.len() <= 32);
    assert_eq!(
        trace.attempted_paths.iter().collect::<BTreeSet<_>>().len(),
        trace.attempted_paths.len(),
        "link chains and cycles must not cause repeated snapshot attempts"
    );
    assert!(inventory.sources.len() <= 32);
    for (name, _) in rejected {
        let alias = format!("denied-{name}");
        assert!(inventory
            .assets
            .iter()
            .all(|asset| asset.native_id != alias));
        assert!(inventory
            .declarations
            .iter()
            .all(|declaration| declaration.native_id != alias));
    }
    assert_eq!(fixture_objects(&root), before);
}
