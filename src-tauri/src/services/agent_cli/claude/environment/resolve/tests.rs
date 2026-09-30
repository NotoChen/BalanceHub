use super::*;
use crate::models::{AgentAssetDiagnostic, AgentAssetRevision, AgentAssetSourceKind};
use crate::services::agent_cli::contracts::{
    AgentAssetParseRequest, AgentAssetSnapshot, AgentDiagnosticEmission, AgentDiagnosticOutput,
    AgentOutputStop, AgentParseOutput,
};
use crate::services::agent_cli::environment::{source, source_with_logical_origins, SourceInput};
use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Parsed {
    declarations: Vec<ParsedAgentAsset>,
}
impl AgentDiagnosticOutput for Parsed {
    fn has_regular_capacity(&self) -> bool {
        true
    }
    fn emit_diagnostic(&mut self, _: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        AgentDiagnosticEmission::Accepted
    }
}
impl AgentParseOutput for Parsed {
    fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        self.declarations.push(value);
        ControlFlow::Continue(())
    }
}
struct Fixture {
    context: AgentConfigurationContext,
    sources: Vec<AgentAssetSourceSpec>,
    declarations: Vec<ParsedAgentAsset>,
}
impl Fixture {
    fn parse(values: &[(&str, serde_json::Value)]) -> Self {
        Self::snapshots(
            values
                .iter()
                .map(|(key, value)| {
                    (
                        *key,
                        AgentAssetSnapshot::File {
                            bytes: serde_json::to_vec(value).expect("fixture JSON"),
                            revision: AgentAssetRevision::default(),
                        },
                    )
                })
                .collect(),
        )
    }
    fn snapshots(values: Vec<(&str, AgentAssetSnapshot)>) -> Self {
        let context = AgentConfigurationContext {
            id: "claude-proof-context".to_owned(),
            environment_id: "native".to_owned(),
            agent_kind: crate::models::AgentCliKind::ClaudeCode,
            config_root: "/fixture/.claude".to_owned(),
            profile: "default".to_owned(),
            parser_version: 3,
            workspace_id: Some("/fixture/work".to_owned()),
            trust_context: AgentTrustState::Trusted,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        };
        let mut sources = Vec::new();
        let mut parsed = Parsed::default();
        for (key, snapshot) in values {
            let (scope, precedence, categories) = match key {
                "managed-settings" => (
                    AgentAssetScope::Managed,
                    40,
                    vec![
                        AgentAssetCategory::Mcp,
                        AgentAssetCategory::Plugin,
                        AgentAssetCategory::Hook,
                        AgentAssetCategory::StatusUi,
                    ],
                ),
                "settings" => (
                    AgentAssetScope::User,
                    10,
                    vec![
                        AgentAssetCategory::Mcp,
                        AgentAssetCategory::Plugin,
                        AgentAssetCategory::Hook,
                        AgentAssetCategory::StatusUi,
                    ],
                ),
                "workspace-settings" => (
                    AgentAssetScope::Workspace,
                    20,
                    vec![
                        AgentAssetCategory::Mcp,
                        AgentAssetCategory::Plugin,
                        AgentAssetCategory::Hook,
                        AgentAssetCategory::StatusUi,
                    ],
                ),
                "workspace-local-settings" => (
                    AgentAssetScope::Local,
                    30,
                    vec![
                        AgentAssetCategory::Mcp,
                        AgentAssetCategory::Plugin,
                        AgentAssetCategory::Hook,
                        AgentAssetCategory::StatusUi,
                    ],
                ),
                "managed-mcp" => (AgentAssetScope::Managed, 40, vec![AgentAssetCategory::Mcp]),
                "workspace-mcp" => (
                    AgentAssetScope::Workspace,
                    20,
                    vec![AgentAssetCategory::Mcp],
                ),
                "plugin-registry" => (AgentAssetScope::User, 5, vec![AgentAssetCategory::Plugin]),
                value if value.starts_with("skill-manifest:") => {
                    (AgentAssetScope::User, 10, vec![AgentAssetCategory::Skill])
                }
                _ => (AgentAssetScope::User, 10, vec![AgentAssetCategory::Mcp]),
            };
            let input = SourceInput {
                origin: crate::models::AgentAssetInstallationOrigin::Unknown,
                native_source_key: key,
                label: "fixture",
                allowed_root: Path::new("/fixture"),
                path: if key == "plugin-registry" {
                    PathBuf::from("/fixture/.claude/plugins/installed_plugins.json")
                } else {
                    PathBuf::from(format!("/fixture/{key}.json"))
                },
                scope,
                precedence,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: &categories,
            };
            let spec = if key == "account" {
                source_with_logical_origins(
                    input,
                    &[
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::User,
                            precedence: 10,
                        },
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Local,
                            precedence: 30,
                        },
                    ],
                )
            } else if key == "plugin-registry" {
                source_with_logical_origins(
                    input,
                    &[
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::User,
                            precedence: 10,
                        },
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Workspace,
                            precedence: 20,
                        },
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Local,
                            precedence: 30,
                        },
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Managed,
                            precedence: 40,
                        },
                    ],
                )
            } else {
                source(input)
            };
            super::super::parse::parse_assets(
                AgentAssetParseRequest {
                    native_home: None,
                    context: &context,
                    source: &spec,
                    snapshot: &snapshot,
                    workspace_canonical: Some(Path::new("/fixture/work")),
                    workspace_lexical: None,
                },
                &mut parsed,
            );
            sources.push(spec);
        }
        Self {
            context,
            sources,
            declarations: parsed.declarations,
        }
    }
    fn index(&self) -> NativeIndex<'_> {
        NativeIndex::new(&self.context, &self.declarations, &self.sources)
    }
    fn target(category: AgentAssetCategory, id: &str) -> AgentAssetAssessmentTarget {
        AgentAssetAssessmentTarget {
            category,
            resolution_group_key: id.to_owned(),
            exact_native_id: id.to_owned(),
            subject: AgentAssetAssessmentSubject::Bucket,
        }
    }
    fn decision(&self, category: AgentAssetCategory, id: &str) -> NativeDecision<'_> {
        self.index()
            .decide(&Self::target(category, id))
            .unwrap_or_else(|failure| panic!("native fixture failed: {failure:?}"))
    }
}
fn account() -> serde_json::Value {
    serde_json::json!({ "mcpServers": { "alpha": {"command": "fixture-node", "args": ["fixture-script"]} } })
}
fn registry() -> serde_json::Value {
    serde_json::json!({"version":2,"plugins":{"demo@fixture":[{"scope":"user","installPath":"/fixture/demo"}]}})
}
fn terminal_evidence(decision: &NativeDecision<'_>) -> usize {
    match &decision.effective {
        AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } => evidence.len(),
        _ => 0,
    }
}

#[test]
fn allowlist_fail_closed_keeps_complete_original_membership() {
    for value in [
        serde_json::json!(false),
        serde_json::json!([]),
        serde_json::json!([{"serverName":"alpha"}, {"serverCommand":5}]),
    ] {
        let expected = value.as_array().map_or(0, Vec::len) + 1;
        let fixture = Fixture::parse(&[
            ("account", account()),
            ("settings", serde_json::json!({"allowedMcpServers": value})),
        ]);
        let decision = fixture.decision(AgentAssetCategory::Mcp, "alpha");
        assert_eq!(
            decision.assessment.declared_state,
            AgentAssetDeclaredState::Enabled
        );
        assert_eq!(
            decision.resolution.terminal,
            Some(AgentAssetResolutionTerminal::PolicyBlocked)
        );
        assert_eq!(terminal_evidence(&decision), expected);
        assert_eq!(
            decision.assessment.control.as_ref().unwrap().authorities,
            BTreeSet::from([AgentAssetControlAuthority::SourceAggregate(
                "settings".to_owned()
            )])
        );
        assert_eq!(
            decision.finalize().unwrap().effective_state,
            crate::models::AgentAssetState::Blocked
        );
    }
}

#[test]
fn invalid_denied_and_independent_same_source_fields_keep_distinct_authorities() {
    for (settings, authority_count, evidence_count) in [
        (
            serde_json::json!({"deniedMcpServers":[{"serverCommand":5}]}),
            1,
            2,
        ),
        (
            serde_json::json!({"deniedMcpServers":[{"serverCommand":5}],"enableAllProjectMcpServers":"bad"}),
            2,
            3,
        ),
    ] {
        let fixture = Fixture::parse(&[("account", account()), ("settings", settings)]);
        let decision = fixture.decision(AgentAssetCategory::Mcp, "alpha");
        assert_eq!(
            decision.resolution.terminal,
            Some(AgentAssetResolutionTerminal::Unknown)
        );
        let control = decision.assessment.control.as_ref().unwrap();
        assert_eq!(control.cause, AgentAssetTerminalCauseDraft::InvalidControl);
        assert_eq!(control.authorities.len(), authority_count);
        assert_eq!(terminal_evidence(&decision), evidence_count);
        assert_eq!(
            decision.resolution.control_source.is_some(),
            authority_count == 1
        );
        assert_eq!(
            decision.assessment.declared_state,
            AgentAssetDeclaredState::Enabled
        );
    }
}

#[test]
fn command_and_url_deny_select_actual_first_matching_rule() {
    for (definition, rules) in [
        (
            serde_json::json!({"command":"fixture-node","args":["fixture-script"]}),
            serde_json::json!([{"serverName":"other"},{"serverCommand":["fixture-node","fixture-script"]},{"serverName":"alpha"}]),
        ),
        (
            serde_json::json!({"url":"https://fixture.test/path","type":"http"}),
            serde_json::json!([{"serverUrl":"https://other.test/*"},{"serverUrl":"https://fixture.test/*"},{"serverName":"alpha"}]),
        ),
    ] {
        let fixture = Fixture::parse(&[
            (
                "account",
                serde_json::json!({"mcpServers":{"alpha":definition}}),
            ),
            ("settings", serde_json::json!({"deniedMcpServers":rules})),
        ]);
        let decision = fixture.decision(AgentAssetCategory::Mcp, "alpha");
        let selected = fixture
            .declarations
            .iter()
            .find(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::RuleEntry {
                        family: ListFamily::Denied,
                        ordinal: 1,
                        ..
                    })
                )
            })
            .unwrap();
        assert!(
            matches!(&decision.resolution.control_source, Some(AgentAssetPolicyReferenceDraft::Declaration { declaration_id }) if declaration_id == &selected.declaration_id)
        );
        assert_eq!(terminal_evidence(&decision), 1);
    }
}

#[test]
fn counted_roots_reject_incomplete_and_contradictory_inputs_before_defaulting() {
    let base = || {
        Fixture::parse(&[
            ("account", account()),
            (
                "settings",
                serde_json::json!({"allowedMcpServers":[{"serverName":"alpha"},{"serverName":"other"}]}),
            ),
        ])
    };
    for mutation in 0..8 {
        let mut fixture = base();
        let root = fixture
            .declarations
            .iter()
            .position(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::ListRoot {
                        family: ListFamily::Allowed,
                        ..
                    })
                )
            })
            .unwrap();
        let entry = fixture
            .declarations
            .iter()
            .position(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::RuleEntry {
                        family: ListFamily::Allowed,
                        ordinal: 0,
                        ..
                    })
                )
            })
            .unwrap();
        let expected =
            match mutation {
                0 => {
                    fixture.declarations.remove(root);
                    AgentAssetAssessmentFailure::IncompleteInput
                }
                1 => {
                    fixture.declarations.remove(entry);
                    AgentAssetAssessmentFailure::IncompleteInput
                }
                2 => {
                    fixture
                        .declarations
                        .push(fixture.declarations[root].clone());
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
                3 => {
                    fixture.declarations[root].declaration_id.push_str("-wrong");
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
                4 => {
                    fixture.declarations[entry].logical_origin.precedence = 90;
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
                5 => {
                    if let AgentAssetNativePayload::ClaudeControl(
                        ClaudeControlPayload::RuleEntry { ordinal, .. },
                    ) = &mut fixture.declarations[entry].native_payload
                    {
                        *ordinal = 7;
                    }
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
                6 => {
                    fixture.declarations[entry].role = AgentAssetDeclarationRole::StateOverlay;
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
                _ => {
                    if let AgentAssetNativePayload::ClaudeControl(
                        ClaudeControlPayload::ListRoot { state, .. },
                    ) = &mut fixture.declarations[root].native_payload
                    {
                        *state = ListState::Absent;
                    }
                    AgentAssetAssessmentFailure::InvalidNativeInput
                }
            };
        assert!(
            matches!(fixture.index().decide(&Fixture::target(AgentAssetCategory::Mcp,"alpha")), Err(failure) if failure == expected),
            "case {mutation}"
        );
    }
}

#[test]
fn plugin_global_invalid_preserves_exact_declared_axis() {
    for enabled in [true, false] {
        for bad in [
            AgentAssetSnapshot::File {
                bytes: b"{".to_vec(),
                revision: AgentAssetRevision::default(),
            },
            AgentAssetSnapshot::File {
                bytes: br#"{"enabledPlugins": false}"#.to_vec(),
                revision: AgentAssetRevision::default(),
            },
            AgentAssetSnapshot::Blocked {
                revision: AgentAssetRevision::default(),
                diagnostic: AgentAssetDiagnostic::ReadFailed {
                    source_id: "managed-settings".to_owned(),
                    error_kind: crate::models::AgentAssetIoErrorKind::Other,
                },
            },
        ] {
            let fixture = Fixture::snapshots(vec![
                (
                    "plugin-registry",
                    AgentAssetSnapshot::File {
                        bytes: serde_json::to_vec(&registry()).unwrap(),
                        revision: AgentAssetRevision::default(),
                    },
                ),
                (
                    "settings",
                    AgentAssetSnapshot::File {
                        bytes: serde_json::to_vec(
                            &serde_json::json!({"enabledPlugins":{"demo@fixture":enabled}}),
                        )
                        .unwrap(),
                        revision: AgentAssetRevision::default(),
                    },
                ),
                ("managed-settings", bad),
            ]);
            let decision = fixture.decision(AgentAssetCategory::Plugin, "demo@fixture");
            assert_eq!(
                decision.assessment.declared_state,
                if enabled {
                    AgentAssetDeclaredState::Enabled
                } else {
                    AgentAssetDeclaredState::Disabled
                }
            );
            assert!(matches!(
                decision.assessment.declared,
                AgentAssetDeclaredStateProofDraft::Overlay { .. }
            ));
            assert_eq!(terminal_evidence(&decision), 1);
            let control = decision.assessment.control.as_ref().unwrap();
            assert!(control.members.iter().all(|material| !decision
                .assessment
                .declared_members
                .iter()
                .any(|declared| declared.declaration_id == material.declaration_id)));
            assert!(matches!(
                decision.resolution.control_source,
                Some(AgentAssetPolicyReferenceDraft::Declaration { .. })
            ));
            assert_eq!(
                decision.finalize().unwrap().effective_state,
                crate::models::AgentAssetState::Unknown
            );
        }
    }
}

#[test]
fn plugin_invalid_member_is_intrinsic_and_never_invents_an_installation() {
    let fixture = Fixture::parse(&[(
        "settings",
        serde_json::json!({"enabledPlugins":{"demo@fixture":"bad"}}),
    )]);
    assert!(fixture.index().targets().is_empty());
    let fixture = Fixture::parse(&[
        ("plugin-registry", registry()),
        (
            "settings",
            serde_json::json!({"enabledPlugins":{"demo@fixture":"bad"}}),
        ),
    ]);
    let decision = fixture.decision(AgentAssetCategory::Plugin, "demo@fixture");
    assert!(matches!(
        decision.assessment.declared,
        AgentAssetDeclaredStateProofDraft::Unknown {
            cause: AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl,
            ..
        }
    ));
    assert_eq!(
        decision.effective,
        AgentAssetEffectiveStateProofDraft::Intrinsic
    );
    assert!(decision.resolution.control_source.is_none());
    assert_eq!(fixture.index().targets().len(), 1);
}

#[test]
fn approval_union_rejection_and_transport_survive_terminal_control() {
    for (settings, local, expected) in [
        (
            serde_json::json!({}),
            serde_json::json!({}),
            AgentMcpApprovalState::Pending,
        ),
        (
            serde_json::json!({"enabledMcpjsonServers":["alpha"]}),
            serde_json::json!({"enabledMcpjsonServers":["other"]}),
            AgentMcpApprovalState::Approved,
        ),
        (
            serde_json::json!({"disabledMcpjsonServers":["alpha"]}),
            serde_json::json!({"enabledMcpjsonServers":["alpha"]}),
            AgentMcpApprovalState::Rejected,
        ),
    ] {
        let fixture = Fixture::parse(&[
            (
                "workspace-mcp",
                serde_json::json!({"mcpServers":{"alpha":{"command":"fixture-node"}}}),
            ),
            ("settings", settings),
            ("workspace-local-settings", local),
            (
                "managed-settings",
                serde_json::json!({"allowedMcpServers":[]}),
            ),
        ]);
        let decision = fixture.decision(AgentAssetCategory::Mcp, "alpha");
        assert!(
            matches!(decision.assessment.intrinsic.details,AgentAssetDetails::Mcp {approval_state,transport:AgentMcpTransport::Stdio,..} if approval_state == expected)
        );
        let draft = decision.finalize().unwrap();
        assert!(
            matches!(draft.details,AgentAssetDetails::Mcp {approval_state,effective_availability:crate::models::AgentAssetEffectiveAvailability::PolicyBlocked,..} if approval_state == expected)
        );
    }
}

#[test]
fn managed_invalid_guards_remain_native_fail_closed_field_policies() {
    let fixture = Fixture::parse(&[
        ("account", account()),
        (
            "settings",
            serde_json::json!({
                "hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"fixture-hook"}]}]},"statusLine":{"type":"command","command":"fixture-status"}
            }),
        ),
        (
            "managed-settings",
            serde_json::json!({"allowManagedMcpServersOnly":"bad","allowManagedHooksOnly":null}),
        ),
    ]);
    let index = fixture.index();
    for target in index.targets() {
        let decision = index.decide(&target).unwrap();
        if target.category == AgentAssetCategory::StatusUi {
            assert!(decision.resolution.terminal.is_none());
        } else {
            assert_eq!(
                decision.resolution.terminal,
                Some(AgentAssetResolutionTerminal::PolicyBlocked)
            );
            assert!(matches!(
                decision.resolution.control_source,
                Some(AgentAssetPolicyReferenceDraft::Declaration { .. })
            ));
        }
    }
    assert_eq!(index.targets().len(), 3);
}

#[test]
fn plugin_structural_tie_cannot_be_repaired_by_enabled_overlay() {
    let fixture = Fixture::parse(&[
        (
            "plugin-registry",
            serde_json::json!({"version":2,"plugins":{"demo@fixture":[
            {"scope":"user","installPath":"/fixture/demo-a"},
            {"scope":"user","installPath":"/fixture/demo-b"}]}}),
        ),
        (
            "settings",
            serde_json::json!({"enabledPlugins":{"demo@fixture":true}}),
        ),
    ]);
    let decision = fixture.decision(AgentAssetCategory::Plugin, "demo@fixture");
    assert_eq!(fixture.index().targets().len(), 1);
    assert_eq!(
        decision.resolution.relation,
        AgentAssetResolutionRelation::Unknown
    );
    assert_eq!(
        decision.assessment.declared_state,
        AgentAssetDeclaredState::Unknown
    );
    assert!(matches!(
        decision.assessment.declared,
        AgentAssetDeclaredStateProofDraft::Unknown {
            cause: AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
            ..
        }
    ));
    assert_eq!(decision.assessment.declared_members.len(), 2);
    assert!(decision
        .assessment
        .declared_members
        .iter()
        .all(|member| member.kind == AgentAssetEvidenceKind::Definition));
    assert_eq!(
        decision.finalize().unwrap().effective_state,
        crate::models::AgentAssetState::Unknown
    );
}

#[test]
fn plugin_replacement_loser_keeps_own_basis_and_shadowed_route() {
    let fixture = Fixture::parse(&[
        (
            "plugin-registry",
            serde_json::json!({"version":2,"plugins":{"demo@fixture":[
            {"scope":"user","installPath":"/fixture/demo-user"},
            {"scope":"local","projectPath":"/fixture/work","installPath":"/fixture/demo-local"}]}}),
        ),
        (
            "settings",
            serde_json::json!({"enabledPlugins":{"demo@fixture":false}}),
        ),
        (
            "managed-settings",
            serde_json::json!({"enabledPlugins":false}),
        ),
    ]);
    let index = fixture.index();
    assert_eq!(index.targets().len(), 2);
    let winner = fixture.decision(AgentAssetCategory::Plugin, "demo@fixture");
    assert_eq!(
        winner.assessment.declared_state,
        AgentAssetDeclaredState::Disabled
    );
    assert_eq!(
        winner.resolution.terminal,
        Some(AgentAssetResolutionTerminal::Unknown)
    );
    let loser_target = index
        .targets()
        .into_iter()
        .find(|target| matches!(target.subject, AgentAssetAssessmentSubject::Definition(_)))
        .unwrap();
    let loser = index.decide(&loser_target).unwrap();
    assert_eq!(loser.anchor.logical_origin.scope, AgentAssetScope::User);
    assert_eq!(
        loser.assessment.declared_state,
        AgentAssetDeclaredState::Unknown
    );
    assert!(loser.assessment.control.is_none());
    assert!(
        matches!(loser.effective,AgentAssetEffectiveStateProofDraft::Shadowed {ref input,..} if matches!(input.as_ref(),AgentAssetEffectiveStateProofDraft::Intrinsic))
    );
    assert_eq!(
        loser.finalize().unwrap().effective_state,
        crate::models::AgentAssetState::Shadowed
    );
}

#[test]
fn personal_and_plugin_conflicts_keep_all_decisive_peers_on_declared_axis() {
    let mut fixture = Fixture::parse(&[
        (
            "account",
            serde_json::json!({"mcpServers":{"alpha":{"command":"runner"}},"projects":{"/fixture/work":{
            "enabledMcpServers":["alpha"],"disabledMcpServers":["alpha"]}}}),
        ),
        ("plugin-registry", registry()),
        (
            "settings",
            serde_json::json!({"enabledPlugins":{"demo@fixture":true}}),
        ),
        (
            "workspace-settings",
            serde_json::json!({"enabledPlugins":{"demo@fixture":false}}),
        ),
    ]);
    // The source port admits a same-native-level workspace setting fixture;
    // the parsed payload and original array ordinals are unchanged.
    let source = fixture
        .sources
        .iter_mut()
        .find(|source| source.native_source_key == "workspace-settings")
        .unwrap();
    source.precedence = 10;
    source
        .allowed_logical_origins
        .iter_mut()
        .for_each(|origin| origin.precedence = 10);
    fixture
        .declarations
        .iter_mut()
        .filter(|asset| asset.source_key == "workspace-settings")
        .for_each(|asset| asset.logical_origin.precedence = 10);
    for (category, id) in [
        (AgentAssetCategory::Mcp, "alpha"),
        (AgentAssetCategory::Plugin, "demo@fixture"),
    ] {
        let decision = fixture.decision(category, id);
        assert_eq!(
            decision.assessment.declared_state,
            AgentAssetDeclaredState::Unknown
        );
        assert_eq!(decision.assessment.declared_members.len(), 2);
        assert!(matches!(
            decision.assessment.declared,
            AgentAssetDeclaredStateProofDraft::Unknown {
                cause: AgentAssetDeclaredUnknownCauseDraft::OverlayConflict,
                ..
            }
        ));
        assert!(decision.assessment.control.is_none());
        assert!(decision.resolution.control_source.is_none());
        if category == AgentAssetCategory::Plugin {
            assert_eq!(
                decision.effective,
                AgentAssetEffectiveStateProofDraft::Intrinsic
            );
        }
    }
}
