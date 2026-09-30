//! Real Grok discovery/parser/resolver/assessor/projector, with isolated ports.
use super::*;

pub(super) fn prepare_packages(
    root: &Path,
    directories: &[(&'static str, Vec<AgentAssetDirectoryEntry>)],
) {
    for (key, entries) in directories {
        let base = match *key {
            "plugins" => root.join(".grok/plugins"),
            "workspace-plugins" => root.join("workspace/.grok/plugins"),
            _ => continue,
        };
        for entry in entries.iter().filter(|entry| {
            !entry.is_symlink && entry.source_kind == AgentAssetSourceKind::Directory
        }) {
            let package = base.join(&entry.name);
            fs::create_dir_all(&package).unwrap();
            fs::write(
                package.join("plugin.json"),
                serde_json::to_vec(&serde_json::json!({"name":entry.name})).unwrap(),
            )
            .unwrap();
        }
    }
}

pub(super) fn write_workspace_trust(home: &Path, workspace: &Path, trusted: bool) {
    assert!(home.is_absolute() && workspace.is_absolute());
    fs::create_dir_all(home.join(".grok")).unwrap();
    fs::create_dir_all(workspace.join(".git/objects")).unwrap();
    fs::create_dir_all(workspace.join(".git/refs/heads")).unwrap();
    fs::write(workspace.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
    fs::write(
        workspace.join(".git/config"),
        b"[core]\nrepositoryformatversion = 0\nfilemode = true\nbare = false\nlogallrefupdates = true\n",
    )
    .unwrap();
    let folders = BTreeMap::from([(
        workspace.to_string_lossy().into_owned(),
        BTreeMap::from([("trusted", trusted)]),
    )]);
    fs::write(
        home.join(".grok/trusted_folders.toml"),
        toml::to_string(&BTreeMap::from([("folders", folders)])).unwrap(),
    )
    .unwrap();
}

pub(super) struct PackageSnapshots {
    root: PathBuf,
    scripted: ClaudeFixtureSnapshotPort,
    real: RealSnapshotPort,
    workspace_trust: bool,
}

impl PackageSnapshots {
    pub(super) fn new(root: &Path, scripted: ClaudeFixtureSnapshotPort) -> Self {
        Self {
            root: root.to_owned(),
            scripted,
            real: RealSnapshotPort::default(),
            workspace_trust: false,
        }
    }

    pub(super) fn with_workspace_trust(mut self) -> Self {
        self.workspace_trust = true;
        self
    }
}

impl SnapshotPort for PackageSnapshots {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        if request
            .source
            .native_source_key
            .starts_with("grok-package:")
            || (self.workspace_trust
                && request
                    .source
                    .native_source_key
                    .starts_with("workspace-trust-"))
        {
            assert!(request.source.path.starts_with(&self.root));
            self.real.snapshot(request, run)
        } else {
            self.scripted.snapshot(request, run)
        }
    }

    fn access_anchor(
        &self,
        revision: &AgentAssetRevision,
    ) -> Option<super::super::verified_path::VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

fn entry(name: &str, source_kind: AgentAssetSourceKind) -> AgentAssetDirectoryEntry {
    AgentAssetDirectoryEntry {
        name: name.to_owned(),
        source_kind,
        is_symlink: false,
    }
}

fn build(
    name: &str,
    user: &[u8],
    project: Option<&[u8]>,
    directories: Vec<(&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: Vec<(&'static str, AgentAssetDiagnostic)>,
) -> (PathBuf, crate::models::AgentEnvironmentInventory) {
    build_with_limits(
        name,
        user,
        project,
        directories,
        blocked,
        AgentAssetLimits::DEFAULT,
    )
}

fn build_with_limits(
    name: &str,
    user: &[u8],
    project: Option<&[u8]>,
    directories: Vec<(&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: Vec<(&'static str, AgentAssetDiagnostic)>,
    limits: AgentAssetLimits,
) -> (PathBuf, crate::models::AgentEnvironmentInventory) {
    let root = test_root(&format!("grok-{name}"));
    fs::create_dir_all(&root).unwrap();
    // Package snapshots use the real no-follow boundary, including ancestors.
    let root = root.canonicalize().unwrap();
    let workspace = root.join("workspace");
    fs::create_dir_all(root.join(".grok")).unwrap();
    fs::create_dir_all(workspace.join(".grok")).unwrap();
    let mut files = vec![("config", user.to_vec())];
    if let Some(project) = project {
        files.push(("workspace-config", project.to_vec()));
    }
    // The existing non-strict scripted port is format-independent; its
    // Claude-specific path assertions are only active in strict mode.
    let mut manifests = directories.iter().flat_map(|(parent, entries)| {
        let skill_root = matches!(*parent, "skills" | "shared-skills" | "workspace-skills" | "workspace-shared-skills");
        entries.iter().filter(move |entry| skill_root && entry.source_kind == AgentAssetSourceKind::Directory && !entry.is_symlink)
            .map(move |entry| (
                format!("grok-skill-manifest:{parent}:{}", entry.name),
                format!("---\nname: {}\ndescription: Isolated native fixture\n---\nFixture instructions.\n", entry.name).into_bytes(),
            ))
    }).collect::<Vec<_>>();
    manifests.extend(directories.iter().flat_map(|(parent, entries)| {
        let hook_root = matches!(*parent, "hooks" | "workspace-hooks");
        entries.iter().filter(move |entry| hook_root && entry.name.ends_with(".json") && !entry.is_symlink)
            .map(move |entry| (
                format!("grok-hook-file:{parent}:{}", entry.name),
                br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-fixture-hook"}]}]}}"#.to_vec(),
            ))
    }));
    prepare_packages(&root, &directories);
    let (snapshots, trace) = ClaudeFixtureSnapshotPort::new(files, directories, blocked);
    snapshots.files.lock().unwrap().extend(manifests);
    let snapshots = PackageSnapshots::new(&root, snapshots);
    let mut installation = fixture_installation();
    installation.agent_kind = AgentCliKind::Grok;
    let (installations, _) = FakeInstallationPort::new(vec![installation]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: Some(&workspace),
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: std::slice::from_ref(definition(AgentCliKind::Grok)),
            limits,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    let trace = trace.lock().unwrap();
    assert_eq!(
        trace.native_source_keys.len(),
        trace
            .native_source_keys
            .iter()
            .collect::<BTreeSet<_>>()
            .len(),
        "seed snapshots must be reused"
    );
    (root, inventory)
}

pub(super) fn assert_hook_policy_declaration(
    inventory: &crate::models::AgentEnvironmentInventory,
) -> &crate::models::AgentAssetDeclaration {
    let policies = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Hook
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .collect::<Vec<_>>();
    assert_eq!(policies.len(), 1, "{policies:?}");
    let policy = policies[0];
    assert_eq!(policy.native_id, "hook-disabled-state");
    assert_eq!(policy.declaration_key, "hook-disabled-state");
    assert_eq!(policy.declared_state, AgentAssetDeclaredState::Unknown);
    assert_eq!(policy.trust_state, AgentTrustState::Unknown);
    assert_eq!(
        policy.participation,
        AgentAssetResolutionParticipation::Participates
    );
    assert!(policy.provided_by.is_none());
    assert!(policy.action_owner.is_none());
    assert!(policy.explicitly_affected.is_empty());
    assert!(policy.evidence.facts.is_empty());
    let source = inventory
        .sources
        .iter()
        .find(|source| source.id == policy.source_id)
        .unwrap();
    let context = inventory
        .contexts
        .iter()
        .find(|context| context.id == policy.context_id)
        .unwrap();
    assert_eq!(source.scope, AgentAssetScope::User);
    assert_eq!(
        source.origin,
        crate::models::AgentAssetInstallationOrigin::ConfigEntry
    );
    assert_eq!(source.categories, vec![AgentAssetCategory::Hook]);
    assert_eq!(
        Path::new(&source.path),
        Path::new(&context.config_root).join("disabled-hooks")
    );
    policy
}

fn assert_admitted(
    inventory: &crate::models::AgentEnvironmentInventory,
    rows: usize,
    scenario_declarations: usize,
) {
    assert_eq!(inventory.assets.len(), rows, "{:?}", inventory.diagnostics);
    let hook_policy = assert_hook_policy_declaration(inventory);
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.id != hook_policy.id
                    && !(declaration.native_kind == AgentAssetCategory::Plugin
                        && declaration.role == AgentAssetDeclarationRole::PolicyOverlay)
            })
            .count(),
        scenario_declarations
    );
    let diagnostics = inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics),
        )
        .chain(
            inventory
                .declarations
                .iter()
                .flat_map(|declaration| &declaration.diagnostics),
        )
        .chain(inventory.assets.iter().flat_map(|asset| {
            asset
                .diagnostics
                .iter()
                .chain(&asset.resolution.diagnostics)
        }));
    assert!(
        diagnostics.clone().all(|diagnostic| !matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
        )),
        "{:?}",
        diagnostics.collect::<Vec<_>>()
    );
}

fn asset<'a>(
    inventory: &'a crate::models::AgentEnvironmentInventory,
    id: &str,
) -> &'a crate::models::AgentAssetRecord {
    inventory
        .assets
        .iter()
        .find(|asset| {
            asset.native_id == id
                && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
        })
        .unwrap()
}

#[test]
fn native_baseline_keeps_assets_and_independent_control_declarations() {
    let (root, inventory) = build(
        "baseline",
        br#"
disabled_mcp_servers = ["disabled", "orphan"]
status_line = true
[mcp_servers.defaulted]
command = "runner"
[mcp_servers.explicit_off]
command = "runner"
enabled = false
[mcp_servers.disabled]
command = "runner"
enabled = true
[plugins]
disabled = ["plugin-one"]
"#,
        None,
        vec![
            (
                "plugins",
                vec![entry("plugin-one", AgentAssetSourceKind::Directory)],
            ),
            (
                "skills",
                vec![entry("skill_one", AgentAssetSourceKind::Directory)],
            ),
            (
                "hooks",
                vec![entry("hook_one.json", AgentAssetSourceKind::File)],
            ),
        ],
        vec![],
    );
    assert_admitted(&inventory, 7, 10);
    assert_eq!(
        asset(&inventory, "defaulted").effective_state,
        AgentAssetState::Enabled
    );
    for id in ["explicit_off", "disabled"] {
        assert_eq!(
            asset(&inventory, id).effective_state,
            AgentAssetState::Disabled
        );
    }
    assert_eq!(
        asset(&inventory, "plugin-one").effective_state,
        AgentAssetState::Unknown
    );
    assert_eq!(
        asset(&inventory, "skill-one").effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        asset(&inventory, "global/hook_one:session_start[0].hooks[0]").effective_state,
        AgentAssetState::Enabled
    );
    let hook = asset(&inventory, "global/hook_one:session_start[0].hooks[0]");
    assert_eq!(hook.declared_state, AgentAssetState::Enabled);
    assert_eq!(hook.trust_state, AgentTrustState::Trusted);
    assert_eq!(
        hook.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        asset(&inventory, "plugin-one").resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        asset(&inventory, "plugin-one").details,
        AgentAssetDetails::Plugin {
            install_state: crate::models::AgentAssetInstallState::Installed,
            ..
        }
    ));
    assert!(inventory
        .assets
        .iter()
        .all(|asset| asset.native_id != "orphan"));
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| declaration.native_id == "orphan"
            && declaration.role == crate::models::AgentAssetDeclarationRole::StateOverlay));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mcp_replacement_keeps_each_entry_and_applies_personal_disable_only_to_winner() {
    for (name, prefix, expected) in [
        ("default", "", AgentAssetState::Enabled),
        (
            "disabled",
            "disabled_mcp_servers = [\"same\"]\n",
            AgentAssetState::Disabled,
        ),
    ] {
        let user = format!("{prefix}[mcp_servers.same]\ncommand = \"low\"\nenabled = false\n[mcp_servers.neighbor]\ncommand = \"safe\"");
        let (root, inventory) = build(
            name,
            user.as_bytes(),
            Some(b"[mcp_servers.same]\nurl = \"https://high.invalid\""),
            vec![],
            vec![],
        );
        assert_admitted(&inventory, 3, if prefix.is_empty() { 3 } else { 4 });
        let winner = asset(&inventory, "same");
        assert_eq!(
            winner.resolution.relation,
            AgentAssetResolutionRelation::ReplaceWinner
        );
        assert_eq!(winner.effective_state, expected);
        assert!(matches!(
            winner.details,
            AgentAssetDetails::Mcp {
                transport: crate::models::AgentMcpTransport::Http,
                ..
            }
        ));
        let loser = inventory
            .assets
            .iter()
            .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .unwrap();
        assert_eq!(loser.declared_state, AgentAssetState::Disabled);
        assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
        assert!(matches!(
            loser.details,
            AgentAssetDetails::Mcp {
                transport: crate::models::AgentMcpTransport::Stdio,
                ..
            }
        ));
        assert_eq!(
            loser.resolution.winner_id.as_deref(),
            Some(winner.stable_id.as_str())
        );
        assert_eq!(
            asset(&inventory, "neighbor").effective_state,
            AgentAssetState::Enabled
        );
        fs::remove_dir_all(root).unwrap();
    }
    let (root, inventory) = build(
        "invalid-high",
        b"[mcp_servers.same]\ncommand = \"low\"",
        Some(b"[mcp_servers.same]\ncommand = 4"),
        vec![],
        vec![],
    );
    assert_admitted(&inventory, 2, 2);
    assert!(matches!(
        asset(&inventory, "same").details,
        AgentAssetDetails::Mcp {
            transport: crate::models::AgentMcpTransport::Unknown,
            ..
        }
    ));
    assert_eq!(
        asset(&inventory, "same").effective_state,
        AgentAssetState::Unknown
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transport_uses_native_typed_entry_and_keeps_invalid_siblings_visible() {
    let (root, inventory) = build(
        "typed-transports",
        br#"
[mcp_servers.numeric_command]
command = 4
[mcp_servers.blank_command]
command = "  "
[mcp_servers.invalid_args]
command = "runner"
args = [4]
[mcp_servers.numeric_url]
url = 4
[mcp_servers.invalid_header]
url = "https://example.invalid"
headers = { Authorization = 4 }
[mcp_servers.type_only]
type = "stdio"
[mcp_servers.stdio_first]
command = "runner"
url = "https://example.invalid/sse"
type = "sse"
[mcp_servers.http_fallback]
command = 4
url = "https://example.invalid"
[mcp_servers.sse_url]
url = "https://example.invalid/sse"
[mcp_servers.sse_type]
url = "https://example.invalid"
type = "SSE"
[mcp_servers.url_alias]
url_template = "https://example.invalid"
"#,
        None,
        vec![],
        vec![],
    );
    assert_admitted(&inventory, 11, 11);
    for id in [
        "numeric_command",
        "blank_command",
        "invalid_args",
        "numeric_url",
        "invalid_header",
        "type_only",
        "stdio_first",
        "http_fallback",
        "sse_type",
    ] {
        let row = asset(&inventory, id);
        assert_eq!(row.declared_state, AgentAssetState::Unknown, "{id}");
        assert_eq!(row.effective_state, AgentAssetState::Unknown, "{id}");
        assert!(
            matches!(
                row.details,
                AgentAssetDetails::Mcp {
                    transport: crate::models::AgentMcpTransport::Unknown,
                    ..
                }
            ),
            "{id}"
        );
    }
    for (id, expected) in [
        ("sse_url", crate::models::AgentMcpTransport::Sse),
        ("url_alias", crate::models::AgentMcpTransport::Http),
    ] {
        let row = asset(&inventory, id);
        assert_eq!(row.effective_state, AgentAssetState::Enabled, "{id}");
        assert!(
            matches!(row.details, AgentAssetDetails::Mcp { transport, .. } if transport == expected),
            "{id}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn truncated_parse_cannot_publish_default_before_later_disable_control() {
    let config = b"disabled_mcp_servers = [\"server\"]\n[mcp_servers.server]\ncommand = \"runner\"";
    let (root, complete) = build("complete-control", config, None, vec![], vec![]);
    assert_admitted(&complete, 1, 2);
    assert_eq!(
        asset(&complete, "server").effective_state,
        AgentAssetState::Disabled
    );
    fs::remove_dir_all(root).unwrap();

    let (root, truncated) = build_with_limits(
        "truncated-control",
        config,
        None,
        vec![],
        vec![],
        AgentAssetLimits {
            first_level_entries: 1,
            ..AgentAssetLimits::DEFAULT
        },
    );
    assert!(truncated.assets.is_empty());
    let hook_policy = assert_hook_policy_declaration(&truncated);
    let partial = truncated
        .declarations
        .iter()
        .filter(|declaration| declaration.id != hook_policy.id)
        .collect::<Vec<_>>();
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].native_kind, AgentAssetCategory::Mcp);
    assert_eq!(partial[0].native_id, "server");
    assert!(matches!(
        (partial[0].declaration_key.as_str(), partial[0].role),
        ("mcp_servers.server", AgentAssetDeclarationRole::Definition)
            | (
                "disabled_mcp_servers:server",
                AgentAssetDeclarationRole::StateOverlay
            )
    ));
    assert_eq!(truncated.sources.len(), 25);
    assert_eq!(
        truncated
            .sources
            .iter()
            .map(|source| Path::new(&source.path).strip_prefix(&root).unwrap())
            .collect::<BTreeSet<_>>(),
        [
            ".grok/config.toml",
            ".grok/auth.json",
            ".grok/skills",
            ".agents/skills",
            ".grok/hooks",
            ".grok/disabled-hooks",
            ".grok/managed_config.toml",
            ".grok/requirements.toml",
            "workspace/.grok/config.toml",
            "workspace/.agents/skills",
            "workspace/.grok/skills",
            "workspace/.grok/hooks",
            "workspace/.grok/hooks/hooks.json",
            ".grok/plugins",
            "workspace/.grok/plugins",
            ".grok/bundled",
            ".grok/trusted_folders.toml",
            "",
            ".git",
            ".git/HEAD",
            ".git/config",
            "workspace",
            "workspace/.git",
            "workspace/.git/HEAD",
            "workspace/.git/config",
        ]
        .into_iter()
        .map(Path::new)
        .collect::<BTreeSet<_>>()
    );
    let diagnostics = truncated
        .sources
        .iter()
        .flat_map(|source| &source.diagnostics);
    assert_eq!(
        diagnostics
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: crate::models::AgentAssetLimitKind::FirstLevelEntries,
                    accepted: 1,
                    observed_at_least: 2,
                }
            ))
            .count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_native_list_members_keep_one_control_and_cannot_restore_defaults() {
    for limit in [AgentAssetLimits::DEFAULT.first_level_entries, 6] {
        let (root, inventory) = build_with_limits(
            &format!("repeated-controls-{limit}"),
            br#"
disabled_mcp_servers = ["server", " server ", "server"]
[mcp_servers.server]
command = "runner"
[mcp_servers.neighbor]
command = "safe"
[plugins]
enabled = ["plugin", "plugin", "enabled", "enabled"]
disabled = ["plugin", "plugin"]
"#,
            Some(b"[plugins]\ndisabled=['plugin','plugin']"),
            vec![(
                "plugins",
                vec![
                    entry("plugin", AgentAssetSourceKind::Directory),
                    entry("enabled", AgentAssetSourceKind::Directory),
                ],
            )],
            vec![],
            AgentAssetLimits {
                first_level_entries: limit,
                ..AgentAssetLimits::DEFAULT
            },
        );
        assert_admitted(&inventory, 4, 9);
        assert_eq!(
            asset(&inventory, "server").declared_state,
            AgentAssetState::Disabled
        );
        assert_eq!(
            asset(&inventory, "server").effective_state,
            AgentAssetState::Disabled
        );
        assert_eq!(
            asset(&inventory, "neighbor").effective_state,
            AgentAssetState::Enabled
        );
        for id in ["plugin", "enabled"] {
            assert_eq!(
                asset(&inventory, id).declared_state,
                AgentAssetState::Unknown
            );
            assert_eq!(
                asset(&inventory, id).effective_state,
                AgentAssetState::Unknown
            );
        }
        assert!(inventory
            .sources
            .iter()
            .flat_map(|source| &source.diagnostics)
            .all(|diagnostic| !matches!(diagnostic, AgentAssetDiagnostic::Truncated { .. })));
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn plugin_lists_preserve_membership_without_promoting_unverified_enablement() {
    for (name, user, project, declarations) in [
        (
            "project-deny",
            "[plugins]\nenabled=[\"plugin\"]",
            Some("[plugins]\ndisabled=[\"plugin\"]"),
            3,
        ),
        (
            "project-enable-ignored",
            "[plugins]\ndisabled=[\"plugin\"]",
            Some("[plugins]\nenabled=[\"plugin\"]"),
            2,
        ),
        ("default-unverified", "", None, 1),
        ("user-enabled", "[plugins]\nenabled=[\"plugin\"]", None, 2),
        (
            "same-layer-deny",
            "[plugins]\nenabled=[\"plugin\"]\ndisabled=[\"plugin\"]",
            None,
            3,
        ),
        (
            "multiple-denials",
            "[plugins]\ndisabled=[\"plugin\"]",
            Some("[plugins]\ndisabled=[\"plugin\"]"),
            3,
        ),
    ] {
        let (root, inventory) = build(
            name,
            user.as_bytes(),
            project.map(str::as_bytes),
            vec![(
                "plugins",
                vec![entry("plugin", AgentAssetSourceKind::Directory)],
            )],
            vec![],
        );
        assert_admitted(&inventory, 1, declarations);
        let plugin = asset(&inventory, "plugin");
        assert_eq!(plugin.declared_state, AgentAssetState::Unknown, "{name}");
        assert_eq!(plugin.effective_state, AgentAssetState::Unknown, "{name}");
        assert_eq!(
            plugin.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(matches!(
            plugin.details,
            AgentAssetDetails::Plugin {
                install_state: crate::models::AgentAssetInstallState::Installed,
                ..
            }
        ));
        assert_eq!(plugin.resolution.terminal, None);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn hook_namespaces_coexist_while_skill_replacement_preserves_native_states() {
    let (root, inventory) = build(
        "hooks-skills",
        b"",
        None,
        vec![
            (
                "hooks",
                vec![entry("hook.json", AgentAssetSourceKind::File)],
            ),
            (
                "workspace-hooks",
                vec![entry("hook.json", AgentAssetSourceKind::File)],
            ),
            (
                "workspace-skills",
                vec![entry("skill", AgentAssetSourceKind::Directory)],
            ),
            (
                "workspace-shared-skills",
                vec![entry("skill", AgentAssetSourceKind::Directory)],
            ),
        ],
        vec![],
    );
    assert_admitted(&inventory, 4, 4);
    let hook_rows = inventory
        .assets
        .iter()
        .filter(|row| row.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(hook_rows.len(), 2);
    for (native_id, scope, path) in [
        (
            "global/hook:session_start[0].hooks[0]",
            AgentAssetScope::User,
            root.join(".grok/hooks/hook.json"),
        ),
        (
            "project/hook:session_start[0].hooks[0]",
            AgentAssetScope::Workspace,
            root.join("workspace/.grok/hooks/hook.json"),
        ),
    ] {
        let hook = asset(&inventory, native_id);
        assert_eq!(hook.scope, scope);
        assert_eq!(hook.path.as_deref().map(Path::new), Some(path.as_path()));
        assert_eq!(
            hook.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(hook.declared_state, AgentAssetState::Enabled);
        assert_eq!(hook.effective_state, AgentAssetState::Enabled);
        assert_eq!(hook.resolution.terminal, None);
        assert!(hook.resolution.winner_id.is_none());
        let definitions = inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_kind == AgentAssetCategory::Hook
                    && declaration.native_id == native_id
            })
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1);
        let definition = definitions[0];
        assert_eq!(definition.role, AgentAssetDeclarationRole::Definition);
        assert_eq!(definition.declaration_key, native_id);
        assert_eq!(
            hook.represented_declaration_ids,
            vec![definition.id.clone()]
        );
        assert_eq!(hook.resolution.contributor_ids, vec![definition.id.clone()]);
        assert_eq!(hook.inspection_source_id, definition.source_id);
    }
    assert_ne!(hook_rows[0].stable_id, hook_rows[1].stable_id);
    let skill = asset(&inventory, "skill");
    assert_eq!(
        skill.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert_eq!(skill.declared_state, AgentAssetState::Enabled);
    assert_eq!(skill.effective_state, AgentAssetState::Enabled);
    assert_eq!(skill.resolution.terminal, None);
    let [winner_source, replaced_source] = [".grok", ".agents"].map(|directory| {
        let manifest = PathBuf::from("workspace")
            .join(directory)
            .join("skills/skill/SKILL.md");
        inventory
            .sources
            .iter()
            .find(|source| Path::new(&source.path).ends_with(&manifest))
            .unwrap()
    });
    let [winner_definition, replaced_definition] = [winner_source, replaced_source].map(|source| {
        assert_eq!(source.scope, AgentAssetScope::Workspace);
        assert_eq!(source.source_kind, AgentAssetSourceKind::File);
        let declarations = inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.source_id == source.id)
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 1);
        let declaration = declarations[0];
        assert_eq!(declaration.native_kind, AgentAssetCategory::Skill);
        assert_eq!(declaration.native_id, "skill");
        assert_eq!(declaration.declaration_key, "SKILL.md");
        assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
        assert_eq!(
            declaration.participation,
            AgentAssetResolutionParticipation::Participates
        );
        declaration
    });
    let definition_ids = BTreeSet::from([
        winner_definition.id.as_str(),
        replaced_definition.id.as_str(),
    ]);
    assert_eq!(
        skill
            .represented_declaration_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        definition_ids
    );
    assert_eq!(
        skill
            .resolution
            .contributor_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        definition_ids
    );
    assert_eq!(
        skill
            .source_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([winner_source.id.as_str(), replaced_source.id.as_str()])
    );
    assert_eq!(skill.inspection_source_id, winner_source.id);
    assert_eq!(skill.path.as_deref(), Some(winner_source.path.as_str()));
    assert_eq!(skill.precedence, 40);
    let replaced = inventory
        .assets
        .iter()
        .find(|row| {
            row.native_id == "skill"
                && row.resolution.relation == AgentAssetResolutionRelation::Replaced
        })
        .unwrap();
    assert_eq!(replaced.declared_state, AgentAssetState::Enabled);
    assert_eq!(replaced.effective_state, AgentAssetState::Shadowed);
    assert_eq!(
        replaced.represented_declaration_ids,
        vec![replaced_definition.id.clone()]
    );
    assert_eq!(
        replaced.resolution.contributor_ids,
        vec![replaced_definition.id.clone()]
    );
    assert_eq!(replaced.inspection_source_id, replaced_source.id);
    assert_eq!(replaced.source_ids, vec![replaced_source.id.clone()]);
    assert_eq!(
        replaced.path.as_deref(),
        Some(replaced_source.path.as_str())
    );
    assert_eq!(replaced.precedence, 30);
    assert_ne!(replaced.stable_id, skill.stable_id);
    assert_eq!(
        replaced.resolution.winner_id.as_deref(),
        Some(skill.stable_id.as_str())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn orphan_control_declarations_do_not_invent_assets() {
    let (root, inventory) = build(
        "orphan",
        b"disabled_mcp_servers=[\"orphan\"]\n[plugins]\ndisabled=[\"plugin\"]",
        None,
        vec![],
        vec![],
    );
    assert_admitted(&inventory, 0, 2);
    let controls = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.role == AgentAssetDeclarationRole::StateOverlay)
        .map(|declaration| (declaration.native_kind, declaration.native_id.as_str()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        controls,
        BTreeSet::from([
            (AgentAssetCategory::Mcp, "orphan"),
            (AgentAssetCategory::Plugin, "plugin"),
        ])
    );
    assert_eq!(inventory.declarations.len(), 3);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_plugin_replaces_user_installation_without_losing_either_install_state() {
    let (root, inventory) = build(
        "plugin-roots",
        b"",
        None,
        vec![
            (
                "plugins",
                vec![entry("plugin", AgentAssetSourceKind::Directory)],
            ),
            (
                "workspace-plugins",
                vec![entry("plugin", AgentAssetSourceKind::Directory)],
            ),
        ],
        vec![],
    );
    assert_admitted(&inventory, 2, 2);
    let winner = asset(&inventory, "plugin");
    assert_eq!(winner.scope, AgentAssetScope::Workspace);
    assert_eq!(
        winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert_eq!(winner.effective_state, AgentAssetState::Unknown);
    for asset in &inventory.assets {
        assert_eq!(asset.declared_state, AgentAssetState::Unknown);
        assert!(matches!(
            asset.details,
            AgentAssetDetails::Plugin {
                install_state: crate::models::AgentAssetInstallState::Installed,
                ..
            }
        ));
    }
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.effective_state == AgentAssetState::Shadowed)
            .count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_controls_keep_declared_state_and_passive_inventory_hides_values() {
    let secret = "grok-fixture-never-publish-value";
    let config = format!("disabled_mcp_servers = 5\n[mcp_servers.server]\ncommand = \"{secret}\"\n[mcp_servers.server.env]\nTOKEN = \"{secret}\"");
    let (root, inventory) = build(
        "invalid-control",
        config.as_bytes(),
        None,
        vec![(
            "skills",
            vec![entry("neighbor", AgentAssetSourceKind::Directory)],
        )],
        vec![],
    );
    assert_admitted(&inventory, 2, 3);
    assert_eq!(
        asset(&inventory, "server").declared_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        asset(&inventory, "server").effective_state,
        AgentAssetState::Unknown
    );
    assert!(matches!(
        asset(&inventory, "server").resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { .. })
    ));
    assert!(!serde_json::to_string(&inventory).unwrap().contains(secret));
    assert!(!root.join(secret).exists());
    fs::remove_dir_all(root).unwrap();
}
