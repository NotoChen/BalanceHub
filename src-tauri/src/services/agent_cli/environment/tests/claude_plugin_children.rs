//! Registered packages through real bounded inventory and independent projection.
//! Files and executable metadata are synthetic; no native CLI is executed.
use super::super::{snapshot::revision_for_missing, verified_path::VerifiedPathAnchor};
use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetEffectiveStateProofDraft, AgentAssetParser, AgentAssetProjectedDraft,
    AgentAssetResolver, AgentDiagnosticEmission, AgentOutputStop, ParsedAgentAsset,
};
use crate::{
    models::{
        AgentAssetDiscoveryIncompleteReason, AgentAssetInstallationOrigin, AgentAssetNativeRef,
        AgentAssetPresence, AgentAssetProviderOrigin, AgentAssetProvision, AgentAssetRecord,
        AgentAssetSuppressionReason,
    },
    services::agent_cli::{
        catalog::CatalogService,
        environment::mutation::{MutationInventory, MutationService},
    },
};
use serde_json::{json, Value};
use std::ops::ControlFlow;

struct PackageSnapshots<'a> {
    root: &'a Path,
    external_files: BTreeMap<String, PathBuf>,
    real: RealSnapshotPort,
    reads: Mutex<Vec<PathBuf>>,
    attempts: Mutex<usize>,
    limit: usize,
}
impl SnapshotPort for PackageSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        assert_ne!(request.source.native_source_key, "credentials-cache");
        {
            let mut attempts = self.attempts.lock().unwrap();
            *attempts += 1;
            assert!(*attempts <= self.limit);
        }
        if !request.source.path.starts_with(self.root) {
            assert_eq!(
                self.external_files.get(&request.source.native_source_key),
                Some(&request.source.path),
                "unexpected out-of-fixture snapshot request"
            );
            assert_eq!(request.source.source_kind, AgentAssetSourceKind::File);
            if request
                .source
                .native_source_key
                .starts_with("workspace-instructions:")
            {
                assert!(request.source.categories.is_empty());
            }
            return AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            };
        }
        let path = request.source.path.clone();
        let snapshot = self.real.snapshot(request, run);
        if matches!(snapshot, AgentAssetSnapshot::File { .. }) {
            self.reads.lock().unwrap().push(path);
        }
        snapshot
    }
    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

struct PackageFixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
}
impl PackageFixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let fixture = Self {
            home: root.join("home"),
            workspace: root.join("workspace"),
            _temporary: temporary,
            root,
        };
        fs::create_dir_all(fixture.workspace.join(".claude")).unwrap();
        write_json(&fixture.home.join(".claude/settings.json"), json!({}));
        write_json(
            &fixture.home.join(".claude.json"),
            json!({"projects": {
                fixture.workspace.to_string_lossy().as_ref(): {"hasTrustDialogAccepted": true}
            }}),
        );
        fixture
    }
    fn package(&self, directory: &str, manifest: Option<Value>) -> PathBuf {
        let path = self.home.join("packages").join(directory);
        fs::create_dir_all(&path).unwrap();
        if let Some(manifest) = manifest {
            write_json(&path.join(".claude-plugin/plugin.json"), manifest);
        }
        path
    }
    fn registry(&self, plugins: Value) {
        write_json(
            &self.home.join(".claude/plugins/installed_plugins.json"),
            json!({"version": 2, "plugins": plugins}),
        );
    }
    fn settings(&self, settings: Value) {
        write_json(&self.home.join(".claude/settings.json"), settings);
    }
    fn scan(&self, workspace: bool) -> (MutationInventory, Vec<PathBuf>) {
        self.scan_with(
            workspace,
            definition(AgentCliKind::ClaudeCode).environment,
            AgentAssetLimits::DEFAULT,
        )
    }
    fn scan_with(
        &self,
        workspace: bool,
        adapter: EnvironmentAdapter,
        limits: AgentAssetLimits,
    ) -> (MutationInventory, Vec<PathBuf>) {
        let _config_root = ClaudeConfigRootOverrideGuard::new(self.home.join(".claude"));
        let native = definition(AgentCliKind::ClaudeCode);
        let definitions = [AgentCliDefinition {
            environment: adapter.with_test_contexts(claude_test_discover_contexts),
            ..*native
        }];
        let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
            "2.1.270",
            AgentInstallationChannel::Stable,
        )]);
        let managed_root = claude_managed_root();
        let mut external_files = BTreeMap::from([
            (
                "managed-settings".to_owned(),
                managed_root.join("managed-settings.json"),
            ),
            (
                "managed-mcp".to_owned(),
                managed_root.join("managed-mcp.json"),
            ),
        ]);
        if workspace {
            external_files.extend(
                claude_instruction_source_expectations(&self.home, &self.workspace)
                    .into_iter()
                    .filter(|(_, (path, _))| !path.starts_with(&self.root))
                    .map(|(key, (path, kind))| {
                        assert_eq!(kind, AgentAssetSourceKind::File);
                        (key, path)
                    }),
            );
        }
        let snapshots = PackageSnapshots {
            root: &self.root,
            external_files,
            real: RealSnapshotPort::default(),
            reads: Mutex::new(Vec::new()),
            attempts: Mutex::new(0),
            limit: limits.sources_per_context,
        };
        let inventory = build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: workspace.then_some(self.workspace.as_path()),
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
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        (
            MutationInventory {
                inventory,
                source_anchors,
            },
            snapshots.reads.into_inner().unwrap(),
        )
    }
}

fn write_json(path: &Path, value: Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}
fn write_skill(directory: &Path, name: &str) {
    fs::create_dir_all(directory).unwrap();
    // These positive fixtures use the bounded quoted-scalar frontmatter
    // contract, including namespace separators in root Skill names.
    let name = serde_json::to_string(name).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\n---\nSynthetic package Skill.\n"),
    )
    .unwrap();
}

#[test]
fn claude_plugin_hooks_accumulate_files_and_inline_rules_with_one_physical_read() {
    let fixture = PackageFixture::new();
    let package = fixture.package("hook-provider", Some(json!({"name":"HookProvider","hooks":[
        "./hooks/hooks.json", "./extra.json", "./extra.json",
        {"PostToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"fixture-inline"}]}]}
    ]})));
    let default_path = package.join("hooks/hooks.json");
    let additional_path = package.join("extra.json");
    write_json(
        &default_path,
        json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"fixture-default"}]}]}}),
    );
    write_json(
        &additional_path,
        json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-first"},{"type":"command","command":"${CLAUDE_PLUGIN_ROOT}/fixture-runner"}]}]}}),
    );
    fixture.registry(
        json!({"registered@fixture":[{"scope":"user","installPath":package,"version":"1.0.0"}]}),
    );
    fixture.settings(json!({"enabledPlugins":{"registered@fixture":true}}));
    let (snapshot, reads) = fixture.scan(false);
    let hooks = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 4, "{:?}", snapshot.inventory.diagnostics);
    assert_eq!(
        snapshot
            .inventory
            .hook_rule_counts
            .iter()
            .find(|count| count.agent_kind == AgentCliKind::ClaudeCode)
            .unwrap()
            .rule_count,
        Some(4)
    );
    let adapter = definition(AgentCliKind::ClaudeCode)
        .environment
        .hook_adapter()
        .unwrap();
    let mut dependent = 0;
    for hook in hooks {
        assert_parent(&snapshot, hook, "registered@fixture");
        assert_eq!(hook.effective_state, AgentAssetState::Enabled);
        assert!(matches!(
            hook.details,
            AgentAssetDetails::Hook {
                rule_count: Some(1),
                ..
            }
        ));
        let rule = (adapter.read_hook)(&snapshot, hook).unwrap();
        assert_eq!(
            rule.native_asset_id.as_deref(),
            Some(hook.stable_id.as_str())
        );
        if rule.definition.group["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("CLAUDE_PLUGIN_ROOT")
        {
            dependent += 1;
            assert!((adapter.validate_adoption)(&rule).is_err());
        } else {
            assert!((adapter.validate_adoption)(&rule).is_ok());
        }
    }
    assert_eq!(dependent, 1);
    assert_eq!(
        reads.iter().filter(|path| *path == &default_path).count(),
        1
    );
    assert_eq!(
        reads
            .iter()
            .filter(|path| *path == &additional_path)
            .count(),
        1
    );
    let targets = (adapter.hook_targets)(&snapshot.inventory, &snapshot.inventory.contexts[0]);
    assert!(targets.iter().all(|target| snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == target.source_id)
        .unwrap()
        .origin
        != AgentAssetInstallationOrigin::NativePackage));
}

#[test]
fn claude_disabled_plugin_gates_its_hook_without_rewriting_the_child_definition() {
    let fixture = PackageFixture::new();
    let package = fixture.package("hooks-disabled", Some(json!({"name":"HookProvider"})));
    let hook_path = package.join("hooks/hooks.json");
    write_json(
        &hook_path,
        json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-child"}]}]}}),
    );
    fixture.registry(json!({"registered@fixture":[{"scope":"user","installPath":package}]}));
    fixture.settings(json!({"enabledPlugins":{"registered@fixture":false}}));
    let before = fs::read(&hook_path).unwrap();
    let (snapshot, _) = fixture.scan(false);
    let hook = asset(
        &snapshot,
        AgentAssetCategory::Hook,
        "plugin:registered@fixture:hook:0:SessionStart:group-0:hook-0",
    );
    assert_parent(&snapshot, hook, "registered@fixture");
    assert_eq!(hook.declared_state, AgentAssetState::Enabled);
    assert_eq!(hook.effective_state, AgentAssetState::Disabled);
    assert_eq!(fs::read(&hook_path).unwrap(), before);
}

#[test]
fn claude_unloadable_plugin_guard_keeps_sibling_hooks_unknown() {
    let fixture = PackageFixture::new();
    let package = fixture.package(
        "hooks-invalid",
        Some(json!({"name":"HookProvider","hooks":"./other.json"})),
    );
    write_json(
        &package.join("hooks/hooks.json"),
        json!({"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":false}]}]}}),
    );
    write_json(
        &package.join("other.json"),
        json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"fixture-sibling"}]}]}}),
    );
    fixture.registry(json!({"registered@fixture":[{"scope":"user","installPath":package}]}));
    fixture.settings(json!({"enabledPlugins":{"registered@fixture":true}}));
    let (snapshot, _) = fixture.scan(false);
    let hooks = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert!(!snapshot
        .inventory
        .declarations
        .iter()
        .filter(|asset| asset.native_kind == AgentAssetCategory::Hook
            && asset.role == AgentAssetDeclarationRole::Definition)
        .collect::<Vec<_>>()
        .is_empty());
    assert!(hooks
        .iter()
        .all(|hook| hook.effective_state != AgentAssetState::Enabled));
    assert!(snapshot
        .inventory
        .hook_rule_counts
        .iter()
        .find(|count| count.agent_kind == AgentCliKind::ClaudeCode)
        .unwrap()
        .rule_count
        .is_none());
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Hook,
        AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint
    ));
}
fn asset<'a>(
    snapshot: &'a MutationInventory,
    category: AgentAssetCategory,
    name: &str,
) -> &'a AgentAssetRecord {
    let rows = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.category == category
                && asset.native_id == name
                && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        1,
        "{category:?} {name}: {:#?}",
        snapshot.inventory.diagnostics
    );
    rows[0]
}
fn names(snapshot: &MutationInventory, category: AgentAssetCategory) -> BTreeSet<&str> {
    snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == category)
        .map(|asset| asset.native_id.as_str())
        .collect()
}
fn assert_parent(snapshot: &MutationInventory, child: &AgentAssetRecord, parent: &str) {
    let plugin = asset(snapshot, AgentAssetCategory::Plugin, parent);
    assert_eq!(
        child.relationships.provided_by.as_deref(),
        Some(plugin.stable_id.as_str())
    );
    assert_eq!(
        child.relationships.action_owner.as_deref(),
        Some(plugin.stable_id.as_str())
    );
    assert!(!child.writable);
    assert!(!child.provenance.is_empty());
    for provenance in &child.provenance {
        assert_eq!(provenance.provision, AgentAssetProvision::PluginProvided);
        assert_eq!(
            provenance.installation,
            AgentAssetInstallationOrigin::NativePackage
        );
        assert_eq!(provenance.provider, AgentAssetProviderOrigin::Unknown);
    }
    for declaration in snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            child.represented_declaration_ids.contains(&declaration.id)
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
    {
        let expected = AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: parent.to_owned(),
            qualifier: Some(format!("plugin:{parent}")),
        };
        assert_eq!(declaration.provided_by.as_ref(), Some(&expected));
        assert_eq!(declaration.action_owner.as_ref(), Some(&expected));
    }
}
fn all_diagnostics(snapshot: &MutationInventory) -> impl Iterator<Item = &AgentAssetDiagnostic> {
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
}

fn has_boundary(
    snapshot: &MutationInventory,
    category: AgentAssetCategory,
    reason: AgentAssetDiscoveryIncompleteReason,
) -> bool {
    all_diagnostics(snapshot).any(|diagnostic| matches!(diagnostic,
        AgentAssetDiagnostic::DiscoveryIncomplete { agent_kind: AgentCliKind::ClaudeCode, category: actual, reason: actual_reason }
            if *actual == category && *actual_reason == reason))
}

#[test]
fn claude_registered_package_children_preserve_namespace_parent_and_catalog_boundary() {
    let fixture = PackageFixture::new();
    let package = fixture.package("installed", Some(json!({
        "name": "ToolsV2", "skills": ["./extras"],
        "mcpServers": ["./extra-mcp.json", {"alpha": {"type": "http", "url": "https://fixture.invalid/mcp"}}]
    })));
    write_skill(&package.join("skills/review"), "ignored-frontmatter-name");
    write_skill(&package.join("extras"), "ToolsV2:custom name");
    write_json(
        &package.join(".mcp.json"),
        json!({"mcpServers": {"alpha": {"command": "fixture-default"}}}),
    );
    write_json(
        &package.join("extra-mcp.json"),
        json!({"alpha": {"command": "fixture-middle"}, "beta": {"command": "fixture-beta"}, "broken": {"command": 4}}),
    );
    fixture.registry(json!({"registered@fixture": [{"scope": "user", "installPath": package, "version": "1.2.3"}]}));
    fixture.settings(json!({"enabledPlugins": {"registered@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Plugin),
        BTreeSet::from(["registered@fixture"]),
        "{:?}",
        snapshot.inventory.diagnostics
    );
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["ToolsV2:review", "ToolsV2:custom-name"])
    );
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Mcp),
        BTreeSet::from(["plugin:ToolsV2:alpha", "plugin:ToolsV2:beta"])
    );
    assert_eq!(
        snapshot.inventory.assets.len(),
        7,
        "parent, two Skills, two MCP winners, two alpha losers"
    );
    let plugin = asset(&snapshot, AgentAssetCategory::Plugin, "registered@fixture");
    assert_eq!(plugin.effective_state, AgentAssetState::Enabled);
    assert!(matches!(
        plugin.details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::Installed,
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    for (category, name) in [
        (AgentAssetCategory::Skill, "ToolsV2:review"),
        (AgentAssetCategory::Skill, "ToolsV2:custom-name"),
        (AgentAssetCategory::Mcp, "plugin:ToolsV2:alpha"),
        (AgentAssetCategory::Mcp, "plugin:ToolsV2:beta"),
    ] {
        let child = asset(&snapshot, category, name);
        assert_parent(&snapshot, child, "registered@fixture");
        assert_eq!(child.effective_state, AgentAssetState::Enabled);
    }
    assert!(matches!(
        asset(&snapshot, AgentAssetCategory::Mcp, "plugin:ToolsV2:alpha").details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http,
            ..
        }
    ));
    assert!(reads.contains(&package.join("skills/review/SKILL.md")));
    assert_eq!(
        snapshot
            .inventory
            .declarations
            .iter()
            .filter(
                |declaration| declaration.native_kind == AgentAssetCategory::Plugin
                    && declaration.role == AgentAssetDeclarationRole::Definition
            )
            .count(),
        1
    );
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let bindings = catalog
        .assets
        .iter()
        .flat_map(|asset| &asset.bindings)
        .filter(|binding| binding.native.relationships.provided_by.is_some())
        .collect::<Vec<_>>();
    assert!(!bindings.is_empty());
    assert!(bindings.iter().all(|binding| !binding.can_adopt));
    assert!(bindings
        .iter()
        .any(|binding| binding.native.native_id == "ToolsV2:review"));
}

#[test]
fn claude_root_skill_short_circuits_children_and_explicit_commands_replace_defaults() {
    let fixture = PackageFixture::new();
    let package = fixture.package("root-fallback", None);
    write_skill(&package, "root name");
    write_skill(&package.join("nested"), "unloaded-nested");
    write_json(
        &package.join("plugin.json"),
        json!({"name": "wrong-namespace"}),
    );
    let commands = fixture.package(
        "command-package",
        Some(json!({"name": "Commands", "skills": "./extra", "commands": "./alternate"})),
    );
    write_skill(&commands.join("extra"), "Commands:root skill");
    write_skill(&commands.join("extra/unloaded"), "unloaded-child");
    fs::create_dir_all(commands.join("alternate")).unwrap();
    fs::write(commands.join("alternate/run.md"), b"Synthetic command.\n").unwrap();
    fs::create_dir_all(commands.join("commands")).unwrap();
    fs::write(commands.join("commands/ignored.md"), b"Unloaded command.\n").unwrap();
    fixture.registry(json!({"fallback@fixture": [{"scope": "user", "installPath": package}], "commands@fixture": [{"scope": "user", "installPath": commands}]}));
    fixture
        .settings(json!({"enabledPlugins": {"fallback@fixture": true, "commands@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["fallback:root-name", "Commands:root-skill", "Commands:run"])
    );
    assert_eq!(names(&snapshot, AgentAssetCategory::Plugin).len(), 2);
    assert_eq!(snapshot.inventory.assets.len(), 5);
    assert!(!reads.iter().any(|path| path.ends_with("unloaded/SKILL.md")
        || path.ends_with("nested/SKILL.md")
        || path.ends_with("commands/ignored.md")
        || path.ends_with("root-fallback/plugin.json")));
}

#[test]
fn claude_plugin_parent_gate_keeps_child_configuration_separate() {
    let fixture = PackageFixture::new();
    let package = fixture.package("parent", Some(json!({"name": "Parent"})));
    write_skill(&package.join("skills/run"), "run");
    write_json(
        &package.join(".mcp.json"),
        json!({"runner": {"command": "fixture-runner"}}),
    );
    fixture.registry(json!({"parent@fixture": [{"scope": "user", "installPath": package}]}));
    for (setting, parent_state, child_state) in [
        (
            json!(true),
            AgentAssetState::Enabled,
            AgentAssetState::Enabled,
        ),
        (
            json!(false),
            AgentAssetState::Disabled,
            AgentAssetState::Disabled,
        ),
        (
            json!("invalid"),
            AgentAssetState::Unknown,
            AgentAssetState::Unknown,
        ),
    ] {
        fixture.settings(json!({"enabledPlugins": {"parent@fixture": setting}}));
        let (snapshot, _) = fixture.scan(false);
        assert_eq!(snapshot.inventory.assets.len(), 3);
        assert_eq!(
            asset(&snapshot, AgentAssetCategory::Plugin, "parent@fixture").effective_state,
            parent_state
        );
        for (category, name) in [
            (AgentAssetCategory::Skill, "Parent:run"),
            (AgentAssetCategory::Mcp, "plugin:Parent:runner"),
        ] {
            let child = asset(&snapshot, category, name);
            assert_parent(&snapshot, child, "parent@fixture");
            assert_eq!(child.declared_state, AgentAssetState::Enabled);
            assert_eq!(child.effective_state, child_state);
        }
    }
}

#[test]
fn claude_missing_or_invalid_packages_do_not_hide_valid_neighbors_or_invent_children() {
    let fixture = PackageFixture::new();
    let missing = fixture.home.join("packages/missing");
    let invalid = fixture.package("invalid", Some(json!({"name": "invalid name"})));
    write_skill(&invalid.join("skills/hidden"), "hidden");
    let empty = fixture.package("empty", None);
    let valid = fixture.package("valid", Some(json!({"name": "Valid"})));
    write_skill(&valid.join("skills/present"), "present");
    fixture.registry(json!({"missing@fixture": [{"scope": "user", "installPath": missing}], "invalid@fixture": [{"scope": "user", "installPath": invalid}], "empty@fixture": [{"scope": "user", "installPath": empty}], "valid@fixture": [{"scope": "user", "installPath": valid}]}));
    fixture.settings(json!({"enabledPlugins": {"missing@fixture": true, "invalid@fixture": true, "empty@fixture": true, "valid@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(names(&snapshot, AgentAssetCategory::Plugin).len(), 4);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["Valid:present"])
    );
    assert_eq!(snapshot.inventory.assets.len(), 5);
    for (name, install_state, effective) in [
        (
            "missing@fixture",
            AgentAssetInstallState::NotInstalled,
            AgentAssetState::NotInstalled,
        ),
        (
            "invalid@fixture",
            AgentAssetInstallState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "empty@fixture",
            AgentAssetInstallState::Installed,
            AgentAssetState::Enabled,
        ),
    ] {
        let parent = asset(&snapshot, AgentAssetCategory::Plugin, name);
        assert_eq!(parent.effective_state, effective);
        assert!(
            matches!(parent.details, AgentAssetDetails::Plugin { install_state: actual, .. } if actual == install_state)
        );
    }
    assert!(!reads.contains(&invalid.join("skills/hidden/SKILL.md")));
}

#[test]
fn claude_selected_registry_occurrence_owns_children_and_project_mcp_requires_approval() {
    let fixture = PackageFixture::new();
    let user = fixture.package("user", Some(json!({"name": "UserPackage"})));
    let project = fixture.package("project", Some(json!({"name": "ProjectPackage"})));
    write_skill(&user.join("skills/user-only"), "user-only");
    write_skill(&project.join("skills/selected"), "selected");
    write_json(
        &project.join(".mcp.json"),
        json!({"server": {"command": "fixture-project"}}),
    );
    fixture.registry(json!({"shared@fixture": [
        {"scope": "user", "installPath": user},
        {"scope": "project", "projectPath": fixture.workspace, "installPath": project}
    ]}));
    fixture.settings(json!({"enabledPlugins": {"shared@fixture": true}}));
    let (pending, reads) = fixture.scan(true);
    assert_eq!(
        names(&pending, AgentAssetCategory::Skill),
        BTreeSet::from(["ProjectPackage:selected"])
    );
    assert_eq!(
        pending.inventory.assets.len(),
        4,
        "two parent installation records and two selected children"
    );
    let child = asset(
        &pending,
        AgentAssetCategory::Skill,
        "ProjectPackage:selected",
    );
    assert_parent(&pending, child, "shared@fixture");
    assert_eq!(child.scope, AgentAssetScope::Workspace);
    assert_eq!(child.effective_state, AgentAssetState::Enabled);
    let mcp = asset(
        &pending,
        AgentAssetCategory::Mcp,
        "plugin:ProjectPackage:server",
    );
    assert_eq!(mcp.declared_state, AgentAssetState::Enabled);
    assert_eq!(mcp.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        mcp.details,
        AgentAssetDetails::Mcp {
            approval_state: AgentMcpApprovalState::Pending,
            ..
        }
    ));
    assert!(!reads.contains(&user.join("skills/user-only/SKILL.md")));
    fixture.settings(json!({"enabledPlugins": {"shared@fixture": true}, "enabledMcpjsonServers": ["plugin:ProjectPackage:server"]}));
    let (approved, _) = fixture.scan(true);
    assert_eq!(
        asset(
            &approved,
            AgentAssetCategory::Mcp,
            "plugin:ProjectPackage:server"
        )
        .effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        asset(
            &pending,
            AgentAssetCategory::Skill,
            "ProjectPackage:selected"
        )
        .stable_id,
        asset(
            &approved,
            AgentAssetCategory::Skill,
            "ProjectPackage:selected"
        )
        .stable_id
    );
}

#[test]
fn claude_registry_tie_does_not_expand_either_package() {
    let fixture = PackageFixture::new();
    let first = fixture.package("first", Some(json!({"name": "First"})));
    let second = fixture.package("second", Some(json!({"name": "Second"})));
    write_skill(&first.join("skills/hidden"), "hidden");
    write_skill(&second.join("skills/hidden"), "hidden");
    fixture.registry(json!({"tied@fixture": [
        {"scope": "user", "installPath": first}, {"scope": "user", "installPath": second}
    ]}));
    fixture.settings(json!({"enabledPlugins": {"tied@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(snapshot.inventory.assets.len(), 1);
    assert_eq!(
        asset(&snapshot, AgentAssetCategory::Plugin, "tied@fixture").effective_state,
        AgentAssetState::Unknown
    );
    assert!(names(&snapshot, AgentAssetCategory::Skill).is_empty());
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Plugin,
        AgentAssetDiscoveryIncompleteReason::InstallationUnverified
    ));
    assert!(!reads
        .iter()
        .any(|path| path.starts_with(&first) || path.starts_with(&second)));
}

#[test]
fn claude_same_namespace_from_different_marketplaces_cannot_borrow_parent() {
    let fixture = PackageFixture::new();
    let first = fixture.package("market-a", Some(json!({"name": "Same"})));
    let second = fixture.package("market-b", Some(json!({"name": "Same"})));
    write_skill(&first.join("skills/run"), "run");
    write_skill(&second.join("skills/run"), "run");
    fixture.registry(json!({"same@market-a": [{"scope": "user", "installPath": first}], "same@market-b": [{"scope": "user", "installPath": second}]}));
    fixture.settings(json!({"enabledPlugins": {"same@market-a": false, "same@market-b": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Plugin),
        BTreeSet::from(["same@market-a", "same@market-b"])
    );
    let declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations
            .iter()
            .filter_map(|declaration| declaration
                .provided_by
                .as_ref()
                .map(|parent| parent.native_id.as_str()))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["same@market-a", "same@market-b"])
    );
    // A single public parent relationship cannot express these contradictory
    // definitions. The projector rejects the ambiguous row, not either parent.
    assert!(names(&snapshot, AgentAssetCategory::Skill).is_empty());
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
    assert!(snapshot
        .inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn claude_invalid_later_mcp_does_not_override_valid_definition() {
    let fixture = PackageFixture::new();
    let package = fixture.package("invalid-last", Some(json!({"name": "Merge", "mcpServers": [{"server": {"command": 5}, "valid": {"command": "fixture-valid"}}]})));
    write_json(
        &package.join(".mcp.json"),
        json!({"server": {"command": "fixture-original"}}),
    );
    fixture.registry(json!({"merge@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"merge@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Mcp),
        BTreeSet::from(["plugin:Merge:server", "plugin:Merge:valid"])
    );
    assert_eq!(snapshot.inventory.assets.len(), 3);
    let mcp = asset(&snapshot, AgentAssetCategory::Mcp, "plugin:Merge:server");
    assert_eq!(mcp.effective_state, AgentAssetState::Enabled);
    assert_eq!(
        Path::new(mcp.path.as_deref().unwrap()),
        package.join(".mcp.json")
    );
    let manifest = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == package.join(".claude-plugin/plugin.json"))
        .expect("registered package manifest source");
    assert!(manifest.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Manifest,
            location: Some(location),
        } if location == "plugin.mcpServers.entry"
    )));
}

#[test]
fn claude_runtime_expansion_and_cross_name_equivalence_remain_explicitly_unknown() {
    let fixture = PackageFixture::new();
    let package = fixture.package(
        "runtime",
        Some(json!({"name": "Runtime", "mcpServers": ["https://fixture.invalid/package.mcpb"]})),
    );
    write_json(
        &package.join(".mcp.json"),
        json!({
            "same-command": {"command": "fixture-shared", "args": ["serve"]},
            "expanded": {"command": "${CLAUDE_PLUGIN_ROOT}/bin/serve"},
            "literal": {"command": "fixture-distinct"}
        }),
    );
    write_json(
        &fixture.home.join(".claude.json"),
        json!({"mcpServers": {"manual": {"command": "fixture-shared", "args": ["serve"]}}}),
    );
    fixture.registry(json!({"runtime@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"runtime@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Mcp),
        BTreeSet::from([
            "manual",
            "plugin:Runtime:same-command",
            "plugin:Runtime:expanded",
            "plugin:Runtime:literal"
        ])
    );
    assert_eq!(snapshot.inventory.assets.len(), 5);
    for name in ["plugin:Runtime:same-command", "plugin:Runtime:expanded"] {
        let mcp = asset(&snapshot, AgentAssetCategory::Mcp, name);
        assert_parent(&snapshot, mcp, "runtime@fixture");
        assert_eq!(mcp.effective_state, AgentAssetState::Unknown);
        assert!(matches!(
            mcp.details,
            AgentAssetDetails::Mcp {
                effective_availability: AgentAssetEffectiveAvailability::Unknown,
                ..
            }
        ));
    }
    assert_eq!(
        asset(&snapshot, AgentAssetCategory::Mcp, "manual").effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        asset(&snapshot, AgentAssetCategory::Mcp, "plugin:Runtime:literal").effective_state,
        AgentAssetState::Enabled
    );
    for reason in [
        AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved,
        AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
    ] {
        assert!(has_boundary(&snapshot, AgentAssetCategory::Mcp, reason));
    }
    // An explicitly disabled manual definition does not reserve the endpoint.
    write_json(
        &fixture.home.join(".claude.json"),
        json!({"mcpServers": {"manual": {"command": "fixture-shared", "args": ["serve"]}}, "projects": {
            fixture.workspace.to_string_lossy().as_ref(): {"hasTrustDialogAccepted": true, "disabledMcpServers": ["manual"]}
        }}),
    );
    let (disabled, _) = fixture.scan(true);
    assert_eq!(
        asset(&disabled, AgentAssetCategory::Mcp, "manual").effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        asset(
            &disabled,
            AgentAssetCategory::Mcp,
            "plugin:Runtime:same-command"
        )
        .effective_state,
        AgentAssetState::Enabled
    );
}

#[cfg(unix)]
#[test]
fn claude_plugin_no_follow_and_relative_paths_do_not_expand_access() {
    use std::os::unix::fs::symlink;
    let fixture = PackageFixture::new();
    let outside = fixture.root.join("outside");
    write_skill(&outside, "outside");
    write_json(
        &outside.join("secret.json"),
        json!({"never": {"command": "never-read"}}),
    );
    let package = fixture.package("links", Some(json!({"name": "Links"})));
    fs::create_dir_all(package.join("skills")).unwrap();
    symlink(&outside, package.join("skills/blocked")).unwrap();
    symlink(outside.join("secret.json"), package.join(".mcp.json")).unwrap();
    write_skill(&package.join("skills/visible"), "visible");
    let traversal = fixture.package(
        "traversal",
        Some(json!({"name": "Traversal", "skills": ["./../../outside"]})),
    );
    fixture.registry(json!({
        "links@fixture": [{"scope": "user", "installPath": package}],
        "traversal@fixture": [{"scope": "user", "installPath": traversal}],
        "bad-root@fixture": [{"scope": "user", "installPath": fixture.root.join("packages/../outside")}]
    }));
    fixture.settings(json!({"enabledPlugins": {"links@fixture": true, "traversal@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Plugin),
        BTreeSet::from(["links@fixture", "traversal@fixture"])
    );
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["Links:visible"])
    );
    assert!(names(&snapshot, AgentAssetCategory::Mcp).is_empty());
    assert!(!reads.iter().any(|path| path.starts_with(&outside)
        || path.ends_with("links/.mcp.json")
        || path.ends_with("blocked/SKILL.md")));
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::SourceUnavailable
    ));
}

#[cfg(unix)]
#[test]
fn claude_standalone_skill_keeps_alias_and_suppresses_same_physical_plugin_child() {
    use std::os::unix::fs::symlink;
    let fixture = PackageFixture::new();
    // Readonly native aliases permit only direct children of this shared root.
    // A registered package can provide its root Skill from the same directory.
    let package = fixture.home.join(".agents/skills/shared-content");
    write_json(
        &package.join(".claude-plugin/plugin.json"),
        json!({"name": "Shared"}),
    );
    write_skill(&package, "run");
    fs::create_dir_all(fixture.home.join(".claude/skills")).unwrap();
    symlink(&package, fixture.home.join(".claude/skills/alias")).unwrap();
    fixture.registry(json!({"shared@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"shared@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["alias"])
    );
    assert_eq!(snapshot.inventory.assets.len(), 2);
    let standalone = asset(&snapshot, AgentAssetCategory::Skill, "alias");
    assert_eq!(
        Path::new(standalone.path.as_deref().unwrap()),
        fixture.home.join(".claude/skills/alias/SKILL.md")
    );
    assert!(standalone.relationships.provided_by.is_none());
    let plugin_children = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Skill
                && declaration.native_id == "Shared:run"
        })
        .collect::<Vec<_>>();
    assert_eq!(plugin_children.len(), 1);
    assert!(matches!(
        plugin_children[0].participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::DuplicatePhysicalSource
        }
    ));
}

#[test]
fn claude_package_discovery_stops_at_context_source_limit() {
    let fixture = PackageFixture::new();
    let package = fixture.package("bounded", Some(json!({"name": "Bounded"})));
    for index in 0..40 {
        write_skill(&package.join(format!("skills/skill-{index:02}")), "fixture");
    }
    fixture.registry(json!({"bounded@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"bounded@fixture": true}}));
    let (complete, _) = fixture.scan(false);
    assert_eq!(names(&complete, AgentAssetCategory::Skill).len(), 40);
    let limits = AgentAssetLimits {
        sources_per_context: 16,
        ..AgentAssetLimits::DEFAULT
    };
    let (limited, reads) = fixture.scan_with(
        false,
        definition(AgentCliKind::ClaudeCode).environment,
        limits,
    );
    assert!(limited.inventory.sources.len() <= 16);
    assert!(reads.len() <= 16);
    assert!(names(&limited, AgentAssetCategory::Skill).len() < 40);
    assert!(limited
        .inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::SourcesPerContext,
                ..
            }
        )));
}

struct CorruptChild<'a> {
    output: &'a mut dyn AgentParseOutput,
    fault: u8,
}
impl AgentDiagnosticOutput for CorruptChild<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, diagnostic: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(diagnostic)
    }
}
impl AgentParseOutput for CorruptChild<'_> {
    fn emit_declaration(
        &mut self,
        mut declaration: ParsedAgentAsset,
    ) -> ControlFlow<AgentOutputStop> {
        if self.fault == 6
            && declaration
                .source_key
                .starts_with("claude-plugin:manifest:")
            && declaration.category == AgentAssetCategory::Plugin
        {
            return ControlFlow::Continue(());
        }
        if declaration.category == AgentAssetCategory::Skill
            && declaration.native_id == "Target:run"
        {
            match self.fault {
                1 => {
                    declaration.provided_by.as_mut().unwrap().qualifier = None;
                }
                2 => {
                    let other = AgentAssetNativeRef {
                        category: AgentAssetCategory::Plugin,
                        native_id: "other@fixture".into(),
                        qualifier: Some("plugin:other@fixture".into()),
                    };
                    declaration.provided_by = Some(other.clone());
                    declaration.action_owner = Some(other);
                }
                3 => {
                    declaration.native_id = "Other:run".into();
                    declaration.resolution_group_key = "Other:run".into();
                }
                4 => {
                    declaration.logical_origin.scope = AgentAssetScope::Workspace;
                }
                5 => {
                    declaration.presence = AgentAssetPresence::Missing;
                }
                _ => {}
            }
        }
        self.output.emit_declaration(declaration)
    }
}
fn corrupt_child<const FAULT: u8>(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    claude_test_parse(
        request,
        &mut CorruptChild {
            output,
            fault: FAULT,
        },
    );
}

struct OmitParentGate<'a>(&'a mut dyn AgentResolveOutput);
impl AgentDiagnosticOutput for OmitParentGate<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.0.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, diagnostic: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.0.emit_diagnostic(diagnostic)
    }
}
impl AgentResolveOutput for OmitParentGate<'_> {
    fn emit_draft(&mut self, mut draft: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
        if draft.native_id == "Target:run" {
            draft.state_proof.effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
        }
        self.0.emit_draft(draft)
    }
}
fn omit_parent_gate(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    claude_test_resolve(request, &mut OmitParentGate(output));
}
fn proof_adapter(parser: AgentAssetParser, resolver: AgentAssetResolver) -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        claude_test_discover_contexts,
        claude_test_discover_sources,
        Some(claude_test_discover_follow_up_sources),
        "claude-package-proof-fixture",
        parser,
        resolver,
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .state_assessor(),
    )
    .with_workspace_trust_authority(
        claude_test_discover_workspace_trust_sources,
        claude_test_resolve_workspace_trust,
    )
}

#[test]
fn claude_plugin_proof_rejects_forged_parent_namespace_scope_presence_and_missing_manifest() {
    let fixture = PackageFixture::new();
    let target = fixture.package("target", Some(json!({"name": "Target"})));
    let other = fixture.package("other", Some(json!({"name": "Other"})));
    write_skill(&target.join("skills/run"), "run");
    fixture.registry(json!({"target@fixture": [{"scope": "user", "installPath": target}], "other@fixture": [{"scope": "user", "installPath": other}]}));
    fixture.settings(json!({"enabledPlugins": {"target@fixture": true, "other@fixture": true}}));
    let (baseline, _) = fixture.scan(false);
    assert_eq!(baseline.inventory.assets.len(), 3);
    assert_eq!(
        asset(&baseline, AgentAssetCategory::Skill, "Target:run").effective_state,
        AgentAssetState::Enabled
    );
    let parsers: [AgentAssetParser; 6] = [
        corrupt_child::<1>,
        corrupt_child::<2>,
        corrupt_child::<3>,
        corrupt_child::<4>,
        corrupt_child::<5>,
        corrupt_child::<6>,
    ];
    for parser in parsers {
        let (rejected, _) = fixture.scan_with(
            false,
            proof_adapter(parser, claude_test_resolve),
            AgentAssetLimits::DEFAULT,
        );
        assert_eq!(names(&rejected, AgentAssetCategory::Plugin).len(), 2);
        assert!(
            names(&rejected, AgentAssetCategory::Skill).is_empty(),
            "{:#?}",
            rejected.inventory.diagnostics
        );
        assert!(all_diagnostics(&rejected).any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
        )));
    }
    let (rejected, _) = fixture.scan_with(
        false,
        proof_adapter(claude_test_parse, omit_parent_gate),
        AgentAssetLimits::DEFAULT,
    );
    assert_eq!(names(&rejected, AgentAssetCategory::Plugin).len(), 2);
    assert!(names(&rejected, AgentAssetCategory::Skill).is_empty());
    assert!(all_diagnostics(&rejected)
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn claude_invalid_mcp_component_slots_preserve_package_skills_and_valid_siblings() {
    let fixture = PackageFixture::new();
    let package = fixture.package("component-slots", Some(json!({
        "name": "Slots",
        "mcpServers": ["./valid.json", "../outside.json", 7, {"inline": {"command": "fixture-inline"}}]
    })));
    write_skill(&package.join("skills/kept"), "kept");
    write_json(
        &package.join(".mcp.json"),
        json!({"default": {"command": "fixture-default"}}),
    );
    write_json(
        &package.join("valid.json"),
        json!({"file": {"command": "fixture-file"}}),
    );
    write_json(
        &fixture.home.join("packages/outside.json"),
        json!({"unread": {"command": "fixture-unread"}}),
    );
    fixture.registry(json!({"slots@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"slots@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(snapshot.inventory.assets.len(), 5);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["Slots:kept"])
    );
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Mcp),
        BTreeSet::from([
            "plugin:Slots:default",
            "plugin:Slots:file",
            "plugin:Slots:inline"
        ])
    );
    assert_eq!(
        asset(&snapshot, AgentAssetCategory::Plugin, "slots@fixture").effective_state,
        AgentAssetState::Enabled
    );
    assert!(!reads.iter().any(|path| path.ends_with("outside.json")));
    let occurrence = snapshot
        .inventory
        .declarations
        .iter()
        .find(|declaration| {
            declaration.native_id == "slots@fixture"
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .unwrap()
        .declaration_key
        .strip_prefix("registry:")
        .unwrap();
    assert!(snapshot
        .inventory
        .declarations
        .iter()
        .any(|declaration| declaration.declaration_key
            == format!("plugin.mcp:{occurrence}:4:inline")));
    assert!(snapshot.inventory.diagnostics.iter().any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { location: Some(location), .. } if location == "plugin.mcpServers.component")));
}

#[test]
fn claude_external_registry_path_retains_unknown_parent_without_widening_trusted_roots() {
    let fixture = PackageFixture::new();
    let package = fixture.root.join("external-package");
    write_json(
        &package.join(".claude-plugin/plugin.json"),
        json!({"name": "External"}),
    );
    write_skill(&package.join("skills/unread"), "unread");
    fixture.registry(json!({"external@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"external@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(snapshot.inventory.assets.len(), 1);
    let parent = asset(&snapshot, AgentAssetCategory::Plugin, "external@fixture");
    assert_eq!(parent.declared_state, AgentAssetState::Enabled);
    assert_eq!(parent.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        parent.details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::Unknown,
            ..
        }
    ));
    assert!(names(&snapshot, AgentAssetCategory::Skill).is_empty());
    assert!(!reads.iter().any(|path| path.starts_with(&package)));
    let package_source = snapshot
        .inventory
        .sources
        .iter()
        .find(|source| Path::new(&source.path) == package)
        .expect("rejected package root remains observable");
    assert!(package_source.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::SourceOutsideAllowedRoot { source_id }
            if source_id == &package_source.id
    )));
}

#[test]
fn claude_shared_physical_package_across_qualified_parents_reports_unknown_native_order() {
    let fixture = PackageFixture::new();
    let package = fixture.package("same-physical-package", Some(json!({"name": "SharedRoot"})));
    write_skill(&package, "run");
    fixture.registry(json!({
        "first@fixture": [{"scope": "user", "installPath": package}],
        "second@fixture": [{"scope": "user", "installPath": package}]
    }));
    fixture.settings(json!({"enabledPlugins": {"first@fixture": true, "second@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Plugin),
        BTreeSet::from(["first@fixture", "second@fixture"])
    );
    let declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations
            .iter()
            .map(|declaration| declaration.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["SharedRoot:run"])
    );
    assert_eq!(
        declarations
            .iter()
            .filter_map(|declaration| declaration
                .provided_by
                .as_ref()
                .map(|parent| parent.native_id.as_str()))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["first@fixture", "second@fixture"])
    );
    let physical = declarations
        .iter()
        .map(|declaration| {
            snapshot.source_anchors[&declaration.source_id]
                .physical_path()
                .to_path_buf()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(physical.len(), 1);
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
    assert!(
        names(&snapshot, AgentAssetCategory::Skill).is_empty(),
        "the public single-parent contract must not pick an arbitrary enabled parent"
    );
}

#[test]
fn claude_shared_physical_skill_with_distinct_fallback_namespaces_keeps_unknown_rows() {
    let fixture = PackageFixture::new();
    let package = fixture.package("fallback-shared", None);
    write_skill(&package, "run");
    write_skill(
        &fixture.home.join(".claude/skills/independent"),
        "independent",
    );
    fixture.registry(json!({
        "alpha@fixture": [{"scope": "user", "installPath": package}],
        "beta@fixture": [{"scope": "user", "installPath": package}]
    }));
    fixture.settings(json!({"enabledPlugins": {"alpha@fixture": true, "beta@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(snapshot.inventory.assets.len(), 5);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["alpha:run", "beta:run", "independent"])
    );
    for (name, parent) in [("alpha:run", "alpha@fixture"), ("beta:run", "beta@fixture")] {
        let child = asset(&snapshot, AgentAssetCategory::Skill, name);
        assert_parent(&snapshot, child, parent);
        assert_eq!(child.declared_state, AgentAssetState::Unknown);
        assert_eq!(child.effective_state, AgentAssetState::Unknown);
        assert_eq!(
            asset(&snapshot, AgentAssetCategory::Plugin, parent).effective_state,
            AgentAssetState::Enabled
        );
        let declarations = snapshot
            .inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_kind == AgentAssetCategory::Skill
                    && declaration.native_id == name
            })
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].declared_state,
            AgentAssetDeclaredState::Enabled
        );
        assert_eq!(
            declarations[0].participation,
            AgentAssetResolutionParticipation::Participates
        );
    }
    assert_eq!(
        asset(&snapshot, AgentAssetCategory::Skill, "independent").effective_state,
        AgentAssetState::Enabled
    );
    assert!(has_boundary(
        &snapshot,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
    assert!(!has_boundary(
        &snapshot,
        AgentAssetCategory::Mcp,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let observed = catalog
        .assets
        .iter()
        .flat_map(|entry| &entry.bindings)
        .filter(|binding| binding.native.category == AgentAssetCategory::Skill)
        .map(|binding| binding.native.native_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed,
        BTreeSet::from(["alpha:run", "beta:run", "independent"])
    );
    let skill_entries = catalog
        .assets
        .iter()
        .filter(|entry| entry.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skill_entries.len(), 2);
    let shared = skill_entries
        .iter()
        .find(|entry| {
            entry
                .bindings
                .iter()
                .any(|binding| binding.native.native_id == "alpha:run")
        })
        .unwrap();
    assert_eq!(shared.bindings.len(), 2);
    assert_eq!(
        shared
            .bindings
            .iter()
            .map(|binding| binding.native.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha:run", "beta:run"])
    );
    assert!(shared.bindings.iter().all(|binding| !binding.can_adopt));
    let independent = skill_entries
        .iter()
        .find(|entry| {
            entry
                .bindings
                .iter()
                .any(|binding| binding.native.native_id == "independent")
        })
        .unwrap();
    assert_eq!(independent.bindings.len(), 1);
    assert_ne!(shared.id, independent.id);

    // A disabled parent is not a competing native load candidate. This rescan
    // must recover the proven enabled/disabled parent gates for the two rows.
    fixture.settings(json!({"enabledPlugins": {"alpha@fixture": true, "beta@fixture": false}}));
    let (one_active, _) = fixture.scan(false);
    assert_eq!(one_active.inventory.assets.len(), 5);
    assert_eq!(
        asset(&one_active, AgentAssetCategory::Skill, "alpha:run").effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        asset(&one_active, AgentAssetCategory::Skill, "beta:run").effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        asset(&one_active, AgentAssetCategory::Skill, "independent").effective_state,
        AgentAssetState::Enabled
    );
    assert!(!has_boundary(
        &one_active,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
}

#[test]
fn claude_same_package_default_and_explicit_skill_roots_deduplicate_physical_content() {
    let fixture = PackageFixture::new();
    let package = fixture.package(
        "overlapping-roots",
        Some(json!({
            "name": "OnePackage", "skills": ["./skills/extra"]
        })),
    );
    write_skill(&package.join("skills/extra"), "explicit-root-name");
    fixture.registry(json!({"one@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"one@fixture": true}}));
    let (snapshot, _) = fixture.scan(false);
    assert_eq!(snapshot.inventory.assets.len(), 2);
    assert_eq!(
        names(&snapshot, AgentAssetCategory::Skill),
        BTreeSet::from(["OnePackage:extra"])
    );
    let child = asset(&snapshot, AgentAssetCategory::Skill, "OnePackage:extra");
    assert_parent(&snapshot, child, "one@fixture");
    assert_eq!(child.effective_state, AgentAssetState::Enabled);
    let declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations
            .iter()
            .filter(|declaration| declaration.participation
                == AgentAssetResolutionParticipation::Participates)
            .count(),
        1
    );
    let suppressed = declarations
        .iter()
        .filter(|declaration| {
            matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::DuplicatePhysicalSource
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(suppressed.len(), 1);
    assert_eq!(suppressed[0].native_id, "OnePackage:explicit-root-name");
    assert!(!has_boundary(
        &snapshot,
        AgentAssetCategory::Skill,
        AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved
    ));
    let service = CatalogService::new(
        fixture.root.join("library"),
        Arc::new(MutationService::default()),
    );
    let catalog = service.catalog(&snapshot).unwrap();
    let bindings = catalog
        .assets
        .iter()
        .flat_map(|entry| &entry.bindings)
        .filter(|binding| binding.native.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].native.native_id, "OnePackage:extra");
}

#[test]
fn claude_shared_mcp_file_roles_keep_native_order_and_one_physical_read() {
    let fixture = PackageFixture::new();
    let package = fixture.package("shared-mcp-roles", Some(json!({
        "name": "Shared",
        "mcpServers": ["./.mcp.json", "./.mcp.json", "./.claude-plugin/plugin.json", {"server": {"command": "fixture-inline"}}]
    })));
    let mcp_path = package.join(".mcp.json");
    write_json(&mcp_path, json!({"server": {"command": "fixture-file"}}));
    fixture.registry(json!({"shared@fixture": [{"scope": "user", "installPath": package}]}));
    fixture.settings(json!({"enabledPlugins": {"shared@fixture": true}}));
    let (snapshot, reads) = fixture.scan(false);
    assert_eq!(
        snapshot.inventory.assets.len(),
        5,
        "{:?}",
        snapshot.inventory.diagnostics
    );
    let winner = asset(&snapshot, AgentAssetCategory::Mcp, "plugin:Shared:server");
    assert_eq!(
        winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);
    assert_parent(&snapshot, winner, "shared@fixture");
    let mcp = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Mcp)
        .collect::<Vec<_>>();
    assert_eq!(mcp.len(), 4);
    assert_eq!(
        mcp.iter()
            .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .count(),
        3
    );
    let declarations = snapshot
        .inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Mcp
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 4);
    assert_eq!(
        declarations
            .iter()
            .map(|declaration| &declaration.id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        declarations
            .iter()
            .map(|declaration| &declaration.source_id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(reads.iter().filter(|path| **path == mcp_path).count(), 1);
    assert_eq!(
        reads
            .iter()
            .filter(|path| **path == package.join(".claude-plugin/plugin.json"))
            .count(),
        1
    );
    assert!(
        !snapshot
            .inventory
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )),
        "{:?}",
        snapshot.inventory.diagnostics
    );
}

#[test]
fn claude_shared_package_source_preserves_each_parent_scope_and_mcp_approval() {
    let fixture = PackageFixture::new();
    let package = fixture.package("shared-scopes", None);
    write_json(
        &package.join(".mcp.json"),
        json!({"server": {"command": "fixture-shared"}}),
    );
    fixture.registry(json!({
        "user@fixture": [{"scope": "user", "installPath": package}],
        "project@fixture": [{"scope": "project", "installPath": package, "projectPath": fixture.workspace}]
    }));
    fixture.settings(json!({"enabledPlugins": {"user@fixture": true, "project@fixture": true}}));
    let (snapshot, reads) = fixture.scan(true);
    assert_eq!(
        snapshot.inventory.assets.len(),
        4,
        "{:?}",
        snapshot.inventory.diagnostics
    );
    for (name, parent, scope, approval) in [
        (
            "plugin:user:server",
            "user@fixture",
            AgentAssetScope::User,
            AgentMcpApprovalState::NotRequired,
        ),
        (
            "plugin:project:server",
            "project@fixture",
            AgentAssetScope::Workspace,
            AgentMcpApprovalState::Pending,
        ),
    ] {
        let child = asset(&snapshot, AgentAssetCategory::Mcp, name);
        assert_parent(&snapshot, child, parent);
        assert_eq!(child.scope, scope);
        assert!(
            matches!(child.details, AgentAssetDetails::Mcp { approval_state, .. } if approval_state == approval)
        );
        assert!(child
            .provenance
            .iter()
            .all(|provenance| provenance.scope == scope));
        assert_eq!(
            asset(&snapshot, AgentAssetCategory::Plugin, parent).scope,
            scope
        );
    }
    assert_eq!(
        reads
            .iter()
            .filter(|path| **path == package.join(".mcp.json"))
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .inventory
            .sources
            .iter()
            .filter(|source| Path::new(&source.path) == package)
            .count(),
        1
    );
    assert!(
        !snapshot
            .inventory
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )),
        "{:?}",
        snapshot.inventory.diagnostics
    );
}
