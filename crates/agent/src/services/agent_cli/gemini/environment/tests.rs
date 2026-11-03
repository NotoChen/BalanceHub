use super::*;
use crate::{models::*, services::agent_cli::contracts::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

mod hook_global;

#[derive(Default)]
struct FollowUpCollector {
    sources: Vec<AgentFollowUpSourceSpec>,
    initial_sources: Vec<AgentAssetSourceSpec>,
    diagnostics: Vec<AgentAssetDiagnostic>,
    stop_after_first: bool,
}

impl AgentDiagnosticOutput for FollowUpCollector {
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

impl FollowUpSourceOutput for FollowUpCollector {
    fn emit_follow_up(&mut self, value: AgentFollowUpSourceSpec) -> ControlFlow<AgentOutputStop> {
        self.sources.push(value);
        if self.stop_after_first && self.sources.len() == 1 {
            ControlFlow::Break(AgentOutputStop::SourceLimit)
        } else {
            ControlFlow::Continue(())
        }
    }
}

impl InitialSourceOutput for FollowUpCollector {
    fn emit_initial(
        &mut self,
        value: crate::services::agent_cli::contracts::AgentAssetSourceSpec,
    ) -> ControlFlow<AgentOutputStop> {
        self.initial_sources.push(value);
        ControlFlow::Continue(())
    }
}

#[derive(Default)]
struct ParseCollector {
    declarations: Vec<ParsedAgentAsset>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

impl AgentDiagnosticOutput for ParseCollector {
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

impl AgentParseOutput for ParseCollector {
    fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        self.declarations.push(value);
        ControlFlow::Continue(())
    }
}

fn assert_malformed_hook_slots(diagnostics: &[AgentAssetDiagnostic], locations: &[&str]) {
    let expected = locations
        .iter()
        .flat_map(|location| {
            [
                AgentAssetDiagnostic::Malformed {
                    format: AgentAssetDocumentFormat::Json,
                    location: Some((*location).to_owned()),
                },
                AgentAssetDiagnostic::DiscoveryIncomplete {
                    agent_kind: AgentCliKind::Gemini,
                    category: AgentAssetCategory::Hook,
                    reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                },
            ]
        })
        .collect::<Vec<_>>();
    assert_eq!(diagnostics, expected);
}

#[derive(Default)]
struct ResolveCollector {
    drafts: Vec<AgentAssetProjectedDraft>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

impl AgentDiagnosticOutput for ResolveCollector {
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

impl AgentResolveOutput for ResolveCollector {
    fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
        self.drafts.push(value);
        ControlFlow::Continue(())
    }
}

/// End-to-end evidence for D2 tests. Keeping each stage makes failures
/// diagnosable: a projected row alone cannot tell whether a policy was
/// parsed, resolved, or rejected by the common projector.
#[derive(Debug)]
struct D2ProjectionTrace {
    sources: Vec<AgentAssetSourceSpec>,
    declarations: Vec<ParsedAgentAsset>,
    drafts: Vec<AgentAssetProjectedDraft>,
    records: Vec<AgentAssetRecord>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

fn run_d2_fixture(document: &[u8], trust: AgentTrustState) -> D2ProjectionTrace {
    let mut context = test_context();
    context.trust_context = trust;
    context.workspace_id =
        (trust != AgentTrustState::Trusted).then(|| "workspace-fixture".to_owned());
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Hook,
        AgentAssetCategory::Extension,
        AgentAssetCategory::Skill,
        AgentAssetCategory::StatusUi,
    ];
    let snapshot = AgentAssetSnapshot::File {
        bytes: document.to_vec(),
        revision: AgentAssetRevision::default(),
    };
    run_d2_sources(context, vec![(source, snapshot)])
}

fn run_d2_sources(
    context: crate::models::AgentConfigurationContext,
    inputs: Vec<(AgentAssetSourceSpec, AgentAssetSnapshot)>,
) -> D2ProjectionTrace {
    run_d2_sources_with_options(context, inputs, false, false, None, None)
}

fn run_d2_sources_with_options(
    context: crate::models::AgentConfigurationContext,
    inputs: Vec<(AgentAssetSourceSpec, AgentAssetSnapshot)>,
    reverse_sources: bool,
    reverse_declarations: bool,
    workspace_canonical: Option<&Path>,
    workspace_lexical: Option<&Path>,
) -> D2ProjectionTrace {
    let sources = inputs
        .iter()
        .map(|(source, _)| source.clone())
        .collect::<Vec<_>>();
    let snapshots = inputs
        .into_iter()
        .map(|(_, snapshot)| snapshot)
        .collect::<Vec<_>>();
    let mut source_order = (0..sources.len()).collect::<Vec<_>>();
    if reverse_sources {
        source_order.reverse();
    }
    let mut parsed = ParseCollector::default();
    for index in source_order.iter().copied() {
        let source = &sources[index];
        let snapshot = &snapshots[index];
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source,
                snapshot,
                workspace_canonical,
                workspace_lexical,
            },
            &mut parsed,
        );
    }
    if reverse_declarations {
        parsed.declarations.reverse();
    }
    let resolve_sources = source_order
        .iter()
        .copied()
        .map(|index| AgentAssetResolveSource {
            spec: &sources[index],
            snapshot: &snapshots[index],
        })
        .collect::<Vec<_>>();
    let mut resolved = ResolveCollector::default();
    resolve_assets(
        AgentAssetResolveRequest {
            context: &context,
            declarations: &parsed.declarations,
            sources: &resolve_sources,
        },
        &mut resolved,
    );
    let diagnostics = parsed
        .diagnostics
        .into_iter()
        .chain(resolved.diagnostics)
        .collect();
    project_trace(
        context,
        sources,
        parsed.declarations,
        resolved.drafts,
        diagnostics,
    )
}

fn project_trace(
    context: AgentConfigurationContext,
    sources: Vec<AgentAssetSourceSpec>,
    declarations: Vec<ParsedAgentAsset>,
    drafts: Vec<AgentAssetProjectedDraft>,
    mut diagnostics: Vec<AgentAssetDiagnostic>,
) -> D2ProjectionTrace {
    let public_source = |source: &AgentAssetSourceSpec| {
        let source_id =
            crate::services::agent_cli::environment::source_stable_id(&context, &source.path);
        (
            source_id.clone(),
            AgentAssetSource {
                origin: crate::models::AgentAssetInstallationOrigin::Unknown,
                id: source_id,
                context_id: context.id.clone(),
                label: source.label.clone(),
                scope: source.scope,
                environment_id: context.environment_id.clone(),
                workspace_id: context.workspace_id.clone(),
                path: source.path.to_string_lossy().into_owned(),
                allowed_root: source.allowed_root.to_string_lossy().into_owned(),
                precedence: source.precedence,
                writable: source.writable,
                sensitive: source.sensitive,
                source_kind: source.source_kind,
                categories: source.categories.clone(),
                revision: AgentAssetRevision::default(),
                diagnostics: Vec::new(),
                access: Default::default(),
                actions: Vec::new(),
            },
        )
    };
    let source_map = sources
        .iter()
        .map(public_source)
        .collect::<BTreeMap<_, _>>();
    let environment = AgentEnvironmentDescriptor {
        id: "native".to_owned(),
        kind: AgentEnvironmentKind::Native,
        host_platform: AgentHostPlatform::Macos,
        host_architecture: AgentHostArchitecture::Aarch64,
        guest_platform: None,
        display_name: "Native".to_owned(),
        capabilities: vec![AgentEnvironmentCapability::ReadOnlyInventory],
    };
    let mut run = crate::services::agent_cli::environment::run::AgentInventoryRun::with_clock(
        AgentAssetLimits::DEFAULT,
        std::sync::Arc::new(crate::services::agent_cli::environment::run::SystemMonotonicClock),
    );
    let projection = crate::services::agent_cli::environment::project_records(
        &environment,
        &context,
        &sources,
        &declarations,
        crate::services::agent_cli::environment::AgentAssetProjectionInput {
            drafts: drafts.clone(),
            state_assessor: assess_assets,
        },
        &source_map,
        &mut run,
    );
    diagnostics.extend(run.finish_diagnostics());
    D2ProjectionTrace {
        sources,
        declarations,
        drafts,
        records: projection.records,
        diagnostics,
    }
}

fn d2_source(
    key: &str,
    categories: Vec<AgentAssetCategory>,
    precedence: u32,
    path: &str,
) -> AgentAssetSourceSpec {
    let mut source = test_source(key, AgentAssetSourceKind::File);
    source.path = path.into();
    source.categories = categories;
    source.precedence = precedence;
    source.allowed_logical_origins[0].precedence = precedence;
    source
}

fn assert_no_projection_diagnostics(trace: &D2ProjectionTrace) {
    assert!(
        trace.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
                    | AgentAssetDiagnostic::UnresolvedRelationship { .. }
            )
        }),
        "projection diagnostics: {:?}",
        trace.diagnostics
    );
}

fn record<'a>(
    trace: &'a D2ProjectionTrace,
    category: AgentAssetCategory,
    native_id: &str,
) -> &'a AgentAssetRecord {
    trace
        .records
        .iter()
        .find(|asset| asset.category == category && asset.native_id == native_id)
        .unwrap_or_else(|| panic!("missing {category:?} asset {native_id}"))
}

fn run_d2_collision_fixture() -> D2ProjectionTrace {
    let context = test_context();
    let mut high_source = test_source("settings", AgentAssetSourceKind::File);
    high_source.path = "/tmp/gemini/high.json".into();
    high_source.categories = vec![AgentAssetCategory::Mcp];
    high_source.precedence = 20;
    high_source.allowed_logical_origins[0].precedence = 20;
    let mut low_source = test_source("system-defaults", AgentAssetSourceKind::File);
    low_source.path = "/tmp/gemini/low.json".into();
    low_source.categories = vec![AgentAssetCategory::Mcp];
    low_source.precedence = 10;
    low_source.allowed_logical_origins[0].precedence = 10;
    let high_snapshot = AgentAssetSnapshot::File {
            bytes: br#"{"mcpServers":{"Server":{"url":"https://high.invalid"},"server":{"url":"https://lower-case.invalid"}}}"#.to_vec(),
            revision: AgentAssetRevision::default(),
        };
    let low_snapshot = AgentAssetSnapshot::File {
        bytes: br#"{"mcpServers":{"Server":{"url":"https://low.invalid"}}}"#.to_vec(),
        revision: AgentAssetRevision::default(),
    };
    run_d2_sources(
        context,
        vec![(high_source, high_snapshot), (low_source, low_snapshot)],
    )
}

#[test]
fn gemini_d2_exact_replacement_precedes_normalized_collision() {
    let inventory = run_d2_collision_fixture();
    assert_eq!(inventory.records.len(), 3);
    assert!(inventory
        .records
        .iter()
        .all(|asset| asset.resolution.qualified_collision));
    let winner = inventory
        .records
        .iter()
        .find(|asset| {
            asset.native_id == "Server"
                && asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
        })
        .expect("canonical exact-key winner");
    let loser = inventory
        .records
        .iter()
        .find(|asset| {
            asset.native_id == "Server"
                && asset.resolution.relation == AgentAssetResolutionRelation::Replaced
        })
        .expect("qualified exact-key loser");
    let independent = inventory
        .records
        .iter()
        .find(|asset| asset.native_id == "server")
        .expect("case-distinct independent asset");
    assert_eq!(winner.resolution.contributor_ids.len(), 2);
    assert_eq!(winner.resolution.winner_id, Some(winner.stable_id.clone()));
    assert_eq!(loser.resolution.winner_id, Some(winner.stable_id.clone()));
    assert_eq!(
        independent.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert_eq!(winner.path.as_deref(), Some("/tmp/gemini/high.json"));
    assert_ne!(winner.stable_id, loser.stable_id);
    assert_no_projection_diagnostics(&inventory);
}

#[test]
fn gemini_d2_config_beats_extension_and_preserves_parent_evidence() {
    let context = test_context();
    let settings = d2_source(
        "settings",
        vec![AgentAssetCategory::Mcp],
        20,
        "/tmp/gemini/settings.json",
    );
    let extension = d2_source(
        "gemini-extension-manifest:alpha",
        vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp],
        10,
        "/tmp/gemini/extensions/alpha/gemini-extension.json",
    );
    let primary_inputs = vec![
            (settings, AgentAssetSnapshot::File { bytes: br#"{"mcpServers":{"shared":{"httpUrl":"https://settings.invalid"}}}"#.to_vec(), revision: Default::default() }),
            (extension, AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"shared":{"command":"extension-runner"}}}"#.to_vec(), revision: Default::default() }),
        ];
    let trace = run_d2_sources_with_options(
        context.clone(),
        primary_inputs.clone(),
        false,
        false,
        None,
        None,
    );
    let winner = trace
        .records
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.native_id == "shared"
                && asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
        })
        .expect("canonical shared winner");
    assert_eq!(
        winner.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert!(matches!(
        winner.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http,
            ..
        }
    ));
    let loser = trace
        .records
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.resolution.relation == AgentAssetResolutionRelation::Replaced
        })
        .expect("extension loser");
    assert_eq!(loser.native_id, "shared");
    let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
    assert_eq!(
        loser.relationships.provided_by.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(
        loser.relationships.action_owner.as_deref(),
        Some(parent.stable_id.as_str())
    );
    assert_eq!(winner.represented_declaration_ids.len(), 2);
    assert_eq!(winner.resolution.winner_id, Some(winner.stable_id.clone()));
    assert_eq!(trace.sources.len(), 2);
    assert_eq!(
        trace
            .declarations
            .iter()
            .filter(|value| value.native_id == "shared")
            .count(),
        2
    );
    assert!(trace.drafts.iter().any(|draft| draft.native_id == "shared"
        && draft.resolution.relation == AgentAssetResolutionRelation::Replaced));
    assert_no_projection_diagnostics(&trace);
    let reversed =
        run_d2_sources_with_options(context.clone(), primary_inputs, true, true, None, None);
    assert_eq!(
        format!(
            "{:?}|{:?}|{:?}",
            trace.drafts, trace.records, trace.diagnostics
        ),
        format!(
            "{:?}|{:?}|{:?}",
            reversed.drafts, reversed.records, reversed.diagnostics
        )
    );
    assert_no_projection_diagnostics(&reversed);

    let ambiguous = run_d2_sources(
            context,
            vec![
                (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 10, "/tmp/gemini/extensions/alpha.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"shared":{"command":"alpha"}}}"#.to_vec(), revision: Default::default() }),
                (d2_source("gemini-extension-manifest:beta", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 10, "/tmp/gemini/extensions/beta.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"beta","version":"1.0.0","mcpServers":{"shared":{"command":"beta"}}}"#.to_vec(), revision: Default::default() }),
            ],
        );
    let ambiguous_shared = record(&ambiguous, AgentAssetCategory::Mcp, "shared");
    assert_eq!(
        ambiguous_shared.resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        ambiguous_shared.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(ambiguous_shared.resolution.winner_id.is_none());
}

#[test]
fn gemini_local_admin_neither_rewrites_nor_injects_mcp() {
    let trace = run_d2_fixture(
        br#"{"admin":{"mcp":{"enabled":false,"config":{"ordinary":{"command":"ignored-rewrite"}},"requiredConfig":{"phantom":{"command":"ignored-required"}}}},"mcpServers":{"ordinary":{"url":"https://ordinary.invalid"}}}"#,
        AgentTrustState::Trusted,
    );
    assert_eq!(trace.records.len(), 1, "{:?}", trace.diagnostics);
    assert_eq!(trace.declarations.len(), 1);
    let ordinary = record(&trace, AgentAssetCategory::Mcp, "ordinary");
    assert_eq!(ordinary.declared_state, AgentAssetState::Enabled);
    assert_eq!(ordinary.effective_state, AgentAssetState::Enabled);
    assert_eq!(
        ordinary.resolution.relation,
        AgentAssetResolutionRelation::Independent
    );
    assert!(ordinary.resolution.terminal.is_none());
    assert!(ordinary.resolution.control_source.is_none());
    assert!(matches!(
        ordinary.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Sse,
            effective_availability: AgentAssetEffectiveAvailability::Available,
            ..
        }
    ));
    assert_no_projection_diagnostics(&trace);
}

#[test]
fn gemini_d2_allowed_intersection_excluded_union_and_alias() {
    let context = test_context();
    let settings = d2_source(
        "settings",
        vec![AgentAssetCategory::Mcp],
        20,
        "/tmp/gemini/settings.json",
    );
    let system = d2_source(
        "system-defaults",
        vec![AgentAssetCategory::Mcp],
        10,
        "/tmp/gemini/system.json",
    );
    let trace = run_d2_sources(
            context,
            vec![
                (settings, AgentAssetSnapshot::File { bytes: br#"{"mcp":{"allowed":["allowed","ext:alpha:child"],"excluded":["blocked"]},"mcpServers":{"allowed":{"url":"https://allowed.invalid"},"other":{"url":"https://other.invalid"},"blocked":{"url":"https://blocked.invalid"}}}"#.to_vec(), revision: Default::default() }),
                (system, AgentAssetSnapshot::File { bytes: br#"{"mcp":{"allowed":["allowed","other"],"excluded":["other"]}}"#.to_vec(), revision: Default::default() }),
            ],
        );
    assert_eq!(trace.records.len(), 3);
    let allowed = record(&trace, AgentAssetCategory::Mcp, "allowed");
    assert_eq!(allowed.effective_state, AgentAssetState::Enabled);
    assert_eq!(
        record(&trace, AgentAssetCategory::Mcp, "other").effective_state,
        AgentAssetState::Blocked
    );
    assert_eq!(
        record(&trace, AgentAssetCategory::Mcp, "blocked").effective_state,
        AgentAssetState::Blocked
    );
    assert_eq!(
        record(&trace, AgentAssetCategory::Mcp, "other")
            .resolution
            .terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    assert!(record(&trace, AgentAssetCategory::Mcp, "other")
        .resolution
        .control_source
        .is_none());
    assert_no_projection_diagnostics(&trace);

    let invalid = run_d2_fixture(br#"{"mcp":{"allowed":"bad","excluded":[false]},"mcpServers":{"candidate":{"url":"https://candidate.invalid"}}}"#, AgentTrustState::Trusted);
    assert_eq!(invalid.records.len(), 1);
    let candidate = record(&invalid, AgentAssetCategory::Mcp, "candidate");
    assert_eq!(candidate.declared_state, AgentAssetState::Enabled);
    assert_eq!(candidate.effective_state, AgentAssetState::Unknown);
    assert_eq!(
        candidate.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(candidate.resolution.control_source.is_none());
}

#[test]
fn gemini_d2_trust_is_terminal_and_suppressed_never_contributes() {
    let inventory = run_d2_fixture(
        br#"{"mcpServers":{"trusted":{"url":"https://trusted.invalid"}}}"#,
        AgentTrustState::Untrusted,
    );
    let trusted = inventory
        .records
        .iter()
        .find(|asset| asset.native_id == "trusted")
        .expect("suppressed MCP remains visible as inventory evidence");
    assert_eq!(
        trusted.resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        trusted.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    assert!(trusted.resolution.contributor_ids.is_empty());
    assert_eq!(trusted.declared_state, AgentAssetState::Unknown);
    assert_eq!(trusted.effective_state, AgentAssetState::Unknown);
    assert_eq!(trusted.trust_state, AgentTrustState::Untrusted);
    assert!(matches!(
        trusted.details,
        AgentAssetDetails::Mcp {
            declared_state: AgentAssetDeclaredState::Unknown,
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
            ..
        }
    ));
    assert_no_projection_diagnostics(&inventory);
}

#[test]
fn gemini_d2_persistent_enablement_defaults_and_collisions() {
    let settings = |document: &[u8]| {
        (
            d2_source(
                "settings",
                vec![AgentAssetCategory::Mcp],
                20,
                "/tmp/gemini/settings.json",
            ),
            AgentAssetSnapshot::File {
                bytes: document.to_vec(),
                revision: Default::default(),
            },
        )
    };
    let overlay = |document: &[u8]| {
        (
            d2_source(
                "mcp-enablement",
                vec![AgentAssetCategory::Mcp],
                10,
                "/tmp/gemini/mcp-enablement.json",
            ),
            AgentAssetSnapshot::File {
                bytes: document.to_vec(),
                revision: Default::default(),
            },
        )
    };
    let definition = br#"{"mcpServers":{"server":{"url":"https://server.invalid"}}}"#;
    let cases = [
        (br#"{}"#.as_slice(), AgentAssetState::Enabled, None),
        (
            br#"{"server":{"enabled":true}}"#.as_slice(),
            AgentAssetState::Enabled,
            None,
        ),
        (
            br#"{"server":{"enabled":false}}"#.as_slice(),
            AgentAssetState::Disabled,
            None,
        ),
        (
            br#"{"server":{"enabled":"bad"}}"#.as_slice(),
            AgentAssetState::Unknown,
            None,
        ),
    ];
    for (document, expected, terminal) in cases {
        let trace = run_d2_sources(
            test_context(),
            vec![settings(definition), overlay(document)],
        );
        let server = record(&trace, AgentAssetCategory::Mcp, "server");
        assert_eq!(server.declared_state, expected, "enablement={document:?}");
        assert_eq!(
            server.resolution.terminal, terminal,
            "enablement={document:?}"
        );
    }

    let collision = run_d2_sources(
            test_context(),
            vec![
                settings(br#"{"mcpServers":{"Server":{"url":"https://upper.invalid"},"server":{"url":"https://lower.invalid"}}}"#),
                overlay(br#"{"SERVER":{"enabled":false}}"#),
            ],
        );
    for native_id in ["Server", "server"] {
        let asset = record(&collision, AgentAssetCategory::Mcp, native_id);
        assert_eq!(asset.declared_state, AgentAssetState::Disabled);
        assert!(asset.resolution.qualified_collision);
    }

    let blocked_snapshot = AgentAssetSnapshot::Blocked {
        revision: Default::default(),
        diagnostic: AgentAssetDiagnostic::ReadFailed {
            source_id: "mcp-enablement".to_owned(),
            error_kind: AgentAssetIoErrorKind::PermissionDenied,
        },
    };
    let root_malformed = AgentAssetSnapshot::File {
        bytes: br#"["#.to_vec(),
        revision: Default::default(),
    };
    for snapshot in [blocked_snapshot, root_malformed] {
        let trace = run_d2_sources(
            test_context(),
            vec![
                settings(definition),
                (
                    d2_source(
                        "mcp-enablement",
                        vec![AgentAssetCategory::Mcp],
                        10,
                        "/tmp/gemini/mcp-enablement.json",
                    ),
                    snapshot,
                ),
            ],
        );
        let server = record(&trace, AgentAssetCategory::Mcp, "server");
        assert_eq!(server.declared_state, AgentAssetState::Enabled);
        assert_eq!(
            server.resolution.terminal,
            Some(AgentAssetResolutionTerminal::Unknown)
        );
        assert!(server.resolution.control_source.is_some());
    }

    let orphan = run_d2_sources(
        test_context(),
        vec![
            settings(br#"{}"#),
            overlay(br#"{"orphan":{"enabled":false}}"#),
        ],
    );
    assert!(!orphan
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Mcp));
    assert!(orphan
        .declarations
        .iter()
        .any(|declaration| declaration.native_id == "orphan"));

    for (overlay_document, expected_declared) in [
        (
            br#"{"server":{"enabled":true}}"#.as_slice(),
            AgentAssetState::Enabled,
        ),
        (
            br#"{"server":{"enabled":false}}"#.as_slice(),
            AgentAssetState::Disabled,
        ),
        (
            br#"{"server":{"enabled":"unknown"}}"#.as_slice(),
            AgentAssetState::Unknown,
        ),
    ] {
        let trace = run_d2_sources(test_context(), vec![
                settings(br#"{"mcp":{"allowed":[]},"mcpServers":{"server":{"url":"https://server.invalid"}}}"#),
                overlay(overlay_document),
            ]);
        let server = record(&trace, AgentAssetCategory::Mcp, "server");
        assert_eq!(server.declared_state, expected_declared);
        assert_eq!(
            server.resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_eq!(server.effective_state, AgentAssetState::Blocked);
        assert!(server.resolution.control_source.is_some());
    }
}

#[test]
fn gemini_d2_extension_parent_state_propagates_to_all_children() {
    let context = test_context();
    let trace = run_d2_sources(
            context,
            vec![
                (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 20, "/tmp/gemini/alpha.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"child"}}}"#.to_vec(), revision: Default::default() }),
                (d2_source("gemini-extension-hooks:alpha", vec![AgentAssetCategory::Hook], 20, "/tmp/gemini/alpha-hooks.json"), AgentAssetSnapshot::File { bytes: br#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"child-hook"}]}]}}"#.to_vec(), revision: Default::default() }),
                (d2_source("gemini-extension-skill:alpha:writer", vec![AgentAssetCategory::Skill], 20, "/tmp/gemini/alpha-skill.md"), AgentAssetSnapshot::File { bytes: b"---\nname: writer\ndescription: writer\n---\n".to_vec(), revision: Default::default() }),
            ],
        );
    assert_eq!(trace.records.len(), 4);
    let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
    assert_eq!(parent.effective_state, AgentAssetState::Enabled);
    for child in trace
        .records
        .iter()
        .filter(|asset| asset.relationships.provided_by.is_some())
    {
        assert_eq!(
            child.relationships.provided_by.as_deref(),
            Some(parent.stable_id.as_str())
        );
        assert_eq!(
            child.relationships.action_owner.as_deref(),
            Some(parent.stable_id.as_str())
        );
    }
    assert!(trace
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == "child"));
    assert!(trace
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Hook));
    assert!(trace
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Skill));
    assert_no_projection_diagnostics(&trace);

    let missing = run_d2_sources(
        test_context(),
        vec![(
            d2_source(
                "gemini-extension-hooks:missing",
                vec![AgentAssetCategory::Hook],
                20,
                "/tmp/gemini/missing-hooks.json",
            ),
            AgentAssetSnapshot::File {
                bytes:
                    br#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"orphan"}]}]}}"#
                        .to_vec(),
                revision: Default::default(),
            },
        )],
    );
    assert!(!missing
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Hook));
    assert!(missing
        .declarations
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::Hook));

    let extension_trace = |enablement: AgentAssetSnapshot| {
        let context = AgentConfigurationContext {
            workspace_id: Some("workspace".to_owned()),
            trust_context: AgentTrustState::Trusted,
            ..test_context()
        };
        run_d2_sources_with_options(
                context,
                vec![
                    (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 20, "/tmp/gemini/alpha.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"child"}}}"#.to_vec(), revision: Default::default() }),
                    (d2_source("gemini-extension-hooks:alpha", vec![AgentAssetCategory::Hook], 20, "/tmp/gemini/alpha-hooks.json"), AgentAssetSnapshot::File { bytes: br#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"child-hook"}]}]}}"#.to_vec(), revision: Default::default() }),
                    (d2_source("gemini-extension-skill:alpha:writer", vec![AgentAssetCategory::Skill], 20, "/tmp/gemini/alpha-skill.md"), AgentAssetSnapshot::File { bytes: b"---\nname: writer\ndescription: writer\n---\n".to_vec(), revision: Default::default() }),
                    (d2_source("extension-enablement", vec![AgentAssetCategory::Extension], 30, "/tmp/gemini/extension-enablement.json"), enablement),
                ],
                false,
                false,
                Some(Path::new("/tmp/workspace/project")),
                None,
            )
    };
    for (enablement, expected_state) in [
        (br#"{}"#.to_vec(), AgentAssetState::Enabled),
        (
            br#"{"alpha":{"overrides":[]}}"#.to_vec(),
            AgentAssetState::Enabled,
        ),
        (
            br#"{"alpha":{"overrides":["!/tmp/workspace/*"]}}"#.to_vec(),
            AgentAssetState::Disabled,
        ),
        (br#"{"alpha":false}"#.to_vec(), AgentAssetState::Unknown),
    ] {
        let trace = extension_trace(AgentAssetSnapshot::File {
            bytes: enablement,
            revision: Default::default(),
        });
        let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
        assert_eq!(parent.effective_state, expected_state);
        assert!(trace
            .records
            .iter()
            .any(|asset| asset.category == AgentAssetCategory::Mcp && asset.native_id == "child"));
        assert!(trace
            .records
            .iter()
            .any(|asset| asset.category == AgentAssetCategory::Hook));
        assert!(trace
            .records
            .iter()
            .any(|asset| asset.category == AgentAssetCategory::Skill));
    }
    for enablement in [
        AgentAssetSnapshot::Blocked {
            revision: Default::default(),
            diagnostic: AgentAssetDiagnostic::ReadFailed {
                source_id: "extension-enablement".to_owned(),
                error_kind: AgentAssetIoErrorKind::PermissionDenied,
            },
        },
        AgentAssetSnapshot::File {
            bytes: br#"["#.to_vec(),
            revision: Default::default(),
        },
    ] {
        let trace = extension_trace(enablement);
        let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
        assert_eq!(parent.effective_state, AgentAssetState::Unknown);
        assert!(parent.resolution.terminal.is_some());
        assert!(trace
            .declarations
            .iter()
            .any(|declaration| declaration.native_id == "child"));
    }

    // Duplicate source identities are a distinct failure from valid native
    // parent ambiguity, which is covered by the independent-source proof test.
    let duplicate_ids = run_d2_sources(
            AgentConfigurationContext { trust_context: AgentTrustState::Trusted, ..test_context() },
            vec![
                (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 20, "/tmp/gemini/alpha-one.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"one"}}}"#.to_vec(), revision: Default::default() }),
                (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 20, "/tmp/gemini/alpha-two.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"two"}}}"#.to_vec(), revision: Default::default() }),
            ],
        );
    assert!(duplicate_ids.records.is_empty());
    assert_eq!(duplicate_ids.declarations.len(), 4);
    assert_eq!(
        duplicate_ids
            .declarations
            .iter()
            .filter(|declaration| declaration.category == AgentAssetCategory::Mcp)
            .count(),
        2
    );
    assert!(duplicate_ids.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidProjection { .. }
            | AgentAssetDiagnostic::InvalidResolution { .. }
    )));
}

#[test]
fn gemini_d2_hook_disabled_union_invalid_and_additive_slots() {
    let trace = run_d2_fixture(br#"{"hooksConfig":{"disabled":["named"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"named"},{"type":"runtime","name":"other"}]}]}}"#, AgentTrustState::Trusted);
    let hooks = trace
        .records
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 2);
    assert_eq!(
        hooks
            .iter()
            .find(|asset| asset.native_id == "BeforeTool:0:0")
            .unwrap()
            .declared_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        hooks
            .iter()
            .find(|asset| asset.native_id == "BeforeTool:0:1")
            .unwrap()
            .declared_state,
        AgentAssetState::Enabled
    );
    assert_no_projection_diagnostics(&trace);

    let mut valid_source = test_source("settings", AgentAssetSourceKind::File);
    valid_source.categories = vec![AgentAssetCategory::Hook];
    valid_source.path = "/tmp/gemini/hook-settings.json".into();
    valid_source.precedence = 20;
    valid_source.allowed_logical_origins[0].precedence = 20;
    let mut invalid_source = test_source("system-defaults", AgentAssetSourceKind::File);
    invalid_source.categories = vec![AgentAssetCategory::Hook];
    invalid_source.path = "/tmp/gemini/hook-policy.json".into();
    invalid_source.scope = AgentAssetScope::System;
    invalid_source.allowed_logical_origins[0].scope = AgentAssetScope::System;
    invalid_source.precedence = 10;
    invalid_source.allowed_logical_origins[0].precedence = 10;
    let invalid = run_d2_sources(
            test_context(),
            vec![
                (
                    valid_source,
                    AgentAssetSnapshot::File {
                        bytes: br#"{"hooksConfig":{"disabled":["named"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"named"},{"type":"runtime","name":"other"}]}]}}"#.to_vec(),
                        revision: Default::default(),
                    },
                ),
                (
                    invalid_source,
                    AgentAssetSnapshot::File {
                        bytes: br#"{"hooksConfig":{"disabled":["named",false]}}"#.to_vec(),
                        revision: Default::default(),
                    },
                ),
            ],
        );
    assert_no_projection_diagnostics(&invalid);
    assert_eq!(invalid.records.len(), 2);
    assert_eq!(invalid.declarations.len(), 4);
    assert_eq!(
        record(&invalid, AgentAssetCategory::Hook, "BeforeTool:0:0").declared_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        record(&invalid, AgentAssetCategory::Hook, "BeforeTool:0:0").effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        record(&invalid, AgentAssetCategory::Hook, "BeforeTool:0:1").declared_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        record(&invalid, AgentAssetCategory::Hook, "BeforeTool:0:1").effective_state,
        AgentAssetState::Unknown
    );
    let invalid_policy = invalid
        .declarations
        .iter()
        .find(|declaration| {
            declaration.source_key == "system-defaults"
                && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
                && matches!(
                    declaration.native_payload,
                    AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::InvalidDisabledSet)
                )
        })
        .unwrap();
    assert_eq!(
        record(&invalid, AgentAssetCategory::Hook, "BeforeTool:0:1")
            .resolution
            .control_source,
        Some(AgentAssetPolicyReference::Declaration {
            declaration_id: invalid_policy.declaration_id.clone(),
        })
    );
}

#[test]
fn gemini_d2_status_and_non_extension_assets_keep_precedence() {
    let valid = run_d2_sources(
        test_context(),
        vec![
            (
                d2_source(
                    "settings",
                    vec![AgentAssetCategory::StatusUi],
                    20,
                    "/tmp/gemini/settings.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"{"footer":"high-status"}"#.to_vec(),
                    revision: Default::default(),
                },
            ),
            (
                d2_source(
                    "system-defaults",
                    vec![AgentAssetCategory::StatusUi],
                    10,
                    "/tmp/gemini/system.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"{"footer":"low-status"}"#.to_vec(),
                    revision: Default::default(),
                },
            ),
        ],
    );
    let status = valid
        .records
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::StatusUi
                && asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
        })
        .expect("status winner");
    assert_eq!(status.path.as_deref(), Some("/tmp/gemini/settings.json"));
    let status_loser = valid
        .records
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::StatusUi
                && asset.resolution.relation == AgentAssetResolutionRelation::Replaced
        })
        .expect("lower status evidence");
    assert_eq!(status_loser.native_id, "footer");
    assert_eq!(status.resolution.contributor_ids.len(), 2);
    assert_eq!(status.represented_declaration_ids.len(), 2);
    assert_no_projection_diagnostics(&valid);

    let malformed_high = run_d2_sources(
        test_context(),
        vec![
            (
                d2_source(
                    "settings",
                    vec![AgentAssetCategory::StatusUi],
                    20,
                    "/tmp/gemini/settings.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"["#.to_vec(),
                    revision: Default::default(),
                },
            ),
            (
                d2_source(
                    "system-defaults",
                    vec![AgentAssetCategory::StatusUi],
                    10,
                    "/tmp/gemini/system.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"{"footer":"low-status"}"#.to_vec(),
                    revision: Default::default(),
                },
            ),
        ],
    );
    let unknown_status = record(&malformed_high, AgentAssetCategory::StatusUi, "footer");
    assert_eq!(
        unknown_status.path.as_deref(),
        Some("/tmp/gemini/settings.json")
    );
    assert_eq!(unknown_status.declared_state, AgentAssetState::Unknown);
    assert_eq!(malformed_high.records.len(), 2);
    assert_eq!(
        unknown_status.resolution.relation,
        AgentAssetResolutionRelation::ReplaceWinner
    );
    assert!(malformed_high
        .records
        .iter()
        .any(
            |asset| asset.path.as_deref() == Some("/tmp/gemini/system.json")
                && asset.category == AgentAssetCategory::StatusUi
                && asset.effective_state == AgentAssetState::Shadowed
        ));
    assert_no_projection_diagnostics(&malformed_high);

    let ordinary_and_extension = run_d2_sources(
        test_context(),
        vec![
            (
                d2_source(
                    "gemini-skill-manifest:skills:root",
                    vec![AgentAssetCategory::Skill],
                    20,
                    "/tmp/gemini/skills/root.md",
                ),
                AgentAssetSnapshot::File {
                    bytes: b"---\nname: root\ndescription: root\n---\n".to_vec(),
                    revision: Default::default(),
                },
            ),
            (
                d2_source(
                    "gemini-extension-manifest:alpha",
                    vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp],
                    10,
                    "/tmp/gemini/alpha.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"{"name":"alpha","version":"1.0.0"}"#.to_vec(),
                    revision: Default::default(),
                },
            ),
            (
                d2_source(
                    "gemini-extension-skill:alpha:writer",
                    vec![AgentAssetCategory::Skill],
                    10,
                    "/tmp/gemini/alpha-writer.md",
                ),
                AgentAssetSnapshot::File {
                    bytes: b"---\nname: writer\ndescription: writer\n---\n".to_vec(),
                    revision: Default::default(),
                },
            ),
        ],
    );
    let root = record(&ordinary_and_extension, AgentAssetCategory::Skill, "root");
    let extension_skill = record(&ordinary_and_extension, AgentAssetCategory::Skill, "writer");
    assert_eq!(root.relationships.provided_by, None);
    assert!(extension_skill.relationships.provided_by.is_some());
    assert!(
        ordinary_and_extension
            .records
            .iter()
            .any(|asset| asset.category == AgentAssetCategory::Extension
                && asset.native_id == "alpha")
    );

    let suppressed = run_d2_sources(
        AgentConfigurationContext {
            workspace_id: Some("workspace".to_owned()),
            trust_context: AgentTrustState::Untrusted,
            ..test_context()
        },
        vec![(
            d2_source(
                "workspace-settings",
                vec![AgentAssetCategory::StatusUi],
                20,
                "/tmp/gemini/workspace-settings.json",
            ),
            AgentAssetSnapshot::File {
                bytes: br#"{"footer":"suppressed"}"#.to_vec(),
                revision: Default::default(),
            },
        )],
    );
    assert!(!suppressed
        .records
        .iter()
        .any(|asset| asset.category == AgentAssetCategory::StatusUi));
    assert!(suppressed.declarations.iter().any(|declaration| matches!(
        declaration.participation,
        AgentAssetResolutionParticipation::Suppressed { .. }
    )));
    assert!(suppressed.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::DeclarationSuppressed { .. }
    )));
}

#[test]
fn gemini_d2_declaration_only_guard() {
    let inventory = run_d2_fixture(
        br#"{"mcpServers":{"server":{"url":"https://server.invalid"}}}"#,
        AgentTrustState::Trusted,
    );
    assert!(inventory.records.iter().all(|asset| asset.path.is_some()));
    assert!(!inventory.sources.is_empty());
    assert!(!inventory.declarations.is_empty());
    assert!(!inventory.drafts.is_empty());
    let resolver_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/services/agent_cli/gemini/environment");
    let split_root = resolver_root.join("resolve");
    let resolver_paths = if split_root.is_dir() {
        fs::read_dir(split_root)
            .expect("read split resolver directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|value| value.to_str()) == Some("rs")
                    && path.file_name().and_then(|value| value.to_str()) != Some("tests.rs")
            })
            .collect::<Vec<_>>()
    } else {
        vec![resolver_root.join("resolve.rs")]
    };
    assert!(!resolver_paths.is_empty());
    for path in resolver_paths {
        let source = fs::read_to_string(&path).expect("read resolver source");
        for forbidden in [
            "AgentAssetSnapshot",
            "serde_json",
            "std::fs",
            "std::process",
            "reqwest",
            ".facts",
        ] {
            assert!(
                !source.contains(forbidden),
                "resolver reads forbidden runtime input {forbidden}: {path:?}"
            );
        }
        assert!(
            !source.contains("fn qualified_projection_key"),
            "resolver owns a second projection-key formatter: {path:?}"
        );
        assert!(
            !source.contains("@{}"),
            "resolver manually formats qualified projection keys: {path:?}"
        );
    }
}

#[test]
fn gemini_d2_reversal_is_stable_for_every_stage() {
    let compound = || {
        vec![
            (d2_source("settings", vec![AgentAssetCategory::Mcp, AgentAssetCategory::Hook, AgentAssetCategory::StatusUi], 20, "/tmp/gemini/settings.json"), AgentAssetSnapshot::File { bytes: br#"{"mcp":{"allowed":["Server"]},"mcpServers":{"Server":{"url":"https://high.invalid"}},"hooksConfig":{"disabled":["named"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"named"}]}]},"footer":"status"}"#.to_vec(), revision: Default::default() }),
            (d2_source("system-defaults", vec![AgentAssetCategory::Mcp, AgentAssetCategory::Hook, AgentAssetCategory::StatusUi], 10, "/tmp/gemini/system.json"), AgentAssetSnapshot::File { bytes: br#"{"mcpServers":{"server":{"url":"https://low.invalid"}},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"other"}]}]},"footer":"lower"}"#.to_vec(), revision: Default::default() }),
            (d2_source("mcp-enablement", vec![AgentAssetCategory::Mcp], 30, "/tmp/gemini/mcp.json"), AgentAssetSnapshot::File { bytes: br#"{"server":{"enabled":true}}"#.to_vec(), revision: Default::default() }),
            (d2_source("gemini-extension-manifest:alpha", vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp], 15, "/tmp/gemini/alpha.json"), AgentAssetSnapshot::File { bytes: br#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"child"}}}"#.to_vec(), revision: Default::default() }),
            (d2_source("gemini-extension-hooks:alpha", vec![AgentAssetCategory::Hook], 15, "/tmp/gemini/alpha-hooks.json"), AgentAssetSnapshot::File { bytes: br#"{"hooks":{"AfterTool":[{"hooks":[{"type":"runtime","name":"extension-hook"}]}]}}"#.to_vec(), revision: Default::default() }),
            (d2_source("gemini-extension-skill:alpha:writer", vec![AgentAssetCategory::Skill], 15, "/tmp/gemini/alpha-skill.md"), AgentAssetSnapshot::File { bytes: b"---\nname: writer\ndescription: writer\n---\n".to_vec(), revision: Default::default() }),
        ]
    };
    let first = run_d2_sources_with_options(test_context(), compound(), false, false, None, None);
    let second = run_d2_sources_with_options(test_context(), compound(), true, true, None, None);
    for trace in [&first, &second] {
        assert_eq!(trace.records.len(), 10);
        for (category, expected) in [
            (AgentAssetCategory::Mcp, 3),
            (AgentAssetCategory::Extension, 1),
            (AgentAssetCategory::Hook, 3),
            (AgentAssetCategory::Skill, 1),
            (AgentAssetCategory::StatusUi, 2),
            (AgentAssetCategory::Plugin, 0),
        ] {
            assert_eq!(
                trace
                    .records
                    .iter()
                    .filter(|asset| asset.category == category)
                    .count(),
                expected,
                "category {category:?}"
            );
        }
        let parent = record(trace, AgentAssetCategory::Extension, "alpha");
        assert_eq!(parent.effective_state, AgentAssetState::Enabled);
        for native_id in ["Server", "server"] {
            let mcp = record(trace, AgentAssetCategory::Mcp, native_id);
            assert_eq!(mcp.declared_state, AgentAssetState::Enabled);
            assert_eq!(mcp.effective_state, AgentAssetState::Enabled);
            assert!(mcp.resolution.qualified_collision);
        }
        let child = record(trace, AgentAssetCategory::Mcp, "child");
        assert_eq!(child.effective_state, AgentAssetState::Blocked);
        assert_eq!(
            child.relationships.provided_by.as_deref(),
            Some(parent.stable_id.as_str())
        );
        assert_eq!(
            child.relationships.action_owner.as_deref(),
            Some(parent.stable_id.as_str())
        );
        let additive = trace
            .records
            .iter()
            .filter(|asset| {
                asset.category == AgentAssetCategory::Hook && asset.native_id == "BeforeTool:0:0"
            })
            .collect::<Vec<_>>();
        assert_eq!(additive.len(), 2);
        assert!(additive
            .iter()
            .all(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Additive));
        assert_eq!(
            additive
                .iter()
                .filter(|asset| asset.declared_state == AgentAssetState::Disabled
                    && asset.effective_state == AgentAssetState::Disabled)
                .count(),
            1
        );
        assert_eq!(
            additive
                .iter()
                .filter(|asset| asset.declared_state == AgentAssetState::Enabled
                    && asset.effective_state == AgentAssetState::Enabled)
                .count(),
            1
        );
        for (category, id) in [
            (AgentAssetCategory::Hook, "alpha:AfterTool:0:0"),
            (AgentAssetCategory::Skill, "writer"),
        ] {
            let child = record(trace, category, id);
            assert_eq!(child.effective_state, AgentAssetState::Enabled);
            assert_eq!(
                child.relationships.provided_by.as_deref(),
                Some(parent.stable_id.as_str())
            );
        }
        assert_eq!(
            trace
                .records
                .iter()
                .filter(|asset| asset.category == AgentAssetCategory::StatusUi
                    && asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
                .count(),
            1
        );
        let loser = trace
            .drafts
            .iter()
            .find(|draft| {
                draft.native_kind == AgentAssetCategory::StatusUi
                    && draft.resolution.relation == AgentAssetResolutionRelation::Replaced
            })
            .unwrap();
        assert!(
            matches!(&loser.state_proof.effective, AgentAssetEffectiveStateProofDraft::Shadowed { input, .. } if **input == AgentAssetEffectiveStateProofDraft::Intrinsic)
        );
        assert_no_projection_diagnostics(trace);
    }
    let signature = |trace: &D2ProjectionTrace| {
        let mut sources = trace.sources.clone();
        sources.sort_by(|left, right| left.native_source_key.cmp(&right.native_source_key));
        let mut declarations = trace.declarations.clone();
        declarations.sort_by(|left, right| left.declaration_id.cmp(&right.declaration_id));
        let mut drafts = trace.drafts.clone();
        drafts.sort_by(|left, right| left.projection_key.cmp(&right.projection_key));
        let mut records = trace.records.clone();
        records.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
        let mut diagnostics = trace.diagnostics.clone();
        diagnostics.sort_by_key(|value| format!("{value:?}"));
        format!("sources={sources:?};declarations={declarations:?};drafts={drafts:?};records={records:?};diagnostics={diagnostics:?}")
    };
    assert_eq!(signature(&first), signature(&second));
}

#[test]
fn gemini_d2_break_is_terminal() {
    #[derive(Default)]
    struct BreakCollector {
        drafts: Vec<AgentAssetProjectedDraft>,
        diagnostics: Vec<AgentAssetDiagnostic>,
    }
    impl AgentDiagnosticOutput for BreakCollector {
        fn has_regular_capacity(&self) -> bool {
            true
        }
        fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            self.diagnostics.push(value);
            AgentDiagnosticEmission::Accepted
        }
    }
    impl AgentResolveOutput for BreakCollector {
        fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
            self.drafts.push(value);
            ControlFlow::Break(AgentOutputStop::EntryLimit)
        }
    }
    let break_once = |reverse: bool| {
        let context = test_context();
        let source = d2_source(
            "settings",
            vec![AgentAssetCategory::Mcp],
            10,
            "/tmp/gemini/settings.json",
        );
        let snapshot = AgentAssetSnapshot::File { bytes: br#"{"mcpServers":{"first":{"url":"https://first.invalid"},"second":{"url":"https://second.invalid"},"third":{"url":"https://third.invalid"}}}"#.to_vec(), revision: Default::default() };
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        if reverse {
            parsed.declarations.reverse();
        }
        let resolve_source = AgentAssetResolveSource {
            spec: &source,
            snapshot: &snapshot,
        };
        let mut output = BreakCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.declarations,
                sources: &[resolve_source],
            },
            &mut output,
        );
        output
    };
    let first = break_once(false);
    let second = break_once(true);
    assert_eq!(first.drafts.len(), 1);
    assert_eq!(second.drafts.len(), 1);
    assert_eq!(first.drafts[0].native_id, "first");
    assert_eq!(
        format!("{:?}", first.drafts[0]),
        format!("{:?}", second.drafts[0])
    );
    assert!(first.diagnostics.is_empty());
    assert!(second.diagnostics.is_empty());
}

fn test_context() -> crate::models::AgentConfigurationContext {
    crate::models::AgentConfigurationContext {
        id: "gemini-context".to_owned(),
        environment_id: "native".to_owned(),
        agent_kind: crate::models::AgentCliKind::Gemini,
        config_root: "/tmp/gemini".to_owned(),
        profile: "default".to_owned(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: 1,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    }
}

fn test_source(
    native_source_key: &str,
    source_kind: AgentAssetSourceKind,
) -> crate::services::agent_cli::contracts::AgentAssetSourceSpec {
    crate::services::agent_cli::contracts::AgentAssetSourceSpec {
        verified_physical_path: None,
        hook_definition_source: true,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: native_source_key.to_owned(),
        label: native_source_key.to_owned(),
        scope: AgentAssetScope::User,
        path: "/tmp/gemini/source".into(),
        allowed_root: "/tmp/gemini".into(),
        precedence: if native_source_key == EXTENSIONS_SOURCE_KEY {
            0
        } else {
            10
        },
        writable: true,
        sensitive: false,
        source_kind,
        categories: vec![AgentAssetCategory::Extension],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: if native_source_key == EXTENSIONS_SOURCE_KEY {
                    0
                } else {
                    10
                },
            },
        ],
    }
}

fn inventory_test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-gemini-inventory-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    ))
}

struct StopAfterFirst {
    declarations: usize,
    stop_after: usize,
    native_ids: Vec<String>,
}

impl Default for StopAfterFirst {
    fn default() -> Self {
        Self {
            declarations: 0,
            stop_after: 1,
            native_ids: Vec::new(),
        }
    }
}

impl AgentDiagnosticOutput for StopAfterFirst {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        _value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentParseOutput for StopAfterFirst {
    fn emit_declaration(
        &mut self,
        value: crate::services::agent_cli::contracts::ParsedAgentAsset,
    ) -> ControlFlow<AgentOutputStop> {
        self.declarations += 1;
        self.native_ids.push(value.native_id);
        if self.declarations >= self.stop_after {
            ControlFlow::Break(AgentOutputStop::EntryLimit)
        } else {
            ControlFlow::Continue(())
        }
    }
}

#[test]
fn parser_stops_before_hooks_after_mcp_break() {
    let context = crate::models::AgentConfigurationContext {
        id: "gemini-context".to_owned(),
        environment_id: "native".to_owned(),
        agent_kind: crate::models::AgentCliKind::Gemini,
        config_root: "/tmp/gemini".to_owned(),
        profile: "default".to_owned(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: 1,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    };
    let source = crate::services::agent_cli::contracts::AgentAssetSourceSpec {
        verified_physical_path: None,
        hook_definition_source: true,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "settings".to_owned(),
        label: "settings".to_owned(),
        scope: AgentAssetScope::User,
        path: "/tmp/gemini/settings.json".into(),
        allowed_root: "/tmp/gemini".into(),
        precedence: 10,
        writable: true,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Hook,
            AgentAssetCategory::StatusUi,
        ],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
  "mcpServers": {"first": {"command": "server"}},
  "hooks": {"after": []},
  "footer": "status"
}"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = StopAfterFirst::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations, 1);

    let snapshot = AgentAssetSnapshot::File {
            bytes: br#"{"mcp":{"allowed":["first","second"],"excluded":[]},"mcpServers":{"first":{"command":"first-command"},"second":{"command":"second-command"}},"footer":true}"#.to_vec(),
            revision: Default::default(),
        };
    let mut aggregate_output = StopAfterFirst {
        declarations: 0,
        stop_after: 4,
        native_ids: Vec::new(),
    };
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut aggregate_output,
    );
    assert_eq!(aggregate_output.declarations, 4);
    assert_eq!(
        aggregate_output.native_ids,
        vec!["mcp.allowed", "mcp.excluded", "first", "second",]
    );
}

#[test]
fn hook_slot_matrix_uses_same_decoder_for_settings_and_extensions() {
    let events = [
        "BeforeTool",
        "AfterTool",
        "BeforeAgent",
        "Notification",
        "AfterAgent",
        "SessionStart",
        "SessionEnd",
        "PreCompress",
        "BeforeModel",
        "AfterModel",
        "BeforeToolSelection",
    ];
    for (source_key, expected_prefix) in
        [("settings", ""), ("gemini-extension-hooks:alpha", "alpha:")]
    {
        for event in events {
            let mut source = test_source(source_key, AgentAssetSourceKind::File);
            source.categories = vec![AgentAssetCategory::Hook];
            let snapshot = AgentAssetSnapshot::File {
                    bytes: format!(
                        r#"{{"hooks":{{"{event}":[{{"hooks":[{{"type":"runtime","name":"{event}"}}]}}]}}}}"#
                    )
                    .into_bytes(),
                    revision: Default::default(),
                };
            let mut output = ParseCollector::default();
            parse_assets(
                AgentAssetParseRequest {
                    native_home: None,
                    context: &test_context(),
                    source: &source,
                    snapshot: &snapshot,
                    workspace_canonical: None,
                    workspace_lexical: None,
                },
                &mut output,
            );
            assert_eq!(output.diagnostics, Vec::<AgentAssetDiagnostic>::new());
            assert_eq!(
                output.declarations.len(),
                1,
                "event: {event}, source: {source_key}"
            );
            assert_eq!(
                output.declarations[0].native_id,
                format!("{expected_prefix}{event}:0:0")
            );
        }
    }
}

fn complete_hook_fixture() -> Vec<u8> {
    br#"{
          "hooks": {
            "enabled": true,
            "disabled": false,
            "notifications": true,
            "BeforeTool": [{"hooks": [
              {"type":"command","name":"named-command","command":"command-secret"},
              {"type":"command","command":"unnamed-command-secret"},
              {"type":"runtime","name":"named-runtime"},
              {"type":"plugin","name":"named-plugin"},
              {"type":"plugin"},
              {"type":"command"},
              {"type":"runtime"},
              {"type":"bad","name":"bad-type"},
              {"type":"command","command":"optional-name-command","name":false}
            ]}],
            "AfterTool": [{"hooks":[]}]
          }
        }"#
    .to_vec()
}

fn parse_complete_hook_fixture(source_key: &str) -> ParseCollector {
    let mut source = test_source(source_key, AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Hook];
    let snapshot = AgentAssetSnapshot::File {
        bytes: complete_hook_fixture(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &test_context(),
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    output
}

#[test]
fn complete_hook_fixture_matches_between_settings_and_extension_entrypoints() {
    let settings = parse_complete_hook_fixture("settings");
    let extension = parse_complete_hook_fixture("gemini-extension-hooks:alpha");
    assert_eq!(settings.diagnostics, extension.diagnostics);
    assert_eq!(settings.declarations.len(), 5);
    assert_malformed_hook_slots(
        &settings.diagnostics,
        &[
            "BeforeTool.0.5",
            "BeforeTool.0.6",
            "BeforeTool.0.7",
            "BeforeTool.0.8",
        ],
    );
    assert_eq!(settings.declarations.len(), extension.declarations.len());
    for (settings_declaration, extension_declaration) in settings
        .declarations
        .iter()
        .zip(extension.declarations.iter())
    {
        assert_eq!(
            settings_declaration.native_id,
            extension_declaration
                .native_id
                .strip_prefix("alpha:")
                .expect("extension parent binding")
        );
        assert_eq!(
            settings_declaration.declaration_key,
            extension_declaration
                .declaration_key
                .strip_prefix("alpha:")
                .expect("extension declaration binding")
        );
        assert_eq!(settings_declaration.label, extension_declaration.label);
        assert_eq!(
            settings_declaration.declared_state,
            extension_declaration.declared_state
        );
        assert_eq!(settings_declaration.role, extension_declaration.role);
        assert_eq!(
            settings_declaration.participation,
            extension_declaration.participation
        );
        assert_eq!(
            format!("{:?}", settings_declaration.native_payload),
            format!("{:?}", extension_declaration.native_payload)
        );
    }
    assert!(settings
        .declarations
        .iter()
        .all(|declaration| declaration.facts.is_empty()));
    assert!(extension
        .declarations
        .iter()
        .all(|declaration| declaration.facts.is_empty()));
}

#[test]
fn hook_first_slot_break_stops_later_slots() {
    fn parse_with_break(source_key: &str) -> usize {
        let mut source = test_source(source_key, AgentAssetSourceKind::File);
        source.categories = vec![AgentAssetCategory::Hook];
        let snapshot = AgentAssetSnapshot::File {
                bytes: br#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"first"},{"type":"runtime","name":"second"}]}]}}"#.to_vec(),
                revision: Default::default(),
            };
        let mut output = StopAfterFirst::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &test_context(),
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        output.declarations
    }

    assert_eq!(parse_with_break("settings"), 1);
    assert_eq!(parse_with_break("gemini-extension-hooks:alpha"), 1);
}

#[test]
fn enablement_ids_are_case_insensitive() {
    assert_eq!(" Server-A ".trim().to_ascii_lowercase(), "server-a");
}

#[test]
fn discovers_fixed_sources_without_runtime_environment_overrides() {
    let context = test_context();
    let mut output = FollowUpCollector::default();
    discover_sources(
        AgentSourceDiscoveryRequest {
            installations: &[],
            context: &context,
            home: Path::new("/tmp/home"),
            workspace: Some(Path::new("/tmp/workspace")),
        },
        &mut output,
    );
    let keys = output
        .initial_sources
        .iter()
        .map(|source| source.native_source_key.as_str())
        .collect::<BTreeSet<_>>();
    for key in [
        "system-defaults",
        "settings",
        "system-settings",
        "mcp-enablement",
        "trusted-folders",
        "extensions",
        "extension-enablement",
        "skills",
        "shared-skills",
        "workspace-settings",
        "workspace-skills",
        "workspace-shared-skills",
    ] {
        assert!(keys.contains(key), "missing source {key}");
    }
    assert_eq!(
        output
            .initial_sources
            .iter()
            .find(|source| source.native_source_key == "system-defaults")
            .map(|source| source.precedence),
        Some(5)
    );
    assert_eq!(
        output
            .initial_sources
            .iter()
            .find(|source| source.native_source_key == "extensions")
            .map(|source| source.precedence),
        Some(0)
    );
}

#[test]
fn settings_jsonc_emits_typed_policy_and_gemini_transport() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Mcp];
    let snapshot = AgentAssetSnapshot::File {
            bytes: br#"{
 // comments are accepted by Gemini settings
 "mcp": {"allowed": [" Server-A "], "excluded": ["blocked"]},
 "admin": {"mcp": {"enabled": true, "config": {"Server-A": {"httpUrl": "https://secret.invalid"}}, "requiredConfig": {"required": {"command": "secret"}}}},
 "mcpServers": {"Server-A": {"httpUrl": "https://secret.invalid"}}
}"#
            .to_vec(),
            revision: Default::default(),
        };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert!(output.declarations.iter().any(|asset| matches!(
        asset.native_payload,
        AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::Allowed(_))
    )));
    assert!(output.declarations.iter().any(|asset| matches!(
        asset.native_payload,
        AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::Excluded(_))
    )));
    assert_eq!(output.declarations.len(), 3);
    assert!(output
        .declarations
        .iter()
        .all(|asset| { !asset.declaration_key.starts_with("admin") }));
    let server = output
        .declarations
        .iter()
        .find(|asset| {
            asset.native_id == "Server-A"
                && asset.category == AgentAssetCategory::Mcp
                && asset.role == crate::models::AgentAssetDeclarationRole::Definition
        })
        .expect("server definition");
    assert!(matches!(
        server.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Http,
            ..
        }
    ));
    assert!(!format!("{:?}", server.native_payload).contains("secret.invalid"));

    assert!(!output
        .declarations
        .iter()
        .any(|asset| asset.facts.contains_key("enablementOnly")
            || asset.facts.contains_key("configuration")));
}

#[test]
fn malformed_policy_is_typed_and_absent_policy_stays_absent() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Mcp];
    let absent = AgentAssetSnapshot::File {
        bytes: br#"{}"#.to_vec(),
        revision: Default::default(),
    };
    let mut absent_output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &absent,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut absent_output,
    );
    assert!(absent_output.declarations.is_empty());

    let malformed = AgentAssetSnapshot::File {
        bytes: br#"{"mcp":{"allowed":"bad","excluded":[false]}}"#.to_vec(),
        revision: Default::default(),
    };
    let mut malformed_output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &malformed,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut malformed_output,
    );
    assert!(malformed_output.declarations.iter().any(|asset| matches!(
        asset.native_payload,
        AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::InvalidAllowed)
    )));
    assert!(malformed_output.declarations.iter().any(|asset| matches!(
        asset.native_payload,
        AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::InvalidExcluded)
    )));
    assert!(!format!("{:?}", malformed_output.declarations).contains("bad"));
}

#[test]
fn blocked_and_root_malformed_settings_emit_all_private_mcp_control_markers() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Mcp];
    let blocked = AgentAssetSnapshot::Blocked {
        revision: Default::default(),
        diagnostic: AgentAssetDiagnostic::Malformed {
            format: AgentAssetDocumentFormat::Json,
            location: Some("blocked".to_owned()),
        },
    };
    let root_malformed = AgentAssetSnapshot::File {
        bytes: br#"["#.to_vec(),
        revision: Default::default(),
    };
    for snapshot in [&blocked, &root_malformed] {
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert!(output.declarations.iter().any(|asset| matches!(
            asset.native_payload,
            AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::InvalidAllowed)
        )));
        assert!(output.declarations.iter().any(|asset| matches!(
            asset.native_payload,
            AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::InvalidExcluded)
        )));
        assert_eq!(output.declarations.len(), 2);
    }
}

#[test]
fn policy_marker_debug_is_shape_only_and_policy_break_is_terminal() {
    let policies = [
        AgentMcpPolicyPayload::Allowed(BTreeSet::from(["secret-server".to_owned()])),
        AgentMcpPolicyPayload::Excluded(BTreeSet::from(["secret-server".to_owned()])),
        AgentMcpPolicyPayload::InvalidAllowed,
        AgentMcpPolicyPayload::InvalidExcluded,
    ];
    for policy in policies {
        let debug = format!("{policy:?}");
        assert!(!debug.contains("secret-server"));
        assert!(!debug.contains("secret.invalid"));
    }
    for control in [
        AgentAssetInvalidControl::McpEnablement,
        AgentAssetInvalidControl::ExtensionEnablement,
    ] {
        let debug = format!("{:?}", AgentAssetNativePayload::InvalidControl(control));
        assert!(debug.contains("InvalidControl"));
        assert!(!debug.contains("secret"));
    }

    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Mcp];
    let snapshot = AgentAssetSnapshot::File {
        bytes:
            br#"{"mcp":{"allowed":["secret-server"]},"mcpServers":{"secret-server":{"url":"https://secret.invalid"}}}"#
                .to_vec(),
        revision: Default::default(),
    };
    let mut output = StopAfterFirst::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations, 1);
}

#[test]
fn settings_root_failure_preserves_hook_and_status_unknown_evidence() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Hook,
        AgentAssetCategory::StatusUi,
    ];
    for snapshot in [
        AgentAssetSnapshot::Blocked {
            revision: Default::default(),
            diagnostic: AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Json,
                location: Some("blocked".to_owned()),
            },
        },
        AgentAssetSnapshot::File {
            bytes: br#"["#.to_vec(),
            revision: Default::default(),
        },
    ] {
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert!(output.declarations.iter().any(|asset| matches!(
            asset.native_payload,
            AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::InvalidDisabledSet)
        )));
        assert!(output.declarations.iter().any(|asset| {
            asset.category == AgentAssetCategory::StatusUi
                && asset.native_id == "footer"
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && matches!(
                    asset.details,
                    AgentAssetDetails::StatusUi {
                        mode: AgentStatusUiMode::Unknown,
                        ..
                    }
                )
        }));
    }

    let malformed_hooks_config = AgentAssetSnapshot::File {
        bytes: br#"{"hooksConfig":false}"#.to_vec(),
        revision: Default::default(),
    };
    let mut malformed_output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &malformed_hooks_config,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut malformed_output,
    );
    assert!(malformed_output.declarations.iter().any(|asset| matches!(
        asset.native_payload,
        AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::InvalidDisabledSet)
    )));
    let absent_hooks_config = AgentAssetSnapshot::File {
        bytes: br#"{}"#.to_vec(),
        revision: Default::default(),
    };
    let mut absent_output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &absent_hooks_config,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut absent_output,
    );
    assert!(!absent_output
        .declarations
        .iter()
        .any(|asset| { matches!(asset.native_payload, AgentAssetNativePayload::HookPolicy(_)) }));
}

#[test]
fn enablement_absence_malformed_entries_and_source_failures_are_distinct() {
    let context = test_context();
    for (source_key, category, invalid_control) in [
        (
            "mcp-enablement",
            AgentAssetCategory::Mcp,
            AgentAssetInvalidControl::McpEnablement,
        ),
        (
            "extension-enablement",
            AgentAssetCategory::Extension,
            AgentAssetInvalidControl::ExtensionEnablement,
        ),
    ] {
        let mut source = test_source(source_key, AgentAssetSourceKind::File);
        source.categories = vec![category];
        let absent = AgentAssetSnapshot::File {
            bytes: br#"{}"#.to_vec(),
            revision: Default::default(),
        };
        let mut absent_output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &absent,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut absent_output,
        );
        assert!(absent_output.declarations.is_empty());

        let malformed_entry = AgentAssetSnapshot::File {
            bytes: br#"{"server":{"enabled":"bad"}}"#.to_vec(),
            revision: Default::default(),
        };
        let mut malformed_entry_output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &malformed_entry,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut malformed_entry_output,
        );
        assert!(malformed_entry_output.declarations.iter().any(|asset| {
            asset.native_id == "server" && asset.declared_state == AgentAssetDeclaredState::Unknown
        }));

        for snapshot in [
            AgentAssetSnapshot::Blocked {
                revision: Default::default(),
                diagnostic: AgentAssetDiagnostic::Malformed {
                    format: AgentAssetDocumentFormat::Json,
                    location: Some("blocked".to_owned()),
                },
            },
            AgentAssetSnapshot::File {
                bytes: br#"["#.to_vec(),
                revision: Default::default(),
            },
        ] {
            let mut output = ParseCollector::default();
            parse_assets(
                AgentAssetParseRequest {
                    native_home: None,
                    context: &context,
                    source: &source,
                    snapshot: &snapshot,
                    workspace_canonical: None,
                    workspace_lexical: None,
                },
                &mut output,
            );
            assert!(output.declarations.iter().any(|asset| {
                asset.declared_state == AgentAssetDeclaredState::Unknown
                    && matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::InvalidControl(control)
                            if control == invalid_control
                    )
            }));
        }
    }
}

#[test]
fn skill_requires_frontmatter_without_publishing_description() {
    let mut context = test_context();
    context.workspace_id = Some("workspace".to_owned());
    context.trust_context = AgentTrustState::Untrusted;
    let mut source = test_source(
        "gemini-skill-manifest:skills:SKILL.md",
        AgentAssetSourceKind::File,
    );
    source.categories = vec![AgentAssetCategory::Skill];
    let snapshot = AgentAssetSnapshot::File {
        bytes: b"---\r\nname: release\r\ndescription: deploy safely\r\n---\r\nbody".to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 1);
    assert_eq!(output.declarations[0].label, "release");
    assert!(matches!(
        output.declarations[0].participation,
        AgentAssetResolutionParticipation::Participates
    ));
    assert!(output.declarations[0].facts.is_empty());

    let invalid = AgentAssetSnapshot::File {
        bytes: b"---\nname: release\n---\nbody".to_vec(),
        revision: Default::default(),
    };
    let mut invalid_output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &invalid,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut invalid_output,
    );
    assert!(invalid_output.declarations.is_empty());
    assert_eq!(invalid_output.diagnostics.len(), 1);
}

#[test]
fn extension_skill_relationship_uses_physical_extension_identity() {
    let context = test_context();
    let mut source = test_source(
        "gemini-extension-skill:alpha:writer",
        AgentAssetSourceKind::File,
    );
    source.categories = vec![AgentAssetCategory::Skill];
    let snapshot = AgentAssetSnapshot::File {
        bytes: b"---\nname: writer\ndescription: writes docs\n---\n".to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 1);
    let declaration = &output.declarations[0];
    assert_eq!(
        declaration
            .provided_by
            .as_ref()
            .map(|value| value.native_id.as_str()),
        Some("alpha")
    );
    assert_eq!(
        declaration
            .action_owner
            .as_ref()
            .map(|value| value.native_id.as_str()),
        Some("alpha")
    );
}

#[test]
fn extension_enablement_matches_current_path_and_last_rule_wins() {
    let mut context = test_context();
    context.workspace_id = Some("workspace".to_owned());
    let source = test_source("extension-enablement", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
            bytes: br#"{"alpha":{"overrides":["!/tmp/project/*","/tmp/project/*"]},"beta":{"overrides":["!/other/*"]}}"#.to_vec(),
            revision: Default::default(),
        };
    let workspace = Path::new("/tmp/project/current");
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: Some(workspace),
            workspace_lexical: Some(workspace),
        },
        &mut output,
    );
    let alpha = output
        .declarations
        .iter()
        .find(|value| value.native_id == "alpha")
        .expect("alpha state");
    let beta = output
        .declarations
        .iter()
        .find(|value| value.native_id == "beta")
        .expect("beta state");
    assert_eq!(alpha.declared_state, AgentAssetDeclaredState::Enabled);
    assert_eq!(beta.declared_state, AgentAssetDeclaredState::Enabled);
}

#[test]
fn extension_enablement_malformed_entries_fail_closed_with_unknown_overlay() {
    for bytes in [
        br#"{"alpha":{}}"#.as_slice(),
        br#"{"alpha":{"overrides":[false]}}"#.as_slice(),
        br#"{"alpha":{"overrides":["!"]}}"#.as_slice(),
        br#"{"alpha":false}"#.as_slice(),
    ] {
        let context = test_context();
        let source = test_source("extension-enablement", AgentAssetSourceKind::File);
        let snapshot = AgentAssetSnapshot::File {
            bytes: bytes.to_vec(),
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        let overlay = output
            .declarations
            .iter()
            .find(|asset| asset.native_id == "alpha")
            .expect("typed unknown overlay");
        assert_eq!(overlay.declared_state, AgentAssetDeclaredState::Unknown);
        assert!(!output.diagnostics.is_empty());
    }
}

#[test]
fn skill_parser_matches_yaml_fallback_sanitization_and_bom_rejection() {
    let context = test_context();
    let source = test_source(
        "gemini-skill-manifest:skills:SKILL.md",
        AgentAssetSourceKind::File,
    );
    let cases = [
            (b"---\nname: \"publish:tool\"\ndescription: \"Unicode \xE6\x8F\x8F\xE8\xBF\xB0\"\n---\nbody".to_vec(), "publish-tool", "Unicode 描述"),
            (b"---\nname: writer\ndescription: |\n  first line\n  second line\n---\nbody".to_vec(), "writer", "first line\nsecond line\n"),
            (b"---\nname: first\nname: second\ndescription: fallback\n---\nbody".to_vec(), "second", "fallback"),
        ];
    for (bytes, expected_name, _) in cases {
        let snapshot = AgentAssetSnapshot::File {
            bytes,
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert_eq!(output.declarations.len(), 1);
        assert_eq!(output.declarations[0].label, expected_name);
        assert!(output.declarations[0].facts.is_empty());
    }
    let bom = AgentAssetSnapshot::File {
        bytes: b"\xEF\xBB\xBF---\nname: bom\ndescription: rejected\n---".to_vec(),
        revision: Default::default(),
    };
    let mut rejected = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &bom,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut rejected,
    );
    assert!(rejected.declarations.is_empty());
    assert_eq!(rejected.diagnostics.len(), 1);
}

#[test]
fn skill_fallback_matches_official_whitespace_and_presence_semantics() {
    let context = test_context();
    let source = test_source(
        "gemini-skill-manifest:skills:SKILL.md",
        AgentAssetSourceKind::File,
    );
    let cases = [
        (
            "---\n  name: fallback-name\n  description: fallback description\nbroken: [\n---",
            "fallback-name",
            "fallback description",
        ),
        ("---\nname: 42\ndescription: 9\n---", "42", "9"),
        ("---\nname:\ndescription:\n---", "", ""),
        (
            "---\nname: \"broken\ndescription: readable fallback\n---",
            "-broken",
            "readable fallback",
        ),
        (
            "---\ndescription: fallback\nname: after\nbroken: [\n---",
            "after",
            "fallback",
        ),
        (
            "---\nname: 'single quoted'\ndescription: >\n  folded\n  description\n---",
            "single quoted",
            "folded description\n",
        ),
        (
            "---\nname: \"broken\ndescription: |\n  name: continuation\n---",
            "-broken",
            "| name: continuation",
        ),
        (
            "---\nname: a:/\\<>*?\"|中\ndescription: safe\n---",
            "a---------中",
            "safe",
        ),
    ];
    for (text, expected_name, _) in cases {
        let snapshot = AgentAssetSnapshot::File {
            bytes: text.as_bytes().to_vec(),
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert_eq!(output.declarations.len(), 1, "fixture: {text}");
        assert_eq!(
            output.declarations[0].label, expected_name,
            "fixture: {text}"
        );
        assert!(output.declarations[0].facts.is_empty());
    }
    let crlf = AgentAssetSnapshot::File {
        bytes: b"---\r\nname: crlf\r\ndescription: works\r\n---\r\n".to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &crlf,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations[0].label, "crlf");
}

#[test]
fn settings_hooks_are_independent_slots_and_sensitive_values_stay_out_of_facts() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Hook];
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
              "hooks": {
                "BeforeTool": [{
                  "matcher": "*",
                  "hooks": [
                    {"type":"command","command":"secret-command"},
                    {"type":"unknown","command":"bad"},
                    {"type":"plugin","name":"secret-plugin"}
                  ]
                }]
              }
            }"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 2);
    assert!(output
        .declarations
        .iter()
        .any(|asset| asset.native_id == "BeforeTool:0:0"));
    assert!(output
        .declarations
        .iter()
        .any(|asset| asset.native_id == "BeforeTool:0:2"));
    assert!(output
        .declarations
        .iter()
        .all(|asset| asset.facts.is_empty()));
    let debug = format!("{:?}", output.declarations);
    assert!(!debug.contains("secret-command"));
    assert!(!debug.contains("secret-plugin"));
    assert_malformed_hook_slots(&output.diagnostics, &["BeforeTool.0.1"]);
}

#[test]
fn settings_hook_decoder_validates_native_schema_and_disabled_names() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Hook];
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
              "disabledHooks": ["ignored-root-field"],
              "hooksConfig": {"disabled": ["disabled-command", "runtime-hook"]},
              "hooks": {
                "BeforeTool": [{"hooks": [
                  {"type":"command","name":"disabled-command","command":"secret-command"},
                  {"type":"runtime","name":"runtime-hook"},
                  {"type":"plugin","name":"plugin-hook"},
                  {"type":"command"},
                  {"type":"http","url":"secret-url"}
                ]}],
                "InvalidEvent": [{"hooks": [{"type":"command","command":"sibling"}]}]
              }
            }"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 4);
    assert!(matches!(
        output.declarations[0].native_payload,
        AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::DisabledSet(ref members))
            if members.len() == 2
    ));
    assert!(output.declarations[1..].iter().all(|asset| {
        asset.declared_state == AgentAssetDeclaredState::Unknown
            && matches!(
                asset.native_payload,
                AgentAssetNativePayload::HookDefinition(_)
            )
    }));
    assert_malformed_hook_slots(
        &output.diagnostics,
        &["BeforeTool.0.3", "BeforeTool.0.4", "InvalidEvent"],
    );
    let debug = format!("{:?}", output.declarations);
    for marker in [
        "secret-command",
        "secret-url",
        "sibling",
        "ignored-root-field",
    ] {
        assert!(
            !debug.contains(marker),
            "sensitive/invalid hook value leaked: {marker}"
        );
    }
}

#[test]
fn hook_slots_follow_native_matcher_and_empty_slot_semantics() {
    let context = test_context();
    let mut source = test_source("settings", AgentAssetSourceKind::File);
    source.categories = vec![AgentAssetCategory::Hook];
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
              "hooks": {"BeforeTool": [{"hooks": [
                {"type":"command","command":"command-name"},
                {"type":"command","name":"named-command","command":"secret-command"},
                {"type":"runtime","name":"runtime-name"},
                {"type":"plugin"},
                {"type":"plugin","name":""},
                {"type":"plugin","name":false},
                {"type":"runtime"},
                {"type":"command","command":""}
              ]}]}
            }"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 5);
    assert_malformed_hook_slots(
        &output.diagnostics,
        &["BeforeTool.0.5", "BeforeTool.0.6", "BeforeTool.0.7"],
    );
    let debug = format!("{:?}", output.declarations);
    for marker in ["command-name", "named-command", "runtime-name"] {
        assert!(!debug.contains(marker));
    }
}

#[test]
fn hook_disabled_policy_preserves_exact_union_members_and_local_diagnostics() {
    let cases = [
        (r#"{"hooksConfig":{}}"#, 0, false),
        (r#"{"hooksConfig":{"disabled":[]}}"#, 0, false),
        (
            r#"{"hooksConfig":{"disabled":["same","same","Case"]}}"#,
            2,
            false,
        ),
        (r#"{"hooksConfig":{"disabled":["case","Case"]}}"#, 2, false),
        (r#"{"hooksConfig":{"disabled":null}}"#, 0, true),
        (r#"{"hooksConfig":{"disabled":["valid",false]}}"#, 0, true),
    ];
    for (bytes, member_count, invalid) in cases {
        let context = test_context();
        let mut source = test_source("settings", AgentAssetSourceKind::File);
        source.categories = vec![AgentAssetCategory::Hook];
        let snapshot = AgentAssetSnapshot::File {
            bytes: bytes.as_bytes().to_vec(),
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        if bytes == r#"{"hooksConfig":{}}"# {
            assert!(output.declarations.is_empty());
            continue;
        }
        assert_eq!(output.declarations.len(), 1);
        assert!(
            matches!(
                output.declarations[0].native_payload,
                AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::DisabledSet(ref members))
                    if !invalid && members.len() == member_count
            ) || matches!(
                output.declarations[0].native_payload,
                AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::InvalidDisabledSet)
                    if invalid
            )
        );
        assert_eq!(output.diagnostics.len(), usize::from(invalid));
        assert_eq!(
            output.declarations[0].participation,
            AgentAssetResolutionParticipation::Participates
        );
    }
}

#[test]
fn hook_native_payload_debug_is_shape_only() {
    let matcher = AgentHookDisableMatcherIdentity::new("hook-command-secret".to_owned());
    let matcher_debug = format!("{matcher:?}");
    assert_eq!(
        matcher_debug,
        "AgentHookDisableMatcherIdentity { redacted: true }"
    );
    let policy = AgentHookPolicyPayload::DisabledSet(
        [AgentHookDisableMatcherIdentity::new(
            "policy-secret".to_owned(),
        )]
        .into_iter()
        .collect(),
    );
    let policy_debug = format!("{policy:?}");
    let payload_debug = format!("{:?}", AgentAssetNativePayload::HookPolicy(policy));
    let definition_debug = format!("{:?}", AgentAssetNativePayload::HookDefinition(matcher));
    for debug in [policy_debug, payload_debug, definition_debug] {
        assert!(!debug.contains("hook-command-secret"));
        assert!(!debug.contains("policy-secret"));
    }
}

#[test]
fn mcp_definition_debug_is_shape_only_for_each_matcher() {
    let declared = AgentMcpDefinitionPayload {
        identity: AgentMcpMatcherIdentity::Stdio {
            argv: vec!["secret-command".to_owned(), "secret-arg".to_owned()],
        },
        origin: AgentMcpDefinitionOrigin::Declared,
    };
    let remote = AgentMcpDefinitionPayload {
        identity: AgentMcpMatcherIdentity::Remote {
            url: "https://secret.invalid".to_owned(),
        },
        origin: AgentMcpDefinitionOrigin::Declared,
    };
    let debug = format!("{declared:?}{remote:?}");
    assert!(debug.contains("Declared"));
    assert!(debug.contains("Stdio"));
    assert!(debug.contains("Remote"));
    assert!(debug.contains("argument_count: 2"));
    assert!(!debug.contains("secret-command"));
    assert!(!debug.contains("secret-arg"));
    assert!(!debug.contains("secret.invalid"));
}

#[test]
fn real_gemini_inventory_discovers_root_child_skills_extension_children_and_hooks() {
    let home = inventory_test_root("real-production");
    let gemini = home.join(".gemini");
    fs::create_dir_all(gemini.join("skills/release")).unwrap();
    fs::create_dir_all(gemini.join("extensions/alpha/hooks")).unwrap();
    fs::create_dir_all(gemini.join("extensions/alpha/skills/writer")).unwrap();
    fs::write(
        gemini.join("settings.json"),
        br#"{
              "hooksConfig": {"disabled": ["settings-command", "extension-plugin", "unknown-hook"]},
              "hooks": {"BeforeTool": [{"hooks": [
                {"type":"command","command":"settings-command"},
                {"type":"plugin"},
                {"type":"unknown","command":"settings-invalid-secret"}
              ]}]}
            }"#,
    )
    .unwrap();
    let skill = |name: &str| format!("---\nname: {name}\ndescription: {name} description\n---\n");
    fs::write(gemini.join("skills/SKILL.md"), skill("root-skill")).unwrap();
    fs::write(
        gemini.join("skills/release/SKILL.md"),
        skill("release-skill"),
    )
    .unwrap();
    fs::write(
        gemini.join("extensions/alpha/gemini-extension.json"),
        br#"{"name":"alpha","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        gemini.join("extensions/alpha/hooks/hooks.json"),
        br#"{"hooks":{"AfterTool":[{"hooks":[
              {"type":"plugin","name":"extension-plugin"},
              {"type":"plugin"},
              {"type":"unknown","command":"extension-invalid-secret"}
            ]}]}}"#,
    )
    .unwrap();
    fs::write(
        gemini.join("extensions/alpha/skills/SKILL.md"),
        skill("extension-root"),
    )
    .unwrap();
    fs::write(
        gemini.join("extensions/alpha/skills/writer/SKILL.md"),
        skill("extension-child"),
    )
    .unwrap();
    let home = fs::canonicalize(home).unwrap();

    let definition = super::super::definition(AgentCliKind::Gemini);
    let inventory = crate::services::agent_cli::environment::build_inventory_for_test(
        &home,
        None,
        &[definition],
    )
    .unwrap();
    let paths = inventory
        .sources
        .iter()
        .map(|source| source.path.as_str())
        .collect::<Vec<_>>();
    for suffix in [
        ".gemini/skills/SKILL.md",
        ".gemini/skills/release/SKILL.md",
        ".gemini/extensions/alpha/skills/SKILL.md",
        ".gemini/extensions/alpha/skills/writer/SKILL.md",
        ".gemini/extensions/alpha/hooks/hooks.json",
    ] {
        assert!(
            paths.iter().any(|path| path.replace('\\', "/").ends_with(suffix)),
            "missing production source {suffix}"
        );
    }
    for native_id in [
        "root-skill",
        "release-skill",
        "extension-root",
        "extension-child",
        "hooksConfig.disabled",
        "BeforeTool:0:0",
    ] {
        assert!(
            inventory
                .declarations
                .iter()
                .any(|declaration| declaration.native_id == native_id),
            "missing production declaration {native_id}"
        );
    }
    assert!(inventory
        .declarations
        .iter()
        .any(|declaration| declaration.native_id == "alpha:AfterTool:0:0"));
    assert_eq!(inventory.assets.len(), 9);
    for (category, expected) in [
        (AgentAssetCategory::Extension, 1),
        (AgentAssetCategory::Skill, 4),
        (AgentAssetCategory::Hook, 4),
        (AgentAssetCategory::Mcp, 0),
        (AgentAssetCategory::Plugin, 0),
        (AgentAssetCategory::StatusUi, 0),
    ] {
        assert_eq!(
            inventory
                .assets
                .iter()
                .filter(|asset| asset.category == category)
                .count(),
            expected,
            "category {category:?}"
        );
    }
    let parent = inventory
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Extension && asset.native_id == "alpha")
        .unwrap();
    assert_eq!(parent.effective_state, AgentAssetState::Enabled);
    for native_id in [
        "root-skill",
        "release-skill",
        "extension-root",
        "extension-child",
    ] {
        let asset = inventory
            .assets
            .iter()
            .find(|asset| {
                asset.category == AgentAssetCategory::Skill && asset.native_id == native_id
            })
            .unwrap();
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        let expected_parent = native_id
            .starts_with("extension-")
            .then_some(parent.stable_id.as_str());
        assert_eq!(asset.relationships.provided_by.as_deref(), expected_parent);
        assert_eq!(asset.relationships.action_owner.as_deref(), expected_parent);
    }
    for native_id in [
        "BeforeTool:0:0",
        "BeforeTool:0:1",
        "alpha:AfterTool:0:0",
        "alpha:AfterTool:0:1",
    ] {
        let asset = inventory
            .assets
            .iter()
            .find(|asset| {
                asset.category == AgentAssetCategory::Hook && asset.native_id == native_id
            })
            .unwrap();
        assert_eq!(asset.declared_state, AgentAssetState::Disabled);
        assert_eq!(asset.effective_state, AgentAssetState::Disabled);
        let expected_parent = native_id
            .starts_with("alpha:")
            .then_some(parent.stable_id.as_str());
        assert_eq!(asset.relationships.provided_by.as_deref(), expected_parent);
        assert_eq!(asset.relationships.action_owner.as_deref(), expected_parent);
    }
    assert!(!inventory.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidProjection { .. }
            | AgentAssetDiagnostic::InvalidResolution { .. }
    )));
    let serialized = serde_json::to_string(&inventory).unwrap();
    let debug = format!("{inventory:?}");
    let diagnostics_debug = format!("{:?}", inventory.diagnostics);
    for evidence in [serialized, debug, diagnostics_debug] {
        for secret in [
            "settings-command",
            "settings-invalid-secret",
            "extension-plugin",
            "extension-invalid-secret",
            "unknown-hook",
        ] {
            assert!(
                !evidence.contains(secret),
                "production inventory leaked {secret}: {evidence}"
            );
        }
    }
    let _ = fs::remove_dir_all(home);
}

#[test]
fn extension_hooks_reuse_native_slot_schema_and_keep_parent_binding() {
    let context = test_context();
    let source = test_source("gemini-extension-hooks:alpha", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
              "hooks": {
                "AfterTool": [{"hooks": [
                  {"type":"runtime","name":"runtime-hook"},
                  {"type":"command","command":"secret-command"},
                  {"type":"plugin","name":"plugin-hook"},
                  {"type":"runtime"}
                ]}],
                "NotAnEvent": [{"hooks": [{"type":"command","command":"sibling"}]}]
              }
            }"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations.len(), 3);
    assert!(output
        .declarations
        .iter()
        .all(|asset| asset.native_id.starts_with("alpha:AfterTool:")));
    assert!(output.declarations.iter().all(|asset| asset
        .provided_by
        .as_ref()
        .map(|value| value.native_id.as_str())
        == Some("alpha")));
    assert!(output.declarations.iter().all(|asset| {
        asset.declared_state == AgentAssetDeclaredState::Unknown
            && matches!(
                asset.native_payload,
                AgentAssetNativePayload::HookDefinition(_)
            )
    }));
    assert_malformed_hook_slots(&output.diagnostics, &["AfterTool.0.3", "NotAnEvent"]);
    let debug = format!("{:?}", output.declarations);
    for marker in ["secret-command", "sibling"] {
        assert!(!debug.contains(marker), "hook value leaked: {marker}");
    }
}

#[test]
fn extension_enablement_considers_canonical_and_lexical_workspace_forms() {
    let mut context = test_context();
    context.workspace_id = Some("workspace".to_owned());
    let source = test_source("extension-enablement", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{"alpha":{"overrides":["!/tmp/project/current/*"]}}"#.to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: Some(Path::new("/tmp/project/current")),
            workspace_lexical: Some(Path::new("/tmp/alias/current")),
        },
        &mut output,
    );
    assert_eq!(
        output
            .declarations
            .iter()
            .find(|asset| asset.native_id == "alpha")
            .map(|asset| asset.declared_state),
        Some(AgentAssetDeclaredState::Disabled)
    );
}

#[test]
fn extension_enablement_uses_one_gemini_normalizer_and_complete_state_matrix() {
    let context = AgentConfigurationContext {
        workspace_id: Some("workspace".to_owned()),
        ..test_context()
    };
    let source = test_source("extension-enablement", AgentAssetSourceKind::File);
    let parse = |json: &str, canonical: Option<&Path>, lexical: Option<&Path>| {
        let snapshot = AgentAssetSnapshot::File {
            bytes: json.as_bytes().to_vec(),
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: canonical,
                workspace_lexical: lexical,
            },
            &mut output,
        );
        output
    };
    let windows = parse(
        r#"{"windows":{"overrides":["!C:/work/*"]}}"#,
        Some(Path::new("C:/work/current")),
        None,
    );
    assert_eq!(
        windows.declarations[0].declared_state,
        AgentAssetDeclaredState::Disabled
    );
    let canonical_only = parse(
        r#"{"canonical":{"overrides":["!/canonical/*"]}}"#,
        Some(Path::new("/canonical/current")),
        Some(Path::new("/other/current")),
    );
    assert_eq!(
        canonical_only.declarations[0].declared_state,
        AgentAssetDeclaredState::Disabled
    );
    let lexical_only = parse(
        r#"{"lexical":{"overrides":["!/alias/*"]}}"#,
        Some(Path::new("/canonical/current")),
        Some(Path::new("/alias/current")),
    );
    assert_eq!(
        lexical_only.declarations[0].declared_state,
        AgentAssetDeclaredState::Disabled
    );
    let conflict = parse(
        r#"{"conflict":{"overrides":["!/canonical/*","/alias/*"]}}"#,
        Some(Path::new("/canonical/current")),
        Some(Path::new("/alias/current")),
    );
    assert_eq!(
        conflict.declarations[0].declared_state,
        AgentAssetDeclaredState::Enabled
    );
    let empty = parse(r#"{"empty":{"overrides":[]}}"#, None, None);
    assert_eq!(
        empty.declarations[0].declared_state,
        AgentAssetDeclaredState::Enabled
    );
    let missing = parse(r#"{"other":{"overrides":[]}}"#, None, None);
    assert!(missing
        .declarations
        .iter()
        .all(|asset| asset.native_id != "empty"));
    let unknown = parse(r#"{"unknown":{"overrides":["/workspace/*"]}}"#, None, None);
    assert_eq!(
        unknown.declarations[0].declared_state,
        AgentAssetDeclaredState::Unknown
    );
    assert!(unknown.diagnostics.is_empty());
}

#[test]
fn local_admin_only_settings_emit_no_definitions_or_policy() {
    let trace = run_d2_fixture(
        br#"{"admin":{"mcp":{"enabled":false,"config":{"bad":{},"ok":{"httpUrl":"https://example.invalid"}},"requiredConfig":{"required":{"command":"tool"},"bad":{"args":[]}}}}}"#,
        AgentTrustState::Trusted,
    );
    assert!(trace.declarations.is_empty());
    assert!(trace.drafts.is_empty());
    assert!(trace.records.is_empty());
    assert!(trace.diagnostics.is_empty());
}

#[test]
fn jsonc_duplicate_control_key_fails_closed_without_partial_policy() {
    let context = test_context();
    let source = test_source("settings", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{"mcp":{"allowed":["one"],"allowed":["two"]}}"#.to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert!(output.declarations.is_empty());
    assert!(output.diagnostics.iter().any(|value| matches!(value, AgentAssetDiagnostic::Malformed { location: Some(location), .. } if location == "duplicate-control-key")));
}

#[test]
fn trusted_folders_uses_longest_match_and_parent_rules() {
    let source = test_source("trusted-folders", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
  "/tmp": "TRUST_PARENT",
  "/tmp/workspace": "DO_NOT_TRUST",
  "/tmp/workspace/project": "TRUST_FOLDER"
}"#
        .to_vec(),
        revision: Default::default(),
    };
    let source_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &snapshot,
    };
    let mut output = FollowUpCollector::default();
    assert_eq!(
        resolve_workspace_trust(
            crate::services::agent_cli::contracts::AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp/workspace/project/child"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&source_ref),
            },
            &mut output,
        ),
        AgentTrustState::Trusted
    );
    assert_eq!(
        resolve_workspace_trust(
            crate::services::agent_cli::contracts::AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp/workspace/other"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&source_ref),
            },
            &mut output,
        ),
        AgentTrustState::Untrusted
    );
}

#[test]
fn trust_parent_includes_effective_parent_and_platform_paths_are_injectable() {
    let source = test_source("trusted-folders", AgentAssetSourceKind::File);
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{"/tmp/project":"TRUST_PARENT"}"#.to_vec(),
        revision: Default::default(),
    };
    let source_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &snapshot,
    };
    let mut output = FollowUpCollector::default();
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&source_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Trusted
    );

    let windows = AgentAssetSnapshot::File {
        bytes: br#"{"C:/Work":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let windows_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &windows,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("c:/work/Child"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&windows_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Windows,
        ),
        AgentTrustState::Trusted
    );

    let mixed_case = AgentAssetSnapshot::File {
        bytes: br#"{"/tmp/Work":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let mixed_case_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &mixed_case,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp/work"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&mixed_case_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Macos,
        ),
        AgentTrustState::Trusted
    );
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp/work"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&mixed_case_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Unknown
    );

    let posix_root = AgentAssetSnapshot::File {
        bytes: br#"{"/":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let posix_root_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &posix_root,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/any/path"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&posix_root_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Trusted
    );

    let windows_root = AgentAssetSnapshot::File {
        bytes: br#"{"C:/":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let windows_root_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &windows_root,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("C:/workspace"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&windows_root_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Windows,
        ),
        AgentTrustState::Trusted
    );
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("D:/workspace"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&windows_root_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Windows,
        ),
        AgentTrustState::Unknown
    );

    let canonical_lexical = AgentAssetSnapshot::File {
        bytes: br#"{"/canonical":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let canonical_lexical_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &canonical_lexical,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/canonical/workspace"),
                workspace_lexical: Some(Path::new("/alias/workspace")),
                sources: std::slice::from_ref(&canonical_lexical_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Trusted
    );
    let lexical_only = AgentAssetSnapshot::File {
        bytes: br#"{"/alias":"TRUST_FOLDER"}"#.to_vec(),
        revision: Default::default(),
    };
    let lexical_only_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &lexical_only,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/canonical/workspace"),
                workspace_lexical: Some(Path::new("/alias/workspace")),
                sources: std::slice::from_ref(&lexical_only_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Trusted
    );

    let longest = AgentAssetSnapshot::File {
        bytes: br#"{"/":"TRUST_FOLDER","/tmp":"DO_NOT_TRUST"}"#.to_vec(),
        revision: Default::default(),
    };
    let longest_ref = crate::services::agent_cli::contracts::AgentAssetResolveSource {
        spec: &source,
        snapshot: &longest,
    };
    assert_eq!(
        super::trust::resolve_workspace_trust_with_platform(
            AgentWorkspaceTrustResolveRequest {
                workspace: Path::new("/tmp/workspace"),
                workspace_lexical: None,
                sources: std::slice::from_ref(&longest_ref)
            },
            &mut output,
            super::trust::TrustPathPlatform::Linux,
        ),
        AgentTrustState::Untrusted
    );
}

#[test]
fn extensions_follow_up_emits_fixed_manifest_sources_for_directories_only() {
    let parent = test_source(EXTENSIONS_SOURCE_KEY, AgentAssetSourceKind::Directory);
    let manifest = vec![
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "alpha".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "plain-file".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        },
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "linked".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: true,
        },
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "  ".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
    ];
    let mut output = FollowUpCollector::default();
    discover_follow_up_sources(
        AgentFollowUpSourceDiscoveryRequest {
            parent: &parent,
            manifest: &manifest,
        },
        &mut output,
    );

    assert!(output.diagnostics.is_empty());
    assert_eq!(output.sources.len(), 3);
    let source = &output.sources[0];
    assert_eq!(source.parent_source_key, "extensions");
    assert!(matches!(
        source.target,
        AgentFollowUpSourceTarget::Descendant { ref directory_entry_name, ref relative_path }
            if directory_entry_name == "alpha" && relative_path == std::path::Path::new("gemini-extension.json")
    ));
    assert_eq!(source.native_source_key, "gemini-extension-manifest:alpha");
    assert_eq!(source.label, "Gemini CLI Extension 清单：alpha");
    assert_eq!(source.scope, AgentAssetScope::User);
    assert_eq!(source.precedence, 0);
    assert!(source.sensitive);
    assert_eq!(source.source_kind, AgentAssetSourceKind::File);
    assert_eq!(
        source.categories,
        vec![AgentAssetCategory::Extension, AgentAssetCategory::Mcp]
    );
}

#[test]
fn extensions_follow_up_ignores_non_extension_parents_and_stops_after_break() {
    let manifest = vec![
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "first".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
        crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
            name: "second".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        },
    ];
    let skills = test_source("skills", AgentAssetSourceKind::Directory);
    let mut output = FollowUpCollector::default();
    discover_follow_up_sources(
        AgentFollowUpSourceDiscoveryRequest {
            parent: &skills,
            manifest: &manifest,
        },
        &mut output,
    );
    assert_eq!(output.sources.len(), 2);
    assert!(output
        .sources
        .iter()
        .all(|source| source.source_kind == AgentAssetSourceKind::File));

    let parent = test_source(EXTENSIONS_SOURCE_KEY, AgentAssetSourceKind::Directory);
    let mut output = FollowUpCollector {
        stop_after_first: true,
        ..FollowUpCollector::default()
    };
    discover_follow_up_sources(
        AgentFollowUpSourceDiscoveryRequest {
            parent: &parent,
            manifest: &manifest,
        },
        &mut output,
    );
    assert_eq!(output.sources.len(), 1);
    assert_eq!(
        output.sources[0].native_source_key,
        "gemini-extension-manifest:first"
    );
}

#[test]
fn extensions_directory_is_only_a_follow_up_parent() {
    let context = test_context();
    let source = test_source(EXTENSIONS_SOURCE_KEY, AgentAssetSourceKind::Directory);
    let snapshot = AgentAssetSnapshot::DirectoryManifest {
        entries: vec![
            crate::services::agent_cli::contracts::AgentAssetDirectoryEntry {
                name: "alpha".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            },
        ],
        revision: Default::default(),
        complete: true,
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert!(output.declarations.is_empty());
    assert!(output.diagnostics.is_empty());
}

#[test]
fn valid_extension_manifest_declares_extension_and_mcp_child() {
    let context = test_context();
    let source = test_source(
        "gemini-extension-manifest:alpha",
        AgentAssetSourceKind::File,
    );
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"{
  "name": "official-alpha",
  "version": " 1.2.3 ",
  "mcpServers": {"ignored": {"command": "secret"}},
  "hooks": {"ignored": []},
  "contextFileName": "../outside"
}"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = ParseCollector::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );

    assert!(output.diagnostics.is_empty());
    assert_eq!(output.declarations.len(), 2);
    let declaration = output
        .declarations
        .iter()
        .find(|value| value.category == AgentAssetCategory::Extension)
        .expect("extension parent");
    assert_eq!(declaration.category, AgentAssetCategory::Extension);
    assert_eq!(declaration.native_id, "alpha");
    assert_eq!(declaration.label, "official-alpha");
    assert_eq!(declaration.resolution_group_key, "alpha");
    assert_eq!(
        declaration.facts,
        BTreeMap::from([(String::from("version"), String::from("1.2.3"))])
    );
    assert!(matches!(
        &declaration.details,
        AgentAssetDetails::Extension {
            install_state: crate::models::AgentAssetInstallState::Installed,
            enabled: AgentAssetDeclaredState::Unknown,
            trusted: AgentTrustState::Unknown,
        }
    ));
    let child = output
        .declarations
        .iter()
        .find(|value| value.category == AgentAssetCategory::Mcp)
        .expect("mcp child");
    assert_eq!(child.native_id, "ignored");
    assert_eq!(
        child
            .provided_by
            .as_ref()
            .map(|value| value.native_id.as_str()),
        Some("alpha")
    );
    assert!(matches!(
        child.native_payload,
        AgentAssetNativePayload::McpDefinition(_)
    ));
}

#[test]
fn invalid_extension_manifests_are_malformed_without_declarations() {
    for bytes in [
        br#"not json"#.as_slice(),
        br#"[]"#.as_slice(),
        br#"{}"#.as_slice(),
        br#"{"name":"alpha"}"#.as_slice(),
        br#"{"name":"alpha","version":false}"#.as_slice(),
        br#"{"name":" ","version":"1.0.0"}"#.as_slice(),
        br#"{"name":" official-alpha","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"official-alpha ","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"official_alpha","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"official/alpha","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"\u5b98\u65b9-alpha","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"","version":"1.0.0"}"#.as_slice(),
        br#"{"name":"alpha","version":" "}"#.as_slice(),
    ] {
        let context = test_context();
        let source = test_source(
            "gemini-extension-manifest:invalid",
            AgentAssetSourceKind::File,
        );
        let snapshot = AgentAssetSnapshot::File {
            bytes: bytes.to_vec(),
            revision: Default::default(),
        };
        let mut output = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert!(output.declarations.is_empty());
        assert_eq!(output.diagnostics.len(), 1);
        assert!(matches!(
            output.diagnostics.first(),
            Some(AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Manifest,
                location: None,
            })
        ));
    }
}

#[path = "tests/gemini_proof.rs"]
mod gemini_proof;
#[path = "tests/local_admin.rs"]
mod local_admin;
#[path = "tests/native_home.rs"]
mod native_home;
#[path = "tests/skill_state.rs"]
mod skill_state;
