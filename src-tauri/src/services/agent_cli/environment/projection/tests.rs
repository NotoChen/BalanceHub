use super::fixture::fixture_assessor;
use super::*;
use crate::models::{
    AgentAssetActionKind, AgentAssetActionUnavailableReason, AgentAssetEffectiveAvailability,
    AgentAssetPresence, AgentAssetResolutionTerminal, AgentAssetScope, AgentAssetSourceKind,
    AgentEnvironmentKind, AgentHostPlatform, AgentMcpApprovalState, AgentMcpTransport,
    AgentSkillInvocationPolicy, AgentStatusUiMode,
};
use crate::services::agent_cli::contracts::{
    AgentAssetDeclaredStateProofDraft, AgentAssetDeclaredUnknownCauseDraft,
    AgentAssetEffectiveStateProofDraft, AgentAssetNativePayload, AgentAssetResolutionDraft,
    AgentAssetStateEvidenceRefDraft, AgentAssetStateProofDraft, AgentAssetTerminalCauseDraft,
};
use crate::services::agent_cli::environment::run::ManualClock;
use std::path::PathBuf;
use std::sync::Arc;

fn context() -> AgentConfigurationContext {
    AgentConfigurationContext {
        id: "context:test".to_string(),
        environment_id: "native:test".to_string(),
        agent_kind: crate::models::AgentCliKind::Codex,
        config_root: "/tmp/balancehub-fixture".to_string(),
        profile: "default".to_string(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: 1,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    }
}

fn environment() -> AgentEnvironmentDescriptor {
    AgentEnvironmentDescriptor {
        id: "native:test".to_string(),
        kind: AgentEnvironmentKind::Native,
        host_platform: AgentHostPlatform::Macos,
        host_architecture: crate::models::AgentHostArchitecture::Aarch64,
        guest_platform: None,
        display_name: "fixture".to_string(),
        capabilities: Vec::new(),
    }
}

fn source(key: &str, suffix: &str, categories: &[AgentAssetCategory]) -> AgentAssetSourceSpec {
    AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: key.to_string(),
        label: key.to_string(),
        scope: AgentAssetScope::User,
        path: PathBuf::from(format!("/tmp/balancehub-fixture/{suffix}")),
        allowed_root: PathBuf::from("/tmp/balancehub-fixture"),
        precedence: 10,
        writable: true,
        sensitive: false,
        source_kind: AgentAssetSourceKind::File,
        categories: categories.to_vec(),
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    }
}

fn declaration(
    context: &AgentConfigurationContext,
    source_key: &str,
    native_id: &str,
    category: AgentAssetCategory,
    declared_state: AgentAssetDeclaredState,
) -> ParsedAgentAsset {
    let category_key = category.key();
    ParsedAgentAsset {
        declaration_id: stable_id(
            "declaration",
            &[
                context.id.as_str(),
                source_key,
                category_key.as_str(),
                native_id,
            ],
        ),
        resolution_group_key: native_id.to_string(),
        source_key: source_key.to_string(),
        logical_origin: crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
            scope: AgentAssetScope::User,
            precedence: 10,
        },
        native_id: native_id.to_string(),
        declaration_key: native_id.to_string(),
        label: native_id.to_string(),
        category,
        presence: AgentAssetPresence::Present,
        declared_state,
        trust_state: AgentTrustState::Unknown,
        role: crate::models::AgentAssetDeclarationRole::Definition,
        participation: crate::models::AgentAssetResolutionParticipation::Participates,
        provided_by: None,
        action_owner: None,
        explicitly_affected: Vec::new(),
        details: details(category, declared_state),
        facts: BTreeMap::new(),
        native_payload: crate::services::agent_cli::contracts::AgentAssetNativePayload::None,
    }
}

fn declaration_with_key(
    context: &AgentConfigurationContext,
    source_key: &str,
    native_id: &str,
    declaration_key: &str,
    category: AgentAssetCategory,
    declared_state: AgentAssetDeclaredState,
) -> ParsedAgentAsset {
    let mut value = declaration(context, source_key, native_id, category, declared_state);
    value.declaration_key = declaration_key.to_string();
    value.declaration_id = stable_id(
        "declaration",
        &[
            context.id.as_str(),
            source_key,
            category.key().as_str(),
            declaration_key,
        ],
    );
    value
}

fn prefer_definition(sources: &mut [AgentAssetSourceSpec], definition: &mut ParsedAgentAsset) {
    definition.logical_origin.precedence = 20;
    let source = sources
        .iter_mut()
        .find(|source| source.native_source_key == definition.source_key)
        .unwrap();
    source
        .allowed_logical_origins
        .push(definition.logical_origin);
}

fn group_scoped_overlay(overlay: &mut ParsedAgentAsset) {
    fixture_input(overlay, "group_scope = true");
}

/// Native fixture configuration is written to parser input before proposing a
/// projection. This helper has no draft/proof/expected-ID input.
fn fixture_input(asset: &mut ParsedAgentAsset, input: &str) {
    let fields: toml::map::Map<String, toml::Value> = toml::from_str(input).unwrap();
    match &mut asset.native_payload {
        AgentAssetNativePayload::None => {
            asset.native_payload = AgentAssetNativePayload::TomlTable(fields);
        }
        AgentAssetNativePayload::TomlTable(table) => table.extend(fields),
        _ => panic!("fixture input cannot overwrite native typed data"),
    }
}

fn details(
    category: AgentAssetCategory,
    declared_state: AgentAssetDeclaredState,
) -> AgentAssetDetails {
    match category {
        AgentAssetCategory::Skill => AgentAssetDetails::Skill {
            enabled: declared_state,
            invocation_policy: AgentSkillInvocationPolicy::Unknown,
        },
        AgentAssetCategory::Mcp => AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: match declared_state {
                AgentAssetDeclaredState::Enabled => AgentAssetEffectiveAvailability::Available,
                AgentAssetDeclaredState::Disabled => AgentAssetEffectiveAvailability::Disabled,
                AgentAssetDeclaredState::Pending => {
                    AgentAssetEffectiveAvailability::ApprovalRequired
                }
                AgentAssetDeclaredState::Rejected => AgentAssetEffectiveAvailability::PolicyBlocked,
                AgentAssetDeclaredState::Unknown => AgentAssetEffectiveAvailability::Unknown,
            },
        },
        AgentAssetCategory::Plugin => AgentAssetDetails::Plugin {
            install_state: crate::models::AgentAssetInstallState::Installed,
            enabled: declared_state,
            trusted: AgentTrustState::Unknown,
        },
        AgentAssetCategory::Extension => AgentAssetDetails::Extension {
            install_state: crate::models::AgentAssetInstallState::Installed,
            enabled: declared_state,
            trusted: AgentTrustState::Unknown,
        },
        AgentAssetCategory::Hook => AgentAssetDetails::Hook {
            managed: false,
            enabled: declared_state,
            rule_count: None,
        },
        AgentAssetCategory::StatusUi => AgentAssetDetails::StatusUi {
            mode: match declared_state {
                AgentAssetDeclaredState::Enabled => AgentStatusUiMode::BuiltIn,
                AgentAssetDeclaredState::Disabled => AgentStatusUiMode::Disabled,
                _ => AgentStatusUiMode::Unknown,
            },
            command_present: false,
        },
    }
}

fn draft(
    asset: &ParsedAgentAsset,
    key: &str,
    kind: AgentAssetResolutionRelation,
    contributors: &[&ParsedAgentAsset],
) -> AgentAssetProjectedDraft {
    let effective_state = match kind {
        AgentAssetResolutionRelation::Replaced => AgentAssetState::Shadowed,
        AgentAssetResolutionRelation::Unknown => AgentAssetState::Unknown,
        _ => state_from_declared(asset.declared_state),
    };
    let details = match &asset.details {
        AgentAssetDetails::Mcp {
            transport,
            declared_state,
            approval_state,
            effective_availability,
        } => AgentAssetDetails::Mcp {
            transport: *transport,
            declared_state: *declared_state,
            approval_state: *approval_state,
            effective_availability: match kind {
                AgentAssetResolutionRelation::Unknown => AgentAssetEffectiveAvailability::Unknown,
                _ => *effective_availability,
            },
        },
        details => details.clone(),
    };
    AgentAssetProjectedDraft {
        projection_key: key.to_string(),
        resolution_group_key: asset.resolution_group_key.clone(),
        native_kind: asset.category,
        native_id: asset.native_id.clone(),
        label: asset.label.clone(),
        declared_state: asset.declared_state,
        effective_state,
        trust_state: asset.trust_state,
        inspection_source_id: asset.source_key.clone(),
        represented_declaration_ids: contributors
            .iter()
            .map(|value| value.declaration_id.clone())
            .collect(),
        contributor_ids: contributors
            .iter()
            .map(|value| value.declaration_id.clone())
            .collect(),
        resolution: AgentAssetResolutionDraft {
            relation: kind,
            qualified_collision: false,
            terminal: None,
            winner: None,
            control_source: None,
        },
        details,
        provided_by: asset.provided_by.clone(),
        action_owner: asset.action_owner.clone(),
        explicitly_affected: asset.explicitly_affected.clone(),
        state_proof: AgentAssetStateProofDraft {
            declared: if asset.declared_state == AgentAssetDeclaredState::Unknown {
                AgentAssetDeclaredStateProofDraft::Unknown {
                    evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                        declaration_id: asset.declaration_id.clone(),
                    }],
                    cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
                }
            } else {
                AgentAssetDeclaredStateProofDraft::Definition {
                    declaration_ids: vec![asset.declaration_id.clone()],
                    selected_id: asset.declaration_id.clone(),
                }
            },
            effective: AgentAssetEffectiveStateProofDraft::Intrinsic,
        },
    }
}

#[derive(Debug)]
struct ProjectTestResult {
    records: Vec<AgentAssetRecord>,
    diagnostics: Vec<AgentAssetDiagnostic>,
}

fn assert_rows(result: &ProjectTestResult, expected: &[&str]) {
    let mut actual = result
        .records
        .iter()
        .map(|record| record.native_id.as_str())
        .collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected, "{:?}", result.diagnostics);
}

fn assert_rejected_with_neighbor(result: &ProjectTestResult) {
    assert_rows(result, &["neighbor"]);
    assert!(
        result.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
        )),
        "{:?}",
        result.diagnostics
    );
}

fn project(
    sources: &[AgentAssetSourceSpec],
    declarations: &[ParsedAgentAsset],
    drafts: Vec<AgentAssetProjectedDraft>,
) -> ProjectTestResult {
    let mut run = AgentInventoryRun::with_clock(
        crate::models::AgentAssetLimits::DEFAULT,
        Arc::new(ManualClock::new()),
    );
    let result = project_records(
        &environment(),
        &context(),
        sources,
        declarations,
        AgentAssetProjectionInput {
            drafts,
            state_assessor: fixture_assessor,
        },
        &BTreeMap::new(),
        &mut run,
    );
    ProjectTestResult {
        records: result.records,
        diagnostics: run.finish_diagnostics(),
    }
}

#[test]
fn rejects_native_id_alias_and_missing_inspection_source() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let declarations = vec![declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut invalid = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[0]],
    );
    invalid.inspection_source_id = "missing".to_string();
    let result = project(&sources, &declarations, vec![invalid]);
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidProjection { .. })));
}

#[test]
fn supports_all_resolution_relations_and_terminal_axes_without_dangling_ids() {
    let ctx = context();
    let mut sources = (1..=12)
        .map(|index| {
            source(
                &format!("s{index}"),
                &format!("s{index}.json"),
                &[AgentAssetCategory::Mcp],
            )
        })
        .collect::<Vec<_>>();
    let mut declarations = vec![
        declaration(
            &ctx,
            "s1",
            "independent",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s2",
            "additive",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s3",
            "additive",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "s4",
            "collision",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "s5",
            "collision",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s6",
            "merged",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s7",
            "merged",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s8",
            "winner",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s9",
            "winner",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "s10",
            "policy",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "s11",
            "unknown",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Unknown,
        ),
        declaration(
            &ctx,
            "s12",
            "policy-target",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    for index in [1, 2] {
        fixture_input(&mut declarations[index], "fixture_mode = 'additive'");
    }
    for index in [5, 6] {
        fixture_input(&mut declarations[index], "fixture_mode = 'merge'");
    }
    for index in [10, 11] {
        fixture_input(&mut declarations[index], "fixture_mode = 'unresolved'");
    }
    prefer_definition(&mut sources, &mut declarations[7]);
    let mut policy_declaration = declarations[9].clone();
    policy_declaration.native_id = "policy-target".to_owned();
    policy_declaration.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    fixture_input(
        &mut policy_declaration,
        "source_aggregate = true
block_targets = ['policy-target']",
    );
    policy_declaration.resolution_group_key = "policy".to_string();
    declarations[9] = policy_declaration;
    declarations[11].resolution_group_key = "policy".to_string();
    let mut drafts = vec![
        draft(
            &declarations[0],
            "mcp:independent",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[0]],
        ),
        draft(
            &declarations[1],
            "mcp:additive@s2",
            AgentAssetResolutionRelation::Additive,
            &[&declarations[1]],
        ),
        draft(
            &declarations[2],
            "mcp:additive@s3",
            AgentAssetResolutionRelation::Additive,
            &[&declarations[2]],
        ),
        draft(
            &declarations[3],
            "mcp:collision@s4",
            AgentAssetResolutionRelation::Unknown,
            &[&declarations[3]],
        ),
        draft(
            &declarations[4],
            "mcp:collision@s5",
            AgentAssetResolutionRelation::Unknown,
            &[&declarations[4]],
        ),
        draft(
            &declarations[5],
            "mcp:merged",
            AgentAssetResolutionRelation::Merged,
            &[&declarations[5], &declarations[6]],
        ),
        draft(
            &declarations[7],
            "mcp:winner",
            AgentAssetResolutionRelation::ReplaceWinner,
            &[&declarations[7], &declarations[8]],
        ),
        draft(
            &declarations[8],
            "mcp:winner@s9",
            AgentAssetResolutionRelation::Replaced,
            &[&declarations[8]],
        ),
        draft(
            &declarations[11],
            "mcp:policy",
            AgentAssetResolutionRelation::Unknown,
            &[&declarations[11], &declarations[9]],
        ),
        draft(
            &declarations[10],
            "mcp:unknown",
            AgentAssetResolutionRelation::Unknown,
            &[&declarations[10]],
        ),
    ];
    let winner_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "winner".to_string(),
        qualifier: Some("mcp:winner".to_string()),
    };
    drafts[3].represented_declaration_ids = vec![
        declarations[3].declaration_id.clone(),
        declarations[4].declaration_id.clone(),
    ];
    drafts[3].contributor_ids = drafts[3].represented_declaration_ids.clone();
    drafts[3].declared_state = AgentAssetDeclaredState::Unknown;
    drafts[3].effective_state = AgentAssetState::Unknown;
    drafts[3].resolution.terminal = Some(crate::models::AgentAssetResolutionTerminal::Unknown);
    drafts[3].state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
        evidence: vec![
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declarations[3].declaration_id.clone(),
            },
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declarations[4].declaration_id.clone(),
            },
        ],
        cause: crate::services::agent_cli::contracts::AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
    };
    if let AgentAssetDeclaredStateProofDraft::Unknown { evidence, .. } =
        &mut drafts[3].state_proof.declared
    {
        evidence.sort();
    }
    drafts[3].state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause:
            crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::StructuralUnknown,
        evidence: Vec::new(),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    drafts[3].details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Unknown,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Unknown,
    };
    drafts.remove(4);
    drafts[5].resolution.winner = Some(winner_reference.clone());
    drafts[6].resolution.winner = Some(winner_reference);
    drafts[6].state_proof.effective = AgentAssetEffectiveStateProofDraft::Shadowed {
        winner: drafts[6]
            .resolution
            .winner
            .clone()
            .expect("winner proof reference"),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    drafts[7].resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "s10".to_string(),
    });
    drafts[7].resolution.terminal =
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked);
    drafts[7].state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Source {
            source_key: "s10".to_string(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    drafts[7].effective_state = AgentAssetState::Blocked;
    drafts[7].details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };

    let result = project(&sources, &declarations, drafts);
    let nested_diagnostic_count = result
        .records
        .iter()
        .map(|record| record.diagnostics.len() + record.resolution.diagnostics.len())
        .sum::<usize>();
    assert_eq!(result.records.len(), 9, "{:?}", result.diagnostics);
    assert_eq!(nested_diagnostic_count, 1, "{:?}", result.diagnostics);
    let policy = result
        .records
        .iter()
        .find(|record| {
            record.resolution.relation == AgentAssetResolutionRelation::Unknown
                && record.resolution.terminal == Some(AgentAssetResolutionTerminal::PolicyBlocked)
                && matches!(
                    record.resolution.control_source,
                    Some(AgentAssetPolicyReference::Source { .. })
                )
        })
        .expect("policy-blocked record");
    assert!(policy
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::PolicyBlocked)));
    assert!(!policy
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
    assert!(matches!(
        policy.resolution.control_source,
        Some(AgentAssetPolicyReference::Source { .. })
    ));
    assert_eq!(
        result
            .records
            .iter()
            .filter(|record| {
                record.resolution.relation == AgentAssetResolutionRelation::Unknown
                    && record.resolution.terminal == Some(AgentAssetResolutionTerminal::Unknown)
            })
            .count(),
        1
    );
    let unknown = result
        .records
        .iter()
        .find(|record| {
            record.resolution.relation == AgentAssetResolutionRelation::Unknown
                && record.resolution.terminal == Some(AgentAssetResolutionTerminal::Unknown)
                && record.resolution.control_source.is_none()
        })
        .expect("unknown-resolution record");
    assert!(unknown.resolution.diagnostics.is_empty());
    let relations = result
        .records
        .iter()
        .map(|record| format!("{:?}", record.resolution.relation))
        .collect::<BTreeSet<_>>();
    assert_eq!(relations.len(), 6);
    let ids = result
        .records
        .iter()
        .map(|record| record.stable_id.as_str())
        .collect::<BTreeSet<_>>();
    for record in &result.records {
        for target in record.resolution.winner_id.iter() {
            assert!(ids.contains(target.as_str()));
        }
    }
    let winner = result
        .records
        .iter()
        .find(|record| record.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .unwrap();
    assert_eq!(
        winner.represented_declaration_ids,
        vec![
            declarations[7].declaration_id.clone(),
            declarations[8].declaration_id.clone()
        ]
    );
    assert_eq!(
        result
            .records
            .iter()
            .filter(|record| record.resolution.relation == AgentAssetResolutionRelation::Additive)
            .count(),
        2
    );
    assert_eq!(
        result
            .records
            .iter()
            .filter(|record| {
                record.resolution.relation == AgentAssetResolutionRelation::Unknown
            })
            .count(),
        3
    );
}

#[test]
fn unknown_resolution_requires_a_diagnostic() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let mut declarations = vec![declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    )];
    fixture_input(&mut declarations[0], "fixture_mode = 'unresolved'");
    let unknown = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&declarations[0]],
    );
    let result = project(&sources, &declarations, vec![unknown]);
    assert_eq!(result.records.len(), 1);
    assert!(result.records[0].resolution.diagnostics.is_empty());
}

#[test]
fn control_source_rejects_missing_self_ambiguous_and_rejected_targets() {
    let ctx = context();

    let missing_sources = vec![source(
        "blocked",
        "blocked.json",
        &[AgentAssetCategory::Mcp],
    )];
    let missing_declarations = vec![declaration(
        &ctx,
        "blocked",
        "blocked",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut missing = draft(
        &missing_declarations[0],
        "mcp:blocked",
        AgentAssetResolutionRelation::Unknown,
        &[&missing_declarations[0]],
    );
    missing.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "missing".to_string(),
    });
    let result = project(&missing_sources, &missing_declarations, vec![missing]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "mcp:blocked"
    )));

    let mut self_reference = draft(
        &missing_declarations[0],
        "mcp:blocked",
        AgentAssetResolutionRelation::Unknown,
        &[&missing_declarations[0]],
    );
    self_reference.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
        declaration_id: missing_declarations[0].declaration_id.clone(),
    });
    let result = project(
        &missing_sources,
        &missing_declarations,
        vec![self_reference],
    );
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "mcp:blocked"
    )));

    let ambiguous_sources = vec![
        source("policy-a", "policy-a.json", &[AgentAssetCategory::Mcp]),
        source("policy-b", "policy-b.json", &[AgentAssetCategory::Mcp]),
        source("blocked", "blocked.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut ambiguous_declarations = vec![
        declaration(
            &ctx,
            "policy-a",
            "policy",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "policy-b",
            "policy",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "blocked",
            "blocked",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    for definition in &mut ambiguous_declarations[..2] {
        fixture_input(definition, "fixture_mode = 'additive'");
    }
    let policy_a = draft(
        &ambiguous_declarations[0],
        "mcp:policy@a",
        AgentAssetResolutionRelation::Additive,
        &[&ambiguous_declarations[0]],
    );
    let policy_b = draft(
        &ambiguous_declarations[1],
        "mcp:policy@b",
        AgentAssetResolutionRelation::Additive,
        &[&ambiguous_declarations[1]],
    );
    let mut ambiguous = draft(
        &ambiguous_declarations[2],
        "mcp:blocked",
        AgentAssetResolutionRelation::Unknown,
        &[&ambiguous_declarations[2]],
    );
    ambiguous.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "policy".to_string(),
    });
    let result = project(
        &ambiguous_sources,
        &ambiguous_declarations,
        vec![policy_a, policy_b, ambiguous],
    );
    assert_eq!(result.records.len(), 2);
    assert!(result
        .records
        .iter()
        .all(|record| record.native_id == "policy"));
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "mcp:blocked"
    )));

    let rejected_sources = vec![
        source("policy-a", "policy-a.json", &[AgentAssetCategory::Mcp]),
        source("policy-b", "policy-b.json", &[AgentAssetCategory::Mcp]),
        source("blocked", "blocked.json", &[AgentAssetCategory::Mcp]),
    ];
    let rejected_declarations = vec![
        declaration(
            &ctx,
            "policy-a",
            "policy",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "policy-b",
            "policy",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "blocked",
            "blocked",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    let mut incomplete_winner = draft(
        &rejected_declarations[0],
        "mcp:policy",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&rejected_declarations[0], &rejected_declarations[1]],
    );
    incomplete_winner.resolution.winner = Some(AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "policy".to_string(),
        qualifier: Some("mcp:policy".to_string()),
    });
    let mut rejected_target = draft(
        &rejected_declarations[2],
        "mcp:blocked",
        AgentAssetResolutionRelation::Unknown,
        &[&rejected_declarations[2]],
    );
    rejected_target.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
        declaration_id: rejected_declarations[0].declaration_id.clone(),
    });
    let result = project(
        &rejected_sources,
        &rejected_declarations,
        vec![incomplete_winner, rejected_target],
    );
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "mcp:blocked"
    )));
}

#[test]
fn policy_reference_rejects_cross_context_group_role_and_suppressed_policy() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("policy", "policy.json", &[AgentAssetCategory::Mcp]),
    ];
    let build = |policy: ParsedAgentAsset| {
        let definition = declaration_with_key(
            &ctx,
            "definition",
            "server",
            "definition",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        );
        let mut blocked = draft(
            &definition,
            "mcp:server",
            AgentAssetResolutionRelation::Unknown,
            &[&definition, &policy],
        );
        blocked.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
            declaration_id: policy.declaration_id.clone(),
        });
        project(&sources, &[definition, policy], vec![blocked])
    };

    let mut wrong_group = declaration_with_key(
        &ctx,
        "policy",
        "policy",
        "policy",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    wrong_group.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    wrong_group.resolution_group_key = "other".to_string();
    assert!(build(wrong_group).records.is_empty());

    let mut wrong_role = declaration_with_key(
        &ctx,
        "policy",
        "policy",
        "policy-role",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    wrong_role.role = crate::models::AgentAssetDeclarationRole::Definition;
    wrong_role.resolution_group_key = "server".to_string();
    assert!(build(wrong_role).records.is_empty());

    let mut suppressed = declaration_with_key(
        &ctx,
        "policy",
        "policy",
        "policy-suppressed",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    suppressed.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    suppressed.resolution_group_key = "server".to_string();
    suppressed.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UnsupportedContext,
    };
    assert!(build(suppressed).records.is_empty());

    let mut cross_context = declaration_with_key(
        &ctx,
        "policy",
        "policy",
        "policy-cross-context",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    cross_context.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    cross_context.resolution_group_key = "server".to_string();
    cross_context.declaration_id = stable_id(
        "declaration",
        &["context:other", "policy", "mcp", "policy-cross-context"],
    );
    assert!(build(cross_context).records.is_empty());
}

#[test]
fn rejects_incomplete_replacement_and_coexistence_groups() {
    let ctx = context();
    let replacement_sources = vec![
        source("a", "a.json", &[AgentAssetCategory::Mcp]),
        source("b", "b.json", &[AgentAssetCategory::Mcp]),
    ];
    let replacement_declarations = vec![
        declaration(
            &ctx,
            "a",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "b",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
    ];
    let mut winner = draft(
        &replacement_declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&replacement_declarations[0], &replacement_declarations[1]],
    );
    winner.resolution.winner = Some(AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "server".to_string(),
        qualifier: Some("mcp:server".to_string()),
    });
    let result = project(
        &replacement_sources,
        &replacement_declarations,
        vec![winner],
    );
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidResolution { .. })));

    for kind in [
        AgentAssetResolutionRelation::Additive,
        AgentAssetResolutionRelation::Unknown,
    ] {
        let sources = vec![
            source("a", "a.json", &[AgentAssetCategory::Mcp]),
            source("b", "b.json", &[AgentAssetCategory::Mcp]),
        ];
        let declarations = vec![
            declaration(
                &ctx,
                "a",
                "coexisting",
                AgentAssetCategory::Mcp,
                AgentAssetDeclaredState::Enabled,
            ),
            declaration(
                &ctx,
                "b",
                "coexisting",
                AgentAssetCategory::Mcp,
                AgentAssetDeclaredState::Enabled,
            ),
        ];
        let partial = draft(
            &declarations[0],
            "mcp:coexisting@a",
            kind,
            &[&declarations[0]],
        );
        let result = project(&sources, &declarations, vec![partial]);
        assert!(result.records.is_empty(), "kind={kind:?}");
        assert!(result
            .diagnostics
            .iter()
            .any(|item| matches!(item, AgentAssetDiagnostic::InvalidResolution { .. })));
    }
}

#[test]
fn rejects_unsorted_group_contributors() {
    let ctx = context();
    let sources = vec![
        source("a", "a.json", &[AgentAssetCategory::Mcp]),
        source("b", "b.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut declarations = vec![
        declaration(
            &ctx,
            "a",
            "merged",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "b",
            "merged",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    for definition in &mut declarations {
        fixture_input(definition, "fixture_mode = 'merge'");
    }
    let reversed = draft(
        &declarations[0],
        "mcp:merged",
        AgentAssetResolutionRelation::Merged,
        &[&declarations[1], &declarations[0]],
    );
    let result = project(&sources, &declarations, vec![reversed]);
    assert_eq!(result.records.len(), 1);
}

#[test]
fn hook_projection_preserves_native_switch_and_rejects_untrusted_enablement() {
    let ctx = context();
    let sources = vec![source("hook", "hooks.json", &[AgentAssetCategory::Hook])];
    for trust in [AgentTrustState::Required, AgentTrustState::Untrusted] {
        let mut hook = declaration(
            &ctx,
            "hook",
            "configured",
            AgentAssetCategory::Hook,
            AgentAssetDeclaredState::Enabled,
        );
        hook.trust_state = trust;
        let mut projected = draft(
            &hook,
            "hook:configured",
            AgentAssetResolutionRelation::Independent,
            &[&hook],
        );
        let declarations = [hook];
        assert!(project(&sources, &declarations, vec![projected.clone()])
            .records
            .is_empty());
        projected.effective_state = AgentAssetState::Unknown;
        let result = project(&sources, &declarations, vec![projected]);
        assert_rows(&result, &["configured"]);
        assert_eq!(result.records[0].declared_state, AgentAssetState::Enabled);
        assert_eq!(result.records[0].effective_state, AgentAssetState::Unknown);
        assert_eq!(result.records[0].trust_state, trust);
    }
}

#[test]
fn rejects_typed_details_state_and_trust_mismatches() {
    let ctx = context();
    let skill_sources = vec![source("skill", "skill.json", &[AgentAssetCategory::Skill])];
    let skill_declarations = vec![declaration(
        &ctx,
        "skill",
        "review",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut skill = draft(
        &skill_declarations[0],
        "skill:review",
        AgentAssetResolutionRelation::Independent,
        &[&skill_declarations[0]],
    );
    skill.details = AgentAssetDetails::Skill {
        enabled: AgentAssetDeclaredState::Disabled,
        invocation_policy: AgentSkillInvocationPolicy::Unknown,
    };
    assert!(project(&skill_sources, &skill_declarations, vec![skill])
        .records
        .is_empty());

    let mcp_sources = vec![source("mcp", "mcp.json", &[AgentAssetCategory::Mcp])];
    let mcp_declarations = vec![declaration(
        &ctx,
        "mcp",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut mcp = draft(
        &mcp_declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&mcp_declarations[0]],
    );
    mcp.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Stdio,
        declared_state: AgentAssetDeclaredState::Disabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Disabled,
    };
    assert!(project(&mcp_sources, &mcp_declarations, vec![mcp])
        .records
        .is_empty());

    let plugin_sources = vec![source(
        "plugin",
        "plugin.json",
        &[AgentAssetCategory::Plugin],
    )];
    let plugin_declarations = vec![declaration(
        &ctx,
        "plugin",
        "provider",
        AgentAssetCategory::Plugin,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut plugin = draft(
        &plugin_declarations[0],
        "plugin:provider",
        AgentAssetResolutionRelation::Independent,
        &[&plugin_declarations[0]],
    );
    plugin.trust_state = AgentTrustState::Trusted;
    assert!(project(&plugin_sources, &plugin_declarations, vec![plugin])
        .records
        .is_empty());

    let extension_sources = vec![source(
        "extension",
        "extension.json",
        &[AgentAssetCategory::Extension],
    )];
    let extension_declarations = vec![declaration(
        &ctx,
        "extension",
        "provider",
        AgentAssetCategory::Extension,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut extension = draft(
        &extension_declarations[0],
        "extension:provider",
        AgentAssetResolutionRelation::Independent,
        &[&extension_declarations[0]],
    );
    extension.trust_state = AgentTrustState::Trusted;
    assert!(
        project(&extension_sources, &extension_declarations, vec![extension])
            .records
            .is_empty()
    );

    let hook_sources = vec![source("hook", "hook.json", &[AgentAssetCategory::Hook])];
    let hook_declarations = vec![declaration(
        &ctx,
        "hook",
        "session-start",
        AgentAssetCategory::Hook,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut hook = draft(
        &hook_declarations[0],
        "hook:session-start",
        AgentAssetResolutionRelation::Independent,
        &[&hook_declarations[0]],
    );
    hook.details = AgentAssetDetails::Hook {
        managed: false,
        enabled: AgentAssetDeclaredState::Disabled,
        rule_count: None,
    };
    assert!(project(&hook_sources, &hook_declarations, vec![hook])
        .records
        .is_empty());

    let status_sources = vec![source(
        "status",
        "status.json",
        &[AgentAssetCategory::StatusUi],
    )];
    let status_declarations = vec![declaration(
        &ctx,
        "status",
        "status-line",
        AgentAssetCategory::StatusUi,
        AgentAssetDeclaredState::Enabled,
    )];
    let mut status = draft(
        &status_declarations[0],
        "statusUi:status-line",
        AgentAssetResolutionRelation::Independent,
        &[&status_declarations[0]],
    );
    status.details = AgentAssetDetails::StatusUi {
        mode: AgentStatusUiMode::Command,
        command_present: false,
    };
    assert!(project(&status_sources, &status_declarations, vec![status])
        .records
        .is_empty());
}

#[test]
fn replaced_actions_are_shadowed_instead_of_policy_blocked() {
    let ctx = context();
    let mut sources = vec![
        source("a", "a.json", &[AgentAssetCategory::Mcp]),
        source("b", "b.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut declarations = vec![
        declaration(
            &ctx,
            "a",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "b",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
    ];
    prefer_definition(&mut sources, &mut declarations[0]);
    let mut winner = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&declarations[0], &declarations[1]],
    );
    let winner_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "server".to_string(),
        qualifier: Some("mcp:server".to_string()),
    };
    winner.resolution.winner = Some(winner_reference.clone());
    let mut loser = draft(
        &declarations[1],
        "mcp:server@b",
        AgentAssetResolutionRelation::Replaced,
        &[&declarations[1]],
    );
    loser.resolution.winner = Some(winner_reference);
    loser.state_proof.effective = AgentAssetEffectiveStateProofDraft::Shadowed {
        winner: loser
            .resolution
            .winner
            .clone()
            .expect("winner proof reference"),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };

    let result = project(&sources, &declarations, vec![winner, loser]);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let loser = result
        .records
        .iter()
        .find(|record| record.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .unwrap();
    assert!(loser
        .actions
        .iter()
        .filter(|action| matches!(
            action.action,
            AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
        ))
        .all(|action| {
            action.reason == Some(AgentAssetActionUnavailableReason::Shadowed)
                && action.reason != Some(AgentAssetActionUnavailableReason::PolicyBlocked)
        }));
}

#[test]
fn duplicate_projection_keys_are_all_rejected() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let declarations = vec![declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    )];
    let duplicate = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[0]],
    );
    let result = project(&sources, &declarations, vec![duplicate.clone(), duplicate]);
    assert!(result.records.is_empty());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|item| matches!(item, AgentAssetDiagnostic::InvalidProjection { .. }))
            .count(),
        1
    );
}

#[test]
fn qualifier_must_match_referenced_category_and_native_id() {
    let ctx = context();
    let mut sources = vec![
        source("a-winner", "winner.json", &[AgentAssetCategory::Mcp]),
        source("b-loser", "loser.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut declarations = vec![
        declaration(
            &ctx,
            "a-winner",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "b-loser",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
    ];
    prefer_definition(&mut sources, &mut declarations[0]);
    let mut winner = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&declarations[0], &declarations[1]],
    );
    let valid_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "server".to_string(),
        qualifier: Some("mcp:server".to_string()),
    };
    winner.resolution.winner = Some(valid_reference.clone());
    let mut loser = draft(
        &declarations[1],
        "mcp:server@loser",
        AgentAssetResolutionRelation::Replaced,
        &[&declarations[1]],
    );
    loser.resolution.winner = Some(valid_reference);
    loser.state_proof.effective = AgentAssetEffectiveStateProofDraft::Shadowed {
        winner: loser
            .resolution
            .winner
            .clone()
            .expect("winner proof reference"),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let baseline = project(&sources, &declarations, vec![winner.clone(), loser.clone()]);
    assert_eq!(baseline.records.len(), 2, "{:?}", baseline.diagnostics);
    assert!(
        baseline.diagnostics.is_empty(),
        "{:?}",
        baseline.diagnostics
    );

    let mismatched_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "different".to_string(),
        qualifier: Some("mcp:server".to_string()),
    };
    winner.resolution.winner = Some(mismatched_reference.clone());
    loser.resolution.winner = Some(mismatched_reference);
    let result = project(&sources, &declarations, vec![winner, loser]);
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn projection_output_is_stable_across_input_permutations() {
    let ctx = context();
    let mut sources = vec![
        source("a", "a.json", &[AgentAssetCategory::Mcp]),
        source("b", "b.json", &[AgentAssetCategory::Mcp]),
        source("plugin", "plugin.json", &[AgentAssetCategory::Plugin]),
        source("skill", "skill.json", &[AgentAssetCategory::Skill]),
    ];
    let mut declarations = vec![
        declaration(
            &ctx,
            "a",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "b",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "plugin",
            "provider",
            AgentAssetCategory::Plugin,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "skill",
            "child",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    let winner_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "server".to_string(),
        qualifier: Some("mcp:server".to_string()),
    };
    prefer_definition(&mut sources, &mut declarations[0]);
    let mut winner = draft(
        &declarations[0],
        "mcp:server",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&declarations[0], &declarations[1]],
    );
    winner.resolution.winner = Some(winner_reference.clone());
    let mut loser = draft(
        &declarations[1],
        "mcp:server@b",
        AgentAssetResolutionRelation::Replaced,
        &[&declarations[1]],
    );
    loser.resolution.winner = Some(winner_reference.clone());
    loser.state_proof.effective = AgentAssetEffectiveStateProofDraft::Shadowed {
        winner: winner_reference,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };

    let parent_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "provider".to_string(),
        qualifier: Some("plugin:provider".to_string()),
    };
    let child_reference = AgentAssetNativeRef {
        category: AgentAssetCategory::Skill,
        native_id: "child".to_string(),
        qualifier: Some("skill:child".to_string()),
    };
    declarations[2].explicitly_affected.push(child_reference);
    declarations[3].provided_by = Some(parent_reference.clone());
    declarations[3].action_owner = Some(parent_reference);
    let parent = draft(
        &declarations[2],
        "plugin:provider",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[2]],
    );
    let child = draft(
        &declarations[3],
        "skill:child",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[3]],
    );
    let drafts = vec![winner, loser, parent, child];

    let run = |sources: Vec<AgentAssetSourceSpec>,
               declarations: Vec<ParsedAgentAsset>,
               drafts: Vec<AgentAssetProjectedDraft>| {
        let mut diagnostic_run = AgentInventoryRun::with_clock(
            crate::models::AgentAssetLimits::DEFAULT,
            Arc::new(ManualClock::new()),
        );
        let mut result = project_records(
            &environment(),
            &ctx,
            &sources,
            &declarations,
            AgentAssetProjectionInput {
                drafts,
                state_assessor: fixture_assessor,
            },
            &BTreeMap::new(),
            &mut diagnostic_run,
        );
        for record in &mut result.records {
            record.revision.observed_at.clear();
        }
        (
            serde_json::to_value(result.records).unwrap(),
            serde_json::to_value(diagnostic_run.finish_diagnostics()).unwrap(),
        )
    };

    let baseline = run(sources.clone(), declarations.clone(), drafts.clone());
    let permuted = run(
        sources.into_iter().rev().collect(),
        declarations.into_iter().rev().collect(),
        drafts.into_iter().rev().collect(),
    );
    assert_eq!(baseline.0.as_array().unwrap().len(), 4);
    assert_eq!(baseline, permuted);
}

#[test]
fn parent_child_relationships_are_bidirectional_and_delegate_actions() {
    let ctx = context();
    let sources = vec![
        source("plugin", "plugin.json", &[AgentAssetCategory::Plugin]),
        source("skill", "skill.json", &[AgentAssetCategory::Skill]),
    ];
    let mut declarations = vec![
        declaration(
            &ctx,
            "plugin",
            "provider",
            AgentAssetCategory::Plugin,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "skill",
            "child",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    let parent_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "provider".to_string(),
        qualifier: Some("plugin:provider".to_string()),
    };
    declarations[0]
        .explicitly_affected
        .push(AgentAssetNativeRef {
            category: AgentAssetCategory::Skill,
            native_id: "child".to_string(),
            qualifier: Some("skill:child".to_string()),
        });
    declarations[1].provided_by = Some(parent_ref.clone());
    declarations[1].action_owner = Some(parent_ref);
    let parent = draft(
        &declarations[0],
        "plugin:provider",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[0]],
    );
    let child = draft(
        &declarations[1],
        "skill:child",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[1]],
    );

    let result = project(&sources, &declarations, vec![parent, child]);
    assert!(result.diagnostics.is_empty());
    let parent = result
        .records
        .iter()
        .find(|record| record.native_id == "provider")
        .unwrap();
    let child = result
        .records
        .iter()
        .find(|record| record.native_id == "child")
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
    assert!(
        child
            .actions
            .iter()
            .filter(|action| matches!(
                action.action,
                AgentAssetActionKind::Enable | AgentAssetActionKind::Disable
            ))
            .all(|action| action.reason
                == Some(AgentAssetActionUnavailableReason::ChildOwnedByParent))
    );
}

#[test]
fn all_suppressed_group_keeps_evidence_and_source_identity() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let mut declaration = declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    declaration.trust_state = AgentTrustState::Untrusted;
    declaration.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
    };
    let mut unknown = draft(
        &declaration,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[],
    );
    unknown.represented_declaration_ids = vec![declaration.declaration_id.clone()];
    unknown.resolution.terminal = Some(crate::models::AgentAssetResolutionTerminal::Unknown);
    unknown.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declaration.declaration_id.clone(),
        }],
        cause: crate::services::agent_cli::contracts::AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
    };
    unknown.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: crate::models::AgentAssetResolutionTerminal::Unknown,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TrustSuppressed,
        evidence: Vec::new(),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    unknown.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Unknown,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
    };
    let result = project(&sources, &[declaration], vec![unknown]);
    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert!(record.resolution.contributor_ids.is_empty());
    assert_eq!(record.represented_declaration_ids.len(), 1);
    assert_eq!(record.source_ids.len(), 1);
    assert_eq!(record.trust_state, AgentTrustState::Untrusted);
    assert_eq!(
        record.inspection_source_id,
        source_stable_id(
            &ctx,
            std::path::Path::new("/tmp/balancehub-fixture/main.json")
        )
    );
    assert_eq!(
        record.stable_id,
        stable_id(
            "asset",
            &[
                ctx.id.as_str(),
                ctx.agent_kind.key(),
                AgentAssetCategory::Mcp.key().as_str(),
                "mcp:server",
                record.source_ids[0].as_str(),
            ],
        )
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::DeclarationSuppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            ))
            .count(),
        1
    );
    assert!(!record
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::DeclarationSuppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
            }
        )));
}

#[test]
fn parent_suppressed_mcp_evidence_does_not_require_or_authorize_a_row() {
    assert_parent_suppressed_evidence(AgentAssetCategory::Mcp);
}

#[test]
fn parent_suppressed_hook_evidence_does_not_require_or_authorize_a_row() {
    assert_parent_suppressed_evidence(AgentAssetCategory::Hook);
}

fn assert_parent_suppressed_evidence(category: AgentAssetCategory) {
    let ctx = context();
    let sources = vec![
        source("suppressed", "plugin.json", &[category]),
        source("healthy", "SKILL.md", &[AgentAssetCategory::Skill]),
    ];
    let mut suppressed = declaration(
        &ctx,
        "suppressed",
        "loser-only",
        category,
        AgentAssetDeclaredState::Enabled,
    );
    suppressed.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::ParentNotSelected,
    };
    let healthy = declaration(
        &ctx,
        "healthy",
        "neighbor",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let healthy_draft = draft(
        &healthy,
        "skill:neighbor",
        AgentAssetResolutionRelation::Independent,
        &[&healthy],
    );
    let declarations = [suppressed.clone(), healthy.clone()];
    let result = project(&sources, &declarations, vec![healthy_draft.clone()]);
    assert_rows(&result, &["neighbor"]);
    assert_eq!(result.diagnostics.len(), 1);
    assert!(matches!(
        result.diagnostics[0],
        AgentAssetDiagnostic::DeclarationSuppressed {
            reason: crate::models::AgentAssetSuppressionReason::ParentNotSelected
        }
    ));

    let mut participating = suppressed.clone();
    participating.participation = AgentAssetResolutionParticipation::Participates;
    let mut other_suppression = suppressed.clone();
    other_suppression.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UnsupportedContext,
    };
    let mut invalid_identity = suppressed.clone();
    invalid_identity.declaration_id = "forged-id".to_owned();
    let mut empty_group = suppressed.clone();
    empty_group.resolution_group_key.clear();
    for mut invalid in [
        vec![participating],
        vec![other_suppression],
        vec![invalid_identity],
        vec![empty_group],
        vec![suppressed.clone(), suppressed.clone()],
    ] {
        invalid.push(healthy.clone());
        assert_rejected_with_neighbor(&project(&sources, &invalid, vec![healthy_draft.clone()]));
    }

    for relation in [
        AgentAssetResolutionRelation::Independent,
        AgentAssetResolutionRelation::Unknown,
    ] {
        let mut fabricated = draft(
            &suppressed,
            &format!("{}:loser-only", category.key()),
            relation,
            &[],
        );
        fabricated.represented_declaration_ids = vec![suppressed.declaration_id.clone()];
        assert_rejected_with_neighbor(&project(
            &sources,
            &declarations,
            vec![healthy_draft.clone(), fabricated],
        ));
    }
}

#[test]
fn overlay_only_declarations_are_retained_without_asset() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let mut overlay = declaration(
        &ctx,
        "main",
        "orphan",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Disabled,
    );
    overlay.role = crate::models::AgentAssetDeclarationRole::StateOverlay;
    let result = project(&sources, &[overlay], Vec::new());
    assert!(result.records.is_empty());
    assert!(!result
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn definition_group_cannot_disappear_with_overlay_only_exception() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let definition = declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut overlay = declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Disabled,
    );
    overlay.role = crate::models::AgentAssetDeclarationRole::StateOverlay;
    let result = project(&sources, &[definition, overlay], Vec::new());
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|item| matches!(item, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn overlays_do_not_change_definition_cardinality() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("suppressed", "suppressed.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut definition = declaration_with_key(
        &ctx,
        "definition",
        "server",
        "definition",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    fixture_input(&mut definition, "fixture_mode = 'unresolved'");
    let mut state = declaration_with_key(
        &ctx,
        "definition",
        "server",
        "state-overlay",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    state.role = crate::models::AgentAssetDeclarationRole::StateOverlay;
    let mut policy = declaration_with_key(
        &ctx,
        "definition",
        "server",
        "policy-overlay",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    policy.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    fixture_input(
        &mut policy,
        "source_aggregate = true
block_targets = ['server']",
    );
    let mut suppressed = declaration_with_key(
        &ctx,
        "suppressed",
        "server",
        "suppressed-definition",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    suppressed.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
    };
    let mut independent = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&definition, &state, &policy],
    );
    independent.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
        declaration_ids: vec![state.declaration_id.clone()],
        scope:
            crate::services::agent_cli::contracts::AgentAssetStateOverlayScopeDraft::ExactNativeId,
        outcome: AgentAssetDeclaredState::Enabled,
    };
    independent.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "definition".to_string(),
    });
    independent.resolution.terminal =
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked);
    independent.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: policy.declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    independent.effective_state = AgentAssetState::Blocked;
    independent.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    independent.represented_declaration_ids = vec![
        definition.declaration_id.clone(),
        state.declaration_id.clone(),
        policy.declaration_id.clone(),
        suppressed.declaration_id.clone(),
    ];
    let result = project(
        &sources,
        &[definition, state, policy, suppressed],
        vec![independent],
    );
    assert_eq!(result.records.len(), 1, "{:?}", result.diagnostics);
    assert_eq!(result.records[0].resolution.contributor_ids.len(), 3);
    assert_eq!(result.records[0].represented_declaration_ids.len(), 4);
}

#[test]
fn inspection_source_must_be_a_participating_definition() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("overlay", "overlay.json", &[AgentAssetCategory::Mcp]),
    ];
    let definition = declaration_with_key(
        &ctx,
        "definition",
        "server",
        "definition",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut overlay = declaration_with_key(
        &ctx,
        "overlay",
        "server",
        "state",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    overlay.role = crate::models::AgentAssetDeclarationRole::StateOverlay;
    let mut invalid = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&definition, &overlay],
    );
    invalid.inspection_source_id = "overlay".to_string();
    let result = project(&sources, &[definition, overlay], vec![invalid]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        matches!(diagnostic, AgentAssetDiagnostic::InvalidProjection { .. })
    }));
}

#[test]
fn declaration_formula_is_checked_before_resolution() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let mut declaration = declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    declaration.declaration_id = "forged-declaration-id".to_string();
    let invalid = draft(
        &declaration,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&declaration],
    );
    let result = project(&sources, &[declaration], vec![invalid]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        matches!(diagnostic, AgentAssetDiagnostic::InvalidProjection { projection_key } if projection_key == "forged-declaration-id")
    }));
}

#[test]
fn same_source_category_and_key_collision_is_rejected() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let first = declaration_with_key(
        &ctx,
        "main",
        "server-a",
        "same-key",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut second = declaration_with_key(
        &ctx,
        "main",
        "server-b",
        "same-key",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    second.declaration_id = first.declaration_id.clone();
    let first_draft = draft(
        &first,
        "mcp:server-a",
        AgentAssetResolutionRelation::Independent,
        &[&first],
    );
    let second_draft = draft(
        &second,
        "mcp:server-b",
        AgentAssetResolutionRelation::Independent,
        &[&second],
    );
    let result = project(&sources, &[first, second], vec![first_draft, second_draft]);
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidProjection { .. })));
}

#[test]
fn unresolved_provider_cannot_become_an_independent_asset() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Skill])];
    let mut declaration = declaration(
        &ctx,
        "main",
        "child",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    declaration.provided_by = Some(AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "missing".to_string(),
        qualifier: Some("plugin:missing".to_string()),
    });
    let child = draft(
        &declaration,
        "skill:child",
        AgentAssetResolutionRelation::Independent,
        &[&declaration],
    );
    let result = project(&sources, &[declaration], vec![child]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "skill:child"
    )));
}

#[test]
fn required_ownership_rejection_is_transitive_while_explicit_impacts_remain_soft() {
    let ctx = context();
    let sources = vec![
        source("plugin", "plugin.json", &[AgentAssetCategory::Plugin]),
        source("skill", "skill.json", &[AgentAssetCategory::Skill]),
        source("observer", "observer.json", &[AgentAssetCategory::Plugin]),
    ];
    let parent_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "provider".to_owned(),
        qualifier: Some("plugin:provider".to_owned()),
    };
    let child_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Skill,
        native_id: "child".to_owned(),
        qualifier: Some("skill:child".to_owned()),
    };
    let missing_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "missing".to_owned(),
        qualifier: Some("plugin:missing".to_owned()),
    };
    let observer_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: "observer".to_owned(),
        qualifier: Some("plugin:observer".to_owned()),
    };
    for case in [
        "valid",
        "missing-provider",
        "missing-owner",
        "self-parent",
        "rejected-parent",
        "missing-parent-draft",
    ] {
        let mut declarations = vec![
            declaration(
                &ctx,
                "plugin",
                "provider",
                AgentAssetCategory::Plugin,
                AgentAssetDeclaredState::Enabled,
            ),
            declaration(
                &ctx,
                "skill",
                "child",
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            ),
            declaration(
                &ctx,
                "observer",
                "observer",
                AgentAssetCategory::Plugin,
                AgentAssetDeclaredState::Enabled,
            ),
        ];
        declarations[1].provided_by = Some(parent_ref.clone());
        declarations[1].action_owner = Some(parent_ref.clone());
        declarations[2].explicitly_affected =
            vec![child_ref.clone(), missing_ref.clone(), observer_ref.clone()];
        match case {
            "missing-provider" => declarations[1].provided_by = Some(missing_ref.clone()),
            "missing-owner" => declarations[1].action_owner = Some(missing_ref.clone()),
            "self-parent" => declarations[0].provided_by = Some(parent_ref.clone()),
            _ => {}
        }
        let mut parent = draft(
            &declarations[0],
            "plugin:provider",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[0]],
        );
        let child = draft(
            &declarations[1],
            "skill:child",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[1]],
        );
        let observer = draft(
            &declarations[2],
            "plugin:observer",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[2]],
        );
        if case == "rejected-parent" {
            parent.effective_state = AgentAssetState::Disabled;
        }
        let mut drafts = vec![child, observer];
        if case != "missing-parent-draft" {
            drafts.push(parent);
        }
        let result = project(&sources, &declarations, drafts);
        let ids = result
            .records
            .iter()
            .map(|row| row.native_id.as_str())
            .collect::<BTreeSet<_>>();
        let expected = match case {
            "valid" => BTreeSet::from(["child", "observer", "provider"]),
            "missing-provider" | "missing-owner" => BTreeSet::from(["observer", "provider"]),
            _ => BTreeSet::from(["observer"]),
        };
        assert_eq!(ids, expected, "{case}: {:?}", result.diagnostics);
        let observer = result
            .records
            .iter()
            .find(|row| row.native_id == "observer")
            .unwrap();
        assert!(observer.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::UnresolvedRelationship { relation: AgentAssetRelationKind::ExplicitImpact, native_id }
                if native_id == "missing"
        )));
        if case == "valid" {
            let child = result
                .records
                .iter()
                .find(|row| row.native_id == "child")
                .unwrap();
            let parent = result
                .records
                .iter()
                .find(|row| row.native_id == "provider")
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
                observer.relationships.affected_asset_ids.as_slice(),
                std::slice::from_ref(&child.stable_id)
            );
        } else {
            assert!(observer.relationships.affected_asset_ids.is_empty());
            assert!(result.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidResolution { projection_key, .. } if projection_key == "skill:child"
            )));
        }
    }
}

#[test]
fn policy_reference_is_typed_and_does_not_create_policy_asset() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("policy", "policy.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut definition = declaration(
        &ctx,
        "definition",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    fixture_input(&mut definition, "fixture_mode = 'unresolved'");
    let mut policy = declaration(
        &ctx,
        "policy",
        "workspace-policy",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    definition.resolution_group_key = "server".to_string();
    policy.resolution_group_key = "server".to_string();
    policy.native_id = "server".to_owned();
    policy.role = crate::models::AgentAssetDeclarationRole::PolicyOverlay;
    policy.native_payload = AgentAssetNativePayload::McpPolicy(
        crate::services::agent_cli::contracts::AgentMcpPolicyPayload::Excluded(BTreeSet::from([
            "server".to_owned(),
        ])),
    );
    let mut blocked = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&definition, &policy],
    );
    blocked.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
        declaration_id: policy.declaration_id.clone(),
    });
    blocked.resolution.terminal = Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked);
    blocked.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: crate::models::AgentAssetResolutionTerminal::PolicyBlocked,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: policy.declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    blocked.effective_state = AgentAssetState::Blocked;
    blocked.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    let result = project(&sources, &[definition, policy], vec![blocked]);
    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert!(matches!(
        record.resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { .. })
    ));
    assert!(record
        .resolution
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::PolicyBlocked)));
    assert_eq!(record.represented_declaration_ids.len(), 2);
}

#[test]
fn replace_winner_requires_a_resolvable_terminal_control_source() {
    let ctx = context();
    let sources = vec![
        source("winner", "winner.json", &[AgentAssetCategory::Mcp]),
        source("loser", "loser.json", &[AgentAssetCategory::Mcp]),
    ];
    let winner = declaration(
        &ctx,
        "winner",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let loser = declaration(
        &ctx,
        "loser",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut draft = draft(
        &winner,
        "mcp:server",
        AgentAssetResolutionRelation::ReplaceWinner,
        &[&winner, &loser],
    );
    draft.resolution.winner = Some(AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: "server".to_owned(),
        qualifier: Some("mcp:server".to_owned()),
    });
    draft.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
    draft.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "missing-control".to_owned(),
    });
    draft.effective_state = AgentAssetState::Blocked;
    draft.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    let result = project(&sources, &[winner, loser], vec![draft]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "mcp:server"
    )));
}

#[test]
fn normalized_collision_rejects_cross_bucket_definition_contributors() {
    let ctx = context();
    let sources = vec![
        source("upper", "upper.json", &[AgentAssetCategory::Mcp]),
        source("lower", "lower.json", &[AgentAssetCategory::Mcp]),
    ];
    let upper = declaration(
        &ctx,
        "upper",
        "Server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let lower = declaration(
        &ctx,
        "lower",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut upper_draft = draft(
        &upper,
        "mcp:Server",
        AgentAssetResolutionRelation::Independent,
        &[&upper],
    );
    let mut lower_draft = draft(
        &lower,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&lower],
    );
    upper_draft.resolution.qualified_collision = true;
    lower_draft.resolution.qualified_collision = true;
    upper_draft.resolution_group_key = "normalized-server".to_owned();
    lower_draft.resolution_group_key = "normalized-server".to_owned();
    upper_draft.represented_declaration_ids =
        vec![upper.declaration_id.clone(), lower.declaration_id.clone()];
    lower_draft.represented_declaration_ids = upper_draft.represented_declaration_ids.clone();
    // Deliberately attach the lowercase Definition to the uppercase row.
    upper_draft.contributor_ids = vec![lower.declaration_id.clone()];
    let result = project(&sources, &[upper, lower], vec![upper_draft, lower_draft]);
    assert!(result.records.is_empty());
    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { .. })));
}

#[test]
fn parent_effective_state_does_not_rewrite_child_declared_state() {
    let ctx = context();
    let categories = [
        AgentAssetCategory::Extension,
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Hook,
        AgentAssetCategory::Skill,
    ];
    let sources = vec![source("assets", "assets.json", &categories)];
    let mut declarations = vec![
        declaration(
            &ctx,
            "assets",
            "bundle",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "assets",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "assets",
            "hook",
            AgentAssetCategory::Hook,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "assets",
            "skill",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    let parent_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Extension,
        native_id: "bundle".to_owned(),
        qualifier: Some("extension:bundle".to_owned()),
    };
    for child in &mut declarations[1..] {
        child.provided_by = Some(parent_ref.clone());
        fixture_input(child, "parent_gate = true");
    }
    let parent = draft(
        &declarations[0],
        "extension:bundle",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[0]],
    );
    let mut mcp = draft(
        &declarations[1],
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[1]],
    );
    let mut hook = draft(
        &declarations[2],
        "hook:hook",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[2]],
    );
    let mut skill = draft(
        &declarations[3],
        "skill:skill",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[3]],
    );
    for child in [&mut mcp, &mut hook, &mut skill] {
        child.provided_by = Some(parent_ref.clone());
        child.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: parent_ref.clone(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        child.effective_state = AgentAssetState::Disabled;
    }
    mcp.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Disabled,
    };
    let result = project(&sources, &declarations, vec![parent, mcp, hook, skill]);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    for native_id in ["server", "hook", "skill"] {
        let record = result
            .records
            .iter()
            .find(|record| record.native_id == native_id)
            .expect("child record");
        assert_eq!(record.declared_state, AgentAssetState::Enabled);
        assert_eq!(record.effective_state, AgentAssetState::Disabled);
        assert!(record.relationships.provided_by.is_some());
        assert!(record.diagnostics.iter().all(|diagnostic| !matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidResolution { .. }
        )));
        match native_id {
            "hook" => assert!(matches!(
                record.details,
                AgentAssetDetails::Hook {
                    enabled: AgentAssetDeclaredState::Enabled,
                    ..
                }
            )),
            "skill" => assert!(matches!(
                record.details,
                AgentAssetDetails::Skill {
                    enabled: AgentAssetDeclaredState::Enabled,
                    ..
                }
            )),
            _ => {}
        }
    }
    let mcp = result
        .records
        .iter()
        .find(|record| record.category == AgentAssetCategory::Mcp)
        .unwrap();
    assert!(matches!(
        mcp.details,
        AgentAssetDetails::Mcp {
            declared_state: AgentAssetDeclaredState::Enabled,
            effective_availability: AgentAssetEffectiveAvailability::Disabled,
            ..
        }
    ));
}

#[test]
fn parent_unknown_is_typed_and_category_specific() {
    let ctx = context();
    let categories = [
        AgentAssetCategory::Extension,
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Hook,
        AgentAssetCategory::Skill,
    ];
    let sources = vec![source("assets", "assets.json", &categories)];
    let mut declarations = vec![
        declaration(
            &ctx,
            "assets",
            "bundle",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Unknown,
        ),
        declaration(
            &ctx,
            "assets",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "assets",
            "blocked",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "assets",
            "hook-disabled",
            AgentAssetCategory::Hook,
            AgentAssetDeclaredState::Disabled,
        ),
        declaration(
            &ctx,
            "assets",
            "hook-enabled",
            AgentAssetCategory::Hook,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &ctx,
            "assets",
            "skill",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        ),
    ];
    let mut policy = declaration_with_key(
        &ctx,
        "assets",
        "blocked",
        "blocked-policy",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    policy.role = AgentAssetDeclarationRole::PolicyOverlay;
    policy.native_payload = AgentAssetNativePayload::McpPolicy(
        crate::services::agent_cli::contracts::AgentMcpPolicyPayload::Excluded(BTreeSet::from([
            "blocked".to_owned(),
        ])),
    );
    declarations.push(policy);
    let parent_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Extension,
        native_id: "bundle".to_owned(),
        qualifier: Some("extension:bundle".to_owned()),
    };
    for (index, child) in declarations.iter_mut().enumerate().take(6).skip(1) {
        child.provided_by = Some(parent_ref.clone());
        if index != 3 {
            fixture_input(child, "parent_gate = true");
        }
    }
    let mut parent = draft(
        &declarations[0],
        "extension:bundle",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[0]],
    );
    parent.effective_state = AgentAssetState::Unknown;
    let mut ordinary_mcp = draft(
        &declarations[1],
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[1]],
    );
    let mut blocked_mcp = draft(
        &declarations[2],
        "mcp:blocked",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[2], &declarations[6]],
    );
    blocked_mcp.represented_declaration_ids = blocked_mcp.contributor_ids.clone();
    blocked_mcp.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
    blocked_mcp.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
        declaration_id: declarations[6].declaration_id.clone(),
    });
    blocked_mcp.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declarations[6].declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    blocked_mcp.effective_state = AgentAssetState::Blocked;
    blocked_mcp.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    let mut hook_disabled = draft(
        &declarations[3],
        "hook:hook-disabled",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[3]],
    );
    hook_disabled.effective_state = AgentAssetState::Disabled;
    let mut hook_enabled = draft(
        &declarations[4],
        "hook:hook-enabled",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[4]],
    );
    let mut skill = draft(
        &declarations[5],
        "skill:skill",
        AgentAssetResolutionRelation::Independent,
        &[&declarations[5]],
    );
    for child in [
        &mut ordinary_mcp,
        &mut blocked_mcp,
        &mut hook_disabled,
        &mut hook_enabled,
        &mut skill,
    ] {
        child.provided_by = Some(parent_ref.clone());
        child.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: parent_ref.clone(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
    }
    blocked_mcp.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declarations[6].declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: parent_ref.clone(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        }),
    };
    // A native disabled Hook is already a complete local decision. The
    // unknown parent must not erase that category-specific declaration.
    hook_disabled.state_proof.effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
    ordinary_mcp.effective_state = AgentAssetState::Unknown;
    ordinary_mcp.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Unknown,
    };
    hook_enabled.effective_state = AgentAssetState::Unknown;
    skill.effective_state = AgentAssetState::Unknown;
    let result = project(
        &sources,
        &declarations,
        vec![
            parent,
            ordinary_mcp,
            blocked_mcp,
            hook_disabled,
            hook_enabled,
            skill,
        ],
    );
    assert!(
        !result.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
                | AgentAssetDiagnostic::UnresolvedRelationship { .. }
        )),
        "{:?}",
        result.diagnostics
    );
    let find = |native_id: &str| {
        result
            .records
            .iter()
            .find(|record| record.native_id == native_id)
            .unwrap()
    };
    assert_eq!(find("server").effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        find("server").details,
        AgentAssetDetails::Mcp {
            effective_availability: AgentAssetEffectiveAvailability::Unknown,
            ..
        }
    ));
    let blocked = find("blocked");
    assert_eq!(blocked.declared_state, AgentAssetState::Enabled);
    assert_eq!(blocked.effective_state, AgentAssetState::Blocked);
    assert_eq!(
        blocked.resolution.terminal,
        Some(AgentAssetResolutionTerminal::PolicyBlocked)
    );
    assert!(matches!(
        blocked.resolution.control_source,
        Some(crate::models::AgentAssetPolicyReference::Declaration { .. })
    ));
    assert_eq!(
        find("hook-disabled").effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        find("hook-enabled").effective_state,
        AgentAssetState::Unknown
    );
    assert_eq!(find("skill").effective_state, AgentAssetState::Unknown);
    assert!(matches!(
        find("hook-disabled").details,
        AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Disabled,
            ..
        }
    ));
    assert!(matches!(
        find("hook-enabled").details,
        AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
    assert!(matches!(
        find("skill").details,
        AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled,
            ..
        }
    ));
}

#[test]
fn parent_effective_proof_rejects_missing_ambiguous_and_mismatched_parent() {
    let run_case = |case: &str| {
        let ctx = context();
        let categories = [AgentAssetCategory::Extension, AgentAssetCategory::Skill];
        let sources = vec![source("assets", "assets.json", &categories)];
        let mut declarations = vec![
            declaration(
                &ctx,
                "assets",
                "bundle",
                AgentAssetCategory::Extension,
                AgentAssetDeclaredState::Enabled,
            ),
            declaration(
                &ctx,
                "assets",
                "skill",
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            ),
        ];
        let valid_ref = AgentAssetNativeRef {
            category: AgentAssetCategory::Extension,
            native_id: "bundle".to_owned(),
            qualifier: Some("extension:bundle".to_owned()),
        };
        declarations[1].provided_by = Some(valid_ref.clone());
        fixture_input(&mut declarations[1], "parent_gate = true");
        let parent = draft(
            &declarations[0],
            "extension:bundle",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[0]],
        );
        let mut child = draft(
            &declarations[1],
            "skill:skill",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[1]],
        );
        child.provided_by = Some(valid_ref.clone());
        child.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: valid_ref.clone(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        let baseline = project(&sources, &declarations, vec![parent.clone(), child.clone()]);
        assert_rows(&baseline, &["bundle", "skill"]);
        let baseline_parent = baseline
            .records
            .iter()
            .find(|record| record.native_id == "bundle")
            .unwrap();
        let baseline_child = baseline
            .records
            .iter()
            .find(|record| record.native_id == "skill")
            .unwrap();
        assert_eq!(
            baseline_child.relationships.provided_by.as_deref(),
            Some(baseline_parent.stable_id.as_str())
        );
        assert_eq!(
            baseline_parent.relationships.affected_asset_ids,
            vec![baseline_child.stable_id.clone()]
        );
        match case {
            "wrong-category" => {
                let wrong = AgentAssetNativeRef {
                    category: AgentAssetCategory::Skill,
                    ..valid_ref.clone()
                };
                child.provided_by = Some(wrong.clone());
                child.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
                    parent: wrong,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
            }
            "missing" => {
                let missing = AgentAssetNativeRef {
                    native_id: "missing".to_owned(),
                    qualifier: Some("extension:missing".to_owned()),
                    ..valid_ref.clone()
                };
                child.provided_by = Some(missing.clone());
                child.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
                    parent: missing,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
            }
            "mismatched-state" => child.effective_state = AgentAssetState::Disabled,
            _ => unreachable!(),
        }
        let result = project(&sources, &declarations, vec![parent, child]);
        assert_rows(&result, &["bundle"]);
        assert_eq!(result.records[0].stable_id, baseline_parent.stable_id);
        assert!(result.records[0]
            .relationships
            .affected_asset_ids
            .is_empty());
        // Changing the parent reference disagrees with the independently
        // assessed native relationship before graph evaluation. A matching
        // reference with a forged final state reaches the graph evaluator.
        let rejection_key = match case {
            "wrong-category" | "missing" => "skill:skill:assessment:InvalidNativeInput",
            "mismatched-state" => "skill:skill",
            _ => unreachable!(),
        };
        assert_eq!(
            result.diagnostics,
            vec![AgentAssetDiagnostic::InvalidResolution {
                projection_key: rejection_key.to_owned(),
                resolution: AgentAssetResolutionRelation::Independent,
            }],
            "{case} rejected at an unexpected stage"
        );
    };
    for case in ["wrong-category", "missing", "mismatched-state"] {
        run_case(case);
    }

    let ctx = context();
    let categories = [AgentAssetCategory::Extension];
    let sources = vec![source("assets", "assets.json", &categories)];
    let mut declaration = declaration(
        &ctx,
        "assets",
        "bundle",
        AgentAssetCategory::Extension,
        AgentAssetDeclaredState::Enabled,
    );
    let self_ref = AgentAssetNativeRef {
        category: AgentAssetCategory::Extension,
        native_id: "bundle".to_owned(),
        qualifier: Some("extension:bundle".to_owned()),
    };
    declaration.provided_by = Some(self_ref.clone());
    fixture_input(&mut declaration, "parent_gate = true");
    let mut self_edge = draft(
        &declaration,
        "extension:bundle",
        AgentAssetResolutionRelation::Independent,
        &[&declaration],
    );
    self_edge.provided_by = Some(self_ref.clone());
    self_edge.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
        parent: self_ref,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let result = project(&sources, &[declaration], vec![self_edge]);
    assert!(result.records.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidResolution { projection_key, .. }
            if projection_key == "extension:bundle"
    )));
}

#[test]
fn shared_state_overlay_can_contribute_to_case_distinct_exact_buckets() {
    let ctx = context();
    let sources = vec![
        source("upper", "upper.json", &[AgentAssetCategory::Mcp]),
        source("lower", "lower.json", &[AgentAssetCategory::Mcp]),
        source("overlay", "overlay.json", &[AgentAssetCategory::Mcp]),
    ];
    let upper = declaration_with_key(
        &ctx,
        "upper",
        "Server",
        "upper-definition",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let lower = declaration_with_key(
        &ctx,
        "lower",
        "server",
        "lower-definition",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut overlay = declaration_with_key(
        &ctx,
        "overlay",
        "server",
        "shared-state",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Disabled,
    );
    overlay.resolution_group_key = "normalized-server".to_owned();
    overlay.role = AgentAssetDeclarationRole::StateOverlay;
    group_scoped_overlay(&mut overlay);
    let mut upper = upper;
    let mut lower = lower;
    upper.resolution_group_key = "normalized-server".to_owned();
    lower.resolution_group_key = "normalized-server".to_owned();
    let make = |asset: &ParsedAgentAsset| {
        let mut value = draft(
            asset,
            &format!("mcp:{}", asset.native_id),
            AgentAssetResolutionRelation::Independent,
            &[asset, &overlay],
        );
        value.resolution.qualified_collision = true;
        value.declared_state = AgentAssetDeclaredState::Disabled;
        value.effective_state = AgentAssetState::Disabled;
        value.details = AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state: AgentAssetDeclaredState::Disabled,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: AgentAssetEffectiveAvailability::Disabled,
        };
        value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
            declaration_ids: vec![overlay.declaration_id.clone()],
            scope: crate::services::agent_cli::contracts::AgentAssetStateOverlayScopeDraft::ResolutionGroup,
            outcome: AgentAssetDeclaredState::Disabled,
        };
        value
    };
    let upper_draft = make(&upper);
    let lower_draft = make(&lower);
    let result = project(
        &sources,
        &[upper, lower, overlay.clone()],
        vec![upper_draft, lower_draft],
    );
    assert!(
        !result.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
                | AgentAssetDiagnostic::UnresolvedRelationship { .. }
        )),
        "{:?}",
        result.diagnostics
    );
    assert_eq!(result.records.len(), 2);
    for record in &result.records {
        assert!(record.resolution.qualified_collision);
        assert_eq!(record.declared_state, AgentAssetState::Disabled);
        assert_eq!(record.effective_state, AgentAssetState::Disabled);
        assert!(record
            .resolution
            .contributor_ids
            .iter()
            .any(|id| id == &overlay.declaration_id));
    }
}

#[test]
fn shared_state_overlay_proof_rejects_unbound_or_incomplete_evidence() {
    for case in [
        "unbound",
        "wrong-role",
        "wrong-category",
        "wrong-group",
        "wrong-state",
        "suppressed",
        "not-listed",
    ] {
        let ctx = context();
        let sources = vec![
            source("upper", "upper.json", &[AgentAssetCategory::Mcp]),
            source("lower", "lower.json", &[AgentAssetCategory::Mcp]),
            source("overlay", "overlay.json", &[AgentAssetCategory::Mcp]),
        ];
        let mut upper = declaration_with_key(
            &ctx,
            "upper",
            "Server",
            "upper-definition",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        );
        let mut lower = declaration_with_key(
            &ctx,
            "lower",
            "server",
            "lower-definition",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        );
        let mut overlay = declaration_with_key(
            &ctx,
            "overlay",
            "server",
            "shared-state",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Disabled,
        );
        upper.resolution_group_key = "normalized-server".to_owned();
        lower.resolution_group_key = "normalized-server".to_owned();
        overlay.resolution_group_key = "normalized-server".to_owned();
        overlay.role = AgentAssetDeclarationRole::StateOverlay;
        group_scoped_overlay(&mut overlay);

        let make = |asset: &ParsedAgentAsset| {
            let mut value = draft(
                asset,
                &format!("mcp:{}", asset.native_id),
                AgentAssetResolutionRelation::Independent,
                &[asset, &overlay],
            );
            value.resolution.qualified_collision = true;
            value.declared_state = AgentAssetDeclaredState::Disabled;
            value.effective_state = AgentAssetState::Disabled;
            value.details = AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Unknown,
                declared_state: AgentAssetDeclaredState::Disabled,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: AgentAssetEffectiveAvailability::Disabled,
            };
            value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
                declaration_ids: vec![overlay.declaration_id.clone()],
                scope: crate::services::agent_cli::contracts::AgentAssetStateOverlayScopeDraft::ResolutionGroup,
                outcome: AgentAssetDeclaredState::Disabled,
            };
            value
        };
        let mut upper_draft = make(&upper);
        let mut lower_draft = make(&lower);
        match case {
            "unbound" => {
                upper_draft.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
                    declaration_ids: vec!["missing-overlay".to_owned()],
                    scope: crate::services::agent_cli::contracts::AgentAssetStateOverlayScopeDraft::ResolutionGroup,
                    outcome: AgentAssetDeclaredState::Disabled,
                };
            }
            "wrong-role" => overlay.role = AgentAssetDeclarationRole::Definition,
            "wrong-category" => overlay.category = AgentAssetCategory::Skill,
            "wrong-group" => overlay.resolution_group_key = "other-group".to_owned(),
            "wrong-state" => overlay.declared_state = AgentAssetDeclaredState::Enabled,
            "suppressed" => {
                overlay.participation = AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::CompatibilitySourceDisabled,
                };
            }
            "not-listed" => {
                for value in [&mut upper_draft, &mut lower_draft] {
                    value.represented_declaration_ids = value
                        .represented_declaration_ids
                        .iter()
                        .filter(|id| **id != overlay.declaration_id)
                        .cloned()
                        .collect();
                    value.contributor_ids = value
                        .contributor_ids
                        .iter()
                        .filter(|id| **id != overlay.declaration_id)
                        .cloned()
                        .collect();
                }
            }
            _ => unreachable!(),
        }
        let result = project(
            &sources,
            &[upper, lower, overlay],
            vec![upper_draft, lower_draft],
        );
        assert!(
            result.records.len() < 2,
            "shared overlay negative case {case} was accepted: {:?}",
            result.records
        );
        assert!(
            result.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
            )),
            "shared overlay negative case {case} had no rejection: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn trust_required_accepts_only_the_complete_suppressed_shape() {
    let ctx = context();
    let sources = vec![source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let mut declaration = declaration(
        &ctx,
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    declaration.trust_state = AgentTrustState::Untrusted;
    declaration.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
    };
    let base_draft = || {
        let mut value = draft(
            &declaration,
            "mcp:server",
            AgentAssetResolutionRelation::Unknown,
            &[],
        );
        value.represented_declaration_ids = vec![declaration.declaration_id.clone()];
        value.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
        value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declaration.declaration_id.clone(),
            }],
            cause: crate::services::agent_cli::contracts::AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
        };
        value.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: AgentAssetResolutionTerminal::Unknown,
            cause:
                crate::services::agent_cli::contracts::AgentAssetTerminalCauseDraft::TrustSuppressed,
            evidence: Vec::new(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        value.details = AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state: AgentAssetDeclaredState::Unknown,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
        };
        value
    };
    assert_eq!(
        project(&sources, &[declaration.clone()], vec![base_draft()])
            .records
            .len(),
        1
    );

    type TrustCaseMutation = Box<dyn Fn(&mut ParsedAgentAsset, &mut AgentAssetProjectedDraft)>;
    let mut cases: Vec<(&str, TrustCaseMutation)> = vec![
        (
            "contributor",
            Box::new(|_, draft| {
                draft
                    .contributor_ids
                    .push(draft.represented_declaration_ids[0].clone())
            }),
        ),
        (
            "participating",
            Box::new(|declaration, _| {
                declaration.participation = AgentAssetResolutionParticipation::Participates;
            }),
        ),
        (
            "wrong-suppression",
            Box::new(|declaration, _| {
                declaration.participation = AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::CompatibilitySourceDisabled,
                };
            }),
        ),
        (
            "wrong-definition-trust",
            Box::new(|declaration, _| {
                declaration.trust_state = AgentTrustState::Required;
            }),
        ),
        (
            "wrong-draft-trust",
            Box::new(|_, draft| draft.trust_state = AgentTrustState::Trusted),
        ),
        (
            "wrong-declared-state",
            Box::new(|_, draft| {
                draft.declared_state = AgentAssetDeclaredState::Enabled;
                draft.details = AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Unknown,
                    declared_state: AgentAssetDeclaredState::Enabled,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
                };
            }),
        ),
        (
            "wrong-availability",
            Box::new(|_, draft| {
                draft.details = AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Unknown,
                    declared_state: AgentAssetDeclaredState::Unknown,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: AgentAssetEffectiveAvailability::Unknown,
                };
            }),
        ),
        (
            "winner",
            Box::new(|_, draft| {
                draft.resolution.winner = Some(AgentAssetNativeRef {
                    category: AgentAssetCategory::Mcp,
                    native_id: "server".to_owned(),
                    qualifier: Some("mcp:server".to_owned()),
                })
            }),
        ),
        (
            "control-source",
            Box::new(|_, draft| {
                draft.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
                    source_key: "main".to_owned(),
                });
            }),
        ),
    ];
    for (name, mutate) in cases.drain(..) {
        let mut declaration = declaration.clone();
        let mut draft = base_draft();
        mutate(&mut declaration, &mut draft);
        let result = project(&sources, &[declaration], vec![draft]);
        assert!(
            result.records.is_empty(),
            "TrustRequired case {name} was accepted"
        );
    }
}

#[test]
fn terminal_proof_requires_typed_policy_evidence_and_unique_owner() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("policy", "policy.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut definition = declaration(
        &ctx,
        "definition",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    fixture_input(&mut definition, "fixture_mode = 'unresolved'");
    let mut policy = declaration(
        &ctx,
        "policy",
        "global-policy",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    policy.role = AgentAssetDeclarationRole::PolicyOverlay;
    policy.resolution_group_key = "global-policy".to_owned();
    policy.native_payload = AgentAssetNativePayload::McpPolicy(
        crate::services::agent_cli::contracts::AgentMcpPolicyPayload::Excluded(BTreeSet::from([
            "server".to_owned(),
        ])),
    );
    let mut blocked = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&definition],
    );
    blocked.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
    blocked.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
        declaration_id: policy.declaration_id.clone(),
    });
    blocked.effective_state = AgentAssetState::Blocked;
    blocked.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    blocked.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: policy.declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let result = project(&sources, &[definition, policy], vec![blocked]);
    assert_eq!(result.records.len(), 1, "{:?}", result.diagnostics);
}

#[test]
fn terminal_proof_rejects_forged_source_and_untyped_policy_evidence() {
    let ctx = context();
    let sources = vec![source(
        "definition",
        "definition.json",
        &[AgentAssetCategory::Mcp],
    )];
    let definition = declaration(
        &ctx,
        "definition",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut blocked = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&definition],
    );
    blocked.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
    blocked.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "missing-policy".to_owned(),
    });
    blocked.effective_state = AgentAssetState::Blocked;
    blocked.details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::PolicyBlocked,
    };
    blocked.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::PolicyBlocked,
        cause: AgentAssetTerminalCauseDraft::TypedPolicy,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Source {
            source_key: "missing-policy".to_owned(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let result = project(&sources, &[definition], vec![blocked]);
    assert!(result.records.is_empty());
}

#[test]
fn invalid_control_terminal_accepts_complete_multi_owner_evidence_without_owner() {
    let ctx = context();
    let sources = vec![
        source("definition", "definition.json", &[AgentAssetCategory::Mcp]),
        source("one", "one.json", &[AgentAssetCategory::Mcp]),
        source("two", "two.json", &[AgentAssetCategory::Mcp]),
    ];
    let mut definition = declaration(
        &ctx,
        "definition",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    fixture_input(&mut definition, "fixture_mode = 'unresolved'");
    let mut one = declaration(
        &ctx,
        "one",
        "global-control",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    let mut two = declaration(
        &ctx,
        "two",
        "global-control",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    one.role = AgentAssetDeclarationRole::StateOverlay;
    two.role = AgentAssetDeclarationRole::StateOverlay;
    one.native_payload = AgentAssetNativePayload::InvalidControl(
        crate::services::agent_cli::contracts::AgentAssetInvalidControl::McpEnablement,
    );
    two.native_payload = AgentAssetNativePayload::InvalidControl(
        crate::services::agent_cli::contracts::AgentAssetInvalidControl::McpEnablement,
    );
    let mut value = draft(
        &definition,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&definition],
    );
    value.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
    value.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause: AgentAssetTerminalCauseDraft::InvalidControl,
        evidence: vec![
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: one.declaration_id.clone(),
            },
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: two.declaration_id.clone(),
            },
        ],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
        &mut value.state_proof.effective
    {
        evidence.sort();
    }
    let result = project(&sources, &[definition, one, two], vec![value]);
    assert_eq!(result.records.len(), 1, "{:?}", result.diagnostics);
}

#[test]
fn invalid_control_unique_owner_without_control_source_is_rejected() {
    let ctx = context();
    let sources = vec![source("one", "one.json", &[AgentAssetCategory::Mcp])];
    let mut declaration = declaration(
        &ctx,
        "one",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    declaration.role = AgentAssetDeclarationRole::StateOverlay;
    declaration.native_payload = AgentAssetNativePayload::InvalidControl(
        crate::services::agent_cli::contracts::AgentAssetInvalidControl::McpEnablement,
    );
    let mut value = draft(
        &declaration,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&declaration],
    );
    value.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
    value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declaration.declaration_id.clone(),
        }],
        cause: AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl,
    };
    value.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause: AgentAssetTerminalCauseDraft::InvalidControl,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declaration.declaration_id.clone(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let result = project(&sources, &[declaration], vec![value]);
    assert!(result.records.is_empty());
}

#[test]
fn terminal_cause_composition_rejects_parent_unknown_without_parent_gate() {
    let ctx = context();
    let sources = vec![source("one", "one.json", &[AgentAssetCategory::Mcp])];
    let declaration = declaration(
        &ctx,
        "one",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    let mut value = draft(
        &declaration,
        "mcp:server",
        AgentAssetResolutionRelation::Unknown,
        &[&declaration],
    );
    value.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
    value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declaration.declaration_id.clone(),
        }],
        cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
    };
    value.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause: AgentAssetTerminalCauseDraft::ParentUnknown,
        evidence: Vec::new(),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    let result = project(&sources, &[declaration], vec![value]);
    assert!(result.records.is_empty());
}

#[test]
fn terminal_unknown_causes_reject_evidence_and_control_source() {
    let ctx = context();
    let sources = vec![source("one", "one.json", &[AgentAssetCategory::Mcp])];
    for cause in [
        AgentAssetTerminalCauseDraft::DeclaredUnknown,
        AgentAssetTerminalCauseDraft::StructuralUnknown,
        AgentAssetTerminalCauseDraft::TrustSuppressed,
    ] {
        let declaration = declaration(
            &ctx,
            "one",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Unknown,
        );
        let mut value = draft(
            &declaration,
            "mcp:server",
            AgentAssetResolutionRelation::Unknown,
            &[&declaration],
        );
        value.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
        value.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
            source_key: "one".to_owned(),
        });
        value.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declaration.declaration_id.clone(),
            }],
            cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
        };
        value.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: AgentAssetResolutionTerminal::Unknown,
            cause,
            evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declaration.declaration_id.clone(),
            }],
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        let result = project(&sources, &[declaration], vec![value]);
        assert!(result.records.is_empty(), "cause {cause:?} was accepted");
    }
}

mod assessment;
mod identity;
mod native_projection;
mod policy_scope;
mod source_guards;
