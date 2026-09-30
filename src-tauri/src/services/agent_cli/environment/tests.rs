#[cfg(unix)]
mod claude_linked_skills;
mod claude_plugin_children;
mod claude_proof;
mod claude_skill_frontmatter;
mod claude_statusline;
mod codex_native_matrix;
mod codex_proof;
#[cfg(unix)]
mod cross_agent_linked_skills;
mod discovery_seed;
mod gemini_builtin;
mod grok;
mod grok_plugin_children;
mod grok_proof;
mod hook_counts;
mod production_limits;
mod profile_isolation;

#[cfg(any(unix, windows))]
use super::snapshot::{
    directory_names_from_open_handle_after, directory_probe_from_open_handle_after,
};
use super::{
    diagnostics::{rebind_source_id, DiagnosticOwner},
    fixture_assessor, fixture_draft,
    identity::{context_stable_id, lexical_absolute, source_stable_id},
    inventory::{
        build_inventory_with, materialize_source_diagnostics, native_environment,
        InstallationDiscoveryPort, InstallationDiscoveryRequest, InventoryInput,
        InventoryPipelineDeps, RealInstallationDiscoveryPort,
    },
    output::{BoundedSourceOutput, ContextSourceState},
    parsed_asset, physical_origin, read_only_action_set,
    run::{
        AgentInventoryRun, AgentInventoryStage, InventoryCheckpointProbe, InventoryPipelineEvent,
        ManualClock, StageCheckpointProbe,
    },
    snapshot::{
        snapshot_revision, snapshot_source, RealSnapshotPort, SnapshotPort, SnapshotRequest,
    },
    versioning::{
        latest_stable_version, releases::failure_backoff, version_channel, version_state,
    },
    ParsedAssetInput,
};
use crate::{
    models::{
        AgentAssetActionKind, AgentAssetActionUnavailableReason, AgentAssetCategory,
        AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
        AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetEffectiveAvailability,
        AgentAssetInstallState, AgentAssetLimitKind, AgentAssetLimits,
        AgentAssetResolutionParticipation, AgentAssetResolutionRelation,
        AgentAssetResolutionTerminal, AgentAssetRevision, AgentAssetScope, AgentAssetSource,
        AgentAssetSourceKind, AgentAssetState, AgentCliKind, AgentDiscoverySource,
        AgentExecutableIdentity, AgentInstallation, AgentInstallationAvailability,
        AgentInstallationChannel, AgentLifecycleVersionState, AgentMcpApprovalState,
        AgentMcpTransport, AgentSkillInvocationPolicy, AgentStatusUiMode, AgentTrustState,
        AgentVersionSource,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetDirectoryEntry, AgentAssetNativePayload, AgentAssetParseRequest,
            AgentAssetResolveRequest, AgentAssetResolveSource, AgentAssetSnapshot,
            AgentAssetSourceSpec, AgentContextDiscoveryRequest, AgentDiagnosticOutput,
            AgentFollowUpSourceDiscoveryRequest, AgentFollowUpSourceSpec,
            AgentFollowUpSourceTarget, AgentMcpDefinitionOrigin, AgentMcpDefinitionPayload,
            AgentMcpMatcherIdentity, AgentMcpPolicyPayload, AgentParseOutput, AgentResolveOutput,
            AgentSourceDiscoveryRequest, AgentWorkspaceTrustResolveRequest,
            AgentWorkspaceTrustSourceRequest, CodexAssetPayload, EndpointAdapter,
            EnvironmentAdapter, FollowUpSourceOutput, InitialSourceOutput,
        },
        definition, AgentCliDefinition,
    },
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

fn test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-agent-env-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn directory_source(path: PathBuf, allowed_root: PathBuf) -> AgentAssetSourceSpec {
    AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "skills".to_string(),
        label: "Skills".to_string(),
        scope: AgentAssetScope::User,
        path,
        allowed_root,
        precedence: 20,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 20,
            },
        ],
    }
}

fn inventory_run(limits: AgentAssetLimits) -> AgentInventoryRun {
    AgentInventoryRun::with_clock(limits, Arc::new(ManualClock::new()))
}

#[derive(Default)]
struct PublicDiagnosticTree {
    inventory: Vec<AgentAssetDiagnostic>,
    installations: Vec<AgentAssetDiagnostic>,
    sources: Vec<AgentAssetDiagnostic>,
    declarations: Vec<AgentAssetDiagnostic>,
    records: Vec<AgentAssetDiagnostic>,
    resolutions: Vec<AgentAssetDiagnostic>,
}

impl PublicDiagnosticTree {
    fn flatten(&self) -> impl Iterator<Item = &AgentAssetDiagnostic> {
        self.inventory
            .iter()
            .chain(self.installations.iter())
            .chain(self.sources.iter())
            .chain(self.declarations.iter())
            .chain(self.records.iter())
            .chain(self.resolutions.iter())
    }
}

#[cfg(test)]
#[derive(Default)]
struct NativePayloadDebugCollector {
    payload_debug: Vec<String>,
    facts: Vec<String>,
    diagnostics: Vec<AgentAssetDiagnostic>,
    declarations: Vec<crate::services::agent_cli::contracts::ParsedAgentAsset>,
}

thread_local! {
    static CLAUDE_NATIVE_PAYLOAD_TEE: RefCell<Option<NativePayloadDebugCollector>> =
        const { RefCell::new(None) };
}

struct TeeParseOutput<'a> {
    capture: &'a mut NativePayloadDebugCollector,
    output: &'a mut dyn AgentParseOutput,
}

impl AgentDiagnosticOutput for TeeParseOutput<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.capture.emit_diagnostic(value.clone());
        self.output.emit_diagnostic(value)
    }
}

impl AgentParseOutput for TeeParseOutput<'_> {
    fn emit_declaration(
        &mut self,
        value: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        let _ = self.capture.emit_declaration(value.clone());
        self.output.emit_declaration(value)
    }
}

impl AgentDiagnosticOutput for NativePayloadDebugCollector {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentParseOutput for NativePayloadDebugCollector {
    fn emit_declaration(
        &mut self,
        value: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.declarations.push(value.clone());
        self.payload_debug
            .push(format!("{:?}", value.native_payload));
        self.facts.extend(value.facts.into_values());
        std::ops::ControlFlow::Continue(())
    }
}

#[derive(Default)]
struct ReorderingInitialSourceOutput<'a> {
    sources: Vec<AgentAssetSourceSpec>,
    diagnostics: Vec<AgentAssetDiagnostic>,
    // Pure projection fixtures collect sources without doing any IO; inventory
    // reversal fixtures supply the real bounded reader for dependent seeds.
    output: Option<&'a mut dyn InitialSourceOutput>,
}

impl AgentDiagnosticOutput for ReorderingInitialSourceOutput<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output
            .as_ref()
            .is_none_or(|output| output.has_regular_capacity())
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl InitialSourceOutput for ReorderingInitialSourceOutput<'_> {
    fn emit_initial(
        &mut self,
        value: AgentAssetSourceSpec,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.sources.push(value);
        std::ops::ControlFlow::Continue(())
    }

    fn snapshot_initial(
        &mut self,
        source: AgentAssetSourceSpec,
    ) -> std::ops::ControlFlow<
        crate::services::agent_cli::contracts::AgentOutputStop,
        Option<AgentAssetSnapshot>,
    > {
        // Discovery needs these seed bytes synchronously. Forward their single
        // admission/read; reverse only emissions with no snapshot dependency.
        if let Some(output) = self.output.as_deref_mut() {
            return output.snapshot_initial(source);
        }
        self.emit_initial(source)?;
        std::ops::ControlFlow::Continue(None)
    }
}

#[derive(Default)]
struct ReorderingResolveOutput {
    drafts: Vec<crate::services::agent_cli::contracts::AgentAssetProjectedDraft>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

impl AgentDiagnosticOutput for ReorderingResolveOutput {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentResolveOutput for ReorderingResolveOutput {
    fn emit_draft(
        &mut self,
        value: crate::services::agent_cli::contracts::AgentAssetProjectedDraft,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.drafts.push(value);
        std::ops::ControlFlow::Continue(())
    }
}

fn claude_test_discover_sources_reversed(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let mut captured = ReorderingInitialSourceOutput {
        sources: Vec::new(),
        diagnostics: Vec::new(),
        output: Some(output),
    };
    claude_test_discover_sources(request, &mut captured);
    let ReorderingInitialSourceOutput {
        sources,
        diagnostics,
        ..
    } = captured;
    for diagnostic in diagnostics {
        if output.emit_diagnostic(diagnostic)
            == crate::services::agent_cli::contracts::AgentDiagnosticEmission::Saturated
        {
            return;
        }
    }
    for source in sources.into_iter().rev() {
        if output.emit_initial(source).is_break() {
            return;
        }
    }
}

fn claude_test_parse_reversed(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let mut captured = NativePayloadDebugCollector::default();
    claude_test_parse(request, &mut captured);
    for diagnostic in captured.diagnostics {
        if output.emit_diagnostic(diagnostic)
            == crate::services::agent_cli::contracts::AgentDiagnosticEmission::Saturated
        {
            return;
        }
    }
    for declaration in captured.declarations.into_iter().rev() {
        if output.emit_declaration(declaration).is_break() {
            return;
        }
    }
}

fn claude_test_resolve_reversed(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut captured = ReorderingResolveOutput::default();
    claude_test_resolve(request, &mut captured);
    for diagnostic in captured.diagnostics {
        if output.emit_diagnostic(diagnostic)
            == crate::services::agent_cli::contracts::AgentDiagnosticEmission::Saturated
        {
            return;
        }
    }
    for draft in captured.drafts.into_iter().rev() {
        if output.emit_draft(draft).is_break() {
            return;
        }
    }
}

fn materialize_test_diagnostic_tree(run: &mut AgentInventoryRun) -> PublicDiagnosticTree {
    PublicDiagnosticTree {
        installations: run.take_diagnostics_for(DiagnosticOwner::Installation(
            "installation:test".to_owned(),
        )),
        sources: run.take_diagnostics_for(DiagnosticOwner::Source("source:test".to_owned())),
        declarations: run
            .take_diagnostics_for(DiagnosticOwner::Declaration("declaration:test".to_owned())),
        records: run.take_diagnostics_for(DiagnosticOwner::Record("record:test".to_owned())),
        resolutions: run
            .take_diagnostics_for(DiagnosticOwner::Resolution("resolution:test".to_owned())),
        inventory: run.finish_diagnostics(),
    }
}

fn diagnostic(key: &str) -> AgentAssetDiagnostic {
    AgentAssetDiagnostic::InvalidProjection {
        projection_key: key.to_owned(),
    }
}

fn snapshot_directory_at(
    path: &Path,
    allowed_root: &Path,
    limits: AgentAssetLimits,
) -> AgentAssetSnapshot {
    let source = directory_source(path.to_path_buf(), allowed_root.to_path_buf());
    let mut budget = inventory_run(limits);
    snapshot_source(&source, "source:test", &[allowed_root], &mut budget)
}

#[test]
fn template_rejects_parent_traversal() {
    assert!(lexical_absolute(Path::new("/tmp/home/../outside")).is_none());
    assert!(lexical_absolute(Path::new("relative/path")).is_none());
}

#[test]
fn mcp_policy_native_payload_debug_is_typed_without_secret_values() {
    let policy_members = |first: &str, second: &str| {
        [first.to_owned(), second.to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>()
    };
    let cases = [
        (
            AgentMcpPolicyPayload::Allowed(policy_members(
                "allowed-member-secret",
                "allowed-second-secret",
            )),
            "Allowed",
            "member_count: 2",
        ),
        (
            AgentMcpPolicyPayload::Excluded(policy_members(
                "excluded-member-secret",
                "excluded-second-secret",
            )),
            "Excluded",
            "member_count: 2",
        ),
        (
            AgentMcpPolicyPayload::InvalidAllowed,
            "InvalidAllowed",
            "policy_kind",
        ),
        (
            AgentMcpPolicyPayload::InvalidExcluded,
            "InvalidExcluded",
            "policy_kind",
        ),
    ];

    for (policy, kind, metadata) in cases {
        let policy_debug = format!("{policy:?}");
        let payload_debug = format!("{:?}", AgentAssetNativePayload::McpPolicy(policy));
        for debug in [policy_debug, payload_debug] {
            assert!(debug.contains(kind), "policy kind missing from {debug}");
            assert!(
                debug.contains(metadata),
                "policy metadata missing from {debug}"
            );
            assert!(debug.contains("McpPolicy") || debug.contains("AgentMcpPolicyPayload"));
            for sentinel in [
                "allowed-member-secret",
                "allowed-second-secret",
                "excluded-member-secret",
                "excluded-second-secret",
            ] {
                assert!(
                    !debug.contains(sentinel),
                    "Debug leaked {sentinel}: {debug}"
                );
            }
        }
    }
}

#[test]
fn mcp_matcher_identity_debug_keeps_shape_without_argv_or_url_values() {
    let cases = [
        (
            AgentMcpMatcherIdentity::Stdio {
                argv: vec![
                    "matcher-command-secret".to_owned(),
                    "matcher-argument-secret".to_owned(),
                ],
            },
            "Stdio",
            "argument_count: 2",
        ),
        (
            AgentMcpMatcherIdentity::Remote {
                url: "https://matcher-url-secret.invalid/mcp".to_owned(),
            },
            "Remote",
            "argument_count: 0",
        ),
    ];

    for (identity, variant, metadata) in cases {
        let debug = format!("{identity:?}");
        assert!(
            debug.contains(variant),
            "matcher variant missing from {debug}"
        );
        assert!(
            debug.contains(metadata),
            "matcher metadata missing from {debug}"
        );
        for sentinel in [
            "matcher-command-secret",
            "matcher-argument-secret",
            "matcher-url-secret",
        ] {
            assert!(
                !debug.contains(sentinel),
                "Debug leaked {sentinel}: {debug}"
            );
        }
    }
}

#[test]
fn asset_limits_are_clamped_to_the_central_hard_caps() {
    let requested = AgentAssetLimits {
        candidate_paths_per_agent: usize::MAX,
        installations_per_agent: usize::MAX,
        sources_per_context: usize::MAX,
        first_level_entries: usize::MAX,
        bytes_per_source: usize::MAX,
        bytes_per_refresh: usize::MAX,
        refresh_budget_ms: u64::MAX,
        cli_output_bytes: usize::MAX,
        cli_concurrency: usize::MAX,
        watchers: usize::MAX,
        diagnostics: usize::MAX,
    };
    assert_eq!(requested.bounded(), AgentAssetLimits::HARD_CAP);
}

#[test]
fn public_diagnostic_tree_exact_capacity_is_bounded_across_all_nested_owners() {
    let limits = AgentAssetLimits {
        diagnostics: 6,
        ..AgentAssetLimits::DEFAULT
    };
    let mut run = AgentInventoryRun::new(limits);
    for (owner, value) in [
        (
            DiagnosticOwner::Installation("installation:test".to_owned()),
            "installation",
        ),
        (DiagnosticOwner::Source("source:test".to_owned()), "source"),
        (
            DiagnosticOwner::Declaration("declaration:test".to_owned()),
            "declaration",
        ),
        (DiagnosticOwner::Record("record:test".to_owned()), "record"),
        (
            DiagnosticOwner::Resolution("resolution:test".to_owned()),
            "resolution",
        ),
    ] {
        assert!(matches!(
            run.emit(owner, diagnostic(value)),
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        ));
    }

    let tree = materialize_test_diagnostic_tree(&mut run);
    assert_eq!(tree.flatten().count(), 5);
    assert!(!tree
        .flatten()
        .any(|value| matches!(value, AgentAssetDiagnostic::Truncated { .. })));
}

#[test]
fn public_diagnostic_tree_one_over_has_one_inventory_terminal_marker() {
    let limits = AgentAssetLimits {
        diagnostics: 4,
        ..AgentAssetLimits::DEFAULT
    };
    let mut run = AgentInventoryRun::new(limits);
    for (owner, value) in [
        (
            DiagnosticOwner::Installation("installation:test".to_owned()),
            "installation",
        ),
        (DiagnosticOwner::Source("source:test".to_owned()), "source"),
        (
            DiagnosticOwner::Declaration("declaration:test".to_owned()),
            "declaration",
        ),
        (DiagnosticOwner::Record("record:test".to_owned()), "record"),
    ] {
        run.emit(owner, diagnostic(value));
    }
    for index in 0..100 {
        run.emit(
            DiagnosticOwner::Resolution("resolution:test".to_owned()),
            diagnostic(&format!("overflow-{index}")),
        );
    }

    let tree = materialize_test_diagnostic_tree(&mut run);
    assert_eq!(tree.flatten().count(), 4);
    assert_eq!(
        tree.inventory
            .iter()
            .filter(|value| matches!(value, AgentAssetDiagnostic::Truncated { .. }))
            .count(),
        1
    );
    assert!(matches!(
        tree.inventory.last(),
        Some(AgentAssetDiagnostic::Truncated { .. })
    ));
    assert!(!tree
        .installations
        .iter()
        .chain(tree.sources.iter())
        .chain(tree.declarations.iter())
        .chain(tree.records.iter())
        .chain(tree.resolutions.iter())
        .any(|value| matches!(value, AgentAssetDiagnostic::Truncated { .. })));
}

#[test]
fn public_diagnostic_tree_deduplicates_same_owner_but_keeps_distinct_owners() {
    let mut run = AgentInventoryRun::new(AgentAssetLimits {
        diagnostics: 4,
        ..AgentAssetLimits::DEFAULT
    });
    let value = diagnostic("same-value");
    assert!(matches!(
        run.emit(
            DiagnosticOwner::Source("source:test".to_owned()),
            value.clone(),
        ),
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    ));
    assert!(matches!(
        run.emit(
            DiagnosticOwner::Source("source:test".to_owned()),
            value.clone(),
        ),
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Duplicate
    ));
    assert!(matches!(
        run.emit(DiagnosticOwner::Record("record:test".to_owned()), value,),
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    ));

    let tree = materialize_test_diagnostic_tree(&mut run);
    assert_eq!(tree.flatten().count(), 2);
    assert_eq!(tree.sources, vec![diagnostic("same-value")]);
    assert_eq!(tree.records, vec![diagnostic("same-value")]);
}

#[test]
fn source_owner_survives_deadline_before_parse_materialization() {
    let source_id = "source:test";
    let mut sources = BTreeMap::from([(
        source_id.to_owned(),
        AgentAssetSource {
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            id: source_id.to_owned(),
            context_id: "context:test".to_owned(),
            label: "fixture".to_owned(),
            scope: AgentAssetScope::User,
            environment_id: "native:test".to_owned(),
            workspace_id: None,
            path: "/tmp/fixture.json".to_owned(),
            allowed_root: "/tmp".to_owned(),
            precedence: 1,
            writable: false,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Mcp],
            revision: AgentAssetRevision::default(),
            diagnostics: Vec::new(),
            access: crate::models::AgentAssetAccess::default(),
            actions: Vec::new(),
        },
    )]);
    let clock = Arc::new(ManualClock::new());
    let mut run = AgentInventoryRun::with_clock(AgentAssetLimits::DEFAULT, clock.clone());
    clock.advance(std::time::Duration::from_millis(
        AgentAssetLimits::DEFAULT.refresh_budget_ms,
    ));

    assert!(run
        .deadline_diagnostic(DiagnosticOwner::Source(source_id.to_owned()))
        .is_some());
    materialize_source_diagnostics(source_id, &mut sources, &mut run);
    assert!(matches!(
        sources[source_id].diagnostics.as_slice(),
        [AgentAssetDiagnostic::BudgetExceeded { .. }]
    ));
    assert!(run.finish_diagnostics().is_empty());
}

#[test]
fn directory_inventory_uses_the_central_first_level_limit() {
    let root = test_root("directory-limit");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    for name in ["zulu", "alpha", "bravo"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "skills".to_string(),
        label: "Skills".to_string(),
        scope: AgentAssetScope::User,
        path: root.clone(),
        allowed_root: root.clone(),
        precedence: 20,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 20,
            },
        ],
    };
    let limits = AgentAssetLimits {
        first_level_entries: 2,
        ..AgentAssetLimits::DEFAULT
    };
    let mut budget = inventory_run(limits);
    let snapshot = snapshot_source(&source, "source:test", &[root.as_path()], &mut budget);
    let AgentAssetSnapshot::DirectoryManifest { entries, .. } = snapshot else {
        panic!("expected a directory manifest");
    };
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "bravo"]
    );
    let diagnostics = budget.finish_diagnostics();
    assert!(diagnostics.iter().any(|item| matches!(
        item,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::FirstLevelEntries,
            accepted: 2,
            observed_at_least: 3,
        }
    )));

    let exact_limits = AgentAssetLimits {
        first_level_entries: 3,
        ..AgentAssetLimits::DEFAULT
    };
    let mut exact_budget = inventory_run(exact_limits);
    let exact = snapshot_source(&source, "source:test", &[root.as_path()], &mut exact_budget);
    let AgentAssetSnapshot::DirectoryManifest { entries, .. } = exact else {
        panic!("expected an exact-boundary directory manifest");
    };
    assert_eq!(entries.len(), 3);
    let diagnostics = exact_budget.finish_diagnostics();
    assert!(!diagnostics.iter().any(|item| matches!(
        item,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::FirstLevelEntries,
            ..
        }
    )));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn directory_inventory_first_n_is_independent_of_creation_order() {
    let fixture = test_root("directory-creation-order");
    let first = fixture.join("first");
    let second = fixture.join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let first = fixture.join("first");
    let second = fixture.join("second");
    for name in ["zulu", "alpha", "bravo"] {
        fs::create_dir(first.join(name)).unwrap();
    }
    for name in ["bravo", "zulu", "alpha"] {
        fs::create_dir(second.join(name)).unwrap();
    }

    let limits = AgentAssetLimits {
        first_level_entries: 2,
        ..AgentAssetLimits::DEFAULT
    };
    let names = |path: &Path| {
        let AgentAssetSnapshot::DirectoryManifest { entries, .. } =
            snapshot_directory_at(path, &fixture, limits.clone())
        else {
            panic!("expected directory manifest");
        };
        entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&first), vec!["alpha", "bravo"]);
    assert_eq!(names(&second), vec!["alpha", "bravo"]);
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn directory_revision_tracks_the_opened_object_identity() {
    let fixture = test_root("directory-object-revision");
    let source = fixture.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("asset.md"), b"same manifest").unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");
    let held = fixture.join("held");

    let revision = |path: &Path| {
        let AgentAssetSnapshot::DirectoryManifest { revision, .. } =
            snapshot_directory_at(path, &fixture, AgentAssetLimits::DEFAULT)
        else {
            panic!("expected directory manifest");
        };
        revision.identity
    };
    let initial = revision(&source);
    assert_eq!(revision(&source), initial);

    fs::rename(&source, &held).unwrap();
    fs::create_dir(&source).unwrap();
    fs::write(source.join("asset.md"), b"same manifest").unwrap();
    assert_ne!(revision(&source), initial);
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(unix)]
#[test]
fn directory_manifest_records_a_first_level_symlink_without_recursing() {
    let fixture = test_root("directory-entry-symlink");
    let source = fixture.join("source");
    let outside = fixture.join("outside");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(outside.join("nested")).unwrap();
    fs::write(outside.join("nested/secret.txt"), b"not read").unwrap();
    std::os::unix::fs::symlink(&outside, source.join("linked-assets")).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");

    let AgentAssetSnapshot::DirectoryManifest { entries, .. } =
        snapshot_directory_at(&source, &fixture, AgentAssetLimits::DEFAULT)
    else {
        panic!("expected directory manifest");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "linked-assets");
    assert!(entries[0].is_symlink);
    assert_eq!(entries[0].source_kind, AgentAssetSourceKind::File);
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(any(unix, windows))]
#[test]
fn directory_revalidation_detects_content_epoch_change() {
    let fixture = test_root("directory-epoch-change");
    let source = fixture.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("before.txt"), b"before").unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");

    // Set a distinct epoch so the test does not depend on filesystem clock resolution.
    #[cfg(unix)]
    fs::File::open(&source)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .access_mode(0x100)
            .custom_flags(0x02000000)
            .open(&source)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH)
            .unwrap();
    }
    let probe = directory_probe_from_open_handle_after(&source, || {
        fs::write(source.join("after.txt"), b"after").unwrap();
    })
    .unwrap();
    assert_eq!(probe.names, vec!["after.txt", "before.txt"]);
    assert!(!probe.revalidated);
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(unix)]
#[test]
fn directory_enumeration_stays_bound_to_the_opened_root_after_replacement() {
    let fixture = test_root("directory-root-replacement");
    fs::create_dir_all(&fixture).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");
    let held = fixture.join("held");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("original-entry")).unwrap();

    let probe = directory_probe_from_open_handle_after(&source, || {
        fs::rename(&source, &held).unwrap();
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("replacement-entry")).unwrap();
    })
    .unwrap();
    assert_eq!(probe.names, vec!["original-entry"]);
    assert!(!probe.revalidated);

    fs::remove_dir_all(&source).unwrap();
    fs::rename(&held, &source).unwrap();
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(unix)]
#[test]
fn directory_enumeration_stays_bound_to_the_opened_root_through_aba() {
    let fixture = test_root("directory-root-aba");
    fs::create_dir_all(&fixture).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");
    let held = fixture.join("held");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("original-entry")).unwrap();

    let names = directory_names_from_open_handle_after(&source, || {
        fs::rename(&source, &held).unwrap();
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("replacement-entry")).unwrap();
        fs::remove_dir_all(&source).unwrap();
        fs::rename(&held, &source).unwrap();
    })
    .unwrap();
    assert_eq!(names, vec!["original-entry"]);
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(unix)]
#[test]
fn directory_enumeration_stays_bound_after_parent_replacement() {
    let fixture = test_root("directory-parent-replacement");
    fs::create_dir_all(&fixture).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let parent = fixture.join("parent");
    let held_parent = fixture.join("held-parent");
    let source = parent.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir(source.join("original-entry")).unwrap();

    let probe = directory_probe_from_open_handle_after(&source, || {
        fs::rename(&parent, &held_parent).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(source.join("replacement-entry")).unwrap();
    })
    .unwrap();
    assert_eq!(probe.names, vec!["original-entry"]);
    assert!(!probe.revalidated);

    fs::remove_dir_all(&parent).unwrap();
    fs::rename(&held_parent, &parent).unwrap();
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(windows)]
#[test]
fn directory_handle_prevents_root_replacement_on_windows() {
    use std::cell::RefCell;

    let fixture = test_root("directory-root-share-delete");
    fs::create_dir_all(&fixture).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let source = fixture.join("source");
    let replacement = fixture.join("replacement");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("original-entry")).unwrap();
    let rename_result = RefCell::new(None);

    let names = directory_names_from_open_handle_after(&source, || {
        *rename_result.borrow_mut() = Some(fs::rename(&source, &replacement));
    })
    .unwrap();
    assert!(rename_result.into_inner().unwrap().is_err());
    assert_eq!(names, vec!["original-entry"]);
    assert!(source.is_dir());
    assert!(!replacement.exists());
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(windows)]
#[test]
fn directory_handle_chain_prevents_ancestor_replacement_on_windows() {
    use std::cell::RefCell;

    let fixture = test_root("directory-ancestor-share-delete");
    let parent = fixture.join("parent");
    let source = parent.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("original-entry"), b"entry").unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let parent = fixture.join("parent");
    let source = parent.join("source");
    let replacement = fixture.join("replacement");
    let rename_result = RefCell::new(None);

    let names = directory_names_from_open_handle_after(&source, || {
        *rename_result.borrow_mut() = Some(fs::rename(&parent, &replacement));
    })
    .unwrap();
    assert!(rename_result.into_inner().unwrap().is_err());
    assert_eq!(names, vec!["original-entry"]);
    assert!(parent.is_dir());
    assert!(!replacement.exists());
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(windows)]
#[test]
fn windows_directory_reader_continues_across_multiple_native_batches() {
    let fixture = test_root("directory-multiple-windows-batches");
    let source = fixture.join("source");
    fs::create_dir_all(&source).unwrap();
    let expected = (0..600)
        .map(|index| format!("entry-{index:04}-{}", "x".repeat(180)))
        .collect::<Vec<_>>();
    for name in &expected {
        fs::write(source.join(name), b"entry").unwrap();
    }
    let source = fs::canonicalize(source).unwrap();

    let names = directory_names_from_open_handle_after(&source, || {}).unwrap();
    assert_eq!(names, expected);
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(windows)]
#[test]
fn windows_directory_snapshot_reports_final_and_ancestor_reparse_as_symlink_rejected() {
    use std::os::windows::fs::symlink_dir;

    let fixture = test_root("directory-reparse-diagnostic");
    let actual = fixture.join("actual");
    let actual_source = actual.join("source");
    fs::create_dir_all(&actual_source).unwrap();
    fs::write(actual_source.join("asset.md"), b"asset").unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let actual = fixture.join("actual");
    let actual_source = actual.join("source");
    let final_link = fixture.join("final-link");
    let ancestor_link = fixture.join("ancestor-link");
    symlink_dir(&actual_source, &final_link)
        .expect("Windows native test runner must permit directory symlink fixtures");
    symlink_dir(&actual, &ancestor_link)
        .expect("Windows native test runner must permit directory symlink fixtures");

    for path in [final_link.clone(), ancestor_link.join("source")] {
        let source = directory_source(path, fixture.clone());
        let mut budget = inventory_run(AgentAssetLimits::DEFAULT);
        assert!(matches!(
            snapshot_source(&source, "source:reparse", &[fixture.as_path()], &mut budget),
            AgentAssetSnapshot::Blocked {
                diagnostic: AgentAssetDiagnostic::SymlinkRejected { .. },
                ..
            }
        ));
    }

    fs::remove_dir(&final_link).unwrap();
    fs::remove_dir(&ancestor_link).unwrap();
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn snapshot_revision_distinguishes_missing_and_changed_files() {
    let root = test_root("snapshot-revision");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let path = root.join("settings.json");
    let missing = snapshot_revision(&path).expect("missing identity");
    fs::write(&path, b"first").unwrap();
    let first = snapshot_revision(&path).expect("first revision");
    fs::write(&path, b"second").unwrap();
    let second = snapshot_revision(&path).expect("second revision");
    assert_ne!(missing, first);
    assert_ne!(first, second);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn snapshot_rejects_parent_escape_without_target_metadata() {
    let root = test_root("parent-escape");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "escape".to_string(),
        label: "Escape".to_string(),
        scope: AgentAssetScope::User,
        path: root.join("../outside/secret.json"),
        allowed_root: root.clone(),
        precedence: 10,
        writable: false,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };
    let limits = AgentAssetLimits::DEFAULT;
    let mut budget = inventory_run(limits);
    let snapshot = snapshot_source(&source, "source:escape", &[root.as_path()], &mut budget);
    let AgentAssetSnapshot::Blocked {
        revision,
        diagnostic: AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. },
    } = snapshot
    else {
        panic!("parent escape must be blocked");
    };
    assert_eq!(revision.size_bytes, None);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_symlinked_root_and_nested_symlink() {
    let fixture = test_root("symlink-boundary");
    fs::create_dir_all(&fixture).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();
    let actual = fixture.join("actual");
    fs::create_dir_all(&actual).unwrap();
    fs::write(actual.join("settings.json"), b"{}").unwrap();
    let linked = fixture.join("linked");
    std::os::unix::fs::symlink(&actual, &linked).unwrap();

    let source = |allowed_root: PathBuf, path: PathBuf| AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "settings".to_string(),
        label: "Settings".to_string(),
        scope: AgentAssetScope::User,
        path,
        allowed_root,
        precedence: 10,
        writable: false,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };
    let limits = AgentAssetLimits::DEFAULT;
    for spec in [
        source(linked.clone(), linked.join("settings.json")),
        source(fixture.clone(), linked.join("settings.json")),
    ] {
        let mut budget = inventory_run(limits.clone());
        assert!(matches!(
            snapshot_source(&spec, "source:symlink", &[fixture.as_path()], &mut budget),
            AgentAssetSnapshot::Blocked {
                diagnostic: AgentAssetDiagnostic::SymlinkRejected { .. },
                ..
            }
        ));
    }
    let _ = fs::remove_dir_all(fixture);
}

#[cfg(unix)]
#[test]
fn directory_snapshot_rejects_symlinked_source_and_ancestor() {
    let fixture = test_root("directory-symlink-boundary");
    let actual = fixture.join("actual");
    let actual_source = actual.join("skills");
    fs::create_dir_all(&actual_source).unwrap();
    fs::write(actual_source.join("skill.md"), b"skill").unwrap();
    let linked = fixture.join("linked");
    std::os::unix::fs::symlink(&actual, &linked).unwrap();
    let fixture = fs::canonicalize(fixture).unwrap();

    for path in [fixture.join("linked"), fixture.join("linked/skills")] {
        let source = directory_source(path, fixture.clone());
        let mut budget = inventory_run(AgentAssetLimits::DEFAULT);
        assert!(matches!(
            snapshot_source(&source, "source:symlink", &[fixture.as_path()], &mut budget),
            AgentAssetSnapshot::Blocked {
                diagnostic: AgentAssetDiagnostic::SymlinkRejected { .. },
                ..
            }
        ));
    }
    let _ = fs::remove_dir_all(fixture);
}

#[test]
fn snapshot_enforces_source_and_refresh_byte_limits() {
    let root = test_root("byte-limits");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let first = root.join("first.json");
    let second = root.join("second.json");
    fs::write(&first, b"1234").unwrap();
    fs::write(&second, b"5678").unwrap();
    let source = |key: &str, path: PathBuf| AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: key.to_string(),
        label: key.to_string(),
        scope: AgentAssetScope::User,
        path,
        allowed_root: root.clone(),
        precedence: 10,
        writable: false,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };

    let mut source_budget = inventory_run(AgentAssetLimits {
        bytes_per_source: 3,
        bytes_per_refresh: 8,
        ..AgentAssetLimits::DEFAULT
    });
    assert!(matches!(
        snapshot_source(
            &source("first", first.clone()),
            "source:first",
            &[root.as_path()],
            &mut source_budget,
        ),
        AgentAssetSnapshot::Blocked {
            diagnostic: AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::BytesPerSource,
                ..
            },
            ..
        }
    ));

    let mut refresh_budget = inventory_run(AgentAssetLimits {
        bytes_per_source: 4,
        bytes_per_refresh: 7,
        ..AgentAssetLimits::DEFAULT
    });
    assert!(matches!(
        snapshot_source(
            &source("first", first),
            "source:first",
            &[root.as_path()],
            &mut refresh_budget,
        ),
        AgentAssetSnapshot::File { .. }
    ));
    assert!(matches!(
        snapshot_source(
            &source("second", second),
            "source:second",
            &[root.as_path()],
            &mut refresh_budget,
        ),
        AgentAssetSnapshot::Blocked {
            diagnostic: AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::BytesPerRefresh,
                ..
            },
            ..
        }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn foundation_keeps_all_mutation_actions_unavailable() {
    let actions = read_only_action_set();
    for action in actions.iter().filter(|action| {
        matches!(
            action.action,
            AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
        )
    }) {
        assert!(!action.available);
        assert_eq!(
            action.reason,
            Some(AgentAssetActionUnavailableReason::MutationDisabled)
        );
        assert!(action.mechanism_id.is_none());
    }
}

#[test]
fn version_response_accepts_only_stable_latest() {
    let stable = serde_json::json!({"version": "1.2.3"});
    assert_eq!(latest_stable_version(&stable).unwrap(), "1.2.3");
    let prerelease = serde_json::json!({"version": "1.2.3-beta.1"});
    assert!(latest_stable_version(&prerelease).is_err());
    assert!(latest_stable_version(&serde_json::json!({})).is_err());
}

#[test]
fn version_comparison_keeps_prerelease_ahead_state() {
    assert_eq!(
        version_state(Some("1.2.3"), Some("1.2.3")),
        AgentLifecycleVersionState::UpToDate
    );
    assert_eq!(
        version_state(Some("1.1.9"), Some("1.2.3")),
        AgentLifecycleVersionState::UpdateAvailable
    );
    assert_eq!(
        version_state(Some("1.3.0-alpha.1"), Some("1.2.3")),
        AgentLifecycleVersionState::AheadOfLatest
    );
    assert_eq!(
        version_state(Some("agent 1.2.3-beta.1"), Some("1.2.3")),
        AgentLifecycleVersionState::UpdateAvailable
    );
    assert_eq!(
        version_state(Some("not-a-version"), Some("1.2.3")),
        AgentLifecycleVersionState::Unknown
    );
    assert_eq!(
        version_channel("gemini 1.2.3-nightly.4"),
        AgentInstallationChannel::Nightly
    );
}

#[test]
fn version_failure_backoff_is_bounded() {
    assert_eq!(failure_backoff(1), std::time::Duration::from_secs(30));
    assert_eq!(failure_backoff(2), std::time::Duration::from_secs(60));
    assert_eq!(failure_backoff(20), std::time::Duration::from_secs(30 * 60));
}

#[test]
fn installation_availability_and_platform_capabilities_are_typed() {
    assert_eq!(
        serde_json::to_value(AgentInstallationAvailability::Available).unwrap(),
        "available"
    );
    let environment = native_environment();
    let serialized = serde_json::to_value(environment).unwrap();
    assert!(matches!(
        serialized["hostPlatform"].as_str(),
        Some("macos" | "linux" | "windows")
    ));
    assert_eq!(
        serialized["capabilities"],
        serde_json::json!(["readOnlyInventory", "boundedPreview"])
    );
    let installation = AgentInstallation {
        id: "installation:test".to_string(),
        environment_id: "native:test".to_string(),
        agent_kind: AgentCliKind::Codex,
        label: definition(AgentCliKind::Codex).label.to_string(),
        availability: AgentInstallationAvailability::Unavailable,
        executable_path: None,
        executable_identity: None,
        executable_revision: None,
        installed_version: None,
        discovery_source: AgentDiscoverySource::Automatic,
        distribution: crate::models::AgentCliDistribution::Unknown,
        channel: AgentInstallationChannel::Unknown,
        installed_version_source: AgentVersionSource::Unknown,
        diagnostics: vec![AgentAssetDiagnostic::InstallationProbeFailed {
            candidate_source: AgentDiscoverySource::Automatic,
            error_kind: crate::models::AgentExecutableProbeErrorKind::NotFound,
        }],
    };
    let serialized = serde_json::to_value(installation).unwrap();
    assert_eq!(serialized["label"], "Codex CLI");
    assert_eq!(serialized["availability"], "unavailable");
}

#[test]
fn public_asset_contract_round_trips_typed_relationship_and_policy_fields() {
    let reference = crate::models::AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "provider".to_owned(),
        qualifier: Some("plugin:provider".to_owned()),
    };
    let declaration = crate::models::AgentAssetDeclaration {
        id: "declaration:test".to_owned(),
        context_id: "context:test".to_owned(),
        source_id: "source:test".to_owned(),
        scope: AgentAssetScope::User,
        native_kind: AgentAssetCategory::Skill,
        native_id: "child".to_owned(),
        declaration_key: "child".to_owned(),
        label: "Child".to_owned(),
        precedence: 1,
        presence: crate::models::AgentAssetPresence::Present,
        declared_state: AgentAssetDeclaredState::Enabled,
        trust_state: AgentTrustState::Trusted,
        role: crate::models::AgentAssetDeclarationRole::Definition,
        participation: crate::models::AgentAssetResolutionParticipation::Participates,
        evidence: crate::models::AgentAssetEvidence::default(),
        diagnostics: Vec::new(),
        provided_by: Some(reference.clone()),
        action_owner: Some(reference.clone()),
        explicitly_affected: vec![reference.clone()],
    };
    let encoded = serde_json::to_value(&declaration).unwrap();
    assert_eq!(encoded["role"], "definition");
    assert_eq!(encoded["participation"]["kind"], "participates");
    assert_eq!(encoded["providedBy"]["nativeId"], "provider");
    let decoded: crate::models::AgentAssetDeclaration = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.provided_by, Some(reference.clone()));
    assert_eq!(decoded.action_owner, Some(reference.clone()));
    assert_eq!(decoded.explicitly_affected, vec![reference]);

    let resolution = crate::models::AgentAssetResolution {
        relation: crate::models::AgentAssetResolutionRelation::Unknown,
        qualified_collision: false,
        terminal: Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked),
        contributor_ids: vec!["declaration:test".to_owned()],
        winner_id: None,
        control_source: Some(crate::models::AgentAssetPolicyReference::Declaration {
            declaration_id: "policy:test".to_owned(),
        }),
        diagnostics: vec![AgentAssetDiagnostic::PolicyBlocked],
    };
    let decoded: crate::models::AgentAssetResolution =
        serde_json::from_value(serde_json::to_value(&resolution).unwrap()).unwrap();
    assert!(matches!(
        decoded.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { .. })
    ));
}

#[test]
fn agent_adapters_declare_documented_user_and_workspace_sources() {
    let home = Path::new(if cfg!(windows) {
        "C:/home/tester"
    } else {
        "/home/tester"
    });
    let workspace = Path::new(if cfg!(windows) {
        "C:/work/project"
    } else {
        "/work/project"
    });
    let expected = [
        (
            AgentCliKind::Codex,
            vec![
                home.join(".codex/config.toml"),
                home.join(".codex/hooks.json"),
                workspace.join(".codex/config.toml"),
                workspace.join(".codex/hooks.json"),
            ],
        ),
        (
            AgentCliKind::ClaudeCode,
            vec![
                home.join(".claude/settings.json"),
                workspace.join(".claude/settings.json"),
                workspace.join(".claude/settings.local.json"),
                workspace.join(".mcp.json"),
            ],
        ),
        (
            AgentCliKind::Gemini,
            vec![
                home.join(".gemini/settings.json"),
                workspace.join(".gemini/settings.json"),
                workspace.join(".gemini/skills"),
            ],
        ),
        (
            AgentCliKind::Grok,
            vec![
                home.join(".grok/config.toml"),
                workspace.join(".grok/config.toml"),
                workspace.join(".grok/skills"),
            ],
        ),
    ];

    for (kind, expected_paths) in expected {
        let adapter = definition(kind).environment();
        let mut run = AgentInventoryRun::new(AgentAssetLimits::defaults());
        let contexts = adapter.discover_contexts(
            AgentContextDiscoveryRequest {
                environment_id: "native:test",
                agent_kind: kind,
                home,
                workspace: Some(workspace),
                installations: &[],
                workspace_trust: AgentTrustState::Unknown,
            },
            &mut run,
        );
        assert_eq!(contexts.len(), 1);
        let mut state = ContextSourceState::new(AgentAssetLimits::DEFAULT.sources_per_context);
        let mut output = BoundedSourceOutput::initial(&mut state, &mut run, &contexts[0].id);
        adapter.discover_sources(
            AgentSourceDiscoveryRequest {
                installations: &[],
                context: &contexts[0],
                home,
                workspace: Some(workspace),
            },
            &mut output,
        );
        drop(output);
        let declarations = state.finish(&mut run, &contexts[0].id);
        for path in expected_paths {
            assert!(
                declarations.iter().any(|item| item.path == path),
                "{} does not declare {}",
                kind.key(),
                path.display()
            );
        }
    }

    let source_paths = |kind| {
        let adapter = definition(kind).environment();
        let mut run = AgentInventoryRun::new(AgentAssetLimits::defaults());
        let contexts = adapter.discover_contexts(
            AgentContextDiscoveryRequest {
                environment_id: "native:test",
                agent_kind: kind,
                home,
                workspace: Some(workspace),
                installations: &[],
                workspace_trust: AgentTrustState::Unknown,
            },
            &mut run,
        );
        let mut state = ContextSourceState::new(AgentAssetLimits::DEFAULT.sources_per_context);
        let mut output = BoundedSourceOutput::initial(&mut state, &mut run, &contexts[0].id);
        adapter.discover_sources(
            AgentSourceDiscoveryRequest {
                installations: &[],
                context: &contexts[0],
                home,
                workspace: Some(workspace),
            },
            &mut output,
        );
        drop(output);
        state.finish(&mut run, &contexts[0].id)
    };
    let codex = source_paths(AgentCliKind::Codex);
    assert!(!codex
        .iter()
        .any(|item| item.path == home.join(".codex/hooks")
            && item.source_kind == AgentAssetSourceKind::Directory));

    let claude = source_paths(AgentCliKind::ClaudeCode);
    assert!(!claude
        .iter()
        .any(|item| item.path == home.join(".claude/settings.local.json")));
}

use std::sync::Mutex;

#[derive(Default)]
struct FakeInstallationState {
    calls: Vec<String>,
    version_probes: usize,
}

struct FakeInstallationPort {
    state: Arc<Mutex<FakeInstallationState>>,
    response: Vec<AgentInstallation>,
}

impl FakeInstallationPort {
    fn new(response: Vec<AgentInstallation>) -> (Self, Arc<Mutex<FakeInstallationState>>) {
        let state = Arc::new(Mutex::new(FakeInstallationState::default()));
        (
            Self {
                state: Arc::clone(&state),
                response,
            },
            state,
        )
    }
}

fn fixture_installation() -> AgentInstallation {
    AgentInstallation {
        id: "installation:fixture".to_owned(),
        environment_id: "native:test".to_owned(),
        agent_kind: AgentCliKind::Codex,
        label: "Fixture Agent".to_owned(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some("/fixture/bin/agent".to_owned()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: "fixture".to_owned(),
            canonical_path: "/fixture/bin/agent".to_owned(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-revision".to_owned()),
        installed_version: Some("1.0.0".to_owned()),
        discovery_source: AgentDiscoverySource::Configured,
        distribution: crate::models::AgentCliDistribution::Unknown,
        channel: AgentInstallationChannel::Stable,
        installed_version_source: AgentVersionSource::LocalExecutable,
        diagnostics: Vec::new(),
    }
}

fn claude_fixture_installation(
    version: &str,
    channel: AgentInstallationChannel,
) -> AgentInstallation {
    let mut installation = fixture_installation();
    installation.id = "installation:claude-fixture".to_owned();
    installation.agent_kind = AgentCliKind::ClaudeCode;
    installation.label = "Claude Code Fixture".to_owned();
    installation.installed_version = Some(version.to_owned());
    installation.channel = channel;
    installation
}

impl InstallationDiscoveryPort for FakeInstallationPort {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        _run: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        let mut state = self.state.lock().expect("fake installation state lock");
        state.calls.push(request.definition.kind.key().to_string());
        // One discovery invocation represents one scripted version-probe
        // attempt. The fake must measure work performed, never infer it from
        // the number of records returned by the response.
        state.version_probes = state.version_probes.saturating_add(1);
        self.response.clone()
    }
}

#[derive(Default)]
struct FakeSnapshotState {
    attempts: Vec<String>,
    native_source_keys: Vec<String>,
    snapshot_paths: Vec<(String, PathBuf)>,
    opens: Vec<String>,
    charges: Vec<(String, usize)>,
    snapshots: Vec<(String, String, Vec<u8>)>,
    strict_consumed_source_keys: BTreeSet<String>,
    strict_remaining_files: BTreeSet<String>,
    strict_remaining_missing_files: BTreeSet<String>,
    strict_remaining_directories: BTreeSet<String>,
    strict_remaining_blocked: BTreeSet<String>,
    strict_unexpected_source_keys: BTreeSet<String>,
    strict_repeated_source_keys: BTreeSet<String>,
    strict_violations: Vec<String>,
}

#[derive(Debug)]
enum ExpectedSnapshotResult {
    File,
    Missing,
    Blocked(AgentAssetDiagnostic),
    DirectoryManifest {
        entries: Vec<AgentAssetDirectoryEntry>,
        complete: bool,
    },
}

#[derive(Debug)]
struct ExpectedSnapshot {
    native_source_key: String,
    bytes: usize,
    payload: Option<Vec<u8>>,
    source_kind: AgentAssetSourceKind,
    result: ExpectedSnapshotResult,
}

impl ExpectedSnapshot {
    fn file(native_source_key: &str, bytes: usize) -> Self {
        Self {
            native_source_key: native_source_key.to_owned(),
            bytes,
            payload: None,
            source_kind: AgentAssetSourceKind::File,
            result: ExpectedSnapshotResult::File,
        }
    }

    fn file_bytes(native_source_key: &str, bytes: &[u8]) -> Self {
        let mut expected = Self::file(native_source_key, bytes.len());
        expected.payload = Some(bytes.to_vec());
        expected
    }

    fn missing(native_source_key: &str, source_kind: AgentAssetSourceKind) -> Self {
        Self {
            native_source_key: native_source_key.to_owned(),
            bytes: 0,
            payload: None,
            source_kind,
            result: ExpectedSnapshotResult::Missing,
        }
    }

    fn blocked(
        native_source_key: &str,
        source_kind: AgentAssetSourceKind,
        diagnostic: AgentAssetDiagnostic,
    ) -> Self {
        Self {
            native_source_key: native_source_key.to_owned(),
            bytes: 0,
            payload: None,
            source_kind,
            result: ExpectedSnapshotResult::Blocked(diagnostic),
        }
    }

    fn directory(
        native_source_key: &str,
        bytes: usize,
        entries: Vec<AgentAssetDirectoryEntry>,
        complete: bool,
    ) -> Self {
        Self {
            native_source_key: native_source_key.to_owned(),
            bytes,
            payload: None,
            source_kind: AgentAssetSourceKind::Directory,
            result: ExpectedSnapshotResult::DirectoryManifest { entries, complete },
        }
    }

    fn parent_directory() -> Self {
        Self::directory(
            "fixture-parent",
            10,
            vec![AgentAssetDirectoryEntry {
                name: "childdir".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            }],
            true,
        )
    }
}

struct FakeSnapshotPort {
    state: Arc<Mutex<FakeSnapshotState>>,
    scripted: Mutex<VecDeque<ExpectedSnapshot>>,
}

impl FakeSnapshotPort {
    fn scripted(
        expected: impl IntoIterator<Item = ExpectedSnapshot>,
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        let state = Arc::new(Mutex::new(FakeSnapshotState::default()));
        (
            Self {
                state: Arc::clone(&state),
                scripted: Mutex::new(expected.into_iter().collect()),
            },
            state,
        )
    }

    fn assert_scripted_exhausted(&self) {
        assert!(
            self.scripted
                .lock()
                .expect("scripted snapshot queue lock")
                .is_empty(),
            "snapshot test script contains an unconsumed expectation"
        );
    }

    fn assert_next_scripted(&self, native_source_key: &str) {
        let scripted = self.scripted.lock().expect("scripted snapshot queue lock");
        assert_eq!(
            scripted
                .front()
                .map(|expected| expected.native_source_key.as_str()),
            Some(native_source_key),
            "unexpected scripted snapshot queue state"
        );
    }
}

impl SnapshotPort for FakeSnapshotPort {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let source_id = request.source_id.to_owned();
        {
            let mut state = self.state.lock().expect("fake snapshot state lock");
            state.attempts.push(source_id.clone());
            state
                .native_source_keys
                .push(request.source.native_source_key.clone());
        }

        let (expected_bytes, payload, expected_kind, expected_result) = {
            let mut scripted = self.scripted.lock().expect("scripted snapshot queue lock");
            let expected = scripted
                .front()
                .expect("snapshot called after the scripted queue was exhausted");
            assert_eq!(
                request.source.native_source_key, expected.native_source_key,
                "snapshot source order differs from the scripted queue"
            );
            let expected = scripted
                .pop_front()
                .expect("scripted snapshot queue changed while locked");
            (
                expected.bytes,
                expected.payload,
                expected.source_kind,
                expected.result,
            )
        };
        assert_eq!(
            request.source.source_kind, expected_kind,
            "snapshot source kind differs from the scripted queue"
        );
        let revision = synthetic_revision(
            &source_id,
            expected_bytes,
            expected_kind == AgentAssetSourceKind::Directory,
        );
        if matches!(expected_result, ExpectedSnapshotResult::Missing) {
            return AgentAssetSnapshot::Missing { revision };
        }
        if let ExpectedSnapshotResult::Blocked(diagnostic) = &expected_result {
            return AgentAssetSnapshot::Blocked {
                revision,
                diagnostic: diagnostic.clone(),
            };
        }

        // This is an ordered synthetic port. It models the physical open and
        // the resulting byte charge explicitly, while delegating both gates to
        // the same inventory run as production snapshots. No filesystem state
        // or second byte ledger is involved in this fake.
        if let Err(diagnostic) = run.can_start_read(expected_bytes) {
            return AgentAssetSnapshot::Blocked {
                revision: synthetic_revision(
                    &source_id,
                    expected_bytes,
                    expected_kind == AgentAssetSourceKind::Directory,
                ),
                diagnostic,
            };
        }
        self.state
            .lock()
            .expect("fake snapshot state lock")
            .opens
            .push(source_id.clone());
        run.commit_read(expected_bytes);
        self.state
            .lock()
            .expect("fake snapshot state lock")
            .charges
            .push((source_id.clone(), expected_bytes));

        match expected_result {
            ExpectedSnapshotResult::File => AgentAssetSnapshot::File {
                bytes: payload.unwrap_or_else(|| vec![b'x'; expected_bytes]),
                revision,
            },
            ExpectedSnapshotResult::DirectoryManifest { entries, complete } => {
                AgentAssetSnapshot::DirectoryManifest {
                    entries,
                    revision,
                    complete,
                }
            }
            ExpectedSnapshotResult::Missing | ExpectedSnapshotResult::Blocked(_) => {
                unreachable!("terminal snapshot results returned above")
            }
        }
    }
}

#[test]
fn fake_snapshot_supports_exact_missing_and_blocked_results() {
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-exact".to_owned(),
        label: "Fixture exact".to_owned(),
        scope: AgentAssetScope::User,
        path: PathBuf::from("/tmp/fixture-exact.toml"),
        allowed_root: PathBuf::from("/tmp"),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    };
    let missing = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        native_source_key: "fixture-missing".to_owned(),
        ..source.clone()
    };
    let blocked = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        native_source_key: "fixture-blocked".to_owned(),
        ..source.clone()
    };
    let (snapshots, state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file_bytes("fixture-exact", b"abc"),
        ExpectedSnapshot::missing("fixture-missing", AgentAssetSourceKind::File),
        ExpectedSnapshot::blocked(
            "fixture-blocked",
            AgentAssetSourceKind::File,
            AgentAssetDiagnostic::PolicyBlocked,
        ),
    ]);
    let mut run = inventory_run(AgentAssetLimits::DEFAULT);
    let roots: [&Path; 0] = [];
    let first = snapshots.snapshot(
        SnapshotRequest {
            source: &source,
            source_id: "source:exact",
            trusted_roots: &roots,
        },
        &mut run,
    );
    let second = snapshots.snapshot(
        SnapshotRequest {
            source: &missing,
            source_id: "source:missing",
            trusted_roots: &roots,
        },
        &mut run,
    );
    let third = snapshots.snapshot(
        SnapshotRequest {
            source: &blocked,
            source_id: "source:blocked",
            trusted_roots: &roots,
        },
        &mut run,
    );
    assert!(matches!(first, AgentAssetSnapshot::File { bytes, .. } if bytes == b"abc"));
    assert!(matches!(second, AgentAssetSnapshot::Missing { .. }));
    assert!(matches!(third, AgentAssetSnapshot::Blocked { .. }));
    let state = state.lock().expect("exact snapshot state lock");
    assert_eq!(state.opens, vec!["source:exact"]);
    assert_eq!(state.charges, vec![("source:exact".to_owned(), 3)]);
    drop(state);
    snapshots.assert_scripted_exhausted();
}

struct CodexFixtureSnapshotPort {
    state: Arc<Mutex<FakeSnapshotState>>,
    files: BTreeMap<String, Vec<u8>>,
    blocked: BTreeMap<String, AgentAssetDiagnostic>,
}

impl CodexFixtureSnapshotPort {
    fn new(
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
        blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        let state = Arc::new(Mutex::new(FakeSnapshotState::default()));
        (
            Self {
                state: Arc::clone(&state),
                files: files
                    .into_iter()
                    .map(|(key, bytes)| (key.to_owned(), bytes))
                    .collect(),
                blocked: blocked
                    .into_iter()
                    .map(|(key, diagnostic)| (key.to_owned(), diagnostic))
                    .collect(),
            },
            state,
        )
    }
}

impl SnapshotPort for CodexFixtureSnapshotPort {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let source_id = request.source_id.to_owned();
        let native_source_key = request.source.native_source_key.clone();
        self.state
            .lock()
            .expect("codex fixture snapshot state lock")
            .attempts
            .push(source_id.clone());
        self.state
            .lock()
            .expect("codex fixture snapshot state lock")
            .native_source_keys
            .push(native_source_key.clone());
        let is_directory = request.source.source_kind == AgentAssetSourceKind::Directory;
        let revision = synthetic_revision(&source_id, 0, is_directory);
        if let Some(diagnostic) = self.blocked.get(&native_source_key) {
            return AgentAssetSnapshot::Blocked {
                revision,
                diagnostic: diagnostic.clone(),
            };
        }
        let Some(bytes) = self.files.get(&native_source_key) else {
            if is_directory {
                let entries = if native_source_key == "plugins" {
                    vec![
                        AgentAssetDirectoryEntry {
                            name: "registry".to_owned(),
                            source_kind: AgentAssetSourceKind::Directory,
                            is_symlink: false,
                        },
                        AgentAssetDirectoryEntry {
                            name: "plugin-name".to_owned(),
                            source_kind: AgentAssetSourceKind::Directory,
                            is_symlink: false,
                        },
                        AgentAssetDirectoryEntry {
                            name: "1.2.3".to_owned(),
                            source_kind: AgentAssetSourceKind::Directory,
                            is_symlink: false,
                        },
                    ]
                } else {
                    Vec::new()
                };
                return AgentAssetSnapshot::DirectoryManifest {
                    entries,
                    revision,
                    complete: true,
                };
            }
            return AgentAssetSnapshot::Missing { revision };
        };
        let bytes_len = bytes.len();
        if let Err(diagnostic) = run.can_start_read(bytes_len) {
            return AgentAssetSnapshot::Blocked {
                revision: synthetic_revision(&source_id, bytes_len, is_directory),
                diagnostic,
            };
        }
        run.commit_read(bytes_len);
        let revision = synthetic_revision(&source_id, bytes_len, is_directory);
        let mut state = self
            .state
            .lock()
            .expect("codex fixture snapshot state lock");
        state.opens.push(source_id.clone());
        state.charges.push((source_id, bytes_len));
        drop(state);
        if is_directory {
            AgentAssetSnapshot::DirectoryManifest {
                entries: Vec::new(),
                revision,
                complete: true,
            }
        } else {
            AgentAssetSnapshot::File {
                bytes: bytes.clone(),
                revision,
            }
        }
    }
}

struct ClaudeFixtureSnapshotPort {
    state: Arc<Mutex<FakeSnapshotState>>,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    directories: Mutex<BTreeMap<String, Vec<AgentAssetDirectoryEntry>>>,
    blocked: Mutex<BTreeMap<String, AgentAssetDiagnostic>>,
    strict_expectations: Option<BTreeMap<String, (PathBuf, AgentAssetSourceKind)>>,
    strict_missing_directories: Mutex<BTreeSet<PathBuf>>,
    strict_missing_files: Mutex<BTreeSet<String>>,
}

enum ClaudeStrictSnapshotValue {
    File(Vec<u8>),
    Directory(Vec<AgentAssetDirectoryEntry>),
    Blocked(AgentAssetDiagnostic),
    Missing,
}

fn claude_fixture_source_expectation(
    root: &Path,
    workspace: &Path,
    key: &str,
) -> Option<(PathBuf, AgentAssetSourceKind)> {
    let expectation = match key {
        "account" => (root.join(".claude.json"), AgentAssetSourceKind::File),
        "settings" => (
            root.join(".claude/settings.json"),
            AgentAssetSourceKind::File,
        ),
        "skills" => (root.join(".claude/skills"), AgentAssetSourceKind::Directory),
        "plugin-registry" => (
            root.join(".claude/plugins/installed_plugins.json"),
            AgentAssetSourceKind::File,
        ),
        "plugin-cache" => (
            root.join(".claude/plugins/cache"),
            AgentAssetSourceKind::Directory,
        ),
        "known-marketplaces" => (
            root.join(".claude/plugins/known_marketplaces.json"),
            AgentAssetSourceKind::File,
        ),
        "managed-settings" => (
            claude_managed_root().join("managed-settings.json"),
            AgentAssetSourceKind::File,
        ),
        "managed-mcp" => (
            claude_managed_root().join("managed-mcp.json"),
            AgentAssetSourceKind::File,
        ),
        "workspace-settings" => (
            workspace.join(".claude/settings.json"),
            AgentAssetSourceKind::File,
        ),
        "workspace-local-settings" => (
            workspace.join(".claude/settings.local.json"),
            AgentAssetSourceKind::File,
        ),
        "workspace-mcp" => (workspace.join(".mcp.json"), AgentAssetSourceKind::File),
        "workspace-skills" => (
            workspace.join(".claude/skills"),
            AgentAssetSourceKind::Directory,
        ),
        _ if key.starts_with("skill-manifest:skills:") => {
            let name = key.strip_prefix("skill-manifest:skills:")?;
            (
                root.join(".claude/skills").join(name).join("SKILL.md"),
                AgentAssetSourceKind::File,
            )
        }
        _ if key.starts_with("skill-manifest:workspace-skills:") => {
            let name = key.strip_prefix("skill-manifest:workspace-skills:")?;
            (
                workspace.join(".claude/skills").join(name).join("SKILL.md"),
                AgentAssetSourceKind::File,
            )
        }
        _ => return None,
    };
    Some(expectation)
}

fn claude_managed_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    }
}

fn claude_instruction_source_expectations(
    home: &Path,
    workspace: &Path,
) -> BTreeMap<String, (PathBuf, AgentAssetSourceKind)> {
    // This fixture matrix is independent of native discovery. Ancestor paths
    // vary with the test temp directory, while their four documented filenames
    // and the single user instruction source remain exact.
    let user_instructions = home.join(".claude/CLAUDE.md");
    let mut expected = BTreeMap::from([(
        "user-instructions".to_owned(),
        (user_instructions.clone(), AgentAssetSourceKind::File),
    )]);
    let mut ancestors = workspace.ancestors().collect::<Vec<_>>();
    assert!(
        ancestors.len() <= 32,
        "fixture exceeds the native ancestor bound"
    );
    ancestors.reverse();
    for (index, directory) in ancestors.into_iter().enumerate() {
        if directory.parent().is_none() {
            continue;
        }
        for relative in [
            "CLAUDE.md",
            ".claude/CLAUDE.md",
            "CLAUDE.local.md",
            "AGENTS.md",
        ] {
            let path = directory.join(relative);
            if path == user_instructions {
                continue;
            }
            assert!(expected
                .insert(
                    format!("workspace-instructions:{index}:{relative}"),
                    (path, AgentAssetSourceKind::File),
                )
                .is_none());
        }
    }
    expected
}

impl ClaudeFixtureSnapshotPort {
    fn new(
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
        directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
        blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        Self::build(files, directories, blocked, None)
    }

    fn strict(
        root: &Path,
        workspace: &Path,
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
        directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
        blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
        missing_directories: &[PathBuf],
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        let files = files.into_iter().collect::<Vec<_>>();
        let directories = directories.into_iter().collect::<Vec<_>>();
        let blocked = blocked.into_iter().collect::<Vec<_>>();
        let mut expectations = BTreeMap::new();
        for key in files
            .iter()
            .map(|(key, _)| *key)
            .chain(directories.iter().map(|(key, _)| *key))
            .chain(blocked.iter().map(|(key, _)| *key))
        {
            if let Some(expectation) = claude_fixture_source_expectation(root, workspace, key) {
                expectations.insert(key.to_owned(), expectation);
            }
        }
        let instruction_expectations = claude_instruction_source_expectations(root, workspace);
        let supplied_keys = files
            .iter()
            .map(|(key, _)| *key)
            .chain(directories.iter().map(|(key, _)| *key))
            .chain(blocked.iter().map(|(key, _)| *key))
            .collect::<BTreeSet<_>>();
        let missing_files = instruction_expectations
            .keys()
            .filter(|key| !supplied_keys.contains(key.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>();
        expectations.extend(instruction_expectations);
        let (mut port, state) = Self::build(files, directories, blocked, Some(expectations));
        port.strict_missing_directories = Mutex::new(missing_directories.iter().cloned().collect());
        state
            .lock()
            .expect("strict fixture state lock")
            .strict_remaining_missing_files = missing_files.clone();
        port.strict_missing_files = Mutex::new(missing_files);
        (port, state)
    }

    fn build(
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
        directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
        blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
        strict_expectations: Option<BTreeMap<String, (PathBuf, AgentAssetSourceKind)>>,
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        let files = files.into_iter().collect::<Vec<_>>();
        let directories = directories.into_iter().collect::<Vec<_>>();
        let blocked = blocked.into_iter().collect::<Vec<_>>();
        let state = Arc::new(Mutex::new(FakeSnapshotState::default()));
        if strict_expectations.is_some() {
            let mut state_guard = state.lock().expect("strict fixture state lock");
            let mut file_keys = BTreeSet::new();
            for (key, _) in &files {
                if !file_keys.insert(*key) {
                    state_guard
                        .strict_violations
                        .push(format!("duplicate file fixture input for {key}"));
                }
            }
            let mut directory_keys = BTreeSet::new();
            for (key, _) in &directories {
                if !directory_keys.insert(*key) {
                    state_guard
                        .strict_violations
                        .push(format!("duplicate directory fixture input for {key}"));
                }
            }
            let mut blocked_keys = BTreeSet::new();
            for (key, _) in &blocked {
                if !blocked_keys.insert(*key) {
                    state_guard
                        .strict_violations
                        .push(format!("duplicate blocked fixture input for {key}"));
                }
            }
            state_guard.strict_remaining_files =
                files.iter().map(|(key, _)| (*key).to_owned()).collect();
            state_guard.strict_remaining_directories = directories
                .iter()
                .map(|(key, _)| (*key).to_owned())
                .collect();
            state_guard.strict_remaining_blocked =
                blocked.iter().map(|(key, _)| (*key).to_owned()).collect();
            drop(state_guard);
            return Self::build_with_collected(
                state,
                files,
                directories,
                blocked,
                strict_expectations,
            );
        }
        Self::build_with_collected(state, files, directories, blocked, strict_expectations)
    }

    fn build_with_collected(
        state: Arc<Mutex<FakeSnapshotState>>,
        files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
        directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
        blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
        strict_expectations: Option<BTreeMap<String, (PathBuf, AgentAssetSourceKind)>>,
    ) -> (Self, Arc<Mutex<FakeSnapshotState>>) {
        (
            Self {
                state: Arc::clone(&state),
                files: Mutex::new(
                    files
                        .into_iter()
                        .map(|(key, bytes)| (key.to_owned(), bytes))
                        .collect(),
                ),
                directories: Mutex::new(
                    directories
                        .into_iter()
                        .map(|(key, entries)| (key.to_owned(), entries))
                        .collect(),
                ),
                blocked: Mutex::new(
                    blocked
                        .into_iter()
                        .map(|(key, diagnostic)| (key.to_owned(), diagnostic))
                        .collect(),
                ),
                strict_expectations,
                strict_missing_directories: Mutex::default(),
                strict_missing_files: Mutex::default(),
            },
            state,
        )
    }

    fn take_strict_value(
        &self,
        key: &str,
        path: &Path,
        source_kind: AgentAssetSourceKind,
    ) -> ClaudeStrictSnapshotValue {
        let blocked = self
            .blocked
            .lock()
            .expect("claude fixture blocked store lock")
            .remove(key);
        let directory = self
            .directories
            .lock()
            .expect("claude fixture directory store lock")
            .remove(key);
        let file = self
            .files
            .lock()
            .expect("claude fixture file store lock")
            .remove(key);
        let missing_directory = source_kind == AgentAssetSourceKind::Directory
            && self
                .strict_missing_directories
                .lock()
                .expect("strict missing directory store lock")
                .remove(path);
        let missing_file = source_kind == AgentAssetSourceKind::File
            && self
                .strict_missing_files
                .lock()
                .expect("strict missing file store lock")
                .remove(key);
        let supplied = usize::from(missing_directory)
            + usize::from(missing_file)
            + usize::from(blocked.is_some())
            + usize::from(directory.is_some())
            + usize::from(file.is_some());
        let mut state = self.state.lock().expect("claude fixture strict state lock");
        if !state.strict_consumed_source_keys.insert(key.to_owned()) {
            state.strict_repeated_source_keys.insert(key.to_owned());
            state
                .strict_violations
                .push(format!("repeated snapshot request for {key}"));
        }
        match self
            .strict_expectations
            .as_ref()
            .and_then(|items| items.get(key))
        {
            Some((expected_path, expected_kind)) => {
                if expected_path != path {
                    state.strict_violations.push(format!(
                        "source path mismatch for {key}: expected {}, got {}",
                        expected_path.display(),
                        path.display()
                    ));
                }
                if *expected_kind != source_kind {
                    state.strict_violations.push(format!(
                        "source kind mismatch for {key}: expected {expected_kind:?}, got {source_kind:?}"
                    ));
                }
            }
            None if !missing_directory => {
                state.strict_unexpected_source_keys.insert(key.to_owned());
                state
                    .strict_violations
                    .push(format!("unexpected snapshot source {key}"));
            }
            None => {}
        }
        if supplied == 0 {
            state
                .strict_violations
                .push(format!("missing fixture input for {key}"));
        }
        if supplied > 1 {
            state
                .strict_violations
                .push(format!("duplicate fixture inputs for {key}"));
        }
        state.strict_remaining_blocked.remove(key);
        state.strict_remaining_directories.remove(key);
        state.strict_remaining_files.remove(key);
        state.strict_remaining_missing_files.remove(key);
        drop(state);
        if let Some(diagnostic) = blocked {
            ClaudeStrictSnapshotValue::Blocked(diagnostic)
        } else if let Some(entries) = directory {
            ClaudeStrictSnapshotValue::Directory(entries)
        } else if let Some(bytes) = file {
            ClaudeStrictSnapshotValue::File(bytes)
        } else {
            // A missing optional source is represented as Missing, while the
            // strict violation above makes an omitted fixture input visible.
            ClaudeStrictSnapshotValue::Missing
        }
    }
}

impl SnapshotPort for ClaudeFixtureSnapshotPort {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let source_id = request.source_id.to_owned();
        let key = request.source.native_source_key.clone();
        let is_directory = request.source.source_kind == AgentAssetSourceKind::Directory;
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .attempts
            .push(source_id.clone());
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .native_source_keys
            .push(key.clone());
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .snapshot_paths
            .push((key.clone(), request.source.path.clone()));
        let revision = synthetic_revision(&source_id, 0, is_directory);
        let value = if self.strict_expectations.is_some() {
            self.take_strict_value(&key, &request.source.path, request.source.source_kind)
        } else {
            let diagnostic = self
                .blocked
                .lock()
                .expect("claude fixture blocked store lock")
                .get(&key)
                .cloned();
            if let Some(diagnostic) = diagnostic {
                return AgentAssetSnapshot::Blocked {
                    revision,
                    diagnostic: rebind_source_id(diagnostic, &source_id),
                };
            }
            if self
                .directories
                .lock()
                .expect("claude fixture directory store lock")
                .contains_key(&key)
            {
                let entries = self
                    .directories
                    .lock()
                    .expect("claude fixture directory store lock")
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                ClaudeStrictSnapshotValue::Directory(entries)
            } else {
                let bytes = self
                    .files
                    .lock()
                    .expect("claude fixture file store lock")
                    .get(&key)
                    .cloned();
                bytes.map_or(
                    ClaudeStrictSnapshotValue::Missing,
                    ClaudeStrictSnapshotValue::File,
                )
            }
        };
        let value = match value {
            ClaudeStrictSnapshotValue::Blocked(diagnostic) => {
                return AgentAssetSnapshot::Blocked {
                    revision,
                    diagnostic: rebind_source_id(diagnostic, &source_id),
                }
            }
            ClaudeStrictSnapshotValue::Missing => {
                return AgentAssetSnapshot::Missing {
                    revision: if self.strict_expectations.is_some() {
                        super::snapshot::revision_for_missing(&request.source.path)
                    } else {
                        revision
                    },
                }
            }
            value => value,
        };
        if let ClaudeStrictSnapshotValue::Directory(entries) = value {
            if let Err(diagnostic) = run.can_start_read(0) {
                return AgentAssetSnapshot::Blocked {
                    revision,
                    diagnostic,
                };
            }
            return AgentAssetSnapshot::DirectoryManifest {
                entries,
                revision,
                complete: true,
            };
        }
        let ClaudeStrictSnapshotValue::File(bytes) = value else {
            unreachable!("strict snapshot value handled above")
        };
        if let Err(diagnostic) = run.can_start_read(bytes.len()) {
            return AgentAssetSnapshot::Blocked {
                revision: synthetic_revision(&source_id, bytes.len(), false),
                diagnostic,
            };
        }
        run.commit_read(bytes.len());
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .opens
            .push(source_id.clone());
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .snapshots
            .push((
                source_id.clone(),
                synthetic_revision(request.source_id, bytes.len(), false).identity,
                bytes.clone(),
            ));
        self.state
            .lock()
            .expect("claude fixture snapshot state lock")
            .charges
            .push((source_id, bytes.len()));
        AgentAssetSnapshot::File {
            bytes: bytes.clone(),
            revision: synthetic_revision(request.source_id, bytes.len(), false),
        }
    }
}

#[derive(Default)]
struct BreakInitialSourceOutput {
    emissions: usize,
    diagnostics: Vec<AgentAssetDiagnostic>,
    stop: Option<crate::services::agent_cli::contracts::AgentOutputStop>,
}

#[derive(Default)]
struct BreakFollowUpSourceOutput {
    emissions: usize,
    diagnostics: Vec<AgentAssetDiagnostic>,
    stop: Option<crate::services::agent_cli::contracts::AgentOutputStop>,
}

#[derive(Default)]
struct BreakParseOutput {
    emissions: usize,
    diagnostics: Vec<AgentAssetDiagnostic>,
    declarations: Vec<crate::services::agent_cli::contracts::ParsedAgentAsset>,
}

#[derive(Default)]
struct BreakResolveOutput {
    emissions: usize,
    diagnostics: Vec<AgentAssetDiagnostic>,
    drafts: Vec<crate::services::agent_cli::contracts::AgentAssetProjectedDraft>,
}

impl AgentDiagnosticOutput for BreakParseOutput {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentParseOutput for BreakParseOutput {
    fn emit_declaration(
        &mut self,
        value: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.emissions += 1;
        self.declarations.push(value);
        std::ops::ControlFlow::Break(
            crate::services::agent_cli::contracts::AgentOutputStop::EntryLimit,
        )
    }
}

impl AgentDiagnosticOutput for BreakResolveOutput {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentResolveOutput for BreakResolveOutput {
    fn emit_draft(
        &mut self,
        value: crate::services::agent_cli::contracts::AgentAssetProjectedDraft,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.emissions += 1;
        self.drafts.push(value);
        std::ops::ControlFlow::Break(
            crate::services::agent_cli::contracts::AgentOutputStop::EntryLimit,
        )
    }
}

impl AgentDiagnosticOutput for BreakFollowUpSourceOutput {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl FollowUpSourceOutput for BreakFollowUpSourceOutput {
    fn emit_follow_up(
        &mut self,
        _value: crate::services::agent_cli::contracts::AgentFollowUpSourceSpec,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.emissions += 1;
        let stop = crate::services::agent_cli::contracts::AgentOutputStop::SourceLimit;
        self.stop = Some(stop);
        std::ops::ControlFlow::Break(stop)
    }
}

impl AgentDiagnosticOutput for BreakInitialSourceOutput {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        self.diagnostics.push(value);
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl InitialSourceOutput for BreakInitialSourceOutput {
    fn emit_initial(
        &mut self,
        _value: AgentAssetSourceSpec,
    ) -> std::ops::ControlFlow<crate::services::agent_cli::contracts::AgentOutputStop> {
        self.emissions += 1;
        let stop = crate::services::agent_cli::contracts::AgentOutputStop::SourceLimit;
        self.stop = Some(stop);
        std::ops::ControlFlow::Break(stop)
    }
}

fn assert_claude_source_callback_stops_after_break(home: &Path, workspace: &Path) {
    let adapter = definition(AgentCliKind::ClaudeCode).environment();
    let mut run = inventory_run(AgentAssetLimits::DEFAULT);
    let contexts = adapter.discover_contexts(
        AgentContextDiscoveryRequest {
            environment_id: "native:test",
            agent_kind: AgentCliKind::ClaudeCode,
            home,
            workspace: Some(workspace),
            installations: &[],
            workspace_trust: AgentTrustState::Unknown,
        },
        &mut run,
    );
    let mut output = BreakInitialSourceOutput::default();
    adapter.discover_sources(
        AgentSourceDiscoveryRequest {
            installations: &[],
            context: &contexts[0],
            home,
            workspace: Some(workspace),
        },
        &mut output,
    );
    assert_eq!(output.emissions, 1);
    assert_eq!(
        output.stop,
        Some(crate::services::agent_cli::contracts::AgentOutputStop::SourceLimit)
    );
}

thread_local! {
    static CLAUDE_CONFIG_ROOT_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

struct ClaudeConfigRootOverrideGuard {
    previous: Option<PathBuf>,
}

impl ClaudeConfigRootOverrideGuard {
    fn new(config_root: PathBuf) -> Self {
        let previous = CLAUDE_CONFIG_ROOT_OVERRIDE.with(|slot| slot.replace(Some(config_root)));
        Self { previous }
    }
}

impl Drop for ClaudeConfigRootOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CLAUDE_CONFIG_ROOT_OVERRIDE.with(|slot| {
            slot.replace(previous);
        });
    }
}

fn claude_test_discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let workspace = request.workspace.map(fs::canonicalize).transpose().unwrap();
    let adapter = definition(AgentCliKind::ClaudeCode).environment();
    let mut contexts = adapter.discover_contexts(request, output);
    let config_root_override =
        CLAUDE_CONFIG_ROOT_OVERRIDE.with(|slot| slot.borrow().as_ref().cloned());
    if let Some(config_root) = config_root_override {
        for context in &mut contexts {
            context.config_root = config_root.to_string_lossy().into_owned();
        }
    }
    if let Some(workspace) = workspace {
        for context in &mut contexts {
            context.workspace_id = Some(workspace.to_string_lossy().into_owned());
        }
    }
    contexts
}

fn claude_test_discover_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .discover_sources(request, output);
}

fn claude_test_discover_follow_up_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .discover_follow_up_sources(request, output);
}

fn claude_test_parse(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .parse(request, output);
}

fn claude_test_parse_with_tee(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    CLAUDE_NATIVE_PAYLOAD_TEE.with(|slot| {
        let mut tee_slot = slot.borrow_mut();
        let Some(capture) = tee_slot.as_mut() else {
            claude_test_parse(request, output);
            return;
        };
        let mut tee = TeeParseOutput { capture, output };
        claude_test_parse(request, &mut tee);
    });
}

fn claude_test_resolve(request: AgentAssetResolveRequest<'_>, output: &mut dyn AgentResolveOutput) {
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .resolve(request, output);
}

fn claude_test_discover_workspace_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .discover_workspace_trust_sources(request, output);
}

fn claude_test_resolve_workspace_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    let canonical = fs::canonicalize(request.workspace).unwrap();
    definition(AgentCliKind::ClaudeCode)
        .environment()
        .resolve_workspace_trust(
            AgentWorkspaceTrustResolveRequest {
                workspace: &canonical,
                workspace_lexical: request.workspace_lexical,
                sources: request.sources,
            },
            output,
        )
}

fn claude_test_adapter() -> EnvironmentAdapter {
    // Full inventory fixtures intentionally omit the native mutation catalog.
    // The mechanism gate reports NoOfficialMechanism for otherwise ungated rows.
    EnvironmentAdapter::with_pipeline(
        claude_test_discover_contexts,
        claude_test_discover_sources,
        Some(claude_test_discover_follow_up_sources),
        "claude-code-test",
        claude_test_parse,
        claude_test_resolve,
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .state_assessor(),
    )
    .with_workspace_trust_authority(
        claude_test_discover_workspace_trust_sources,
        claude_test_resolve_workspace_trust,
    )
}

fn claude_test_tee_adapter() -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        claude_test_discover_contexts,
        claude_test_discover_sources,
        Some(claude_test_discover_follow_up_sources),
        "claude-code-test-tee",
        claude_test_parse_with_tee,
        claude_test_resolve,
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .state_assessor(),
    )
    .with_workspace_trust_authority(
        claude_test_discover_workspace_trust_sources,
        claude_test_resolve_workspace_trust,
    )
}

fn claude_test_reversed_adapter() -> EnvironmentAdapter {
    EnvironmentAdapter::with_pipeline(
        claude_test_discover_contexts,
        claude_test_discover_sources_reversed,
        Some(claude_test_discover_follow_up_sources),
        "claude-code-test-reversed",
        claude_test_parse_reversed,
        claude_test_resolve_reversed,
        definition(AgentCliKind::ClaudeCode)
            .environment()
            .state_assessor(),
    )
    .with_workspace_trust_authority(
        claude_test_discover_workspace_trust_sources,
        claude_test_resolve_workspace_trust,
    )
}

fn build_claude_inventory(
    name: &str,
    files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    installations: &dyn InstallationDiscoveryPort,
) -> (
    PathBuf,
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    build_claude_inventory_at_root(test_root(name), files, directories, blocked, installations)
}

fn build_claude_inventory_at_root(
    root: PathBuf,
    files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    installations: &dyn InstallationDiscoveryPort,
) -> (
    PathBuf,
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    build_claude_inventory_at_root_with_workspace(
        root,
        files,
        directories,
        blocked,
        installations,
        None,
        None,
    )
}

struct ClaudeInventoryBuildOptions<'a> {
    installations: &'a dyn InstallationDiscoveryPort,
    workspace_input: Option<PathBuf>,
    settings: Option<&'a crate::models::AppSettings>,
    test_adapter: EnvironmentAdapter,
    strict_snapshots: bool,
    strict_missing_directories: &'a [PathBuf],
}

fn build_claude_inventory_at_root_with_workspace(
    root: PathBuf,
    files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    installations: &dyn InstallationDiscoveryPort,
    workspace_input: Option<PathBuf>,
    settings: Option<&crate::models::AppSettings>,
) -> (
    PathBuf,
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    build_claude_inventory_at_root_with_workspace_and_adapter(
        root,
        files,
        directories,
        blocked,
        ClaudeInventoryBuildOptions {
            installations,
            workspace_input,
            settings,
            test_adapter: claude_test_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    )
}

fn build_claude_inventory_at_root_with_workspace_and_adapter(
    root: PathBuf,
    files: impl IntoIterator<Item = (&'static str, Vec<u8>)>,
    directories: impl IntoIterator<Item = (&'static str, Vec<AgentAssetDirectoryEntry>)>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    options: ClaudeInventoryBuildOptions<'_>,
) -> (
    PathBuf,
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    let ClaudeInventoryBuildOptions {
        installations,
        workspace_input,
        settings,
        test_adapter,
        strict_snapshots,
        strict_missing_directories,
    } = options;
    let workspace = root.join("workspace");
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    let workspace_input = workspace_input.unwrap_or_else(|| workspace.clone());
    let replace_bytes = |mut bytes: Vec<u8>, needle: &[u8], replacement: &[u8]| {
        while let Some(index) = bytes
            .windows(needle.len())
            .position(|window| window == needle)
        {
            bytes.splice(index..index + needle.len(), replacement.iter().copied());
        }
        bytes
    };
    let expand = |bytes: Vec<u8>| {
        let bytes = if cfg!(windows) {
            replace_bytes(bytes, b"/fixture", b"C:/fixture")
        } else {
            bytes
        };
        let bytes = replace_bytes(
            bytes,
            b"__WORKSPACE__",
            serde_json::to_string(&workspace.to_string_lossy())
                .unwrap()
                .trim_matches('"')
                .as_bytes(),
        );
        replace_bytes(
            bytes,
            b"__WORKSPACE_LEXICAL__",
            serde_json::to_string(&workspace_input.to_string_lossy())
                .unwrap()
                .trim_matches('"')
                .as_bytes(),
        )
    };
    let files = files
        .into_iter()
        .map(|(key, bytes)| (key, expand(bytes)))
        .collect::<Vec<_>>();
    let (snapshots, state) = if strict_snapshots {
        ClaudeFixtureSnapshotPort::strict(
            &root,
            workspace_input.as_path(),
            files,
            directories,
            blocked,
            strict_missing_directories,
        )
    } else {
        ClaudeFixtureSnapshotPort::new(files, directories, blocked)
    };
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
        environment: test_adapter,
        ..*template
    }];
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: Some(&workspace_input),
            settings,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    (root, workspace, inventory, state)
}

fn claude_json(value: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}

fn build_claude_trust_case(
    name: &str,
    account: Option<Vec<u8>>,
    snapshot_kind: Option<AgentAssetDiagnostic>,
    directory_snapshot: bool,
    lexical_workspace: bool,
) -> (
    PathBuf,
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    let root = test_root(name);
    let blocked = snapshot_kind
        .into_iter()
        .map(|diagnostic| ("account", diagnostic))
        .collect::<Vec<_>>();
    let files = account
        .into_iter()
        .map(|bytes| ("account", bytes))
        .collect::<Vec<_>>();
    let directories = if directory_snapshot {
        vec![("account", Vec::new())]
    } else {
        Vec::new()
    };
    let workspace_input = lexical_workspace.then(|| {
        fs::create_dir_all(root.join("workspace")).unwrap();
        fs::canonicalize(root.join("workspace")).unwrap().join(".")
    });
    build_claude_inventory_at_root_with_workspace(
        root,
        files,
        directories,
        blocked,
        &RealInstallationDiscoveryPort,
        workspace_input,
        None,
    )
}

fn claude_entries(
    names: impl IntoIterator<Item = (&'static str, AgentAssetSourceKind, bool)>,
) -> Vec<AgentAssetDirectoryEntry> {
    names
        .into_iter()
        .map(|(name, source_kind, is_symlink)| AgentAssetDirectoryEntry {
            name: name.to_owned(),
            source_kind,
            is_symlink,
        })
        .collect()
}

fn claude_focused_context() -> crate::models::AgentConfigurationContext {
    let home = PathBuf::from("/fixture/home");
    let workspace = PathBuf::from("/fixture/workspace");
    let request = AgentContextDiscoveryRequest {
        environment_id: "native:claude-focused",
        agent_kind: AgentCliKind::ClaudeCode,
        home: &home,
        workspace: Some(&workspace),
        installations: &[],
        workspace_trust: AgentTrustState::Trusted,
    };
    let mut context = super::default_context(request, home.join(".claude"), 3)
        .into_iter()
        .next()
        .expect("focused Claude context");
    context.id = "context:claude-focused".to_owned();
    context
}

fn claude_focused_source(
    native_source_key: &str,
    scope: AgentAssetScope,
    precedence: u32,
    categories: &[AgentAssetCategory],
) -> AgentAssetSourceSpec {
    let root = PathBuf::from("/fixture/.claude");
    AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: native_source_key.to_owned(),
        label: native_source_key.to_owned(),
        scope,
        path: root.join(format!("{native_source_key}.json")),
        allowed_root: root,
        precedence,
        writable: matches!(
            scope,
            AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
        ),
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: categories.to_vec(),
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin { scope, precedence },
        ],
    }
}

fn synthetic_revision(source_id: &str, size: usize, is_directory: bool) -> AgentAssetRevision {
    AgentAssetRevision {
        identity: format!("synthetic:{source_id}:{size}"),
        observed_at: super::snapshot::now_string(),
        size_bytes: Some(size as u64),
        is_missing: false,
        is_directory,
        is_symlink: false,
    }
}

fn fixture_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    super::default_context(request, request.home.join("fixture"), 1)
}

fn fixture_two_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let mut contexts = super::default_context(request, request.home.join("fixture/first"), 1);
    contexts.extend(super::default_context(
        request,
        request.home.join("fixture/second"),
        1,
    ));
    contexts
}

fn fixture_file_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    output
        .emit_initial(AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: "fixture-file".to_owned(),
            label: "Fixture file".to_owned(),
            scope: AgentAssetScope::User,
            path: root.join("config.json"),
            allowed_root: root.to_path_buf(),
            precedence: 1,
            writable: true,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 1,
                },
            ],
        })
        .is_break();
}

fn fixture_suppression_parse(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    output
        .emit_declaration(parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: "suppressed",
                resolution_group_key: "suppressed",
                category: AgentAssetCategory::Skill,
                native_id: "suppressed",
                label: "Suppressed fixture",
                logical_origin: physical_origin(request.source),
                declared_state: AgentAssetDeclaredState::Unknown,
                trust_state: AgentTrustState::Untrusted,
                role: crate::models::AgentAssetDeclarationRole::Definition,
                participation: crate::models::AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
                },
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Skill {
                    enabled: AgentAssetDeclaredState::Unknown,
                    invocation_policy: AgentSkillInvocationPolicy::Unknown,
                },
                facts: BTreeMap::new(),
            },
        ))
        .is_break();
}

fn fixture_relationship_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let _ = output.emit_initial(AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-relationships".to_owned(),
        label: "Fixture relationships".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join("relationships.json"),
        allowed_root: root.to_path_buf(),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Plugin, AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    });
}

fn fixture_relationship_parse(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let parent = crate::models::AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "provider".to_owned(),
        qualifier: Some("plugin:provider".to_owned()),
    };
    for (category, native_id, provided_by, action_owner, affected) in [
        (
            AgentAssetCategory::Plugin,
            "provider",
            None,
            None,
            vec![crate::models::AgentAssetNativeRef {
                category: AgentAssetCategory::Skill,
                native_id: "child".to_owned(),
                qualifier: Some("skill:child".to_owned()),
            }],
        ),
        (
            AgentAssetCategory::Skill,
            "child",
            Some(parent.clone()),
            Some(parent.clone()),
            Vec::new(),
        ),
    ] {
        if output
            .emit_declaration(parsed_asset(
                request,
                ParsedAssetInput {
                    declaration_key: native_id,
                    resolution_group_key: native_id,
                    category,
                    native_id,
                    label: native_id,
                    logical_origin: physical_origin(request.source),
                    declared_state: AgentAssetDeclaredState::Enabled,
                    trust_state: AgentTrustState::Trusted,
                    role: crate::models::AgentAssetDeclarationRole::Definition,
                    participation: crate::models::AgentAssetResolutionParticipation::Participates,
                    provided_by,
                    action_owner,
                    explicitly_affected: affected,
                    details: match category {
                        AgentAssetCategory::Plugin => AgentAssetDetails::Plugin {
                            install_state: AgentAssetInstallState::Installed,
                            enabled: AgentAssetDeclaredState::Enabled,
                            trusted: AgentTrustState::Trusted,
                        },
                        AgentAssetCategory::Skill => AgentAssetDetails::Skill {
                            enabled: AgentAssetDeclaredState::Enabled,
                            invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
                        },
                        _ => unreachable!("relationship fixture category"),
                    },
                    facts: BTreeMap::new(),
                },
            ))
            .is_break()
        {
            return;
        }
    }
}

fn fixture_passive_asset_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-passive-assets".to_owned(),
        label: "Fixture passive assets".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join("passive-assets.json"),
        allowed_root: root.to_path_buf(),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Plugin,
            AgentAssetCategory::Hook,
            AgentAssetCategory::StatusUi,
        ],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    };
    let _terminal_result = output.emit_initial(source);
}

#[cfg(unix)]
fn fixture_empty_parse(_request: AgentAssetParseRequest<'_>, _output: &mut dyn AgentParseOutput) {}

fn fixture_empty_resolve(
    _request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    _output: &mut dyn AgentResolveOutput,
) {
}

#[cfg(unix)]
fn fixture_system_boundary_sources(
    _request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let sources = [
        (
            "system-positive",
            "/etc/codex/config.toml",
            "/etc/codex",
            AgentAssetScope::System,
            false,
            AgentAssetSourceKind::File,
        ),
        (
            "managed-positive",
            "/etc/codex/requirements.toml",
            "/etc/codex",
            AgentAssetScope::Managed,
            false,
            AgentAssetSourceKind::File,
        ),
        (
            "system-writable",
            "/etc/codex/writable.toml",
            "/etc/codex",
            AgentAssetScope::System,
            true,
            AgentAssetSourceKind::File,
        ),
        (
            "system-directory",
            "/etc/codex/cache",
            "/etc/codex",
            AgentAssetScope::System,
            false,
            AgentAssetSourceKind::Directory,
        ),
        (
            "system-root",
            "/config.toml",
            "/",
            AgentAssetScope::System,
            false,
            AgentAssetSourceKind::File,
        ),
        (
            "system-nested",
            "/etc/codex/nested/config.toml",
            "/etc/codex",
            AgentAssetScope::System,
            false,
            AgentAssetSourceKind::File,
        ),
    ];
    for (key, path, allowed_root, scope, writable, source_kind) in sources {
        if output
            .emit_initial(AgentAssetSourceSpec {
                hook_definition_source: true,
                verified_physical_path: None,
                provider: crate::models::AgentAssetProviderOrigin::Unknown,
                origin: crate::models::AgentAssetInstallationOrigin::Unknown,
                path_policy: Default::default(),
                native_source_key: key.to_owned(),
                label: key.to_owned(),
                scope,
                path: PathBuf::from(path),
                allowed_root: PathBuf::from(allowed_root),
                precedence: 1,
                writable,
                sensitive: false,
                source_kind,
                categories: vec![AgentAssetCategory::Skill],
                allowed_logical_origins: vec![
                    crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                        scope,
                        precedence: 1,
                    },
                ],
            })
            .is_break()
        {
            return;
        }
    }
}

#[cfg(unix)]
struct BoundarySnapshotPort {
    elevated_roots: Arc<Mutex<Vec<(String, bool)>>>,
}

#[cfg(unix)]
impl SnapshotPort for BoundarySnapshotPort {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        _run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let elevated = request
            .trusted_roots
            .iter()
            .any(|root| *root == Path::new("/etc/codex"));
        self.elevated_roots
            .lock()
            .expect("boundary snapshot state lock")
            .push((request.source.native_source_key.clone(), elevated));
        let revision = AgentAssetRevision {
            identity: format!("boundary:{}", request.source.native_source_key),
            is_directory: request.source.source_kind == AgentAssetSourceKind::Directory,
            is_missing: request.source.native_source_key == "system-directory",
            ..AgentAssetRevision::default()
        };
        match request.source.native_source_key.as_str() {
            "system-positive" | "managed-positive" => AgentAssetSnapshot::File {
                bytes: b"".to_vec(),
                revision,
            },
            "system-directory" => AgentAssetSnapshot::Missing { revision },
            _ => AgentAssetSnapshot::Blocked {
                revision,
                diagnostic: AgentAssetDiagnostic::SourceOutsideAllowedRoot {
                    source_id: request.source_id.to_owned(),
                },
            },
        }
    }
}

fn fixture_parse_passive_assets(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let declarations = [
        ParsedAssetInput {
            declaration_key: "fixture-mcp",
            resolution_group_key: "fixture-mcp",
            category: AgentAssetCategory::Mcp,
            native_id: "fixture-mcp",
            label: "Fixture MCP",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                declared_state: AgentAssetDeclaredState::Enabled,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: AgentAssetEffectiveAvailability::Available,
            },
            facts: BTreeMap::new(),
        },
        ParsedAssetInput {
            declaration_key: "fixture-plugin",
            resolution_group_key: "fixture-plugin",
            category: AgentAssetCategory::Plugin,
            native_id: "fixture-plugin",
            label: "Fixture Plugin",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                enabled: AgentAssetDeclaredState::Enabled,
                trusted: AgentTrustState::Unknown,
            },
            facts: BTreeMap::new(),
        },
        ParsedAssetInput {
            declaration_key: "fixture-hook",
            resolution_group_key: "fixture-hook",
            category: AgentAssetCategory::Hook,
            native_id: "fixture-hook",
            label: "Fixture Hook",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Hook {
                managed: true,
                enabled: AgentAssetDeclaredState::Enabled,
                rule_count: None,
            },
            facts: BTreeMap::new(),
        },
        ParsedAssetInput {
            declaration_key: "fixture-status-ui",
            resolution_group_key: "fixture-status-ui",
            category: AgentAssetCategory::StatusUi,
            native_id: "fixture-status-ui",
            label: "Fixture Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::StatusUi {
                mode: AgentStatusUiMode::Command,
                command_present: true,
            },
            facts: BTreeMap::new(),
        },
    ];

    for declaration in declarations {
        if output
            .emit_declaration(parsed_asset(request, declaration))
            .is_break()
        {
            return;
        }
    }
}

fn fixture_resolve_passive_assets(
    request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    for declaration in request.declarations {
        if output
            .emit_draft(
                fixture_draft(&request, declaration).expect("complete independent fixture input"),
            )
            .is_break()
        {
            return;
        }
    }
}

fn fixture_parse_with_first_invalid_group(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let invalid_group = Path::new(&request.context.config_root).ends_with("first");
    output
        .emit_declaration(parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: if invalid_group { "first" } else { "second" },
                resolution_group_key: if invalid_group {
                    "invalid\nresolution-group"
                } else {
                    "healthy"
                },
                category: AgentAssetCategory::Skill,
                native_id: if invalid_group { "first" } else { "second" },
                label: "Fixture asset",
                logical_origin: physical_origin(request.source),
                declared_state: AgentAssetDeclaredState::Enabled,
                trust_state: AgentTrustState::Unknown,
                role: crate::models::AgentAssetDeclarationRole::Definition,
                participation: crate::models::AgentAssetResolutionParticipation::Participates,
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Skill {
                    enabled: AgentAssetDeclaredState::Enabled,
                    invocation_policy: crate::models::AgentSkillInvocationPolicy::Unknown,
                },
                facts: BTreeMap::new(),
            },
        ))
        .is_break();
}

fn fixture_parent_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let _terminal_result = output.emit_initial(AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-parent".to_owned(),
        label: "Fixture parent".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join("parent"),
        allowed_root: root.to_path_buf(),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    });
}

fn fixture_two_directory_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    for (native_source_key, directory) in [
        ("fixture-parent-a", "parent-a"),
        ("fixture-parent-z", "parent-z"),
    ] {
        if output
            .emit_initial(AgentAssetSourceSpec {
                hook_definition_source: true,
                verified_physical_path: None,
                provider: crate::models::AgentAssetProviderOrigin::Unknown,
                origin: crate::models::AgentAssetInstallationOrigin::Unknown,
                path_policy: Default::default(),
                native_source_key: native_source_key.to_owned(),
                label: native_source_key.to_owned(),
                scope: AgentAssetScope::User,
                path: root.join(directory),
                allowed_root: root.to_path_buf(),
                precedence: 1,
                writable: true,
                sensitive: false,
                source_kind: AgentAssetSourceKind::Directory,
                categories: vec![AgentAssetCategory::Skill],
                allowed_logical_origins: vec![
                    crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                        scope: AgentAssetScope::User,
                        precedence: 1,
                    },
                ],
            })
            .is_break()
        {
            return;
        }
    }
}

fn fixture_two_file_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    fixture_two_file_sources_in_order(request, output, ["fixture-first", "fixture-second"]);
}

fn fixture_two_file_sources_reversed(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    fixture_two_file_sources_in_order(request, output, ["fixture-second", "fixture-first"]);
}

fn fixture_two_file_sources_in_order(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
    native_source_keys: [&str; 2],
) {
    let root = Path::new(&request.context.config_root);
    for native_source_key in native_source_keys {
        let source = AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: native_source_key.to_owned(),
            label: native_source_key.to_owned(),
            scope: AgentAssetScope::User,
            path: root.join(format!("{native_source_key}.json")),
            allowed_root: root.to_path_buf(),
            precedence: 1,
            writable: true,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 1,
                },
            ],
        };
        if output.emit_initial(source).is_break() {
            break;
        }
    }
}

fn fixture_three_file_sources_until_break(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    for native_source_key in ["fixture-z", "fixture-a", "fixture-b", "fixture-never"] {
        let source = AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: native_source_key.to_owned(),
            label: native_source_key.to_owned(),
            scope: AgentAssetScope::User,
            path: root.join(format!("{native_source_key}.json")),
            allowed_root: root.to_path_buf(),
            precedence: 1,
            writable: true,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 1,
                },
            ],
        };
        if output.emit_initial(source).is_break() {
            break;
        }
    }
}

fn fixture_parse_source_key(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let native_id = request.source.native_source_key.as_str();
    let _terminal_result = output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: native_id,
            resolution_group_key: native_id,
            category: AgentAssetCategory::Skill,
            native_id,
            label: native_id,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Enabled,
                invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
            },
            facts: BTreeMap::new(),
        },
    ));
}

fn fixture_parse_three_assets(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    for native_id in ["first", "second", "third"] {
        if output
            .emit_declaration(parsed_asset(
                request,
                ParsedAssetInput {
                    declaration_key: native_id,
                    resolution_group_key: native_id,
                    category: AgentAssetCategory::Skill,
                    native_id,
                    label: native_id,
                    logical_origin: physical_origin(request.source),
                    declared_state: AgentAssetDeclaredState::Enabled,
                    trust_state: AgentTrustState::Unknown,
                    role: AgentAssetDeclarationRole::Definition,
                    participation: AgentAssetResolutionParticipation::Participates,
                    provided_by: None,
                    action_owner: None,
                    explicitly_affected: Vec::new(),
                    details: AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Enabled,
                        invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
                    },
                    facts: BTreeMap::new(),
                },
            ))
            .is_break()
        {
            return;
        }
    }
}

fn fixture_parse_first_one_second_two(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    fixture_parse_first_one_second_two_in_order(request, output, false);
}

fn fixture_parse_first_one_second_two_reversed(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    fixture_parse_first_one_second_two_in_order(request, output, true);
}

fn fixture_parse_first_one_second_two_in_order(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    reversed: bool,
) {
    let second = request.source.native_source_key == "fixture-second";
    let mut ids = if second {
        ["second-one", "second-two"]
    } else {
        ["first-one", ""]
    };
    if reversed {
        ids.reverse();
    }
    for native_id in ids.into_iter().filter(|value| !value.is_empty()) {
        if output
            .emit_declaration(parsed_asset(
                request,
                ParsedAssetInput {
                    declaration_key: native_id,
                    resolution_group_key: native_id,
                    category: AgentAssetCategory::Skill,
                    native_id,
                    label: native_id,
                    logical_origin: physical_origin(request.source),
                    declared_state: AgentAssetDeclaredState::Enabled,
                    trust_state: AgentTrustState::Unknown,
                    role: crate::models::AgentAssetDeclarationRole::Definition,
                    participation: crate::models::AgentAssetResolutionParticipation::Participates,
                    provided_by: None,
                    action_owner: None,
                    explicitly_affected: Vec::new(),
                    details: AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Enabled,
                        invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
                    },
                    facts: BTreeMap::new(),
                },
            ))
            .is_break()
        {
            break;
        }
    }
}

fn fixture_follow_up_two_children(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    for relative_path in ["file1", "file2"] {
        if output
            .emit_follow_up(
                crate::services::agent_cli::contracts::AgentFollowUpSourceSpec {
                    parent_source_key: request.parent.native_source_key.clone(),
                    target: crate::services::agent_cli::contracts::AgentFollowUpSourceTarget::Descendant {
                        directory_entry_name: "childdir".to_owned(),
                        relative_path: PathBuf::from(relative_path),
                    },
                    native_source_key: format!("fixture-{relative_path}"),
                    label: format!("Fixture {relative_path}"),
                    scope: AgentAssetScope::User,
                    precedence: 1,
                    sensitive: false,
                    source_kind: AgentAssetSourceKind::File,
                    categories: vec![AgentAssetCategory::Skill],
                },
            )
            .is_break()
        {
            break;
        }
    }
}

fn fixture_follow_up_collides_with_parent(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let Some(entry_name) = request.manifest.first().map(|entry| entry.name.clone()) else {
        return;
    };
    let _terminal_result = output.emit_follow_up(
        crate::services::agent_cli::contracts::AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: crate::services::agent_cli::contracts::AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: entry_name,
                relative_path: PathBuf::from("child-config"),
            },
            native_source_key: request.parent.native_source_key.clone(),
            label: "Fixture colliding child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
    );
}

fn fixture_follow_up_one_child(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let Some(entry_name) = request.manifest.first().map(|entry| entry.name.clone()) else {
        return;
    };
    let _terminal_result = output.emit_follow_up(
        crate::services::agent_cli::contracts::AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: crate::services::agent_cli::contracts::AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: entry_name,
                relative_path: PathBuf::from("file"),
            },
            native_source_key: "fixture-child".to_owned(),
            label: "Fixture child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: vec![AgentAssetCategory::Skill],
        },
    );
}

fn fixture_follow_up_chain(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let Some(entry_name) = request.manifest.first().map(|entry| entry.name.clone()) else {
        return;
    };
    let (native_source_key, relative_path) = match request.parent.native_source_key.as_str() {
        "fixture-parent" => ("fixture-child", "child-config"),
        "fixture-child" => ("fixture-grandchild", "grandchild-config"),
        _ => return,
    };
    let _terminal_result = output.emit_follow_up(
        crate::services::agent_cli::contracts::AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: crate::services::agent_cli::contracts::AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: entry_name,
                relative_path: PathBuf::from(relative_path),
            },
            native_source_key: native_source_key.to_owned(),
            label: native_source_key.to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: vec![AgentAssetCategory::Skill],
        },
    );
}

fn fixture_parse(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    for native_id in ["one", "two"] {
        if output
            .emit_declaration(parsed_asset(
                request,
                ParsedAssetInput {
                    declaration_key: native_id,
                    resolution_group_key: native_id,
                    category: AgentAssetCategory::Skill,
                    native_id,
                    label: native_id,
                    logical_origin: physical_origin(request.source),
                    declared_state: AgentAssetDeclaredState::Enabled,
                    trust_state: AgentTrustState::Unknown,
                    role: crate::models::AgentAssetDeclarationRole::Definition,
                    participation: crate::models::AgentAssetResolutionParticipation::Participates,
                    provided_by: None,
                    action_owner: None,
                    explicitly_affected: Vec::new(),
                    details: AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Enabled,
                        invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
                    },
                    facts: BTreeMap::new(),
                },
            ))
            .is_break()
        {
            break;
        }
    }
}

fn fixture_resolve(
    request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    for declaration in request.declarations {
        if output
            .emit_draft(
                fixture_draft(&request, declaration).expect("complete independent fixture input"),
            )
            .is_break()
        {
            break;
        }
    }
}

fn fixture_resolve_reversed(
    request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    for declaration in request.declarations.iter().rev() {
        if output
            .emit_draft(
                fixture_draft(&request, declaration).expect("complete independent fixture input"),
            )
            .is_break()
        {
            break;
        }
    }
}

fn fixture_resolve_second_context(
    request: crate::services::agent_cli::contracts::AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    for declaration in request.declarations {
        if output
            .emit_draft(
                fixture_draft(&request, declaration).expect("complete independent fixture input"),
            )
            .is_break()
        {
            break;
        }
    }
}

fn fixture_definition(
    discover_sources: crate::services::agent_cli::contracts::AgentSourceDiscovery,
    follow_up: Option<crate::services::agent_cli::contracts::AgentFollowUpSourceDiscovery>,
) -> AgentCliDefinition {
    fixture_definition_for_kind(
        AgentCliKind::Codex,
        discover_sources,
        follow_up,
        fixture_parse,
        fixture_resolve,
    )
}

fn fixture_definition_with_pipeline(
    discover_sources: crate::services::agent_cli::contracts::AgentSourceDiscovery,
    follow_up: Option<crate::services::agent_cli::contracts::AgentFollowUpSourceDiscovery>,
    parse: crate::services::agent_cli::contracts::AgentAssetParser,
    resolve: crate::services::agent_cli::contracts::AgentAssetResolver,
) -> AgentCliDefinition {
    fixture_definition_for_kind(
        AgentCliKind::Codex,
        discover_sources,
        follow_up,
        parse,
        resolve,
    )
}

fn fixture_suppression_definition() -> AgentCliDefinition {
    fixture_definition_with_pipeline(
        fixture_file_source,
        None,
        fixture_suppression_parse,
        fixture_empty_resolve,
    )
}

fn fixture_relationship_definition() -> AgentCliDefinition {
    fixture_definition_with_pipeline(
        fixture_relationship_source,
        None,
        fixture_relationship_parse,
        fixture_resolve,
    )
}

fn fixture_definition_for_kind(
    kind: AgentCliKind,
    discover_sources: crate::services::agent_cli::contracts::AgentSourceDiscovery,
    follow_up: Option<crate::services::agent_cli::contracts::AgentFollowUpSourceDiscovery>,
    parse: crate::services::agent_cli::contracts::AgentAssetParser,
    resolve: crate::services::agent_cli::contracts::AgentAssetResolver,
) -> AgentCliDefinition {
    let template = definition(kind);
    AgentCliDefinition {
        kind,
        label: "Fixture Agent",
        executable: template.executable,
        session_name_hint: template.session_name_hint,
        additional_env_keys: template.additional_env_keys,
        home_scan: template.home_scan,
        invalid_path_reason: template.invalid_path_reason,
        require_version_substring: template.require_version_substring,
        endpoint: EndpointAdapter::new(|value| value.to_owned()),
        temporary_launch: None,
        sessions: None,
        liveness: None,
        default_config: None,
        environment: EnvironmentAdapter::with_pipeline(
            fixture_contexts,
            discover_sources,
            follow_up,
            "fixture-agent",
            parse,
            resolve,
            fixture_assessor,
        ),
        ..*template
    }
}

fn build_codex_fixture(
    name: &str,
    system_config: Option<&[u8]>,
    config: &[u8],
    workspace_config: Option<&[u8]>,
    requirements: Option<&[u8]>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
) -> (
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    let root = test_root(name);
    let workspace = root.join("workspace");
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::create_dir_all(workspace.join(".codex")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    if name == "passive-no-secret" {
        fs::write(root.join("filesystem-sentinel"), b"untouched").unwrap();
    }
    let expand_workspace = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .replace(
                "__WORKSPACE__",
                serde_json::to_string(&workspace.to_string_lossy())
                    .unwrap()
                    .trim_matches('"'),
            )
            .into_bytes()
    };
    let mut files = vec![("config", expand_workspace(config))];
    if let Some(bytes) = system_config {
        files.push(("system-config", expand_workspace(bytes)));
    }
    if let Some(bytes) = workspace_config {
        files.push(("workspace-config", expand_workspace(bytes)));
    }
    if let Some(bytes) = requirements {
        files.push(("system-requirements", expand_workspace(bytes)));
    }
    let (snapshots, state) = CodexFixtureSnapshotPort::new(files, blocked);
    let template = definition(AgentCliKind::Codex);
    let codex_definition = AgentCliDefinition {
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
        environment: template.environment,
        ..*template
    };
    let definitions = [codex_definition];
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: Some(&workspace),
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    (root, inventory, state)
}

struct CodexFixtureBuildInput<'a> {
    system_config: Option<&'a [u8]>,
    config: &'a [u8],
    workspace_config: Option<&'a [u8]>,
    requirements: Option<&'a [u8]>,
    blocked: Vec<(&'static str, AgentAssetDiagnostic)>,
    installations: &'a dyn InstallationDiscoveryPort,
    checkpoint_probe: Option<Arc<dyn InventoryCheckpointProbe>>,
}

fn build_codex_inventory_at_root(
    root: &Path,
    system_config: Option<&[u8]>,
    config: &[u8],
    workspace_config: Option<&[u8]>,
    requirements: Option<&[u8]>,
    blocked: impl IntoIterator<Item = (&'static str, AgentAssetDiagnostic)>,
    installations: &dyn InstallationDiscoveryPort,
) -> (
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    build_codex_inventory_at_root_with_probe(
        root,
        CodexFixtureBuildInput {
            system_config,
            config,
            workspace_config,
            requirements,
            blocked: blocked.into_iter().collect(),
            installations,
            checkpoint_probe: None,
        },
    )
}

fn build_codex_inventory_at_root_with_probe(
    root: &Path,
    input: CodexFixtureBuildInput<'_>,
) -> (
    crate::models::AgentEnvironmentInventory,
    Arc<Mutex<FakeSnapshotState>>,
) {
    let workspace = root.join("workspace");
    let expand_workspace = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .replace(
                "__WORKSPACE__",
                serde_json::to_string(&workspace.to_string_lossy())
                    .unwrap()
                    .trim_matches('"'),
            )
            .into_bytes()
    };
    let mut files = vec![("config", expand_workspace(input.config))];
    if let Some(bytes) = input.system_config {
        files.push(("system-config", expand_workspace(bytes)));
    }
    if let Some(bytes) = input.workspace_config {
        files.push(("workspace-config", expand_workspace(bytes)));
    }
    if let Some(bytes) = input.requirements {
        files.push(("system-requirements", expand_workspace(bytes)));
    }
    let (snapshots, state) = CodexFixtureSnapshotPort::new(files, input.blocked);
    let template = definition(AgentCliKind::Codex);
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
        environment: template.environment,
        ..*template
    }];
    let mut settings = crate::models::AppSettings::default();
    settings.set_agent_cli_path(AgentCliKind::Codex, "/fixture/bin/agent".to_owned());
    let inventory = build_inventory_with(
        InventoryInput {
            home: root,
            workspace: Some(&workspace),
            settings: Some(&settings),
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: input.installations,
            snapshots: &snapshots,
            checkpoint_probe: input.checkpoint_probe,
        },
    )
    .unwrap();
    (inventory, state)
}

fn codex_mcp_declarations<'a>(
    inventory: &'a crate::models::AgentEnvironmentInventory,
    native_id: &str,
) -> Vec<&'a crate::models::AgentAssetDeclaration> {
    inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Mcp && declaration.native_id == native_id
        })
        .collect()
}

fn codex_mcp_asset<'a>(
    inventory: &'a crate::models::AgentEnvironmentInventory,
    native_id: &str,
) -> &'a crate::models::AgentAssetRecord {
    inventory
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == native_id)
        .expect("Codex fixture MCP asset")
}

const CODEX_FIXTURE_CONFIG_SOURCES: &[(&str, AgentAssetScope)] = &[
    ("config", AgentAssetScope::User),
    #[cfg(unix)]
    ("system-config", AgentAssetScope::System),
    ("workspace-config", AgentAssetScope::Workspace),
];

fn assert_codex_feature_policy_payloads(
    declarations: &[crate::services::agent_cli::contracts::ParsedAgentAsset],
    config_sources: &[(&str, AgentAssetScope)],
) {
    let features = declarations
        .iter()
        .filter(|declaration| {
            matches!(
                &declaration.native_payload,
                AgentAssetNativePayload::CodexHook(
                    crate::services::agent_cli::codex::CodexHookPayload::FeaturePolicy { .. }
                )
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(features.len(), config_sources.len());
    assert_eq!(
        features
            .iter()
            .map(|feature| (feature.source_key.as_str(), feature.logical_origin.scope))
            .collect::<BTreeSet<_>>(),
        config_sources.iter().copied().collect::<BTreeSet<_>>()
    );
    for feature in features {
        assert_eq!(feature.category, AgentAssetCategory::Hook);
        assert_eq!(feature.role, AgentAssetDeclarationRole::PolicyOverlay);
        assert_eq!(feature.declaration_key, "codex-hook-feature-policy");
        assert_eq!(feature.resolution_group_key, feature.declaration_key);
        assert_eq!(feature.native_id, feature.declaration_key);
        assert_eq!(feature.declared_state, AgentAssetDeclaredState::Unknown);
        assert_eq!(feature.trust_state, AgentTrustState::Unknown);
        assert!(feature.provided_by.is_none());
        assert!(feature.action_owner.is_none());
        assert!(feature.explicitly_affected.is_empty());
    }
}

fn assert_codex_empty_config_policy_payloads(
    declarations: &[crate::services::agent_cli::contracts::ParsedAgentAsset],
    config_sources: &[(&str, AgentAssetScope)],
) {
    assert_codex_feature_policy_payloads(declarations, config_sources);
    let native_controls = declarations
        .iter()
        .filter(|declaration| {
            matches!(
                &declaration.native_payload,
                AgentAssetNativePayload::CodexAsset(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(native_controls.len(), config_sources.len() + 1);
    let skill_rules = native_controls
        .iter()
        .filter(|declaration| {
            matches!(
                &declaration.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillRules { .. })
            )
        })
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(skill_rules.len(), 1);
    let skill_rules = skill_rules[0];
    assert_eq!(skill_rules.source_key, "config");
    assert_eq!(skill_rules.logical_origin.scope, AgentAssetScope::User);
    assert_eq!(skill_rules.category, AgentAssetCategory::Skill);
    assert_eq!(skill_rules.declaration_key, "skills.config");
    assert_eq!(skill_rules.native_id, "__balancehub_codex_skill_rules__");
    assert!(matches!(
        &skill_rules.native_payload,
        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillRules { rules, available: true })
            if rules.is_empty()
    ));
    for &(source_key, scope) in config_sources {
        let roots = native_controls
            .iter()
            .filter(|declaration| {
                declaration.source_key == source_key
                    && matches!(
                        &declaration.native_payload,
                        AgentAssetNativePayload::CodexAsset(
                            CodexAssetPayload::PluginConfigRoot { .. }
                        )
                    )
            })
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(roots.len(), 1, "{source_key}");
        let root = roots[0];
        assert_eq!(root.logical_origin.scope, scope);
        assert_eq!(root.category, AgentAssetCategory::Plugin);
        assert_eq!(root.declaration_key, "plugins:root");
        assert_eq!(root.native_id, "__balancehub_codex_plugin_config__");
        assert!(matches!(
            &root.native_payload,
            AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfigRoot {
                entry_count: 0,
                valid: true
            })
        ));
    }
    for declaration in native_controls {
        assert_eq!(declaration.role, AgentAssetDeclarationRole::PolicyOverlay);
        assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Unknown);
        assert!(declaration.provided_by.is_none());
        assert!(declaration.action_owner.is_none());
        assert!(declaration.explicitly_affected.is_empty());
    }
}

fn assert_codex_empty_config_policy_declarations(
    inventory: &crate::models::AgentEnvironmentInventory,
) {
    let config_sources = CODEX_FIXTURE_CONFIG_SOURCES
        .iter()
        .map(|&(_, scope)| {
            let sources = inventory
                .sources
                .iter()
                .filter(|source| {
                    source.scope == scope
                        && Path::new(&source.path)
                            .file_name()
                            .is_some_and(|name| name == "config.toml")
                })
                .collect::<Vec<_>>();
            assert_eq!(sources.len(), 1, "{scope:?}");
            sources[0]
        })
        .collect::<Vec<_>>();
    let skill_rules = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skill_rules.len(), 1);
    assert_eq!(skill_rules[0].declaration_key, "skills.config");
    assert_eq!(skill_rules[0].native_id, "__balancehub_codex_skill_rules__");
    assert_eq!(skill_rules[0].scope, AgentAssetScope::User);
    assert_eq!(
        skill_rules[0].source_id,
        config_sources
            .iter()
            .find(|source| source.scope == AgentAssetScope::User)
            .unwrap()
            .id
    );
    let plugin_roots = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Plugin)
        .collect::<Vec<_>>();
    assert_eq!(plugin_roots.len(), config_sources.len());
    for root in &plugin_roots {
        assert_eq!(root.declaration_key, "plugins:root");
        assert_eq!(root.native_id, "__balancehub_codex_plugin_config__");
    }
    assert_eq!(
        plugin_roots
            .iter()
            .map(|root| (root.source_id.as_str(), root.scope))
            .collect::<BTreeSet<_>>(),
        config_sources
            .iter()
            .map(|source| (source.id.as_str(), source.scope))
            .collect::<BTreeSet<_>>()
    );
    let feature_roots = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.declaration_key == "codex-hook-feature-policy")
        .collect::<Vec<_>>();
    assert_eq!(feature_roots.len(), config_sources.len());
    assert_eq!(
        feature_roots
            .iter()
            .map(|root| (root.source_id.as_str(), root.scope))
            .collect::<BTreeSet<_>>(),
        config_sources
            .iter()
            .map(|source| (source.id.as_str(), source.scope))
            .collect::<BTreeSet<_>>()
    );
    for root in &feature_roots {
        assert_eq!(root.native_kind, AgentAssetCategory::Hook);
        assert_eq!(root.native_id, "codex-hook-feature-policy");
        assert!(inventory.assets.iter().all(|asset| {
            !asset.represented_declaration_ids.contains(&root.id)
                && !asset.resolution.contributor_ids.contains(&root.id)
        }));
    }
    for declaration in skill_rules
        .into_iter()
        .chain(plugin_roots)
        .chain(feature_roots)
    {
        assert_eq!(declaration.role, AgentAssetDeclarationRole::PolicyOverlay);
        assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Unknown);
        assert!(declaration.provided_by.is_none());
        assert!(declaration.action_owner.is_none());
        assert!(declaration.explicitly_affected.is_empty());
    }
    assert!(inventory.assets.iter().all(|asset| !matches!(
        asset.category,
        AgentAssetCategory::Skill | AgentAssetCategory::Plugin
    )));
}

#[cfg(unix)]
#[test]
fn codex_deep_merges_same_id() {
    let (root, inventory, state) = build_codex_fixture(
        "deep-merge",
        Some(b"[mcp_servers.same]\ncommand = \"system\"\n"),
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\nargs = [\"base\"]\n[ mcp_servers.same.tools.search ]\napproval_mode = \"auto\"\n[mcp_servers.independent]\nurl = \"https://independent.invalid\"\n",
        Some(b"[mcp_servers.same]\nenabled = true\nargs = [\"workspace\"]\n[mcp_servers.same.tools.search]\noutput_token_limit = 10\n"),
        None,
        [],
    );
    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Trusted
    );
    let declarations = codex_mcp_declarations(&inventory, "same");
    assert_eq!(declarations.len(), 3);
    let independent = codex_mcp_asset(&inventory, "independent");
    assert_eq!(
        independent.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(independent.represented_declaration_ids.len(), 1);
    assert_eq!(independent.resolution.contributor_ids.len(), 1);
    let asset = codex_mcp_asset(&inventory, "same");
    assert_eq!(
        asset.resolution.relation,
        AgentAssetResolutionRelation::Merged
    );
    assert_eq!(asset.represented_declaration_ids.len(), 3);
    assert_eq!(asset.resolution.contributor_ids.len(), 3);
    let declaration_source_ids = declarations
        .iter()
        .map(|declaration| declaration.source_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(declaration_source_ids.len(), 3);
    assert_eq!(
        asset
            .source_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        declaration_source_ids
    );
    assert!(inventory
        .sources
        .iter()
        .find(|source| source.id == asset.inspection_source_id)
        .is_some_and(|source| source
            .path
            .replace('\\', "/")
            .ends_with("workspace/.codex/config.toml")));
    let state = state.lock().expect("deep merge snapshot state lock");
    assert_eq!(
        state
            .native_source_keys
            .iter()
            .filter(|key| *key == "config" || *key == "system-config")
            .count(),
        2
    );
    assert_eq!(
        state
            .native_source_keys
            .iter()
            .filter(|key| *key == "workspace-config")
            .count(),
        1
    );
    drop(state);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_enabled_overlay_and_default() {
    let (omitted_root, omitted, _) = build_codex_fixture(
        "enabled-default-omitted",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        None,
        None,
        [],
    );
    let omitted_asset = codex_mcp_asset(&omitted, "same");
    assert_eq!(omitted_asset.declared_state, AgentAssetState::Enabled);
    assert_eq!(
        omitted_asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(omitted_asset.represented_declaration_ids.len(), 1);
    assert_eq!(omitted_asset.resolution.contributor_ids.len(), 1);
    let _ = fs::remove_dir_all(omitted_root);

    let (root, inventory, _) = build_codex_fixture(
        "enabled-overlay",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        Some(b"[mcp_servers.same]\nenabled = false\n"),
        None,
        [],
    );
    let declarations = codex_mcp_declarations(&inventory, "same");
    assert_eq!(declarations.len(), 2);
    assert!(declarations
        .iter()
        .any(|declaration| declaration.role
            == crate::models::AgentAssetDeclarationRole::StateOverlay));
    assert_eq!(
        codex_mcp_asset(&inventory, "same").declared_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        codex_mcp_asset(&inventory, "same").resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        codex_mcp_asset(&inventory, "same")
            .represented_declaration_ids
            .len(),
        2
    );
    assert_eq!(
        codex_mcp_asset(&inventory, "same")
            .resolution
            .contributor_ids
            .len(),
        2
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_id == "same"
                    && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            })
            .count(),
        1
    );
    let _ = fs::remove_dir_all(root);

    let (higher_root, higher, _) = build_codex_fixture(
        "enabled-overlay-higher-true",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\nenabled = false\n",
        Some(b"[mcp_servers.same]\nenabled = true\n"),
        None,
        [],
    );
    let higher_asset = codex_mcp_asset(&higher, "same");
    assert_eq!(higher_asset.declared_state, AgentAssetState::Enabled);
    assert_eq!(
        higher_asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(higher_asset.represented_declaration_ids.len(), 2);
    assert_eq!(higher_asset.resolution.contributor_ids.len(), 2);
    assert_eq!(
        higher
            .declarations
            .iter()
            .filter(|declaration| declaration.native_id == "same")
            .count(),
        2
    );
    let _ = fs::remove_dir_all(higher_root);
}

#[test]
fn codex_trusted_and_suppressed_project() {
    let (trusted_root, trusted, trusted_state) = build_codex_fixture(
        "trusted-project",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        Some(b"[mcp_servers.same]\ncommand = \"workspace\"\n"),
        None,
        [],
    );
    assert_eq!(trusted.contexts[0].trust_context, AgentTrustState::Trusted);
    assert_eq!(codex_mcp_declarations(&trusted, "same").len(), 2);
    let trusted_asset = trusted
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == "same")
        .unwrap_or_else(|| {
            panic!(
                "trusted Codex MCP missing: diagnostics={:?} declarations={:?}",
                trusted.diagnostics,
                codex_mcp_declarations(&trusted, "same")
            )
        });
    assert_eq!(
        trusted_asset.resolution.relation,
        AgentAssetResolutionRelation::Merged
    );
    assert_eq!(
        codex_mcp_asset(&trusted, "same")
            .represented_declaration_ids
            .len(),
        2
    );
    assert_eq!(
        codex_mcp_asset(&trusted, "same")
            .resolution
            .contributor_ids
            .len(),
        2
    );
    assert!(!codex_mcp_asset(&trusted, "same")
        .revision
        .identity
        .is_empty());
    let trusted_workspace_source_id = trusted
        .sources
        .iter()
        .find(|source| source.label == "Codex 工作区配置")
        .map(|source| source.id.clone())
        .expect("trusted workspace source");
    assert_eq!(
        codex_mcp_asset(&trusted, "same").inspection_source_id,
        trusted_workspace_source_id
    );
    let trusted_state = trusted_state.lock().expect("trusted snapshot state lock");
    assert_eq!(
        trusted_state
            .native_source_keys
            .iter()
            .filter(|key| *key == "config")
            .count(),
        1
    );
    drop(trusted_state);
    let _ = fs::remove_dir_all(trusted_root);

    let (untrusted_root, untrusted, untrusted_state) = build_codex_fixture(
        "untrusted-project",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"untrusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        Some(b"[mcp_servers.same]\ncommand = \"workspace\"\n"),
        None,
        [],
    );
    assert_eq!(
        untrusted.contexts[0].trust_context,
        AgentTrustState::Untrusted
    );
    let declarations = codex_mcp_declarations(&untrusted, "same");
    assert_eq!(declarations.len(), 2);
    assert!(declarations.iter().any(|declaration| matches!(
        declaration.participation,
        crate::models::AgentAssetResolutionParticipation::Suppressed { .. }
    )));
    let untrusted_asset = untrusted
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == "same")
        .unwrap_or_else(|| {
            panic!(
                "untrusted Codex MCP missing: diagnostics={:?} declarations={declarations:?}",
                untrusted.diagnostics
            )
        });
    assert_eq!(
        untrusted_asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(untrusted
        .declarations
        .iter()
        .filter(|declaration| declaration.native_id == "same")
        .flat_map(|declaration| &declaration.diagnostics)
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DeclarationSuppressed { .. }
        )));
    let untrusted_config_source_id = untrusted
        .sources
        .iter()
        .find(|source| source.label == "Codex 配置")
        .map(|source| source.id.clone())
        .expect("untrusted user source");
    assert_eq!(
        codex_mcp_asset(&untrusted, "same").inspection_source_id,
        untrusted_config_source_id
    );
    assert!(codex_mcp_asset(&untrusted, "same")
        .revision
        .size_bytes
        .is_some());
    let untrusted_state = untrusted_state
        .lock()
        .expect("untrusted snapshot state lock");
    assert_eq!(
        untrusted_state
            .native_source_keys
            .iter()
            .filter(|key| *key == "config")
            .count(),
        1
    );
    drop(untrusted_state);
    let _ = fs::remove_dir_all(untrusted_root);

    let (unknown_root, unknown, unknown_state) = build_codex_fixture(
        "unknown-project",
        None,
        b"[mcp_servers.same]\ncommand = \"runner\"\n",
        Some(b"[mcp_servers.same]\ncommand = \"workspace\"\n"),
        None,
        [],
    );
    assert_eq!(unknown.contexts[0].trust_context, AgentTrustState::Unknown);
    assert!(codex_mcp_declarations(&unknown, "same")
        .iter()
        .any(|declaration| {
            matches!(
                declaration.participation,
                crate::models::AgentAssetResolutionParticipation::Suppressed { .. }
            )
        }));
    let unknown_user_source_id = unknown
        .sources
        .iter()
        .find(|source| source.label == "Codex 配置")
        .map(|source| source.id.clone())
        .expect("unknown user source");
    let unknown_workspace_source = unknown
        .sources
        .iter()
        .find(|source| source.label == "Codex 工作区配置")
        .expect("unknown workspace source");
    assert!(!unknown_workspace_source.revision.is_missing);
    let unknown_declarations = codex_mcp_declarations(&unknown, "same");
    let unknown_user_declaration = unknown_declarations
        .iter()
        .find(|declaration| declaration.source_id == unknown_user_source_id)
        .expect("unknown user declaration");
    let unknown_workspace_declaration = unknown_declarations
        .iter()
        .find(|declaration| declaration.source_id == unknown_workspace_source.id)
        .expect("unknown workspace declaration");
    assert!(matches!(
        unknown_workspace_declaration.participation,
        crate::models::AgentAssetResolutionParticipation::Suppressed { .. }
    ));
    assert!(unknown_workspace_declaration
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DeclarationSuppressed { .. }
        )));
    let unknown_asset = codex_mcp_asset(&unknown, "same");
    assert_eq!(
        unknown_asset.resolution.contributor_ids,
        vec![unknown_user_declaration.id.clone()]
    );
    assert_eq!(
        unknown_asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(unknown_asset.inspection_source_id, unknown_user_source_id);
    assert_eq!(
        unknown_asset
            .represented_declaration_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            unknown_user_declaration.id.clone(),
            unknown_workspace_declaration.id.clone(),
        ])
    );
    let unknown_state = unknown_state.lock().expect("unknown snapshot state lock");
    assert_eq!(
        unknown_state
            .native_source_keys
            .iter()
            .filter(|key| *key == "config")
            .count(),
        1
    );
    assert_eq!(
        unknown_state
            .native_source_keys
            .iter()
            .filter(|key| *key == "workspace-config")
            .count(),
        1
    );
    assert_eq!(unknown_state.opens.len(), 2);
    assert_eq!(unknown_state.charges.len(), 2);
    drop(unknown_state);
    let _ = fs::remove_dir_all(unknown_root);

    let (ambiguous_root, ambiguous, _) = build_codex_fixture(
        "ambiguous-missing-trust-level",
        None,
        b"[projects.\"__WORKSPACE__\"]\n[projects.\"__WORKSPACE__/./\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        Some(b"[mcp_servers.same]\ncommand = \"workspace\"\n"),
        None,
        [],
    );
    assert_eq!(
        ambiguous.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    assert!(codex_mcp_declarations(&ambiguous, "same")
        .iter()
        .any(|declaration| matches!(
            declaration.participation,
            crate::models::AgentAssetResolutionParticipation::Suppressed { .. }
        )));
    let _ = fs::remove_dir_all(ambiguous_root);

    let (policy_only_root, policy_only, _) = build_codex_fixture(
        "policy-only-suppressed-project",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"untrusted\"\n",
        Some(b"[mcp_servers.same]\ncommand = \"workspace\"\n"),
        Some(b"[mcp_servers.same.identity]\ncommand = \"workspace\"\n"),
        [],
    );
    let policy_only_asset = codex_mcp_asset(&policy_only, "same");
    assert_eq!(
        policy_only_asset.resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert!(policy_only_asset.resolution.contributor_ids.is_empty());
    assert_eq!(
        policy_only_asset.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert_eq!(policy_only_asset.effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        policy_only_asset.details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    assert_eq!(
        policy_only
            .declarations
            .iter()
            .filter(|declaration| declaration.native_id == "same")
            .count(),
        2
    );
    assert!(!policy_only_asset.revision.identity.is_empty());
    assert!(policy_only_asset.source_ids.iter().all(|source_id| {
        policy_only
            .sources
            .iter()
            .any(|source| &source.id == source_id)
    }));
    assert_eq!(
        policy_only
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_id == "same"
                    && declaration.role == crate::models::AgentAssetDeclarationRole::PolicyOverlay
            })
            .count(),
        1
    );
    let policy_only_ids = policy_only
        .declarations
        .iter()
        .filter(|declaration| declaration.native_id == "same")
        .map(|declaration| declaration.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(policy_only_ids.len(), 2);
    assert_eq!(
        policy_only_asset
            .represented_declaration_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        policy_only_ids
    );
    assert!(policy_only_asset.resolution.contributor_ids.is_empty());
    let _ = fs::remove_dir_all(policy_only_root);
}

#[cfg(unix)]
#[test]
fn codex_authority_diagnostic_rebinds_to_final_source_id() {
    let (root, inventory, _) = build_codex_fixture(
        "authority-final-id",
        None,
        b"[mcp_servers.same]\ncommand = \"runner\"\n",
        None,
        None,
        [(
            "system-config",
            AgentAssetDiagnostic::ReadFailed {
                source_id: "provisional-source-id".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
            },
        )],
    );
    let source = inventory
        .sources
        .iter()
        .find(|source| {
            source.scope == AgentAssetScope::System && source.path.ends_with("config.toml")
        })
        .expect("system authority source");
    assert!(source.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::ReadFailed { source_id, .. } if source_id == &source.id
    )));
    assert!(inventory
        .sources
        .iter()
        .flat_map(|source| &source.diagnostics)
        .all(|diagnostic| !matches!(
            diagnostic,
            AgentAssetDiagnostic::ReadFailed { source_id, .. }
                if source_id == "provisional-source-id"
        )));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_different_ids_are_additive_set() {
    let (root, inventory, _) = build_codex_fixture(
        "additive-ids",
        None,
        b"[mcp_servers.first]\ncommand = \"one\"\n[mcp_servers.second]\nurl = \"https://two.invalid\"\n",
        None,
        None,
        [],
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Mcp)
            .count(),
        2
    );
    let first = codex_mcp_asset(&inventory, "first");
    let second = codex_mcp_asset(&inventory, "second");
    assert_ne!(first.stable_id, second.stable_id);
    assert_eq!(
        first.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        second.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(first.represented_declaration_ids.len(), 1);
    assert_eq!(second.represented_declaration_ids.len(), 1);
    assert_eq!(first.resolution.contributor_ids.len(), 1);
    assert_eq!(second.resolution.contributor_ids.len(), 1);
    assert_eq!(first.source_ids.len(), 1);
    assert_eq!(second.source_ids.len(), 1);
    assert_eq!(first.context_id, second.context_id);
    let (rerun, _) = build_codex_inventory_at_root(
        &root,
        None,
        b"[mcp_servers.second]\nurl = \"https://two.invalid\"\n[mcp_servers.first]\ncommand = \"one\"\n",
        None,
        None,
        [],
        &RealInstallationDiscoveryPort,
    );
    let original_order = inventory
        .assets
        .iter()
        .map(|asset| (asset.native_id.clone(), asset.stable_id.clone()))
        .collect::<Vec<_>>();
    let rerun_order = rerun
        .assets
        .iter()
        .map(|asset| (asset.native_id.clone(), asset.stable_id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(rerun_order, original_order);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn codex_lists_replace_and_deny_wins() {
    let (root, inventory, _) = build_codex_fixture(
        "lists-replace",
        Some(
            b"[mcp_servers.same]\ncommand = \"runner\"\nenabled_tools = [\"a\", \"b\"]\ndisabled_tools = [\"x\"]\n[mcp_servers.same.tools.search]\napproval_mode = \"auto\"\n",
        ),
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\nenabled_tools = [\"b\", \"c\"]\ndisabled_tools = [\"b\"]\n[mcp_servers.same.tools.search]\noutput_token_limit = 10\n",
        None,
        None,
        [],
    );
    let asset = codex_mcp_asset(&inventory, "same");
    assert_eq!(
        asset.resolution.relation,
        AgentAssetResolutionRelation::Merged
    );
    let serialized = serde_json::to_string(&inventory).unwrap();
    assert!(!serialized.contains("enabled_tools"));
    assert!(!serialized.contains("output_token_limit"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_requirements_declaration_ref() {
    let (root, inventory, _) = build_codex_fixture(
        "requirements-declaration",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\nargs = [\"actual\"]\n",
        None,
        Some(b"[mcp_servers.same.identity]\ncommand = { executable = \"runner\", args = [] }\n"),
        [],
    );
    let asset = codex_mcp_asset(&inventory, "same");
    assert_eq!(
        asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        asset.resolution.terminal,
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked)
    );
    assert_eq!(asset.effective_state, AgentAssetState::Blocked);
    assert!(matches!(
        asset.details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
            ..
        }
    ));
    let policy_id = inventory
        .declarations
        .iter()
        .find(|declaration| {
            declaration.native_id == "same"
                && declaration.role == crate::models::AgentAssetDeclarationRole::PolicyOverlay
        })
        .map(|declaration| declaration.id.clone())
        .expect("requirements policy declaration");
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_id == "same")
            .count(),
        2
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.native_id == "same")
            .count(),
        1
    );
    let declaration_ids = inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_id == "same")
        .map(|declaration| declaration.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(declaration_ids.len(), 2);
    assert_eq!(
        asset
            .represented_declaration_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        declaration_ids
    );
    assert_eq!(
        asset
            .resolution
            .contributor_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        declaration_ids
    );
    assert!(asset
        .represented_declaration_ids
        .iter()
        .any(|id| id == &policy_id));
    assert!(asset
        .resolution
        .contributor_ids
        .iter()
        .any(|id| id == &policy_id));
    assert!(inventory
        .assets
        .iter()
        .all(|asset| asset.category != AgentAssetCategory::Plugin));
    assert!(inventory.assets.iter().all(|asset| {
        asset.relationships.provided_by.is_none()
            && asset.relationships.action_owner.is_none()
            && asset.relationships.affected_asset_ids.is_empty()
    }));
    assert!(asset.represented_declaration_ids.iter().all(|id| inventory
        .declarations
        .iter()
        .any(|declaration| &declaration.id == id)));
    assert!(asset.resolution.contributor_ids.iter().all(|id| inventory
        .declarations
        .iter()
        .any(|declaration| &declaration.id == id)));
    assert!(asset
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::PolicyBlocked)));
    assert_eq!(
        asset.resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration {
            declaration_id: policy_id
        })
    );
    let _ = fs::remove_dir_all(root);

    let (malformed_root, malformed, _) = build_codex_fixture(
        "requirements-declaration-malformed-config",
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\nenabled = \"invalid\"\n",
        None,
        Some(b"[mcp_servers.same.identity]\ncommand = \"runner\"\n"),
        [],
    );
    let malformed_asset = codex_mcp_asset(&malformed, "same");
    assert_eq!(
        malformed_asset.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(malformed_asset.effective_state, AgentAssetState::Unknown);
    assert_eq!(malformed_asset.resolution.contributor_ids.len(), 2);
    let malformed_ids = malformed
        .declarations
        .iter()
        .filter(|declaration| declaration.native_id == "same")
        .map(|declaration| declaration.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(malformed_ids.len(), 2);
    assert_eq!(
        malformed_asset
            .resolution
            .contributor_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        malformed_ids
    );
    assert_eq!(
        malformed_asset
            .represented_declaration_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        malformed_ids
    );
    assert!(malformed
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
    assert!(malformed_asset
        .resolution
        .diagnostics
        .iter()
        .all(|diagnostic| !matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
    let _ = fs::remove_dir_all(malformed_root);
}

#[test]
fn codex_requirements_source_ref() {
    for (name, requirements) in [
        (
            "requirements-source-missing-id",
            b"[mcp_servers.other.identity]\ncommand = \"other\"\n".as_slice(),
        ),
        (
            "requirements-source-empty",
            b"mcp_servers = {}\n".as_slice(),
        ),
    ] {
        let (root, inventory, _) = build_codex_fixture(
            name,
            None,
            b"[mcp_servers.same]\ncommand = \"runner\"\n",
            None,
            Some(requirements),
            [],
        );
        let asset = codex_mcp_asset(&inventory, "same");
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(
            asset.resolution.terminal,
            Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_eq!(asset.effective_state, AgentAssetState::Blocked);
        assert!(matches!(
            asset.details,
            AgentAssetDetails::Mcp {
                effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
                ..
            }
        ));
        assert!(matches!(
            asset.resolution.control_source,
            Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
                if inventory.sources.iter().any(|source| {
                    source.scope == AgentAssetScope::Managed && &source.id == source_id
                })
        ));
        assert_eq!(asset.represented_declaration_ids.len(), 1);
        assert_eq!(asset.resolution.contributor_ids.len(), 1);
        assert!(asset
            .represented_declaration_ids
            .iter()
            .all(|id| id.starts_with("declaration:")));
        assert!(asset
            .resolution
            .contributor_ids
            .iter()
            .all(|id| id.starts_with("declaration:")));
        assert_eq!(inventory.assets.len(), 1);
        assert!(asset
            .resolution
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidResolution { .. }
            )));
        assert!(asset
            .resolution
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::PolicyBlocked)));
        assert_eq!(
            inventory
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.native_id == "same"
                        && declaration.role
                            == crate::models::AgentAssetDeclarationRole::PolicyOverlay
                })
                .count(),
            0
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn codex_requirements_missing_malformed_blocked() {
    let cases = [
        ("requirements-missing", None, Vec::new()),
        (
            "requirements-malformed",
            Some(b"mcp_servers = \"invalid\"\n".as_slice()),
            Vec::new(),
        ),
        (
            "requirements-blocked",
            None,
            vec![("system-requirements", AgentAssetDiagnostic::PolicyBlocked)],
        ),
    ];
    for (name, requirements, blocked) in cases {
        let (root, inventory, _) = build_codex_fixture(
            name,
            None,
            b"[mcp_servers.same]\ncommand = \"runner\"\n",
            None,
            requirements,
            blocked,
        );
        let asset = codex_mcp_asset(&inventory, "same");
        if name == "requirements-missing" {
            assert_eq!(
                asset.resolution.relation,
                AgentAssetResolutionRelation::Independent
            );
            assert_eq!(asset.resolution.terminal, None);
            assert_eq!(asset.represented_declaration_ids.len(), 1);
            assert_eq!(asset.resolution.contributor_ids.len(), 1);
            assert_eq!(
                inventory
                    .assets
                    .iter()
                    .filter(|value| value.native_id == "same")
                    .count(),
                1
            );
        } else {
            assert_eq!(
                asset.resolution.relation,
                AgentAssetResolutionRelation::Independent
            );
            assert_eq!(
                asset.resolution.terminal,
                Some(crate::models::AgentAssetResolutionTerminal::Unknown)
            );
            assert_eq!(asset.effective_state, AgentAssetState::Unknown);
            assert_ne!(asset.effective_state, AgentAssetState::Enabled);
            assert!(matches!(
                asset.details,
                AgentAssetDetails::Mcp {
                    effective_availability: AgentAssetEffectiveAvailability::Unknown,
                    ..
                }
            ));
            assert!(asset
                .resolution
                .diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic, AgentAssetDiagnostic::PolicyBlocked)));
            assert_eq!(asset.represented_declaration_ids.len(), 1);
            assert_eq!(asset.resolution.contributor_ids.len(), 1);
            assert!(inventory
                .sources
                .iter()
                .filter(|source| source.scope == AgentAssetScope::User)
                .any(|source| !source.revision.is_missing));
            assert!(asset
                .actions
                .iter()
                .filter(|action| matches!(
                    action.action,
                    AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
                ))
                .all(|action| !action.available));
            assert!(inventory
                .sources
                .iter()
                .filter(|source| source.scope == AgentAssetScope::Managed)
                .flat_map(|source| &source.diagnostics)
                .any(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::Malformed { .. } | AgentAssetDiagnostic::PolicyBlocked
                )));
        }
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn codex_passive_and_no_secret() {
    let secret_config = b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.stdio]\ncommand = \"sentinel-command-secret\"\nargs = [\"argument-secret\"]\nenv = { SECRET_ENV = \"environment-secret\" }\nenv_vars = [\"secret-env-name\", { name = \"secret-env-descriptor\", source = \"source-secret\" }]\nunknown_secret = \"unknown-field-secret\"\n[mcp_servers.http]\nurl = \"https://url-secret.invalid\"\nbearer_token = \"bearer-secret\"\nhttp_headers = { Authorization = \"header-secret\" }\nenv_http_headers = { X_Secret = \"env-header-secret\" }\nhttp_headers_helper = \"helper-secret\"\n[ mcp_servers.http.oauth ]\nclient_id = \"oauth-client-secret\"\ncallback_url = \"https://oauth-secret.invalid\"\n";
    let secret_requirements = b"[mcp_servers.stdio.identity]\ncommand = { executable = \"matcher-secret\", args = [{ match = \"regex\", expression = \"secret-regex\" }] }\n";
    let (root, inventory, _) = build_codex_fixture(
        "passive-no-secret",
        None,
        secret_config,
        None,
        Some(secret_requirements),
        [],
    );
    let serialized = serde_json::to_string(&inventory).unwrap();
    assert_eq!(
        fs::read(root.join("filesystem-sentinel")).unwrap(),
        b"untouched"
    );
    let secrets = [
        "sentinel-command-secret",
        "argument-secret",
        "environment-secret",
        "secret-env-name",
        "secret-env-descriptor",
        "source-secret",
        "url-secret.invalid",
        "bearer-secret",
        "header-secret",
        "env-header-secret",
        "helper-secret",
        "oauth-client-secret",
        "oauth-secret.invalid",
        "unknown-field-secret",
        "matcher-secret",
        "secret-regex",
    ];
    for secret in secrets {
        assert!(!serialized.contains(secret), "secret leaked: {secret}");
    }
    assert!(inventory
        .assets
        .iter()
        .flat_map(|asset| asset.actions.iter())
        .filter(|action| matches!(
            action.action,
            AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
        ))
        .all(|action| !action.available));
    for diagnostic in inventory
        .installations
        .iter()
        .flat_map(|installation| installation.diagnostics.iter())
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| source.diagnostics.iter()),
        )
        .chain(
            inventory
                .declarations
                .iter()
                .flat_map(|declaration| declaration.diagnostics.iter()),
        )
        .chain(
            inventory
                .assets
                .iter()
                .flat_map(|asset| asset.diagnostics.iter()),
        )
        .chain(
            inventory
                .assets
                .iter()
                .flat_map(|asset| asset.resolution.diagnostics.iter()),
        )
        .chain(inventory.diagnostics.iter())
    {
        let rendered = format!("{diagnostic:?}");
        for secret in secrets {
            assert!(!rendered.contains(secret), "diagnostic leaked: {secret}");
        }
    }
    for fact in inventory
        .declarations
        .iter()
        .flat_map(|declaration| declaration.evidence.facts.values())
    {
        for secret in secrets {
            assert!(!fact.contains(secret), "fact leaked: {secret}");
        }
    }
    assert!(inventory
        .declarations
        .iter()
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Mcp)
        .all(|declaration| declaration.id.starts_with("declaration:")));
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| declaration.native_id == "stdio"));
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| declaration.native_id == "http"));
    assert!(inventory
        .assets
        .iter()
        .filter(|asset| matches!(asset.native_id.as_str(), "stdio" | "http"))
        .all(|asset| asset.effective_state == AgentAssetState::Unknown));
    assert!(serialized.contains("nativeId"));
    assert!(serialized.contains("mcp"));
    let context = &inventory.contexts[0];
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "config".to_owned(),
        label: "Codex 配置".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join(".codex/config.toml"),
        allowed_root: root.clone(),
        precedence: 10,
        writable: true,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };
    let snapshot = AgentAssetSnapshot::File {
        bytes: secret_config.to_vec(),
        revision: AgentAssetRevision::default(),
    };
    let mut payloads = NativePayloadDebugCollector::default();
    definition(AgentCliKind::Codex).environment().parse(
        AgentAssetParseRequest {
            context,
            source: &source,
            snapshot: &snapshot,
            native_home: None,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut payloads,
    );
    let requirements_source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "system-requirements".to_owned(),
        label: "Codex 管理要求".to_owned(),
        scope: AgentAssetScope::Managed,
        path: root.join("requirements.toml"),
        allowed_root: root.clone(),
        precedence: 100,
        writable: false,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Managed,
                precedence: 100,
            },
        ],
    };
    let requirements_snapshot = AgentAssetSnapshot::File {
        bytes: secret_requirements.to_vec(),
        revision: AgentAssetRevision::default(),
    };
    definition(AgentCliKind::Codex).environment().parse(
        AgentAssetParseRequest {
            context,
            source: &requirements_source,
            snapshot: &requirements_snapshot,
            native_home: None,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut payloads,
    );
    assert!(payloads
        .payload_debug
        .iter()
        .any(|value| value.contains("variant: \"TomlTable\"")));
    assert_eq!(payloads.payload_debug.len(), 8);
    assert_codex_empty_config_policy_payloads(
        &payloads.declarations,
        &[("config", AgentAssetScope::User)],
    );
    assert!(payloads
        .payload_debug
        .iter()
        .any(|value| value.contains("AllowlistRoot")));
    assert!(payloads
        .payload_debug
        .iter()
        .all(|value| secrets.iter().all(|secret| !value.contains(secret))));
    assert!(payloads.facts.is_empty());
    assert!(payloads.diagnostics.iter().all(|diagnostic| {
        let rendered = format!("{diagnostic:?}");
        secrets.iter().all(|secret| !rendered.contains(secret))
    }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_context_and_stable_ids() {
    let root = test_root("stable-ids");
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::create_dir_all(root.join("workspace/.codex")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let first_installation = fixture_installation();
    let (first_port, first_installation_state) =
        FakeInstallationPort::new(vec![first_installation.clone()]);
    let (first, _) = build_codex_inventory_at_root(
        &root,
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        None,
        None,
        [],
        &first_port,
    );
    let context_id = first.contexts[0].id.clone();
    let declaration_id = codex_mcp_declarations(&first, "same")[0].id.clone();
    let asset_id = codex_mcp_asset(&first, "same").stable_id.clone();
    assert!(context_id.starts_with("context:"));
    assert!(declaration_id.starts_with("declaration:"));
    assert!(asset_id.starts_with("asset:"));
    assert!(!context_id.contains("1.0.0"));
    assert!(!asset_id.contains("synthetic:"));
    let mut changed_evidence = first.contexts[0].clone();
    changed_evidence.parser_version = changed_evidence.parser_version.saturating_add(1);
    changed_evidence
        .schema_facts
        .insert("schema".to_owned(), "changed".to_owned());
    assert_eq!(
        context_stable_id(&first.environment, AgentCliKind::Codex, &changed_evidence),
        context_id
    );
    changed_evidence.trust_context = AgentTrustState::Untrusted;
    assert_ne!(
        context_stable_id(&first.environment, AgentCliKind::Codex, &changed_evidence),
        context_id
    );

    let mut changed_installation = first_installation;
    changed_installation.installed_version = Some("2.0.0".to_owned());
    changed_installation.executable_revision = Some("replacement-evidence".to_owned());
    let (second_port, second_installation_state) =
        FakeInstallationPort::new(vec![changed_installation]);
    let (second, _) = build_codex_inventory_at_root(
        &root,
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\n",
        None,
        None,
        [],
        &second_port,
    );
    assert_eq!(first.installations[0].id, second.installations[0].id);
    assert_eq!(first.contexts[0].id, second.contexts[0].id);
    assert_eq!(
        codex_mcp_declarations(&second, "same")[0].id,
        declaration_id
    );
    assert_eq!(codex_mcp_asset(&second, "same").stable_id, asset_id);
    assert_eq!(
        first_installation_state
            .lock()
            .expect("first installation state lock")
            .version_probes,
        1
    );
    assert_eq!(
        second_installation_state
            .lock()
            .expect("second installation state lock")
            .version_probes,
        1
    );

    let (state_port, _) = FakeInstallationPort::new(vec![fixture_installation()]);
    let (state_changed, _) = build_codex_inventory_at_root(
        &root,
        None,
        b"[projects.\"__WORKSPACE__\"]\ntrust_level = \"trusted\"\n[mcp_servers.same]\ncommand = \"runner\"\nenabled = false\n",
        None,
        None,
        [],
        &state_port,
    );
    assert_eq!(state_changed.contexts[0].id, context_id);
    assert_eq!(codex_mcp_asset(&state_changed, "same").stable_id, asset_id);

    let (trust_port, _) = FakeInstallationPort::new(vec![fixture_installation()]);
    let (trust_changed, _) = build_codex_inventory_at_root(
        &root,
        None,
        b"[mcp_servers.same]\ncommand = \"runner\"\n",
        None,
        None,
        [],
        &trust_port,
    );
    assert_ne!(trust_changed.contexts[0].id, context_id);
    assert_ne!(codex_mcp_asset(&trust_changed, "same").stable_id, asset_id);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_plugin_cache_is_physical_only() {
    let root = test_root("plugin-cache-physical");
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::create_dir_all(root.join("workspace/.codex")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let probe = Arc::new(StageCheckpointProbe::new(Arc::new(ManualClock::new())));
    let (inventory, state) = build_codex_inventory_at_root_with_probe(
        &root,
        CodexFixtureBuildInput {
            system_config: None,
            config: b"",
            workspace_config: None,
            requirements: None,
            blocked: Vec::new(),
            installations: &RealInstallationDiscoveryPort,
            checkpoint_probe: Some(probe.clone()),
        },
    );
    let cache_source = inventory
        .sources
        .iter()
        .find(|source| source.path.replace('\\', "/").ends_with("plugins/cache"))
        .expect("physical plugin cache source");
    assert!(cache_source.revision.is_directory);
    assert!(!cache_source.revision.identity.is_empty());
    assert_codex_empty_config_policy_declarations(&inventory);
    assert!(inventory.declarations.iter().all(|declaration| {
        declaration.native_kind != AgentAssetCategory::Plugin
            || declaration.role != AgentAssetDeclarationRole::Definition
    }));
    assert!(inventory
        .assets
        .iter()
        .all(|asset| asset.category != AgentAssetCategory::Plugin));
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.relationships.provided_by.is_some())
            .count(),
        0
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.relationships.action_owner.is_some())
            .count(),
        0
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .flat_map(|asset| asset.relationships.affected_asset_ids.iter())
            .count(),
        0
    );
    assert!(inventory
        .assets
        .iter()
        .filter(|asset| asset.category != AgentAssetCategory::Plugin)
        .all(|asset| {
            asset.relationships.provided_by.is_none()
                && asset.relationships.action_owner.is_none()
                && asset.relationships.affected_asset_ids.is_empty()
        }));
    assert_eq!(
        state
            .lock()
            .expect("plugin cache snapshot state lock")
            .native_source_keys
            .iter()
            .filter(|key| *key == "plugins")
            .count(),
        1
    );
    assert!(definition(AgentCliKind::Codex)
        .environment()
        .has_follow_up_sources());
    assert!(probe.events().iter().any(|event| {
        matches!(
            event,
            InventoryPipelineEvent::FollowUpSourceDiscovery { parent_source_key, .. }
                if parent_source_key == "plugins"
        )
    }));
    assert_eq!(
        inventory
            .sources
            .iter()
            .filter(|source| Path::new(&source.path).starts_with(&cache_source.path))
            .map(|source| source.id.as_str())
            .collect::<Vec<_>>(),
        vec![cache_source.id.as_str()]
    );
    assert!(state
        .lock()
        .unwrap()
        .native_source_keys
        .iter()
        .all(|key| { !key.starts_with("codex-plugin-") && !key.starts_with("codex-skill:") }));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn codex_system_exact_file_boundary() {
    let root = test_root("system-exact-boundary");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_system_boundary_sources,
        None,
        fixture_empty_parse,
        fixture_empty_resolve,
    )];
    let elevated_roots = Arc::new(Mutex::new(Vec::new()));
    let snapshots = BoundarySnapshotPort {
        elevated_roots: Arc::clone(&elevated_roots),
    };
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    assert_eq!(inventory.sources.len(), 6);
    let observations = elevated_roots
        .lock()
        .expect("boundary observations lock")
        .clone();
    assert_eq!(observations.len(), 6);
    assert_eq!(
        observations
            .iter()
            .filter(|(key, elevated)| {
                *elevated && (key == "system-positive" || key == "managed-positive")
            })
            .count(),
        2
    );
    assert!(observations
        .iter()
        .filter(|(_, elevated)| *elevated)
        .all(|(key, _)| { key == "system-positive" || key == "managed-positive" }));
    for key in ["system-positive", "managed-positive"] {
        let source = inventory
            .sources
            .iter()
            .find(|source| source.label == key)
            .expect("positive boundary source");
        assert!(!source.revision.is_missing);
        assert!(!source.revision.is_directory);
    }
    let directory = inventory
        .sources
        .iter()
        .find(|source| source.label == "system-directory")
        .expect("missing directory boundary source");
    assert!(directory.revision.is_missing);
    for key in ["system-writable", "system-root", "system-nested"] {
        let source = inventory
            .sources
            .iter()
            .find(|source| source.label == key)
            .expect("blocked boundary source");
        assert!(source.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::SourceOutsideAllowedRoot { source_id }
                if source_id == &source.id
        )));
    }
    assert!(inventory.assets.is_empty());
    let _ = fs::remove_dir_all(root);
}

fn fixture_two_context_definition() -> AgentCliDefinition {
    fixture_two_context_definition_with_pipeline(
        fixture_parse_with_first_invalid_group,
        fixture_resolve_second_context,
    )
}

fn fixture_two_context_definition_with_pipeline(
    parse: crate::services::agent_cli::contracts::AgentAssetParser,
    resolve: crate::services::agent_cli::contracts::AgentAssetResolver,
) -> AgentCliDefinition {
    let template = definition(AgentCliKind::Codex);
    AgentCliDefinition {
        kind: AgentCliKind::Codex,
        label: "Two Context Fixture Agent",
        executable: template.executable,
        session_name_hint: template.session_name_hint,
        additional_env_keys: template.additional_env_keys,
        home_scan: template.home_scan,
        invalid_path_reason: template.invalid_path_reason,
        require_version_substring: template.require_version_substring,
        endpoint: EndpointAdapter::new(|value| value.to_owned()),
        temporary_launch: None,
        sessions: None,
        liveness: None,
        default_config: None,
        environment: EnvironmentAdapter::with_pipeline(
            fixture_two_contexts,
            fixture_file_source,
            None,
            "two-context-fixture-agent",
            parse,
            resolve,
            fixture_assessor,
        ),
        ..*template
    }
}

fn healthy_two_context_inventory_with_deadline(
    stage: AgentInventoryStage,
    hit: u32,
) -> (
    PathBuf,
    crate::models::AgentEnvironmentInventory,
    Arc<StageCheckpointProbe>,
    Arc<Mutex<FakeSnapshotState>>,
    FakeSnapshotPort,
) {
    let root = test_root("multi-context-deadline");
    fs::create_dir_all(root.join("fixture/first")).unwrap();
    fs::create_dir_all(root.join("fixture/second")).unwrap();
    fs::write(root.join("fixture/first/config.json"), b"{}").unwrap();
    fs::write(root.join("fixture/second/config.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        stage,
        hit,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let definitions = [fixture_two_context_definition_with_pipeline(
        fixture_parse,
        fixture_resolve,
    )];
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file("fixture-file", 2),
        ExpectedSnapshot::file("fixture-file", 2),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();
    (root, inventory, probe, snapshot_state, snapshots)
}

fn ordered_context_ids(inventory: &crate::models::AgentEnvironmentInventory) -> (String, String) {
    assert_eq!(inventory.contexts.len(), 2);
    (
        inventory.contexts[0].id.clone(),
        inventory.contexts[1].id.clone(),
    )
}

fn assert_two_context_snapshot_state(
    inventory: &crate::models::AgentEnvironmentInventory,
    snapshot_state: &Arc<Mutex<FakeSnapshotState>>,
) {
    let state = snapshot_state
        .lock()
        .expect("multi-context snapshot state lock");
    assert_eq!(state.native_source_keys, ["fixture-file", "fixture-file"]);
    assert_eq!(state.attempts.len(), 2);
    assert_eq!(state.opens, state.attempts);
    assert_eq!(
        state.charges,
        state
            .attempts
            .iter()
            .map(|source_id| (source_id.clone(), 2))
            .collect::<Vec<_>>()
    );
    let source_ids = inventory
        .sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(state
        .attempts
        .iter()
        .all(|source_id| source_ids.contains(source_id.as_str())));
}

fn assert_first_context_is_complete(
    inventory: &crate::models::AgentEnvironmentInventory,
    first_context_id: &str,
) {
    assert_eq!(
        inventory
            .sources
            .iter()
            .filter(|source| source.context_id == first_context_id)
            .count(),
        1
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.context_id == first_context_id)
            .count(),
        2
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.context_id == first_context_id)
            .count(),
        2
    );
}

fn assert_two_context_events(
    inventory: &crate::models::AgentEnvironmentInventory,
    actual: Vec<InventoryPipelineEvent>,
    second_context_events: Vec<InventoryPipelineEvent>,
) {
    let (first, second) = ordered_context_ids(inventory);
    let mut expected = vec![
        InventoryPipelineEvent::ContextDiscovery {
            agent_kind: AgentCliKind::Codex,
        },
        InventoryPipelineEvent::InitialSourceDiscovery {
            context_id: first.clone(),
        },
        InventoryPipelineEvent::Parse {
            context_id: first.clone(),
            native_source_key: "fixture-file".to_owned(),
        },
        InventoryPipelineEvent::Resolve {
            context_id: first.clone(),
        },
        InventoryPipelineEvent::Project { context_id: first },
        InventoryPipelineEvent::InitialSourceDiscovery { context_id: second },
    ];
    expected.extend(second_context_events);
    assert_eq!(actual, expected);
}

fn assert_second_context_outputs(
    inventory: &crate::models::AgentEnvironmentInventory,
    second_context_id: &str,
    expected_declarations: usize,
) {
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.context_id == second_context_id)
            .count(),
        expected_declarations
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.context_id == second_context_id)
            .count(),
        0
    );
}

#[test]
fn deadline_during_second_context_parse_keeps_first_context_outputs() {
    let (root, inventory, probe, snapshot_state, snapshots) =
        healthy_two_context_inventory_with_deadline(AgentInventoryStage::Parse, 6);
    let (first, second) = ordered_context_ids(&inventory);
    assert_first_context_is_complete(&inventory, &first);
    assert_second_context_outputs(&inventory, &second, 0);
    assert_two_context_snapshot_state(&inventory, &snapshot_state);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 7);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 4);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 1);
    assert_two_context_events(
        &inventory,
        probe.events(),
        vec![InventoryPipelineEvent::Parse {
            context_id: second,
            native_source_key: "fixture-file".to_owned(),
        }],
    );
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_after_second_context_parse_keeps_first_context_outputs() {
    let (root, inventory, probe, snapshot_state, snapshots) =
        healthy_two_context_inventory_with_deadline(AgentInventoryStage::Resolve, 4);
    let (first, second) = ordered_context_ids(&inventory);
    assert_first_context_is_complete(&inventory, &first);
    assert_second_context_outputs(&inventory, &second, 2);
    assert_two_context_snapshot_state(&inventory, &snapshot_state);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 8);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 4);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 1);
    assert_two_context_events(
        &inventory,
        probe.events(),
        vec![InventoryPipelineEvent::Parse {
            context_id: second,
            native_source_key: "fixture-file".to_owned(),
        }],
    );
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_during_second_context_resolve_keeps_first_context_outputs() {
    let (root, inventory, probe, snapshot_state, snapshots) =
        healthy_two_context_inventory_with_deadline(AgentInventoryStage::Resolve, 6);
    let (first, second) = ordered_context_ids(&inventory);
    assert_first_context_is_complete(&inventory, &first);
    assert_second_context_outputs(&inventory, &second, 2);
    assert_two_context_snapshot_state(&inventory, &snapshot_state);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 8);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 6);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 1);
    assert_two_context_events(
        &inventory,
        probe.events(),
        vec![
            InventoryPipelineEvent::Parse {
                context_id: second.clone(),
                native_source_key: "fixture-file".to_owned(),
            },
            InventoryPipelineEvent::Resolve { context_id: second },
        ],
    );
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_before_second_context_project_keeps_first_context_outputs() {
    let (root, inventory, probe, snapshot_state, snapshots) =
        healthy_two_context_inventory_with_deadline(AgentInventoryStage::Project, 2);
    let (first, second) = ordered_context_ids(&inventory);
    assert_first_context_is_complete(&inventory, &first);
    assert_second_context_outputs(&inventory, &second, 2);
    assert_two_context_snapshot_state(&inventory, &snapshot_state);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 8);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 6);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 2);
    assert_two_context_events(
        &inventory,
        probe.events(),
        vec![
            InventoryPipelineEvent::Parse {
                context_id: second.clone(),
                native_source_key: "fixture-file".to_owned(),
            },
            InventoryPipelineEvent::Resolve { context_id: second },
        ],
    );
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_one_over_keeps_and_projects_the_deterministic_partial_inventory() {
    let root = test_root("source-one-over-partial-inventory");
    fs::create_dir_all(root.join("fixture")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_three_file_sources_until_break,
        None,
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let mut limits = AgentAssetLimits::DEFAULT;
    limits.sources_per_context = 2;
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file("fixture-a", 1),
        ExpectedSnapshot::file("fixture-b", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();

    let source_ids_by_label = inventory
        .sources
        .iter()
        .map(|source| (source.label.as_str(), source.id.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        source_ids_by_label.keys().copied().collect::<Vec<_>>(),
        ["fixture-a", "fixture-b"]
    );
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(inventory.assets.len(), 2);
    let snapshot_state = snapshot_state.lock().expect("snapshot state lock");
    assert_eq!(
        snapshot_state.attempts,
        ["fixture-a", "fixture-b"]
            .map(|label| source_ids_by_label[label].clone())
            .to_vec()
    );
    assert_eq!(snapshot_state.opens, snapshot_state.attempts);
    assert_eq!(snapshot_state.charges.len(), 2);
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::SourcesPerContext,
                    accepted: 2,
                    observed_at_least: 3,
                }
            ))
            .count(),
        1
    );
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn parse_entry_limit_keeps_prefix_but_suppresses_resolution_and_projection() {
    let root = test_root("parse-entry-limit-production");
    fs::create_dir_all(root.join("fixture")).unwrap();
    fs::write(root.join("fixture/config.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_file_source,
        None,
        fixture_parse_three_assets,
        fixture_resolve,
    )];
    let mut limits = AgentAssetLimits::DEFAULT;
    limits.first_level_entries = 2;
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &RealSnapshotPort::default(),
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(inventory.declarations.len(), 2);
    let declaration_ids = inventory
        .declarations
        .iter()
        .map(|declaration| declaration.native_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(declaration_ids.len(), 2);
    assert!(declaration_ids
        .iter()
        .all(|native_id| ["first", "second", "third"].contains(native_id)));
    assert!(inventory.assets.is_empty());
    assert!(inventory.sources.iter().any(|source| {
        source.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::FirstLevelEntries,
                    accepted: 2,
                    observed_at_least: 3,
                }
            )
        })
    }));
    assert!(!probe.events().iter().any(|event| matches!(
        event,
        InventoryPipelineEvent::Resolve { .. } | InventoryPipelineEvent::Project { .. }
    )));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_source_one_over_keeps_completed_parent_and_child_pipeline_outputs() {
    let root = test_root("follow-up-source-one-over");
    fs::create_dir_all(root.join("fixture/parent/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let mut limits = AgentAssetLimits::DEFAULT;
    limits.sources_per_context = 2;
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_two_children),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::parent_directory(),
        ExpectedSnapshot::file("fixture-file1", 1),
        ExpectedSnapshot::file("fixture-file2", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(inventory.assets.len(), 2);
    assert!(inventory
        .sources
        .iter()
        .any(|source| source.path.replace('\\', "/").ends_with("fixture/parent")));
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent/childdir/file1")));
    assert!(!inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent/childdir/file2")));
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::SourcesPerContext,
                    accepted: 2,
                    observed_at_least: 3,
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        probe.hit_count(AgentInventoryStage::FollowUpSourceDiscovery),
        3
    );
    assert_eq!(probe.hit_count(AgentInventoryStage::Snapshot), 2);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 6);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 3);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 1);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::FollowUpSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
                parent_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-file1".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Project {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    let snapshot_state = snapshot_state
        .lock()
        .expect("follow-up source snapshot state lock");
    assert_eq!(
        snapshot_state.native_source_keys,
        ["fixture-parent", "fixture-file1"]
    );
    assert_eq!(snapshot_state.attempts.len(), 2);
    assert_eq!(snapshot_state.opens, snapshot_state.attempts);
    assert_eq!(
        snapshot_state.charges,
        snapshot_state
            .attempts
            .iter()
            .zip([10, 1])
            .map(|(source_id, bytes)| (source_id.clone(), bytes))
            .collect::<Vec<_>>()
    );
    drop(snapshot_state);
    snapshots.assert_next_scripted("fixture-file2");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_break_stops_later_parent_callbacks_and_snapshots() {
    let root = test_root("follow-up-terminal-break");
    fs::create_dir_all(root.join("fixture/parent-a/childdir")).unwrap();
    fs::create_dir_all(root.join("fixture/parent-z/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let mut limits = AgentAssetLimits::DEFAULT;
    limits.sources_per_context = 3;
    let definitions = [fixture_definition_with_pipeline(
        fixture_two_directory_sources,
        Some(fixture_follow_up_two_children),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    let child_entry = AgentAssetDirectoryEntry {
        name: "childdir".to_owned(),
        source_kind: AgentAssetSourceKind::Directory,
        is_symlink: false,
    };
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::directory("fixture-parent-a", 10, vec![child_entry], true),
        ExpectedSnapshot::file("fixture-file1", 1),
        ExpectedSnapshot::file("fixture-file2", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(inventory.sources.len(), 3);
    assert!(!inventory
        .sources
        .iter()
        .any(|source| source.path.replace('\\', "/").ends_with("fixture/parent-z")));
    assert_eq!(
        probe
            .events()
            .iter()
            .filter_map(|event| match event {
                InventoryPipelineEvent::FollowUpSourceDiscovery {
                    parent_source_key, ..
                } => Some(parent_source_key.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["fixture-parent-a"]
    );
    let snapshot_state = snapshot_state
        .lock()
        .expect("terminal follow-up snapshot state lock");
    assert_eq!(
        snapshot_state.native_source_keys,
        ["fixture-parent-a", "fixture-file1", "fixture-file2"]
    );
    assert!(!snapshot_state
        .native_source_keys
        .iter()
        .any(|key| key == "fixture-parent-z"));
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_collision_cannot_reject_or_resnapshot_its_shallower_parent() {
    let root = test_root("follow-up-shallower-collision");
    fs::create_dir_all(root.join("fixture/parent/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_collides_with_parent),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();

    assert_eq!(inventory.sources.len(), 1);
    assert_eq!(inventory.declarations.len(), 1);
    assert_eq!(inventory.assets.len(), 1);
    assert!(inventory.sources[0]
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent"));
    assert!(inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidProjection { projection_key }
            if projection_key == "source:fixture-parent"
    )));
    let snapshot_state = snapshot_state
        .lock()
        .expect("shallower collision snapshot state lock");
    assert_eq!(snapshot_state.native_source_keys, ["fixture-parent"]);
    assert_eq!(snapshot_state.attempts.len(), 1);
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_before_next_agent_keeps_completed_agent_and_skips_later_ports() {
    let root = test_root("deadline-before-next-agent");
    fs::create_dir_all(root.join("fixture/parent")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [
        fixture_definition(fixture_parent_source, None),
        fixture_definition_for_kind(
            AgentCliKind::ClaudeCode,
            fixture_parent_source,
            None,
            fixture_parse,
            fixture_resolve,
        ),
    ];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::InstallationDiscovery,
        2,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let (installations, installation_state) = FakeInstallationPort::new(Vec::new());
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let settings = crate::models::AppSettings::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: Some(&settings),
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert!(!inventory.assets.is_empty());
    assert_eq!(
        probe.hit_count(AgentInventoryStage::InstallationDiscovery),
        2,
        "the second Agent is gated before installation discovery"
    );
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Project {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert_eq!(
        installation_state
            .lock()
            .expect("installation state lock")
            .calls
            .len(),
        1
    );
    assert!(!snapshot_state
        .lock()
        .expect("snapshot state lock")
        .attempts
        .is_empty());
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn normal_inventory_limits_installation_port_to_version_probe_contract() {
    let root = test_root("normal-inventory-installation-port");
    fs::create_dir_all(root.join("fixture/parent")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition(fixture_parent_source, None)];
    let settings = crate::models::AppSettings::default();
    let (installations, installation_state) =
        FakeInstallationPort::new(vec![fixture_installation()]);
    let snapshots = RealSnapshotPort::default();

    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: Some(&settings),
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

    assert_eq!(inventory.installations.len(), 1);
    let installation_state = installation_state.lock().expect("installation state lock");
    assert_eq!(installation_state.calls, vec!["codex"]);
    assert_eq!(installation_state.version_probes, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_before_initial_source_keeps_context_without_source_work() {
    let root = test_root("deadline-before-initial-source");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition(fixture_parent_source, None)];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::InitialSourceDiscovery,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(inventory.contexts.len(), 1);
    assert_eq!(
        probe.hit_count(AgentInventoryStage::InitialSourceDiscovery),
        1,
        "the source stage is entered once and expires before the adapter callback"
    );
    assert_eq!(
        probe.events(),
        vec![InventoryPipelineEvent::ContextDiscovery {
            agent_kind: AgentCliKind::Codex,
        }]
    );
    assert!(inventory.sources.is_empty());
    assert!(inventory.declarations.is_empty());
    assert!(inventory.assets.is_empty());
    assert!(snapshot_state
        .lock()
        .expect("snapshot state lock")
        .attempts
        .is_empty());
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_after_parent_snapshot_keeps_cached_parent_and_skips_follow_up() {
    let root = test_root("deadline-after-parent-snapshot");
    fs::create_dir_all(root.join("fixture/parent/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_one_child),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::FollowUpSourceDiscovery,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(
        probe.hit_count(AgentInventoryStage::FollowUpSourceDiscovery),
        1,
        "the cached parent is followed up only after the shared deadline gate"
    );
    assert_eq!(probe.hit_count(AgentInventoryStage::Snapshot), 1);
    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 1);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 1);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 0);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert!(inventory.declarations.is_empty());
    assert!(inventory.assets.is_empty());
    let snapshot_state = snapshot_state.lock().expect("snapshot state lock");
    assert_eq!(snapshot_state.attempts.len(), 1);
    assert_eq!(snapshot_state.opens.len(), 1);
    assert_eq!(
        snapshot_state.charges,
        vec![(snapshot_state.attempts[0].clone(), 10)]
    );
    assert_eq!(inventory.sources.len(), 1);
    assert!(inventory.sources[0]
        .revision
        .identity
        .starts_with("synthetic:"));
    assert!(!inventory.contexts.is_empty());
    assert!(!inventory
        .sources
        .iter()
        .any(|source| source.path.contains("childdir")));
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_snapshot_reuses_cached_parent_and_allows_bounded_child_inspection() {
    let root = test_root("follow-up-cached-parent");
    fs::create_dir_all(root.join("fixture/parent/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_one_child),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::parent_directory(),
        ExpectedSnapshot::directory("fixture-child", 10, Vec::new(), true),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::FollowUpSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
                parent_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::FollowUpSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
                parent_source_key: "fixture-child".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-child".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Project {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert_eq!(
        probe.hit_count(AgentInventoryStage::FollowUpSourceDiscovery),
        3,
        "one follow-up stage entry and one bounded child offer per depth"
    );
    assert_eq!(probe.hit_count(AgentInventoryStage::Snapshot), 2);
    let snapshot_state = snapshot_state.lock().expect("snapshot state lock");
    assert_eq!(snapshot_state.attempts.len(), 2);
    assert_eq!(snapshot_state.opens, snapshot_state.attempts);
    assert_eq!(
        snapshot_state.charges,
        vec![
            (snapshot_state.attempts[0].clone(), 10),
            (snapshot_state.attempts[1].clone(), 10),
        ]
    );
    assert!(inventory
        .sources
        .iter()
        .any(|source| source.path.replace('\\', "/").ends_with("fixture/parent")));
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent/childdir/file")));
    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(inventory.assets.len(), 2);
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_expands_breadth_first_through_depth_two_only() {
    let root = test_root("follow-up-depth-two");
    fs::create_dir_all(root.join("fixture/parent/childdir/nested")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_chain),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(Arc::clone(&clock)));
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::parent_directory(),
        ExpectedSnapshot::directory(
            "fixture-child",
            10,
            vec![AgentAssetDirectoryEntry {
                name: "nested".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            }],
            true,
        ),
        ExpectedSnapshot::directory(
            "fixture-grandchild",
            10,
            vec![AgentAssetDirectoryEntry {
                name: "depth-three".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            }],
            true,
        ),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    let snapshot_state = snapshot_state
        .lock()
        .expect("depth-two snapshot state lock");
    assert_eq!(
        snapshot_state.native_source_keys,
        ["fixture-parent", "fixture-child", "fixture-grandchild"]
    );
    assert_eq!(snapshot_state.attempts.len(), 3);
    assert_eq!(inventory.sources.len(), 3);
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent/childdir/child-config")));
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("fixture/parent/childdir/child-config/nested/grandchild-config")));
    assert!(!probe.events().iter().any(|event| {
        matches!(
            event,
            InventoryPipelineEvent::FollowUpSourceDiscovery {
                parent_source_key,
                ..
            } if parent_source_key == "fixture-grandchild"
        )
    }));
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn follow_up_does_not_expand_an_incomplete_parent_manifest() {
    let root = test_root("follow-up-incomplete-parent");
    fs::create_dir_all(root.join("fixture/parent/childdir")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_chain),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(Arc::clone(&clock)));
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([ExpectedSnapshot::directory(
        "fixture-parent",
        10,
        vec![AgentAssetDirectoryEntry {
            name: "childdir".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }],
        false,
    )]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(inventory.sources.len(), 1);
    assert_eq!(
        snapshot_state
            .lock()
            .expect("incomplete snapshot lock")
            .attempts
            .len(),
        1
    );
    assert!(!probe.events().iter().any(|event| {
        matches!(
            event,
            InventoryPipelineEvent::FollowUpSourceDiscovery { .. }
        )
    }));
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_during_parse_discards_current_source_buffer() {
    let root = test_root("deadline-during-parse");
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    fs::write(root.join("fixture-first.json"), b"first").unwrap();
    fs::write(root.join("fixture-second.json"), b"second").unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_two_file_sources,
        None,
        fixture_parse_first_one_second_two,
        fixture_resolve,
    )];
    let mut limits = AgentAssetLimits::DEFAULT;
    limits.bytes_per_refresh = 64;
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::Parse,
        6,
        std::time::Duration::from_millis(limits.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file("fixture-first", 1),
        ExpectedSnapshot::file("fixture-second", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(probe.hit_count(AgentInventoryStage::Parse), 7);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 1);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-first".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-second".to_owned(),
            },
        ]
    );
    assert_eq!(
        snapshot_state
            .lock()
            .expect("snapshot state lock")
            .attempts
            .len(),
        2
    );
    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(inventory.declarations.len(), 1);
    assert_eq!(inventory.declarations[0].native_id, "first-one");
    assert!(inventory.assets.is_empty());
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

fn strip_observation_timestamps(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("scannedAt");
            object.remove("observedAt");
            for value in object.values_mut() {
                strip_observation_timestamps(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                strip_observation_timestamps(value);
            }
        }
        _ => {}
    }
}

#[test]
fn full_inventory_is_stable_when_source_offer_order_changes() {
    let root = test_root("inventory-order-stability");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fixture-first.json"), b"first").unwrap();
    fs::write(root.join("fixture-second.json"), b"second").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let build = |discover_sources| {
        let definitions = [fixture_definition_with_pipeline(
            discover_sources,
            None,
            fixture_parse_first_one_second_two,
            fixture_resolve,
        )];
        let installations = RealInstallationDiscoveryPort;
        let clock = Arc::new(ManualClock::new());
        let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
        let (snapshots, _) = FakeSnapshotPort::scripted([
            ExpectedSnapshot::file("fixture-first", 1),
            ExpectedSnapshot::file("fixture-second", 1),
        ]);
        let inventory = build_inventory_with(
            InventoryInput {
                home: &root,
                workspace: None,
                settings: None,
            },
            InventoryPipelineDeps {
                definitions: &definitions,
                limits: AgentAssetLimits::DEFAULT,
                clock,
                installations: &installations,
                snapshots: &snapshots,
                checkpoint_probe: Some(probe.clone()),
            },
        )
        .unwrap();
        snapshots.assert_scripted_exhausted();
        let stage_hits = [
            AgentInventoryStage::InitialSourceDiscovery,
            AgentInventoryStage::Snapshot,
            AgentInventoryStage::Parse,
            AgentInventoryStage::Resolve,
            AgentInventoryStage::Project,
        ]
        .map(|stage| probe.hit_count(stage));
        (inventory, probe.events(), stage_hits)
    };

    let (first_inventory, first_events, first_stage_hits) = build(fixture_two_file_sources);
    let (reversed_inventory, reversed_events, reversed_stage_hits) =
        build(fixture_two_file_sources_reversed);
    let mut first = serde_json::to_value(first_inventory).unwrap();
    let mut reversed = serde_json::to_value(reversed_inventory).unwrap();
    strip_observation_timestamps(&mut first);
    strip_observation_timestamps(&mut reversed);
    assert_eq!(first, reversed);
    assert_eq!(first_events, reversed_events);
    assert_eq!(first_stage_hits, reversed_stage_hits);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn full_inventory_is_stable_when_declarations_and_drafts_change_order() {
    let root = test_root("inventory-declaration-draft-order-stability");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("fixture-first.json"), b"first").unwrap();
    fs::write(root.join("fixture-second.json"), b"second").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let build =
        |parse: crate::services::agent_cli::contracts::AgentAssetParser,
         resolve: crate::services::agent_cli::contracts::AgentAssetResolver| {
            let definitions = [fixture_definition_with_pipeline(
                fixture_two_file_sources,
                None,
                parse,
                resolve,
            )];
            let clock = Arc::new(ManualClock::new());
            let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
            let (snapshots, _) = FakeSnapshotPort::scripted([
                ExpectedSnapshot::file("fixture-first", 1),
                ExpectedSnapshot::file("fixture-second", 1),
            ]);
            let inventory = build_inventory_with(
                InventoryInput {
                    home: &root,
                    workspace: None,
                    settings: None,
                },
                InventoryPipelineDeps {
                    definitions: &definitions,
                    limits: AgentAssetLimits::DEFAULT,
                    clock,
                    installations: &RealInstallationDiscoveryPort,
                    snapshots: &snapshots,
                    checkpoint_probe: Some(probe.clone()),
                },
            )
            .unwrap();
            snapshots.assert_scripted_exhausted();
            let stage_hits = [
                AgentInventoryStage::InitialSourceDiscovery,
                AgentInventoryStage::Snapshot,
                AgentInventoryStage::Parse,
                AgentInventoryStage::Resolve,
                AgentInventoryStage::Project,
            ]
            .map(|stage| probe.hit_count(stage));
            (inventory, probe.events(), stage_hits)
        };

    let (normal_inventory, normal_events, normal_stage_hits) =
        build(fixture_parse_first_one_second_two, fixture_resolve);
    let (reversed_inventory, reversed_events, reversed_stage_hits) = build(
        fixture_parse_first_one_second_two_reversed,
        fixture_resolve_reversed,
    );
    let mut normal = serde_json::to_value(normal_inventory).unwrap();
    let mut reversed = serde_json::to_value(reversed_inventory).unwrap();
    strip_observation_timestamps(&mut normal);
    strip_observation_timestamps(&mut reversed);
    assert_eq!(normal, reversed);
    assert_eq!(normal_events, reversed_events);
    assert_eq!(normal_stage_hits, reversed_stage_hits);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_after_parse_keeps_declarations_but_skips_resolution() {
    let root = test_root("deadline-after-parse");
    fs::create_dir_all(root.join("fixture/parent")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition(fixture_parent_source, None)];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::Resolve,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 1);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 0);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
        ]
    );
    assert_eq!(
        snapshot_state
            .lock()
            .expect("snapshot state lock")
            .attempts
            .len(),
        1
    );
    assert_eq!(inventory.declarations.len(), 2);
    assert!(inventory.assets.is_empty());
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_during_resolution_discards_all_current_drafts() {
    let root = test_root("deadline-during-resolution");
    fs::create_dir_all(root.join("fixture/parent")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition(fixture_parent_source, None)];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::Resolve,
        3,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 3);
    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 0);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert_eq!(
        snapshot_state
            .lock()
            .expect("snapshot state lock")
            .attempts
            .len(),
        1
    );
    assert_eq!(inventory.declarations.len(), 2);
    assert!(inventory.assets.is_empty());
    assert!(inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::BudgetExceeded { .. })));
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deadline_before_project_keeps_declarations_without_partial_records() {
    let root = test_root("deadline-before-project");
    fs::create_dir_all(root.join("fixture/parent")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition(fixture_parent_source, None)];
    let clock = Arc::new(ManualClock::new());
    let probe = Arc::new(StageCheckpointProbe::new(clock.clone()));
    probe.advance_on(
        AgentInventoryStage::Project,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::parent_directory()]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock,
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: Some(probe.clone()),
        },
    )
    .unwrap();

    assert_eq!(probe.hit_count(AgentInventoryStage::Project), 1);
    assert_eq!(probe.hit_count(AgentInventoryStage::Resolve), 3);
    assert_eq!(
        probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert_eq!(
        snapshot_state
            .lock()
            .expect("snapshot state lock")
            .attempts
            .len(),
        1
    );
    assert_eq!(inventory.declarations.len(), 2);
    assert!(inventory.assets.is_empty());
    assert!(inventory
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::BudgetExceeded { .. })));
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inventory_closes_follow_up_at_deadline_and_bytes_and_hides_unsnapshotted_children() {
    let deadline_root = test_root("inventory-deadline");
    fs::create_dir_all(deadline_root.join("fixture/parent/childdir")).unwrap();
    let deadline_root = fs::canonicalize(deadline_root).unwrap();
    let deadline_definition =
        fixture_definition(fixture_parent_source, Some(fixture_follow_up_one_child));
    let deadline_clock = Arc::new(ManualClock::new());
    let deadline_probe = Arc::new(StageCheckpointProbe::new(deadline_clock.clone()));
    let (deadline_snapshots, deadline_snapshot_state) = FakeSnapshotPort::scripted([]);
    deadline_probe.advance_on(
        AgentInventoryStage::Snapshot,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let installations = RealInstallationDiscoveryPort;
    let inventory = build_inventory_with(
        InventoryInput {
            home: &deadline_root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[deadline_definition],
            limits: AgentAssetLimits::DEFAULT,
            clock: deadline_clock,
            installations: &installations,
            snapshots: &deadline_snapshots,
            checkpoint_probe: Some(deadline_probe),
        },
    )
    .unwrap();
    assert!(inventory.sources.is_empty());
    assert!(deadline_snapshot_state
        .lock()
        .expect("deadline snapshot state lock")
        .attempts
        .is_empty());
    deadline_snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(deadline_root);

    let closed_root = test_root("inventory-bytes-closed");
    fs::create_dir_all(closed_root.join("fixture/parent/childdir")).unwrap();
    let closed_root = fs::canonicalize(closed_root).unwrap();
    let closed_fixture = closed_root.join("fixture");
    let closed_definition = fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_one_child),
        fixture_parse_source_key,
        fixture_resolve,
    );
    let closed_limits = AgentAssetLimits {
        bytes_per_refresh: 10,
        ..AgentAssetLimits::DEFAULT
    };
    let closed_clock = Arc::new(ManualClock::new());
    let closed_probe = Arc::new(StageCheckpointProbe::new(closed_clock.clone()));
    let (closed_snapshots, closed_snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::parent_directory(),
        ExpectedSnapshot::directory("fixture-child", 10, Vec::new(), true),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &closed_root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[closed_definition],
            limits: closed_limits,
            clock: closed_clock,
            installations: &installations,
            snapshots: &closed_snapshots,
            checkpoint_probe: Some(closed_probe.clone()),
        },
    )
    .unwrap();
    assert!(inventory
        .sources
        .iter()
        .any(|source| source.path == closed_fixture.join("parent").to_string_lossy()));
    assert_eq!(inventory.declarations.len(), 1);
    assert_eq!(inventory.assets.len(), 1);
    assert_eq!(closed_probe.hit_count(AgentInventoryStage::Snapshot), 1);
    assert_eq!(closed_probe.hit_count(AgentInventoryStage::Parse), 3);
    assert_eq!(closed_probe.hit_count(AgentInventoryStage::Resolve), 2);
    assert_eq!(closed_probe.hit_count(AgentInventoryStage::Project), 1);
    assert_eq!(
        closed_probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Project {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    let closed_snapshot_state = closed_snapshot_state
        .lock()
        .expect("closed snapshot state lock");
    assert_eq!(closed_snapshot_state.native_source_keys, ["fixture-parent"]);
    assert_eq!(closed_snapshot_state.attempts.len(), 1);
    assert_eq!(closed_snapshot_state.opens.len(), 1);
    assert!(!inventory
        .sources
        .iter()
        .any(|source| source.path.contains("childdir")));
    drop(closed_snapshot_state);
    closed_snapshots.assert_next_scripted("fixture-child");
    let _ = fs::remove_dir_all(closed_root);

    let child_root = test_root("inventory-unsnapshotted-child");
    fs::create_dir_all(child_root.join("fixture/parent/childdir")).unwrap();
    let child_root = fs::canonicalize(child_root).unwrap();
    let child_fixture = child_root.join("fixture");
    fs::write(child_fixture.join("parent/childdir/file1"), b"a").unwrap();
    fs::write(child_fixture.join("parent/childdir/file2"), b"b").unwrap();
    let child_definition = fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_follow_up_two_children),
        fixture_parse_source_key,
        fixture_resolve,
    );
    let child_limits = AgentAssetLimits {
        bytes_per_refresh: 11,
        ..AgentAssetLimits::DEFAULT
    };
    let child_clock = Arc::new(ManualClock::new());
    let child_probe = Arc::new(StageCheckpointProbe::new(child_clock.clone()));
    let (child_snapshots, child_snapshot_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::parent_directory(),
        ExpectedSnapshot::file("fixture-file1", 1),
        ExpectedSnapshot::file("fixture-file2", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &child_root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[child_definition],
            limits: child_limits,
            clock: child_clock,
            installations: &installations,
            snapshots: &child_snapshots,
            checkpoint_probe: Some(child_probe.clone()),
        },
    )
    .unwrap();
    let child_snapshot_state = child_snapshot_state
        .lock()
        .expect("child snapshot state lock");
    assert_eq!(
        child_snapshot_state.native_source_keys,
        ["fixture-parent", "fixture-file1"]
    );
    assert_eq!(child_snapshot_state.attempts.len(), 2);
    assert_eq!(child_snapshot_state.opens, child_snapshot_state.attempts);
    assert_eq!(
        child_snapshot_state.charges,
        vec![
            (child_snapshot_state.attempts[0].clone(), 10),
            (child_snapshot_state.attempts[1].clone(), 1),
        ]
    );
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::BytesPerRefresh,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(inventory.assets.len(), 2);
    assert_eq!(child_probe.hit_count(AgentInventoryStage::Snapshot), 4);
    assert_eq!(child_probe.hit_count(AgentInventoryStage::Parse), 6);
    assert_eq!(child_probe.hit_count(AgentInventoryStage::Resolve), 3);
    assert_eq!(child_probe.hit_count(AgentInventoryStage::Project), 1);
    assert_eq!(
        child_probe.events(),
        vec![
            InventoryPipelineEvent::ContextDiscovery {
                agent_kind: AgentCliKind::Codex,
            },
            InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::FollowUpSourceDiscovery {
                context_id: inventory.contexts[0].id.clone(),
                parent_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-parent".to_owned(),
            },
            InventoryPipelineEvent::Parse {
                context_id: inventory.contexts[0].id.clone(),
                native_source_key: "fixture-file1".to_owned(),
            },
            InventoryPipelineEvent::Resolve {
                context_id: inventory.contexts[0].id.clone(),
            },
            InventoryPipelineEvent::Project {
                context_id: inventory.contexts[0].id.clone(),
            },
        ]
    );
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("parent/childdir/file1")));
    assert!(!inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("parent/childdir/file2")));
    drop(child_snapshot_state);
    child_snapshots.assert_next_scripted("fixture-file2");
    let _ = fs::remove_dir_all(child_root);
}

#[test]
fn inventory_materializes_projection_suppression_on_its_declaration() {
    let root = test_root("suppression-owner");
    fs::create_dir_all(root.join("fixture")).unwrap();
    fs::write(root.join("fixture/config.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let installations = RealInstallationDiscoveryPort;
    let snapshots = RealSnapshotPort::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[fixture_suppression_definition()],
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();

    assert!(inventory.assets.is_empty());
    assert_eq!(inventory.declarations.len(), 1);
    let declaration = inventory
        .declarations
        .iter()
        .find(|value| value.native_id == "suppressed")
        .expect("suppressed declaration");
    assert!(declaration.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::DeclarationSuppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
        }
    )));
    assert!(!inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::DeclarationSuppressed { .. }
    )));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inventory_assessment_failure_preserves_raw_evidence_and_healthy_target() {
    use crate::services::agent_cli::contracts::{
        AgentAssetAssessmentIndex, AgentAssetAssessmentRequest,
    };

    fn incomplete_first_source(
        request: AgentAssetAssessmentRequest<'_>,
    ) -> AgentAssetAssessmentIndex {
        let sources = request
            .sources
            .iter()
            .filter(|source| source.native_source_key != "fixture-first")
            .cloned()
            .collect::<Vec<_>>();
        fixture_assessor(AgentAssetAssessmentRequest {
            sources: &sources,
            ..request
        })
    }

    let root = test_root("assessment-failure-evidence");
    fs::create_dir_all(root.join("fixture")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let mut registered = fixture_definition_with_pipeline(
        fixture_two_file_sources,
        None,
        fixture_parse_source_key,
        fixture_resolve,
    );
    registered.environment = EnvironmentAdapter::with_pipeline(
        fixture_contexts,
        fixture_two_file_sources,
        None,
        "fixture-assessment-failure",
        fixture_parse_source_key,
        fixture_resolve,
        incomplete_first_source,
    );
    let (installations, _) = FakeInstallationPort::new(Vec::new());
    let (snapshots, _) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::file("fixture-first", 1),
        ExpectedSnapshot::file("fixture-second", 1),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[registered],
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    snapshots.assert_scripted_exhausted();
    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(inventory.declarations.len(), 2);
    assert_eq!(
        inventory
            .declarations
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["fixture-first", "fixture-second"])
    );
    for declaration in &inventory.declarations {
        assert!(inventory
            .sources
            .iter()
            .any(|source| source.id == declaration.source_id));
        assert!(declaration.diagnostics.is_empty());
    }
    assert!(inventory
        .sources
        .iter()
        .all(|source| source.diagnostics.is_empty()));
    assert_eq!(inventory.assets.len(), 1, "{:?}", inventory.diagnostics);
    let healthy = &inventory.assets[0];
    assert_eq!(healthy.native_id, "fixture-second");
    assert_eq!(healthy.effective_state, AgentAssetState::Enabled);
    assert!(healthy.diagnostics.is_empty());
    assert!(healthy.resolution.diagnostics.is_empty());
    let failures = inventory
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(failures.as_slice(), [AgentAssetDiagnostic::InvalidResolution { projection_key, .. }]
        if projection_key == "skill:fixture-first:assessment:IncompleteInput"),
        "{failures:?}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inventory_propagates_parser_relationships_to_public_records() {
    let root = test_root("relationship-propagation");
    fs::create_dir_all(root.join("fixture")).unwrap();
    fs::write(root.join("fixture/relationships.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let installations = RealInstallationDiscoveryPort;
    let snapshots = RealSnapshotPort::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[fixture_relationship_definition()],
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    assert_eq!(inventory.assets.len(), 2, "{:?}", inventory.diagnostics);
    let parent = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "provider")
        .unwrap();
    let child = inventory
        .assets
        .iter()
        .find(|asset| asset.native_id == "child")
        .unwrap();
    assert_eq!(
        child.relationships.provided_by.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(
        child.relationships.action_owner.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(
        parent.relationships.affected_asset_ids,
        vec![child.stable_id.clone()]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inventory_materializes_each_context_declaration_slice_exactly_once() {
    let root = test_root("declaration-context-slices");
    fs::create_dir_all(root.join("fixture/first")).unwrap();
    fs::create_dir_all(root.join("fixture/second")).unwrap();
    fs::write(root.join("fixture/first/config.json"), b"{}").unwrap();
    fs::write(root.join("fixture/second/config.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let installations = RealInstallationDiscoveryPort;
    let snapshots = RealSnapshotPort::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &[fixture_two_context_definition()],
            limits: AgentAssetLimits {
                diagnostics: 8,
                ..AgentAssetLimits::DEFAULT
            },
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();

    let first = inventory
        .declarations
        .iter()
        .find(|declaration| declaration.native_id == "first")
        .expect("first context declaration");
    assert_eq!(
        first
            .diagnostics
            .iter()
            .filter(|value| matches!(value, AgentAssetDiagnostic::InvalidProjection { .. }))
            .count(),
        1
    );
    let second = inventory
        .declarations
        .iter()
        .find(|declaration| declaration.native_id == "second")
        .expect("second context declaration");
    assert!(second.diagnostics.is_empty(), "{second:?}");

    let flattened = inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .installations
                .iter()
                .flat_map(|item| item.diagnostics.iter()),
        )
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|item| item.diagnostics.iter()),
        )
        .chain(
            inventory
                .declarations
                .iter()
                .flat_map(|item| item.diagnostics.iter()),
        )
        .chain(inventory.assets.iter().flat_map(|item| {
            item.diagnostics
                .iter()
                .chain(item.resolution.diagnostics.iter())
        }))
        .collect::<Vec<_>>();
    assert_eq!(flattened.len(), 1, "{flattened:?}");
    assert_eq!(
        flattened
            .iter()
            .filter(|value| matches!(value, AgentAssetDiagnostic::InvalidProjection { .. }))
            .count(),
        1
    );
    assert_eq!(
        flattened
            .iter()
            .filter(|value| matches!(value, AgentAssetDiagnostic::InvalidResolution { .. }))
            .count(),
        0
    );
    assert_eq!(
        flattened
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        flattened.len(),
        "a diagnostic must materialize exactly once in the public tree: {flattened:?}"
    );
    assert!(flattened.len() <= inventory.limits.diagnostics);
    assert!(inventory.diagnostics.is_empty(), "{inventory:?}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn passive_inventory_indexes_mcp_plugin_hook_and_status_without_execution() {
    let root = test_root("passive-asset-categories");
    fs::create_dir_all(root.join("fixture")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let sentinel = root.join("passive-asset-command-must-not-run");
    fs::write(
        root.join("fixture/passive-assets.json"),
        format!(
            r#"{{
                "mcp": {{"command": "touch {}", "url": "https://example.invalid/mcp"}},
                "plugin": {{"entrypoint": "touch {}"}},
                "hook": {{"command": "touch {}"}},
                "statusLine": {{"type": "command", "command": "touch {}"}}
            }}"#,
            sentinel.display(),
            sentinel.display(),
            sentinel.display(),
            sentinel.display(),
        ),
    )
    .unwrap();

    let definitions = [fixture_definition_with_pipeline(
        fixture_passive_asset_source,
        None,
        fixture_parse_passive_assets,
        fixture_resolve_passive_assets,
    )];
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &RealSnapshotPort::default(),
            checkpoint_probe: None,
        },
    )
    .unwrap();

    let source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Fixture passive assets");
    assert!(source.is_some(), "passive fixture source must be indexed");
    assert_eq!(inventory.declarations.len(), 4);
    assert_eq!(inventory.assets.len(), 4);
    for category in [
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Plugin,
        AgentAssetCategory::Hook,
        AgentAssetCategory::StatusUi,
    ] {
        assert!(
            inventory
                .declarations
                .iter()
                .any(|declaration| declaration.native_kind == category),
            "missing declaration for {category:?}"
        );
        assert!(
            inventory
                .assets
                .iter()
                .any(|asset| asset.category == category),
            "missing projected asset for {category:?}"
        );
    }
    assert!(!sentinel.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn access_inventory_does_not_call_installation_port() {
    let root = test_root("access-no-installation-probe");
    fs::create_dir_all(root.join("fixture")).unwrap();
    fs::write(root.join("fixture/config.json"), b"{}").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let (installations, installation_state) = FakeInstallationPort::new(Vec::new());
    let definitions = [fixture_definition(fixture_file_source, None)];
    let (snapshots, snapshot_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::file("fixture-file", 2)]);

    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
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

    assert!(installation_state
        .lock()
        .expect("access installation state lock")
        .calls
        .is_empty());
    assert!(inventory.installations.is_empty());
    assert!(inventory.mechanisms.is_empty());
    assert!(!inventory.sources.is_empty());
    assert!(!inventory.declarations.is_empty());
    assert!(!inventory.assets.is_empty());
    let snapshot_state = snapshot_state.lock().expect("snapshot state lock");
    assert_eq!(snapshot_state.attempts.len(), 1);
    assert_eq!(snapshot_state.opens.len(), 1);
    assert_eq!(snapshot_state.charges.len(), 1);
    drop(snapshot_state);
    snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(root);
}

fn claude_declarations<'a>(
    inventory: &'a crate::models::AgentEnvironmentInventory,
    category: AgentAssetCategory,
    native_id: &str,
) -> Vec<&'a crate::models::AgentAssetDeclaration> {
    inventory
        .declarations
        .iter()
        .filter(|item| item.native_kind == category && item.native_id == native_id)
        .collect()
}

fn claude_assets<'a>(
    inventory: &'a crate::models::AgentEnvironmentInventory,
    category: AgentAssetCategory,
    native_id: &str,
) -> Vec<&'a crate::models::AgentAssetRecord> {
    inventory
        .assets
        .iter()
        .filter(|item| item.category == category && item.native_id == native_id)
        .collect()
}

fn assert_no_invalid_resolution_diagnostics(inventory: &crate::models::AgentEnvironmentInventory) {
    let is_invalid = |diagnostic: &AgentAssetDiagnostic| {
        !matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })
    };
    // Context-level diagnostics are exposed by the public inventory diagnostic
    // bucket; contexts themselves intentionally carry no diagnostic field.
    assert!(
        inventory.diagnostics.iter().all(is_invalid),
        "{inventory:?}"
    );
    assert!(
        inventory
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .all(is_invalid),
        "{inventory:?}"
    );
    assert!(
        inventory
            .declarations
            .iter()
            .flat_map(|declaration| declaration.diagnostics.iter())
            .all(is_invalid),
        "{inventory:?}"
    );
    assert!(
        inventory
            .assets
            .iter()
            .flat_map(|asset| asset
                .diagnostics
                .iter()
                .chain(asset.resolution.diagnostics.iter()))
            .all(is_invalid),
        "{inventory:?}"
    );
}

fn assert_no_structural_projection_diagnostics(
    inventory: &crate::models::AgentEnvironmentInventory,
) {
    let is_structural = |diagnostic: &AgentAssetDiagnostic| {
        matches!(
            diagnostic,
            AgentAssetDiagnostic::DuplicateNativeId { .. }
                | AgentAssetDiagnostic::InvalidCompatibleInstallation { .. }
                | AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
        )
    };
    assert!(
        inventory
            .diagnostics
            .iter()
            .all(|item| !is_structural(item)),
        "{inventory:?}"
    );
    assert!(
        inventory
            .installations
            .iter()
            .flat_map(|item| item.diagnostics.iter())
            .all(|item| !is_structural(item)),
        "{inventory:?}"
    );
    assert!(
        inventory
            .sources
            .iter()
            .flat_map(|item| item.diagnostics.iter())
            .chain(
                inventory
                    .declarations
                    .iter()
                    .flat_map(|item| item.diagnostics.iter()),
            )
            .chain(inventory.assets.iter().flat_map(|item| {
                item.diagnostics
                    .iter()
                    .chain(item.resolution.diagnostics.iter())
            }))
            .all(|item| !is_structural(item)),
        "{inventory:?}"
    );
}

fn assert_read_only_asset_actions(
    asset: &crate::models::AgentAssetRecord,
    mutation_reason: AgentAssetActionUnavailableReason,
) {
    let mut expected = vec![
        AgentAssetActionKind::Inspect,
        AgentAssetActionKind::Preview,
        AgentAssetActionKind::Open,
        AgentAssetActionKind::Reveal,
        AgentAssetActionKind::Remove,
    ];
    match asset.effective_state {
        AgentAssetState::Enabled => expected.push(AgentAssetActionKind::Disable),
        AgentAssetState::Disabled => expected.push(AgentAssetActionKind::Enable),
        _ => {}
    }
    assert_eq!(asset.actions.len(), expected.len(), "{}", asset.stable_id);
    for action_kind in expected {
        let matching = asset
            .actions
            .iter()
            .filter(|action| action.action == action_kind)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{}: {action_kind:?}", asset.stable_id);
        let action = matching[0];
        match action_kind {
            AgentAssetActionKind::Enable
            | AgentAssetActionKind::Disable
            | AgentAssetActionKind::Remove => {
                assert!(!action.available, "{}: {action_kind:?}", asset.stable_id);
                if action_kind == AgentAssetActionKind::Remove {
                    // Removal has independent ownership/install support gates.
                    assert!(
                        action.reason.is_some(),
                        "{}: {action_kind:?}",
                        asset.stable_id
                    );
                } else {
                    assert_eq!(
                        action.reason,
                        Some(mutation_reason),
                        "{}: {action_kind:?}",
                        asset.stable_id
                    );
                }
                assert!(action.confirmation_required);
                assert!(action.mechanism_id.is_none());
                assert!(action.selected_installation_id.is_none());
                assert!(action.reload_effect.is_none());
                assert!(action.trust_effect.is_none());
            }
            AgentAssetActionKind::Inspect
            | AgentAssetActionKind::Preview
            | AgentAssetActionKind::Open
            | AgentAssetActionKind::Reveal => {
                assert!(action.available, "{}: {action_kind:?}", asset.stable_id);
                assert!(
                    action.reason.is_none(),
                    "{}: {action_kind:?}",
                    asset.stable_id
                );
                assert_eq!(
                    action.confirmation_required,
                    matches!(
                        action_kind,
                        AgentAssetActionKind::Open | AgentAssetActionKind::Reveal
                    )
                );
            }
        }
    }
}

fn assert_claude_policy_declaration(
    inventory: &crate::models::AgentEnvironmentInventory,
    reference: &Option<crate::models::AgentAssetPolicyReference>,
    category: AgentAssetCategory,
    source_id: &str,
    declaration_key: &str,
) {
    let Some(crate::models::AgentAssetPolicyReference::Declaration { declaration_id }) = reference
    else {
        panic!("expected declaration owner for {declaration_key}: {reference:?}");
    };
    let declaration = inventory
        .declarations
        .iter()
        .find(|declaration| &declaration.id == declaration_id)
        .expect("real Claude policy declaration");
    assert_eq!(declaration.native_kind, category);
    assert_eq!(declaration.source_id, source_id);
    assert_eq!(declaration.declaration_key, declaration_key);
    assert_eq!(declaration.role, AgentAssetDeclarationRole::PolicyOverlay);
    assert_eq!(
        declaration.participation,
        AgentAssetResolutionParticipation::Participates
    );
}

struct HookAssetExpectation<'a> {
    declared: AgentAssetDeclaredState,
    effective: AgentAssetState,
    participation: AgentAssetResolutionParticipation,
    contributors: &'a [String],
    resolution: AgentAssetResolutionRelation,
    control_source: Option<(&'a str, &'a str)>,
    mutation_reason: AgentAssetActionUnavailableReason,
}

fn assert_hook_asset_binding(
    inventory: &crate::models::AgentEnvironmentInventory,
    native_id: &str,
    expected: HookAssetExpectation<'_>,
) {
    let declarations = claude_declarations(inventory, AgentAssetCategory::Hook, native_id);
    assert_eq!(declarations.len(), 1, "{native_id}: {inventory:?}");
    let declaration = declarations[0];
    let assets = claude_assets(inventory, AgentAssetCategory::Hook, native_id);
    assert_eq!(assets.len(), 1, "{native_id}: {inventory:?}");
    let asset = assets[0];
    assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
    assert_eq!(declaration.native_id, native_id);
    assert_eq!(declaration.declared_state, expected.declared);
    assert_eq!(declaration.participation, expected.participation);
    let expected_asset_declared = match expected.declared {
        AgentAssetDeclaredState::Enabled => AgentAssetState::Enabled,
        AgentAssetDeclaredState::Disabled => AgentAssetState::Disabled,
        AgentAssetDeclaredState::Rejected => AgentAssetState::Blocked,
        AgentAssetDeclaredState::Pending | AgentAssetDeclaredState::Unknown => {
            AgentAssetState::Unknown
        }
    };
    assert_eq!(
        asset.declared_state, expected_asset_declared,
        "{native_id}: asset declared state"
    );
    assert_eq!(asset.effective_state, expected.effective);
    assert_eq!(asset.resolution.relation, expected.resolution);
    match expected.control_source {
        Some((source_id, declaration_key)) => assert_claude_policy_declaration(
            inventory,
            &asset.resolution.control_source,
            AgentAssetCategory::Hook,
            source_id,
            declaration_key,
        ),
        None => assert!(asset.resolution.control_source.is_none()),
    }
    let expected_hook_enabled = expected.declared;
    assert!(matches!(
        asset.details,
        AgentAssetDetails::Hook { enabled, .. } if enabled == expected_hook_enabled
    ));
    assert_eq!(
        asset.represented_declaration_ids,
        vec![declaration.id.clone()]
    );
    assert_eq!(asset.source_ids, vec![declaration.source_id.clone()]);
    assert_eq!(asset.inspection_source_id, declaration.source_id);
    assert_eq!(asset.revision, declaration.evidence.revision);
    let mut actual_contributors = asset.resolution.contributor_ids.clone();
    actual_contributors.sort();
    let mut expected_contributors = expected.contributors.to_vec();
    expected_contributors.sort();
    assert_eq!(actual_contributors, expected_contributors);
    assert!(asset.resolution.winner_id.is_none());
    assert!(asset.relationships.provided_by.is_none());
    assert!(asset.relationships.action_owner.is_none());
    assert!(asset.relationships.affected_asset_ids.is_empty());
    if expected_contributors.is_empty() {
        assert!(asset.resolution.contributor_ids.is_empty());
    }
    assert_read_only_asset_actions(asset, expected.mutation_reason);
}

struct HookPolicyExpectation<'a> {
    resolution: AgentAssetResolutionRelation,
    terminal: Option<crate::models::AgentAssetResolutionTerminal>,
    effective: AgentAssetState,
    control_source_id: Option<(&'a str, &'a str)>,
    resolution_diagnostics: &'a [AgentAssetDiagnostic],
    source_diagnostic: Option<&'a AgentAssetDiagnostic>,
    managed: bool,
    hook_enabled: AgentAssetDeclaredState,
}

fn assert_hook_policy_binding(
    inventory: &crate::models::AgentEnvironmentInventory,
    native_id: &str,
    expected: HookPolicyExpectation<'_>,
) {
    let declaration = claude_declarations(inventory, AgentAssetCategory::Hook, native_id)
        .into_iter()
        .next()
        .expect("policy Hook declaration");
    let asset = claude_assets(inventory, AgentAssetCategory::Hook, native_id)
        .into_iter()
        .next()
        .expect("policy Hook asset");
    assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
    assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Enabled);
    assert!(matches!(
        declaration.participation,
        AgentAssetResolutionParticipation::Participates
    ));
    assert_eq!(
        asset.represented_declaration_ids,
        vec![declaration.id.clone()]
    );
    assert_eq!(
        asset.resolution.contributor_ids,
        vec![declaration.id.clone()]
    );
    assert_eq!(asset.source_ids, vec![declaration.source_id.clone()]);
    assert_eq!(asset.inspection_source_id, declaration.source_id);
    assert_eq!(asset.revision, declaration.evidence.revision);
    assert_eq!(
        asset.declared_state,
        if expected.hook_enabled == AgentAssetDeclaredState::Unknown {
            AgentAssetState::Unknown
        } else {
            AgentAssetState::Enabled
        },
        "{native_id}: policy asset declared state"
    );
    assert_eq!(asset.effective_state, expected.effective);
    assert_eq!(asset.resolution.relation, expected.resolution);
    assert_eq!(asset.resolution.terminal, expected.terminal);
    assert!(asset.resolution.winner_id.is_none());
    match expected.control_source_id {
        Some((source_id, declaration_key)) => assert_claude_policy_declaration(
            inventory,
            &asset.resolution.control_source,
            AgentAssetCategory::Hook,
            source_id,
            declaration_key,
        ),
        None => assert!(asset.resolution.control_source.is_none()),
    }
    assert_eq!(
        asset.resolution.diagnostics,
        expected.resolution_diagnostics
    );
    assert!(asset.diagnostics.is_empty());
    assert!(asset.relationships.provided_by.is_none());
    assert!(asset.relationships.action_owner.is_none());
    assert!(asset.relationships.affected_asset_ids.is_empty());
    assert!(matches!(
        asset.details,
        AgentAssetDetails::Hook {
            managed,
            enabled,
            ..
        } if managed == expected.managed && enabled == expected.hook_enabled
    ));
    assert_read_only_asset_actions(
        asset,
        match expected.terminal {
            Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
                AgentAssetActionUnavailableReason::PolicyBlocked
            }
            Some(crate::models::AgentAssetResolutionTerminal::Unknown) => {
                AgentAssetActionUnavailableReason::Unknown
            }
            None => AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    if let Some(expected_source_diagnostic) = expected.source_diagnostic {
        let matching_diagnostics = inventory
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| *diagnostic == expected_source_diagnostic)
            .count();
        assert!(
            matching_diagnostics == 1,
            "expected exactly one source diagnostic {expected_source_diagnostic:?}: {inventory:?}"
        );
    }
}

struct StatusAssetExpectation<'a> {
    mode: AgentStatusUiMode,
    declared: AgentAssetDeclaredState,
    effective: AgentAssetState,
    resolution: AgentAssetResolutionRelation,
    terminal: Option<crate::models::AgentAssetResolutionTerminal>,
    contributors: &'a [String],
    represented: &'a [String],
    control_source: Option<(&'a str, &'a str)>,
    resolution_diagnostics: &'a [AgentAssetDiagnostic],
    mutation_reason: AgentAssetActionUnavailableReason,
}

fn assert_status_asset_binding(
    inventory: &crate::models::AgentEnvironmentInventory,
    expected: StatusAssetExpectation<'_>,
) {
    let assets = claude_assets(inventory, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(assets.len(), 1, "{inventory:?}");
    let asset = assets[0];
    let declarations = claude_declarations(inventory, AgentAssetCategory::StatusUi, "status-line");
    assert!(!declarations.is_empty(), "{inventory:?}");
    let declaration = declarations
        .iter()
        .find(|declaration| declaration.source_id == asset.inspection_source_id)
        .copied()
        .expect("StatusLine inspection declaration");
    assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
    assert_eq!(declaration.declared_state, expected.declared);
    assert_eq!(
        asset.declared_state,
        match expected.declared {
            AgentAssetDeclaredState::Enabled => AgentAssetState::Enabled,
            AgentAssetDeclaredState::Disabled => AgentAssetState::Disabled,
            AgentAssetDeclaredState::Rejected => AgentAssetState::Blocked,
            AgentAssetDeclaredState::Pending | AgentAssetDeclaredState::Unknown => {
                AgentAssetState::Unknown
            }
        }
    );
    assert_eq!(asset.effective_state, expected.effective);
    assert_eq!(asset.resolution.relation, expected.resolution);
    assert_eq!(asset.resolution.terminal, expected.terminal);
    let mut actual_contributors = asset.resolution.contributor_ids.clone();
    actual_contributors.sort();
    let mut expected_contributors = expected.contributors.to_vec();
    expected_contributors.sort();
    assert_eq!(actual_contributors, expected_contributors);
    let mut actual_represented = asset.represented_declaration_ids.clone();
    actual_represented.sort();
    let mut expected_represented = expected.represented.to_vec();
    expected_represented.sort();
    assert_eq!(actual_represented, expected_represented);
    assert_eq!(asset.source_ids, vec![declaration.source_id.clone()]);
    assert_eq!(asset.inspection_source_id, declaration.source_id);
    assert_eq!(asset.revision, declaration.evidence.revision);
    assert_eq!(asset.resolution.winner_id, None);
    assert_eq!(asset.relationships.provided_by, None);
    assert_eq!(asset.relationships.action_owner, None);
    assert!(asset.relationships.affected_asset_ids.is_empty());
    assert_eq!(
        asset.resolution.diagnostics,
        expected.resolution_diagnostics
    );
    assert!(asset.diagnostics.is_empty());
    match expected.control_source {
        Some((source_id, declaration_key)) => assert_claude_policy_declaration(
            inventory,
            &asset.resolution.control_source,
            AgentAssetCategory::StatusUi,
            source_id,
            declaration_key,
        ),
        None => assert!(asset.resolution.control_source.is_none()),
    }
    assert!(matches!(
        asset.details,
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } if mode == expected.mode && command_present == (expected.mode == AgentStatusUiMode::Command)
    ));
    assert_read_only_asset_actions(asset, expected.mutation_reason);
}

fn claude_public_signature(inventory: &crate::models::AgentEnvironmentInventory) -> Vec<String> {
    let mut signature = inventory
        .contexts
        .iter()
        .map(|context| format!("context:{:?}", (context.id.as_str(), context.trust_context)))
        .chain(inventory.declarations.iter().map(|declaration| {
            format!(
                "declaration:{:?}",
                (
                    declaration.id.as_str(),
                    declaration.source_id.as_str(),
                    declaration.native_kind,
                    declaration.native_id.as_str(),
                    declaration.declaration_key.as_str(),
                    declaration.role,
                    declaration.participation,
                    declaration.declared_state,
                )
            )
        }))
        .chain(inventory.assets.iter().map(|asset| {
            let mut source_ids = asset.source_ids.clone();
            source_ids.sort();
            let mut represented = asset.represented_declaration_ids.clone();
            represented.sort();
            let mut contributors = asset.resolution.contributor_ids.clone();
            contributors.sort();
            format!(
                "asset:{:?}",
                (
                    asset.stable_id.as_str(),
                    asset.category,
                    asset.native_id.as_str(),
                    source_ids,
                    asset.inspection_source_id.as_str(),
                    asset.declared_state,
                    asset.effective_state,
                    represented,
                    asset.resolution.relation,
                    contributors,
                    asset.resolution.winner_id.as_deref(),
                    asset.resolution.control_source.as_ref(),
                )
            )
        }))
        .collect::<Vec<_>>();
    signature.sort();
    signature
}

fn claude_full_public_signature(
    inventory: &crate::models::AgentEnvironmentInventory,
) -> Vec<String> {
    let mut signature = inventory
        .sources
        .iter()
        .map(|source| {
            format!(
                "source:{:?}",
                (
                    source.id.as_str(),
                    source.scope,
                    source.precedence,
                    source.revision.identity.as_str(),
                    &source.diagnostics,
                )
            )
        })
        .chain(inventory.declarations.iter().map(|declaration| {
            format!(
                "declaration:{:?}",
                (
                    declaration.id.as_str(),
                    declaration.source_id.as_str(),
                    declaration.scope,
                    declaration.precedence,
                    declaration.native_kind,
                    declaration.native_id.as_str(),
                    declaration.declaration_key.as_str(),
                    declaration.role,
                    declaration.participation,
                    declaration.declared_state,
                    &declaration.evidence.facts,
                    &declaration.diagnostics,
                )
            )
        }))
        .chain(inventory.assets.iter().map(|asset| {
            let mut source_ids = asset.source_ids.clone();
            source_ids.sort();
            let mut represented = asset.represented_declaration_ids.clone();
            represented.sort();
            let mut contributors = asset.resolution.contributor_ids.clone();
            contributors.sort();
            format!(
                "asset:{:?}:{:?}",
                (
                    asset.stable_id.as_str(),
                    asset.category,
                    asset.native_id.as_str(),
                    asset.scope,
                    asset.precedence,
                    source_ids,
                    asset.inspection_source_id.as_str(),
                    asset.declared_state,
                    asset.effective_state,
                    represented,
                    asset.resolution.relation,
                    contributors,
                ),
                (
                    &asset.details,
                    asset.resolution.winner_id.as_ref(),
                    asset.resolution.control_source.as_ref(),
                    &asset.relationships,
                    &asset.diagnostics,
                )
            )
        }))
        .collect::<Vec<_>>();
    signature.sort();
    signature
}

fn claude_non_record_diagnostic_signature(
    inventory: &crate::models::AgentEnvironmentInventory,
) -> Vec<String> {
    let mut signature = inventory
        .diagnostics
        .iter()
        .map(|diagnostic| format!("inventory:{diagnostic:?}"))
        .chain(inventory.installations.iter().flat_map(|installation| {
            installation
                .diagnostics
                .iter()
                .map(|diagnostic| format!("installation:{}:{diagnostic:?}", installation.id))
        }))
        .chain(inventory.assets.iter().flat_map(|asset| {
            asset
                .resolution
                .diagnostics
                .iter()
                .map(|diagnostic| format!("resolution:{}:{diagnostic:?}", asset.stable_id))
        }))
        .collect::<Vec<_>>();
    signature.sort();
    signature
}

fn remove_claude_fixture(root: PathBuf) {
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_global_state_path_respects_config_root() {
    let default_home = test_root("claude-global-state-path-default");
    let (default_home, workspace, inventory, state) = build_claude_inventory_at_root(
        default_home,
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            ("settings", claude_json(serde_json::json!({}))),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_claude_source_callback_stops_after_break(&default_home, &workspace);
    let account = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with(".claude.json"))
        .expect("Claude account source");
    assert_eq!(
        account.path,
        default_home.join(".claude.json").to_string_lossy()
    );
    assert_ne!(
        account.path,
        default_home.join(".claude/.claude.json").to_string_lossy()
    );
    let state = state.lock().expect("Claude global state snapshot state");
    let account_paths = state
        .snapshot_paths
        .iter()
        .filter(|(key, _)| key == "account")
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    assert_eq!(account_paths, vec![default_home.join(".claude.json")]);
    drop(state);
    let first_signature = claude_public_signature(&inventory);
    let first_context_id = inventory.contexts[0].id.clone();
    drop(inventory);

    let profile_root = default_home.join("custom-profile");
    fs::create_dir_all(&profile_root).unwrap();
    let (custom_port, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.262",
        AgentInstallationChannel::Stable,
    )]);
    let custom_settings = crate::models::AppSettings::default();
    let custom_config_root_guard = ClaudeConfigRootOverrideGuard::new(profile_root.clone());
    let (custom_root, custom_workspace, custom_inventory, custom_state) =
        build_claude_inventory_at_root_with_workspace(
            default_home.clone(),
            [(
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            )],
            [],
            [],
            &custom_port,
            None,
            Some(&custom_settings),
        );
    drop(custom_config_root_guard);
    assert_eq!(custom_root, default_home);
    assert_eq!(custom_workspace, workspace);
    assert_eq!(custom_inventory.contexts.len(), 1);
    assert_ne!(custom_inventory.contexts[0].id, first_context_id);
    assert_eq!(
        custom_inventory.contexts[0].config_root,
        profile_root.to_string_lossy().as_ref()
    );
    let custom_account = custom_inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with(".claude.json"))
        .expect("custom Claude account source");
    assert_eq!(
        custom_account.path,
        profile_root.join(".claude.json").to_string_lossy()
    );
    assert_ne!(
        custom_account.path,
        default_home.join(".claude.json").to_string_lossy()
    );
    let custom_paths = custom_state
        .lock()
        .expect("custom Claude snapshot state")
        .snapshot_paths
        .iter()
        .filter(|(key, _)| key == "account")
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    assert_eq!(custom_paths, vec![profile_root.join(".claude.json")]);
    assert!(!custom_paths.contains(&default_home.join(".claude.json")));
    let custom_context_id = custom_inventory.contexts[0].id.clone();
    let custom_declaration_id =
        claude_declarations(&custom_inventory, AgentAssetCategory::Mcp, "same")[0]
            .id
            .clone();
    let custom_asset_id = claude_assets(&custom_inventory, AgentAssetCategory::Mcp, "same")[0]
        .stable_id
        .clone();
    let custom_signature = claude_public_signature(&custom_inventory);
    let custom_revision = custom_account.revision.identity.clone();
    let custom_parser_facts =
        claude_declarations(&custom_inventory, AgentAssetCategory::Mcp, "same")[0]
            .evidence
            .facts
            .clone();
    assert_eq!(
        claude_declarations(&custom_inventory, AgentAssetCategory::Mcp, "same").len(),
        1
    );
    assert_eq!(
        claude_assets(&custom_inventory, AgentAssetCategory::Mcp, "same").len(),
        1
    );
    drop(custom_inventory);

    let (changed_port, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.263",
        AgentInstallationChannel::Preview,
    )]);
    let changed_settings = crate::models::AppSettings::default();
    let changed_config_root_guard = ClaudeConfigRootOverrideGuard::new(profile_root.clone());
    let (_, changed_workspace, changed, changed_state) =
        build_claude_inventory_at_root_with_workspace(
            default_home.clone(),
            [(
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "type": "sse", "url": "https://example.invalid/mcp" } }
                })),
            )],
            [],
            [],
            &changed_port,
            None,
            Some(&changed_settings),
        );
    drop(changed_config_root_guard);
    assert_eq!(changed_workspace, workspace);
    let changed_account = changed
        .sources
        .iter()
        .find(|source| source.path.ends_with(".claude.json"))
        .expect("changed Claude account source");
    assert_eq!(
        changed_account.path,
        profile_root.join(".claude.json").to_string_lossy()
    );
    assert_ne!(changed_account.revision.identity, custom_revision);
    assert_eq!(changed.contexts[0].id, custom_context_id);
    assert_eq!(
        claude_declarations(&changed, AgentAssetCategory::Mcp, "same")[0].id,
        custom_declaration_id
    );
    assert_eq!(
        claude_assets(&changed, AgentAssetCategory::Mcp, "same")[0].stable_id,
        custom_asset_id
    );
    assert_eq!(claude_public_signature(&changed), custom_signature);
    assert_eq!(
        changed.installations[0].installed_version.as_deref(),
        Some("2.1.263")
    );
    assert_eq!(
        changed.installations[0].channel,
        AgentInstallationChannel::Preview
    );
    assert_eq!(changed.contexts[0].parser_version, 4);
    assert_ne!(
        claude_declarations(&changed, AgentAssetCategory::Mcp, "same")[0]
            .evidence
            .facts,
        custom_parser_facts
    );
    let changed_state = changed_state.lock().expect("changed Claude snapshot state");
    assert_eq!(
        changed_state
            .snapshot_paths
            .iter()
            .filter(|(key, _)| key == "account")
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>(),
        vec![profile_root.join(".claude.json")]
    );
    drop(changed_state);

    let (_, _, reversed, _) = build_claude_inventory_at_root(
        default_home.clone(),
        [
            ("settings", claude_json(serde_json::json!({}))),
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(claude_public_signature(&reversed), first_signature);
    remove_claude_fixture(default_home);
}

#[test]
fn claude_workspace_trust_is_exact_and_fail_closed() {
    let (root, _workspace, trusted, _) = build_claude_inventory(
        "claude-trust-true",
        [(
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": true,
                    "enabledMcpServers": ["orphan-mcp"]
                } }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(trusted.contexts[0].trust_context, AgentTrustState::Trusted);
    remove_claude_fixture(root);

    let (root, workspace, unknown, _) = build_claude_inventory(
        "claude-trust-parent-only",
        [(
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__/child": { "hasTrustDialogAccepted": true } }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(unknown.contexts[0].trust_context, AgentTrustState::Unknown);
    assert!(!workspace.as_os_str().is_empty());
    remove_claude_fixture(root);

    let cases = vec![
        (
            "claude-trust-false",
            Some(claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
            }))),
            None,
            false,
            false,
            AgentTrustState::Untrusted,
            0,
        ),
        (
            "claude-trust-lexical-true",
            Some(claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__/.": { "hasTrustDialogAccepted": true } }
            }))),
            None,
            false,
            true,
            AgentTrustState::Trusted,
            0,
        ),
        (
            "claude-trust-missing-entry",
            Some(claude_json(serde_json::json!({ "projects": {} }))),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            0,
        ),
        (
            "claude-trust-missing-field",
            Some(claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {} }
            }))),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            0,
        ),
        (
            "claude-trust-wrong-root",
            Some(b"[]".to_vec()),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            2,
        ),
        (
            "claude-trust-wrong-projects",
            Some(claude_json(serde_json::json!({ "projects": [] }))),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            1,
        ),
        (
            "claude-trust-wrong-entry",
            Some(claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": [] }
            }))),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            1,
        ),
        (
            "claude-trust-wrong-field",
            Some(claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": "true" } }
            }))),
            None,
            false,
            false,
            AgentTrustState::Unknown,
            1,
        ),
        (
            "claude-trust-ambiguous-canonical-lexical",
            Some(claude_json(serde_json::json!({
                "projects": {
                    "__WORKSPACE__": { "hasTrustDialogAccepted": true },
                    "__WORKSPACE__/.": { "hasTrustDialogAccepted": false }
                }
            }))),
            None,
            false,
            true,
            AgentTrustState::Unknown,
            0,
        ),
        (
            "claude-trust-missing-file",
            None,
            None,
            false,
            false,
            AgentTrustState::Unknown,
            0,
        ),
        (
            "claude-trust-blocked",
            None,
            Some(AgentAssetDiagnostic::ReadFailed {
                source_id: "fixture-secret".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
            }),
            false,
            false,
            AgentTrustState::Unknown,
            0,
        ),
        (
            "claude-trust-directory",
            None,
            None,
            true,
            false,
            AgentTrustState::Unknown,
            1,
        ),
    ];
    for (name, account, blocked, directory, lexical, expected, malformed_count) in cases {
        let (root, workspace, inventory, _state) =
            build_claude_trust_case(name, account, blocked, directory, lexical);
        assert_eq!(inventory.contexts[0].trust_context, expected, "{name}");
        let account_source = inventory
            .sources
            .iter()
            .find(|source| source.label == "Claude Code 账户与 MCP 状态")
            .expect("account source");
        let malformed = inventory
            .diagnostics
            .iter()
            .chain(account_source.diagnostics.iter())
            .filter(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. }))
            .count();
        assert_eq!(malformed, malformed_count, "{name}");
        assert!(inventory
            .diagnostics
            .iter()
            .chain(account_source.diagnostics.iter())
            .all(|diagnostic| {
                let serialized = serde_json::to_string(diagnostic).unwrap();
                !serialized.contains(
                    serde_json::to_string(&workspace.to_string_lossy())
                        .unwrap()
                        .trim_matches('"'),
                ) && !serialized.contains("fixture-secret")
            }));
        if name == "claude-trust-blocked" {
            assert_eq!(account_source.diagnostics.len(), 1);
            assert!(matches!(
                &account_source.diagnostics[0],
                AgentAssetDiagnostic::ReadFailed { source_id, .. } if source_id == &account_source.id
            ));
        }
        if name == "claude-trust-ambiguous-canonical-lexical" {
            assert!(claude_declarations(&inventory, AgentAssetCategory::Mcp, "local").is_empty());
            assert!(inventory.diagnostics.is_empty());
        }
        assert_claude_source_callback_stops_after_break(&root, &workspace);
        remove_claude_fixture(root);
    }

    let (root, _, first, _) = build_claude_inventory(
        "claude-trust-reversal-a",
        [(
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let (_, _, second, _) = build_claude_inventory_at_root(
        root.clone(),
        [(
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        first.contexts[0].trust_context,
        second.contexts[0].trust_context
    );
    assert_eq!(
        claude_public_signature(&first),
        claude_public_signature(&second)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_missing_account_snapshot_is_unknown_without_malformed_diagnostic() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-trust-missing-account",
        [],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    let diagnostics = inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| source.diagnostics.iter()),
        )
        .filter(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. }))
        .count();
    assert_eq!(diagnostics, 0);
    remove_claude_fixture(root);
}

#[test]
fn claude_authority_snapshot_is_reused() {
    let account_bytes = claude_json(serde_json::json!({
        "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true, "mcpServers": { "local": { "command": "node" } } } }
    }));
    let (root, workspace, inventory, state) = build_claude_inventory(
        "claude-authority-reuse",
        [
            ("account", account_bytes.clone()),
            ("settings", b"{".to_vec()),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Trusted
    );
    let local = claude_declarations(&inventory, AgentAssetCategory::Mcp, "local");
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].scope, AgentAssetScope::Local);
    assert_eq!(local[0].precedence, 30);
    let account_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 账户与 MCP 状态")
        .expect("Claude authority account source");
    assert_eq!(local[0].source_id, account_source.id);
    let state = state.lock().expect("Claude authority snapshot state");
    assert_eq!(
        state
            .native_source_keys
            .iter()
            .filter(|key| *key == "account")
            .count(),
        1
    );
    let account_attempt_indices = state
        .native_source_keys
        .iter()
        .enumerate()
        .filter(|(_, key)| *key == "account")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(account_attempt_indices.len(), 1);
    let provisional_account_id = &state.attempts[account_attempt_indices[0]];
    let account_opens = state
        .opens
        .iter()
        .filter(|source_id| *source_id == provisional_account_id)
        .count();
    let account_charges = state
        .charges
        .iter()
        .filter(|(source_id, _)| source_id == provisional_account_id)
        .cloned()
        .collect::<Vec<_>>();
    let account_reads = state
        .snapshots
        .iter()
        .filter(|(source_id, _, _)| source_id == provisional_account_id)
        .collect::<Vec<_>>();
    assert_eq!(account_reads.len(), 1);
    let account_read = account_reads[0];
    assert_eq!(account_opens, 1);
    assert_eq!(
        account_charges,
        vec![(provisional_account_id.clone(), account_read.2.len())]
    );
    assert_eq!(account_read.1, local[0].evidence.revision.identity);
    let expected_account_bytes = String::from_utf8_lossy(&account_bytes)
        .replace(
            "__WORKSPACE__",
            serde_json::to_string(&workspace.to_string_lossy())
                .unwrap()
                .trim_matches('"'),
        )
        .into_bytes();
    assert_eq!(account_read.2.as_slice(), expected_account_bytes.as_slice());
    assert_eq!(
        account_source.revision.identity,
        local[0].evidence.revision.identity
    );
    drop(state);
    let settings_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("Claude settings source");
    let malformed_source_diagnostics = inventory
        .sources
        .iter()
        .flat_map(|source| source.diagnostics.iter())
        .filter(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. }))
        .count();
    assert_eq!(malformed_source_diagnostics, 1);
    assert!(account_source.diagnostics.is_empty());
    assert_eq!(settings_source.diagnostics.len(), 2);
    assert!(settings_source
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed {
                format: crate::models::AgentAssetDocumentFormat::Json,
                location: Some(_)
            }
        )));
    assert!(settings_source
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: AgentCliKind::ClaudeCode,
                category: AgentAssetCategory::Hook,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            }
        )));
    assert!(!workspace.as_os_str().is_empty());
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    let first_signature = claude_public_signature(&inventory);
    drop(inventory);
    let (_, _, reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        [("settings", b"{".to_vec()), ("account", account_bytes)],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(claude_public_signature(&reversed), first_signature);
    remove_claude_fixture(root.clone());
}

#[test]
fn claude_untrusted_workspace_is_suppressed() {
    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-untrusted-workspace",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "user": { "command": "user" } },
                    "projects": { "__WORKSPACE__": {
                        "hasTrustDialogAccepted": false,
                        "mcpServers": { "local": { "command": "local" } },
                        "disabledMcpServers": ["local"]
                    } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(
                    serde_json::json!({ "mcpServers": { "workspace": { "command": "workspace" } } }),
                ),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["workspace"]
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["workspace"],
                    "enabledPlugins": { "workspace-plugin": true }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "disabledMcpServers": ["workspace"]
                })),
            ),
            (
                "plugin-registry",
                claude_json(serde_json::json!({
                    "version": 2,
                    "plugins": {
                        "workspace-plugin": [{
                            "scope": "project",
                            "projectPath": "__WORKSPACE__",
                            "installPath": "/fixture/workspace-plugin"
                        }]
                    }
                })),
            ),
            ("managed-settings", claude_json(serde_json::json!({}))),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
            (
                "skill-manifest:workspace-skills:skill",
                b"---\nname: Workspace\n---\nbody".to_vec(),
            ),
        ],
        [(
            "workspace-skills",
            claude_entries([("skill", AgentAssetSourceKind::Directory, false)]),
        )],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(workspace, workspace.canonicalize().unwrap());
    let suppressed = |category, native_id| {
        claude_declarations(&inventory, category, native_id)
            .into_iter()
            .filter(|item| {
                matches!(
                    item.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                    }
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(
        claude_declarations(&inventory, AgentAssetCategory::Mcp, "user")
            .iter()
            .all(|item| matches!(
                item.participation,
                AgentAssetResolutionParticipation::Participates
            ))
    );
    for id in ["local", "workspace"] {
        assert!(!suppressed(AgentAssetCategory::Mcp, id).is_empty());
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, id);
        assert!(declarations
            .iter()
            .filter(|item| item.role == AgentAssetDeclarationRole::Definition)
            .all(|item| matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            )));
        assert!(declarations.iter().all(|item| {
            inventory
                .sources
                .iter()
                .any(|source| source.id == item.source_id && !source.revision.identity.is_empty())
        }));
    }
    assert!(
        claude_declarations(&inventory, AgentAssetCategory::Skill, "skill")
            .iter()
            .all(|item| matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            ))
    );
    assert!(
        claude_declarations(&inventory, AgentAssetCategory::Plugin, "workspace-plugin")
            .iter()
            .all(|item| matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            ))
    );
    let managed = claude_assets(&inventory, AgentAssetCategory::Mcp, "managed");
    assert_eq!(
        managed.len(),
        1,
        "diagnostics={:?} declarations={:?}",
        inventory.diagnostics,
        claude_declarations(&inventory, AgentAssetCategory::Mcp, "managed")
    );
    assert_eq!(
        managed[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(
        claude_declarations(&inventory, AgentAssetCategory::Mcp, "managed")
            .iter()
            .all(|item| matches!(
                item.participation,
                AgentAssetResolutionParticipation::Participates
            ))
    );
    for id in ["local", "workspace"] {
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, id);
        let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, id);
        assert_eq!(assets.len(), 1, "suppressed MCP {id}");
        assert_eq!(
            assets[0].resolution.relation,
            AgentAssetResolutionRelation::Unknown
        );
        assert_eq!(
            assets[0].resolution.terminal,
            Some(AgentAssetResolutionTerminal::Unknown)
        );
        assert!(assets[0].resolution.contributor_ids.is_empty());
        assert!(assets[0].resolution.winner_id.is_none());
        assert!(assets[0].resolution.control_source.is_none());
        assert!(!assets[0].source_ids.is_empty());
        assert!(assets[0]
            .source_ids
            .contains(&assets[0].inspection_source_id));
        assert!(!assets[0].revision.identity.is_empty());
        let mut represented = assets[0].represented_declaration_ids.clone();
        represented.sort();
        let mut declaration_ids = declarations
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect::<Vec<_>>();
        declaration_ids.sort();
        assert_eq!(represented, declaration_ids);
        assert_eq!(assets[0].declared_state, AgentAssetState::Unknown);
        assert_eq!(assets[0].effective_state, AgentAssetState::Unknown);
        assert!(matches!(
            assets[0].details,
            AgentAssetDetails::Mcp {
                declared_state: AgentAssetDeclaredState::Unknown,
                effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
                ..
            }
        ));
    }
    for (category, id) in [
        (AgentAssetCategory::Skill, "skill"),
        (AgentAssetCategory::Plugin, "workspace-plugin"),
    ] {
        assert!(claude_assets(&inventory, category, id).is_empty());
    }
    let unknown_account = serde_json::to_vec(&serde_json::json!({
        "projects": { "__WORKSPACE__/parent": { "hasTrustDialogAccepted": true } }
    }))
    .unwrap();
    let (unknown_matrix_root, unknown_matrix_workspace, unknown_matrix, unknown_matrix_state) =
        build_claude_inventory(
            "claude-unknown-workspace-matrix",
            [
                (
                    "account",
                    claude_json(serde_json::json!({
                        "mcpServers": { "user": { "command": "user" } },
                        "projects": { "__WORKSPACE__": {
                            "mcpServers": { "local": { "command": "local" } },
                            "disabledMcpServers": ["local"],
                            "enabledMcpServers": ["local"]
                        } }
                    })),
                ),
                (
                    "workspace-mcp",
                    claude_json(serde_json::json!({
                        "mcpServers": { "workspace": { "command": "workspace" } }
                    })),
                ),
                (
                    "settings",
                    claude_json(serde_json::json!({
                        "enabledMcpjsonServers": ["workspace"]
                    })),
                ),
                (
                    "workspace-settings",
                    claude_json(serde_json::json!({
                        "enabledMcpjsonServers": ["workspace"],
                        "enabledPlugins": { "workspace-plugin": true }
                    })),
                ),
                (
                    "workspace-local-settings",
                    claude_json(serde_json::json!({
                        "disabledMcpServers": ["workspace"]
                    })),
                ),
                (
                    "plugin-registry",
                    claude_json(serde_json::json!({
                        "version": 2,
                        "plugins": { "workspace-plugin": [{
                            "scope": "project",
                            "projectPath": "__WORKSPACE__",
                            "installPath": "/fixture/workspace-plugin"
                        }] }
                    })),
                ),
                ("managed-settings", claude_json(serde_json::json!({}))),
                (
                    "managed-mcp",
                    claude_json(serde_json::json!({
                        "mcpServers": { "managed": { "command": "managed" } }
                    })),
                ),
                (
                    "skill-manifest:workspace-skills:skill",
                    b"---\nname: Workspace\n---\nbody".to_vec(),
                ),
            ],
            [(
                "workspace-skills",
                claude_entries([("skill", AgentAssetSourceKind::Directory, false)]),
            )],
            [],
            &RealInstallationDiscoveryPort,
        );
    assert_eq!(
        unknown_matrix.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    let unknown_matrix_declarations = unknown_matrix
        .declarations
        .iter()
        .filter(|declaration| {
            matches!(
                declaration.native_id.as_str(),
                "workspace" | "workspace-plugin" | "skill" | "local"
            )
        })
        .collect::<Vec<_>>();
    assert!(!unknown_matrix_declarations.is_empty());
    assert_eq!(unknown_matrix_declarations.len(), 9);
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace")
            .count(),
        3
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace-plugin")
            .count(),
        2
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "skill")
            .count(),
        1
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "local")
            .count(),
        3
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace"
                && declaration.role == AgentAssetDeclarationRole::Definition)
            .count(),
        1
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay)
            .count(),
        2
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace-plugin"
                && declaration.role == AgentAssetDeclarationRole::Definition)
            .count(),
        1
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "workspace-plugin"
                && declaration.role == AgentAssetDeclarationRole::StateOverlay)
            .count(),
        1
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "local"
                && declaration.role == AgentAssetDeclarationRole::Definition)
            .count(),
        1
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| declaration.native_id == "local"
                && declaration.role == AgentAssetDeclarationRole::StateOverlay)
            .count(),
        2
    );
    assert_eq!(
        unknown_matrix_declarations
            .iter()
            .filter(|declaration| matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ))
            .count(),
        8
    );
    let participating_overlay = unknown_matrix_declarations
        .iter()
        .find(|declaration| {
            declaration.native_id == "workspace"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
                && matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Participates
                )
        })
        .expect("participating user workspace policy overlay");
    assert!(matches!(
        participating_overlay.participation,
        AgentAssetResolutionParticipation::Participates
    ));
    assert_eq!(
        unknown_matrix
            .sources
            .iter()
            .find(|source| source.id == participating_overlay.source_id)
            .expect("participating overlay source")
            .label,
        "Claude Code 设置"
    );
    assert!(unknown_matrix_declarations
        .iter()
        .filter(|declaration| declaration.source_id != participating_overlay.source_id)
        .all(|declaration| {
            matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
        }));
    for (category, native_id) in [
        (AgentAssetCategory::Mcp, "workspace"),
        (AgentAssetCategory::Plugin, "workspace-plugin"),
        (AgentAssetCategory::Skill, "skill"),
        (AgentAssetCategory::Mcp, "local"),
    ] {
        let assets = claude_assets(&unknown_matrix, category, native_id);
        assert!(
            assets.is_empty(),
            "unknown-trust suppressed {category:?} {native_id} must stay declaration-only"
        );
    }
    let user_unknown = claude_declarations(&unknown_matrix, AgentAssetCategory::Mcp, "user");
    assert_eq!(user_unknown.len(), 1);
    assert!(matches!(
        user_unknown[0].role,
        AgentAssetDeclarationRole::Definition
    ));
    assert!(matches!(
        user_unknown[0].participation,
        AgentAssetResolutionParticipation::Participates
    ));
    let managed_unknown = claude_declarations(&unknown_matrix, AgentAssetCategory::Mcp, "managed");
    assert_eq!(managed_unknown.len(), 1);
    assert!(matches!(
        managed_unknown[0].participation,
        AgentAssetResolutionParticipation::Participates
    ));
    let local_unknown = claude_declarations(&unknown_matrix, AgentAssetCategory::Mcp, "local");
    assert_eq!(local_unknown.len(), 3);
    assert!(local_unknown.iter().all(|declaration| {
        matches!(
            declaration.participation,
            AgentAssetResolutionParticipation::Suppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
            }
        )
    }));
    let account_source = unknown_matrix
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 账户与 MCP 状态")
        .expect("unknown matrix account source");
    let local_definition = local_unknown
        .iter()
        .find(|declaration| declaration.role == AgentAssetDeclarationRole::Definition)
        .expect("unknown local definition");
    assert_eq!(local_definition.source_id, account_source.id);
    assert!(local_unknown.iter().all(|declaration| {
        unknown_matrix.sources.iter().any(|source| {
            source.id == declaration.source_id && !source.revision.identity.is_empty()
        })
    }));
    assert!(local_unknown.iter().any(|declaration| {
        declaration.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::DeclarationSuppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
        })
    }));
    assert!(unknown_matrix.sources.iter().all(|source| {
        source.diagnostics.iter().all(|diagnostic| {
            !serde_json::to_string(diagnostic).unwrap().contains(
                serde_json::to_string(&unknown_matrix_workspace.to_string_lossy())
                    .unwrap()
                    .trim_matches('"'),
            )
        })
    }));
    assert_claude_source_callback_stops_after_break(
        &unknown_matrix_root,
        &unknown_matrix_workspace,
    );
    let unknown_matrix_signature = claude_public_signature(&unknown_matrix);
    drop(unknown_matrix);
    drop(unknown_matrix_state);
    let (_, _, unknown_matrix_reversed, _) = build_claude_inventory_at_root(
        unknown_matrix_root.clone(),
        [
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "user": { "command": "user" } },
                    "projects": { "__WORKSPACE__": {
                        "mcpServers": { "local": { "command": "local" } },
                        "disabledMcpServers": ["local"],
                        "enabledMcpServers": ["local"]
                    } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "workspace": { "command": "workspace" } }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["workspace"],
                    "enabledPlugins": { "workspace-plugin": true }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "disabledMcpServers": ["workspace"]
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["workspace"]
                })),
            ),
            (
                "plugin-registry",
                claude_json(serde_json::json!({
                    "version": 2,
                    "plugins": { "workspace-plugin": [{
                        "scope": "project",
                        "projectPath": "__WORKSPACE__",
                        "installPath": "/fixture/workspace-plugin"
                    }] }
                })),
            ),
            ("managed-settings", claude_json(serde_json::json!({}))),
            (
                "skill-manifest:workspace-skills:skill",
                b"---\nname: Workspace\n---\nbody".to_vec(),
            ),
        ],
        [(
            "workspace-skills",
            claude_entries([("skill", AgentAssetSourceKind::Directory, false)]),
        )],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_public_signature(&unknown_matrix_reversed),
        unknown_matrix_signature
    );
    remove_claude_fixture(unknown_matrix_root);
    let (unknown_root, _, unknown_inventory, _) = build_claude_inventory(
        "claude-unknown-workspace",
        [("account", unknown_account)],
        [("workspace-skills", claude_entries([]))],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        unknown_inventory.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    assert!(claude_declarations(&unknown_inventory, AgentAssetCategory::Mcp, "missing").is_empty());
    remove_claude_fixture(unknown_root);
    remove_claude_fixture(root);
}

#[test]
fn claude_mcp_whole_entry_precedence() {
    let root = test_root("claude-mcp-precedence");
    let (root, workspace, inventory, _) = build_claude_inventory_at_root(
        root,
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "same": {
                            "command": "user",
                            "args": ["u"],
                            "env": { "OWNER": "user" }
                        },
                        "independent": { "command": "independent" }
                    },
                    "projects": { "__WORKSPACE__": {
                        "hasTrustDialogAccepted": true,
                        "mcpServers": { "same": {
                            "command": "local",
                            "args": ["l"],
                            "env": { "OWNER": "local" }
                        } }
                    } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": {
                        "command": "workspace",
                        "args": ["w"],
                        "env": { "OWNER": "workspace" }
                    } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let same = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(same.len(), 3);
    assert!(same.iter().any(|item| item.scope == AgentAssetScope::Local));
    let records = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(records.len(), 3);
    let winner = records
        .iter()
        .find(|item| item.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("unique local replacement winner");
    let local = same
        .iter()
        .find(|item| item.scope == AgentAssetScope::Local)
        .expect("local declaration");
    assert_eq!(
        local.source_id,
        inventory
            .sources
            .iter()
            .find(|source| source.label == "Claude Code 账户与 MCP 状态")
            .expect("account source")
            .id
    );
    assert_eq!(winner.inspection_source_id, local.source_id);
    assert!(matches!(
        winner.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    ));
    let mut represented = same.iter().map(|item| item.id.clone()).collect::<Vec<_>>();
    represented.sort();
    let mut winner_represented = winner.represented_declaration_ids.clone();
    winner_represented.sort();
    let mut winner_contributors = winner.resolution.contributor_ids.clone();
    winner_contributors.sort();
    assert_eq!(winner_represented, represented);
    assert_eq!(winner_contributors, represented);
    assert_eq!(winner.resolution.winner_id, Some(winner.stable_id.clone()));
    assert!(winner.source_ids.contains(&winner.inspection_source_id));
    let account_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 账户与 MCP 状态")
        .expect("account source");
    let payload_source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "account".to_owned(),
        label: account_source.label.clone(),
        scope: account_source.scope,
        path: PathBuf::from(&account_source.path),
        allowed_root: PathBuf::from(&account_source.allowed_root),
        precedence: account_source.precedence,
        writable: account_source.writable,
        sensitive: account_source.sensitive,
        source_kind: account_source.source_kind,
        categories: account_source.categories.clone(),
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Local,
                precedence: 30,
            },
        ],
    };
    let payload_bytes = String::from_utf8_lossy(&claude_json(serde_json::json!({
        "mcpServers": {
            "same": { "command": "user", "args": ["u"], "env": { "OWNER": "user" } },
            "independent": { "command": "independent" }
        },
        "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true, "mcpServers": {
            "same": { "command": "local", "args": ["l"], "env": { "OWNER": "local" } }
        } } }
    })))
    .replace(
        "__WORKSPACE__",
        serde_json::to_string(&workspace.to_string_lossy())
            .unwrap()
            .trim_matches('"'),
    )
    .into_bytes();
    let payload_snapshot = AgentAssetSnapshot::File {
        bytes: payload_bytes,
        revision: AgentAssetRevision::default(),
    };
    let mut payloads = NativePayloadDebugCollector::default();
    definition(AgentCliKind::ClaudeCode).environment().parse(
        AgentAssetParseRequest {
            context: &inventory.contexts[0],
            source: &payload_source,
            snapshot: &payload_snapshot,
            native_home: None,
            workspace_canonical: Some(&workspace),
            workspace_lexical: Some(&workspace),
        },
        &mut payloads,
    );
    let local_payload = payloads
        .declarations
        .iter()
        .find(|declaration| {
            declaration.native_id == "same"
                && declaration.logical_origin.scope == AgentAssetScope::Local
        })
        .expect("local native MCP payload");
    assert!(matches!(
        &local_payload.native_payload,
        AgentAssetNativePayload::McpDefinition(AgentMcpDefinitionPayload {
            identity: AgentMcpMatcherIdentity::Stdio { argv },
            origin: AgentMcpDefinitionOrigin::Declared,
        })
            if argv == &["local".to_owned(), "l".to_owned()]
    ));
    let losers = records
        .iter()
        .filter(|item| item.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .collect::<Vec<_>>();
    assert_eq!(losers.len(), 2);
    assert!(losers.iter().all(|item| {
        item.represented_declaration_ids == item.resolution.contributor_ids
            && item.represented_declaration_ids.len() == 1
            && item.resolution.winner_id == winner.resolution.winner_id
            && item.stable_id != winner.stable_id
            && item.source_ids.contains(&item.inspection_source_id)
    }));
    let independent = claude_assets(&inventory, AgentAssetCategory::Mcp, "independent");
    assert_eq!(independent.len(), 1);
    assert_eq!(
        independent[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(independent[0].resolution.contributor_ids.len(), 1);
    assert_eq!(independent[0].represented_declaration_ids.len(), 1);
    assert_eq!(independent[0].scope, AgentAssetScope::User);
    assert_eq!(independent[0].inspection_source_id, account_source.id);
    assert!(matches!(
        independent[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    ));
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    let first_signature = claude_public_signature(&inventory);
    drop(inventory);

    let (root, _, reversed, _) = build_claude_inventory_at_root(
        root,
        [
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": {
                        "command": "workspace",
                        "args": ["w"],
                        "env": { "OWNER": "workspace" }
                    } }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": {
                        "hasTrustDialogAccepted": true,
                        "mcpServers": { "same": {
                            "command": "local",
                            "args": ["l"],
                            "env": { "OWNER": "local" }
                        } }
                    } },
                    "mcpServers": {
                        "independent": { "command": "independent" },
                        "same": {
                            "command": "user",
                            "args": ["u"],
                            "env": { "OWNER": "user" }
                        }
                    }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(claude_public_signature(&reversed), first_signature);
    remove_claude_fixture(root);

    let (root, workspace, tie, _) = build_claude_inventory(
        "claude-mcp-precedence-tie",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "managedMcpServers": { "same": { "command": "managed-a" } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "managed-b" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let tied = claude_assets(&tie, AgentAssetCategory::Mcp, "same");
    assert_eq!(tied.len(), 1);
    let mut tie_declaration_ids = claude_declarations(&tie, AgentAssetCategory::Mcp, "same")
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    tie_declaration_ids.sort();
    assert!(tied.iter().all(|item| {
        item.resolution.relation == AgentAssetResolutionRelation::Unknown
            && item.declared_state == AgentAssetState::Unknown
            && item.effective_state == AgentAssetState::Unknown
            && item.resolution.winner_id.is_none()
            && item.resolution.control_source.is_none()
            && item.resolution.terminal == Some(AgentAssetResolutionTerminal::Unknown)
            && item.resolution.contributor_ids.len() == 2
            && item.source_ids.contains(&item.inspection_source_id)
            && {
                let mut represented = item.represented_declaration_ids.clone();
                represented.sort();
                represented == tie_declaration_ids
            }
    }));
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    let tie_signature = claude_public_signature(&tie);
    drop(tie);
    let (_, _, tie_reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "managed-b" } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "managedMcpServers": { "same": { "command": "managed-a" } }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(claude_public_signature(&tie_reversed), tie_signature);
    remove_claude_fixture(root.clone());

    let (suppressed_root, suppressed_workspace, suppressed, _) = build_claude_inventory(
        "claude-mcp-suppressed-contributor",
        [(
            "account",
            claude_json(serde_json::json!({
                "mcpServers": { "same": { "command": "user" } },
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": false,
                    "mcpServers": { "same": { "command": "local" } }
                } }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let suppressed_declarations = claude_declarations(&suppressed, AgentAssetCategory::Mcp, "same");
    let suppressed_record = claude_assets(&suppressed, AgentAssetCategory::Mcp, "same");
    assert_eq!(suppressed_record.len(), 1);
    assert_eq!(suppressed_record[0].represented_declaration_ids.len(), 2);
    assert_eq!(suppressed_record[0].resolution.contributor_ids.len(), 1);
    assert_eq!(
        suppressed_record[0].resolution.contributor_ids[0],
        suppressed_declarations
            .iter()
            .find(|declaration| declaration.scope == AgentAssetScope::User)
            .expect("participating user contributor")
            .id
    );
    assert!(suppressed_declarations.iter().any(|declaration| matches!(
        declaration.participation,
        AgentAssetResolutionParticipation::Suppressed { .. }
    )));
    assert_claude_source_callback_stops_after_break(&suppressed_root, &suppressed_workspace);
    remove_claude_fixture(suppressed_root);
}

#[test]
fn claude_mcp_top_precedence_tie_is_qualified_and_terminal() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-mcp-top-tie",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "managedMcpServers": { "same": { "command": "managed-a" } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "managed-b" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(assets.len(), 1);
    assert!(assets.iter().all(|asset| {
        asset.resolution.relation == AgentAssetResolutionRelation::Unknown
            && asset.declared_state == AgentAssetState::Unknown
            && asset.effective_state == AgentAssetState::Unknown
            && asset.resolution.winner_id.is_none()
            && asset.resolution.control_source.is_none()
            && asset.resolution.terminal == Some(AgentAssetResolutionTerminal::Unknown)
            && asset.resolution.contributor_ids.len() == 2
            && asset.represented_declaration_ids.len() == 2
            && !asset.resolution.qualified_collision
    }));
    assert!(assets.iter().all(|asset| {
        matches!(
            asset.details,
            AgentAssetDetails::Mcp {
                effective_availability: AgentAssetEffectiveAvailability::Unknown,
                ..
            }
        )
    }));
    remove_claude_fixture(root);
}

#[test]
fn claude_mcp_approval_and_personal_state_are_independent() {
    let run_case = |name: &str,
                    settings: serde_json::Value,
                    personal_state: Option<&str>,
                    expected_approval: AgentMcpApprovalState,
                    expected_declared: AgentAssetDeclaredState,
                    expected_availability: AgentAssetEffectiveAvailability,
                    expected_resolution: AgentAssetResolutionRelation| {
        let mut project = serde_json::json!({
            "hasTrustDialogAccepted": true,
        });
        if let Some(field) = personal_state {
            project[field] = serde_json::json!(["same"]);
        }
        let (root, workspace, inventory, _) = build_claude_inventory(
            name,
            [
                (
                    "account",
                    claude_json(serde_json::json!({
                        "projects": { "__WORKSPACE__": project }
                    })),
                ),
                (
                    "workspace-mcp",
                    claude_json(serde_json::json!({
                        "mcpServers": { "same": { "command": "node" } }
                    })),
                ),
                ("settings", claude_json(settings)),
            ],
            [],
            [],
            &RealInstallationDiscoveryPort,
        );
        assert_eq!(
            inventory.contexts[0].trust_context,
            AgentTrustState::Trusted
        );
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same");
        let approval = declarations.iter().find(|item| {
            item.role == AgentAssetDeclarationRole::PolicyOverlay
                && item.declaration_key.starts_with("approval:")
                && (expected_approval == AgentMcpApprovalState::Approved
                    && item.declared_state == AgentAssetDeclaredState::Enabled
                    || expected_approval == AgentMcpApprovalState::Rejected
                        && item.declared_state == AgentAssetDeclaredState::Rejected)
        });
        if expected_approval == AgentMcpApprovalState::Pending {
            assert!(approval.is_none());
        } else if let Some(approval) = approval {
            assert_eq!(
                approval.declared_state,
                if expected_approval == AgentMcpApprovalState::Approved {
                    AgentAssetDeclaredState::Enabled
                } else {
                    AgentAssetDeclaredState::Rejected
                }
            );
        }
        let state_overlay = declarations
            .iter()
            .find(|item| item.role == AgentAssetDeclarationRole::StateOverlay);
        if personal_state.is_some() {
            if let Some(approval) = approval {
                let state_overlay = state_overlay.expect("personal state overlay");
                assert_ne!(approval.id, state_overlay.id);
                assert_ne!(
                    (approval.scope, approval.precedence),
                    (state_overlay.scope, state_overlay.precedence)
                );
            }
        }
        let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
        assert_eq!(assets.len(), 1, "{name}");
        let asset = assets[0];
        assert_eq!(
            asset.effective_state,
            match expected_availability {
                AgentAssetEffectiveAvailability::Available => AgentAssetState::Enabled,
                AgentAssetEffectiveAvailability::Disabled => AgentAssetState::Disabled,
                AgentAssetEffectiveAvailability::PolicyBlocked => AgentAssetState::Blocked,
                AgentAssetEffectiveAvailability::ApprovalRequired
                | AgentAssetEffectiveAvailability::TrustRequired
                | AgentAssetEffectiveAvailability::Unknown
                | AgentAssetEffectiveAvailability::Invalid => AgentAssetState::Unknown,
            },
            "{name}"
        );
        assert_eq!(
            asset.resolution.relation,
            if expected_resolution == AgentAssetResolutionRelation::Unknown {
                AgentAssetResolutionRelation::Independent
            } else {
                expected_resolution
            },
            "{name}"
        );
        assert_eq!(
            asset.resolution.terminal,
            (expected_resolution == AgentAssetResolutionRelation::Unknown)
                .then_some(AgentAssetResolutionTerminal::PolicyBlocked),
            "{name}"
        );
        assert!(matches!(
            asset.details,
            AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                declared_state,
                approval_state,
                effective_availability,
            } if declared_state == expected_declared
                && approval_state == expected_approval
                && effective_availability == expected_availability
        ));
        if expected_resolution == AgentAssetResolutionRelation::Unknown {
            assert!(asset.resolution.control_source.is_some());
        }
        assert!(!workspace.as_os_str().is_empty());
        remove_claude_fixture(root);
    };

    run_case(
        "claude-mcp-approval-pending",
        serde_json::json!({}),
        None,
        AgentMcpApprovalState::Pending,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::ApprovalRequired,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-exact",
        serde_json::json!({ "enabledMcpjsonServers": ["same"] }),
        None,
        AgentMcpApprovalState::Approved,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::Available,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-all",
        serde_json::json!({ "enableAllProjectMcpServers": true }),
        None,
        AgentMcpApprovalState::Approved,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::Available,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-rejected-over-approved",
        serde_json::json!({
            "enabledMcpjsonServers": ["same"],
            "disabledMcpjsonServers": ["same"]
        }),
        None,
        AgentMcpApprovalState::Rejected,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::PolicyBlocked,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-disabled-account",
        serde_json::json!({ "enabledMcpjsonServers": ["same"] }),
        Some("disabledMcpServers"),
        AgentMcpApprovalState::Approved,
        AgentAssetDeclaredState::Disabled,
        AgentAssetEffectiveAvailability::Disabled,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-rejected-enabled-account",
        serde_json::json!({ "disabledMcpjsonServers": ["same"] }),
        Some("enabledMcpServers"),
        AgentMcpApprovalState::Rejected,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::PolicyBlocked,
        AgentAssetResolutionRelation::Independent,
    );
    run_case(
        "claude-mcp-approval-policy-deny-enabled",
        serde_json::json!({
            "deniedMcpServers": [{ "serverName": "same" }]
        }),
        Some("enabledMcpServers"),
        AgentMcpApprovalState::Pending,
        AgentAssetDeclaredState::Enabled,
        AgentAssetEffectiveAvailability::PolicyBlocked,
        AgentAssetResolutionRelation::Unknown,
    );

    let (first_root, _, first, _) = build_claude_inventory(
        "claude-mcp-approval-reversal-a",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let (_, _, second, _) = build_claude_inventory_at_root(
        first_root.clone(),
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"]
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_public_signature(&first),
        claude_public_signature(&second)
    );
    remove_claude_fixture(first_root);
}

#[test]
fn claude_mcp_approval_unions_arrays_and_rejection_wins() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-mcp-approval-union",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"]
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "disabledMcpjsonServers": ["same"]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(assets.len(), 1);
    assert!(matches!(
        assets[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            declared_state: AgentAssetDeclaredState::Enabled,
            approval_state: AgentMcpApprovalState::Rejected,
            effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
        }
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_user_approval_survives_local_trust_gate() {
    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-user-approval-trust",
        [
            (
                "account",
                claude_json(
                    serde_json::json!({ "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } } }),
                ),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "same": { "command": "node" },
                        "all-only": { "command": "approve-all" }
                    }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({ "enabledMcpjsonServers": ["same"] })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"],
                    "disabledMcpjsonServers": ["same"],
                    "enableAllProjectMcpServers": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same");
    let user_settings_source_id = inventory
        .sources
        .iter()
        .find(|source| {
            source.scope == AgentAssetScope::User && source.path.ends_with("settings.json")
        })
        .expect("user settings source")
        .id
        .clone();
    let local_settings_source_id = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("settings.local.json"))
        .expect("local settings source")
        .id
        .clone();
    let user_approval = declarations
        .iter()
        .find(|item| {
            item.source_id == user_settings_source_id
                && item.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .expect("participating user approval");
    assert_eq!(user_approval.scope, AgentAssetScope::User);
    assert!(matches!(
        user_approval.participation,
        AgentAssetResolutionParticipation::Participates
    ));
    let local_approval = declarations
        .iter()
        .find(|item| {
            item.source_id == local_settings_source_id
                && item.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .expect("suppressed local approval");
    assert_eq!(local_approval.scope, AgentAssetScope::Local);
    assert!(matches!(
        local_approval.participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
        }
    ));
    let local_policy_overlays = declarations
        .iter()
        .filter(|item| item.source_id == local_settings_source_id)
        .collect::<Vec<_>>();
    assert_eq!(local_policy_overlays.len(), 2);
    assert!(local_policy_overlays.iter().all(|item| {
        item.role == AgentAssetDeclarationRole::PolicyOverlay
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }));
    let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(assets.len(), 1);
    assert_eq!(
        assets[0].resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        assets[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(assets[0].resolution.contributor_ids.is_empty());
    assert!(assets[0].resolution.winner_id.is_none());
    assert!(assets[0].resolution.control_source.is_none());
    assert_eq!(
        assets[0].inspection_source_id,
        declarations
            .iter()
            .find(|item| {
                item.role == AgentAssetDeclarationRole::Definition
                    && matches!(
                        item.participation,
                        AgentAssetResolutionParticipation::Suppressed { .. }
                    )
            })
            .expect("suppressed workspace definition")
            .source_id
    );
    let untrusted_definition_id = declarations
        .iter()
        .find(|item| item.role == AgentAssetDeclarationRole::Definition)
        .expect("untrusted workspace definition")
        .id
        .clone();
    let untrusted_inspection_source_id = assets[0].inspection_source_id.clone();
    assert!(assets[0].resolution.contributor_ids.is_empty());
    assert!(matches!(
        assets[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            approval_state: AgentMcpApprovalState::Unknown,
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    let all_only_untrusted = claude_assets(&inventory, AgentAssetCategory::Mcp, "all-only");
    assert_eq!(all_only_untrusted.len(), 1);
    assert_eq!(
        all_only_untrusted[0].resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        all_only_untrusted[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(all_only_untrusted[0].resolution.contributor_ids.is_empty());
    assert!(all_only_untrusted[0].resolution.winner_id.is_none());
    assert!(all_only_untrusted[0].resolution.control_source.is_none());
    assert!(matches!(
        all_only_untrusted[0].details,
        AgentAssetDetails::Mcp {
            approval_state: AgentMcpApprovalState::Unknown,
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    let first_signature = claude_public_signature(&inventory);
    let (_, _, reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"],
                    "disabledMcpjsonServers": ["same"],
                    "enableAllProjectMcpServers": true
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({ "enabledMcpjsonServers": ["same"] })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "same": { "command": "node" },
                        "all-only": { "command": "approve-all" }
                    }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(first_signature, claude_public_signature(&reversed));
    remove_claude_fixture(root);

    let (root, workspace, trusted, _) = build_claude_inventory(
        "claude-user-approval-trusted-local-precedence",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "workspace-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "same": { "command": "node" },
                        "all-only": { "command": "approve-all" }
                    }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({ "enabledMcpjsonServers": ["same"] })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "enabledMcpjsonServers": ["same"],
                    "disabledMcpjsonServers": ["same"],
                    "enableAllProjectMcpServers": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(trusted.contexts[0].trust_context, AgentTrustState::Trusted);
    let trusted_declarations = claude_declarations(&trusted, AgentAssetCategory::Mcp, "same");
    let trusted_local_source_id = trusted
        .sources
        .iter()
        .find(|source| source.path.ends_with("settings.local.json"))
        .expect("trusted local settings source")
        .id
        .clone();
    let trusted_local = trusted_declarations
        .iter()
        .find(|item| {
            item.source_id == trusted_local_source_id
                && item.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .expect("trusted local approval");
    let trusted_definition_id = trusted_declarations
        .iter()
        .find(|item| item.role == AgentAssetDeclarationRole::Definition)
        .expect("trusted workspace definition")
        .id
        .clone();
    assert_ne!(trusted_definition_id, untrusted_definition_id);
    let trusted_local_policy_overlays = trusted_declarations
        .iter()
        .filter(|item| item.source_id == trusted_local_source_id)
        .collect::<Vec<_>>();
    assert_eq!(trusted_local_policy_overlays.len(), 2);
    assert!(trusted_local_policy_overlays.iter().all(|item| {
        item.role == AgentAssetDeclarationRole::PolicyOverlay
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Participates
            )
    }));
    assert!(matches!(
        trusted_local.participation,
        AgentAssetResolutionParticipation::Participates
    ));
    let trusted_assets = claude_assets(&trusted, AgentAssetCategory::Mcp, "same");
    assert_eq!(trusted_assets.len(), 1);
    assert_eq!(
        trusted_assets[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(trusted_assets[0].effective_state, AgentAssetState::Blocked);
    assert!(matches!(
        trusted_assets[0].details,
        AgentAssetDetails::Mcp {
            approval_state: AgentMcpApprovalState::Rejected,
            effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
            ..
        }
    ));
    assert!(!trusted_assets[0].resolution.contributor_ids.is_empty());
    let mut trusted_same_declaration_ids = trusted_declarations
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    trusted_same_declaration_ids.sort();
    let mut trusted_same_represented = trusted_assets[0].represented_declaration_ids.clone();
    trusted_same_represented.sort();
    let mut trusted_same_contributors = trusted_assets[0].resolution.contributor_ids.clone();
    trusted_same_contributors.sort();
    assert_eq!(trusted_same_represented, trusted_same_declaration_ids);
    assert_eq!(trusted_same_contributors, trusted_same_declaration_ids);
    assert_ne!(
        trusted_assets[0].inspection_source_id,
        untrusted_inspection_source_id
    );
    let trusted_workspace_source_id = trusted
        .sources
        .iter()
        .find(|source| source.path.ends_with(".mcp.json"))
        .expect("trusted workspace MCP source")
        .id
        .clone();
    assert_eq!(
        trusted_assets[0].inspection_source_id,
        trusted_workspace_source_id
    );
    let all_only_trusted = claude_assets(&trusted, AgentAssetCategory::Mcp, "all-only");
    assert_eq!(all_only_trusted.len(), 1);
    assert_eq!(
        all_only_trusted[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        all_only_trusted[0].details,
        AgentAssetDetails::Mcp {
            approval_state: AgentMcpApprovalState::Approved,
            effective_availability: AgentAssetEffectiveAvailability::Available,
            ..
        }
    ));
    assert_eq!(all_only_trusted[0].resolution.contributor_ids.len(), 1);
    assert_eq!(all_only_trusted[0].represented_declaration_ids.len(), 1);
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    remove_claude_fixture(root);
}

#[test]
fn claude_mcp_policy_matchers_fail_closed() {
    let run_case = |name: &str,
                    server: serde_json::Value,
                    settings: serde_json::Value,
                    expected_policy_key: Option<&str>,
                    expected_availability: AgentAssetEffectiveAvailability| {
        let (root, workspace, inventory, _) = build_claude_inventory(
            name,
            [
                (
                    "account",
                    claude_json(serde_json::json!({ "mcpServers": { "same": server } })),
                ),
                ("settings", claude_json(settings)),
                (
                    "workspace-mcp",
                    claude_json(serde_json::json!({
                        "mcpServers": { "pollution": { "command": "ignored" } },
                        "deniedMcpServers": [{ "serverName": "same" }]
                    })),
                ),
            ],
            [],
            [],
            &RealInstallationDiscoveryPort,
        );
        let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
        assert_eq!(assets.len(), 1, "{name}");
        assert_eq!(
            assets[0].resolution.relation,
            AgentAssetResolutionRelation::Independent,
            "{name}"
        );
        assert_eq!(
            assets[0].resolution.terminal,
            match expected_availability {
                AgentAssetEffectiveAvailability::Unknown => {
                    Some(AgentAssetResolutionTerminal::Unknown)
                }
                AgentAssetEffectiveAvailability::PolicyBlocked => {
                    Some(AgentAssetResolutionTerminal::PolicyBlocked)
                }
                _ => None,
            },
            "{name}"
        );
        assert_eq!(
            assets[0].effective_state,
            if expected_availability == AgentAssetEffectiveAvailability::PolicyBlocked {
                AgentAssetState::Blocked
            } else if expected_availability == AgentAssetEffectiveAvailability::Unknown {
                AgentAssetState::Unknown
            } else {
                AgentAssetState::Enabled
            },
            "{name}"
        );
        assert!(matches!(
            assets[0].details,
            AgentAssetDetails::Mcp {
                effective_availability,
                ..
            } if effective_availability == expected_availability
        ));
        if expected_policy_key.is_some()
            || expected_availability == AgentAssetEffectiveAvailability::PolicyBlocked
        {
            let settings_id = inventory
                .sources
                .iter()
                .find(|source| source.label == "Claude Code 设置")
                .expect("settings source")
                .id
                .as_str();
            if let Some(declaration_key) = expected_policy_key {
                assert_claude_policy_declaration(
                    &inventory,
                    &assets[0].resolution.control_source,
                    AgentAssetCategory::Mcp,
                    settings_id,
                    declaration_key,
                );
            } else {
                assert!(matches!(
                    assets[0].resolution.control_source,
                    Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
                        if source_id == settings_id
                ));
            }
        } else {
            assert!(assets[0].resolution.control_source.is_none(), "{name}");
        }
        assert!(inventory
            .declarations
            .iter()
            .filter(|item| item.native_id == "same")
            .all(|item| item.source_id != "ignored"));
        assert_claude_source_callback_stops_after_break(&root, &workspace);
        remove_claude_fixture(root);
    };

    run_case(
        "claude-mcp-policy-name",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({
            "allowedMcpServers": [{ "serverName": "same" }],
            "deniedMcpServers": [{ "serverName": "other" }]
        }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-command-exact",
        serde_json::json!({ "command": "node", "args": ["run"] }),
        serde_json::json!({
            "allowedMcpServers": [{ "serverCommand": ["node", "run"] }]
        }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-command-mismatch",
        serde_json::json!({ "command": "node", "args": ["other"] }),
        serde_json::json!({
            "deniedMcpServers": [{ "serverCommand": ["node"] }]
        }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-url-wildcard",
        serde_json::json!({ "type": "sse", "url": "https://api.example.test/v1" }),
        serde_json::json!({
            "deniedMcpServers": [{ "serverUrl": "https://*.example.test/*" }]
        }),
        Some("policy:deniedMcpServers:0:control:deniedMcpServers:0"),
        AgentAssetEffectiveAvailability::PolicyBlocked,
    );
    run_case(
        "claude-mcp-policy-url-anchored-negative",
        serde_json::json!({ "type": "sse", "url": "https://api.example.test.evil/v1" }),
        serde_json::json!({
            "deniedMcpServers": [{ "serverUrl": "https://*.example.test/*" }]
        }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-deny-over-allow",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({
            "allowedMcpServers": [{ "serverName": "same" }],
            "deniedMcpServers": [{ "serverName": "same" }]
        }),
        Some("policy:deniedMcpServers:0:same"),
        AgentAssetEffectiveAvailability::PolicyBlocked,
    );
    run_case(
        "claude-mcp-policy-missing-allow",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({}),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-empty-allow",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({ "allowedMcpServers": [] }),
        None,
        AgentAssetEffectiveAvailability::PolicyBlocked,
    );
    run_case(
        "claude-mcp-policy-invalid-allow",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({
            "allowedMcpServers": [{ "serverName": "same", "extra": true }]
        }),
        None,
        AgentAssetEffectiveAvailability::PolicyBlocked,
    );
    run_case(
        "claude-mcp-policy-invalid-deny",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({
            "deniedMcpServers": [{ "serverCommand": ["node"], "extra": true }]
        }),
        Some("control:mcp:deniedMcpServers:root"),
        AgentAssetEffectiveAvailability::Unknown,
    );
    run_case(
        "claude-mcp-policy-user-managed-only-is-neutral",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({ "allowManagedMcpServersOnly": true }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-managed-only-invalid-authority",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({}),
        None,
        AgentAssetEffectiveAvailability::Available,
    );
    run_case(
        "claude-mcp-policy-user-managed-only-invalid-is-neutral",
        serde_json::json!({ "command": "node" }),
        serde_json::json!({ "allowManagedMcpServersOnly": "invalid" }),
        None,
        AgentAssetEffectiveAvailability::Available,
    );

    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-mcp-policy-managed-only",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let managed_only = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(managed_only.len(), 1);
    assert_eq!(
        managed_only[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    let managed_settings_source = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("managed-settings.json"))
        .expect("managed settings source");
    assert_claude_policy_declaration(
        &inventory,
        &managed_only[0].resolution.control_source,
        AgentAssetCategory::Mcp,
        &managed_settings_source.id,
        "control:mcp:allowManagedMcpServersOnly",
    );
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    remove_claude_fixture(root);

    let (root, _, inventory, _) = build_claude_inventory(
        "claude-mcp-policy-managed-only-invalid",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": "invalid"
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_assets(&inventory, AgentAssetCategory::Mcp, "same")[0]
            .resolution
            .relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        claude_assets(&inventory, AgentAssetCategory::Mcp, "same")[0]
            .resolution
            .terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    remove_claude_fixture(root);

    let (root, _, inventory, _) = build_claude_inventory(
        "claude-mcp-policy-terminal-priority",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "other": { "command": "managed" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let terminal = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(terminal.len(), 1);
    assert_eq!(
        terminal[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    let managed_source_id = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("managed-mcp.json"))
        .expect("managed MCP source")
        .id
        .clone();
    assert!(matches!(
        terminal[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
            if source_id == &managed_source_id
    ));
    remove_claude_fixture(root);

    let (root, _, first, _) = build_claude_inventory(
        "claude-mcp-policy-reversal-a",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let (_, _, second, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_public_signature(&first),
        claude_public_signature(&second)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_policy_name_deny_keeps_exact_overlay_reference() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-name-ownership",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "allowedMcpServers": [{ "serverName": "same" }],
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same");
    let denied = declarations
        .iter()
        .find(|declaration| declaration.declaration_key == "policy:deniedMcpServers:0:same")
        .expect("exact denied policy declaration");
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(asset.len(), 1);
    assert!(matches!(
        asset[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { ref declaration_id })
            if declaration_id == &denied.id
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_policy_command_and_url_use_actual_rule_declaration_reference() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-command-url-ownership",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "command": { "command": "node", "args": ["run"] },
                        "remote": { "type": "sse", "url": "https://mcp.example.test/v1" }
                    }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [
                        { "serverCommand": ["node", "run"] },
                        { "serverUrl": "https://mcp.example.test/*" }
                    ]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let settings_id = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("settings source")
        .id
        .clone();
    for (native_id, ordinal) in [("command", 0), ("remote", 1)] {
        let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, native_id);
        assert_eq!(asset.len(), 1);
        assert_eq!(asset[0].effective_state, AgentAssetState::Blocked);
        assert_eq!(
            asset[0].resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_claude_policy_declaration(
            &inventory,
            &asset[0].resolution.control_source,
            AgentAssetCategory::Mcp,
            &settings_id,
            &format!("policy:deniedMcpServers:{ordinal}:control:deniedMcpServers:{ordinal}"),
        );
    }
    remove_claude_fixture(root);
}

#[test]
fn claude_policy_precedence_keeps_cross_layer_deny_owner() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-cross-layer-ownership",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same");
    let workspace_denied = declarations
        .iter()
        .find(|declaration| {
            declaration.source_id
                == inventory
                    .sources
                    .iter()
                    .find(|source| source.label == "Claude Code 工作区设置")
                    .expect("workspace settings source")
                    .id
                && declaration.declaration_key == "policy:deniedMcpServers:0:same"
        })
        .expect("workspace denied policy declaration");
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert!(matches!(
        asset[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { ref declaration_id })
            if declaration_id == &workspace_denied.id
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_terminal_policy_priority_prefers_name_deny_over_managed_only() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-terminal-name-deny",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "same" }]
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let denied = claude_declarations(&inventory, AgentAssetCategory::Mcp, "same")
        .into_iter()
        .find(|declaration| declaration.declaration_key == "policy:deniedMcpServers:0:same")
        .expect("exact denied declaration");
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        asset[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { ref declaration_id })
            if declaration_id == &denied.id
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_terminal_policy_priority_prefers_command_and_url_deny_over_managed_only() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-terminal-command-url-deny",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": {
                        "command": { "command": "node", "args": ["run"] },
                        "remote": { "type": "sse", "url": "https://mcp.example.test/v1" }
                    }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [
                        { "serverCommand": ["node", "run"] },
                        { "serverUrl": "https://mcp.example.test/*" }
                    ]
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let settings_id = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("settings source")
        .id
        .clone();
    for (native_id, ordinal) in [("command", 0), ("remote", 1)] {
        let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, native_id);
        assert_eq!(asset.len(), 1);
        assert_eq!(
            asset[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(asset[0].effective_state, AgentAssetState::Blocked);
        assert_eq!(
            asset[0].resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_claude_policy_declaration(
            &inventory,
            &asset[0].resolution.control_source,
            AgentAssetCategory::Mcp,
            &settings_id,
            &format!("policy:deniedMcpServers:{ordinal}:control:deniedMcpServers:{ordinal}"),
        );
    }
    remove_claude_fixture(root);
}

#[test]
fn claude_terminal_policy_priority_keeps_managed_mcp_owner_over_allow_list() {
    for (label, allow_list) in [
        ("no-match", serde_json::json!([{ "serverName": "other" }])),
        ("empty", serde_json::json!([])),
        (
            "malformed",
            serde_json::json!([{ "serverName": "ordinary", "extra": true }]),
        ),
    ] {
        let (root, _, inventory, _) = build_claude_inventory(
            &format!("claude-policy-terminal-managed-{label}"),
            [
                (
                    "account",
                    claude_json(serde_json::json!({
                        "mcpServers": { "ordinary": { "command": "ordinary" } }
                    })),
                ),
                (
                    "managed-mcp",
                    claude_json(serde_json::json!({
                        "mcpServers": { "managed": { "command": "managed" } }
                    })),
                ),
                (
                    "settings",
                    claude_json(serde_json::json!({ "allowedMcpServers": allow_list })),
                ),
            ],
            [],
            [],
            &RealInstallationDiscoveryPort,
        );
        let managed_mcp_id = inventory
            .sources
            .iter()
            .find(|source| source.path.ends_with("managed-mcp.json"))
            .expect("managed MCP source")
            .id
            .clone();
        let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "ordinary");
        assert_eq!(asset.len(), 1, "{label}");
        assert_eq!(
            asset[0].resolution.relation,
            AgentAssetResolutionRelation::Independent,
            "{label}"
        );
        assert!(
            matches!(
                asset[0].resolution.control_source,
                Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
                    if source_id == &managed_mcp_id
            ),
            "{label}"
        );
        remove_claude_fixture(root);
    }
}

#[test]
fn claude_terminal_policy_priority_invalid_deny_stays_unknown_with_root_owner() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-terminal-invalid-deny",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "ordinary": { "command": "ordinary" } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [{ "serverName": "ordinary", "extra": true }],
                    "allowedMcpServers": []
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "ordinary");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    let settings_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("invalid denied list source");
    assert_claude_policy_declaration(
        &inventory,
        &asset[0].resolution.control_source,
        AgentAssetCategory::Mcp,
        &settings_source.id,
        "control:mcp:deniedMcpServers:root",
    );
    assert_eq!(asset[0].effective_state, AgentAssetState::Unknown);
    assert_eq!(
        asset[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(matches!(
        asset[0].details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::Unknown,
            ..
        }
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_mixed_invalid_allowlist_blocks_even_when_valid_sibling_matches() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-mixed-invalid-allow",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "allowedMcpServers": [
                        { "serverName": "same" },
                        { "serverName": "same", "extra": true }
                    ]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let settings_id = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("settings source")
        .id
        .clone();
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        asset[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
            if source_id == &settings_id
    ));
    assert_eq!(asset[0].effective_state, AgentAssetState::Blocked);
    assert_eq!(
        asset[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_mixed_invalid_deny_stays_unknown_even_when_valid_sibling_matches() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-mixed-invalid-deny",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [
                        { "serverName": "same" },
                        { "serverName": "same", "extra": true }
                    ]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    let settings_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("invalid denied list source");
    assert_claude_policy_declaration(
        &inventory,
        &asset[0].resolution.control_source,
        AgentAssetCategory::Mcp,
        &settings_source.id,
        "control:mcp:deniedMcpServers:root",
    );
    assert_eq!(asset[0].effective_state, AgentAssetState::Unknown);
    assert_eq!(
        asset[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_cross_layer_invalid_allowlist_blocks_over_valid_allow_match() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-cross-layer-invalid-allow",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node" } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "allowedMcpServers": [{ "serverName": "same", "extra": true }]
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "allowedMcpServers": [{ "serverName": "same" }]
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let settings_id = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("settings source")
        .id
        .clone();
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        asset[0].resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Source { ref source_id })
            if source_id == &settings_id
    ));
    assert_eq!(asset[0].effective_state, AgentAssetState::Blocked);
    assert_eq!(
        asset[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_invalid_deny_stays_unknown_over_valid_deny_and_other_blocks() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-policy-invalid-deny-over-blocks",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "ordinary": { "command": "ordinary" } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "deniedMcpServers": [
                        { "serverName": "ordinary", "extra": true }
                    ],
                    "allowedMcpServers": []
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } },
                    "deniedMcpServers": [{ "serverName": "ordinary" }]
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedMcpServersOnly": true
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let asset = claude_assets(&inventory, AgentAssetCategory::Mcp, "ordinary");
    assert_eq!(asset.len(), 1);
    assert_eq!(
        asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    let settings_source = inventory
        .sources
        .iter()
        .find(|source| source.label == "Claude Code 设置")
        .expect("invalid denied list source");
    assert_claude_policy_declaration(
        &inventory,
        &asset[0].resolution.control_source,
        AgentAssetCategory::Mcp,
        &settings_source.id,
        "control:mcp:deniedMcpServers:root",
    );
    assert_eq!(asset[0].effective_state, AgentAssetState::Unknown);
    assert_eq!(
        asset[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_managed_mcp_is_exclusive() {
    let ordinary = serde_json::json!({ "command": "ordinary" });
    let managed = serde_json::json!({ "command": "managed" });
    let run_case = |name: &str,
                    managed_file: Option<serde_json::Value>,
                    blocked: Option<AgentAssetDiagnostic>,
                    expected_state: AgentAssetState,
                    expected_terminal: Option<AgentAssetResolutionTerminal>,
                    expected_mcp_cardinality: usize| {
        let files = managed_file
            .map(|value| vec![("managed-mcp", claude_json(value))])
            .unwrap_or_default();
        let (root, workspace, inventory, _) = build_claude_inventory(
            name,
            std::iter::once((
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "ordinary": ordinary }
                })),
            ))
            .chain(files),
            [],
            blocked
                .map(|diagnostic| vec![("managed-mcp", diagnostic)])
                .unwrap_or_default(),
            &RealInstallationDiscoveryPort,
        );
        let assets = claude_assets(&inventory, AgentAssetCategory::Mcp, "ordinary");
        assert_eq!(assets.len(), 1, "{name}");
        assert_eq!(
            assets[0].resolution.relation,
            AgentAssetResolutionRelation::Independent,
            "{name}"
        );
        assert_eq!(assets[0].effective_state, expected_state, "{name}");
        assert_eq!(assets[0].resolution.terminal, expected_terminal, "{name}");
        let declarations = inventory
            .declarations
            .iter()
            .filter(|item| {
                item.native_kind == AgentAssetCategory::Mcp
                    && item.role == AgentAssetDeclarationRole::Definition
            })
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), expected_mcp_cardinality, "{name}");
        let source_ids = inventory
            .sources
            .iter()
            .map(|source| source.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(declarations
            .iter()
            .all(|item| source_ids.contains(item.source_id.as_str())));
        let all_assets = inventory
            .assets
            .iter()
            .filter(|item| item.category == AgentAssetCategory::Mcp)
            .collect::<Vec<_>>();
        assert_eq!(all_assets.len(), expected_mcp_cardinality, "{name}");
        assert!(all_assets.iter().all(|item| {
            item.source_ids
                .iter()
                .all(|source_id| source_ids.contains(source_id.as_str()))
        }));
        if expected_terminal.is_none() {
            assert!(assets[0].resolution.control_source.is_none());
        }
        assert_claude_source_callback_stops_after_break(&root, &workspace);
        remove_claude_fixture(root);
    };

    run_case(
        "claude-managed-mcp-exclusive-valid",
        Some(serde_json::json!({
            "mcpServers": { "managed": managed }
        })),
        None,
        AgentAssetState::Blocked,
        Some(AgentAssetResolutionTerminal::PolicyBlocked),
        2,
    );
    run_case(
        "claude-managed-mcp-exclusive-empty",
        Some(serde_json::json!({ "mcpServers": {} })),
        None,
        AgentAssetState::Blocked,
        Some(AgentAssetResolutionTerminal::PolicyBlocked),
        1,
    );
    run_case(
        "claude-managed-mcp-exclusive-missing",
        None,
        None,
        AgentAssetState::Enabled,
        None,
        1,
    );
    run_case(
        "claude-managed-mcp-exclusive-malformed",
        Some(serde_json::json!({ "mcpServers": [] })),
        None,
        AgentAssetState::Unknown,
        Some(AgentAssetResolutionTerminal::Unknown),
        1,
    );
    run_case(
        "claude-managed-mcp-exclusive-blocked",
        None,
        Some(AgentAssetDiagnostic::ReadFailed {
            source_id: "managed-secret-source".to_owned(),
            error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
        }),
        AgentAssetState::Unknown,
        Some(AgentAssetResolutionTerminal::Unknown),
        1,
    );

    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-managed-mcp-exclusive-same-id",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "ordinary", "args": ["old"] } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "type": "sse", "url": "https://managed.example/mcp" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let same = claude_assets(&inventory, AgentAssetCategory::Mcp, "same");
    assert_eq!(same.len(), 2);
    assert!(same
        .iter()
        .any(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced));
    let same_winner = same
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("managed MCP replacement winner");
    assert_eq!(
        same_winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert!(matches!(
        same_winner.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Sse,
            ..
        }
    ));
    assert!(!same_winner.resolution.contributor_ids.is_empty());
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    remove_claude_fixture(root);

    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-managed-mcp-exclusive-nested-definition",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "nested": { "command": "ordinary" } }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "managedMcpServers": {
                        "nested": {
                            "type": "http",
                            "url": "https://managed.example/mcp"
                        }
                    }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let nested = claude_assets(&inventory, AgentAssetCategory::Mcp, "nested");
    assert_eq!(nested.len(), 2);
    let nested_winner = nested
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("nested managed MCP replacement winner");
    assert_eq!(
        nested_winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert!(matches!(
        nested_winner.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http,
            ..
        }
    ));
    let nested_source_id = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("managed-settings.json"))
        .expect("nested managed settings source")
        .id
        .clone();
    assert_eq!(nested_winner.inspection_source_id, nested_source_id);
    let nested_declarations = claude_declarations(&inventory, AgentAssetCategory::Mcp, "nested");
    assert_eq!(nested_declarations.len(), 2);
    assert!(nested_declarations
        .iter()
        .all(|item| item.role == AgentAssetDeclarationRole::Definition));
    let nested_source_ids = inventory
        .sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(nested_declarations
        .iter()
        .all(|item| nested_source_ids.contains(item.source_id.as_str())));
    let mut nested_represented = nested_winner.represented_declaration_ids.clone();
    nested_represented.sort();
    let mut nested_contributors = nested_winner.resolution.contributor_ids.clone();
    nested_contributors.sort();
    let mut nested_declaration_ids = nested_declarations
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    nested_declaration_ids.sort();
    assert_eq!(nested_represented, nested_declaration_ids);
    assert_eq!(nested_contributors, nested_declaration_ids);
    assert!(claude_assets(&inventory, AgentAssetCategory::Mcp, "nested")
        .iter()
        .all(|asset| asset.resolution.control_source.is_none()));
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|item| item.category == AgentAssetCategory::Mcp)
            .count(),
        2
    );

    let nested_payload_source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "managed-settings".to_owned(),
        label: "Claude Code 托管设置".to_owned(),
        scope: AgentAssetScope::Managed,
        path: PathBuf::from("/etc/claude-code/managed-settings.json"),
        allowed_root: PathBuf::from("/etc/claude-code"),
        precedence: 40,
        writable: false,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Managed,
                precedence: 40,
            },
        ],
    };
    let nested_payload_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "managedMcpServers": {
                "other": { "type": "http", "url": "https://managed.example/other" },
                "nested": { "type": "http", "url": "https://managed.example/nested" }
            }
        })),
        revision: AgentAssetRevision::default(),
    };
    let mut nested_payloads = NativePayloadDebugCollector::default();
    definition(AgentCliKind::ClaudeCode).environment().parse(
        AgentAssetParseRequest {
            context: &inventory.contexts[0],
            source: &nested_payload_source,
            snapshot: &nested_payload_snapshot,
            native_home: None,
            workspace_canonical: Some(&workspace),
            workspace_lexical: Some(&workspace),
        },
        &mut nested_payloads,
    );
    let nested_payload = nested_payloads
        .declarations
        .iter()
        .find(|item| item.native_id == "nested")
        .expect("nested managed MCP native payload");
    assert!(matches!(
        &nested_payload.native_payload,
        AgentAssetNativePayload::McpDefinition(AgentMcpDefinitionPayload {
            identity: AgentMcpMatcherIdentity::Remote { url },
            origin: AgentMcpDefinitionOrigin::Declared,
        })
            if url == "https://managed.example/nested"
    ));
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    remove_claude_fixture(root);

    let (root, _, first, _) = build_claude_inventory(
        "claude-managed-mcp-exclusive-reversal-a",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "ordinary" } }
                })),
            ),
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let (_, _, second, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "managed-mcp",
                claude_json(serde_json::json!({
                    "mcpServers": { "managed": { "command": "managed" } }
                })),
            ),
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "ordinary" } }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_public_signature(&first),
        claude_public_signature(&second)
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_plugin_registry_v2_is_only_registration_authority() {
    let registry = claude_json(serde_json::json!({
        "version": 2,
        "plugins": {
            "alpha": [
                { "scope": "managed", "installPath": "/fixture/alpha-managed", "version": "4" },
                { "scope": "user", "installPath": "/fixture/alpha-user", "version": "1" },
                { "scope": "project", "projectPath": "__WORKSPACE__", "installPath": "/fixture/alpha-project", "version": "2" },
                { "scope": "local", "projectPath": "__WORKSPACE__", "installPath": "/fixture/alpha-local", "version": "3" },
                { "scope": "user", "installPath": "relative/path" }
            ],
            "mismatch": [
                { "scope": "project", "projectPath": "/other/workspace", "installPath": "/fixture/mismatch" }
            ]
        }
    }));
    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-plugin-registry",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            ("plugin-registry", registry.clone()),
            (
                "known-marketplaces",
                claude_json(serde_json::json!({
                    "plugins": {
                        "marketplace-only": { "installPath": "/fixture/marketplace-only" }
                    }
                })),
            ),
        ],
        [(
            "plugin-cache",
            claude_entries([
                ("alpha", AgentAssetSourceKind::Directory, false),
                ("cache-only", AgentAssetSourceKind::Directory, false),
            ]),
        )],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Trusted
    );
    let alpha_all = claude_declarations(&inventory, AgentAssetCategory::Plugin, "alpha");
    let alpha = alpha_all
        .iter()
        .copied()
        .filter(|item| item.role == AgentAssetDeclarationRole::Definition)
        .collect::<Vec<_>>();
    assert_eq!(alpha.len(), 4);
    let package_metadata = alpha_all
        .iter()
        .filter(|item| item.declaration_key.starts_with("package.root:"))
        .collect::<Vec<_>>();
    assert_eq!(package_metadata.len(), 1);
    assert_eq!(package_metadata[0].scope, AgentAssetScope::Managed);
    assert_eq!(
        package_metadata[0].presence,
        crate::models::AgentAssetPresence::Missing
    );
    let mut origins = alpha
        .iter()
        .map(|item| (item.scope, item.precedence, item.evidence.facts.clone()))
        .collect::<Vec<_>>();
    origins.sort_by_key(|(scope, precedence, _)| (*precedence, *scope));
    assert_eq!(
        origins
            .iter()
            .map(|(scope, precedence, _)| (*scope, *precedence))
            .collect::<Vec<_>>(),
        vec![
            (AgentAssetScope::User, 10),
            (AgentAssetScope::Workspace, 20),
            (AgentAssetScope::Local, 30),
            (AgentAssetScope::Managed, 40),
        ]
    );
    assert!(alpha.iter().all(|item| {
        item.role == AgentAssetDeclarationRole::Definition
            && item.presence == crate::models::AgentAssetPresence::Present
            && item.declared_state == AgentAssetDeclaredState::Unknown
            && item.evidence.facts.contains_key("version")
    }));
    let alpha_assets = claude_assets(&inventory, AgentAssetCategory::Plugin, "alpha");
    assert_eq!(alpha_assets.len(), 4, "{:?}", inventory.diagnostics);
    let mut alpha_ids = alpha_all
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    alpha_ids.sort();
    let winner = alpha_assets
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("managed Plugin winner");
    assert_eq!(winner.scope, AgentAssetScope::Managed);
    assert_eq!(winner.precedence, 40);
    assert_eq!(winner.declared_state, AgentAssetState::Unknown);
    assert_eq!(winner.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        winner.resolution.winner_id.as_deref(),
        Some(winner.stable_id.as_str())
    );
    let mut winner_represented = winner.represented_declaration_ids.clone();
    winner_represented.sort();
    assert_eq!(winner_represented, alpha_ids);
    let mut winner_contributors = winner.resolution.contributor_ids.clone();
    winner_contributors.sort();
    assert_eq!(winner_contributors, alpha_ids);
    assert!(matches!(
        winner.details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::NotInstalled,
            enabled: AgentAssetDeclaredState::Unknown,
            ..
        }
    ));
    let losers = alpha_assets
        .iter()
        .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .collect::<Vec<_>>();
    assert_eq!(losers.len(), 3);
    for loser in losers {
        assert_eq!(loser.declared_state, AgentAssetState::Unknown);
        assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
        assert_eq!(loser.resolution.winner_id, winner.resolution.winner_id);
        assert_eq!(loser.represented_declaration_ids.len(), 1);
        assert_eq!(loser.resolution.contributor_ids.len(), 1);
        assert_eq!(
            loser.represented_declaration_ids,
            loser.resolution.contributor_ids
        );
        assert!(alpha.iter().any(|item| {
            item.id == loser.resolution.contributor_ids[0]
                && item.source_id == loser.inspection_source_id
        }));
    }
    assert!(claude_declarations(&inventory, AgentAssetCategory::Plugin, "mismatch").is_empty());
    assert!(claude_assets(&inventory, AgentAssetCategory::Plugin, "mismatch").is_empty());
    let registry_source_id = inventory
        .sources
        .iter()
        .find(|source| {
            source
                .path
                .replace('\\', "/")
                .ends_with("plugins/installed_plugins.json")
        })
        .expect("Claude plugin registry source")
        .id
        .clone();
    assert!(inventory
        .declarations
        .iter()
        .filter(|item| {
            item.native_kind == AgentAssetCategory::Plugin
                && item.role == AgentAssetDeclarationRole::Definition
        })
        .all(|item| item.source_id == registry_source_id));
    let malformed = inventory
        .sources
        .iter()
        .filter(|source| source.id == registry_source_id)
        .flat_map(|source| source.diagnostics.iter())
        .filter(|diagnostic| {
            matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed {
                    location: Some(location), ..
                } if location == "plugin.registry.installPath"
            )
        })
        .count();
    assert_eq!(malformed, 1);
    assert!(inventory
        .sources
        .iter()
        .filter(|source| {
            source.path.replace('\\', "/").ends_with("plugins/cache")
                || source.path.ends_with("known_marketplaces.json")
        })
        .all(|source| source.categories.is_empty()));
    let cache_source_ids = inventory
        .sources
        .iter()
        .filter(|source| source.path.replace('\\', "/").ends_with("plugins/cache"))
        .map(|source| source.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let marketplace_source_ids = inventory
        .sources
        .iter()
        .filter(|source| source.path.ends_with("known_marketplaces.json"))
        .map(|source| source.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!cache_source_ids.is_empty());
    assert!(!marketplace_source_ids.is_empty());
    assert!(inventory.declarations.iter().all(|item| {
        !cache_source_ids.contains(&item.source_id)
            && !marketplace_source_ids.contains(&item.source_id)
    }));
    assert!(inventory.assets.iter().all(|asset| {
        !asset.source_ids.iter().any(|source_id| {
            cache_source_ids.contains(source_id) || marketplace_source_ids.contains(source_id)
        })
    }));
    assert!(inventory.assets.iter().all(|asset| {
        asset.relationships.provided_by.is_none()
            && asset.relationships.action_owner.is_none()
            && asset.relationships.affected_asset_ids.is_empty()
    }));
    let cache_root = inventory
        .sources
        .iter()
        .find(|source| cache_source_ids.contains(&source.id))
        .expect("physical plugin cache source")
        .path
        .clone();
    let marketplace_file = inventory
        .sources
        .iter()
        .find(|source| marketplace_source_ids.contains(&source.id))
        .expect("physical marketplace source")
        .path
        .clone();
    let is_descendant = |path: &str, root: &str| {
        let path = Path::new(path);
        let root = Path::new(root);
        path != root && path.starts_with(root)
    };
    assert!(inventory
        .sources
        .iter()
        .filter(|source| source.path.ends_with("SKILL.md"))
        .all(|source| {
            !is_descendant(&source.path, &cache_root)
                && !is_descendant(&source.path, &marketplace_file)
        }));
    assert!(inventory
        .declarations
        .iter()
        .all(|item| { item.native_id != "cache-only" && item.native_id != "marketplace-only" }));
    assert!(inventory
        .assets
        .iter()
        .all(|asset| { asset.native_id != "cache-only" && asset.native_id != "marketplace-only" }));
    assert!(inventory
        .sources
        .iter()
        .filter(|source| source.id == registry_source_id)
        .flat_map(|source| source.diagnostics.iter())
        .all(|diagnostic| !matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed {
                location: Some(location), ..
            } if location.starts_with("plugins.mismatch")
        )));
    assert!(inventory
        .declarations
        .iter()
        .filter(|item| {
            item.native_kind == AgentAssetCategory::Plugin
                && item.role == AgentAssetDeclarationRole::Definition
        })
        .all(|item| item.source_id == registry_source_id));
    assert!(!workspace.as_os_str().is_empty());
    assert_claude_source_callback_stops_after_break(&root, &workspace);
    let reversed_registry = claude_json(serde_json::json!({
        "version": 2,
        "plugins": {
            "alpha": [
                { "scope": "user", "installPath": "/fixture/alpha-user", "version": "1" },
                { "scope": "managed", "installPath": "/fixture/alpha-managed", "version": "4" },
                { "scope": "local", "projectPath": "__WORKSPACE__", "installPath": "/fixture/alpha-local", "version": "3" },
                { "scope": "project", "projectPath": "__WORKSPACE__", "installPath": "/fixture/alpha-project", "version": "2" },
                { "scope": "user", "installPath": "relative/path" }
            ],
            "mismatch": [
                { "scope": "project", "projectPath": "/other/workspace", "installPath": "/fixture/mismatch" }
            ]
        }
    }));
    let (_, _, reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "known-marketplaces",
                claude_json(serde_json::json!({
                    "plugins": {
                        "marketplace-only": { "installPath": "/fixture/marketplace-only" }
                    }
                })),
            ),
            ("plugin-registry", reversed_registry),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
        ],
        [(
            "plugin-cache",
            claude_entries([
                ("cache-only", AgentAssetSourceKind::Directory, false),
                ("alpha", AgentAssetSourceKind::Directory, false),
            ]),
        )],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_full_public_signature(&inventory),
        claude_full_public_signature(&reversed)
    );
    drop(inventory);
    remove_claude_fixture(root);
}

#[test]
fn claude_plugin_state_is_typed_overlay() {
    let registry = |reverse: bool| {
        let mut entries = vec![
            serde_json::json!({
                "scope": "user",
                "installPath": "/fixture/collision-user"
            }),
            serde_json::json!({
                "scope": "project",
                "projectPath": "__WORKSPACE__",
                "installPath": "/fixture/collision-project"
            }),
        ];
        if reverse {
            entries.reverse();
        }
        serde_json::json!({
            "version": 2,
            "plugins": {
                "alpha": [{ "scope": "user", "installPath": "/fixture/alpha" }],
                "collision": entries
            }
        })
    };
    let settings = || {
        claude_json(serde_json::json!({
            "enabledPlugins": {
                "alpha": false,
                "collision": true,
                "stale": true
            }
        }))
    };
    let build = |name: &str, trusted: bool, reverse: bool| {
        build_claude_inventory(
            name,
            [
                (
                    "account",
                    claude_json(serde_json::json!({
                        "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": trusted } }
                    })),
                ),
                ("plugin-registry", claude_json(registry(reverse))),
                ("settings", settings()),
                (
                    "workspace-settings",
                    claude_json(serde_json::json!({
                        "enabledPlugins": { "alpha": true, "collision": false, "stale": false }
                    })),
                ),
                (
                    "workspace-local-settings",
                    claude_json(serde_json::json!({
                        "enabledPlugins": { "alpha": false, "collision": true, "stale": true }
                    })),
                ),
                (
                    "managed-settings",
                    claude_json(serde_json::json!({
                        "enabledPlugins": { "alpha": true, "collision": false, "stale": false }
                    })),
                ),
            ],
            [(
                "plugin-cache",
                claude_entries([
                    ("alpha", AgentAssetSourceKind::Directory, false),
                    ("collision", AgentAssetSourceKind::Directory, false),
                ]),
            )],
            [],
            &RealInstallationDiscoveryPort,
        )
    };

    let (root, workspace, inventory, _) = build("claude-plugin-state", true, false);
    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Trusted
    );
    let alpha = claude_declarations(&inventory, AgentAssetCategory::Plugin, "alpha");
    assert_eq!(alpha.len(), 6);
    assert!(alpha
        .iter()
        .any(|item| item.role == AgentAssetDeclarationRole::Definition));
    let alpha_overlays = alpha
        .iter()
        .filter(|item| {
            item.role == AgentAssetDeclarationRole::StateOverlay
                && item.declaration_key == "state:alpha"
        })
        .collect::<Vec<_>>();
    assert_eq!(alpha_overlays.len(), 4);
    let mut alpha_origins = alpha_overlays
        .iter()
        .map(|item| (item.scope, item.precedence, item.declared_state))
        .collect::<Vec<_>>();
    alpha_origins.sort_by_key(|(_, precedence, _)| *precedence);
    assert_eq!(
        alpha_origins,
        vec![
            (AgentAssetScope::User, 10, AgentAssetDeclaredState::Disabled),
            (
                AgentAssetScope::Workspace,
                20,
                AgentAssetDeclaredState::Enabled
            ),
            (
                AgentAssetScope::Local,
                30,
                AgentAssetDeclaredState::Disabled
            ),
            (
                AgentAssetScope::Managed,
                40,
                AgentAssetDeclaredState::Enabled
            ),
        ]
    );
    let alpha_asset = claude_assets(&inventory, AgentAssetCategory::Plugin, "alpha");
    assert_eq!(alpha_asset.len(), 1);
    assert_eq!(
        alpha_asset[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(alpha_asset[0].declared_state, AgentAssetState::Enabled);
    assert_eq!(
        alpha_asset[0].effective_state,
        AgentAssetState::NotInstalled
    );
    assert!(matches!(
        alpha_asset[0].details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::NotInstalled,
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    let metadata = alpha
        .iter()
        .filter(|item| item.declaration_key.starts_with("package.root:"))
        .collect::<Vec<_>>();
    assert_eq!(metadata.len(), 1);
    assert_eq!(
        metadata[0].presence,
        crate::models::AgentAssetPresence::Missing
    );
    assert_eq!(alpha_asset[0].resolution.contributor_ids.len(), 6);
    assert_eq!(alpha_asset[0].represented_declaration_ids.len(), 6);

    let stale = claude_declarations(&inventory, AgentAssetCategory::Plugin, "stale");
    assert_eq!(stale.len(), 4);
    assert!(stale
        .iter()
        .all(|item| item.role == AgentAssetDeclarationRole::StateOverlay));
    assert!(stale
        .iter()
        .all(|item| !item.evidence.facts.contains_key("stateOnly")));
    assert!(claude_assets(&inventory, AgentAssetCategory::Plugin, "stale").is_empty());
    assert!(!inventory
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidProjection { .. })));

    let collision = claude_declarations(&inventory, AgentAssetCategory::Plugin, "collision");
    assert_eq!(collision.len(), 7);
    assert!(collision
        .iter()
        .filter(|item| item.role == AgentAssetDeclarationRole::Definition)
        .all(
            |item| item.presence == crate::models::AgentAssetPresence::Present
                && item.scope != AgentAssetScope::Managed
        ));
    let collision_assets = claude_assets(&inventory, AgentAssetCategory::Plugin, "collision");
    assert_eq!(collision_assets.len(), 2, "{:?}", inventory.diagnostics);
    let mut collision_ids = collision
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    collision_ids.sort();
    let collision_definition_ids = collision
        .iter()
        .filter(|item| item.role == AgentAssetDeclarationRole::Definition)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let collision_overlay_ids = collision
        .iter()
        .filter(|item| {
            item.role == AgentAssetDeclarationRole::StateOverlay
                && item.declaration_key == "state:collision"
        })
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut collision_definition_ids = collision_definition_ids;
    collision_definition_ids.sort();
    let mut collision_overlay_ids = collision_overlay_ids;
    collision_overlay_ids.sort();
    assert_eq!(collision_definition_ids.len(), 2);
    assert_eq!(collision_overlay_ids.len(), 4);
    let winner = collision_assets
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("Plugin collision winner");
    assert_eq!(winner.declared_state, AgentAssetState::Disabled);
    assert_eq!(winner.effective_state, AgentAssetState::Disabled);
    assert_eq!(
        winner.resolution.winner_id.as_deref(),
        Some(winner.stable_id.as_str())
    );
    let mut winner_represented = winner.represented_declaration_ids.clone();
    winner_represented.sort();
    assert_eq!(winner_represented, collision_ids);
    let mut winner_contributors = winner.resolution.contributor_ids.clone();
    winner_contributors.sort();
    assert_eq!(winner_contributors, collision_ids);
    assert!(matches!(
        winner.details,
        AgentAssetDetails::Plugin {
            install_state: AgentAssetInstallState::NotInstalled,
            enabled: AgentAssetDeclaredState::Disabled,
            ..
        }
    ));
    let loser = collision_assets
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .expect("Plugin collision loser");
    assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
    assert_eq!(loser.resolution.winner_id, winner.resolution.winner_id);
    assert_eq!(loser.represented_declaration_ids.len(), 1);
    assert_eq!(loser.resolution.contributor_ids.len(), 1);
    assert_eq!(
        loser.represented_declaration_ids,
        loser.resolution.contributor_ids
    );
    assert!(collision_definition_ids.contains(&loser.resolution.contributor_ids[0]));
    assert!(inventory
        .sources
        .iter()
        .filter(|source| source.path.replace('\\', "/").ends_with("plugins/cache"))
        .all(|source| source.categories.is_empty()));
    assert_claude_source_callback_stops_after_break(&root, &workspace);

    let (_, _, reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        [
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "enabledPlugins": { "alpha": true, "collision": false, "stale": false }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "enabledPlugins": { "alpha": false, "collision": true, "stale": true }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "enabledPlugins": { "alpha": true, "collision": false, "stale": false }
                })),
            ),
            ("settings", settings()),
            ("plugin-registry", claude_json(registry(true))),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
        ],
        [("plugin-cache", claude_entries([]))],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_full_public_signature(&inventory),
        claude_full_public_signature(&reversed)
    );
    remove_claude_fixture(root);

    let (untrusted_root, untrusted_workspace, untrusted, _) =
        build("claude-plugin-state-untrusted", false, false);
    assert_eq!(
        untrusted.contexts[0].trust_context,
        AgentTrustState::Untrusted
    );
    let untrusted_alpha = claude_declarations(&untrusted, AgentAssetCategory::Plugin, "alpha");
    let suppressed = untrusted_alpha
        .iter()
        .filter(|item| {
            matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(suppressed.len(), 2);
    assert!(suppressed.iter().all(|item| {
        item.role == AgentAssetDeclarationRole::StateOverlay
            && matches!(
                item.scope,
                AgentAssetScope::Workspace | AgentAssetScope::Local
            )
    }));
    let untrusted_alpha_asset = claude_assets(&untrusted, AgentAssetCategory::Plugin, "alpha");
    assert_eq!(untrusted_alpha_asset.len(), 1);
    assert_eq!(untrusted_alpha_asset[0].resolution.contributor_ids.len(), 4);
    assert!(suppressed.iter().all(|item| {
        !untrusted_alpha_asset[0]
            .resolution
            .contributor_ids
            .contains(&item.id)
    }));
    assert_eq!(
        untrusted_alpha_asset[0].represented_declaration_ids.len(),
        6
    );
    assert_eq!(
        untrusted_alpha_asset[0].declared_state,
        AgentAssetState::Enabled
    );
    assert!(matches!(
        untrusted_alpha_asset[0].details,
        AgentAssetDetails::Plugin {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    assert_claude_source_callback_stops_after_break(&untrusted_root, &untrusted_workspace);
    remove_claude_fixture(untrusted_root);
}

#[test]
fn claude_skill_follow_up_and_manual_only() {
    let files = [
        (
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": true,
                    "enabledMcpServers": ["orphan-mcp"]
                } }
            })),
        ),
        (
            "skill-manifest:skills:manual",
            b"---\nname: Manual\ndisable-model-invocation: true\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:auto",
            b"---\nname: Automatic\ndisable-model-invocation: false\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:plain",
            b"body without frontmatter".to_vec(),
        ),
        (
            "skill-manifest:skills:malformed",
            b"---\nname: [bad]\n---\nbody".to_vec(),
        ),
        ("skill-manifest:skills:invalid", vec![0xff, 0xfe, 0xfd]),
        (
            "skill-manifest:skills:indicator-key-dash",
            b"---\n-name: value\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-key-question",
            b"---\n?name: value\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-key-percent",
            b"---\n%name: value\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-key-at",
            b"---\n@name: value\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-key-backtick",
            b"---\n`name: value\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-value-dash",
            b"---\nname: - sequence\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-value-question",
            b"---\nname: ? explicit\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-value-percent",
            b"---\nname: %directive\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-value-at",
            b"---\nname: @reserved\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:skills:indicator-value-backtick",
            b"---\nname: `tagged`\n---\nbody".to_vec(),
        ),
        (
            "skill-manifest:workspace-skills:manual",
            b"---\nname: Workspace manual\ndisable-model-invocation: false\n---\nbody".to_vec(),
        ),
        (
            "known-marketplaces",
            claude_json(serde_json::json!({
                "skills": { "marketplace-skill": { "path": "/fixture/marketplace-skill" } }
            })),
        ),
    ];
    let directories = [
        (
            "skills",
            claude_entries([
                ("manual", AgentAssetSourceKind::Directory, false),
                ("auto", AgentAssetSourceKind::Directory, false),
                ("plain", AgentAssetSourceKind::Directory, false),
                ("malformed", AgentAssetSourceKind::Directory, false),
                ("invalid", AgentAssetSourceKind::Directory, false),
                ("indicator-key-dash", AgentAssetSourceKind::Directory, false),
                (
                    "indicator-key-question",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                (
                    "indicator-key-percent",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                ("indicator-key-at", AgentAssetSourceKind::Directory, false),
                (
                    "indicator-key-backtick",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                (
                    "indicator-value-dash",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                (
                    "indicator-value-question",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                (
                    "indicator-value-percent",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                ("indicator-value-at", AgentAssetSourceKind::Directory, false),
                (
                    "indicator-value-backtick",
                    AgentAssetSourceKind::Directory,
                    false,
                ),
                ("missing", AgentAssetSourceKind::Directory, false),
                ("unsafe/name", AgentAssetSourceKind::Directory, false),
                ("unsafe\\name", AgentAssetSourceKind::Directory, false),
                ("C:skill", AgentAssetSourceKind::Directory, false),
                ("/absolute", AgentAssetSourceKind::Directory, false),
                (".", AgentAssetSourceKind::Directory, false),
                ("..", AgentAssetSourceKind::Directory, false),
                ("plugin/skill", AgentAssetSourceKind::Directory, false),
                ("regular-file", AgentAssetSourceKind::File, false),
                ("symlinked", AgentAssetSourceKind::Directory, true),
                (" ", AgentAssetSourceKind::Directory, false),
                ("", AgentAssetSourceKind::Directory, false),
            ]),
        ),
        (
            "workspace-skills",
            claude_entries([
                ("manual", AgentAssetSourceKind::Directory, false),
                ("workspace-missing", AgentAssetSourceKind::Directory, false),
                ("workspace/nested", AgentAssetSourceKind::Directory, false),
                ("workspace-link", AgentAssetSourceKind::Directory, true),
            ]),
        ),
        (
            "plugin-cache",
            claude_entries([("cache-skill", AgentAssetSourceKind::Directory, false)]),
        ),
    ];
    let (root, workspace, inventory, _) = build_claude_inventory(
        "claude-skill-follow-up",
        files.clone(),
        directories.clone(),
        [],
        &RealInstallationDiscoveryPort,
    );

    assert_eq!(
        inventory.contexts[0].trust_context,
        AgentTrustState::Trusted
    );
    let skill_source_paths = inventory
        .sources
        .iter()
        .filter(|source| source.path.ends_with("SKILL.md"))
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    assert!(skill_source_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("skills/manual/SKILL.md")));
    assert!(skill_source_paths.iter().any(|path| path
        .replace('\\', "/")
        .ends_with("workspace/.claude/skills/manual/SKILL.md")));
    assert!(skill_source_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("skills/auto/SKILL.md")));
    assert!(skill_source_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("skills/plain/SKILL.md")));
    assert!(skill_source_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("skills/missing/SKILL.md")));
    assert!(skill_source_paths.iter().any(|path| path
        .replace('\\', "/")
        .ends_with("workspace/.claude/skills/workspace-missing/SKILL.md")));
    assert!(skill_source_paths
        .iter()
        .all(|path| !path.contains("unsafe") && !path.contains("plugin")));
    assert!(skill_source_paths
        .iter()
        .all(|path| path.ends_with("SKILL.md")));

    let manual = claude_declarations(&inventory, AgentAssetCategory::Skill, "manual");
    assert_eq!(manual.len(), 2);
    assert!(manual.iter().all(|item| {
        item.role == AgentAssetDeclarationRole::Definition
            && item.presence == crate::models::AgentAssetPresence::Present
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Participates
            )
    }));
    let mut manual_ids = manual
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    manual_ids.sort();
    let manual_assets = claude_assets(&inventory, AgentAssetCategory::Skill, "manual");
    assert_eq!(manual_assets.len(), 2, "manual skill assets");
    assert_eq!(
        manual_assets
            .iter()
            .filter(|asset| {
                asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
            })
            .count(),
        1
    );
    assert_eq!(
        manual_assets
            .iter()
            .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .count(),
        1
    );
    let winner = manual_assets
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("manual skill winner");
    let loser = manual_assets
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .expect("manual skill loser");
    let winner_declaration = manual
        .iter()
        .find(|item| item.source_id == winner.inspection_source_id)
        .expect("manual skill winner declaration");
    let loser_declaration = manual
        .iter()
        .find(|item| loser.represented_declaration_ids.contains(&item.id))
        .expect("manual skill loser declaration");
    assert_eq!(
        winner.resolution.winner_id.as_deref(),
        Some(winner.stable_id.as_str())
    );
    assert_eq!(loser.resolution.winner_id, winner.resolution.winner_id);
    let mut winner_represented = winner.represented_declaration_ids.clone();
    winner_represented.sort();
    assert_eq!(winner_represented, manual_ids);
    let mut winner_contributors = winner.resolution.contributor_ids.clone();
    winner_contributors.sort();
    assert_eq!(winner_contributors, manual_ids);
    assert_eq!(
        loser.represented_declaration_ids,
        vec![loser_declaration.id.clone()]
    );
    assert_eq!(
        loser.resolution.contributor_ids,
        vec![loser_declaration.id.clone()]
    );
    assert_eq!(winner.inspection_source_id, winner_declaration.source_id);
    assert_eq!(loser.inspection_source_id, loser_declaration.source_id);
    let mut winner_source_ids = winner.source_ids.clone();
    winner_source_ids.sort();
    let mut manual_source_ids = manual
        .iter()
        .map(|item| item.source_id.clone())
        .collect::<Vec<_>>();
    manual_source_ids.sort();
    manual_source_ids.dedup();
    assert_eq!(winner_source_ids, manual_source_ids);
    assert_eq!(loser.source_ids, vec![loser_declaration.source_id.clone()]);
    assert_eq!(winner.declared_state, AgentAssetState::Enabled);
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);
    assert_eq!(loser.declared_state, AgentAssetState::Enabled);
    assert_eq!(loser.effective_state, AgentAssetState::Shadowed);
    assert!(matches!(
        winner.details,
        AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled,
            invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
        }
    ));
    assert!(matches!(
        loser.details,
        AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled,
            invocation_policy: AgentSkillInvocationPolicy::ManualOnly,
        }
    ));
    assert!(manual
        .iter()
        .any(|item| item.scope == AgentAssetScope::User && item.precedence == 10));
    assert!(manual
        .iter()
        .any(|item| item.scope == AgentAssetScope::Workspace && item.precedence == 20));

    for (native_id, expected_policy) in [
        ("auto", AgentSkillInvocationPolicy::ModelInvocable),
        ("plain", AgentSkillInvocationPolicy::ModelInvocable),
        (
            "indicator-key-dash",
            AgentSkillInvocationPolicy::ModelInvocable,
        ),
        (
            "indicator-key-question",
            AgentSkillInvocationPolicy::ModelInvocable,
        ),
    ] {
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Skill, native_id);
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            claude_assets(&inventory, AgentAssetCategory::Skill, native_id).len(),
            1
        );
        assert_eq!(
            declarations[0].declared_state,
            AgentAssetDeclaredState::Enabled
        );
        let asset = claude_assets(&inventory, AgentAssetCategory::Skill, native_id)[0];
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert!(matches!(
            asset.details,
            AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Enabled,
                invocation_policy,
            } if invocation_policy == expected_policy
        ));
    }
    for (native_id, expected_location) in [
        ("malformed", "frontmatter.name"),
        ("invalid", "frontmatter.encoding"),
    ] {
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Skill, native_id);
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].declared_state,
            AgentAssetDeclaredState::Unknown
        );
        let source = inventory
            .sources
            .iter()
            .find(|source| source.id == declarations[0].source_id)
            .expect("skill source for malformed declaration");
        assert_eq!(
            source
                .diagnostics
                .iter()
                .filter(|item| matches!(
                    item,
                    AgentAssetDiagnostic::Malformed {
                        format: AgentAssetDocumentFormat::Yaml,
                        location: Some(location),
                    } if location == expected_location
                ))
                .count(),
            1
        );
        let assets = claude_assets(&inventory, AgentAssetCategory::Skill, native_id);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].effective_state, AgentAssetState::Unknown);
    }
    for native_id in [
        "indicator-key-percent",
        "indicator-key-at",
        "indicator-key-backtick",
        "indicator-value-dash",
        "indicator-value-question",
        "indicator-value-percent",
        "indicator-value-at",
        "indicator-value-backtick",
    ] {
        let declarations = claude_declarations(&inventory, AgentAssetCategory::Skill, native_id);
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].declared_state,
            AgentAssetDeclaredState::Unknown
        );
        let source = inventory
            .sources
            .iter()
            .find(|source| source.id == declarations[0].source_id)
            .expect("skill source for indicator declaration");
        assert_eq!(
            source
                .diagnostics
                .iter()
                .filter(|item| matches!(
                    item,
                    AgentAssetDiagnostic::Malformed {
                        format: AgentAssetDocumentFormat::Yaml,
                        location: Some(location),
                    } if location == "frontmatter.syntax"
                ))
                .count(),
            1
        );
        assert_eq!(
            claude_assets(&inventory, AgentAssetCategory::Skill, native_id).len(),
            1
        );
        assert_eq!(
            claude_assets(&inventory, AgentAssetCategory::Skill, native_id)[0].effective_state,
            AgentAssetState::Unknown
        );
    }
    assert!(claude_declarations(&inventory, AgentAssetCategory::Skill, "missing").is_empty());
    assert!(claude_assets(&inventory, AgentAssetCategory::Skill, "missing").is_empty());
    let skill_declarations = inventory
        .declarations
        .iter()
        .filter(|item| item.native_kind == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    assert_eq!(skill_source_paths.len(), 20);
    for (directory, native_id) in [
        (root.join(".claude/skills/symlinked"), "symlinked"),
        (
            workspace.join(".claude/skills/workspace-link"),
            "workspace-link",
        ),
    ] {
        assert!(
            skill_source_paths.contains(&directory.join("SKILL.md").to_string_lossy().into_owned())
        );
        // A linked candidate is observable, but absent snapshot evidence must
        // not fabricate a parsed skill or an enabled asset.
        assert!(claude_declarations(&inventory, AgentAssetCategory::Skill, native_id).is_empty());
        assert!(claude_assets(&inventory, AgentAssetCategory::Skill, native_id).is_empty());
    }
    let cache_source = inventory
        .sources
        .iter()
        .find(|source| source.path.replace('\\', "/").ends_with("plugins/cache"))
        .expect("Claude plugin cache source");
    let marketplace_source = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("known_marketplaces.json"))
        .expect("Claude marketplace source");
    assert!(cache_source.categories.is_empty());
    assert!(marketplace_source.categories.is_empty());
    assert!(skill_declarations.iter().all(|item| {
        item.source_id != cache_source.id && item.source_id != marketplace_source.id
    }));
    assert!(inventory
        .sources
        .iter()
        .filter(|source| { source.path.ends_with("SKILL.md") })
        .all(|source| {
            !source.path.starts_with(&cache_source.path)
                && !source.path.starts_with(&marketplace_source.path)
        }));
    assert!(!skill_declarations
        .iter()
        .any(|item| item.native_id == "cache-skill" || item.native_id == "marketplace-skill"));

    let skill_parent = directory_source(root.join(".claude/skills"), root.join(".claude"));
    let mut follow_up_break = BreakFollowUpSourceOutput::default();
    claude_test_discover_follow_up_sources(
        AgentFollowUpSourceDiscoveryRequest {
            parent: &skill_parent,
            manifest: &claude_entries([
                ("manual", AgentAssetSourceKind::Directory, false),
                ("automatic", AgentAssetSourceKind::Directory, false),
            ]),
        },
        &mut follow_up_break,
    );
    assert_eq!(follow_up_break.emissions, 1);
    assert_eq!(
        follow_up_break.stop,
        Some(crate::services::agent_cli::contracts::AgentOutputStop::SourceLimit)
    );
    assert_claude_source_callback_stops_after_break(&root, &workspace);

    let reversed_files = files
        .iter()
        .rev()
        .map(|(key, bytes)| (*key, bytes.clone()))
        .collect::<Vec<_>>();
    let reversed_directories = directories
        .iter()
        .rev()
        .map(|(key, entries)| {
            let mut entries = entries.clone();
            entries.reverse();
            (*key, entries)
        })
        .collect::<Vec<_>>();
    let (_, _, reversed, _) = build_claude_inventory_at_root(
        root.clone(),
        reversed_files,
        reversed_directories,
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        claude_full_public_signature(&inventory),
        claude_full_public_signature(&reversed)
    );
    let (untrusted_root, untrusted_workspace, untrusted, _) = build_claude_inventory(
        "claude-skill-follow-up-untrusted",
        files
            .iter()
            .map(|(key, bytes)| {
                if *key == "account" {
                    (
                        *key,
                        claude_json(serde_json::json!({
                            "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
                        })),
                    )
                } else {
                    (*key, bytes.clone())
                }
            })
            .collect::<Vec<_>>(),
        directories,
        [],
        &RealInstallationDiscoveryPort,
    );
    let untrusted_context = untrusted.contexts.first().expect("untrusted skill context");
    assert_eq!(untrusted_context.trust_context, AgentTrustState::Untrusted);
    let untrusted_manual = claude_declarations(&untrusted, AgentAssetCategory::Skill, "manual");
    assert_eq!(untrusted_manual.len(), 2);
    assert!(untrusted_manual.iter().any(|item| {
        item.scope == AgentAssetScope::User
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Participates
            )
    }));
    assert!(untrusted_manual.iter().any(|item| {
        item.scope == AgentAssetScope::Workspace
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }));
    let untrusted_manual_asset = claude_assets(&untrusted, AgentAssetCategory::Skill, "manual");
    assert_eq!(untrusted_manual_asset.len(), 1);
    assert_eq!(
        untrusted_manual_asset[0].resolution.contributor_ids.len(),
        1
    );
    let untrusted_contributor = untrusted_manual
        .iter()
        .find(|item| item.id == untrusted_manual_asset[0].resolution.contributor_ids[0])
        .expect("untrusted user skill contributor");
    assert_eq!(
        untrusted_manual_asset[0].inspection_source_id,
        untrusted_contributor.source_id
    );
    let mut untrusted_represented = untrusted_manual_asset[0]
        .represented_declaration_ids
        .clone();
    untrusted_represented.sort();
    let mut untrusted_manual_ids = untrusted_manual
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    untrusted_manual_ids.sort();
    assert_eq!(untrusted_represented, untrusted_manual_ids);
    assert_eq!(
        untrusted_manual_asset[0].resolution.contributor_ids,
        vec![untrusted_contributor.id.clone()]
    );
    assert!(matches!(
        untrusted_manual_asset[0].details,
        AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled,
            invocation_policy: AgentSkillInvocationPolicy::ManualOnly,
        }
    ));
    let _ = (untrusted_root, untrusted_workspace);
    remove_claude_fixture(root);
}

#[test]
fn claude_hook_identity_uses_matcher_shape_without_matcher_content() {
    let context = claude_focused_context();
    let source = claude_focused_source(
        "settings",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::Hook],
    );
    let parse = |group: serde_json::Value| {
        let snapshot = AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": { "PostToolUse": [group] }
            })),
            revision: AgentAssetRevision::default(),
        };
        let mut output = NativePayloadDebugCollector::default();
        claude_test_parse(
            AgentAssetParseRequest {
                context: &context,
                source: &source,
                snapshot: &snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert_eq!(output.declarations.len(), 3);
        assert_eq!(
            output
                .declarations
                .iter()
                .filter(|declaration| { declaration.role == AgentAssetDeclarationRole::Definition })
                .count(),
            1
        );
        output
    };

    fn hook_definition(
        parsed: &NativePayloadDebugCollector,
    ) -> &crate::services::agent_cli::contracts::ParsedAgentAsset {
        parsed
            .declarations
            .iter()
            .find(|declaration| {
                declaration.role == AgentAssetDeclarationRole::Definition
                    && declaration.category == AgentAssetCategory::Hook
            })
            .expect("exact Hook definition")
    }

    let absent = parse(serde_json::json!({
        "hooks": [{ "type": "command", "command": "safe-command" }]
    }));
    let explicit_null = parse(serde_json::json!({
        "matcher": null,
        "hooks": [{ "type": "command", "command": "safe-command" }]
    }));
    let string_a = parse(serde_json::json!({
        "matcher": "matcher-secret-a",
        "hooks": [{ "type": "command", "command": "safe-command" }]
    }));
    let string_b = parse(serde_json::json!({
        "matcher": "matcher-secret-b",
        "hooks": [{ "type": "command", "command": "safe-command" }]
    }));
    let invalid = parse(serde_json::json!({
        "matcher": 7,
        "hooks": [{ "type": "command", "command": "safe-command" }]
    }));

    assert!(hook_definition(&absent)
        .native_id
        .contains("matcher-absent"));
    assert!(hook_definition(&explicit_null)
        .native_id
        .contains("matcher-null"));
    assert!(hook_definition(&string_a)
        .native_id
        .contains("matcher-string-present"));
    assert!(hook_definition(&invalid)
        .native_id
        .contains("matcher-invalid"));
    assert_ne!(
        hook_definition(&absent).declaration_id,
        hook_definition(&explicit_null).declaration_id
    );
    assert_ne!(
        hook_definition(&explicit_null).declaration_id,
        hook_definition(&string_a).declaration_id
    );
    assert_ne!(
        hook_definition(&string_a).declaration_id,
        hook_definition(&invalid).declaration_id
    );
    assert_eq!(
        hook_definition(&string_a).declaration_id,
        hook_definition(&string_b).declaration_id
    );
    let public = format!("{:?}{:?}", string_a.declarations, string_a.facts);
    assert!(!public.contains("matcher-secret-a"));
    assert!(!public.contains("safe-command"));
}

#[test]
fn claude_settings_parser_stops_after_first_break() {
    let context = claude_focused_context();
    let source = claude_focused_source(
        "settings",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::Hook, AgentAssetCategory::StatusUi],
    );
    let snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "hooks": {
                "PostToolUse": [{
                    "hooks": [{ "type": "command", "command": "safe-command" }]
                }]
            },
            "statusLine": { "type": "command", "command": "safe-status" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let mut output = BreakParseOutput::default();
    claude_test_parse(
        AgentAssetParseRequest {
            context: &context,
            source: &source,
            snapshot: &snapshot,
            native_home: None,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.emissions, 1);
    assert_eq!(output.declarations.len(), 1);
    assert_eq!(output.declarations[0].category, AgentAssetCategory::Hook);
    assert_eq!(
        output.declarations[0].role,
        AgentAssetDeclarationRole::PolicyOverlay
    );
    assert_eq!(
        output.declarations[0].declaration_key,
        "control:hook:disableAllHooks"
    );
    assert!(!output
        .declarations
        .iter()
        .any(|declaration| { declaration.declaration_key == "control:hook:source" }));
}

#[test]
fn claude_hook_policy_schema_diagnostics_are_typed_and_authoritative() {
    let context = claude_focused_context();
    let parse = |source: AgentAssetSourceSpec, value: serde_json::Value| {
        let snapshot = AgentAssetSnapshot::File {
            bytes: claude_json(value),
            revision: AgentAssetRevision::default(),
        };
        let mut output = NativePayloadDebugCollector::default();
        claude_test_parse(
            AgentAssetParseRequest {
                context: &context,
                source: &source,
                snapshot: &snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        output.diagnostics
    };
    let has_json_malformed = |diagnostics: &[AgentAssetDiagnostic], field: &str| {
        diagnostics
            .iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic,
                    AgentAssetDiagnostic::Malformed {
                        format: crate::models::AgentAssetDocumentFormat::Json,
                        location: Some(location),
                    }
                        if location == field
                )
            })
            .count()
    };

    let hooks = parse(
        claude_focused_source(
            "settings",
            AgentAssetScope::User,
            10,
            &[AgentAssetCategory::Hook],
        ),
        serde_json::json!({ "hooks": [] }),
    );
    assert_eq!(has_json_malformed(&hooks, "hooks"), 1);
    for categories in [
        vec![AgentAssetCategory::Hook],
        vec![AgentAssetCategory::StatusUi],
        vec![AgentAssetCategory::Hook, AgentAssetCategory::StatusUi],
    ] {
        let disable = parse(
            claude_focused_source("settings", AgentAssetScope::User, 10, &categories),
            serde_json::json!({ "disableAllHooks": "yes" }),
        );
        assert_eq!(has_json_malformed(&disable, "disableAllHooks"), 1);
    }
    let managed = parse(
        claude_focused_source(
            "managed-settings",
            AgentAssetScope::Managed,
            40,
            &[AgentAssetCategory::Hook],
        ),
        serde_json::json!({ "allowManagedHooksOnly": "yes" }),
    );
    assert_eq!(has_json_malformed(&managed, "allowManagedHooksOnly"), 1);
    for source_key in ["settings", "workspace-settings", "workspace-local-settings"] {
        let diagnostics = parse(
            claude_focused_source(
                source_key,
                if source_key == "settings" {
                    AgentAssetScope::User
                } else if source_key == "workspace-settings" {
                    AgentAssetScope::Workspace
                } else {
                    AgentAssetScope::Local
                },
                if source_key == "settings" {
                    10
                } else if source_key == "workspace-settings" {
                    20
                } else {
                    30
                },
                &[AgentAssetCategory::Hook],
            ),
            serde_json::json!({ "allowManagedHooksOnly": "invalid" }),
        );
        assert_eq!(has_json_malformed(&diagnostics, "allowManagedHooksOnly"), 0);
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::UnknownField { field_path }
                        if field_path == "allowManagedHooksOnly"
                ))
                .count(),
            1
        );
    }
    let missing = parse(
        claude_focused_source(
            "settings",
            AgentAssetScope::User,
            10,
            &[AgentAssetCategory::Hook],
        ),
        serde_json::json!({}),
    );
    assert!(missing.is_empty());
}

#[test]
fn claude_non_managed_hook_policy_is_inert_on_real_hook_asset() {
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-non-managed-hook-policy",
        [(
            "settings",
            claude_json(serde_json::json!({
                "allowManagedHooksOnly": "invalid",
                "hooks": {
                    "PostToolUse": [{
                        "hooks": [{ "type": "command", "command": "safe-hook" }]
                    }]
                }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let diagnostics = inventory
        .sources
        .iter()
        .flat_map(|source| source.diagnostics.iter())
        .chain(
            inventory
                .declarations
                .iter()
                .flat_map(|item| item.diagnostics.iter()),
        )
        .chain(
            inventory
                .assets
                .iter()
                .flat_map(|item| item.diagnostics.iter()),
        )
        .chain(
            inventory
                .assets
                .iter()
                .flat_map(|item| item.resolution.diagnostics.iter()),
        )
        .chain(inventory.diagnostics.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::UnknownField { field_path }
                    if field_path == "allowManagedHooksOnly"
            ))
            .count(),
        1
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed {
                    format: crate::models::AgentAssetDocumentFormat::Json,
                    location: Some(location),
                } if location == "allowManagedHooksOnly"
            ))
            .count(),
        0
    );

    let hooks = inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 1, "{inventory:?}");
    assert_eq!(
        hooks[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(hooks[0].resolution.control_source.is_none());
    assert_eq!(hooks[0].effective_state, AgentAssetState::Enabled);
    assert!(matches!(
        hooks[0].details,
        AgentAssetDetails::Hook {
            managed: false,
            enabled: AgentAssetDeclaredState::Enabled,
            rule_count: Some(1),
        }
    ));
    remove_claude_fixture(root);
}

#[test]
fn claude_status_policy_precedes_tie_and_managed_hook_policy_fails_closed() {
    let context = claude_focused_context();
    let control_source = claude_focused_source(
        "settings",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi, AgentAssetCategory::Hook],
    );
    let status_a_source = claude_focused_source(
        "status-a",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let status_b_source = claude_focused_source(
        "status-b",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let policy_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({ "disableAllHooks": true })),
        revision: AgentAssetRevision::default(),
    };
    let status_a_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "status-a" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let status_b_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "status-b" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let mut declarations = Vec::new();
    for (source, snapshot) in [
        (&control_source, &policy_snapshot),
        (&status_a_source, &status_a_snapshot),
        (&status_b_source, &status_b_snapshot),
    ] {
        let mut parsed = NativePayloadDebugCollector::default();
        claude_test_parse(
            AgentAssetParseRequest {
                context: &context,
                source,
                snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        declarations.extend(parsed.declarations);
    }
    assert_eq!(declarations.len(), 6);
    let status_policy = declarations
        .iter()
        .find(|declaration| {
            declaration.source_key == "settings"
                && declaration.declaration_key == "control:statusUi:disableAllHooks"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .expect("parsed Status policy field");
    let sources = [
        AgentAssetResolveSource {
            spec: &control_source,
            snapshot: &policy_snapshot,
        },
        AgentAssetResolveSource {
            spec: &status_a_source,
            snapshot: &status_a_snapshot,
        },
        AgentAssetResolveSource {
            spec: &status_b_source,
            snapshot: &status_b_snapshot,
        },
    ];
    let mut output = BreakResolveOutput::default();
    claude_test_resolve(
        AgentAssetResolveRequest {
            context: &context,
            declarations: &declarations,
            sources: &sources,
        },
        &mut output,
    );
    assert_eq!(output.emissions, 1);
    assert_eq!(output.drafts.len(), 1);
    assert_eq!(
        output.drafts[0].resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert!(matches!(
        output.drafts[0].resolution.control_source,
        Some(crate::services::agent_cli::contracts::AgentAssetPolicyReferenceDraft::Declaration {
            ref declaration_id
        }) if declaration_id == &status_policy.declaration_id
    ));

    assert_eq!(output.drafts[0].effective_state, AgentAssetState::Blocked);
    assert_eq!(
        output.drafts[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    assert_eq!(output.drafts[0].represented_declaration_ids.len(), 2);

    let user_source = claude_focused_source(
        "settings",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::Hook],
    );
    let managed_source = claude_focused_source(
        "managed-settings",
        AgentAssetScope::Managed,
        40,
        &[AgentAssetCategory::Hook],
    );
    let user_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "safe" }] }] }
        })),
        revision: AgentAssetRevision::default(),
    };
    let managed_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({ "allowManagedHooksOnly": "invalid" })),
        revision: AgentAssetRevision::default(),
    };
    let mut parsed = NativePayloadDebugCollector::default();
    for (source, snapshot) in [
        (&user_source, &user_snapshot),
        (&managed_source, &managed_snapshot),
    ] {
        claude_test_parse(
            AgentAssetParseRequest {
                context: &context,
                source,
                snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
    }
    assert_eq!(parsed.declarations.len(), 6);
    let managed_policy = parsed
        .declarations
        .iter()
        .find(|declaration| {
            declaration.source_key == "managed-settings"
                && declaration.declaration_key == "control:hook:allowManagedHooksOnly"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
        })
        .expect("parsed managed Hook policy field");
    let hook_sources = [
        AgentAssetResolveSource {
            spec: &user_source,
            snapshot: &user_snapshot,
        },
        AgentAssetResolveSource {
            spec: &managed_source,
            snapshot: &managed_snapshot,
        },
    ];
    let mut hook_output = BreakResolveOutput::default();
    claude_test_resolve(
        AgentAssetResolveRequest {
            context: &context,
            declarations: &parsed.declarations,
            sources: &hook_sources,
        },
        &mut hook_output,
    );
    assert_eq!(hook_output.emissions, 1);
    assert_eq!(hook_output.drafts.len(), 1);
    assert_eq!(
        hook_output.drafts[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(matches!(
        hook_output.drafts[0].resolution.control_source,
        Some(crate::services::agent_cli::contracts::AgentAssetPolicyReferenceDraft::Declaration {
            ref declaration_id
        }) if declaration_id == &managed_policy.declaration_id
    ));
    assert_eq!(
        hook_output.drafts[0].effective_state,
        AgentAssetState::Blocked
    );
    assert_eq!(
        hook_output.drafts[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
}

#[test]
fn claude_status_policy_failures_keep_complete_tie_with_exact_invalid_owner() {
    let context = claude_focused_context();
    let control_source = claude_focused_source(
        "settings",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let status_a_source = claude_focused_source(
        "status-a",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let status_b_source = claude_focused_source(
        "status-b",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let status_a_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "status-a" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let status_b_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "status-b" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let policies = [
        (
            "malformed",
            AgentAssetSnapshot::File {
                bytes: b"{".to_vec(),
                revision: AgentAssetRevision::default(),
            },
        ),
        (
            "blocked",
            AgentAssetSnapshot::Blocked {
                revision: AgentAssetRevision::default(),
                diagnostic: AgentAssetDiagnostic::ReadFailed {
                    source_id: "settings".to_owned(),
                    error_kind: crate::models::AgentAssetIoErrorKind::Other,
                },
            },
        ),
        (
            "wrong-typed",
            AgentAssetSnapshot::File {
                bytes: claude_json(serde_json::json!({
                    "disableAllHooks": "invalid"
                })),
                revision: AgentAssetRevision::default(),
            },
        ),
    ];

    for (case, policy_snapshot) in policies {
        let mut declarations = Vec::new();
        for (source, snapshot) in [
            (&control_source, &policy_snapshot),
            (&status_a_source, &status_a_snapshot),
            (&status_b_source, &status_b_snapshot),
        ] {
            let mut parsed = NativePayloadDebugCollector::default();
            claude_test_parse(
                AgentAssetParseRequest {
                    context: &context,
                    source,
                    snapshot,
                    native_home: None,
                    workspace_canonical: None,
                    workspace_lexical: None,
                },
                &mut parsed,
            );
            declarations.extend(parsed.declarations);
        }
        assert_eq!(
            declarations.len(),
            if case == "wrong-typed" { 4 } else { 3 }
        );
        let expected_key = if case == "wrong-typed" {
            "control:statusUi:disableAllHooks"
        } else {
            "control:statusUi:source"
        };
        let policy = declarations
            .iter()
            .find(|declaration| {
                declaration.source_key == "settings"
                    && declaration.declaration_key == expected_key
                    && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
            })
            .expect("parsed invalid Status policy entity");
        let sources = [
            AgentAssetResolveSource {
                spec: &control_source,
                snapshot: &policy_snapshot,
            },
            AgentAssetResolveSource {
                spec: &status_a_source,
                snapshot: &status_a_snapshot,
            },
            AgentAssetResolveSource {
                spec: &status_b_source,
                snapshot: &status_b_snapshot,
            },
        ];
        let mut output = BreakResolveOutput::default();
        claude_test_resolve(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &declarations,
                sources: &sources,
            },
            &mut output,
        );
        assert_eq!(output.emissions, 1);
        assert_eq!(output.drafts.len(), 1);
        let draft = &output.drafts[0];
        assert_eq!(
            draft.resolution.relation,
            AgentAssetResolutionRelation::Unknown
        );
        assert!(draft.resolution.winner.is_none());
        assert!(matches!(
            draft.resolution.control_source,
            Some(crate::services::agent_cli::contracts::AgentAssetPolicyReferenceDraft::Declaration {
                ref declaration_id
            }) if declaration_id == &policy.declaration_id
        ));
        assert_eq!(draft.represented_declaration_ids.len(), 2);
        assert_eq!(draft.effective_state, AgentAssetState::Unknown);
        assert_eq!(
            draft.resolution.terminal,
            Some(AgentAssetResolutionTerminal::Unknown)
        );
        assert!(matches!(
            draft.details,
            AgentAssetDetails::StatusUi {
                mode: AgentStatusUiMode::Unknown,
                command_present: false,
            }
        ));
    }
}

#[test]
fn claude_hooks_are_additive_and_policy_gated() {
    let account = claude_json(serde_json::json!({
        "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
    }));
    let files = vec![
        ("account", account.clone()),
        (
            "settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "PostToolUse": [
                        { "hooks": [{ "type": "command", "command": "hook-user-command-secret" }] },
                        { "hooks": [{ "type": "prompt", "prompt": "hook-user-prompt-secret" }] },
                        { "matcher": "hook-user-matcher-secret", "hooks": [{ "type": "http", "url": "https://hook-user-url-secret", "headers": { "X-Hook": "hook-user-header-secret" } }] }
                    ]
                }
            })),
        ),
        (
            "workspace-settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{ "matcher": "hook-workspace-matcher-secret", "hooks": [{ "type": "agent", "prompt": "hook-workspace-agent-secret" }] }]
                }
            })),
        ),
        (
            "workspace-local-settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "Notification": [{ "matcher": "hook-local-matcher-secret", "hooks": [{ "type": "command", "command": "hook-local-command-secret" }] }]
                }
            })),
        ),
        (
            "managed-settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "Stop": [{ "matcher": "hook-managed-matcher-secret", "hooks": [{ "type": "command", "command": "hook-managed-command-secret" }] }]
                }
            })),
        ),
    ];
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-hooks-policy",
        files.clone(),
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let hooks = inventory
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 6, "{inventory:?}");
    assert!(hooks.iter().all(|item| {
        item.resolution.relation == AgentAssetResolutionRelation::Independent
            && item.declared_state == AgentAssetState::Enabled
            && item.effective_state == AgentAssetState::Enabled
            && item.resolution.contributor_ids.len() == 1
            && item.resolution.winner_id.is_none()
            && item.resolution.control_source.is_none()
            && item.represented_declaration_ids.len() == 1
            && item.source_ids.len() == 1
            && item.source_ids.contains(&item.inspection_source_id)
            && item.relationships.provided_by.is_none()
            && item.relationships.action_owner.is_none()
            && item.relationships.affected_asset_ids.is_empty()
            && matches!(
                item.details,
                AgentAssetDetails::Hook {
                    enabled: AgentAssetDeclaredState::Enabled,
                    ..
                }
            )
            && item.actions.iter().all(|action| {
                !matches!(
                    action.action,
                    AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
                ) || !action.available
            })
    }));
    let serialized = serde_json::to_string(&inventory).unwrap();
    for secret in [
        "hook-user-command-secret",
        "hook-user-prompt-secret",
        "hook-user-matcher-secret",
        "hook-user-url-secret",
        "hook-user-header-secret",
        "hook-workspace-matcher-secret",
        "hook-workspace-agent-secret",
        "hook-local-matcher-secret",
        "hook-local-command-secret",
        "hook-managed-matcher-secret",
        "hook-managed-command-secret",
    ] {
        assert!(
            !serialized.contains(secret),
            "Hook payload leaked: {secret}"
        );
    }

    let (_, _, reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        root.clone(),
        files.clone(),
        Vec::<(&'static str, Vec<AgentAssetDirectoryEntry>)>::new(),
        Vec::<(&'static str, AgentAssetDiagnostic)>::new(),
        ClaudeInventoryBuildOptions {
            installations: &RealInstallationDiscoveryPort,
            workspace_input: None,
            settings: None,
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        claude_full_public_signature(&inventory),
        claude_full_public_signature(&reversed)
    );

    let (malformed_root, _, malformed, _) = build_claude_inventory(
        "claude-hooks-malformed-sibling",
        [(
            "settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "PostToolUse": [
                        { "hooks": [{ "type": "command", "command": "valid-sibling" }] },
                        { "hooks": [{ "type": "unknown" }] },
                        { "hooks": [{ "type": "prompt", "prompt": "valid-prompt-sibling" }] }
                    ]
                }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let malformed_hooks = malformed
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(malformed_hooks.len(), 3, "{malformed:?}");
    assert_eq!(
        malformed_hooks
            .iter()
            .filter(|item| {
                item.resolution.relation == AgentAssetResolutionRelation::Independent
                    && item.effective_state == AgentAssetState::Enabled
            })
            .count(),
        2
    );
    assert_eq!(
        malformed_hooks
            .iter()
            .filter(|item| item.effective_state == AgentAssetState::Unknown)
            .count(),
        1
    );
    assert_eq!(
        malformed
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                    if location == "hooks.PostToolUse"
            ))
            .count(),
        1
    );
    remove_claude_fixture(malformed_root);

    let (ordinary_malformed_root, _, ordinary_malformed, _) = build_claude_inventory(
        "claude-hooks-ordinary-malformed-matrix",
        [(
            "settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "PostToolUse": [
                        { "hooks": [{ "type": "command", "command": "ordinary-valid-secret" }] },
                        { "hooks": [{ "type": "unknown" }] },
                        { "hooks": [{ "type": "command" }] },
                        { "hooks": [{ "type": "prompt", "prompt": 7 }] },
                        { "matcher": 7, "hooks": [{ "type": "command", "command": "wrong-matcher-secret" }] },
                        { "hooks": { "type": "command", "command": "wrong-hooks-array-secret" } },
                        { "hooks": [{ "type": "command", "command": "bad-aux-secret", "async": "invalid" }] }
                    ]
                }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let ordinary_malformed_hooks = ordinary_malformed
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(ordinary_malformed_hooks.len(), 7, "{ordinary_malformed:?}");
    assert_eq!(
        ordinary_malformed_hooks
            .iter()
            .filter(|item| item.effective_state == AgentAssetState::Enabled)
            .count(),
        1
    );
    assert_eq!(
        ordinary_malformed_hooks
            .iter()
            .filter(|item| item.effective_state == AgentAssetState::Unknown)
            .count(),
        6
    );
    assert_eq!(
        ordinary_malformed
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                    if location == "hooks.PostToolUse"
            ))
            .count(),
        1
    );
    for (native_id, declared_state, effective_state) in [
        (
            "settings:PostToolUse:matcher-absent:group-0:hook-0",
            AgentAssetDeclaredState::Enabled,
            AgentAssetState::Enabled,
        ),
        (
            "settings:PostToolUse:matcher-absent:group-1:hook-0",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "settings:PostToolUse:matcher-absent:group-2:hook-0",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "settings:PostToolUse:matcher-absent:group-3:hook-0",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "settings:PostToolUse:matcher-invalid:group-4:hook-invalid",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "settings:PostToolUse:matcher-absent:group-5:hook-invalid",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            "settings:PostToolUse:matcher-absent:group-6:hook-0",
            AgentAssetDeclaredState::Unknown,
            AgentAssetState::Unknown,
        ),
    ] {
        let declaration =
            claude_declarations(&ordinary_malformed, AgentAssetCategory::Hook, native_id)
                .into_iter()
                .next()
                .unwrap_or_else(|| {
                    panic!(
                        "ordinary malformed declaration {native_id}: {:?}",
                        ordinary_malformed.declarations
                    )
                });
        assert_hook_asset_binding(
            &ordinary_malformed,
            native_id,
            HookAssetExpectation {
                declared: declared_state,
                effective: effective_state,
                participation: AgentAssetResolutionParticipation::Participates,
                contributors: std::slice::from_ref(&declaration.id),
                resolution: AgentAssetResolutionRelation::Independent,
                control_source: None,
                mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
            },
        );
    }
    let (wrong_hooks_root, _, wrong_hooks, _) = build_claude_inventory(
        "claude-hooks-top-level-array",
        [("settings", claude_json(serde_json::json!({ "hooks": [] })))],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert!(wrong_hooks
        .assets
        .iter()
        .all(|item| item.category != AgentAssetCategory::Hook));
    assert_eq!(
        wrong_hooks
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                    if location == "hooks"
            ))
            .count(),
        1
    );
    remove_claude_fixture(ordinary_malformed_root);
    remove_claude_fixture(wrong_hooks_root);

    let (guard_root, _, guarded, _) = build_claude_inventory(
        "claude-hooks-guards",
        [(
            "settings",
            claude_json(serde_json::json!({
                "hooks": {
                    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "ordinary-valid" }] }],
                    "PreToolUse": [
                        { "hooks": [{ "type": "command", "command": "guard-valid" }] },
                        { "matcher": 7, "hooks": [{ "type": "command", "command": "guard-invalid" }] }
                    ],
                    "PermissionRequest": [
                        { "hooks": [{ "type": "command", "command": "permission-valid" }] },
                        { "hooks": [{ "type": "unknown" }] }
                    ]
                }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let guarded_hooks = guarded
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(guarded_hooks.len(), 3, "{guarded:?}");
    assert!(guarded_hooks.iter().any(|item| {
        item.native_id.contains("PostToolUse")
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
    }));
    for (event, native_id) in [
        (
            "PreToolUse",
            "settings:PreToolUse:matcher-invalid:group-1:hook-invalid",
        ),
        (
            "PermissionRequest",
            "settings:PermissionRequest:matcher-absent:group-1:hook-invalid",
        ),
    ] {
        let source_diagnostics = guarded
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| {
                matches!(
                    diagnostic,
                    AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                        if location == &format!("hooks.{event}")
                )
            })
            .count();
        assert_eq!(source_diagnostics, 1, "{event}: {guarded:?}");
        let declaration = claude_declarations(&guarded, AgentAssetCategory::Hook, native_id)
            .into_iter()
            .next()
            .expect("guard declaration");
        assert_hook_asset_binding(
            &guarded,
            native_id,
            HookAssetExpectation {
                declared: AgentAssetDeclaredState::Unknown,
                effective: AgentAssetState::Unknown,
                participation: AgentAssetResolutionParticipation::Participates,
                contributors: std::slice::from_ref(&declaration.id),
                resolution: AgentAssetResolutionRelation::Independent,
                control_source: None,
                mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
            },
        );
    }
    remove_claude_fixture(guard_root);

    let (trust_root, _, untrusted, _) = build_claude_inventory(
        "claude-hooks-untrusted",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "trusted-user-hook" }] }] }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "suppressed-workspace-hook" }] }] }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "suppressed-local-hook" }] }] }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "managed-hook" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let untrusted_hooks = untrusted
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(untrusted_hooks.len(), 2, "{untrusted:?}");
    assert!(untrusted_hooks
        .iter()
        .filter(|item| item.effective_state == AgentAssetState::Enabled)
        .all(|item| {
            item.resolution.contributor_ids.len() == 1
                && item.resolution.relation == AgentAssetResolutionRelation::Independent
        }));
    assert!(untrusted.declarations.iter().any(|item| {
        item.native_kind == AgentAssetCategory::Hook
            && matches!(
                item.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }));
    for (native_id, participation, effective_state, has_contributor) in [
        (
            "settings:PostToolUse:matcher-absent:group-0:hook-0",
            AgentAssetResolutionParticipation::Participates,
            AgentAssetState::Enabled,
            true,
        ),
        (
            "workspace-settings:PostToolUse:matcher-absent:group-0:hook-0",
            AgentAssetResolutionParticipation::Suppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
            },
            AgentAssetState::Unknown,
            false,
        ),
        (
            "workspace-local-settings:PostToolUse:matcher-absent:group-0:hook-0",
            AgentAssetResolutionParticipation::Suppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
            },
            AgentAssetState::Unknown,
            false,
        ),
        (
            "managed-settings:Stop:matcher-absent:group-0:hook-0",
            AgentAssetResolutionParticipation::Participates,
            AgentAssetState::Enabled,
            true,
        ),
    ] {
        let declaration = claude_declarations(&untrusted, AgentAssetCategory::Hook, native_id)
            .into_iter()
            .next()
            .expect("untrusted Hook declaration");
        if has_contributor {
            let expected_contributors = vec![declaration.id.clone()];
            assert_hook_asset_binding(
                &untrusted,
                native_id,
                HookAssetExpectation {
                    declared: AgentAssetDeclaredState::Enabled,
                    effective: effective_state,
                    participation,
                    contributors: &expected_contributors,
                    resolution: AgentAssetResolutionRelation::Independent,
                    control_source: None,
                    mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
                },
            );
        } else {
            assert!(matches!(
                participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ));
            assert!(claude_assets(&untrusted, AgentAssetCategory::Hook, native_id).is_empty());
        }
    }
    remove_claude_fixture(trust_root);

    let (unknown_trust_root, _, unknown_trust, _) = build_claude_inventory(
        "claude-hooks-unknown-trust",
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "unknown-trust-user-secret" }] }] }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "hooks": { "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "unknown-trust-workspace-secret" }] }] }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "hooks": { "Notification": [{ "hooks": [{ "type": "command", "command": "unknown-trust-local-secret" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        unknown_trust.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    let unknown_trust_hooks = unknown_trust
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(unknown_trust_hooks.len(), 1, "{unknown_trust:?}");
    for declaration in unknown_trust.declarations.iter().filter(|item| {
        item.native_kind == AgentAssetCategory::Hook
            && item.role == AgentAssetDeclarationRole::Definition
    }) {
        if declaration.scope == AgentAssetScope::User {
            assert!(matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            ));
            let expected_contributors = vec![declaration.id.clone()];
            assert_hook_asset_binding(
                &unknown_trust,
                &declaration.native_id,
                HookAssetExpectation {
                    declared: AgentAssetDeclaredState::Enabled,
                    effective: AgentAssetState::Enabled,
                    participation: declaration.participation,
                    contributors: &expected_contributors,
                    resolution: AgentAssetResolutionRelation::Independent,
                    control_source: None,
                    mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
                },
            );
        } else {
            assert!(matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ));
            assert!(claude_assets(
                &unknown_trust,
                AgentAssetCategory::Hook,
                &declaration.native_id,
            )
            .is_empty());
        }
    }
    remove_claude_fixture(unknown_trust_root);

    let run_policy_case =
        |name: &str,
         policy: Option<Vec<u8>>,
         blocked: Vec<(&'static str, AgentAssetDiagnostic)>,
         expected: AgentAssetResolutionRelation,
         expected_terminal: Option<crate::models::AgentAssetResolutionTerminal>,
         expected_state: AgentAssetState,
         expected_control_source_key: Option<(&str, &str)>,
         expected_diagnostic_source_key: Option<&str>,
         expected_source_diagnostic: Option<AgentAssetDiagnostic>| {
            let mut case_files = vec![(
                "managed-settings",
                claude_json(serde_json::json!({
                    "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "policy-managed-hook" }] }] }
                })),
            )];
            if let Some(policy) = policy {
                case_files.push(("settings", policy));
            }
            let (case_root, _, case_inventory, _) = build_claude_inventory(
                name,
                case_files,
                [],
                blocked,
                &RealInstallationDiscoveryPort,
            );
            let case_hooks = case_inventory
                .assets
                .iter()
                .filter(|item| item.category == AgentAssetCategory::Hook)
                .collect::<Vec<_>>();
            assert_eq!(case_hooks.len(), 1, "{case_inventory:?}");
            let control_source_id = expected_control_source_key.map(|(source_key, key)| {
                let source = case_inventory
                    .sources
                    .iter()
                    .find(|source| match source_key {
                        "settings" => {
                            source.scope == AgentAssetScope::User
                                && source.path.ends_with("settings.json")
                        }
                        "managed-settings" => {
                            source.scope == AgentAssetScope::Managed
                                && source.path.ends_with("managed-settings.json")
                        }
                        _ => false,
                    })
                    .expect("policy source");
                (source.id.as_str(), key)
            });
            let expected_source_diagnostic = expected_source_diagnostic.map(|diagnostic| {
                let source_key = expected_diagnostic_source_key.expect("diagnostic source");
                let source = case_inventory
                    .sources
                    .iter()
                    .find(|source| match source_key {
                        "settings" => {
                            source.scope == AgentAssetScope::User
                                && source.path.ends_with("settings.json")
                        }
                        "managed-settings" => {
                            source.scope == AgentAssetScope::Managed
                                && source.path.ends_with("managed-settings.json")
                        }
                        _ => false,
                    })
                    .expect("diagnostic source");
                rebind_source_id(diagnostic, &source.id)
            });
            let expected_resolution_diagnostics = if expected_terminal
                == Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked)
            {
                vec![AgentAssetDiagnostic::PolicyBlocked]
            } else {
                Vec::new()
            };
            assert_hook_policy_binding(
                &case_inventory,
                "managed-settings:Stop:matcher-absent:group-0:hook-0",
                HookPolicyExpectation {
                    resolution: expected,
                    terminal: expected_terminal,
                    effective: expected_state,
                    control_source_id,
                    resolution_diagnostics: &expected_resolution_diagnostics,
                    source_diagnostic: expected_source_diagnostic.as_ref(),
                    managed: true,
                    hook_enabled: AgentAssetDeclaredState::Enabled,
                },
            );
            let source_diagnostics = case_inventory
                .sources
                .iter()
                .flat_map(|source| source.diagnostics.iter())
                .collect::<Vec<_>>();
            if let Some(expected) = expected_source_diagnostic.as_ref() {
                let source_unavailable =
                    matches!(expected,
                        AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                            if location == "root"
                    ) || matches!(expected, AgentAssetDiagnostic::ReadFailed { .. });
                let mut expected_diagnostics = vec![expected.clone()];
                if source_unavailable {
                    expected_diagnostics.push(AgentAssetDiagnostic::DiscoveryIncomplete {
                        agent_kind: AgentCliKind::ClaudeCode,
                        category: AgentAssetCategory::Hook,
                        reason:
                            crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                    });
                }
                assert_eq!(
                    source_diagnostics.len(),
                    expected_diagnostics.len(),
                    "{name}: {case_inventory:?}"
                );
                let source_key = expected_diagnostic_source_key.expect("diagnostic source");
                let source = case_inventory
                    .sources
                    .iter()
                    .find(|source| match source_key {
                        "settings" => {
                            source.scope == AgentAssetScope::User
                                && source.path.ends_with("settings.json")
                        }
                        "managed-settings" => {
                            source.scope == AgentAssetScope::Managed
                                && source.path.ends_with("managed-settings.json")
                        }
                        _ => false,
                    })
                    .expect("diagnostic source");
                assert_eq!(
                    source.diagnostics.len(),
                    expected_diagnostics.len(),
                    "{name}: {case_inventory:?}"
                );
                assert!(
                    expected_diagnostics
                        .iter()
                        .all(|diagnostic| source.diagnostics.contains(diagnostic)),
                    "{name}: {case_inventory:?}"
                );
            } else {
                assert!(source_diagnostics.is_empty(), "{case_inventory:?}");
            }
            remove_claude_fixture(case_root);
        };
    run_policy_case(
        "claude-hooks-disable-true",
        Some(claude_json(serde_json::json!({ "disableAllHooks": true }))),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::PolicyBlocked),
        AgentAssetState::Blocked,
        Some(("settings", "control:hook:disableAllHooks")),
        None,
        None,
    );
    run_policy_case(
        "claude-hooks-disable-false",
        Some(claude_json(serde_json::json!({ "disableAllHooks": false }))),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        None,
        AgentAssetState::Enabled,
        None,
        None,
        None,
    );
    run_policy_case(
        "claude-hooks-disable-wrong-type",
        Some(claude_json(
            serde_json::json!({ "disableAllHooks": "invalid" }),
        )),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        Some(("settings", "control:hook:disableAllHooks")),
        Some("settings"),
        Some(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("disableAllHooks".to_owned()),
        }),
    );
    run_policy_case(
        "claude-hooks-disable-malformed",
        Some(b"{".to_vec()),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        Some(("settings", "control:hook:source")),
        Some("settings"),
        Some(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("root".to_owned()),
        }),
    );
    run_policy_case(
        "claude-hooks-disable-blocked",
        None,
        vec![(
            "settings",
            AgentAssetDiagnostic::ReadFailed {
                source_id: "settings".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        )],
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        Some(("settings", "control:hook:source")),
        Some("settings"),
        Some(AgentAssetDiagnostic::ReadFailed {
            source_id: "managed-settings".to_owned(),
            error_kind: crate::models::AgentAssetIoErrorKind::Other,
        }),
    );

    let (managed_only_root, _, managed_only, _) = build_claude_inventory(
        "claude-hooks-managed-only",
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "non-managed-hook" }] }] }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": true,
                    "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "managed-only-hook" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let managed_only_hooks = managed_only
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(managed_only_hooks.len(), 2, "{managed_only:?}");
    assert!(managed_only_hooks.iter().any(|item| {
        item.effective_state == AgentAssetState::Blocked
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
            && item.resolution.terminal == Some(AgentAssetResolutionTerminal::PolicyBlocked)
            && item.resolution.control_source.is_some()
    }));
    assert!(managed_only_hooks.iter().any(|item| {
        item.effective_state == AgentAssetState::Enabled
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
    }));
    let managed_control_source = managed_only
        .sources
        .iter()
        .find(|source| {
            source.scope == AgentAssetScope::Managed
                && source.path.ends_with("managed-settings.json")
        })
        .expect("managed-only policy source");
    assert_hook_policy_binding(
        &managed_only,
        "settings:PostToolUse:matcher-absent:group-0:hook-0",
        HookPolicyExpectation {
            resolution: AgentAssetResolutionRelation::Independent,
            terminal: Some(AgentAssetResolutionTerminal::PolicyBlocked),
            effective: AgentAssetState::Blocked,
            control_source_id: Some((
                &managed_control_source.id,
                "control:hook:allowManagedHooksOnly",
            )),
            resolution_diagnostics: &[AgentAssetDiagnostic::PolicyBlocked],
            source_diagnostic: None,
            managed: false,
            hook_enabled: AgentAssetDeclaredState::Enabled,
        },
    );
    let managed_declaration = claude_declarations(
        &managed_only,
        AgentAssetCategory::Hook,
        "managed-settings:Stop:matcher-absent:group-0:hook-0",
    )[0];
    assert_hook_asset_binding(
        &managed_only,
        "managed-settings:Stop:matcher-absent:group-0:hook-0",
        HookAssetExpectation {
            declared: AgentAssetDeclaredState::Enabled,
            effective: AgentAssetState::Enabled,
            participation: AgentAssetResolutionParticipation::Participates,
            contributors: std::slice::from_ref(&managed_declaration.id),
            resolution: AgentAssetResolutionRelation::Independent,
            control_source: None,
            mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    assert_no_invalid_resolution_diagnostics(&managed_only);
    remove_claude_fixture(managed_only_root);

    let (managed_false_root, _, managed_false, _) = build_claude_inventory(
        "claude-hooks-managed-only-false",
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "managed-false-user" }] }] }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": false,
                    "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "managed-false-managed" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let managed_false_hooks = managed_false
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(managed_false_hooks.len(), 2, "{managed_false:?}");
    assert!(managed_false_hooks.iter().all(|item| {
        item.effective_state == AgentAssetState::Enabled
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
            && item.resolution.control_source.is_none()
    }));
    for native_id in [
        "settings:PostToolUse:matcher-absent:group-0:hook-0",
        "managed-settings:Stop:matcher-absent:group-0:hook-0",
    ] {
        let declaration = claude_declarations(&managed_false, AgentAssetCategory::Hook, native_id)
            .into_iter()
            .next()
            .expect("managed-only false declaration");
        assert_hook_asset_binding(
            &managed_false,
            native_id,
            HookAssetExpectation {
                declared: AgentAssetDeclaredState::Enabled,
                effective: AgentAssetState::Enabled,
                participation: AgentAssetResolutionParticipation::Participates,
                contributors: std::slice::from_ref(&declaration.id),
                resolution: AgentAssetResolutionRelation::Independent,
                control_source: None,
                mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
            },
        );
    }
    assert_no_invalid_resolution_diagnostics(&managed_false);
    remove_claude_fixture(managed_false_root);

    let (managed_blocked_root, _, managed_blocked, _) = build_claude_inventory(
        "claude-hooks-managed-only-blocked",
        [(
            "settings",
            claude_json(serde_json::json!({
                "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "managed-blocked-user" }] }] }
            })),
        )],
        [],
        [(
            "managed-settings",
            AgentAssetDiagnostic::ReadFailed {
                source_id: "managed-settings".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
            },
        )],
        &RealInstallationDiscoveryPort,
    );
    let managed_blocked_hooks = managed_blocked
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    let managed_blocked_source = managed_blocked
        .sources
        .iter()
        .find(|source| {
            source.scope == AgentAssetScope::Managed
                && source.path.ends_with("managed-settings.json")
        })
        .expect("managed-only blocked source");
    let expected_managed_blocked_diagnostics = vec![
        AgentAssetDiagnostic::ReadFailed {
            source_id: managed_blocked_source.id.clone(),
            error_kind: crate::models::AgentAssetIoErrorKind::PermissionDenied,
        },
        AgentAssetDiagnostic::DiscoveryIncomplete {
            agent_kind: AgentCliKind::ClaudeCode,
            category: AgentAssetCategory::Hook,
            reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        },
    ];
    assert_eq!(
        managed_blocked_source.diagnostics,
        expected_managed_blocked_diagnostics
    );
    assert_eq!(
        managed_blocked
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .count(),
        expected_managed_blocked_diagnostics.len(),
        "{managed_blocked:?}"
    );
    assert_eq!(managed_blocked_hooks.len(), 1, "{managed_blocked:?}");
    assert_eq!(
        managed_blocked_hooks[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        managed_blocked_hooks[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert_eq!(
        managed_blocked_hooks[0].effective_state,
        AgentAssetState::Unknown
    );
    assert!(managed_blocked_hooks[0].resolution.control_source.is_some());
    assert!(matches!(
        managed_blocked_hooks[0].details,
        AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    assert_hook_asset_binding(
        &managed_blocked,
        "settings:PostToolUse:matcher-absent:group-0:hook-0",
        HookAssetExpectation {
            declared: AgentAssetDeclaredState::Enabled,
            effective: AgentAssetState::Unknown,
            participation: AgentAssetResolutionParticipation::Participates,
            contributors: &[claude_declarations(
                &managed_blocked,
                AgentAssetCategory::Hook,
                "settings:PostToolUse:matcher-absent:group-0:hook-0",
            )[0]
            .id
            .clone()],
            resolution: AgentAssetResolutionRelation::Independent,
            control_source: Some((&managed_blocked_source.id, "control:hook:source")),
            mutation_reason: AgentAssetActionUnavailableReason::Unknown,
        },
    );
    assert_no_invalid_resolution_diagnostics(&managed_blocked);
    remove_claude_fixture(managed_blocked_root);

    let (authority_root, _, authority, _) = build_claude_inventory(
        "claude-hooks-non-managed-authority-matrix",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": true,
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "authority-user" }] }] }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": false,
                    "hooks": { "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "authority-workspace" }] }] }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": "invalid",
                    "hooks": { "Notification": [{ "hooks": [{ "type": "command", "command": "authority-local" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let authority_hooks = authority
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(authority_hooks.len(), 3, "{authority:?}");
    assert!(authority_hooks.iter().all(|item| {
        item.effective_state == AgentAssetState::Enabled
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
            && item.resolution.control_source.is_none()
    }));
    for field_source in ["settings", "workspace-settings", "workspace-local-settings"] {
        let source = authority
            .sources
            .iter()
            .find(|source| {
                (field_source == "settings"
                    && source.scope == AgentAssetScope::User
                    && source.path.ends_with("settings.json"))
                    || (field_source == "workspace-settings"
                        && source.scope == AgentAssetScope::Workspace
                        && source.path.ends_with("settings.json"))
                    || (field_source == "workspace-local-settings"
                        && source.scope == AgentAssetScope::Local)
            })
            .expect("authority source");
        assert_eq!(
            source
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::UnknownField { field_path }
                        if field_path == "allowManagedHooksOnly"
                ))
                .count(),
            1,
            "{field_source}: {authority:?}"
        );
    }
    remove_claude_fixture(authority_root);

    let (managed_wrong_root, _, managed_wrong, _) = build_claude_inventory(
        "claude-hooks-managed-only-wrong-type",
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "wrong-type-non-managed" }] }] }
                })),
            ),
            (
                "managed-settings",
                claude_json(serde_json::json!({
                    "allowManagedHooksOnly": "invalid",
                    "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "wrong-type-managed" }] }] }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let wrong_hooks = managed_wrong
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(wrong_hooks.len(), 2, "{managed_wrong:?}");
    assert!(wrong_hooks.iter().any(|item| {
        item.effective_state == AgentAssetState::Blocked
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
            && item.resolution.terminal == Some(AgentAssetResolutionTerminal::PolicyBlocked)
    }));
    assert!(wrong_hooks.iter().any(|item| {
        item.effective_state == AgentAssetState::Enabled
            && item.resolution.relation == AgentAssetResolutionRelation::Independent
    }));
    let wrong_control_source = managed_wrong
        .sources
        .iter()
        .find(|source| {
            source.scope == AgentAssetScope::Managed
                && source.path.ends_with("managed-settings.json")
        })
        .expect("wrong-type policy source");
    assert_hook_policy_binding(
        &managed_wrong,
        "settings:PostToolUse:matcher-absent:group-0:hook-0",
        HookPolicyExpectation {
            resolution: AgentAssetResolutionRelation::Independent,
            terminal: Some(AgentAssetResolutionTerminal::PolicyBlocked),
            effective: AgentAssetState::Blocked,
            control_source_id: Some((
                &wrong_control_source.id,
                "control:hook:allowManagedHooksOnly",
            )),
            resolution_diagnostics: &[AgentAssetDiagnostic::PolicyBlocked],
            source_diagnostic: None,
            managed: false,
            hook_enabled: AgentAssetDeclaredState::Enabled,
        },
    );
    let wrong_managed_declaration = claude_declarations(
        &managed_wrong,
        AgentAssetCategory::Hook,
        "managed-settings:Stop:matcher-absent:group-0:hook-0",
    )[0];
    assert_hook_asset_binding(
        &managed_wrong,
        "managed-settings:Stop:matcher-absent:group-0:hook-0",
        HookAssetExpectation {
            declared: AgentAssetDeclaredState::Enabled,
            effective: AgentAssetState::Enabled,
            participation: AgentAssetResolutionParticipation::Participates,
            contributors: std::slice::from_ref(&wrong_managed_declaration.id),
            resolution: AgentAssetResolutionRelation::Independent,
            control_source: None,
            mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    assert_no_invalid_resolution_diagnostics(&managed_wrong);
    assert_eq!(
        managed_wrong
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                    if location == "allowManagedHooksOnly"
            ))
            .count(),
        1
    );
    remove_claude_fixture(managed_wrong_root);

    let (inert_root, _, inert, _) = build_claude_inventory(
        "claude-hooks-non-managed-field",
        [(
            "settings",
            claude_json(serde_json::json!({
                "allowManagedHooksOnly": true,
                "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "inert-field-hook" }] }] }
            })),
        )],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let inert_hooks = inert
        .assets
        .iter()
        .filter(|item| item.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(inert_hooks.len(), 1, "{inert:?}");
    assert_eq!(
        inert_hooks[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(inert_hooks[0].effective_state, AgentAssetState::Enabled);
    assert_eq!(
        inert
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::UnknownField { field_path }
                    if field_path == "allowManagedHooksOnly"
            ))
            .count(),
        1
    );
    assert_hook_asset_binding(
        &inert,
        "settings:PostToolUse:matcher-absent:group-0:hook-0",
        HookAssetExpectation {
            declared: AgentAssetDeclaredState::Enabled,
            effective: AgentAssetState::Enabled,
            participation: AgentAssetResolutionParticipation::Participates,
            contributors: &[claude_declarations(
                &inert,
                AgentAssetCategory::Hook,
                "settings:PostToolUse:matcher-absent:group-0:hook-0",
            )[0]
            .id
            .clone()],
            resolution: AgentAssetResolutionRelation::Independent,
            control_source: None,
            mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    assert_no_invalid_resolution_diagnostics(&inert);
    remove_claude_fixture(inert_root);

    let tee_context = claude_focused_context();
    let tee_sources = [
        claude_focused_source(
            "tee-user",
            AgentAssetScope::User,
            10,
            &[AgentAssetCategory::Hook],
        ),
        claude_focused_source(
            "tee-local",
            AgentAssetScope::Local,
            30,
            &[AgentAssetCategory::Hook],
        ),
        claude_focused_source(
            "tee-managed",
            AgentAssetScope::Managed,
            40,
            &[AgentAssetCategory::Hook],
        ),
    ];
    let tee_snapshots = [
        AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": {
                    "PostToolUse": [{
                        "matcher": "tee-user-matcher-secret",
                        "hooks": [{
                            "type": "prompt",
                            "prompt": "tee-user-prompt-secret"
                        }]
                    }, {
                        "matcher": "tee-user-http-matcher-secret",
                        "hooks": [{
                            "type": "http",
                            "url": "https://tee-user-url-secret",
                            "headers": { "X-Tee": "tee-user-header-secret" }
                        }]
                    }]
                }
            })),
            revision: AgentAssetRevision::default(),
        },
        AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": {
                    "Notification": [{
                        "matcher": "tee-local-matcher-secret",
                        "hooks": [{
                            "type": "command",
                            "command": "tee-local-script-path-secret",
                            "timeout": 17,
                            "async": true,
                            "once": false,
                            "statusMessage": "tee-local-aux-secret"
                        }]
                    }, {
                        "matcher": "tee-local-malformed-matcher-secret",
                        "hooks": [{
                            "type": "command",
                            "command": "tee-local-malformed-script-secret",
                            "async": "tee-malformed-aux-secret"
                        }]
                    }]
                }
            })),
            revision: AgentAssetRevision::default(),
        },
        AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": {
                    "Stop": [{
                        "matcher": "tee-managed-matcher-secret",
                        "hooks": [{
                            "type": "agent",
                            "prompt": "tee-managed-agent-secret"
                        }]
                    }, {
                        "matcher": "tee-managed-command-matcher-secret",
                        "hooks": [{
                            "type": "command",
                            "command": "tee-managed-script-path-secret",
                            "statusMessage": "tee-managed-aux-secret"
                        }]
                    }]
                }
            })),
            revision: AgentAssetRevision::default(),
        },
    ];
    let mut tee = NativePayloadDebugCollector::default();
    for (source, snapshot) in tee_sources.iter().zip(tee_snapshots.iter()) {
        claude_test_parse(
            AgentAssetParseRequest {
                context: &tee_context,
                source,
                snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut tee,
        );
    }
    assert_eq!(tee.declarations.len(), 6);
    let tee_public_facts = tee
        .declarations
        .iter()
        .flat_map(|declaration| declaration.facts.values())
        .collect::<Vec<_>>();
    let tee_debug = format!(
        "{:?}{:?}{:?}{:?}",
        tee.payload_debug, tee.facts, tee_public_facts, tee.diagnostics
    );
    for secret in [
        "tee-user-matcher-secret",
        "tee-user-prompt-secret",
        "tee-user-http-matcher-secret",
        "tee-user-url-secret",
        "tee-user-header-secret",
        "tee-local-matcher-secret",
        "tee-local-script-path-secret",
        "tee-local-aux-secret",
        "tee-local-malformed-matcher-secret",
        "tee-local-malformed-script-secret",
        "tee-malformed-aux-secret",
        "tee-managed-matcher-secret",
        "tee-managed-agent-secret",
        "tee-managed-command-matcher-secret",
        "tee-managed-script-path-secret",
        "tee-managed-aux-secret",
    ] {
        assert!(
            !tee_debug.contains(secret),
            "Hook parser debug leaked: {secret}"
        );
    }
    assert_eq!(
        tee.diagnostics,
        vec![
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Json,
                location: Some("hooks.Notification".to_owned()),
            },
            AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: AgentCliKind::ClaudeCode,
                category: AgentAssetCategory::Hook,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            },
        ]
    );

    let break_hook_sources = [
        claude_focused_source(
            "break-hook-user",
            AgentAssetScope::User,
            10,
            &[AgentAssetCategory::Hook],
        ),
        claude_focused_source(
            "break-hook-managed",
            AgentAssetScope::Managed,
            40,
            &[AgentAssetCategory::Hook],
        ),
    ];
    let break_hook_snapshots = [
        AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": { "PostToolUse": [{ "hooks": [{ "type": "command", "command": "break-hook-user-secret" }] }] }
            })),
            revision: AgentAssetRevision::default(),
        },
        AgentAssetSnapshot::File {
            bytes: claude_json(serde_json::json!({
                "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "break-hook-managed-secret" }] }] }
            })),
            revision: AgentAssetRevision::default(),
        },
    ];
    let mut break_hook_declarations = Vec::new();
    for (source, snapshot) in break_hook_sources.iter().zip(break_hook_snapshots.iter()) {
        let mut parsed = NativePayloadDebugCollector::default();
        claude_test_parse(
            AgentAssetParseRequest {
                context: &tee_context,
                source,
                snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        break_hook_declarations.extend(parsed.declarations);
    }
    assert_eq!(break_hook_declarations.len(), 2);
    let break_hook_resolve_sources = [
        AgentAssetResolveSource {
            spec: &break_hook_sources[0],
            snapshot: &break_hook_snapshots[0],
        },
        AgentAssetResolveSource {
            spec: &break_hook_sources[1],
            snapshot: &break_hook_snapshots[1],
        },
    ];
    let mut break_hook_output = BreakResolveOutput::default();
    claude_test_resolve(
        AgentAssetResolveRequest {
            context: &tee_context,
            declarations: &break_hook_declarations,
            sources: &break_hook_resolve_sources,
        },
        &mut break_hook_output,
    );
    assert_eq!(break_hook_output.emissions, 1);
    assert_eq!(break_hook_output.drafts.len(), 1);
    assert_eq!(
        break_hook_output.drafts[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    remove_claude_fixture(root);
}

#[test]
fn claude_status_line_replaces_and_disable_all_blocks() {
    let files = vec![
        (
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
            })),
        ),
        (
            "settings",
            claude_json(serde_json::json!({
                "statusLine": { "type": "command", "command": "status-user-secret" }
            })),
        ),
        (
            "workspace-settings",
            claude_json(serde_json::json!({
                "statusLine": { "type": "command", "command": "status-workspace-secret" }
            })),
        ),
        (
            "workspace-local-settings",
            claude_json(serde_json::json!({
                "statusLine": { "type": "command", "command": "status-local-secret" }
            })),
        ),
        (
            "managed-settings",
            claude_json(serde_json::json!({
                "statusLine": { "type": "command", "command": "status-managed-secret" }
            })),
        ),
    ];
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-status-line-replacement",
        files.clone(),
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let declarations = claude_declarations(&inventory, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(declarations.len(), 4, "{inventory:?}");
    assert!(declarations.iter().all(|declaration| {
        declaration.declared_state == AgentAssetDeclaredState::Enabled
            && declaration.role == AgentAssetDeclarationRole::Definition
            && matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            )
    }));
    for (scope, precedence) in [
        (AgentAssetScope::User, 10),
        (AgentAssetScope::Workspace, 20),
        (AgentAssetScope::Local, 30),
        (AgentAssetScope::Managed, 40),
    ] {
        assert!(declarations
            .iter()
            .any(|declaration| declaration.scope == scope && declaration.precedence == precedence));
    }
    let status = claude_assets(&inventory, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(status.len(), 4, "{inventory:?}");
    let winner = status
        .iter()
        .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .expect("managed StatusLine winner");
    assert_eq!(winner.scope, AgentAssetScope::Managed);
    assert_eq!(winner.precedence, 40);
    assert_eq!(winner.effective_state, AgentAssetState::Enabled);
    assert_eq!(winner.resolution.contributor_ids.len(), 4);
    assert_eq!(winner.represented_declaration_ids.len(), 4);
    assert!(winner.source_ids.contains(&winner.inspection_source_id));
    assert!(matches!(
        winner.details,
        AgentAssetDetails::StatusUi {
            mode: AgentStatusUiMode::Command,
            command_present: true,
        }
    ));
    assert!(matches!(
        winner.resolution.winner_id,
        Some(ref winner_id) if winner_id == &winner.stable_id
    ));
    let expected_winner_source = inventory
        .sources
        .iter()
        .find(|source| source.path.ends_with("managed-settings.json"))
        .expect("managed settings source");
    assert_eq!(winner.inspection_source_id, expected_winner_source.id);
    let loser_count = status
        .iter()
        .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .count();
    assert_eq!(loser_count, 3);
    assert!(status
        .iter()
        .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .all(|asset| {
            asset.effective_state == AgentAssetState::Shadowed
                && asset.resolution.contributor_ids.len() == 1
                && asset.resolution.winner_id == winner.resolution.winner_id
                && asset.source_ids.contains(&asset.inspection_source_id)
        }));
    assert!(status.iter().all(|asset| {
        asset.relationships.provided_by.is_none()
            && asset.relationships.action_owner.is_none()
            && asset.relationships.affected_asset_ids.is_empty()
            && asset.actions.iter().all(|action| {
                !matches!(
                    action.action,
                    AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
                ) || !action.available
            })
    }));
    let mut all_status_declaration_ids = declarations
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    all_status_declaration_ids.sort();
    let mut winner_represented = winner.represented_declaration_ids.clone();
    winner_represented.sort();
    let mut winner_contributors = winner.resolution.contributor_ids.clone();
    winner_contributors.sort();
    assert_eq!(winner_represented, all_status_declaration_ids);
    assert_eq!(winner_contributors, all_status_declaration_ids);
    assert_read_only_asset_actions(
        winner,
        AgentAssetActionUnavailableReason::NoOfficialMechanism,
    );
    for loser in status
        .iter()
        .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
    {
        let declaration = declarations
            .iter()
            .find(|declaration| declaration.source_id == loser.inspection_source_id)
            .expect("StatusLine loser declaration");
        let mut represented = loser.represented_declaration_ids.clone();
        represented.sort();
        assert_eq!(represented, vec![declaration.id.clone()]);
        assert_eq!(
            loser.resolution.contributor_ids,
            vec![declaration.id.clone()]
        );
        assert_eq!(loser.source_ids, vec![declaration.source_id.clone()]);
        assert_eq!(loser.inspection_source_id, declaration.source_id);
        assert_eq!(loser.revision, declaration.evidence.revision);
        assert_read_only_asset_actions(loser, AgentAssetActionUnavailableReason::Shadowed);
    }
    assert_no_invalid_resolution_diagnostics(&inventory);
    let serialized = serde_json::to_string(&inventory).unwrap();
    for secret in [
        "status-user-secret",
        "status-workspace-secret",
        "status-local-secret",
        "status-managed-secret",
    ] {
        assert!(
            !serialized.contains(secret),
            "StatusLine payload leaked: {secret}"
        );
    }

    let (_, _, reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        root.clone(),
        files.clone(),
        Vec::<(&'static str, Vec<AgentAssetDirectoryEntry>)>::new(),
        Vec::<(&'static str, AgentAssetDiagnostic)>::new(),
        ClaudeInventoryBuildOptions {
            installations: &RealInstallationDiscoveryPort,
            workspace_input: None,
            settings: None,
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        claude_full_public_signature(&inventory),
        claude_full_public_signature(&reversed)
    );

    let (suppressed_root, _, suppressed, _) = build_claude_inventory(
        "claude-status-line-replacement-untrusted",
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": false } }
                })),
            ),
            (
                "settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-trusted-user-secret" }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-suppressed-workspace-secret" }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-suppressed-local-secret" }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let suppressed_declarations =
        claude_declarations(&suppressed, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(suppressed_declarations.len(), 3, "{suppressed:?}");
    assert_eq!(
        suppressed_declarations
            .iter()
            .filter(|declaration| matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            ))
            .count(),
        1
    );
    assert_eq!(
        suppressed_declarations
            .iter()
            .filter(|declaration| matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ))
            .count(),
        2
    );
    let suppressed_status = claude_assets(&suppressed, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(suppressed_status.len(), 1, "{suppressed:?}");
    assert_eq!(
        suppressed_status[0].resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(
        suppressed_status[0].effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(suppressed_status[0].resolution.contributor_ids.len(), 1);
    let suppressed_user = suppressed_declarations
        .iter()
        .find(|declaration| declaration.scope == AgentAssetScope::User)
        .expect("StatusLine trusted declaration");
    let mut expected_suppressed_represented = suppressed_declarations
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    expected_suppressed_represented.sort();
    let mut actual_suppressed_represented =
        suppressed_status[0].represented_declaration_ids.clone();
    actual_suppressed_represented.sort();
    assert_eq!(
        actual_suppressed_represented,
        expected_suppressed_represented
    );
    assert_eq!(
        suppressed_status[0].resolution.contributor_ids,
        vec![suppressed_user.id.clone()]
    );
    assert_eq!(
        suppressed_status[0].source_ids,
        vec![suppressed_user.source_id.clone()]
    );
    assert_eq!(
        suppressed_status[0].inspection_source_id,
        suppressed_user.source_id
    );
    assert_eq!(
        suppressed_status[0].revision,
        suppressed_user.evidence.revision
    );
    assert!(suppressed_status[0].resolution.winner_id.is_none());
    assert!(suppressed_status[0].resolution.control_source.is_none());
    assert!(suppressed_status[0].relationships.provided_by.is_none());
    assert!(suppressed_status[0].relationships.action_owner.is_none());
    assert!(suppressed_status[0]
        .relationships
        .affected_asset_ids
        .is_empty());
    assert!(matches!(
        suppressed_status[0].details,
        AgentAssetDetails::StatusUi {
            mode: AgentStatusUiMode::Command,
            command_present: true,
        }
    ));
    assert_read_only_asset_actions(
        suppressed_status[0],
        AgentAssetActionUnavailableReason::NoOfficialMechanism,
    );
    assert_no_invalid_resolution_diagnostics(&suppressed);
    assert!(!serde_json::to_string(&suppressed)
        .unwrap()
        .contains("status-suppressed"));

    let (unknown_trust_root, _, unknown_trust, _) = build_claude_inventory(
        "claude-status-line-replacement-unknown-trust",
        [
            (
                "settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-unknown-trust-user-secret" }
                })),
            ),
            (
                "workspace-settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-unknown-trust-workspace-secret" }
                })),
            ),
            (
                "workspace-local-settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-unknown-trust-local-secret" }
                })),
            ),
        ],
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    assert_eq!(
        unknown_trust.contexts[0].trust_context,
        AgentTrustState::Unknown
    );
    let unknown_trust_declarations =
        claude_declarations(&unknown_trust, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(unknown_trust_declarations.len(), 3, "{unknown_trust:?}");
    for declaration in &unknown_trust_declarations {
        assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
        assert_eq!(declaration.declared_state, AgentAssetDeclaredState::Enabled);
        assert_eq!(declaration.native_id, "status-line");
        assert_eq!(
            declaration.evidence.revision,
            unknown_trust
                .sources
                .iter()
                .find(|source| source.id == declaration.source_id)
                .expect("Unknown trust StatusLine source")
                .revision
        );
        if declaration.scope == AgentAssetScope::User {
            assert!(matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            ));
        } else {
            assert!(matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ));
        }
    }
    let unknown_trust_user = unknown_trust_declarations
        .iter()
        .find(|declaration| declaration.scope == AgentAssetScope::User)
        .expect("Unknown trust StatusLine user declaration");
    let mut unknown_trust_represented = unknown_trust_declarations
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    unknown_trust_represented.sort();
    let unknown_trust_assets =
        claude_assets(&unknown_trust, AgentAssetCategory::StatusUi, "status-line");
    assert_eq!(unknown_trust_assets.len(), 1, "{unknown_trust:?}");
    assert_status_asset_binding(
        &unknown_trust,
        StatusAssetExpectation {
            mode: AgentStatusUiMode::Command,
            declared: AgentAssetDeclaredState::Enabled,
            effective: AgentAssetState::Enabled,
            resolution: AgentAssetResolutionRelation::Independent,
            terminal: None,
            contributors: std::slice::from_ref(&unknown_trust_user.id),
            represented: &unknown_trust_represented,
            control_source: None,
            resolution_diagnostics: &[],
            mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
        },
    );
    assert_eq!(
        unknown_trust_assets[0].resolution.contributor_ids,
        vec![unknown_trust_user.id.clone()]
    );
    assert_eq!(
        unknown_trust_assets[0].inspection_source_id,
        unknown_trust_user.source_id
    );
    assert_eq!(
        unknown_trust_assets[0].revision,
        unknown_trust_user.evidence.revision
    );
    assert_eq!(unknown_trust_assets[0].resolution.winner_id, None);
    assert!(unknown_trust_assets[0].resolution.control_source.is_none());
    assert!(unknown_trust_assets[0].relationships.provided_by.is_none());
    assert!(unknown_trust_assets[0].relationships.action_owner.is_none());
    assert!(unknown_trust_assets[0]
        .relationships
        .affected_asset_ids
        .is_empty());
    assert!(matches!(
        unknown_trust_assets[0].details,
        AgentAssetDetails::StatusUi {
            mode: AgentStatusUiMode::Command,
            command_present: true,
        }
    ));
    assert_read_only_asset_actions(
        unknown_trust_assets[0],
        AgentAssetActionUnavailableReason::NoOfficialMechanism,
    );
    assert_no_invalid_resolution_diagnostics(&unknown_trust);
    let unknown_trust_serialized = serde_json::to_string(&unknown_trust).unwrap();
    for secret in [
        "status-unknown-trust-user-secret",
        "status-unknown-trust-workspace-secret",
        "status-unknown-trust-local-secret",
    ] {
        assert!(
            !unknown_trust_serialized.contains(secret),
            "Unknown trust StatusLine payload leaked: {secret}"
        );
    }
    remove_claude_fixture(unknown_trust_root);

    let run_shape_case = |name: &str,
                          value: Option<serde_json::Value>,
                          expected_mode: AgentStatusUiMode,
                          expected_state: AgentAssetDeclaredState,
                          expected_assets: usize| {
        let payload = value.map_or_else(
            || serde_json::json!({}),
            |status_line| serde_json::json!({ "statusLine": status_line }),
        );
        let (root, _, inventory, _) = build_claude_inventory(
            name,
            [("settings", claude_json(payload))],
            [],
            [],
            &RealInstallationDiscoveryPort,
        );
        let assets = claude_assets(&inventory, AgentAssetCategory::StatusUi, "status-line");
        assert_eq!(assets.len(), expected_assets, "{name}: {inventory:?}");
        if let Some(asset) = assets.first() {
            let expected_effective = match expected_state {
                AgentAssetDeclaredState::Enabled => AgentAssetState::Enabled,
                AgentAssetDeclaredState::Disabled => AgentAssetState::Disabled,
                AgentAssetDeclaredState::Unknown => AgentAssetState::Unknown,
                AgentAssetDeclaredState::Rejected => AgentAssetState::Blocked,
                AgentAssetDeclaredState::Pending => AgentAssetState::Unknown,
            };
            assert_eq!(asset.declared_state, expected_effective, "{name}");
            assert_eq!(asset.effective_state, expected_effective, "{name}");
            let declaration =
                claude_declarations(&inventory, AgentAssetCategory::StatusUi, "status-line")[0];
            let declaration_ids = vec![declaration.id.clone()];
            assert_status_asset_binding(
                &inventory,
                StatusAssetExpectation {
                    mode: expected_mode,
                    declared: expected_state,
                    effective: expected_effective,
                    resolution: AgentAssetResolutionRelation::Independent,
                    terminal: None,
                    contributors: &declaration_ids,
                    represented: &declaration_ids,
                    control_source: None,
                    resolution_diagnostics: &[],
                    mutation_reason: AgentAssetActionUnavailableReason::NoOfficialMechanism,
                },
            );
        }
        if expected_assets > 0 && expected_mode == AgentStatusUiMode::Unknown {
            assert_eq!(
                inventory
                    .sources
                    .iter()
                    .flat_map(|source| source.diagnostics.iter())
                    .filter(|diagnostic| matches!(
                        diagnostic,
                        AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                            if location == "statusLine"
                    ))
                    .count(),
                1,
                "{name}: {inventory:?}"
            );
        }
        assert_no_invalid_resolution_diagnostics(&inventory);
        remove_claude_fixture(root);
    };
    run_shape_case(
        "claude-status-line-null",
        Some(serde_json::Value::Null),
        AgentStatusUiMode::Unknown,
        AgentAssetDeclaredState::Unknown,
        1,
    );
    run_shape_case(
        "claude-status-line-false",
        Some(serde_json::Value::Bool(false)),
        AgentStatusUiMode::Unknown,
        AgentAssetDeclaredState::Unknown,
        1,
    );
    run_shape_case(
        "claude-status-line-command",
        Some(serde_json::json!({ "type": "command", "command": "status-command-secret" })),
        AgentStatusUiMode::Command,
        AgentAssetDeclaredState::Enabled,
        1,
    );
    for (name, value) in [
        ("arbitrary-object", serde_json::json!({ "type": "command" })),
        (
            "missing-type",
            serde_json::json!({ "command": "status-command-secret" }),
        ),
        ("empty-command", serde_json::json!({ "command": "" })),
        (
            "wrong-command",
            serde_json::json!({ "type": "command", "command": 7 }),
        ),
        (
            "unknown-type",
            serde_json::json!({ "type": "shell", "command": "x" }),
        ),
        ("scalar", serde_json::json!("unsupported")),
    ] {
        run_shape_case(
            &format!("claude-status-line-{name}"),
            Some(value),
            AgentStatusUiMode::Unknown,
            AgentAssetDeclaredState::Unknown,
            1,
        );
    }
    run_shape_case(
        "claude-status-line-missing",
        None,
        AgentStatusUiMode::Unknown,
        AgentAssetDeclaredState::Unknown,
        0,
    );

    let run_policy_case =
        |name: &str,
         policy: Option<Vec<u8>>,
         blocked: Vec<(&'static str, AgentAssetDiagnostic)>,
         expected_kind: AgentAssetResolutionRelation,
         expected_terminal: Option<AgentAssetResolutionTerminal>,
         expected_state: AgentAssetState,
         expected_assets: usize,
         expected_control_source_key: Option<(&str, &str)>,
         expected_diagnostic_source_key: Option<&str>,
         expected_source_diagnostic: Option<AgentAssetDiagnostic>| {
            let mut case_files = vec![(
                "managed-settings",
                claude_json(serde_json::json!({
                    "statusLine": { "type": "command", "command": "status-policy-managed-secret" }
                })),
            )];
            if let Some(policy) = policy {
                case_files.push(("settings", policy));
            }
            let (root, _, inventory, _) = build_claude_inventory(
                name,
                case_files,
                [],
                blocked,
                &RealInstallationDiscoveryPort,
            );
            let assets = claude_assets(&inventory, AgentAssetCategory::StatusUi, "status-line");
            assert_eq!(assets.len(), expected_assets, "{name}: {inventory:?}");
            let control_source_id = expected_control_source_key.map(|(source_key, key)| {
                let source = inventory
                    .sources
                    .iter()
                    .find(|source| match source_key {
                        "settings" => {
                            source.scope == AgentAssetScope::User
                                && source.path.ends_with("settings.json")
                        }
                        _ => false,
                    })
                    .expect("status policy source");
                (source.id.as_str(), key)
            });
            let expected_source_diagnostic = expected_source_diagnostic.map(|diagnostic| {
                let source_key = expected_diagnostic_source_key.expect("status diagnostic source");
                let source = inventory
                    .sources
                    .iter()
                    .find(|source| {
                        source_key == "settings"
                            && source.scope == AgentAssetScope::User
                            && source.path.ends_with("settings.json")
                    })
                    .expect("status diagnostic source");
                rebind_source_id(diagnostic, &source.id)
            });
            let declaration =
                claude_declarations(&inventory, AgentAssetCategory::StatusUi, "status-line")
                    .into_iter()
                    .next()
                    .expect("StatusLine policy declaration");
            let expected_resolution_diagnostics =
                if expected_terminal == Some(AgentAssetResolutionTerminal::PolicyBlocked) {
                    vec![AgentAssetDiagnostic::PolicyBlocked]
                } else {
                    Vec::new()
                };
            if expected_kind == AgentAssetResolutionRelation::ReplaceWinner {
                let winner = assets
                    .iter()
                    .find(|asset| {
                        asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                    })
                    .expect("StatusLine replacement winner");
                assert_eq!(winner.effective_state, expected_state);
                assert_eq!(winner.resolution.terminal, expected_terminal);
                assert_eq!(winner.resolution.contributor_ids.len(), 2);
                assert_eq!(winner.represented_declaration_ids.len(), 2);
                assert!(matches!(
                    winner.details,
                    AgentAssetDetails::StatusUi {
                        mode: AgentStatusUiMode::Command,
                        command_present: true,
                    }
                ));
                match control_source_id {
                    Some((source_id, declaration_key)) => assert_claude_policy_declaration(
                        &inventory,
                        &winner.resolution.control_source,
                        AgentAssetCategory::StatusUi,
                        source_id,
                        declaration_key,
                    ),
                    None => assert!(winner.resolution.control_source.is_none()),
                }
                let losers = assets
                    .iter()
                    .filter(|asset| {
                        asset.resolution.relation == AgentAssetResolutionRelation::Replaced
                    })
                    .collect::<Vec<_>>();
                assert_eq!(losers.len(), 1, "{name}: {inventory:?}");
                assert!(losers.iter().all(|loser| {
                    loser.effective_state == AgentAssetState::Shadowed
                        && loser.resolution.terminal.is_none()
                        && loser.resolution.control_source.is_none()
                        && loser.resolution.winner_id == winner.resolution.winner_id
                }));
            } else {
                let declaration_ids = vec![declaration.id.clone()];
                assert_status_asset_binding(
                    &inventory,
                    StatusAssetExpectation {
                        mode: AgentStatusUiMode::Command,
                        declared: AgentAssetDeclaredState::Enabled,
                        effective: expected_state,
                        resolution: expected_kind,
                        terminal: expected_terminal,
                        contributors: &declaration_ids,
                        represented: &declaration_ids,
                        control_source: control_source_id,
                        resolution_diagnostics: &expected_resolution_diagnostics,
                        mutation_reason: match expected_terminal {
                            Some(AgentAssetResolutionTerminal::PolicyBlocked) => {
                                AgentAssetActionUnavailableReason::PolicyBlocked
                            }
                            Some(AgentAssetResolutionTerminal::Unknown) => {
                                AgentAssetActionUnavailableReason::Unknown
                            }
                            None => AgentAssetActionUnavailableReason::NoOfficialMechanism,
                        },
                    },
                );
            }
            let source_diagnostics = inventory
                .sources
                .iter()
                .flat_map(|source| source.diagnostics.iter())
                .collect::<Vec<_>>();
            if let Some(expected) = expected_source_diagnostic.as_ref() {
                let source_unavailable =
                    matches!(expected,
                        AgentAssetDiagnostic::Malformed { location: Some(location), .. }
                            if location == "root"
                    ) || matches!(expected, AgentAssetDiagnostic::ReadFailed { .. });
                let mut expected_diagnostics = vec![expected.clone()];
                if source_unavailable {
                    expected_diagnostics.push(AgentAssetDiagnostic::DiscoveryIncomplete {
                        agent_kind: AgentCliKind::ClaudeCode,
                        category: AgentAssetCategory::Hook,
                        reason:
                            crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                    });
                }
                assert_eq!(
                    source_diagnostics.len(),
                    expected_diagnostics.len(),
                    "{name}: {inventory:?}"
                );
                let source_key = expected_diagnostic_source_key.expect("status diagnostic source");
                let source = inventory
                    .sources
                    .iter()
                    .find(|source| {
                        source_key == "settings"
                            && source.scope == AgentAssetScope::User
                            && source.path.ends_with("settings.json")
                    })
                    .expect("status diagnostic source");
                assert_eq!(
                    source.diagnostics.len(),
                    expected_diagnostics.len(),
                    "{name}: {inventory:?}"
                );
                assert!(
                    expected_diagnostics
                        .iter()
                        .all(|diagnostic| source.diagnostics.contains(diagnostic)),
                    "{name}: {inventory:?}"
                );
            } else {
                assert!(source_diagnostics.is_empty(), "{name}: {inventory:?}");
            }
            assert_no_invalid_resolution_diagnostics(&inventory);
            remove_claude_fixture(root);
        };
    run_policy_case(
        "claude-status-line-disable-true",
        Some(claude_json(serde_json::json!({ "disableAllHooks": true }))),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::PolicyBlocked),
        AgentAssetState::Blocked,
        1,
        Some(("settings", "control:statusUi:disableAllHooks")),
        None,
        None,
    );
    run_policy_case(
        "claude-status-line-disable-false",
        Some(claude_json(serde_json::json!({ "disableAllHooks": false }))),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        None,
        AgentAssetState::Enabled,
        1,
        None,
        None,
        None,
    );
    run_policy_case(
        "claude-status-line-disable-wrong-type",
        Some(claude_json(
            serde_json::json!({ "disableAllHooks": "invalid" }),
        )),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        1,
        Some(("settings", "control:statusUi:disableAllHooks")),
        Some("settings"),
        Some(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("disableAllHooks".to_owned()),
        }),
    );
    run_policy_case(
        "claude-status-line-disable-malformed",
        Some(b"{".to_vec()),
        Vec::new(),
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        1,
        Some(("settings", "control:statusUi:source")),
        Some("settings"),
        Some(AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("root".to_owned()),
        }),
    );
    run_policy_case(
        "claude-status-line-disable-blocked",
        None,
        vec![(
            "settings",
            AgentAssetDiagnostic::ReadFailed {
                source_id: "settings".to_owned(),
                error_kind: crate::models::AgentAssetIoErrorKind::Other,
            },
        )],
        AgentAssetResolutionRelation::Independent,
        Some(AgentAssetResolutionTerminal::Unknown),
        AgentAssetState::Unknown,
        1,
        Some(("settings", "control:statusUi:source")),
        Some("settings"),
        Some(AgentAssetDiagnostic::ReadFailed {
            source_id: "settings".to_owned(),
            error_kind: crate::models::AgentAssetIoErrorKind::Other,
        }),
    );

    let context = claude_focused_context();
    let user_source = claude_focused_source(
        "status-user",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::StatusUi],
    );
    let workspace_source = claude_focused_source(
        "status-workspace",
        AgentAssetScope::Workspace,
        20,
        &[AgentAssetCategory::StatusUi],
    );
    let user_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "break-status-user-secret" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let workspace_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "statusLine": { "type": "command", "command": "break-status-workspace-secret" }
        })),
        revision: AgentAssetRevision::default(),
    };
    let mut declarations = Vec::new();
    for (source, snapshot) in [
        (&user_source, &user_snapshot),
        (&workspace_source, &workspace_snapshot),
    ] {
        let mut parsed = NativePayloadDebugCollector::default();
        claude_test_parse(
            AgentAssetParseRequest {
                context: &context,
                source,
                snapshot,
                native_home: None,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        declarations.extend(parsed.declarations);
    }
    let sources = [
        AgentAssetResolveSource {
            spec: &user_source,
            snapshot: &user_snapshot,
        },
        AgentAssetResolveSource {
            spec: &workspace_source,
            snapshot: &workspace_snapshot,
        },
    ];
    let mut break_output = BreakResolveOutput::default();
    claude_test_resolve(
        AgentAssetResolveRequest {
            context: &context,
            declarations: &declarations,
            sources: &sources,
        },
        &mut break_output,
    );
    assert_eq!(break_output.emissions, 1);
    assert_eq!(break_output.drafts.len(), 1);
    assert_eq!(
        break_output.drafts[0].resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );

    remove_claude_fixture(suppressed_root);
    remove_claude_fixture(root);
}

#[test]
fn claude_passive_inventory_never_executes_or_leaks() {
    let root = test_root("claude-passive-inventory");
    let workspace = root.join("workspace");
    let mut settings = crate::models::AppSettings::default();
    settings.set_agent_cli_path(AgentCliKind::ClaudeCode, "/isolated/bin/claude".to_owned());
    let (installations, installation_state) =
        FakeInstallationPort::new(vec![claude_fixture_installation(
            "2.1.263",
            AgentInstallationChannel::Stable,
        )]);
    let previous_tee =
        CLAUDE_NATIVE_PAYLOAD_TEE.with(|slot| slot.replace(Some(Default::default())));
    let user_skill =
        "---\nname: Safe Skill\nunknown: skill-unknown-scalar-secret\n---\nskill-body-secret\n";
    let workspace_skill = "---\nname: Workspace Skill\nunknown: workspace-skill-unknown-secret\n---\nworkspace-skill-body-secret\n";
    let account = claude_json(serde_json::json!({
        "projects": {
            "__WORKSPACE__": {
                "hasTrustDialogAccepted": true
            }
        }
    }));
    let settings_file = claude_json(serde_json::json!({
        "enabledPlugins": { "safe-plugin": true },
        "hooks": {
            "PreToolUse": [{
                "matcher": "hook-matcher-secret",
                "hooks": [
                    { "type": "command", "command": "hook-command-secret", "statusMessage": "hook-status-secret" },
                    { "type": "prompt", "prompt": "hook-prompt-secret" },
                    { "type": "http", "url": "https://hook-url-secret.invalid", "headers": { "X-Hook": "hook-header-secret" } },
                    { "type": "agent", "prompt": "hook-agent-secret", "async": true }
                ]
            }],
            "PermissionRequest": [{
                "matcher": 42,
                "hooks": [{ "type": "command", "command": "hook-malformed-command-secret" }]
            }],
            "Notification": [{
                "matcher": "hook-ordinary-matcher-secret",
                "hooks": [{ "type": "command", "command": "hook-ordinary-command-secret", "once": true }]
            }]
        },
        "statusLine": { "type": "command", "command": "status-command-secret" }
    }));
    let workspace_settings = claude_json(serde_json::json!({
        "enabledPlugins": { "workspace-plugin": true },
        "hooks": { "Notification": [{ "hooks": [{ "type": "prompt", "prompt": "workspace-hook-prompt-secret" }] }] }
    }));
    let workspace_local_settings = claude_json(serde_json::json!({
        "hooks": { "Notification": [{ "hooks": [
            { "type": "command", "command": "workspace-local-command-secret" },
            { "type": "http", "url": "https://local-hook-url-secret.invalid" }
        ] }] }
    }));
    let workspace_mcp = claude_json(serde_json::json!({}));
    let managed_settings = claude_json(serde_json::json!({
        "enabledPlugins": { "managed-plugin": true },
        "hooks": { "Notification": [{ "hooks": [{ "type": "command", "command": "managed-hook-command-secret" }] }] },
        "statusLine": { "type": "command", "command": "managed-status-command-secret" }
    }));
    let managed_mcp = claude_json(serde_json::json!({
        "mcpServers": {
            "safe-stdio": {
                "command": "mcp-stdio-command-secret",
                "args": ["mcp-stdio-arg-secret"],
                "env": { "MCP_TOKEN": "mcp-env-secret" }
            },
            "safe-remote": {
                "type": "http",
                "url": "https://mcp-remote-url-secret.invalid",
                "headers": { "Authorization": "mcp-header-secret" },
                "oauth": { "clientId": "mcp-oauth-secret" }
            },
            "managed-only": { "command": "managed-mcp-command-secret" }
        }
    }));
    let registry = claude_json(serde_json::json!({
        "version": 2,
        "plugins": {
            "safe-plugin": [{
                "scope": "user",
                "installPath": "/isolated/safe-plugin",
                "version": "1.2.3",
                "installedAt": "plugin-timestamp-secret",
                "gitCommit": "plugin-git-secret",
                "source": "plugin-source-secret",
                "ignored": "plugin-ignored-secret",
                "ignoredPath": "/isolated/plugin-private-path-secret"
            }],
            "workspace-plugin": [{
                "scope": "project",
                "projectPath": "__WORKSPACE__",
                "installPath": "/isolated/workspace-plugin",
                "version": "2.0.0"
            }],
            "managed-plugin": [{
                "scope": "managed",
                "installPath": "/isolated/managed-plugin",
                "version": "3.0.0"
            }]
        }
    }));
    let marketplaces = claude_json(serde_json::json!({
        "marketplace": "marketplace-name-secret",
        "installPath": "/isolated/marketplace-path-secret",
        "token": "marketplace-token-secret",
        "source": "marketplace-source-secret"
    }));
    let missing_plugin_roots = [
        PathBuf::from("/isolated/safe-plugin"),
        PathBuf::from("/isolated/workspace-plugin"),
        PathBuf::from("/isolated/managed-plugin"),
    ];
    let (root, _workspace, inventory, snapshot_state) =
        build_claude_inventory_at_root_with_workspace_and_adapter(
            root,
            [
                ("account", account),
                ("settings", settings_file),
                ("workspace-settings", workspace_settings),
                ("workspace-local-settings", workspace_local_settings),
                ("workspace-mcp", workspace_mcp),
                ("managed-settings", managed_settings),
                ("managed-mcp", managed_mcp),
                ("plugin-registry", registry),
                ("known-marketplaces", marketplaces),
                (
                    "skill-manifest:skills:safe-skill",
                    user_skill.as_bytes().to_vec(),
                ),
                (
                    "skill-manifest:workspace-skills:workspace-skill",
                    workspace_skill.as_bytes().to_vec(),
                ),
            ],
            [
                (
                    "skills",
                    claude_entries([("safe-skill", AgentAssetSourceKind::Directory, false)]),
                ),
                (
                    "workspace-skills",
                    claude_entries([("workspace-skill", AgentAssetSourceKind::Directory, false)]),
                ),
                (
                    "plugin-cache",
                    claude_entries([
                        (
                            "cache-plugin-name-secret",
                            AgentAssetSourceKind::Directory,
                            false,
                        ),
                        (
                            "marketplace-plugin-name-secret",
                            AgentAssetSourceKind::Directory,
                            false,
                        ),
                    ]),
                ),
            ],
            [],
            ClaudeInventoryBuildOptions {
                installations: &installations,
                workspace_input: Some(workspace.clone()),
                settings: Some(&settings),
                test_adapter: claude_test_tee_adapter(),
                strict_snapshots: true,
                strict_missing_directories: &missing_plugin_roots,
            },
        );

    struct ExpectedPassiveSnapshot<'a> {
        key: &'a str,
        path: PathBuf,
        kind: AgentAssetSourceKind,
    }
    let managed_root = if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/ClaudeCode")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(r"C:\Program Files\ClaudeCode")
    } else {
        PathBuf::from("/etc/claude-code")
    };
    let mut expected_snapshots = vec![
        ExpectedPassiveSnapshot {
            key: "settings",
            path: root.join(".claude/settings.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "account",
            path: root.join(".claude.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "skills",
            path: root.join(".claude/skills"),
            kind: AgentAssetSourceKind::Directory,
        },
        ExpectedPassiveSnapshot {
            key: "plugin-registry",
            path: root.join(".claude/plugins/installed_plugins.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "plugin-cache",
            path: root.join(".claude/plugins/cache"),
            kind: AgentAssetSourceKind::Directory,
        },
        ExpectedPassiveSnapshot {
            key: "known-marketplaces",
            path: root.join(".claude/plugins/known_marketplaces.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "managed-settings",
            path: managed_root.join("managed-settings.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "managed-mcp",
            path: managed_root.join("managed-mcp.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "workspace-settings",
            path: workspace.join(".claude/settings.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "workspace-local-settings",
            path: workspace.join(".claude/settings.local.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "workspace-mcp",
            path: workspace.join(".mcp.json"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "workspace-skills",
            path: workspace.join(".claude/skills"),
            kind: AgentAssetSourceKind::Directory,
        },
        ExpectedPassiveSnapshot {
            key: "skill-manifest:skills:safe-skill",
            path: root.join(".claude/skills/safe-skill/SKILL.md"),
            kind: AgentAssetSourceKind::File,
        },
        ExpectedPassiveSnapshot {
            key: "skill-manifest:workspace-skills:workspace-skill",
            path: workspace.join(".claude/skills/workspace-skill/SKILL.md"),
            kind: AgentAssetSourceKind::File,
        },
    ];
    // Package source keys include opaque occurrence identities. Assert the
    // exact requested paths and one read per path, without rebuilding those
    // identities from the implementation under test.
    let package_snapshots = snapshot_state
        .lock()
        .expect("passive snapshot state lock")
        .snapshot_paths
        .iter()
        .filter(|(_, path)| missing_plugin_roots.contains(path))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(package_snapshots.len(), 3);
    assert_eq!(
        package_snapshots
            .iter()
            .map(|(_, path)| path)
            .collect::<BTreeSet<_>>(),
        missing_plugin_roots.iter().collect::<BTreeSet<_>>()
    );
    for (key, path) in &package_snapshots {
        expected_snapshots.push(ExpectedPassiveSnapshot {
            key,
            path: path.clone(),
            kind: AgentAssetSourceKind::Directory,
        });
    }
    assert_eq!(expected_snapshots.len(), 17);
    let instruction_expectations = claude_instruction_source_expectations(&root, &workspace);
    // Instruction files are scanned by configuration discovery on demand;
    // passive asset discovery must not read them or credential caches.
    for (path, _) in instruction_expectations.values() {
        assert!(inventory
            .sources
            .iter()
            .all(|source| source.path != path.to_string_lossy()));
    }
    assert_eq!(
        expected_snapshots
            .iter()
            .map(|expected| &expected.path)
            .collect::<BTreeSet<_>>()
            .len(),
        expected_snapshots.len(),
        "each passive source has one physical path"
    );
    expected_snapshots.sort_by_key(|expected| {
        (
            expected.key.starts_with("skill-manifest:"),
            expected.key != "account",
            expected.path.clone(),
            expected.kind == AgentAssetSourceKind::Directory,
            expected.key,
        )
    });
    let mut authority_context = inventory.contexts[0].clone();
    authority_context.trust_context = AgentTrustState::Unknown;
    authority_context.id = context_stable_id(
        &native_environment(),
        AgentCliKind::ClaudeCode,
        &authority_context,
    );
    let expected_source_id = |expected: &ExpectedPassiveSnapshot| {
        let context = if expected.key == "account" {
            &authority_context
        } else {
            &inventory.contexts[0]
        };
        source_stable_id(context, &expected.path)
    };
    for expected in &expected_snapshots {
        let source = inventory
            .sources
            .iter()
            .find(|source| source.path == expected.path.to_string_lossy())
            .expect("expected passive source");
        assert_eq!(
            source.source_kind, expected.kind,
            "source kind {}",
            expected.key
        );
        if instruction_expectations.contains_key(expected.key) {
            assert!(
                source.revision.is_missing,
                "{} must be Missing",
                expected.key
            );
            assert_eq!(source.revision.size_bytes, None);
            assert!(source.categories.is_empty());
            assert!(source.diagnostics.is_empty(), "{:?}", source.diagnostics);
        }
    }
    assert!(inventory
        .sources
        .iter()
        .all(|source| Path::new(&source.path) != root.join(".claude/.credentials.json")));
    let snapshot_state_guard = snapshot_state.lock().expect("passive snapshot state lock");
    assert_eq!(
        snapshot_state_guard.native_source_keys.len(),
        expected_snapshots.len()
    );
    assert_eq!(
        snapshot_state_guard
            .native_source_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected_snapshots
            .iter()
            .map(|expected| expected.key.to_owned())
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        snapshot_state_guard.attempts.len(),
        snapshot_state_guard.native_source_keys.len()
    );
    assert_eq!(
        snapshot_state_guard
            .attempts
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected_snapshots
            .iter()
            .map(expected_source_id)
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        snapshot_state_guard
            .snapshot_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        expected_snapshots
            .iter()
            .map(|expected| (expected.key.to_owned(), expected.path.clone()))
            .collect::<BTreeSet<_>>()
    );
    let expected_file_ids = expected_snapshots
        .iter()
        .filter(|expected| {
            expected.kind == AgentAssetSourceKind::File
                && !instruction_expectations.contains_key(expected.key)
        })
        .map(expected_source_id)
        .collect::<Vec<_>>();
    assert_eq!(expected_file_ids.len(), 11);
    assert_eq!(snapshot_state_guard.opens.len(), expected_file_ids.len());
    assert_eq!(snapshot_state_guard.charges.len(), expected_file_ids.len());
    assert_eq!(
        snapshot_state_guard.snapshots.len(),
        expected_file_ids.len()
    );
    assert_eq!(
        snapshot_state_guard.opens.iter().collect::<BTreeSet<_>>(),
        expected_file_ids.iter().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        snapshot_state_guard.charges,
        snapshot_state_guard
            .snapshots
            .iter()
            .map(|(source_id, _, bytes)| (source_id.clone(), bytes.len()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot_state_guard.strict_consumed_source_keys.len(),
        expected_snapshots.len()
    );
    assert_eq!(
        snapshot_state_guard.strict_consumed_source_keys,
        expected_snapshots
            .iter()
            .map(|expected| expected.key.to_owned())
            .collect::<BTreeSet<_>>()
    );
    assert!(
        snapshot_state_guard.strict_remaining_files.is_empty(),
        "unconsumed strict file fixtures: {:?}",
        snapshot_state_guard.strict_remaining_files
    );
    assert_eq!(
        snapshot_state_guard.strict_remaining_missing_files,
        instruction_expectations
            .keys()
            .map(|key| (*key).to_owned())
            .collect::<BTreeSet<_>>(),
        "configuration-only fixtures must remain unread"
    );
    assert!(
        snapshot_state_guard.strict_remaining_directories.is_empty(),
        "unconsumed strict directory fixtures: {:?}",
        snapshot_state_guard.strict_remaining_directories
    );
    assert!(
        snapshot_state_guard.strict_remaining_blocked.is_empty(),
        "unconsumed strict blocked fixtures: {:?}",
        snapshot_state_guard.strict_remaining_blocked
    );
    assert!(
        snapshot_state_guard
            .strict_unexpected_source_keys
            .is_empty(),
        "unexpected strict source requests: {:?}",
        snapshot_state_guard.strict_unexpected_source_keys
    );
    assert!(
        snapshot_state_guard.strict_repeated_source_keys.is_empty(),
        "repeated strict source requests: {:?}",
        snapshot_state_guard.strict_repeated_source_keys
    );
    assert!(
        snapshot_state_guard.strict_violations.is_empty(),
        "strict fixture consumption violations: {:?}",
        snapshot_state_guard.strict_violations
    );
    drop(snapshot_state_guard);
    let installation_state_guard = installation_state
        .lock()
        .expect("passive installation state lock");
    assert_eq!(installation_state_guard.calls, vec!["claudeCode"]);
    assert_eq!(installation_state_guard.version_probes, 1);
    drop(installation_state_guard);

    let public_json = serde_json::to_string(&inventory).unwrap();
    let declarations_debug = format!("{:?}", inventory.declarations);
    let diagnostics_debug = format!(
        "{:?}{:?}{:?}{:?}{:?}",
        inventory.diagnostics,
        inventory
            .installations
            .iter()
            .flat_map(|installation| installation.diagnostics.iter())
            .collect::<Vec<_>>(),
        inventory
            .sources
            .iter()
            .flat_map(|source| source.diagnostics.iter())
            .collect::<Vec<_>>(),
        inventory
            .assets
            .iter()
            .flat_map(|asset| asset
                .diagnostics
                .iter()
                .chain(asset.resolution.diagnostics.iter()))
            .collect::<Vec<_>>(),
        inventory
            .declarations
            .iter()
            .flat_map(|declaration| declaration.diagnostics.iter())
            .collect::<Vec<_>>()
    );
    let all_public_debug = format!("{:?}", inventory);
    let parser_debug = CLAUDE_NATIVE_PAYLOAD_TEE.with(|slot| {
        let captured = slot.replace(previous_tee);
        captured.unwrap_or_default()
    });
    assert!(!parser_debug.declarations.is_empty());
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.source_key == "skill-manifest:skills:safe-skill"));
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.source_key
            == "skill-manifest:workspace-skills:workspace-skill"));
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.category == AgentAssetCategory::Mcp));
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.category == AgentAssetCategory::Plugin));
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.category == AgentAssetCategory::Hook));
    assert!(parser_debug
        .declarations
        .iter()
        .any(|declaration| declaration.category == AgentAssetCategory::StatusUi));
    assert!(parser_debug
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
    let private_debug = format!(
        "{:?}{:?}{:?}{:?}",
        parser_debug.payload_debug,
        parser_debug.facts,
        parser_debug.diagnostics,
        parser_debug.declarations
    );
    assert!(private_debug.contains("argument_count"));
    let unsafe_sentinels = [
        "mcp-stdio-command-secret",
        "mcp-stdio-arg-secret",
        "mcp-env-secret",
        "mcp-remote-url-secret",
        "mcp-header-secret",
        "mcp-oauth-secret",
        "workspace-local-command-secret",
        "managed-mcp-command-secret",
        "hook-matcher-secret",
        "hook-command-secret",
        "hook-status-secret",
        "hook-prompt-secret",
        "hook-url-secret",
        "hook-header-secret",
        "hook-agent-secret",
        "hook-malformed-command-secret",
        "hook-ordinary-matcher-secret",
        "hook-ordinary-command-secret",
        "workspace-hook-prompt-secret",
        "local-hook-url-secret",
        "managed-hook-command-secret",
        "status-command-secret",
        "managed-status-command-secret",
        "plugin-private-path-secret",
        "plugin-timestamp-secret",
        "plugin-git-secret",
        "plugin-source-secret",
        "plugin-ignored-secret",
        "marketplace-name-secret",
        "marketplace-path-secret",
        "marketplace-token-secret",
        "marketplace-source-secret",
        "cache-plugin-name-secret",
        "marketplace-plugin-name-secret",
        "skill-unknown-scalar-secret",
        "skill-body-secret",
        "workspace-skill-unknown-secret",
        "workspace-skill-body-secret",
    ];
    for sentinel in unsafe_sentinels {
        assert!(
            !public_json.contains(sentinel),
            "public JSON leaked {sentinel}"
        );
        assert!(
            !declarations_debug.contains(sentinel),
            "declaration Debug leaked {sentinel}"
        );
        assert!(
            !diagnostics_debug.contains(sentinel),
            "diagnostics Debug leaked {sentinel}"
        );
        assert!(
            !all_public_debug.contains(sentinel),
            "inventory Debug leaked {sentinel}"
        );
        assert!(
            !private_debug.contains(sentinel),
            "private parser output leaked {sentinel}"
        );
    }
    for safe in ["safe-stdio", "safe-remote", "safe-plugin", "safe-skill"] {
        assert!(
            public_json.contains(safe),
            "safe structural evidence missing {safe}"
        );
    }
    assert!(public_json.contains("\"transport\":\"stdio\""));
    assert!(public_json.contains("\"transport\":\"http\""));
    assert!(public_json.contains("\"commandPresent\":true"));
    assert!(public_json.contains("\"version\":\"1.2.3\""));
    let category_definition_ids = |category| {
        inventory
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.native_kind == category
                    && declaration.role == AgentAssetDeclarationRole::Definition
            })
            .map(|declaration| declaration.native_id.clone())
            .collect::<BTreeSet<_>>()
    };
    let category_asset_ids = |category| {
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == category)
            .map(|asset| asset.native_id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        category_definition_ids(AgentAssetCategory::Mcp),
        BTreeSet::from([
            "managed-only".to_owned(),
            "safe-remote".to_owned(),
            "safe-stdio".to_owned(),
        ])
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Mcp)
            .count(),
        33
    );
    assert_eq!(
        category_asset_ids(AgentAssetCategory::Mcp),
        BTreeSet::from([
            "managed-only".to_owned(),
            "safe-remote".to_owned(),
            "safe-stdio".to_owned(),
        ])
    );
    let expected_hook_ids = BTreeSet::from([
        "settings:PreToolUse:matcher-string-present:group-0:hook-0".to_owned(),
        "settings:PreToolUse:matcher-string-present:group-0:hook-1".to_owned(),
        "settings:PreToolUse:matcher-string-present:group-0:hook-2".to_owned(),
        "settings:PreToolUse:matcher-string-present:group-0:hook-3".to_owned(),
        "settings:PermissionRequest:matcher-invalid:group-0:hook-invalid".to_owned(),
        "settings:Notification:matcher-string-present:group-0:hook-0".to_owned(),
        "workspace-settings:Notification:matcher-absent:group-0:hook-0".to_owned(),
        "workspace-local-settings:Notification:matcher-absent:group-0:hook-0".to_owned(),
        "workspace-local-settings:Notification:matcher-absent:group-0:hook-1".to_owned(),
        "managed-settings:Notification:matcher-absent:group-0:hook-0".to_owned(),
    ]);
    assert_eq!(
        category_definition_ids(AgentAssetCategory::Hook),
        expected_hook_ids
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Hook)
            .count(),
        19
    );
    assert_eq!(
        category_asset_ids(AgentAssetCategory::Hook),
        expected_hook_ids
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Hook)
            .count(),
        10
    );
    assert_eq!(
        category_definition_ids(AgentAssetCategory::StatusUi),
        BTreeSet::from(["status-line".to_owned()])
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::StatusUi)
            .count(),
        10
    );
    assert_eq!(
        category_asset_ids(AgentAssetCategory::StatusUi),
        BTreeSet::from(["status-line".to_owned()])
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::StatusUi)
            .count(),
        2
    );
    assert_eq!(
        category_definition_ids(AgentAssetCategory::Plugin),
        BTreeSet::from([
            "managed-plugin".to_owned(),
            "safe-plugin".to_owned(),
            "workspace-plugin".to_owned(),
        ])
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Plugin)
            .count(),
        17
    );
    let package_roots = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Plugin
                && declaration.declaration_key.starts_with("package.root:")
        })
        .collect::<Vec<_>>();
    assert_eq!(package_roots.len(), 3);
    assert_eq!(
        package_roots
            .iter()
            .map(|declaration| declaration.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["managed-plugin", "safe-plugin", "workspace-plugin"])
    );
    assert!(package_roots.iter().all(|declaration| {
        declaration.role == AgentAssetDeclarationRole::StateOverlay
            && declaration.presence == crate::models::AgentAssetPresence::Missing
    }));
    assert_eq!(
        category_asset_ids(AgentAssetCategory::Plugin),
        BTreeSet::from([
            "managed-plugin".to_owned(),
            "safe-plugin".to_owned(),
            "workspace-plugin".to_owned(),
        ])
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Plugin)
            .count(),
        3
    );
    assert_eq!(
        category_definition_ids(AgentAssetCategory::Skill),
        BTreeSet::from(["safe-skill".to_owned(), "workspace-skill".to_owned(),])
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
            .count(),
        2
    );
    assert_eq!(
        category_asset_ids(AgentAssetCategory::Skill),
        BTreeSet::from(["safe-skill".to_owned(), "workspace-skill".to_owned(),])
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
            .count(),
        2
    );
    for source_label in ["Claude Code Plugin 缓存", "Claude Code Marketplace 记录"] {
        let source_id = inventory
            .sources
            .iter()
            .find(|source| source.label == source_label)
            .expect("cache or marketplace source")
            .id
            .clone();
        assert!(inventory
            .declarations
            .iter()
            .all(|declaration| declaration.source_id != source_id));
        assert!(inventory
            .assets
            .iter()
            .all(|asset| !asset.source_ids.contains(&source_id)));
    }
    assert!(inventory.assets.iter().any(|asset| matches!(
        asset.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    )));
    assert!(inventory.assets.iter().any(|asset| matches!(
        asset.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http | AgentMcpTransport::Sse,
            ..
        }
    )));
    assert!(inventory.assets.iter().any(|asset| matches!(
        asset.details,
        AgentAssetDetails::StatusUi {
            command_present: true,
            ..
        }
    )));
    assert!(inventory.assets.iter().all(|asset| {
        asset.relationships.provided_by.is_none()
            && asset.relationships.action_owner.is_none()
            && asset.relationships.affected_asset_ids.is_empty()
    }));
    for asset in &inventory.assets {
        let reason = if asset.resolution.relation == AgentAssetResolutionRelation::Replaced {
            AgentAssetActionUnavailableReason::Shadowed
        } else if asset.category == AgentAssetCategory::Plugin {
            assert!(matches!(
                asset.details,
                AgentAssetDetails::Plugin {
                    install_state: AgentAssetInstallState::NotInstalled,
                    ..
                }
            ));
            AgentAssetActionUnavailableReason::AssetNotInstalled
        } else {
            AgentAssetActionUnavailableReason::NoOfficialMechanism
        };
        assert_read_only_asset_actions(asset, reason);
    }
    remove_claude_fixture(root);
}

#[test]
fn claude_shared_installations_keep_stable_assets() {
    let mut first_installation =
        claude_fixture_installation("2.1.263", AgentInstallationChannel::Stable);
    first_installation.id = "installation:claude-a".to_owned();
    first_installation.executable_path = Some("/fixture/bin/claude-a".to_owned());
    first_installation.executable_identity = Some(AgentExecutableIdentity {
        owner: "fixture-a".to_owned(),
        canonical_path: "/fixture/bin/claude-a".to_owned(),
        installation_source: AgentDiscoverySource::Configured,
    });
    first_installation.executable_revision = Some("revision-a".to_owned());
    let mut second_installation = first_installation.clone();
    second_installation.id = "installation:claude-b".to_owned();
    second_installation.executable_path = Some("/fixture/bin/claude-b".to_owned());
    second_installation.executable_identity = Some(AgentExecutableIdentity {
        owner: "fixture-b".to_owned(),
        canonical_path: "/fixture/bin/claude-b".to_owned(),
        installation_source: AgentDiscoverySource::Configured,
    });
    second_installation.executable_revision = Some("revision-b".to_owned());

    let settings = crate::models::AppSettings::default();
    let root = test_root("claude-shared-installations");
    let (first_port, first_port_state) = FakeInstallationPort::new(vec![
        first_installation.clone(),
        second_installation.clone(),
    ]);
    let (root, _, first, _) = build_claude_inventory_at_root_with_workspace(
        root,
        [
            (
                "account",
                claude_json(serde_json::json!({
                    "mcpServers": { "same": { "command": "node", "args": ["first"] } },
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } }
                })),
            ),
            ("settings", claude_json(serde_json::json!({}))),
        ],
        [],
        [],
        &first_port,
        None,
        Some(&settings),
    );
    assert_eq!(
        first_port_state
            .lock()
            .expect("first fake installation state lock")
            .calls,
        vec!["claudeCode"]
    );

    let mut second_run_first = first_installation.clone();
    second_run_first.installed_version = Some("2.1.264".to_owned());
    second_run_first.channel = AgentInstallationChannel::Preview;
    second_run_first.executable_revision = Some("revision-a-2".to_owned());
    let mut second_run_second = second_installation.clone();
    second_run_second.installed_version = Some("2.1.265".to_owned());
    second_run_second.channel = AgentInstallationChannel::Nightly;
    second_run_second.executable_revision = Some("revision-b-2".to_owned());
    let (second_port, second_port_state) =
        FakeInstallationPort::new(vec![second_run_second.clone(), second_run_first.clone()]);
    let (_, _, second, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        root.clone(),
        [
            ("settings", claude_json(serde_json::json!({}))),
            (
                "account",
                claude_json(serde_json::json!({
                    "projects": { "__WORKSPACE__": { "hasTrustDialogAccepted": true } },
                    "mcpServers": { "same": { "type": "http", "url": "https://example.invalid/mcp" } }
                })),
            ),
        ],
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &second_port,
            workspace_input: None,
            settings: Some(&settings),
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        second_port_state
            .lock()
            .expect("second fake installation state lock")
            .calls,
        vec!["claudeCode"]
    );

    let identity_signature = |inventory: &crate::models::AgentEnvironmentInventory| {
        let mut signature = inventory
            .contexts
            .iter()
            .map(|context| format!("context:{}", context.id))
            .chain(inventory.installations.iter().map(|installation| {
                format!(
                    "installation:{:?}",
                    (
                        installation.id.as_str(),
                        installation.executable_path.as_deref(),
                        installation.executable_identity.as_ref().map(|identity| {
                            (
                                identity.owner.as_str(),
                                identity.canonical_path.as_str(),
                                identity.installation_source,
                            )
                        })
                    )
                )
            }))
            .chain(
                inventory
                    .sources
                    .iter()
                    .map(|source| format!("source:{}", source.id)),
            )
            .chain(
                inventory
                    .declarations
                    .iter()
                    .map(|declaration| format!("declaration:{}", declaration.id)),
            )
            .chain(inventory.assets.iter().map(|asset| {
                format!(
                    "asset:{:?}",
                    (
                        asset.stable_id.as_str(),
                        asset.category,
                        asset.native_id.as_str()
                    )
                )
            }))
            .collect::<Vec<_>>();
        signature.sort();
        signature
    };
    assert_eq!(first.contexts.len(), 1);
    assert_eq!(second.contexts.len(), 1);
    assert_eq!(first.installations.len(), 2);
    assert_eq!(second.installations.len(), 2);
    for inventory in [&first, &second] {
        assert!(inventory
            .installations
            .iter()
            .all(|installation| installation.agent_kind == AgentCliKind::ClaudeCode));
        assert_eq!(
            inventory
                .installations
                .iter()
                .map(|installation| installation.id.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            2
        );
        assert_eq!(
            inventory
                .installations
                .iter()
                .map(|installation| {
                    installation
                        .executable_identity
                        .as_ref()
                        .map(|identity| (identity.owner.as_str(), identity.canonical_path.as_str()))
                })
                .collect::<BTreeSet<_>>()
                .len(),
            2
        );
    }
    assert_eq!(identity_signature(&first), identity_signature(&second));
    assert_eq!(
        first.contexts[0].compatible_installation_ids,
        vec!["installation:claude-a", "installation:claude-b"]
    );
    assert_eq!(
        second.contexts[0].compatible_installation_ids,
        vec!["installation:claude-a", "installation:claude-b"]
    );
    for inventory in [&first, &second] {
        let declarations = claude_declarations(inventory, AgentAssetCategory::Mcp, "same");
        let assets = claude_assets(inventory, AgentAssetCategory::Mcp, "same");
        assert_eq!(declarations.len(), 1, "{inventory:?}");
        assert_eq!(assets.len(), 1, "{inventory:?}");
        let declaration = declarations[0];
        let asset = assets[0];
        let source = inventory
            .sources
            .iter()
            .find(|source| source.id == declaration.source_id)
            .expect("MCP declaration source");
        assert_eq!(
            asset.represented_declaration_ids,
            vec![declaration.id.clone()]
        );
        assert_eq!(
            asset.resolution.contributor_ids,
            vec![declaration.id.clone()]
        );
        assert_eq!(asset.source_ids, vec![source.id.clone()]);
        assert_eq!(asset.inspection_source_id, source.id);
        assert_eq!(asset.revision, declaration.evidence.revision);
        assert_eq!(asset.revision, source.revision);
        assert_eq!(
            asset.compatible_installation_ids,
            vec!["installation:claude-a", "installation:claude-b"]
        );
        assert!(asset.selected_action_installation_id.is_none());
        assert_read_only_asset_actions(
            asset,
            AgentAssetActionUnavailableReason::NoOfficialMechanism,
        );
        assert_no_structural_projection_diagnostics(inventory);
    }
    let mcp_source_revision = |inventory: &crate::models::AgentEnvironmentInventory| {
        let source_id =
            &claude_declarations(inventory, AgentAssetCategory::Mcp, "same")[0].source_id;
        inventory
            .sources
            .iter()
            .find(|source| source.id == *source_id)
            .expect("MCP declaration source")
            .revision
            .identity
            .clone()
    };
    assert_ne!(mcp_source_revision(&first), mcp_source_revision(&second));
    assert_ne!(
        claude_declarations(&first, AgentAssetCategory::Mcp, "same")[0]
            .evidence
            .revision,
        claude_declarations(&second, AgentAssetCategory::Mcp, "same")[0]
            .evidence
            .revision
    );
    assert!(matches!(
        claude_assets(&first, AgentAssetCategory::Mcp, "same")[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    ));
    assert!(matches!(
        claude_assets(&second, AgentAssetCategory::Mcp, "same")[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http | AgentMcpTransport::Sse,
            ..
        }
    ));
    for installation_id in ["installation:claude-a", "installation:claude-b"] {
        let first_installation = first
            .installations
            .iter()
            .find(|installation| installation.id == installation_id)
            .expect("first-run installation");
        let second_installation = second
            .installations
            .iter()
            .find(|installation| installation.id == installation_id)
            .expect("second-run installation");
        assert_ne!(
            first_installation.installed_version,
            second_installation.installed_version
        );
        assert_ne!(first_installation.channel, second_installation.channel);
        assert_ne!(
            first_installation.executable_revision,
            second_installation.executable_revision
        );
    }
    remove_claude_fixture(root);
}

#[test]
fn claude_overlay_only_evidence_does_not_hide_definition_bug() {
    let orphan_files = [
        (
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": true,
                    "enabledMcpServers": ["orphan-mcp"]
                } }
            })),
        ),
        (
            "settings",
            claude_json(serde_json::json!({
                "enabledMcpjsonServers": ["orphan-mcp"],
                "enabledPlugins": { "orphan-plugin": true }
            })),
        ),
    ];
    let (orphan_root, _, orphan, _) = build_claude_inventory(
        "claude-overlay-only",
        orphan_files.clone(),
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let orphan_mcp = claude_declarations(&orphan, AgentAssetCategory::Mcp, "orphan-mcp");
    let orphan_plugin = claude_declarations(&orphan, AgentAssetCategory::Plugin, "orphan-plugin");
    assert_eq!(orphan_mcp.len(), 2);
    assert_eq!(orphan_plugin.len(), 1);
    let orphan_mcp_policy = orphan_mcp
        .iter()
        .find(|item| item.role == AgentAssetDeclarationRole::PolicyOverlay)
        .expect("orphan MCP policy overlay");
    let orphan_mcp_state = orphan_mcp
        .iter()
        .find(|item| item.role == AgentAssetDeclarationRole::StateOverlay)
        .expect("orphan MCP state overlay");
    assert_eq!(orphan_mcp_policy.scope, AgentAssetScope::User);
    assert_eq!(orphan_mcp_policy.precedence, 10);
    assert_eq!(orphan_mcp_state.scope, AgentAssetScope::Local);
    assert_eq!(orphan_mcp_state.precedence, 30);
    assert_eq!(
        orphan_plugin[0].role,
        AgentAssetDeclarationRole::StateOverlay
    );
    assert_eq!(orphan_plugin[0].scope, AgentAssetScope::User);
    assert_eq!(orphan_plugin[0].precedence, 10);
    assert!(claude_assets(&orphan, AgentAssetCategory::Mcp, "orphan-mcp").is_empty());
    assert!(claude_assets(&orphan, AgentAssetCategory::Plugin, "orphan-plugin").is_empty());
    assert_no_structural_projection_diagnostics(&orphan);
    let (_, _, orphan_reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        orphan_root.clone(),
        orphan_files,
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &RealInstallationDiscoveryPort,
            workspace_input: None,
            settings: None,
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        claude_full_public_signature(&orphan),
        claude_full_public_signature(&orphan_reversed)
    );
    assert_eq!(
        claude_non_record_diagnostic_signature(&orphan),
        claude_non_record_diagnostic_signature(&orphan_reversed)
    );
    assert_no_structural_projection_diagnostics(&orphan_reversed);
    remove_claude_fixture(orphan_root);

    let suppressed_files = [
        (
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": false,
                    "enabledMcpServers": ["orphan-mcp"]
                } }
            })),
        ),
        (
            "workspace-mcp",
            claude_json(serde_json::json!({
                "mcpServers": { "orphan-mcp": { "command": "workspace" } }
            })),
        ),
        (
            "settings",
            claude_json(serde_json::json!({
                "enabledMcpjsonServers": ["orphan-mcp"]
            })),
        ),
    ];
    let (suppressed_root, _, suppressed, _) = build_claude_inventory(
        "claude-overlay-only-suppressed",
        suppressed_files.clone(),
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let suppressed_declarations =
        claude_declarations(&suppressed, AgentAssetCategory::Mcp, "orphan-mcp");
    assert_eq!(suppressed_declarations.len(), 3, "{suppressed:?}");
    let suppressed_definition = suppressed_declarations
        .iter()
        .find(|declaration| declaration.role == AgentAssetDeclarationRole::Definition)
        .expect("suppressed MCP definition");
    let suppressed_overlay = suppressed_declarations
        .iter()
        .find(|declaration| declaration.role == AgentAssetDeclarationRole::PolicyOverlay)
        .expect("participating MCP overlay");
    let suppressed_state = suppressed_declarations
        .iter()
        .find(|declaration| declaration.role == AgentAssetDeclarationRole::StateOverlay)
        .expect("suppressed MCP state overlay");
    assert!(matches!(
        suppressed_definition.participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
        }
    ));
    assert!(matches!(
        suppressed_overlay.participation,
        AgentAssetResolutionParticipation::Participates
    ));
    assert!(matches!(
        suppressed_state.participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
        }
    ));
    assert_eq!(suppressed_state.scope, AgentAssetScope::Local);
    assert_eq!(suppressed_state.precedence, 30);
    let suppressed_assets = claude_assets(&suppressed, AgentAssetCategory::Mcp, "orphan-mcp");
    assert_eq!(suppressed_assets.len(), 1, "{suppressed:?}");
    assert_eq!(
        suppressed_assets[0].resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        suppressed_assets[0].resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(suppressed_assets[0].resolution.contributor_ids.is_empty());
    assert!(suppressed_assets[0].resolution.winner_id.is_none());
    assert!(suppressed_assets[0].resolution.control_source.is_none());
    assert_eq!(
        suppressed_assets[0].effective_state,
        AgentAssetState::Unknown
    );
    assert!(matches!(
        suppressed_assets[0].details,
        AgentAssetDetails::Mcp {
            declared_state: AgentAssetDeclaredState::Unknown,
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    let mut represented = suppressed_declarations
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    represented.sort();
    let mut actual_represented = suppressed_assets[0].represented_declaration_ids.clone();
    actual_represented.sort();
    assert_eq!(actual_represented, represented);
    assert_eq!(
        suppressed_assets[0].inspection_source_id,
        suppressed
            .sources
            .iter()
            .find(|source| source.id == suppressed_definition.source_id)
            .expect("suppressed definition source")
            .id
    );
    assert_eq!(
        suppressed_assets[0].revision,
        suppressed_definition.evidence.revision
    );
    assert!(suppressed_definition
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DeclarationSuppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
            }
        )));
    assert_read_only_asset_actions(
        suppressed_assets[0],
        AgentAssetActionUnavailableReason::Unknown,
    );
    assert_no_structural_projection_diagnostics(&suppressed);
    let (_, _, suppressed_reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        suppressed_root.clone(),
        suppressed_files,
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &RealInstallationDiscoveryPort,
            workspace_input: None,
            settings: None,
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        claude_full_public_signature(&suppressed),
        claude_full_public_signature(&suppressed_reversed)
    );
    assert_eq!(
        claude_non_record_diagnostic_signature(&suppressed),
        claude_non_record_diagnostic_signature(&suppressed_reversed)
    );
    assert_no_structural_projection_diagnostics(&suppressed_reversed);
    remove_claude_fixture(suppressed_root);

    let trusted_files = [
        (
            "account",
            claude_json(serde_json::json!({
                "projects": { "__WORKSPACE__": {
                    "hasTrustDialogAccepted": true,
                    "enabledMcpServers": ["orphan-mcp"]
                } }
            })),
        ),
        (
            "workspace-mcp",
            claude_json(serde_json::json!({
                "mcpServers": { "orphan-mcp": { "command": "workspace" } }
            })),
        ),
        (
            "settings",
            claude_json(serde_json::json!({
                "enabledMcpjsonServers": ["orphan-mcp"]
            })),
        ),
    ];
    let (trusted_root, _, trusted, _) = build_claude_inventory(
        "claude-overlay-only-trusted",
        trusted_files.clone(),
        [],
        [],
        &RealInstallationDiscoveryPort,
    );
    let trusted_declarations = claude_declarations(&trusted, AgentAssetCategory::Mcp, "orphan-mcp");
    let trusted_assets = claude_assets(&trusted, AgentAssetCategory::Mcp, "orphan-mcp");
    assert_eq!(trusted_declarations.len(), 3, "{trusted:?}");
    assert_eq!(trusted_assets.len(), 1, "{trusted:?}");
    assert!(trusted_declarations.iter().all(|declaration| matches!(
        declaration.participation,
        AgentAssetResolutionParticipation::Participates
    )));
    let mut trusted_ids = trusted_declarations
        .iter()
        .map(|declaration| declaration.id.clone())
        .collect::<Vec<_>>();
    trusted_ids.sort();
    let mut trusted_represented = trusted_assets[0].represented_declaration_ids.clone();
    trusted_represented.sort();
    assert_eq!(trusted_represented, trusted_ids);
    let mut trusted_contributors = trusted_assets[0].resolution.contributor_ids.clone();
    trusted_contributors.sort();
    assert_eq!(trusted_contributors, trusted_ids);
    assert!(matches!(
        trusted_assets[0].details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            ..
        }
    ));
    assert_read_only_asset_actions(
        trusted_assets[0],
        AgentAssetActionUnavailableReason::NoOfficialMechanism,
    );
    assert_no_structural_projection_diagnostics(&trusted);
    let (_, _, trusted_reversed, _) = build_claude_inventory_at_root_with_workspace_and_adapter(
        trusted_root.clone(),
        trusted_files,
        [],
        [],
        ClaudeInventoryBuildOptions {
            installations: &RealInstallationDiscoveryPort,
            workspace_input: None,
            settings: None,
            test_adapter: claude_test_reversed_adapter(),
            strict_snapshots: false,
            strict_missing_directories: &[],
        },
    );
    assert_eq!(
        claude_full_public_signature(&trusted),
        claude_full_public_signature(&trusted_reversed)
    );
    assert_eq!(
        claude_non_record_diagnostic_signature(&trusted),
        claude_non_record_diagnostic_signature(&trusted_reversed)
    );
    assert_no_structural_projection_diagnostics(&trusted_reversed);
    remove_claude_fixture(trusted_root);

    let direct_context = claude_focused_context();
    let mut direct_source = claude_focused_source(
        "account",
        AgentAssetScope::User,
        10,
        &[AgentAssetCategory::Mcp],
    );
    // The real account file owns both user Definitions and project controls.
    direct_source.allowed_logical_origins.push(
        crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
            scope: AgentAssetScope::Local,
            precedence: 30,
        },
    );
    let direct_snapshot = AgentAssetSnapshot::File {
        bytes: claude_json(serde_json::json!({
            "mcpServers": { "definition": { "command": "node" } }
        })),
        revision: AgentAssetRevision::default(),
    };
    let mut direct_parsed = NativePayloadDebugCollector::default();
    claude_test_parse(
        AgentAssetParseRequest {
            context: &direct_context,
            source: &direct_source,
            snapshot: &direct_snapshot,
            native_home: None,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut direct_parsed,
    );
    assert_eq!(direct_parsed.declarations.len(), 4);
    assert_eq!(
        direct_parsed
            .declarations
            .iter()
            .filter(|declaration| { declaration.role == AgentAssetDeclarationRole::Definition })
            .count(),
        1
    );
    let mut direct_run = inventory_run(AgentAssetLimits::DEFAULT);
    let direct_environment = native_environment();
    let direct_result = super::projection::project_records(
        &direct_environment,
        &direct_context,
        std::slice::from_ref(&direct_source),
        &direct_parsed.declarations,
        super::projection::AgentAssetProjectionInput {
            drafts: Vec::new(),
            state_assessor: definition(AgentCliKind::ClaudeCode)
                .environment()
                .state_assessor(),
        },
        &BTreeMap::new(),
        &mut direct_run,
    );
    assert!(direct_result.records.is_empty());
    let direct_diagnostics = direct_run.finish_diagnostics();
    assert_eq!(
        direct_diagnostics,
        vec![AgentAssetDiagnostic::InvalidResolution {
            projection_key: "resolution-group:mcp:definition".to_owned(),
            resolution: AgentAssetResolutionRelation::Unknown,
        }]
    );
}

fn fixture_manifest_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let _ = output.emit_initial(AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-manifest".to_owned(),
        label: "Manifest fixture".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join("manifest"),
        allowed_root: root.to_path_buf(),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    });
}

fn fixture_manifest_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    for entry in request.manifest {
        let (target, source_kind) = if entry.name == "SKILL.md"
            && entry.source_kind == AgentAssetSourceKind::File
        {
            (
                AgentFollowUpSourceTarget::ManifestFile {
                    entry_name: entry.name.clone(),
                },
                AgentAssetSourceKind::File,
            )
        } else if entry.source_kind == AgentAssetSourceKind::Directory && entry.name == "release" {
            (
                AgentFollowUpSourceTarget::Descendant {
                    directory_entry_name: entry.name.clone(),
                    relative_path: PathBuf::from("SKILL.md"),
                },
                AgentAssetSourceKind::File,
            )
        } else {
            continue;
        };
        let native_source_key = format!("{}:{}", request.parent.native_source_key, entry.name);
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: request.parent.native_source_key.clone(),
                target,
                native_source_key,
                label: entry.name.clone(),
                scope: AgentAssetScope::User,
                precedence: 1,
                sensitive: false,
                source_kind,
                categories: vec![AgentAssetCategory::Skill],
            })
            .is_break()
        {
            return;
        }
    }
}

fn fixture_direct_descendant_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let offers = [
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::ManifestFile {
                entry_name: "SKILL.md".to_owned(),
            },
            native_source_key: "fixture-direct-child".to_owned(),
            label: "Fixture direct child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: "release".to_owned(),
                relative_path: PathBuf::from("SKILL.md"),
            },
            native_source_key: "fixture-descendant-child".to_owned(),
            label: "Fixture descendant child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
    ];
    for offer in offers {
        if output.emit_follow_up(offer).is_break() {
            return;
        }
    }
}

fn fixture_direct_descendant_with_hidden_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let offers = [
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::ManifestFile {
                entry_name: "SKILL.md".to_owned(),
            },
            native_source_key: "fixture-direct-child".to_owned(),
            label: "Fixture direct child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: "release".to_owned(),
                relative_path: PathBuf::from("SKILL.md"),
            },
            native_source_key: "fixture-descendant-child".to_owned(),
            label: "Fixture descendant child".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::ManifestFile {
                entry_name: "z-hidden.md".to_owned(),
            },
            native_source_key: "fixture-hidden-direct".to_owned(),
            label: "Fixture hidden direct".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: "z-hidden-dir".to_owned(),
                relative_path: PathBuf::from("SKILL.md"),
            },
            native_source_key: "fixture-hidden-descendant".to_owned(),
            label: "Fixture hidden descendant".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
    ];
    for offer in offers {
        if output.emit_follow_up(offer).is_break() {
            return;
        }
    }
}

fn fixture_mixed_type_mismatch_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    let offers = [
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::ManifestFile {
                entry_name: "SKILL.md".to_owned(),
            },
            native_source_key: "mixed-direct-mismatch".to_owned(),
            label: "Mixed direct mismatch".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
        AgentFollowUpSourceSpec {
            parent_source_key: request.parent.native_source_key.clone(),
            target: AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: "release".to_owned(),
                relative_path: PathBuf::from("SKILL.md"),
            },
            native_source_key: "mixed-descendant-mismatch".to_owned(),
            label: "Mixed descendant mismatch".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Skill],
        },
    ];
    for offer in offers {
        if output.emit_follow_up(offer).is_break() {
            return;
        }
    }
}

fn emit_unsafe_descendant(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
    relative_path: &str,
) {
    let _ = output.emit_follow_up(AgentFollowUpSourceSpec {
        parent_source_key: request.parent.native_source_key.clone(),
        target: AgentFollowUpSourceTarget::Descendant {
            directory_entry_name: "skills".to_owned(),
            relative_path: PathBuf::from(relative_path),
        },
        native_source_key: "unsafe-descendant".to_owned(),
        label: "Unsafe descendant".to_owned(),
        scope: AgentAssetScope::User,
        precedence: 1,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Skill],
    });
}

fn fixture_unsafe_empty(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "");
}

fn fixture_unsafe_dot(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, ".");
}

fn fixture_unsafe_parent(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "..");
}

fn fixture_unsafe_escape(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "../escape");
}

fn fixture_unsafe_absolute(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "/absolute");
}

fn fixture_unsafe_drive(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "C:/escape");
}

fn fixture_unsafe_separator(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "a\\b");
}

fn fixture_unsafe_control(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "line\nfeed");
}

fn fixture_unsafe_nul(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_unsafe_descendant(request, output, "nul\0byte");
}

fn fixture_mixed_manifest_source(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let root = Path::new(&request.context.config_root);
    let _ = output.emit_initial(AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "fixture-mixed".to_owned(),
        label: "Mixed manifest fixture".to_owned(),
        scope: AgentAssetScope::User,
        path: root.join("mixed"),
        allowed_root: root.to_path_buf(),
        precedence: 1,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: vec![AgentAssetCategory::Skill],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
        ],
    });
}

fn fixture_mixed_manifest_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_mixed_manifest_follow_up(request, output, false);
}

fn emit_mixed_manifest_follow_up(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
    reversed: bool,
) {
    let direct = AgentFollowUpSourceSpec {
        parent_source_key: request.parent.native_source_key.clone(),
        target: AgentFollowUpSourceTarget::ManifestFile {
            entry_name: "SKILL.md".to_owned(),
        },
        native_source_key: "mixed-direct".to_owned(),
        label: "Mixed direct".to_owned(),
        scope: AgentAssetScope::User,
        precedence: 1,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Skill],
    };
    let child = AgentFollowUpSourceSpec {
        parent_source_key: request.parent.native_source_key.clone(),
        target: AgentFollowUpSourceTarget::Descendant {
            directory_entry_name: "release".to_owned(),
            relative_path: PathBuf::from("SKILL.md"),
        },
        native_source_key: "mixed-child".to_owned(),
        label: "Mixed child".to_owned(),
        scope: AgentAssetScope::User,
        precedence: 1,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Skill],
    };
    let collision = AgentFollowUpSourceSpec {
        native_source_key: "mixed-collision".to_owned(),
        label: "Mixed collision".to_owned(),
        ..direct.clone()
    };
    let offers = if reversed {
        vec![child, direct.clone(), collision]
    } else {
        vec![direct.clone(), child, collision]
    };
    for offer in offers {
        if output.emit_follow_up(offer).is_break() {
            return;
        }
    }
    let over_limit = AgentFollowUpSourceSpec {
        native_source_key: "mixed-over-limit".to_owned(),
        label: "Mixed over limit".to_owned(),
        ..direct.clone()
    };
    if output.emit_follow_up(over_limit).is_break() {
        return;
    }
    // Keep this construction after the producer observes the first Break. A
    // real adapter must not even construct work that the bounded sink cannot
    // accept, in addition to never snapshotting it.
    let sentinel = AgentFollowUpSourceSpec {
        native_source_key: "mixed-post-break-sentinel".to_owned(),
        label: "Mixed post-break sentinel".to_owned(),
        ..direct
    };
    let _ = output.emit_follow_up(sentinel);
}

fn fixture_mixed_manifest_follow_up_reversed(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    emit_mixed_manifest_follow_up(request, output, true);
}

#[test]
fn direct_manifest_file_runs_inventory_follow_up_snapshot_and_parse_pipeline() {
    let root = test_root("direct-manifest-production");
    fs::create_dir_all(root.join("fixture/manifest/release")).unwrap();
    fs::write(root.join("fixture/manifest/SKILL.md"), b"root").unwrap();
    fs::write(root.join("fixture/manifest/release/SKILL.md"), b"child").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_manifest_source,
        Some(fixture_manifest_follow_up),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(super::run::SystemMonotonicClock),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &RealSnapshotPort::default(),
            checkpoint_probe: None,
        },
    )
    .unwrap();
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("manifest/SKILL.md")));
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("manifest/release/SKILL.md")));
    let direct_source_ids = inventory
        .sources
        .iter()
        .filter(|source| {
            source
                .path
                .replace('\\', "/")
                .ends_with("manifest/SKILL.md")
        })
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    let child_source_ids = inventory
        .sources
        .iter()
        .filter(|source| {
            source
                .path
                .replace('\\', "/")
                .ends_with("manifest/release/SKILL.md")
        })
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| direct_source_ids.contains(declaration.source_id.as_str())));
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| child_source_ids.contains(declaration.source_id.as_str())));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_and_descendant_children_reuse_parent_snapshot_and_close_resources() {
    let root = test_root("direct-descendant-resource-closure");
    fs::create_dir_all(root.join("fixture/parent/release")).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definitions = [fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_direct_descendant_follow_up),
        fixture_parse_source_key,
        fixture_resolve,
    )];
    let child_entries = vec![
        AgentAssetDirectoryEntry {
            name: "SKILL.md".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        },
        AgentAssetDirectoryEntry {
            name: "release".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
    ];
    let (snapshots, state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::directory("fixture-parent", 10, child_entries, true),
        ExpectedSnapshot::file_bytes("fixture-direct-child", b"direct"),
        ExpectedSnapshot::file_bytes("fixture-descendant-child", b"child"),
    ]);
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(super::run::SystemMonotonicClock),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    let state = state.lock().expect("direct/descendant resource state lock");
    assert_eq!(
        state.native_source_keys,
        [
            "fixture-parent",
            "fixture-direct-child",
            "fixture-descendant-child",
        ]
    );
    assert_eq!(state.opens, state.attempts);
    assert_eq!(inventory.sources.len(), 3);
    assert_eq!(inventory.declarations.len(), 3);
    assert!(inventory
        .sources
        .iter()
        .any(|source| source.path.replace('\\', "/").ends_with("parent/SKILL.md")));
    assert!(inventory.sources.iter().any(|source| source
        .path
        .replace('\\', "/")
        .ends_with("parent/release/SKILL.md")));
    snapshots.assert_scripted_exhausted();
    drop(state);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mixed_direct_descendant_children_close_on_deadline_and_bytes_without_hidden_reads() {
    let manifest = vec![
        AgentAssetDirectoryEntry {
            name: "SKILL.md".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        },
        AgentAssetDirectoryEntry {
            name: "release".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
        AgentAssetDirectoryEntry {
            name: "z-hidden.md".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        },
        AgentAssetDirectoryEntry {
            name: "z-hidden-dir".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
    ];

    let deadline_root = test_root("mixed-direct-descendant-deadline");
    fs::create_dir_all(deadline_root.join("fixture/parent/release")).unwrap();
    let deadline_root = fs::canonicalize(deadline_root).unwrap();
    let deadline_clock = Arc::new(ManualClock::new());
    let deadline_probe = Arc::new(StageCheckpointProbe::new(deadline_clock.clone()));
    deadline_probe.advance_on(
        AgentInventoryStage::FollowUpSourceDiscovery,
        1,
        std::time::Duration::from_millis(AgentAssetLimits::DEFAULT.refresh_budget_ms),
    );
    let (deadline_snapshots, deadline_state) =
        FakeSnapshotPort::scripted([ExpectedSnapshot::directory(
            "fixture-parent",
            10,
            manifest.clone(),
            true,
        )]);
    let deadline_definition = fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_direct_descendant_with_hidden_follow_up),
        fixture_parse_source_key,
        fixture_resolve,
    );
    let deadline_inventory = build_inventory_with(
        InventoryInput {
            home: &deadline_root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: std::slice::from_ref(&deadline_definition),
            limits: AgentAssetLimits::DEFAULT,
            clock: deadline_clock,
            installations: &RealInstallationDiscoveryPort,
            snapshots: &deadline_snapshots,
            checkpoint_probe: Some(deadline_probe),
        },
    )
    .unwrap();
    let deadline_state = deadline_state
        .lock()
        .expect("mixed deadline snapshot state");
    assert_eq!(deadline_state.native_source_keys, ["fixture-parent"]);
    assert_eq!(deadline_state.opens, deadline_state.attempts);
    assert_eq!(deadline_inventory.sources.len(), 1);
    assert!(deadline_inventory
        .sources
        .iter()
        .all(|source| !source.path.contains("hidden") && !source.path.contains("release")));
    drop(deadline_state);
    deadline_snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(deadline_root);

    let bytes_root = test_root("mixed-direct-descendant-bytes");
    fs::create_dir_all(bytes_root.join("fixture/parent/release")).unwrap();
    let bytes_root = fs::canonicalize(bytes_root).unwrap();
    let bytes_definition = fixture_definition_with_pipeline(
        fixture_parent_source,
        Some(fixture_direct_descendant_with_hidden_follow_up),
        fixture_parse_source_key,
        fixture_resolve,
    );
    let bytes_limits = AgentAssetLimits {
        bytes_per_refresh: 11,
        ..AgentAssetLimits::DEFAULT
    };
    let (byte_snapshots, byte_state) = FakeSnapshotPort::scripted([
        ExpectedSnapshot::directory("fixture-parent", 10, manifest, true),
        ExpectedSnapshot::file("fixture-direct-child", 1),
        ExpectedSnapshot::file("fixture-descendant-child", 1),
        ExpectedSnapshot::file("fixture-hidden-direct", 1),
        ExpectedSnapshot::file("fixture-hidden-descendant", 1),
    ]);
    let byte_inventory = build_inventory_with(
        InventoryInput {
            home: &bytes_root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: std::slice::from_ref(&bytes_definition),
            limits: bytes_limits,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &byte_snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();
    let byte_state = byte_state.lock().expect("mixed byte snapshot state");
    assert_eq!(
        byte_state.native_source_keys,
        ["fixture-parent", "fixture-direct-child"]
    );
    assert_eq!(byte_state.opens, byte_state.attempts);
    assert!(byte_state
        .native_source_keys
        .iter()
        .all(|key| !key.contains("hidden")));
    assert!(byte_inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::BytesPerRefresh,
            ..
        }
    )));
    assert!(byte_inventory
        .sources
        .iter()
        .all(|source| !source.path.contains("hidden")));
    drop(byte_state);
    byte_snapshots.assert_next_scripted("fixture-descendant-child");
    let _ = fs::remove_dir_all(bytes_root);
}

#[test]
fn mixed_direct_descendant_type_mismatches_are_reported_by_production_pipeline() {
    let root = test_root("mixed-direct-descendant-diagnostics");
    fs::create_dir_all(root.join("fixture/mixed/SKILL.md")).unwrap();
    fs::write(root.join("fixture/mixed/release"), b"not-a-directory").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let definition = fixture_definition_with_pipeline(
        fixture_mixed_manifest_source,
        Some(fixture_mixed_type_mismatch_follow_up),
        fixture_parse_source_key,
        fixture_resolve,
    );
    let inventory = build_inventory_with(
        InventoryInput {
            home: &root,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: std::slice::from_ref(&definition),
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &RealSnapshotPort::default(),
            checkpoint_probe: None,
        },
    )
    .unwrap();
    assert_eq!(inventory.sources.len(), 1);
    assert_eq!(inventory.declarations.len(), 1);
    assert_eq!(inventory.assets.len(), 1);
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::SourceTypeMismatch {
                    source_id,
                    expected: AgentAssetSourceKind::File,
                    actual: AgentAssetSourceKind::Directory,
                } if source_id == "fixture-mixed"
            ))
            .count(),
        1
    );
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::SourceTypeMismatch {
                    source_id,
                    expected: AgentAssetSourceKind::Directory,
                    actual: AgentAssetSourceKind::File,
                } if source_id == "fixture-mixed"
            ))
            .count(),
        1
    );
    assert_eq!(
        inventory
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::SourceTypeMismatch { .. }
            ))
            .count(),
        2
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unsafe_descendant_paths_report_one_exact_inventory_diagnostic_per_case() {
    let cases: [(
        &str,
        crate::services::agent_cli::contracts::AgentFollowUpSourceDiscovery,
    ); 9] = [
        ("empty", fixture_unsafe_empty),
        ("dot", fixture_unsafe_dot),
        ("parent", fixture_unsafe_parent),
        ("escape", fixture_unsafe_escape),
        ("absolute", fixture_unsafe_absolute),
        ("drive", fixture_unsafe_drive),
        ("separator", fixture_unsafe_separator),
        ("control", fixture_unsafe_control),
        ("nul", fixture_unsafe_nul),
    ];
    for (label, follow_up) in cases {
        let root = test_root(&format!("unsafe-descendant-{label}"));
        fs::create_dir_all(root.join("fixture/mixed/skills")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let definition = fixture_definition_with_pipeline(
            fixture_mixed_manifest_source,
            Some(follow_up),
            fixture_parse_source_key,
            fixture_resolve,
        );
        let inventory = build_inventory_with(
            InventoryInput {
                home: &root,
                workspace: None,
                settings: None,
            },
            InventoryPipelineDeps {
                definitions: std::slice::from_ref(&definition),
                limits: AgentAssetLimits::DEFAULT,
                clock: Arc::new(super::run::SystemMonotonicClock),
                installations: &RealInstallationDiscoveryPort,
                snapshots: &RealSnapshotPort::default(),
                checkpoint_probe: None,
            },
        )
        .unwrap();
        assert_eq!(inventory.sources.len(), 1, "case: {label}");
        assert_eq!(inventory.declarations.len(), 1, "case: {label}");
        assert_eq!(inventory.assets.len(), 1, "case: {label}");
        assert_eq!(inventory.diagnostics.len(), 1, "case: {label}");
        assert!(matches!(
            inventory.diagnostics.first(),
            Some(AgentAssetDiagnostic::InvalidProjection { projection_key })
                if projection_key == "follow-up:fixture-mixed:skills"
        ));
        assert!(!inventory.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::SourceTypeMismatch {
                expected: AgentAssetSourceKind::Directory,
                actual: AgentAssetSourceKind::Directory,
                ..
            }
        )));
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn mixed_direct_descendant_pipeline_is_bounded_collision_free_and_deterministic() {
    fn build(
        root: &Path,
        follow_up: crate::services::agent_cli::contracts::AgentFollowUpSourceDiscovery,
    ) -> (
        crate::models::AgentEnvironmentInventory,
        Arc<Mutex<FakeSnapshotState>>,
        FakeSnapshotPort,
    ) {
        let definitions = [fixture_definition_with_pipeline(
            fixture_mixed_manifest_source,
            Some(follow_up),
            fixture_parse_source_key,
            fixture_resolve,
        )];
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.sources_per_context = 3;
        let entries = vec![
            AgentAssetDirectoryEntry {
                name: "SKILL.md".to_owned(),
                source_kind: AgentAssetSourceKind::File,
                is_symlink: false,
            },
            AgentAssetDirectoryEntry {
                name: "release".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            },
        ];
        let (snapshots, state) = FakeSnapshotPort::scripted([
            ExpectedSnapshot::directory("fixture-mixed", 10, entries, true),
            ExpectedSnapshot::file("mixed-child", 1),
        ]);
        let inventory = build_inventory_with(
            InventoryInput {
                home: root,
                workspace: None,
                settings: None,
            },
            InventoryPipelineDeps {
                definitions: &definitions,
                limits,
                clock: Arc::new(super::run::SystemMonotonicClock),
                installations: &RealInstallationDiscoveryPort,
                snapshots: &snapshots,
                checkpoint_probe: None,
            },
        )
        .unwrap();
        (inventory, state, snapshots)
    }

    let first_root = test_root("mixed-manifest-first");
    fs::create_dir_all(first_root.join("fixture/mixed/release")).unwrap();
    fs::write(first_root.join("fixture/mixed/SKILL.md"), b"direct").unwrap();
    fs::write(first_root.join("fixture/mixed/release/SKILL.md"), b"child").unwrap();
    let first_root = fs::canonicalize(first_root).unwrap();
    let (first, first_snapshot_state, first_snapshots) =
        build(&first_root, fixture_mixed_manifest_follow_up);
    let mut first_paths = first
        .sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    first_paths.sort();
    assert!(first_paths.iter().any(|path| path.ends_with("mixed")));
    assert!(first_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("mixed/release/SKILL.md")));
    assert!(!first_paths
        .iter()
        .any(|path| path.replace('\\', "/").ends_with("mixed/SKILL.md")));
    assert!(first.diagnostics.iter().any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidProjection { projection_key } if projection_key == "source:mixed-collision")));
    assert!(first.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::SourcesPerContext,
            ..
        }
    )));
    let first_snapshot_state = first_snapshot_state
        .lock()
        .expect("mixed bounded snapshot state lock");
    assert_eq!(
        first_snapshot_state.native_source_keys,
        ["fixture-mixed", "mixed-child"]
    );
    assert!(!first_snapshot_state
        .native_source_keys
        .iter()
        .any(|key| key == "mixed-post-break-sentinel"));
    drop(first_snapshot_state);
    first_snapshots.assert_scripted_exhausted();
    let first_root_prefix = first_root.to_string_lossy().into_owned();

    let second_root = test_root("mixed-manifest-reversed");
    fs::create_dir_all(second_root.join("fixture/mixed/release")).unwrap();
    fs::write(second_root.join("fixture/mixed/SKILL.md"), b"direct").unwrap();
    fs::write(second_root.join("fixture/mixed/release/SKILL.md"), b"child").unwrap();
    let second_root = fs::canonicalize(second_root).unwrap();
    let (second, second_snapshot_state, second_snapshots) =
        build(&second_root, fixture_mixed_manifest_follow_up_reversed);
    let mut second_paths = second
        .sources
        .iter()
        .map(|source| {
            source
                .path
                .replace(second_root.to_string_lossy().as_ref(), "<root>")
        })
        .collect::<Vec<_>>();
    second_paths.sort();
    assert_eq!(
        second_paths,
        first_paths
            .iter()
            .map(|path| path.replace(&first_root_prefix, "<root>"))
            .collect::<Vec<_>>()
    );
    let second_snapshot_state = second_snapshot_state
        .lock()
        .expect("reversed mixed bounded snapshot state lock");
    assert_eq!(
        second_snapshot_state.native_source_keys,
        ["fixture-mixed", "mixed-child"]
    );
    drop(second_snapshot_state);
    second_snapshots.assert_scripted_exhausted();
    let _ = fs::remove_dir_all(first_root);
    let _ = fs::remove_dir_all(second_root);
}

mod gemini_proof;
