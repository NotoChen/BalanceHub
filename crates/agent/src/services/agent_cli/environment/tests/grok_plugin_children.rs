//! Nonzero production inventory/projector/catalog fixtures; never execute Grok.
mod hooks;
use super::*;
use crate::models::{
    AgentAssetDiscoveryIncompleteReason, AgentAssetDocumentFormat, AgentAssetInstallationOrigin,
    AgentAssetProviderOrigin, AgentAssetRecord, AgentAssetSuppressionReason, AgentCliDistribution,
    AgentConfigurationContext,
};
use crate::services::agent_cli::{
    catalog::CatalogService,
    environment::{
        mutation::{MutationInventory, MutationService},
        verified_path::VerifiedPathAnchor,
    },
};
use serde_json::json;

struct IsolatedSnapshots {
    root: PathBuf,
    real: RealSnapshotPort,
    reads: Mutex<Vec<PathBuf>>,
}

impl SnapshotPort for IsolatedSnapshots {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        assert!(
            request.source.path.starts_with(&self.root),
            "fixture read escaped its temporary root"
        );
        self.reads.lock().unwrap().push(request.source.path.clone());
        self.real.snapshot(request, run)
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

fn contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let mut contexts = definition(AgentCliKind::Grok)
        .environment()
        .discover_contexts(request, output);
    for context in &mut contexts {
        context.config_root = request.home.join(".grok").to_string_lossy().into_owned();
    }
    contexts
}

// Only the resolver-corruption fixture injects a context. Native inventory
// fixtures below read the real durable authority instead.
fn untrusted_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    let mut contexts = contexts(request, output);
    for context in &mut contexts {
        context.trust_context = AgentTrustState::Untrusted;
    }
    contexts
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
        fs::create_dir_all(home.join(".grok")).unwrap();
        fs::create_dir_all(workspace.join(".grok")).unwrap();
        fs::write(home.join(".grok/config.toml"), b"").unwrap();
        Self {
            _temporary: temporary,
            root,
            home,
            workspace,
        }
    }

    fn package(&self, project: bool, directory: &str) -> PathBuf {
        let base = if project { &self.workspace } else { &self.home };
        let path = base.join(".grok/plugins").join(directory);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = path.as_ref();
        assert!(path.starts_with(&self.root));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn manifest(&self, package: &Path, value: serde_json::Value) {
        self.write(
            package.join("plugin.json"),
            serde_json::to_vec(&value).unwrap(),
        );
    }

    fn skill(&self, directory: &Path, name: &str) {
        self.write(directory.join("SKILL.md"), format!("---\nname: {name}\ndescription: Synthetic Grok fixture\n---\nPrivate fixture instruction body.\n"));
    }

    fn record_workspace_trust(&self, trusted: bool) {
        super::grok::write_workspace_trust(&self.home, &self.workspace, trusted);
    }

    fn scan(&self) -> (MutationInventory, Vec<PathBuf>) {
        self.scan_with(AgentAssetLimits::DEFAULT)
    }

    fn scan_with(&self, limits: AgentAssetLimits) -> (MutationInventory, Vec<PathBuf>) {
        self.scan_with_resolver(limits, false, None)
    }

    fn scan_with_resolver(
        &self,
        limits: AgentAssetLimits,
        untrusted: bool,
        resolver: Option<crate::services::agent_cli::contracts::AgentAssetResolver>,
    ) -> (MutationInventory, Vec<PathBuf>) {
        assert!(!untrusted || resolver.is_some());
        let native = definition(AgentCliKind::Grok);
        let environment = if let Some(resolver) = resolver {
            EnvironmentAdapter::with_pipeline(
                if untrusted {
                    untrusted_contexts
                } else {
                    contexts
                },
                |request, output| {
                    definition(AgentCliKind::Grok)
                        .environment()
                        .discover_sources(request, output)
                },
                Some(|request, output| {
                    definition(AgentCliKind::Grok)
                        .environment()
                        .discover_follow_up_sources(request, output)
                }),
                "grok-parent-source-proof-fixture",
                |request, output| {
                    definition(AgentCliKind::Grok)
                        .environment()
                        .parse(request, output)
                },
                resolver,
                native.environment().state_assessor(),
            )
            .with_definition_selector(|request| {
                definition(AgentCliKind::Grok)
                    .environment()
                    .definition_suppressions(request)
            })
        } else {
            native.environment.with_test_contexts(contexts)
        };
        let definitions = [AgentCliDefinition {
            environment,
            ..*native
        }];
        let mut installation = fixture_installation();
        installation.agent_kind = AgentCliKind::Grok;
        installation.installed_version = Some("1.0.24".to_owned());
        installation.distribution = AgentCliDistribution::VendorNative;
        let (installations, _) = FakeInstallationPort::new(vec![installation]);
        let snapshots = IsolatedSnapshots {
            root: self.root.clone(),
            real: RealSnapshotPort::default(),
            reads: Mutex::new(Vec::new()),
        };
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: Some(&self.workspace),
                settings: Some(&crate::models::AppSettings::default()),
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
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        let reads = snapshots.reads.into_inner().unwrap();
        assert_eq!(
            reads.len(),
            reads.iter().collect::<BTreeSet<_>>().len(),
            "each isolated source path must reuse its first snapshot"
        );
        (
            MutationInventory {
                inventory,
                source_anchors,
            },
            reads,
        )
    }
}

fn rows(snapshot: &MutationInventory, category: AgentAssetCategory) -> Vec<&AgentAssetRecord> {
    snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == category)
        .collect()
}

fn selected<'a>(
    snapshot: &'a MutationInventory,
    category: AgentAssetCategory,
    id: &str,
) -> &'a AgentAssetRecord {
    snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.category == category
                && asset.native_id == id
                && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
        })
        .unwrap()
}

fn assert_admitted(snapshot: &MutationInventory, count: usize) {
    assert_eq!(
        snapshot.inventory.assets.len(),
        count,
        "{:#?}",
        snapshot.inventory.diagnostics
    );
    super::assert_no_structural_projection_diagnostics(&snapshot.inventory);
}

fn all_diagnostics(snapshot: &MutationInventory) -> Vec<&AgentAssetDiagnostic> {
    snapshot
        .inventory
        .diagnostics
        .iter()
        .chain(
            snapshot
                .inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics),
        )
        .chain(
            snapshot
                .inventory
                .declarations
                .iter()
                .flat_map(|declaration| &declaration.diagnostics),
        )
        .chain(snapshot.inventory.assets.iter().flat_map(|asset| {
            asset
                .diagnostics
                .iter()
                .chain(&asset.resolution.diagnostics)
        }))
        .collect()
}

#[test]
fn grok_each_manifest_layout_proves_namespace_without_claiming_enabled() {
    for (rank, layout) in [
        "plugin.json",
        ".grok-plugin/plugin.json",
        ".claude-plugin/plugin.json",
    ]
    .iter()
    .enumerate()
    {
        let fixture = Fixture::new();
        let package = fixture.package(false, "Different_Directory");
        fixture.write(
            package.join(layout),
            br#"{"name":"actual-namespace","author":{"name":"Official-looking text"}}"#,
        );
        fixture.write(
            fixture.home.join(".grok/config.toml"),
            b"[plugins]\nenabled=['actual-namespace']\ndisabled=['actual-namespace']\n",
        );
        let (snapshot, _) = fixture.scan();
        assert_admitted(&snapshot, 1);
        assert_eq!(snapshot.inventory.declarations.len(), rank + 5);
        let plugin = selected(&snapshot, AgentAssetCategory::Plugin, "actual-namespace");
        assert_eq!(plugin.declared_state, AgentAssetState::Unknown);
        assert_eq!(plugin.effective_state, AgentAssetState::Unknown);
        assert!(matches!(
            plugin.details,
            AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                enabled: AgentAssetDeclaredState::Unknown,
                ..
            }
        ));
        assert_eq!(plugin.provenance.len(), 1);
        assert_eq!(
            plugin.provenance[0].provider,
            AgentAssetProviderOrigin::Unknown
        );
        assert_eq!(
            plugin.provenance[0].installation,
            AgentAssetInstallationOrigin::NativePackage
        );
        assert!(all_diagnostics(&snapshot).iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: AgentCliKind::Grok,
                category: AgentAssetCategory::Plugin,
                reason: AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved
            }
        )));
    }
}

#[test]
fn grok_only_recognized_components_prove_manifestless_packages() {
    for (component, directory, installed) in [
        ("", false, false),
        ("README.md", false, false),
        ("random.json", false, false),
        ("skills", true, true),
        ("commands", true, true),
        ("agents", true, true),
        (".mcp.json", false, true),
        (".lsp.json", false, true),
        ("hooks/hooks.json", false, true),
    ] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "__Fallback_Name__");
        if directory {
            fs::create_dir_all(package.join(component)).unwrap();
        } else if !component.is_empty() {
            fixture.write(package.join(component), b"{\"mcpServers\":{}}");
        }
        let (snapshot, _) = fixture.scan();
        assert_admitted(&snapshot, usize::from(installed));
        assert!(rows(&snapshot, AgentAssetCategory::Skill).is_empty());
        assert!(rows(&snapshot, AgentAssetCategory::Mcp).is_empty());
        if installed {
            assert_eq!(snapshot.inventory.assets[0].native_id, "fallback-name");
        }
    }
}

#[test]
fn grok_invalid_first_manifest_stops_lower_manifests_and_conventions() {
    for invalid in [
        "{",
        r#"{"name":"UPPER"}"#,
        r#"{"name":"-bad"}"#,
        r#"{"name":"okay","skills":[1]}"#,
        r#"{"name":"okay","author":"wrong-type"}"#,
        r#"{"name":"first","name":"second","skills":["skills"]}"#,
        r#"{"name":"bad","skills":["unused"],"skills":["skills"]}"#,
    ] {
        let fixture = Fixture::new();
        let bad = fixture.package(false, "bad");
        fixture.write(bad.join("plugin.json"), invalid);
        fixture.write(
            bad.join(".grok-plugin/plugin.json"),
            br#"{"name":"fallback-must-not-load"}"#,
        );
        fixture.skill(&bad.join("skills/decoy"), "decoy");
        let good = fixture.package(false, "good");
        fixture.manifest(&good, json!({"name":"good"}));
        fixture.skill(&good.join("skills/safe"), "safe");
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, 2);
        assert_eq!(rows(&snapshot, AgentAssetCategory::Plugin).len(), 1);
        assert_eq!(
            rows(&snapshot, AgentAssetCategory::Plugin)[0].native_id,
            "good"
        );
        assert_eq!(
            selected(&snapshot, AgentAssetCategory::Skill, "good:safe").effective_state,
            AgentAssetState::Unknown
        );
        assert_eq!(
            reads
                .iter()
                .filter(|path| path.starts_with(&bad))
                .cloned()
                .collect::<Vec<_>>(),
            vec![bad.clone(), bad.join("plugin.json")]
        );
        assert!(all_diagnostics(&snapshot).iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Manifest,
                ..
            }
        )));
    }
}

#[test]
fn grok_project_then_canonical_root_selects_only_one_parent_occurrence_for_children() {
    let fixture = Fixture::new();
    for (project, directory, skill) in [
        (false, "user", "user-child"),
        (true, "b-root", "loser-child"),
        (true, "a-root", "selected-child"),
    ] {
        let package = fixture.package(project, directory);
        fixture.manifest(&package, json!({"name":"same"}));
        fixture.skill(&package.join("skills").join(skill), skill);
    }
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 4);
    assert_eq!(rows(&snapshot, AgentAssetCategory::Plugin).len(), 3);
    assert_eq!(rows(&snapshot, AgentAssetCategory::Skill).len(), 1);
    let parent = selected(&snapshot, AgentAssetCategory::Plugin, "same");
    assert_eq!(parent.scope, AgentAssetScope::Workspace);
    assert_eq!(
        parent.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    let source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == parent.inspection_source_id)
        .unwrap();
    assert!(Path::new(&source.path).starts_with(fixture.workspace.join(".grok/plugins/a-root")));
    let child = selected(&snapshot, AgentAssetCategory::Skill, "same:selected-child");
    assert_eq!(
        child.relationships.provided_by.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(
        child.relationships.action_owner,
        child.relationships.provided_by
    );
    assert_eq!(child.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        child.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(!reads.iter().any(
        |path| path.ends_with("user-child/SKILL.md") || path.ends_with("loser-child/SKILL.md")
    ));
}

#[test]
fn grok_skill_overrides_include_root_and_six_descendant_levels_and_flat_commands() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "package");
    fixture.manifest(
        &package,
        json!({"name":"pack","skills":["custom"],"commands":"cmd-alt"}),
    );
    fixture.skill(&package.join("skills/default-decoy"), "default-decoy");
    fixture.skill(&package.join("custom"), "中文显示名");
    let mut directory = package.join("custom");
    for depth in 1..=7 {
        directory.push(format!("level-{depth}"));
        fixture.skill(&directory, "display-only");
    }
    fixture.write(
        package.join("cmd-alt/Do_Work.md"),
        b"---\nname: command-label\n---\nCommand body.\n",
    );
    fixture.write(
        package.join("cmd-alt/nested/ignored.md"),
        b"Nested command is not loaded.",
    );
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 9);
    let skills = rows(&snapshot, AgentAssetCategory::Skill);
    assert_eq!(skills.len(), 8);
    assert_eq!(
        selected(&snapshot, AgentAssetCategory::Skill, "pack:custom").label,
        "中文显示名"
    );
    assert_eq!(
        selected(&snapshot, AgentAssetCategory::Skill, "pack:do-work").label,
        "command-label"
    );
    for depth in 1..=6 {
        selected(
            &snapshot,
            AgentAssetCategory::Skill,
            &format!("pack:level-{depth}"),
        );
    }
    assert!(!reads.iter().any(|path| path.ends_with("level-7/SKILL.md")
        || path.ends_with("nested/ignored.md")
        || path.ends_with("default-decoy/SKILL.md")));
}

#[test]
fn grok_qualified_skill_and_standalone_coexist_and_bare_control_disables_both() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(&package, json!({"name":"pack"}));
    fixture.skill(&package.join("skills/tool"), "plugin-label");
    fixture.skill(&fixture.home.join(".grok/skills/tool"), "tool");
    fixture.write(
        fixture.home.join(".grok/config.toml"),
        b"[skills]\ndisabled=['tool']\n",
    );
    let (snapshot, _) = fixture.scan();
    assert_eq!(snapshot.inventory.assets.len(), 3);
    let mut collisions = Vec::new();
    for diagnostic in all_diagnostics(&snapshot) {
        match diagnostic {
            AgentAssetDiagnostic::DuplicateNativeId {
                category,
                native_id,
            } => {
                collisions.push((*category, native_id.as_str()));
            }
            AgentAssetDiagnostic::InvalidCompatibleInstallation { .. }
            | AgentAssetDiagnostic::InvalidProjection { .. }
            | AgentAssetDiagnostic::InvalidResolution { .. } => {
                panic!("unexpected structural diagnostic: {diagnostic:?}");
            }
            _ => {}
        }
    }
    collisions.sort();
    assert_eq!(
        collisions,
        [
            (AgentAssetCategory::Skill, "pack:tool"),
            (AgentAssetCategory::Skill, "tool"),
        ]
    );
    assert_eq!(rows(&snapshot, AgentAssetCategory::Skill).len(), 2);
    for id in ["tool", "pack:tool"] {
        let skill = selected(&snapshot, AgentAssetCategory::Skill, id);
        assert_eq!(skill.declared_state, AgentAssetState::Disabled);
        assert_eq!(skill.effective_state, AgentAssetState::Disabled);
        assert!(skill.resolution.qualified_collision);
        assert_eq!(skill.resolution.terminal, None);
        assert_eq!(skill.represented_declaration_ids.len(), 2);
    }
    assert!(selected(&snapshot, AgentAssetCategory::Skill, "tool")
        .relationships
        .provided_by
        .is_none());
    assert!(selected(&snapshot, AgentAssetCategory::Skill, "pack:tool")
        .relationships
        .provided_by
        .is_some());
}

#[test]
fn grok_mcp_file_precedes_inline_and_native_disabled_toml_still_claims_name() {
    let fixture = Fixture::new();
    let alpha = fixture.package(false, "alpha");
    fixture.manifest(&alpha, json!({"name":"alpha","mcpServers":{"same":{"url":"https://inline.invalid/mcp"},"inline-only":{"command":"inline-private"}}}));
    fixture.write(alpha.join(".mcp.json"), br#"{"mcpServers":{"same":{"command":"file-private"},"claimed":{"command":"child-private"}}}"#);
    let beta = fixture.package(false, "beta");
    fixture.manifest(&beta, json!({"name":"beta"}));
    fixture.write(beta.join(".mcp.json"), br#"{"mcpServers":{"same":{"url":"https://beta.invalid/sse"},"beta-only":{"command":"beta-private"}}}"#);
    fixture.write(
        fixture.home.join(".grok/config.toml"),
        b"[mcp_servers.claimed]\ncommand='native-private'\nenabled=false\n",
    );
    let (snapshot, _) = fixture.scan();
    assert_admitted(&snapshot, 9);
    assert_eq!(rows(&snapshot, AgentAssetCategory::Mcp).len(), 7);
    let same = selected(&snapshot, AgentAssetCategory::Mcp, "same");
    assert!(matches!(
        same.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    ));
    assert_eq!(
        same.relationships.provided_by.as_deref(),
        Some(
            selected(&snapshot, AgentAssetCategory::Plugin, "alpha")
                .stable_id
                .as_str()
        )
    );
    assert_eq!(
        same.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert_eq!(
        snapshot
            .inventory
            .assets
            .iter()
            .filter(|asset| asset.native_id == "same")
            .count(),
        3
    );
    let claimed = selected(&snapshot, AgentAssetCategory::Mcp, "claimed");
    assert_eq!(claimed.effective_state, AgentAssetState::Disabled);
    assert!(claimed.relationships.provided_by.is_none());
    assert_eq!(
        claimed.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert!(rows(&snapshot, AgentAssetCategory::Mcp)
        .iter()
        .all(|asset| !asset.native_id.starts_with("plugin:")));
}

#[test]
fn grok_mcp_path_replaces_default_and_bad_file_entries_do_not_poison_siblings() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(
        &package,
        json!({"name":"pack","mcpServers":"config/servers.json"}),
    );
    fixture.write(
        package.join(".mcp.json"),
        br#"{"mcpServers":{"decoy":{"command":"decoy"}}}"#,
    );
    fixture.write(
        package.join("config/servers.json"),
        br#"{"mcpServers":{"valid":{"url":"https://valid.invalid/mcp"},"bad":{"command":42}}}"#,
    );
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 2);
    assert_eq!(rows(&snapshot, AgentAssetCategory::Mcp).len(), 1);
    selected(&snapshot, AgentAssetCategory::Mcp, "valid");
    assert!(!reads.contains(&package.join(".mcp.json")));
    assert!(all_diagnostics(&snapshot)
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
}

#[test]
fn grok_mcp_inline_wrapped_and_bare_maps_load_but_invalid_inline_is_atomic() {
    for wrapped in [false, true] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "pack");
        let entries = json!({"inline":{"command":"private-command"}});
        fixture.manifest(&package, json!({"name":"pack","mcpServers":if wrapped { json!({"mcpServers":entries}) } else { entries }}));
        let (snapshot, _) = fixture.scan();
        assert_admitted(&snapshot, 2);
        let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
        let child = selected(&snapshot, AgentAssetCategory::Mcp, "inline");
        assert_eq!(parent.inspection_source_id, child.inspection_source_id);
        fixture.manifest(&package, json!({"name":"pack","mcpServers":{"valid":{"command":"private-command"},"bad":{"args":[]}}}));
        fixture.write(
            package.join(".mcp.json"),
            br#"{"mcpServers":{"file-survivor":{"command":"file-private"}}}"#,
        );
        let (snapshot, _) = fixture.scan();
        assert_admitted(&snapshot, 2);
        assert_eq!(rows(&snapshot, AgentAssetCategory::Mcp).len(), 1);
        selected(&snapshot, AgentAssetCategory::Mcp, "file-survivor");
    }
}

#[test]
fn grok_mcp_convention_file_is_one_source_for_parent_and_children() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    let path = package.join(".mcp.json");
    fixture.write(
        &path,
        br#"{"mcpServers":{"server":{"command":"fixture-command"}}}"#,
    );
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 2);
    let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
    let child = selected(&snapshot, AgentAssetCategory::Mcp, "server");
    assert_eq!(parent.inspection_source_id, child.inspection_source_id);
    assert_eq!(child.declared_state, AgentAssetState::Enabled);
    assert_eq!(child.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        child.relationships.provided_by.as_ref(),
        Some(&parent.stable_id)
    );
    assert_eq!(reads.iter().filter(|read| *read == &path).count(), 1);
}

#[test]
fn grok_shared_skill_and_command_directory_reuses_one_snapshot() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(
        &package,
        json!({"name":"pack","skills":["entries"],"commands":["entries"]}),
    );
    let directory = package.join("entries");
    fixture.skill(&directory.join("tool"), "tool");
    fixture.write(directory.join("slash.md"), b"A synthetic command body.\n");
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 3);
    assert_eq!(
        rows(&snapshot, AgentAssetCategory::Skill)
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["pack:slash", "pack:tool"])
    );
    assert_eq!(reads.iter().filter(|read| *read == &directory).count(), 1);
}

#[test]
fn grok_one_skill_file_retains_skill_command_and_explicit_mcp_roles() {
    for with_mcp in [false, true] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "physical-package");
        let mut manifest = json!({
            "name":"pack", "skills":["entries"], "commands":["entries"]
        });
        if with_mcp {
            manifest["mcpServers"] = json!("entries/SKILL.md");
        }
        fixture.manifest(&package, manifest);
        let file = package.join("entries/SKILL.md");
        fixture.write(
            &file,
            br#"{"mcpServers":{"co-file":{"command":"fixture-command"}}}"#,
        );
        let (snapshot, reads) = fixture.scan();
        assert_admitted(&snapshot, if with_mcp { 4 } else { 3 });
        let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
        let skill = selected(&snapshot, AgentAssetCategory::Skill, "pack:entries");
        let command = selected(&snapshot, AgentAssetCategory::Skill, "pack:skill");
        assert_eq!(skill.label, "entries");
        assert_eq!(command.label, "skill");
        assert_eq!(skill.inspection_source_id, command.inspection_source_id);
        for child in [skill, command] {
            assert_eq!(child.declared_state, AgentAssetState::Enabled);
            assert_eq!(child.effective_state, AgentAssetState::Unknown);
            assert_eq!(child.scope, AgentAssetScope::User);
            assert_eq!(
                child.relationships.provided_by.as_ref(),
                Some(&parent.stable_id)
            );
            assert_eq!(child.represented_declaration_ids.len(), 1);
        }
        let declarations = snapshot
            .inventory
            .declarations
            .iter()
            .filter(|asset| asset.native_kind == AgentAssetCategory::Skill)
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 2);
        assert_eq!(
            declarations
                .iter()
                .map(|asset| asset.declaration_key.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["SKILL.md:0", "command:0"])
        );
        assert_eq!(declarations[0].source_id, declarations[1].source_id);
        if with_mcp {
            let mcp = selected(&snapshot, AgentAssetCategory::Mcp, "co-file");
            assert_eq!(mcp.inspection_source_id, skill.inspection_source_id);
            assert_eq!(mcp.declared_state, AgentAssetState::Enabled);
            assert_eq!(mcp.effective_state, AgentAssetState::Unknown);
            assert_eq!(
                mcp.relationships.provided_by.as_ref(),
                Some(&parent.stable_id)
            );
        } else {
            assert!(rows(&snapshot, AgentAssetCategory::Mcp).is_empty());
        }
        assert_eq!(reads.iter().filter(|path| **path == file).count(), 1);
    }
}

fn verify_overlapping_skill_role_priority(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    use crate::services::agent_cli::contracts::{
        AgentAssetAssessmentRequest, AgentAssetAssessmentResult, AgentAssetAssessmentSubject,
        AgentAssetAssessmentTarget,
    };
    let native = super::grok_proof::native_resolve(request);
    let expected = request
        .declarations
        .iter()
        .find(|asset| asset.native_id == "pack:tool" && asset.declaration_key == "SKILL.md:0")
        .unwrap();
    let target = AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Skill,
        resolution_group_key: "tool".to_owned(),
        exact_native_id: "pack:tool".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let targets = [target.clone()];
    let mut sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let mut declarations = request.declarations.to_vec();
    for reversed in [false, true] {
        if reversed {
            sources.reverse();
            declarations.reverse();
        }
        let assessment = definition(AgentCliKind::Grok)
            .environment()
            .state_assessor()(AgentAssetAssessmentRequest {
            context: request.context,
            targets: &targets,
            declarations: &declarations,
            sources: &sources,
        });
        let Some(AgentAssetAssessmentResult::Assessed { projection, .. }) = assessment.get(&target)
        else {
            panic!("complete overlapping Skill roles must be assessed");
        };
        assert_eq!(projection.anchor_declaration_id, expected.declaration_id);
        assert_eq!(
            projection.relation,
            AgentAssetResolutionRelation::ReplaceWinner
        );
    }
    super::grok_proof::emit_captured(native, output);
}

#[test]
fn grok_overlapping_skill_roots_preserve_fallback_names_and_root_precedence() {
    for reverse_roots in [false, true] {
        let fixture = Fixture::new();
        let package = fixture.package(false, "physical-package");
        let roots = if reverse_roots {
            json!(["skills/tool", "skills"])
        } else {
            json!(["skills", "skills/tool"])
        };
        fixture.manifest(
            &package,
            json!({"name":"pack", "skills":roots, "commands":[]}),
        );
        let file = package.join("skills/tool/SKILL.md");
        fixture.write(&file, b"A synthetic Skill without a frontmatter name.\n");
        let (snapshot, reads) = fixture.scan_with_resolver(
            AgentAssetLimits::DEFAULT,
            false,
            Some(verify_overlapping_skill_role_priority),
        );
        assert_admitted(&snapshot, 3);
        let parent = selected(&snapshot, AgentAssetCategory::Plugin, "pack");
        let selected = selected(&snapshot, AgentAssetCategory::Skill, "pack:tool");
        let skills = rows(&snapshot, AgentAssetCategory::Skill);
        assert_eq!(skills.len(), 2);
        assert_eq!(
            selected.resolution.relation,
            AgentAssetResolutionRelation::ReplaceWinner
        );
        assert_eq!(selected.represented_declaration_ids.len(), 2);
        assert_eq!(selected.declared_state, AgentAssetState::Enabled);
        assert_eq!(selected.effective_state, AgentAssetState::Unknown);
        for asset in skills {
            assert_eq!(asset.native_id, "pack:tool");
            assert_eq!(asset.label, "tool");
            assert_eq!(asset.inspection_source_id, selected.inspection_source_id);
            assert_eq!(
                asset.relationships.provided_by.as_ref(),
                Some(&parent.stable_id)
            );
        }
        let declarations = snapshot
            .inventory
            .declarations
            .iter()
            .filter(|asset| asset.native_kind == AgentAssetCategory::Skill)
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].source_id, declarations[1].source_id);
        assert_eq!(
            declarations
                .iter()
                .map(|asset| asset.declaration_key.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["SKILL.md:0", "SKILL.md:1"])
        );
        assert_eq!(reads.iter().filter(|path| **path == file).count(), 1);
    }
}

#[test]
fn grok_instruction_skill_does_not_inherit_mcp_executable_trust_and_children_stay_read_only() {
    let fixture = Fixture::new();
    fixture.record_workspace_trust(false);
    let package = fixture.package(true, "pack");
    fixture.manifest(&package, json!({"name":"pack"}));
    fixture.skill(&package.join("skills/tool"), "tool");
    fixture.write(package.join(".mcp.json"), br#"{"mcpServers":{"server":{"command":"${GROK_PLUGIN_ROOT}/private","env":{"TOKEN":"private-fixture-secret"}}}}"#);
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 3);
    assert_eq!(
        snapshot.inventory.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    assert!(reads.contains(&fixture.home.join(".grok/trusted_folders.toml")));
    let skill = selected(&snapshot, AgentAssetCategory::Skill, "pack:tool");
    assert_eq!(skill.declared_state, AgentAssetState::Enabled);
    assert_eq!(
        skill.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    let mcp = selected(&snapshot, AgentAssetCategory::Mcp, "server");
    assert!(matches!(
        mcp.details,
        AgentAssetDetails::Mcp {
            declared_state: AgentAssetDeclaredState::Enabled,
            effective_availability: AgentAssetEffectiveAvailability::Unknown,
            ..
        }
    ));
    // A durable false record cannot observe the native runtime decision. The
    // unresolved package parent remains the exact cause of both child states.
    assert_eq!(
        mcp.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert_eq!(mcp.trust_state, AgentTrustState::Unknown);
    assert_eq!(mcp.declared_state, AgentAssetState::Enabled);
    assert_eq!(mcp.effective_state, AgentAssetState::Unknown);
    for asset in [skill, mcp] {
        assert!(!asset.writable);
        assert_eq!(
            asset.relationships.action_owner,
            asset.relationships.provided_by
        );
        assert!(asset
            .actions
            .iter()
            .filter(|action| matches!(
                action.action,
                AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
            ))
            .all(|action| !action.available));
    }
    let serialized = serde_json::to_string(&snapshot.inventory).unwrap();
    assert!(!serialized.contains("private-fixture-secret"));
    assert!(!serialized.contains("Private fixture instruction body"));
    assert!(!serialized.contains("${GROK_PLUGIN_ROOT}"));
    let catalog = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    )
    .catalog(&snapshot)
    .unwrap();
    let children = catalog
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .filter(|binding| binding.native.relationships.provided_by.is_some())
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|binding| !binding.can_adopt));
}

fn attack_untrusted_parent_candidate(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
    omit: bool,
) {
    use crate::services::agent_cli::contracts::AgentAssetEffectiveStateProofDraft;
    let mut native = super::grok_proof::native_resolve(request);
    assert_eq!(request.context.trust_context, AgentTrustState::Untrusted);
    assert_eq!(native.drafts.len(), 4);
    let child = native
        .drafts
        .iter()
        .find(|asset| asset.native_id == "server")
        .unwrap();
    assert_eq!(child.declared_state, AgentAssetDeclaredState::Enabled);
    assert_eq!(child.effective_state, AgentAssetState::Unknown);
    assert_eq!(child.resolution.terminal, None);
    assert!(matches!(
        child.state_proof.effective,
        AgentAssetEffectiveStateProofDraft::Intrinsic
    ));
    assert!(child.provided_by.is_some());
    assert_eq!(child.action_owner, child.provided_by);
    assert!(matches!(
        child.details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    if omit {
        native
            .drafts
            .retain(|asset| asset.native_kind != AgentAssetCategory::Plugin);
        assert_eq!(native.drafts.len(), 3);
    } else {
        native
            .drafts
            .iter_mut()
            .find(|asset| {
                asset.native_kind == AgentAssetCategory::Plugin && asset.native_id == "pack"
            })
            .unwrap()
            .label = "forged parent candidate".to_owned();
    }
    super::grok_proof::emit_captured(native, output);
}

#[test]
fn grok_untrusted_children_require_a_surviving_parent_candidate() {
    use crate::services::agent_cli::contracts::AgentAssetResolver;
    let attacks: [AgentAssetResolver; 2] = [
        |request, output| attack_untrusted_parent_candidate(request, output, true),
        |request, output| attack_untrusted_parent_candidate(request, output, false),
    ];
    for attack in attacks {
        let fixture = Fixture::new();
        let package = fixture.package(true, "pack");
        fixture.manifest(&package, json!({"name":"pack"}));
        fixture.skill(&package.join("skills/tool"), "tool");
        fixture.write(
            package.join(".mcp.json"),
            br#"{"mcpServers":{"server":{"command":"fixture-command"}}}"#,
        );
        fixture.skill(&fixture.home.join(".grok/skills/healthy"), "healthy");
        let (snapshot, _) =
            fixture.scan_with_resolver(AgentAssetLimits::DEFAULT, true, Some(attack));
        assert_eq!(snapshot.inventory.assets.len(), 1);
        let healthy = selected(&snapshot, AgentAssetCategory::Skill, "healthy");
        assert_eq!(healthy.declared_state, AgentAssetState::Enabled);
        assert_eq!(healthy.effective_state, AgentAssetState::Enabled);
        assert!(healthy.relationships.provided_by.is_none());
        for native_id in ["pack", "pack:tool", "server"] {
            assert_eq!(
                snapshot
                    .inventory
                    .declarations
                    .iter()
                    .filter(|asset| asset.native_id == native_id)
                    .count(),
                1
            );
            assert!(!snapshot
                .inventory
                .assets
                .iter()
                .any(|asset| asset.native_id == native_id));
        }
    }
}

#[test]
fn grok_component_dot_paths_are_bounded_and_parent_components_are_reported() {
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(&package, json!({"name":"pack","skills":[".","../escape","nested/../safe"],"commands":[],"mcpServers":"../private.json"}));
    fixture.skill(&package, "root-label");
    fixture.write(
        fixture.home.join(".grok/plugins/private.json"),
        br#"{"mcpServers":{"must-not-read":{"command":"private"}}}"#,
    );
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 2);
    selected(&snapshot, AgentAssetCategory::Skill, "pack:pack");
    assert!(!reads.contains(&fixture.home.join(".grok/plugins/private.json")));
    assert!(all_diagnostics(&snapshot).iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::DiscoveryIncomplete {
            agent_kind: AgentCliKind::Grok,
            reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            ..
        }
    )));
}

#[cfg(unix)]
#[test]
fn grok_package_and_component_symlinks_never_borrow_target_evidence() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let package = fixture.package(false, "pack");
    fixture.manifest(
        &package,
        json!({"name":"pack","skills":["safe","linked"],"mcpServers":"linked.json"}),
    );
    fixture.skill(&package.join("safe"), "safe");
    let outside = fixture.root.join("outside");
    fixture.skill(&outside, "outside");
    fixture.write(
        outside.join("mcp.json"),
        br#"{"mcpServers":{"outside":{"command":"private"}}}"#,
    );
    symlink(&outside, package.join("linked")).unwrap();
    symlink(outside.join("mcp.json"), package.join("linked.json")).unwrap();
    symlink(&package, fixture.home.join(".grok/plugins/alias")).unwrap();
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 2);
    selected(&snapshot, AgentAssetCategory::Skill, "pack:safe");
    assert!(rows(&snapshot, AgentAssetCategory::Mcp).is_empty());
    assert!(!reads.iter().any(|path| path.starts_with(&outside)));
    assert!(all_diagnostics(&snapshot)
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::SymlinkRejected { .. })));
}

#[test]
fn grok_package_seed_discovery_stops_at_the_shared_source_budget() {
    let fixture = Fixture::new();
    for index in 0..12 {
        let package = fixture.package(false, &format!("package-{index:02}"));
        fixture.manifest(&package, json!({"name":format!("package-{index:02}")}));
        fixture.skill(&package.join("skills/tool"), "tool");
    }
    // The outside-HOME workspace adds five native authority sources and the
    // fixed workspace Hook file adds one to the original package-seed fixture.
    let source_limit = 24;
    let (snapshot, reads) = fixture.scan_with(AgentAssetLimits {
        sources_per_context: source_limit,
        ..AgentAssetLimits::DEFAULT
    });
    assert!(!snapshot.inventory.declarations.is_empty());
    assert!(snapshot.inventory.sources.len() <= source_limit);
    assert!(reads.len() <= source_limit);
    let authority = BTreeSet::from([
        fixture.home.join(".grok/trusted_folders.toml"),
        fixture.workspace.clone(),
        fixture.workspace.join(".git"),
        fixture.workspace.join(".git/HEAD"),
        fixture.workspace.join(".git/config"),
    ]);
    let sources = snapshot
        .inventory
        .sources
        .iter()
        .map(|source| PathBuf::from(&source.path))
        .collect::<BTreeSet<_>>();
    assert!(authority.is_subset(&sources));
    assert!(authority.is_subset(&reads.iter().cloned().collect()));
    assert!(reads
        .iter()
        .any(|path| path.ends_with("plugins/package-00/plugin.json")));
    assert!(all_diagnostics(&snapshot)
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Truncated { .. })));
    assert!(!reads
        .iter()
        .any(|path| path.ends_with("skills/tool/SKILL.md")));
}

#[test]
fn grok_observed_uncovered_entrypoints_have_explicit_diagnostics() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.home.join(".grok/bundled/skills")).unwrap();
    fs::create_dir_all(fixture.home.join(".grok/bundled/plugins")).unwrap();
    fixture.write(fixture.home.join(".grok/config.toml"), b"[skills]\npaths=['/unread/skill-root']\n[plugins]\npaths=['/unread/plugin-root']\n[compat.claude]\nmcps=true\n");
    let (snapshot, reads) = fixture.scan();
    assert_admitted(&snapshot, 0);
    for category in [
        AgentAssetCategory::Skill,
        AgentAssetCategory::Plugin,
        AgentAssetCategory::Mcp,
    ] {
        assert!(all_diagnostics(&snapshot).iter().any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::DiscoveryIncomplete { agent_kind: AgentCliKind::Grok, category: actual, reason: AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint } if *actual == category)));
    }
    assert!(reads.iter().all(|path| path.starts_with(&fixture.root)));
    assert!(!reads
        .iter()
        .any(|path| path.ends_with("bundled/skills") || path.ends_with("bundled/plugins")));
}

fn verify_parent_source_rejection(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    use crate::services::agent_cli::contracts::{
        AgentAssetAssessmentRequest, AgentAssetAssessmentResult, AgentAssetAssessmentSubject,
        AgentAssetAssessmentTarget,
    };
    let native = super::grok_proof::native_resolve(request);
    let child = native
        .drafts
        .iter()
        .find(|draft| draft.native_id == "pack:tool")
        .unwrap();
    let target = AgentAssetAssessmentTarget {
        category: child.native_kind,
        resolution_group_key: child.resolution_group_key.clone(),
        exact_native_id: child.native_id.clone(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let assessor = definition(AgentCliKind::Grok)
        .environment()
        .state_assessor();
    let targets = [target.clone()];
    let baseline = assessor(AgentAssetAssessmentRequest {
        context: request.context,
        targets: &targets,
        declarations: request.declarations,
        sources: &sources,
    });
    assert!(matches!(
        baseline.get(&target),
        Some(AgentAssetAssessmentResult::Assessed { .. })
    ));
    let workspace = Path::new(request.context.workspace_id.as_deref().unwrap());
    let foreign = workspace.with_file_name("foreign-workspace");
    let mut forged_sources = sources.clone();
    for source in forged_sources
        .iter_mut()
        .filter(|source| source.scope == AgentAssetScope::Workspace)
    {
        source.path = foreign.join(source.path.strip_prefix(workspace).unwrap());
        source.allowed_root = foreign.join(source.allowed_root.strip_prefix(workspace).unwrap());
        source.verified_physical_path = Some(source.path.clone());
    }
    let forged = assessor(AgentAssetAssessmentRequest {
        context: request.context,
        targets: &targets,
        declarations: request.declarations,
        sources: &forged_sources,
    });
    assert!(matches!(
        forged.get(&target),
        Some(AgentAssetAssessmentResult::Unsupported(_))
    ));
    for remove_parent in [false, true] {
        let mut declarations = request.declarations.to_vec();
        if remove_parent {
            declarations.retain(|asset| {
                !(asset.category == AgentAssetCategory::Plugin
                    && asset.role == AgentAssetDeclarationRole::Definition)
            });
        } else {
            let child = declarations
                .iter_mut()
                .find(|asset| asset.native_id == "pack:tool")
                .unwrap();
            child.provided_by.as_mut().unwrap().qualifier =
                Some("plugin:wrong-occurrence".to_owned());
            child.action_owner = child.provided_by.clone();
        }
        let forged = assessor(AgentAssetAssessmentRequest {
            context: request.context,
            targets: &targets,
            declarations: &declarations,
            sources: &sources,
        });
        assert!(matches!(
            forged.get(&target),
            Some(AgentAssetAssessmentResult::Unsupported(_))
        ));
    }
    super::grok_proof::emit_captured(native, output);
}

#[test]
fn grok_independent_assessment_rejects_wrong_workspace_and_missing_or_forged_parent() {
    let fixture = Fixture::new();
    let package = fixture.package(true, "pack");
    fixture.manifest(&package, json!({"name":"pack"}));
    fixture.skill(&package.join("skills/tool"), "tool");
    let (snapshot, _) = fixture.scan_with_resolver(
        AgentAssetLimits::DEFAULT,
        false,
        Some(verify_parent_source_rejection),
    );
    assert_admitted(&snapshot, 2);
}

fn verify_parent_suppression_rejection(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    use crate::services::agent_cli::contracts::{
        AgentAssetAssessmentFailure, AgentAssetAssessmentRequest, AgentAssetAssessmentResult,
        AgentAssetAssessmentSubject, AgentAssetAssessmentTarget, AgentDefinitionSelectionRequest,
    };
    let native = super::grok_proof::native_resolve(request);
    let target = AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Mcp,
        resolution_group_key: "same-server".to_owned(),
        exact_native_id: "same-server".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let environment = definition(AgentCliKind::Grok).environment();
    let healthy = AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Mcp,
        resolution_group_key: "healthy-server".to_owned(),
        exact_native_id: "healthy-server".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let targets = [target.clone(), healthy.clone()];
    let assessor = environment.state_assessor();
    let baseline = assessor(AgentAssetAssessmentRequest {
        context: request.context,
        targets: &targets,
        declarations: request.declarations,
        sources: &sources,
    });
    assert!(matches!(
        baseline.get(&target),
        Some(AgentAssetAssessmentResult::Assessed { .. })
    ));
    assert!(matches!(
        baseline.get(&healthy),
        Some(AgentAssetAssessmentResult::Assessed { .. })
    ));
    let winner = request
        .declarations
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.native_id == "same-server"
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .unwrap();
    let loser = request
        .declarations
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.native_id == "same-server"
                && asset.participation
                    == (AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::ParentNotSelected,
                    })
        })
        .unwrap();
    let suppressions = environment.definition_suppressions(AgentDefinitionSelectionRequest {
        context: request.context,
        sources: &sources,
        declarations: request.declarations,
    });
    assert_eq!(suppressions.len(), 2);
    assert!(suppressions.iter().all(|suppression| {
        suppression.reason == AgentAssetSuppressionReason::ParentNotSelected
            && request.declarations.iter().any(|asset| {
                asset.declaration_id == suppression.declaration_id
                    && asset.source_key == loser.source_key
                    && asset.category == AgentAssetCategory::Mcp
            })
    }));
    for attack in [
        "winner-suppressed",
        "loser-active",
        "wrong-reason",
        "missing-winner",
        "missing-loser",
        "missing-winner-child",
        "missing-loser-child",
        "duplicate-loser-child",
    ] {
        let mut declarations = request.declarations.to_vec();
        match attack {
            "missing-winner-child" | "missing-loser-child" => {
                let id = if attack == "missing-winner-child" {
                    &winner.declaration_id
                } else {
                    &loser.declaration_id
                };
                declarations.retain(|asset| &asset.declaration_id != id);
                assert_eq!(declarations.len() + 1, request.declarations.len());
            }
            "duplicate-loser-child" => declarations.push(loser.clone()),
            "missing-winner" | "missing-loser" => {
                let parent_source = if attack == "missing-winner" {
                    &winner.source_key
                } else {
                    &loser.source_key
                };
                declarations.retain(|asset| {
                    !(asset.category == AgentAssetCategory::Plugin
                        && asset.role == AgentAssetDeclarationRole::Definition
                        && &asset.source_key == parent_source)
                });
            }
            _ => {
                let id = if attack == "winner-suppressed" {
                    &winner.declaration_id
                } else {
                    &loser.declaration_id
                };
                declarations
                    .iter_mut()
                    .find(|asset| &asset.declaration_id == id)
                    .unwrap()
                    .participation = match attack {
                    "winner-suppressed" => AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::ParentNotSelected,
                    },
                    "loser-active" => AgentAssetResolutionParticipation::Participates,
                    _ => AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                    },
                };
            }
        }
        let forged = assessor(AgentAssetAssessmentRequest {
            context: request.context,
            targets: &targets,
            declarations: &declarations,
            sources: &sources,
        });
        assert!(
            matches!(
                forged.get(&target),
                Some(AgentAssetAssessmentResult::Unsupported(
                    AgentAssetAssessmentFailure::InvalidNativeInput
                ))
            ),
            "{attack}"
        );
        assert!(
            matches!(
                forged.get(&healthy),
                Some(AgentAssetAssessmentResult::Assessed { .. })
            ),
            "healthy neighbor after {attack}"
        );
    }
    super::grok_proof::emit_captured(native, output);
}

#[test]
fn grok_shadowed_parent_inline_children_preserve_raw_declarations_without_rows() {
    for project in [false, true] {
        let fixture = Fixture::new();
        let loser = fixture.package(false, "z-loser");
        fixture.manifest(
            &loser,
            json!({"name":"same","mcpServers":{
                "same-server":{"command":"loser-command"},
                "loser-only":{"command":"loser-only-command"}
            }}),
        );
        fixture.write(
            loser.join(".mcp.json"),
            br#"{"mcpServers":{"unread-decoy":{"command":"decoy"}}}"#,
        );
        let winner = fixture.package(project, "a-winner");
        fixture.manifest(
            &winner,
            json!({"name":"same","mcpServers":{
                "same-server":{"command":"winner-command"},
                "winner-only":{"command":"winner-only-command"}
            }}),
        );
        let healthy = fixture.package(false, "healthy");
        fixture.manifest(
            &healthy,
            json!({"name":"healthy", "mcpServers":{
                "healthy-server":{"command":"healthy-command"}
            }}),
        );
        let (snapshot, reads) = fixture.scan_with_resolver(
            AgentAssetLimits::DEFAULT,
            false,
            Some(verify_parent_suppression_rejection),
        );
        assert_admitted(&snapshot, 6);
        assert_eq!(snapshot.inventory.declarations.len(), 12);
        assert_eq!(rows(&snapshot, AgentAssetCategory::Plugin).len(), 3);
        assert_eq!(
            rows(&snapshot, AgentAssetCategory::Mcp)
                .iter()
                .map(|asset| asset.native_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["healthy-server", "same-server", "winner-only"])
        );
        let parent = selected(&snapshot, AgentAssetCategory::Plugin, "same");
        let shared = selected(&snapshot, AgentAssetCategory::Mcp, "same-server");
        assert_eq!(shared.declared_state, AgentAssetState::Enabled);
        assert_eq!(shared.effective_state, AgentAssetState::Unknown);
        assert_eq!(
            shared.relationships.provided_by.as_ref(),
            Some(&parent.stable_id)
        );
        assert_eq!(shared.represented_declaration_ids.len(), 2);
        let suppressed = snapshot
            .inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.participation
                    == (AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::ParentNotSelected,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(suppressed.len(), 2);
        assert!(suppressed
            .iter()
            .all(|declaration| declaration.native_kind == AgentAssetCategory::Mcp));
        assert!(suppressed
            .iter()
            .any(|declaration| declaration.native_id == "loser-only"));
        assert!(!reads.contains(&loser.join(".mcp.json")));
    }
}

#[test]
fn grok_shadowed_convention_children_require_complete_unique_raw_sets() {
    let fixture = Fixture::new();
    let loser = fixture.package(false, "same");
    fixture.write(
        loser.join(".mcp.json"),
        br#"{"mcpServers":{
        "same-server":{"command":"loser-command"},
        "loser-only":{"command":"loser-only-command"}
    }}"#,
    );
    let winner = fixture.package(true, "same");
    fixture.write(
        winner.join(".mcp.json"),
        br#"{"mcpServers":{
        "same-server":{"command":"winner-command"},
        "winner-only":{"command":"winner-only-command"}
    }}"#,
    );
    let healthy = fixture.package(false, "healthy");
    fixture.manifest(
        &healthy,
        json!({"name":"healthy", "mcpServers":{
            "healthy-server":{"command":"healthy-command"}
        }}),
    );
    let (snapshot, reads) = fixture.scan_with_resolver(
        AgentAssetLimits::DEFAULT,
        false,
        Some(verify_parent_suppression_rejection),
    );
    assert_admitted(&snapshot, 6);
    assert_eq!(snapshot.inventory.declarations.len(), 18);
    let shared = selected(&snapshot, AgentAssetCategory::Mcp, "same-server");
    let parent = selected(&snapshot, AgentAssetCategory::Plugin, "same");
    assert_eq!(shared.inspection_source_id, parent.inspection_source_id);
    assert_eq!(shared.represented_declaration_ids.len(), 2);
    assert_eq!(shared.declared_state, AgentAssetState::Enabled);
    assert_eq!(shared.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        shared.relationships.provided_by.as_ref(),
        Some(&parent.stable_id)
    );
    assert_eq!(
        snapshot
            .inventory
            .declarations
            .iter()
            .filter(|asset| {
                asset.participation
                    == AgentAssetResolutionParticipation::Suppressed {
                        reason: AgentAssetSuppressionReason::ParentNotSelected,
                    }
            })
            .count(),
        2
    );
    for path in [loser.join(".mcp.json"), winner.join(".mcp.json")] {
        assert_eq!(reads.iter().filter(|read| **read == path).count(), 1);
    }
}
