//! Real bounded snapshots and native adapters prove configured Hook counts.
use super::*;
use crate::models::{AgentAssetIoErrorKind, AgentEnvironmentInventory, AppSettings};
use crate::services::agent_cli::contracts::AgentAssetSnapshot;

struct HookSnapshots {
    root: PathBuf,
    real: RealSnapshotPort,
    blocked: Mutex<BTreeSet<PathBuf>>,
    reads: Mutex<Vec<PathBuf>>,
}

impl SnapshotPort for HookSnapshots {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let revision = AgentAssetRevision {
            identity: format!("fixture:{}", request.source_id),
            observed_at: "2026-09-15T00:00:00Z".to_owned(),
            is_missing: true,
            ..AgentAssetRevision::default()
        };
        // System/native roots stay missing in this isolated fixture; no test
        // reads the developer's config or executes a native Agent.
        if !request.source.path.starts_with(&self.root) {
            return AgentAssetSnapshot::Missing { revision };
        }
        self.reads.lock().unwrap().push(request.source.path.clone());
        if self.blocked.lock().unwrap().contains(&request.source.path) {
            return AgentAssetSnapshot::Blocked {
                revision: AgentAssetRevision {
                    is_missing: false,
                    ..revision
                },
                diagnostic: AgentAssetDiagnostic::ReadFailed {
                    source_id: request.source_id.to_owned(),
                    error_kind: AgentAssetIoErrorKind::PermissionDenied,
                },
            };
        }
        self.real.snapshot(request, run)
    }

    fn access_anchor(
        &self,
        revision: &AgentAssetRevision,
    ) -> Option<super::super::verified_path::VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

struct HookFixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    snapshots: HookSnapshots,
}

struct HookDiagnosticPort(AgentAssetDiagnostic);

impl InstallationDiscoveryPort for HookDiagnosticPort {
    fn discover(
        &self,
        _request: InstallationDiscoveryRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        run.emit_inventory_diagnostic(self.0.clone());
        Vec::new()
    }
}

impl HookFixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        Self {
            _temporary: temporary,
            home: home.clone(),
            snapshots: HookSnapshots {
                root: home,
                real: RealSnapshotPort::default(),
                blocked: Mutex::new(BTreeSet::new()),
                reads: Mutex::new(Vec::new()),
            },
        }
    }

    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.home.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn scan(&self, kind: AgentCliKind) -> AgentEnvironmentInventory {
        self.scan_with(
            native_definition(kind),
            None,
            vec![],
            AgentAssetLimits::DEFAULT,
        )
    }

    fn scan_with(
        &self,
        registered: AgentCliDefinition,
        workspace: Option<&Path>,
        installations: Vec<AgentInstallation>,
        limits: AgentAssetLimits,
    ) -> AgentEnvironmentInventory {
        let (installations, _) = FakeInstallationPort::new(installations);
        let settings = AppSettings::default();
        build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace,
                settings: Some(&settings),
            },
            InventoryPipelineDeps {
                definitions: &[registered],
                limits,
                clock: Arc::new(ManualClock::new()),
                installations: &installations,
                snapshots: &self.snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap()
    }

    fn scan_all_with_diagnostic(
        &self,
        diagnostic: AgentAssetDiagnostic,
    ) -> AgentEnvironmentInventory {
        let settings = AppSettings::default();
        build_inventory_with(
            InventoryInput {
                home: &self.home,
                workspace: None,
                settings: Some(&settings),
            },
            InventoryPipelineDeps {
                definitions: &KINDS.map(native_definition),
                limits: AgentAssetLimits::DEFAULT,
                clock: Arc::new(ManualClock::new()),
                installations: &HookDiagnosticPort(diagnostic),
                snapshots: &self.snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap()
    }
}

fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let mut contexts = definition(request.agent_kind)
        .environment()
        .discover_contexts(request, output);
    let directory = match request.agent_kind {
        AgentCliKind::ClaudeCode => ".claude",
        AgentCliKind::Codex => ".codex",
        AgentCliKind::Gemini => ".gemini",
        AgentCliKind::Grok => ".grok",
    };
    for context in &mut contexts {
        // Host CLI profile overrides must never redirect fixture discovery.
        context.config_root = request.home.join(directory).to_string_lossy().into_owned();
    }
    contexts
}

fn native_definition(kind: AgentCliKind) -> AgentCliDefinition {
    let native = *definition(kind);
    AgentCliDefinition {
        environment: native.environment().with_test_contexts(fixture_contexts),
        ..native
    }
}

fn count(inventory: &AgentEnvironmentInventory, kind: AgentCliKind) -> Option<u32> {
    let rows = inventory
        .hook_rule_counts
        .iter()
        .filter(|row| row.agent_kind == kind)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "{inventory:?}");
    rows[0].rule_count
}

fn hooks(inventory: &AgentEnvironmentInventory) -> Vec<&crate::models::AgentAssetRecord> {
    inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
        .collect()
}

fn hook_file(kind: AgentCliKind) -> &'static str {
    match kind {
        AgentCliKind::Codex => ".codex/hooks.json",
        AgentCliKind::ClaudeCode => ".claude/settings.json",
        AgentCliKind::Gemini => ".gemini/settings.json",
        AgentCliKind::Grok => ".grok/hooks/configured.json",
    }
}

const KINDS: [AgentCliKind; 4] = [
    AgentCliKind::Codex,
    AgentCliKind::ClaudeCode,
    AgentCliKind::Gemini,
    AgentCliKind::Grok,
];
const ONE_RULE: &str = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-test-only-command"}]}]}}"#;

#[test]
fn hook_counts_executable_discovery_limits_do_not_obscure_configured_rules() {
    let fixture = HookFixture::new();
    for kind in KINDS {
        fixture.write(hook_file(kind), ONE_RULE);
    }
    for limit in [
        AgentAssetLimitKind::CandidatePathsPerAgent,
        AgentAssetLimitKind::InstallationsPerAgent,
        AgentAssetLimitKind::CliOutput,
        AgentAssetLimitKind::CliConcurrency,
    ] {
        let diagnostic = AgentAssetDiagnostic::Truncated {
            limit,
            accepted: 32,
            observed_at_least: 33,
        };
        let inventory = fixture.scan_all_with_diagnostic(diagnostic.clone());
        assert!(inventory.diagnostics.contains(&diagnostic), "{inventory:?}");
        assert_eq!(inventory.contexts.len(), KINDS.len(), "{inventory:?}");
        assert_eq!(hooks(&inventory).len(), KINDS.len(), "{inventory:?}");
        for kind in KINDS {
            assert_eq!(count(&inventory, kind), Some(1), "{kind:?}: {inventory:?}");
        }
    }
}

#[test]
fn hook_counts_configuration_budget_and_truncation_keep_counts_unknown() {
    let fixture = HookFixture::new();
    for kind in KINDS {
        fixture.write(hook_file(kind), ONE_RULE);
    }
    let diagnostics = [
        AgentAssetLimitKind::SourcesPerContext,
        AgentAssetLimitKind::FirstLevelEntries,
        AgentAssetLimitKind::BytesPerSource,
        AgentAssetLimitKind::BytesPerRefresh,
        AgentAssetLimitKind::RefreshBudget,
        AgentAssetLimitKind::Diagnostics,
    ]
    .map(|limit| AgentAssetDiagnostic::Truncated {
        limit,
        accepted: 1,
        observed_at_least: 2,
    })
    .into_iter()
    .chain(std::iter::once(AgentAssetDiagnostic::BudgetExceeded {
        elapsed_ms: 2,
        budget_ms: 1,
    }));
    for diagnostic in diagnostics {
        let inventory = fixture.scan_all_with_diagnostic(diagnostic.clone());
        assert!(inventory.diagnostics.contains(&diagnostic), "{inventory:?}");
        assert_eq!(hooks(&inventory).len(), KINDS.len(), "{inventory:?}");
        for kind in KINDS {
            assert_eq!(count(&inventory, kind), None, "{kind:?}: {inventory:?}");
        }
    }
}

#[test]
fn hook_counts_codex_counts_handlers_in_one_file_and_excludes_metadata() {
    let fixture = HookFixture::new();
    let sentinel = fixture.home.join("must-not-run");
    let command = format!("touch {}", sentinel.display());
    fixture.write(".codex/hooks.json", serde_json::to_vec(&serde_json::json!({
        "description": "not a rule",
        "hooks": {
            "SessionStart": [
                {"hooks": [{"type":"command", "command": command}, {"type":"mcp_tool", "server":"fixture", "tool":"observe", "input": {}}]},
                {"matcher":"resume", "hooks": [{"type":"command", "command":"bh-second-group"}]}
            ],
            "SessionEnd": [{"hooks":[{"type":"command", "command":"bh-end", "timeout":3}]}],
            "Stop": []
        }
    })).unwrap());
    fixture.write(
        ".codex/config.toml",
        "[tui]\nstatus_line = ['model-name']\n",
    );
    let inventory = fixture.scan(AgentCliKind::Codex);
    assert!(inventory.installations.is_empty());
    assert_eq!(count(&inventory, AgentCliKind::Codex), Some(4));
    let rows = hooks(&inventory);
    assert_eq!(rows.len(), 4, "{inventory:?}");
    let source_path = fixture.home.join(".codex/hooks.json");
    assert_eq!(
        rows.iter()
            .map(|row| row.native_id.clone())
            .collect::<BTreeSet<_>>(),
        [
            "session_start:0:0",
            "session_start:0:1",
            "session_start:1:0",
            "session_end:0:0"
        ]
        .into_iter()
        .map(|suffix| format!("{}:{suffix}", source_path.display()))
        .collect()
    );
    assert!(rows.iter().all(|row| matches!(
        row.details,
        AgentAssetDetails::Hook {
            rule_count: Some(1),
            ..
        }
    )));
    assert!(inventory
        .assets
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::StatusUi));
    assert!(!sentinel.exists());
}

#[test]
fn hook_counts_grok_reads_only_direct_json_and_counts_command_and_http_rules() {
    let fixture = HookFixture::new();
    fixture.write(".grok/hooks/configured.json", r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-one"},{"type":"http","url":"https://fixture.invalid/hook"}]}],"Stop":[{"hooks":[{"type":"command","command":"bh-two","env":{"SAFE":"fixture"}}]}]}}"#);
    for path in [
        ".grok/hooks/ignored.sh",
        ".grok/hooks/.hidden.json",
        ".grok/hooks/.json",
        ".grok/hooks/backup.json~",
        ".grok/hooks/nested/child.json",
    ] {
        fixture.write(path, ONE_RULE);
    }
    let inventory = fixture.scan(AgentCliKind::Grok);
    assert_eq!(count(&inventory, AgentCliKind::Grok), Some(3));
    let rows = hooks(&inventory);
    assert_eq!(rows.len(), 3, "{inventory:?}");
    assert_eq!(
        rows.iter()
            .map(|row| row.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "global/configured:session_start[0].hooks[0]",
            "global/configured:session_start[0].hooks[1]",
            "global/configured:stop[0].hooks[0]"
        ])
    );
    assert!(rows.iter().all(|row| row
        .path
        .as_ref()
        .is_some_and(|path| path.ends_with("hooks/configured.json"))));
    assert!(rows.iter().all(|row| matches!(
        row.details,
        AgentAssetDetails::Hook {
            rule_count: Some(1),
            ..
        }
    )));
    let reads = fixture.snapshots.reads.lock().unwrap();
    assert_eq!(
        reads
            .iter()
            .filter(|path| path.ends_with("hooks/configured.json"))
            .count(),
        1
    );
    assert!(!reads.iter().any(|path| [
        "ignored.sh",
        ".hidden.json",
        ".json",
        "backup.json~",
        "child.json"
    ]
    .iter()
    .any(|name| path.file_name().is_some_and(|file| file == *name))));
}

#[test]
fn hook_counts_claude_and_gemini_use_handler_facts_without_counting_policy_or_status() {
    for kind in [AgentCliKind::ClaudeCode, AgentCliKind::Gemini] {
        let fixture = HookFixture::new();
        let mut value = serde_json::json!({"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","name":"first","command":"bh-first"},
            {"type":"command","name":"second","command":"bh-second"}
        ]}]}});
        if kind == AgentCliKind::ClaudeCode {
            value["disableAllHooks"] = true.into();
            value["statusLine"] = serde_json::json!({"type":"command","command":"bh-status-only"});
        } else {
            value["hooksConfig"] = serde_json::json!({"disabled":["first"]});
        }
        fixture.write(hook_file(kind), serde_json::to_vec(&value).unwrap());
        let inventory = fixture.scan(kind);
        assert_eq!(count(&inventory, kind), Some(2), "{inventory:?}");
        assert_eq!(hooks(&inventory).len(), 2, "{inventory:?}");
        assert!(hooks(&inventory).iter().all(|asset| matches!(
            asset.details,
            AgentAssetDetails::Hook {
                rule_count: Some(1),
                ..
            }
        )));
        assert!(inventory
            .declarations
            .iter()
            .any(
                |declaration| declaration.native_kind == AgentAssetCategory::Hook
                    && declaration.role != AgentAssetDeclarationRole::Definition
            ));
    }
}

#[test]
fn claude_hook_labels_describe_events_without_changing_native_identity() {
    let fixture = HookFixture::new();
    let kind = AgentCliKind::ClaudeCode;
    fixture.write(
        hook_file(kind),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bh-first"},{"type":"command","command":"bh-second"}]}]}}"#,
    );
    let inventory = fixture.scan(kind);
    let rows = hooks(&inventory);
    assert_eq!(count(&inventory, kind), Some(2));
    assert_eq!(rows.len(), 2, "{inventory:?}");
    assert_eq!(
        rows.iter()
            .map(|row| row.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        [
            "settings:PreToolUse:matcher-string-present:group-0:hook-0",
            "settings:PreToolUse:matcher-string-present:group-0:hook-1",
        ]
        .into_iter()
        .collect()
    );
    for row in rows {
        assert_eq!(row.label, "Claude Code Hook：PreToolUse");
        let declarations = inventory
            .declarations
            .iter()
            .filter(|declaration| row.represented_declaration_ids.contains(&declaration.id))
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 1, "{row:?}");
        assert_eq!(declarations[0].label, row.label);
        assert_eq!(declarations[0].declaration_key, row.native_id);
    }

    fixture.write(
        hook_file(kind),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command"}]}]}}"#,
    );
    let malformed = fixture.scan(kind);
    let rows = hooks(&malformed);
    assert_eq!(count(&malformed, kind), None);
    assert_eq!(rows.len(), 1, "{malformed:?}");
    assert_eq!(rows[0].label, "Claude Code Hook：PreToolUse");
    assert_eq!(
        rows[0].native_id,
        "settings:PreToolUse:matcher-string-present:group-0:hook-invalid"
    );
}

#[test]
fn hook_counts_missing_empty_and_unreadable_sources_do_not_conflate_zero_and_unknown() {
    for kind in KINDS {
        let fixture = HookFixture::new();
        assert_eq!(count(&fixture.scan(kind), kind), Some(0), "{kind:?}");
        let path = fixture.write(hook_file(kind), r#"{"hooks":{}}"#);
        assert_eq!(count(&fixture.scan(kind), kind), Some(0), "{kind:?}");
        fixture
            .snapshots
            .blocked
            .lock()
            .unwrap()
            .insert(path.clone());
        let blocked = fixture.scan(kind);
        assert_eq!(count(&blocked, kind), None);
        assert!(hooks(&blocked).is_empty(), "{blocked:?}");
        assert!(blocked
            .sources
            .iter()
            .any(|source| source.path == path.to_string_lossy()
                && source.diagnostics.iter().any(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::ReadFailed { .. }
                ))));
        fixture.snapshots.blocked.lock().unwrap().clear();
        assert_eq!(count(&fixture.scan(kind), kind), Some(0));
    }
}

#[test]
fn hook_counts_malformed_unknown_and_truncated_sources_preserve_unknown() {
    for kind in KINDS {
        let fixture = HookFixture::new();
        for bytes in [
            "{ broken",
            "[]",
            r#"{"hooks":[]}"#,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-valid"},{"type":"unknown","command":"bh-not-a-rule"}]}]}}"#,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command"}]}]}}"#,
        ] {
            fixture.write(hook_file(kind), bytes);
            let inventory = fixture.scan(kind);
            assert_eq!(count(&inventory, kind), None, "{kind:?}: {inventory:?}");
        }
        if matches!(kind, AgentCliKind::Codex | AgentCliKind::Grok) {
            fixture.write(hook_file(kind), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-valid"}]}]},"hooks":{}}"#);
            assert_eq!(count(&fixture.scan(kind), kind), None);
        }
        fixture.write(hook_file(kind), ONE_RULE);
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.bytes_per_source = 16;
        let inventory = fixture.scan_with(native_definition(kind), None, vec![], limits);
        assert_eq!(count(&inventory, kind), None, "{inventory:?}");
    }
}

fn isolated_claude_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let (sources, diagnostics) = {
        let mut captured = ReorderingInitialSourceOutput {
            output: Some(output),
            ..Default::default()
        };
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .discover_sources(request, &mut captured);
        (captured.sources, captured.diagnostics)
    };
    for diagnostic in diagnostics {
        output.emit_diagnostic(diagnostic);
    }
    for mut source in sources {
        if source.native_source_key == "managed-settings" {
            source.allowed_root = request.home.join("fixture-system");
            source.path = source.allowed_root.join("managed-settings.json");
        }
        if output.emit_initial(source).is_break() {
            return;
        }
    }
}

fn isolated_claude_definition() -> AgentCliDefinition {
    let native = native_definition(AgentCliKind::ClaudeCode);
    AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            fixture_contexts,
            isolated_claude_sources,
            Some(|request, output| {
                definition(AgentCliKind::ClaudeCode)
                    .environment()
                    .discover_follow_up_sources(request, output)
            }),
            "fixture-hook-count",
            |request, output| {
                definition(AgentCliKind::ClaudeCode)
                    .environment()
                    .parse(request, output)
            },
            |request, output| {
                definition(AgentCliKind::ClaudeCode)
                    .environment()
                    .resolve(request, output)
            },
            native.environment().state_assessor(),
        ),
        ..native
    }
}

#[test]
fn hook_counts_unrelated_malformed_fields_and_rejected_mcp_ids_preserve_hook_count() {
    for (kind, field) in [
        (AgentCliKind::ClaudeCode, "statusLine"),
        (AgentCliKind::Gemini, "mcp"),
    ] {
        let fixture = HookFixture::new();
        let mut value: serde_json::Value = serde_json::from_str(ONE_RULE).unwrap();
        value[field] = serde_json::json!(7);
        fixture.write(hook_file(kind), serde_json::to_vec(&value).unwrap());
        let inventory = fixture.scan(kind);
        assert_eq!(count(&inventory, kind), Some(1), "{inventory:?}");
        assert!(inventory.sources.iter().flat_map(|source| &source.diagnostics).any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed { location: Some(location), .. } if location == field
        )), "{inventory:?}");
    }
    for invalid_mcp in [
        serde_json::json!(7),
        serde_json::json!({"": {"command":"bh-invalid-id"}}),
    ] {
        let fixture = HookFixture::new();
        let mut value: serde_json::Value = serde_json::from_str(ONE_RULE).unwrap();
        value["managedMcpServers"] = invalid_mcp;
        fixture.write(
            "fixture-system/managed-settings.json",
            serde_json::to_vec(&value).unwrap(),
        );
        let inventory = fixture.scan_with(
            isolated_claude_definition(),
            None,
            vec![],
            AgentAssetLimits::DEFAULT,
        );
        assert_eq!(
            count(&inventory, AgentCliKind::ClaudeCode),
            Some(1),
            "{inventory:?}"
        );
        assert!(
            inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics)
                .any(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::Malformed { .. }
                        | AgentAssetDiagnostic::InvalidProjection { .. }
                )),
            "{inventory:?}"
        );
    }
}

fn fixture_plugin(
    fixture: &HookFixture,
    kind: AgentCliKind,
    manifest: serde_json::Value,
) -> PathBuf {
    let relative = match kind {
        AgentCliKind::ClaudeCode => "packages/fixture/.claude-plugin/plugin.json",
        AgentCliKind::Codex => ".codex/plugins/cache/local/fixture/1.0.0/.codex-plugin/plugin.json",
        AgentCliKind::Grok => ".grok/plugins/fixture/plugin.json",
        AgentCliKind::Gemini => unreachable!("Gemini extension Hook files have a native parser"),
    };
    let path = fixture.write(relative, serde_json::to_vec(&manifest).unwrap());
    match kind {
        AgentCliKind::ClaudeCode => {
            fixture.write(".claude/plugins/installed_plugins.json", serde_json::to_vec(&serde_json::json!({
                "version": 2,
                "plugins": {"fixture@local":[{"scope":"user", "installPath":fixture.home.join("packages/fixture"), "version":"1.0.0"}]}
            })).unwrap());
        }
        AgentCliKind::Codex => {
            fixture.write(
                ".codex/config.toml",
                "[plugins.\"fixture@local\"]\nenabled=true\n",
            );
        }
        AgentCliKind::Grok => {}
        AgentCliKind::Gemini => unreachable!(),
    }
    path
}

#[test]
fn hook_counts_plugin_files_contribute_rules_but_invalid_metadata_remains_incomplete() {
    for kind in [
        AgentCliKind::ClaudeCode,
        AgentCliKind::Codex,
        AgentCliKind::Grok,
    ] {
        for name in [serde_json::json!("fixture"), serde_json::json!(7)] {
            let fixture = HookFixture::new();
            fixture.write(hook_file(kind), ONE_RULE);
            let manifest = fixture_plugin(&fixture, kind, serde_json::json!({"name":"fixture"}));
            assert_eq!(count(&fixture.scan(kind), kind), Some(1));
            let root = if kind == AgentCliKind::Grok {
                manifest.parent().unwrap()
            } else {
                manifest.parent().unwrap().parent().unwrap()
            };
            fs::write(root.join("hooks.json"), ONE_RULE).unwrap();
            let valid = name.is_string();
            fs::write(
                &manifest,
                serde_json::to_vec(&serde_json::json!({"name":name,"hooks":"./hooks.json"}))
                    .unwrap(),
            )
            .unwrap();
            let inventory = fixture.scan(kind);
            assert_eq!(
                count(&inventory, kind),
                if valid { Some(2) } else { None },
                "{kind:?}: {inventory:?}"
            );
            assert!(fixture.snapshots.reads.lock().unwrap().contains(&manifest));
            assert!(hooks(&inventory).iter().any(|asset| matches!(
                asset.details,
                AgentAssetDetails::Hook {
                    rule_count: Some(1),
                    ..
                }
            )));
            assert_eq!(inventory.sources.iter().flat_map(|source| &source.diagnostics).chain(&inventory.diagnostics).any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::DiscoveryIncomplete { agent_kind, category:AgentAssetCategory::Hook, .. } if *agent_kind == kind
            )), !valid, "{inventory:?}");
        }
    }
}

#[test]
fn hook_counts_observed_plugin_hook_directories_do_not_claim_an_empty_configuration() {
    for (kind, hooks_path) in [
        (
            AgentCliKind::ClaudeCode,
            "packages/fixture/hooks/hooks.json",
        ),
        (
            AgentCliKind::Codex,
            ".codex/plugins/cache/local/fixture/1.0.0/hooks/hooks.json",
        ),
        (AgentCliKind::Grok, ".grok/plugins/fixture/hooks/hooks.json"),
    ] {
        let fixture = HookFixture::new();
        fixture_plugin(&fixture, kind, serde_json::json!({"name":"fixture"}));
        assert_eq!(count(&fixture.scan(kind), kind), Some(0));
        let hook_path = fixture.write(hooks_path, ONE_RULE);
        fixture.snapshots.blocked.lock().unwrap().insert(hook_path);
        let inventory = fixture.scan(kind);
        assert_eq!(count(&inventory, kind), None, "{inventory:?}");
        assert!(hooks(&inventory).is_empty());
    }
}

fn duplicated_contexts<const REVERSE: bool>(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let mut contexts = fixture_contexts(request, output);
    let mut second = contexts[0].clone();
    second.id.clear();
    second.profile = "other-profile".to_owned();
    contexts.push(second);
    if REVERSE {
        contexts.reverse();
    }
    contexts
}

fn repeated_context_definition<const REVERSE: bool>() -> AgentCliDefinition {
    let native = native_definition(AgentCliKind::Grok);
    AgentCliDefinition {
        environment: native
            .environment()
            .with_test_contexts(duplicated_contexts::<REVERSE>),
        ..native
    }
}

#[test]
fn hook_counts_repeated_installations_and_contexts_do_not_multiply_physical_configuration() {
    let fixture = HookFixture::new();
    fixture.write(hook_file(AgentCliKind::Grok), ONE_RULE);
    let installations = (0..2)
        .map(|index| {
            let mut installation = fixture_installation();
            installation.id = format!("installation:hook:{index}");
            installation.agent_kind = AgentCliKind::Grok;
            let path = format!("/fixture/bin/grok-{index}");
            installation.executable_path = Some(path.clone());
            installation
                .executable_identity
                .as_mut()
                .unwrap()
                .canonical_path = path;
            installation
        })
        .collect::<Vec<_>>();
    let first = fixture.scan_with(
        repeated_context_definition::<false>(),
        None,
        installations.clone(),
        AgentAssetLimits::DEFAULT,
    );
    let reversed = fixture.scan_with(
        repeated_context_definition::<true>(),
        None,
        installations,
        AgentAssetLimits::DEFAULT,
    );
    for inventory in [&first, &reversed] {
        assert_eq!(inventory.installations.len(), 2);
        assert_eq!(inventory.contexts.len(), 2);
        assert_eq!(hooks(inventory).len(), 2, "{inventory:?}");
        assert_eq!(count(inventory, AgentCliKind::Grok), Some(1));
    }
    let row_ids = |inventory: &AgentEnvironmentInventory| {
        hooks(inventory)
            .iter()
            .map(|asset| asset.stable_id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(row_ids(&first), row_ids(&reversed));
}

#[test]
fn hook_counts_distinct_user_and_workspace_files_remain_two_configured_definitions() {
    let fixture = HookFixture::new();
    fixture.write(".grok/hooks/configured.json", ONE_RULE);
    fixture.write("workspace/.grok/hooks/configured.json", ONE_RULE);
    let workspace = fixture.home.join("workspace");
    let inventory = fixture.scan_with(
        native_definition(AgentCliKind::Grok),
        Some(&workspace),
        vec![],
        AgentAssetLimits::DEFAULT,
    );
    assert_eq!(hooks(&inventory).len(), 2, "{inventory:?}");
    assert_eq!(count(&inventory, AgentCliKind::Grok), Some(2));
    assert!(hooks(&inventory).iter().all(|asset| matches!(
        asset.details,
        AgentAssetDetails::Hook {
            rule_count: Some(1),
            ..
        }
    )));
}

#[test]
fn hook_counts_user_inline_sources_accumulate_with_standalone_files() {
    for (kind, path) in [
        (AgentCliKind::Codex, ".codex/config.toml"),
        (AgentCliKind::Grok, ".grok/config.toml"),
    ] {
        let fixture = HookFixture::new();
        fixture.write(hook_file(kind), ONE_RULE);
        fixture.write(path, "[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype='command'\ncommand='bh-inline'\n");
        let inventory = fixture.scan(kind);
        assert_eq!(count(&inventory, kind), Some(2), "{inventory:?}");
        assert_eq!(hooks(&inventory).len(), 2);
        assert!(hooks(&inventory).iter().any(|asset| matches!(
            asset.details,
            AgentAssetDetails::Hook {
                rule_count: Some(1),
                ..
            }
        )));
        assert!(!inventory
            .sources
            .iter()
            .flat_map(|source| &source.diagnostics)
            .any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::DiscoveryIncomplete {
                    category: AgentAssetCategory::Hook,
                    ..
                }
            )));
    }
}

#[cfg(unix)]
#[test]
fn hook_counts_grok_rejected_json_symlink_is_unknown_without_reading_its_target() {
    let fixture = HookFixture::new();
    let target = fixture.write("outside/secret.json", ONE_RULE);
    fs::create_dir_all(fixture.home.join(".grok/hooks")).unwrap();
    std::os::unix::fs::symlink(&target, fixture.home.join(".grok/hooks/link.json")).unwrap();
    let inventory = fixture.scan(AgentCliKind::Grok);
    assert_eq!(count(&inventory, AgentCliKind::Grok), None, "{inventory:?}");
    assert!(hooks(&inventory).is_empty());
    assert!(!fixture.snapshots.reads.lock().unwrap().contains(&target));
    assert!(inventory
        .sources
        .iter()
        .flat_map(|source| &source.diagnostics)
        .chain(&inventory.diagnostics)
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::SymlinkRejected { .. })));
}

#[test]
fn hook_counts_unreadable_policy_does_not_erase_known_grok_configuration() {
    let fixture = HookFixture::new();
    fixture.write(hook_file(AgentCliKind::Grok), ONE_RULE);
    let policy = fixture.write(".grok/disabled-hooks", "# fixture policy\n");
    fixture.snapshots.blocked.lock().unwrap().insert(policy);
    let inventory = fixture.scan(AgentCliKind::Grok);
    assert_eq!(count(&inventory, AgentCliKind::Grok), Some(1));
    let rows = hooks(&inventory);
    assert_eq!(rows.len(), 1, "{inventory:?}");
    assert_eq!(rows[0].effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        rows[0].details,
        AgentAssetDetails::Hook {
            rule_count: Some(1),
            ..
        }
    ));
}

#[test]
fn hook_counts_codex_unknown_root_metadata_is_not_a_loadable_hooks_file() {
    let fixture = HookFixture::new();
    fixture.write(".codex/hooks.json", r#"{"customMetadata":{"keep":true},"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bh-fixture"}]}]}}"#);
    let inventory = fixture.scan(AgentCliKind::Codex);
    assert_eq!(count(&inventory, AgentCliKind::Codex), None);
    assert!(hooks(&inventory).is_empty());
}

#[test]
fn codex_plugin_catalog_reads_exact_handlers_without_exposing_package_write_targets() {
    use crate::services::agent_cli::environment::mutation::MutationInventory;
    for inline in [false, true] {
        let fixture = HookFixture::new();
        let file = serde_json::json!({"hooks":{"SessionStart":[{"hooks":[
            {"type":"command","command":"bh-independent-hook"},
            {"type":"command","command":"${PLUGIN_ROOT}/scripts/bh-hook"}
        ]}]}});
        let manifest = fixture_plugin(
            &fixture,
            AgentCliKind::Codex,
            if inline {
                serde_json::json!({"name":"fixture","hooks":[file]})
            } else {
                serde_json::json!({"name":"fixture"})
            },
        );
        let path = if inline {
            manifest
        } else {
            fixture.write(
                ".codex/plugins/cache/local/fixture/1.0.0/hooks/hooks.json",
                serde_json::to_vec(&file).unwrap(),
            )
        };
        let before = fs::read(&path).unwrap();
        let inventory = fixture.scan(AgentCliKind::Codex);
        assert_eq!(
            count(&inventory, AgentCliKind::Codex),
            Some(2),
            "{inventory:?}"
        );
        assert_eq!(hooks(&inventory).len(), 2, "{inventory:?}");
        let source_anchors = inventory
            .sources
            .iter()
            .filter_map(|source| {
                fixture
                    .snapshots
                    .access_anchor(&source.revision)
                    .map(|anchor| (source.id.clone(), anchor))
            })
            .collect();
        let snapshot = MutationInventory {
            inventory,
            source_anchors,
        };
        let adapter = definition(AgentCliKind::Codex)
            .environment()
            .hook_adapter()
            .unwrap();
        let targets = (adapter.hook_targets)(&snapshot.inventory, &snapshot.inventory.contexts[0]);
        assert!(!targets.is_empty());
        for asset in hooks(&snapshot.inventory) {
            assert!(!asset.writable);
            assert!(!targets
                .iter()
                .any(|target| target.source_id == asset.inspection_source_id));
            let rule = (adapter.read_hook)(&snapshot, asset).unwrap();
            assert_eq!(
                rule.native_asset_id.as_deref(),
                Some(asset.stable_id.as_str())
            );
            assert_eq!(
                rule.anchor.original_group["hooks"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            let dependent = rule.definition.group["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains("PLUGIN_ROOT");
            assert_eq!((adapter.validate_adoption)(&rule).is_err(), dependent);
        }
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"{}\n").unwrap();
        assert!((adapter.read_hook)(&snapshot, hooks(&snapshot.inventory)[0]).is_err());
    }
}
