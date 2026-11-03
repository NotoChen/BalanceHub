//! Real native Skill discovery, precedence and catalog grouping across three Agents.
use super::*;
use crate::{
    models::{
        AgentAssetInstallationOrigin, AgentAssetRecord, AgentAssetSuppressionReason,
        AgentConfigurationContext,
    },
    services::agent_cli::{
        catalog::CatalogService,
        contracts::AgentContextDiscoveryRequest,
        environment::{
            mutation::{MutationInventory, MutationService},
            snapshot::revision_for_missing,
            verified_path::VerifiedPathAnchor,
        },
    },
};
use std::os::unix::fs::symlink;

const AGENTS: [AgentCliKind; 3] = [
    AgentCliKind::Codex,
    AgentCliKind::Gemini,
    AgentCliKind::Grok,
];

fn directory(kind: AgentCliKind) -> &'static str {
    match kind {
        AgentCliKind::Codex => ".codex",
        AgentCliKind::Gemini => ".gemini",
        AgentCliKind::Grok => ".grok",
        AgentCliKind::ClaudeCode => unreachable!(),
    }
}

fn isolated_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let root = request.home.join(directory(request.agent_kind));
    let mut contexts = definition(request.agent_kind)
        .environment()
        .discover_contexts(request, output);
    for context in &mut contexts {
        context.config_root = root.to_string_lossy().into_owned();
    }
    contexts
}

struct IsolatedSnapshots<'a> {
    root: &'a Path,
    real: RealSnapshotPort,
    retarget_after_capture: Option<(&'a Path, &'a Path)>,
}

impl SnapshotPort for IsolatedSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        if !request.source.path.starts_with(self.root) {
            // Native system sources remain in the pipeline without reading
            // machine configuration while this isolated fixture runs.
            assert!(matches!(
                request.source.scope,
                AgentAssetScope::System | AgentAssetScope::Managed
            ));
            return AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            };
        }
        let source_path = request.source.path.as_path();
        let snapshot = self.real.snapshot(request, run);
        if let Some((alias_document, replacement)) = self.retarget_after_capture {
            if source_path == alias_document && matches!(snapshot, AgentAssetSnapshot::File { .. })
            {
                let alias_directory = alias_document.parent().unwrap();
                fs::remove_file(alias_directory).unwrap();
                symlink(replacement, alias_directory).unwrap();
            }
        }
        snapshot
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        for base in [&home, &workspace] {
            fs::create_dir_all(base.join(".agents/skills")).unwrap();
            for kind in AGENTS {
                fs::create_dir_all(base.join(directory(kind)).join("skills")).unwrap();
            }
        }
        let trust_key = toml::Value::String(workspace.to_string_lossy().into_owned());
        fs::write(
            home.join(".codex/config.toml"),
            format!("[projects.{trust_key}]\ntrust_level = \"trusted\"\n"),
        )
        .unwrap();
        let folders = BTreeMap::from([(workspace.to_string_lossy().into_owned(), "TRUST_FOLDER")]);
        fs::write(
            home.join(".gemini/trustedFolders.json"),
            serde_json::to_vec(&folders).unwrap(),
        )
        .unwrap();
        Self {
            _temporary: temporary,
            root,
            home,
            workspace,
        }
    }

    fn skill(&self, path: &Path, name: &str) {
        fs::create_dir_all(path.join("scripts")).unwrap();
        fs::write(path.join("SKILL.md"), format!("---\nname: {name}\ndescription: Synthetic cross Agent fixture\n---\nPRIVATE_BODY=cross-agent-private-fixture\n")).unwrap();
        fs::write(
            path.join("scripts/check.py"),
            "print('private-resource-fixture')\n",
        )
        .unwrap();
    }

    fn link(&self, kind: AgentCliKind, workspace: bool, alias: &str, target: &Path) -> PathBuf {
        let base = if workspace {
            &self.workspace
        } else {
            &self.home
        };
        let path = base.join(directory(kind)).join("skills").join(alias);
        symlink(target, &path).unwrap();
        path.join("SKILL.md")
    }

    fn inventory(&self, kinds: &[AgentCliKind], workspace: bool) -> MutationInventory {
        self.inventory_with_retarget(kinds, workspace, None)
    }

    fn inventory_with_retarget(
        &self,
        kinds: &[AgentCliKind],
        workspace: bool,
        retarget_after_capture: Option<(&Path, &Path)>,
    ) -> MutationInventory {
        let definitions = kinds
            .iter()
            .map(|kind| {
                let template = definition(*kind);
                AgentCliDefinition {
                    environment: template.environment().with_test_contexts(isolated_contexts),
                    ..*template
                }
            })
            .collect::<Vec<_>>();
        let (installations, _) = FakeInstallationPort::new(Vec::new());
        let snapshots = IsolatedSnapshots {
            root: &self.root,
            real: RealSnapshotPort::default(),
            retarget_after_capture,
        };
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: workspace.then_some(self.workspace.as_path()),
                settings: None,
            },
            InventoryPipelineDeps {
                definitions: &definitions,
                limits: AgentAssetLimits::DEFAULT,
                clock: Arc::new(ManualClock::new()),
                installations: &installations,
                snapshots: &snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap();
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        MutationInventory {
            inventory,
            source_anchors,
        }
    }
}

fn rows<'a>(
    snapshot: &'a MutationInventory,
    kind: AgentCliKind,
    name: &str,
) -> Vec<&'a AgentAssetRecord> {
    snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.agent_kind == kind
                && asset.category == AgentAssetCategory::Skill
                && asset.native_id == name
        })
        .collect()
}

fn source_at<'a>(
    snapshot: &'a MutationInventory,
    kind: AgentCliKind,
    path: &Path,
) -> &'a AgentAssetSource {
    let context = snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.agent_kind == kind)
        .unwrap();
    snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.context_id == context.id && Path::new(&source.path) == path)
        .unwrap()
}

fn assert_link_source(
    snapshot: &MutationInventory,
    kind: AgentCliKind,
    path: &Path,
    scope: AgentAssetScope,
) {
    let source = source_at(snapshot, kind, path);
    assert_eq!(source.origin, AgentAssetInstallationOrigin::Linked);
    assert_eq!(source.scope, scope);
    assert!(!source.writable);
    assert!(!source.revision.is_missing);
    assert!(!source.revision.is_symlink);
    assert!(source.diagnostics.is_empty());
    let anchor = &snapshot.source_anchors[&source.id];
    assert!(anchor.is_readonly_reference());
    assert_eq!(anchor.display_path(), path);
}

fn assert_winner(
    snapshot: &MutationInventory,
    kind: AgentCliKind,
    name: &str,
    path: &Path,
    count: usize,
) {
    let assets = rows(snapshot, kind, name);
    assert_eq!(
        assets.len(),
        count,
        "{kind:?}: {:#?}",
        snapshot.inventory.diagnostics
    );
    let winners = assets
        .iter()
        .filter(|asset| asset.resolution.relation != AgentAssetResolutionRelation::Replaced)
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1, "{kind:?}");
    assert_eq!(winners[0].path.as_deref().map(Path::new), Some(path));
    for shadowed in assets
        .iter()
        .filter(|asset| asset.path.as_deref().map(Path::new) != Some(path))
    {
        assert_eq!(shadowed.effective_state, AgentAssetState::Shadowed);
    }
    let winning_source = source_at(snapshot, kind, path);
    assert!(winners[0]
        .provenance
        .iter()
        .any(|origin| origin.source_id == winning_source.id
            && origin.installation == winning_source.origin));
    if kind == AgentCliKind::Codex {
        assert_codex_group_evidence(snapshot, name);
    }
}

fn assert_codex_group_evidence(snapshot: &MutationInventory, name: &str) {
    let assets = rows(snapshot, AgentCliKind::Codex, name);
    assert!(!assets.is_empty());
    let declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.context_id == assets[0].context_id
                && declaration.native_kind == AgentAssetCategory::Skill
                && declaration.native_id == name
        })
        .collect::<Vec<_>>();
    assert!(!declarations.is_empty());
    assert_eq!(
        assets
            .iter()
            .flat_map(|asset| asset.represented_declaration_ids.iter().cloned())
            .collect::<BTreeSet<_>>(),
        declarations
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        assets
            .iter()
            .flat_map(|asset| asset.resolution.contributor_ids.iter().cloned())
            .collect::<BTreeSet<_>>(),
        declarations
            .iter()
            .filter(|declaration| {
                declaration.participation == AgentAssetResolutionParticipation::Participates
            })
            .map(|declaration| declaration.id.clone())
            .collect::<BTreeSet<_>>()
    );
    for asset in assets {
        assert_eq!(asset.resolution.contributor_ids.len(), 1);
        assert_eq!(asset.provenance.len(), 1);
        assert_eq!(
            asset.provenance[0].declaration_id,
            asset.resolution.contributor_ids[0]
        );
    }
}

#[test]
fn cross_agent_linked_skills_preserve_native_sources_and_one_global_physical_package() {
    let fixture = Fixture::new();
    let target = fixture.home.join(".agents/skills/physical-package");
    fixture.skill(&target, "shared-name");
    let paths = AGENTS.map(|kind| (kind, fixture.link(kind, false, "user-alias", &target)));
    let snapshot = fixture.inventory(&AGENTS, false);
    for (kind, path) in &paths {
        assert_link_source(&snapshot, *kind, path, AgentAssetScope::User);
        let shared = source_at(&snapshot, *kind, &target.join("SKILL.md"));
        assert_eq!(shared.origin, AgentAssetInstallationOrigin::SharedFiles);
        let definitions = snapshot
            .inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.context_id == shared.context_id
                    && declaration.native_kind == AgentAssetCategory::Skill
                    && declaration.role == AgentAssetDeclarationRole::Definition
            })
            .collect::<Vec<_>>();
        assert_eq!(
            definitions.len(),
            2,
            "all declarations remain visible for {kind:?}"
        );
    }
    let codex_path = paths
        .iter()
        .find(|(kind, _)| *kind == AgentCliKind::Codex)
        .unwrap()
        .1
        .as_path();
    assert_winner(&snapshot, AgentCliKind::Codex, "shared-name", codex_path, 1);
    assert_winner(
        &snapshot,
        AgentCliKind::Gemini,
        "shared-name",
        &target.join("SKILL.md"),
        2,
    );
    let grok_path = paths
        .iter()
        .find(|(kind, _)| *kind == AgentCliKind::Grok)
        .unwrap()
        .1
        .as_path();
    assert_winner(&snapshot, AgentCliKind::Grok, "shared-name", grok_path, 2);
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let skills = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(
        skills.len(),
        1,
        "one physical target must be one global logical asset"
    );
    assert_eq!(skills[0].bindings.len(), 5);
    assert_eq!(skills[0].variants.len(), 1);
    assert!(skills[0].variants[0].complete);
    assert!(skills[0]
        .bindings
        .iter()
        .all(|binding| binding.can_adopt && binding.variant_id.is_some()));
    let public = serde_json::to_string(&catalog).unwrap();
    assert!(!public.contains("cross-agent-private-fixture"));
    assert!(!public.contains("private-resource-fixture"));
    let again = service.catalog(&fixture.inventory(&AGENTS, false)).unwrap();
    assert_eq!(catalog.revision, again.revision);
}

#[test]
fn cross_agent_linked_skills_workspace_scope_keeps_native_precedence() {
    let fixture = Fixture::new();
    let target = fixture.home.join(".agents/skills/physical-package");
    fixture.skill(&target, "same-name");
    let project_shared = fixture.workspace.join(".agents/skills/project-copy");
    fixture.skill(&project_shared, "same-name");
    for kind in AGENTS {
        fixture.link(kind, false, "user-alias", &target);
        fixture.link(kind, true, "project-alias", &target);
    }
    let snapshot = fixture.inventory(&AGENTS, true);
    for kind in AGENTS {
        let project_alias = fixture
            .workspace
            .join(directory(kind))
            .join("skills/project-alias/SKILL.md");
        assert_link_source(&snapshot, kind, &project_alias, AgentAssetScope::Workspace);
        let source = source_at(&snapshot, kind, &project_alias);
        assert_eq!(
            source.precedence,
            match kind {
                AgentCliKind::Codex => 20,
                AgentCliKind::Gemini => 30,
                AgentCliKind::Grok => 40,
                _ => unreachable!(),
            }
        );
        assert_eq!(
            source_at(&snapshot, kind, &project_shared.join("SKILL.md")).origin,
            AgentAssetInstallationOrigin::SharedFiles
        );
    }
    assert_winner(
        &snapshot,
        AgentCliKind::Gemini,
        "same-name",
        &project_shared.join("SKILL.md"),
        4,
    );
    assert_winner(
        &snapshot,
        AgentCliKind::Grok,
        "same-name",
        &fixture
            .workspace
            .join(".grok/skills/project-alias/SKILL.md"),
        4,
    );
    let codex = rows(&snapshot, AgentCliKind::Codex, "same-name");
    assert_eq!(
        codex.len(),
        2,
        "same-name distinct physical documents coexist"
    );
    assert!(codex
        .iter()
        .all(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Additive));
    let expected = BTreeSet::from([
        fixture
            .workspace
            .join(".codex/skills/project-alias/SKILL.md"),
        project_shared.join("SKILL.md"),
    ]);
    assert_eq!(
        codex
            .iter()
            .map(|asset| PathBuf::from(asset.path.as_ref().unwrap()))
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_codex_group_evidence(&snapshot, "same-name");
}

#[test]
fn cross_agent_linked_skills_codex_keeps_native_paths_and_groups_equivalent_catalog_definitions() {
    let fixture = Fixture::new();
    let one = fixture.home.join(".agents/skills/one");
    let two = fixture.home.join(".agents/skills/two");
    fixture.skill(&one, "same-name");
    fixture.skill(&two, "same-name");
    let alias = fixture.link(AgentCliKind::Codex, false, "alias", &one);
    fs::write(
        fixture.home.join(".codex/config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            toml::Value::String(one.join("SKILL.md").to_string_lossy().into_owned())
        ),
    )
    .unwrap();
    let snapshot = fixture.inventory(&[AgentCliKind::Codex], false);
    let assets = rows(&snapshot, AgentCliKind::Codex, "same-name");
    assert_eq!(assets.len(), 2);
    assert_codex_group_evidence(&snapshot, "same-name");
    assert!(assets
        .iter()
        .all(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Additive));
    let selected = assets
        .iter()
        .find(|asset| asset.path.as_deref().map(Path::new) == Some(alias.as_path()))
        .unwrap();
    assert_eq!(selected.effective_state, AgentAssetState::Disabled);
    let separate = assets
        .iter()
        .find(|asset| asset.path.as_deref().map(Path::new) == Some(two.join("SKILL.md").as_path()))
        .unwrap();
    assert_eq!(separate.effective_state, AgentAssetState::Enabled);
    let definitions = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Skill
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 3);
    assert_eq!(
        definitions
            .iter()
            .filter(|declaration| declaration.participation
                == AgentAssetResolutionParticipation::Participates)
            .count(),
        2
    );
    assert_eq!(
        definitions
            .iter()
            .filter(|declaration| matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            ))
            .count(),
        1
    );
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let catalog_skills = catalog
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(catalog_skills.len(), 1);
    assert_eq!(catalog_skills[0].variants.len(), 1);
    assert_eq!(catalog_skills[0].bindings.len(), assets.len());
    for original in assets {
        let binding = catalog_skills[0]
            .bindings
            .iter()
            .find(|binding| binding.id == original.stable_id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&binding.native).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
}

#[test]
fn cross_agent_linked_skills_codex_unnamed_aliases_use_the_physical_directory_name() {
    let fixture = Fixture::new();
    let target = fixture.home.join(".agents/skills/physical-name");
    fixture.skill(&target, "unused-name");
    fs::write(target.join("SKILL.md"), "---\ndescription: Synthetic unnamed Skill\n---\nPRIVATE_BODY=cross-agent-private-fixture\n").unwrap();
    let aliases = ["first-alias", "second-alias"]
        .map(|name| fixture.link(AgentCliKind::Codex, false, name, &target));
    fs::write(
        fixture.home.join(".codex/config.toml"),
        "[[skills.config]]\nname = \"physical-name\"\nenabled = false\n",
    )
    .unwrap();
    let snapshot = fixture.inventory(&[AgentCliKind::Codex], false);
    for alias in &aliases {
        assert_link_source(&snapshot, AgentCliKind::Codex, alias, AgentAssetScope::User);
    }
    let definitions = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Skill
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 3);
    assert!(definitions
        .iter()
        .all(|declaration| declaration.native_id == "physical-name"));
    assert_eq!(
        definitions
            .iter()
            .filter(|declaration| declaration.participation
                == AgentAssetResolutionParticipation::Participates)
            .count(),
        1
    );
    assert_eq!(
        definitions
            .iter()
            .filter(|declaration| matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::DuplicatePhysicalSource
                }
            ))
            .count(),
        2
    );
    let assets = rows(&snapshot, AgentCliKind::Codex, "physical-name");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].effective_state, AgentAssetState::Disabled);
    let selected_path = Path::new(assets[0].path.as_ref().unwrap());
    assert!(aliases.iter().any(|alias| alias == selected_path));
    assert!(!snapshot
        .inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn cross_agent_linked_skills_codex_identity_stays_bound_to_the_captured_bytes() {
    let fixture = Fixture::new();
    let captured = fixture.home.join(".agents/skills/captured-name");
    let replacement = fixture.home.join(".agents/skills/replacement-name");
    for target in [&captured, &replacement] {
        fixture.skill(target, "unused-name");
        fs::write(
            target.join("SKILL.md"),
            "---\ndescription: Synthetic unnamed Skill\n---\n",
        )
        .unwrap();
    }
    let alias = fixture.link(AgentCliKind::Codex, false, "retargeted-alias", &captured);
    let snapshot = fixture.inventory_with_retarget(
        &[AgentCliKind::Codex],
        false,
        Some((&alias, &replacement)),
    );
    assert_eq!(alias.canonicalize().unwrap(), replacement.join("SKILL.md"));
    let alias_source = source_at(&snapshot, AgentCliKind::Codex, &alias);
    let captured_anchor = &snapshot.source_anchors[&alias_source.id];
    assert_eq!(captured_anchor.physical_path(), captured.join("SKILL.md"));
    let alias_definition = snapshot
        .inventory
        .declarations
        .iter()
        .find(|declaration| {
            declaration.source_id == alias_source.id
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .unwrap();
    assert_eq!(alias_definition.native_id, "captured-name");
    assert_eq!(
        alias_definition.participation,
        AgentAssetResolutionParticipation::Participates
    );
    assert_winner(&snapshot, AgentCliKind::Codex, "captured-name", &alias, 1);
    assert_winner(
        &snapshot,
        AgentCliKind::Codex,
        "replacement-name",
        &replacement.join("SKILL.md"),
        1,
    );
    let original_source = source_at(&snapshot, AgentCliKind::Codex, &captured.join("SKILL.md"));
    let original_definition = snapshot
        .inventory
        .declarations
        .iter()
        .find(|declaration| {
            declaration.source_id == original_source.id
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .unwrap();
    assert_eq!(
        original_definition.participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::DuplicatePhysicalSource,
        }
    );
}

#[test]
fn cross_agent_linked_skills_bad_links_do_not_hide_healthy_native_sources() {
    let fixture = Fixture::new();
    let shared = fixture.home.join(".agents/skills");
    let healthy = shared.join("healthy");
    fixture.skill(&healthy, "healthy");
    let outside = fixture.root.join("outside");
    fixture.skill(&outside, "must-not-load-outside");
    fixture.skill(&healthy.join("nested"), "must-not-load-nested");
    fs::write(shared.join("plain-file"), "not a Skill directory").unwrap();
    symlink(&healthy, shared.join("chain")).unwrap();
    for kind in AGENTS {
        let native = fixture.home.join(directory(kind)).join("skills");
        fixture.link(kind, false, "valid-alias", &healthy);
        for (alias, target) in [
            ("outside", outside.clone()),
            ("nested", healthy.join("nested")),
            ("chain", shared.join("chain")),
            ("missing", shared.join("missing")),
            ("file-target", shared.join("plain-file")),
        ] {
            fixture.link(kind, false, alias, &target);
        }
        symlink(healthy.join("SKILL.md"), native.join("SKILL.md")).unwrap();
        fs::create_dir(native.join("leaf-link")).unwrap();
        symlink(healthy.join("SKILL.md"), native.join("leaf-link/SKILL.md")).unwrap();
    }
    let snapshot = fixture.inventory(&AGENTS, false);
    for kind in AGENTS {
        let path = fixture
            .home
            .join(directory(kind))
            .join("skills/valid-alias/SKILL.md");
        assert_link_source(&snapshot, kind, &path, AgentAssetScope::User);
        assert!(!rows(&snapshot, kind, "healthy").is_empty());
    }
    assert!(snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Skill)
        .all(|asset| asset.native_id == "healthy"));
    assert!(snapshot
        .inventory
        .declarations
        .iter()
        .filter(
            |declaration| declaration.native_kind == AgentAssetCategory::Skill
                && declaration.role == AgentAssetDeclarationRole::Definition
        )
        .all(|declaration| declaration.native_id == "healthy"));
    assert!(!snapshot
        .inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
}
