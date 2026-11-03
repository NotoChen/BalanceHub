use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentIndex, AgentAssetAssessmentSubject, AgentAssetAssessmentTarget,
    AgentMcpPolicyPayload,
};

pub(super) fn project_using(
    sources: &[AgentAssetSourceSpec],
    declarations: &[ParsedAgentAsset],
    drafts: Vec<AgentAssetProjectedDraft>,
    state_assessor: AgentAssetStateAssessor,
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
            state_assessor,
        },
        &BTreeMap::new(),
        &mut run,
    );
    ProjectTestResult {
        records: result.records,
        diagnostics: run.finish_diagnostics(),
    }
}

/// Deliberately corrupt only the native result at the assessor contract
/// boundary. The original parser inputs and every neighboring result survive;
/// candidate drafts are neither passed here nor used to derive native facts.
pub(super) fn alter_assessments(
    request: AgentAssetAssessmentRequest<'_>,
    mut alter: impl FnMut(&AgentAssetAssessmentTarget, &mut AgentAssetAssessmentResult),
) -> AgentAssetAssessmentIndex {
    let native = fixture_assessor(AgentAssetAssessmentRequest { ..request });
    let mut result = AgentAssetAssessmentIndex::default();
    for target in request.targets {
        let mut value = native.get(target).unwrap().clone();
        alter(target, &mut value);
        result.insert(target.clone(), value);
    }
    result
}

#[test]
fn duplicate_assessment_is_sticky_for_same_and_different_results() {
    let target = AgentAssetAssessmentTarget {
        category: AgentAssetCategory::Mcp,
        resolution_group_key: "group".to_owned(),
        exact_native_id: "server".to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let declaration = declaration(
        &context(),
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let source = source("main", "main.json", &[AgentAssetCategory::Mcp]);
    let mut native_target = target.clone();
    native_target.resolution_group_key = "server".to_owned();
    let native = fixture_assessor(AgentAssetAssessmentRequest {
        context: &context(),
        targets: &[native_target.clone()],
        declarations: &[declaration],
        sources: &[source],
    });
    let valid = native.get(&native_target).unwrap().clone();
    assert!(matches!(valid, AgentAssetAssessmentResult::Assessed { .. }));
    for second in [
        valid.clone(),
        AgentAssetAssessmentResult::Unsupported(AgentAssetAssessmentFailure::IncompleteInput),
    ] {
        let mut index = AgentAssetAssessmentIndex::default();
        index.insert(target.clone(), valid.clone());
        assert_eq!(index.get(&target), Some(&valid));
        index.insert(target.clone(), second);
        let duplicate =
            AgentAssetAssessmentResult::Unsupported(AgentAssetAssessmentFailure::DuplicateTarget);
        assert_eq!(index.get(&target), Some(&duplicate));
        index.insert(target.clone(), valid.clone());
        assert_eq!(index.get(&target), Some(&duplicate));
        for separate in [
            AgentAssetAssessmentTarget {
                resolution_group_key: "other".to_owned(),
                ..target.clone()
            },
            AgentAssetAssessmentTarget {
                subject: AgentAssetAssessmentSubject::Definition("id".to_owned()),
                ..target.clone()
            },
        ] {
            index.insert(separate.clone(), valid.clone());
            assert_eq!(index.get(&separate), Some(&valid));
        }
    }
}

fn missing_one(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    let targets = request
        .targets
        .iter()
        .filter(|target| target.exact_native_id != "server")
        .cloned()
        .collect::<Vec<_>>();
    fixture_assessor(AgentAssetAssessmentRequest {
        targets: &targets,
        ..request
    })
}

fn duplicate_one(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    let mut index = fixture_assessor(AgentAssetAssessmentRequest { ..request });
    for target in request
        .targets
        .iter()
        .filter(|target| target.exact_native_id == "server")
    {
        let value = index.get(target).unwrap().clone();
        index.insert(target.clone(), value.clone());
        index.insert(target.clone(), value);
    }
    index
}

fn unsupported_one(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    alter_assessments(request, |target, result| {
        if target.exact_native_id == "server" {
            *result = AgentAssetAssessmentResult::Unsupported(
                AgentAssetAssessmentFailure::IncompleteInput,
            );
        }
    })
}

#[test]
fn missing_duplicate_or_unsupported_assessment_rejects_only_its_target() {
    let sources = [source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let declarations = [
        declaration(
            &context(),
            "main",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        ),
        declaration(
            &context(),
            "main",
            "neighbor",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Unknown,
        ),
    ];
    let drafts = declarations
        .iter()
        .map(|asset| {
            draft(
                asset,
                &asset.native_id,
                AgentAssetResolutionRelation::Independent,
                &[asset],
            )
        })
        .collect::<Vec<_>>();
    for (assessor, failure) in [
        (
            missing_one as AgentAssetStateAssessor,
            AgentAssetAssessmentFailure::MissingTarget,
        ),
        (duplicate_one, AgentAssetAssessmentFailure::DuplicateTarget),
        (
            unsupported_one,
            AgentAssetAssessmentFailure::IncompleteInput,
        ),
    ] {
        let result = project_using(&sources, &declarations, drafts.clone(), assessor);
        assert_eq!(result.records.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.records[0].native_id, "neighbor");
        assert_eq!(result.records[0].effective_state, AgentAssetState::Unknown);
        let expected_key = format!("server:assessment:{failure:?}");
        assert!(result.diagnostics.iter().any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::InvalidResolution { projection_key, .. } if projection_key == &expected_key)));
    }
}

#[test]
fn intrinsic_details_cannot_be_forged_when_state_stays_the_same() {
    for category in [
        AgentAssetCategory::Mcp,
        AgentAssetCategory::Skill,
        AgentAssetCategory::Hook,
        AgentAssetCategory::Plugin,
        AgentAssetCategory::Extension,
        AgentAssetCategory::StatusUi,
    ] {
        let sources = [source(
            "main",
            "main.json",
            &[category, AgentAssetCategory::Skill],
        )];
        let declarations = [
            declaration(
                &context(),
                "main",
                "target",
                category,
                AgentAssetDeclaredState::Enabled,
            ),
            declaration(
                &context(),
                "main",
                "neighbor",
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            ),
        ];
        let base = draft(
            &declarations[0],
            "target",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[0]],
        );
        let neighbor = draft(
            &declarations[1],
            "neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&declarations[1]],
        );
        assert_eq!(
            project(
                &sources,
                &declarations,
                vec![base.clone(), neighbor.clone()]
            )
            .records
            .len(),
            2
        );
        let mut forged = base;
        match &mut forged.details {
            AgentAssetDetails::Mcp { transport, .. } => *transport = AgentMcpTransport::Http,
            AgentAssetDetails::Skill {
                invocation_policy, ..
            } => *invocation_policy = AgentSkillInvocationPolicy::ManualOnly,
            AgentAssetDetails::Hook { managed, .. } => *managed = true,
            AgentAssetDetails::Plugin { install_state, .. } => {
                *install_state = crate::models::AgentAssetInstallState::Unknown
            }
            AgentAssetDetails::Extension { trusted, .. } => *trusted = AgentTrustState::Trusted,
            AgentAssetDetails::StatusUi {
                mode,
                command_present,
            } => {
                *mode = AgentStatusUiMode::Command;
                *command_present = true;
            }
        }
        let result = project(&sources, &declarations, vec![forged, neighbor]);
        assert_eq!(
            result.records.len(),
            1,
            "category {category:?}: {:?}",
            result.diagnostics
        );
        assert_eq!(result.records[0].native_id, "neighbor");
    }
}

#[test]
fn approval_cannot_be_forged_under_intrinsic_policy_or_parent_effects() {
    for mode in ["intrinsic", "policy", "parent", "parent-enabled"] {
        let sources = [source(
            "main",
            "main.json",
            &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Extension,
                AgentAssetCategory::Skill,
            ],
        )];
        let mut target = declaration(
            &context(),
            "main",
            "server",
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Enabled,
        );
        target.details = AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Stdio,
            declared_state: AgentAssetDeclaredState::Enabled,
            approval_state: AgentMcpApprovalState::Pending,
            effective_availability: AgentAssetEffectiveAvailability::ApprovalRequired,
        };
        let neighbor = declaration(
            &context(),
            "main",
            "neighbor",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        );
        let neighbor_draft = draft(
            &neighbor,
            "neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&neighbor],
        );
        let mut candidate = draft(
            &target,
            "server",
            AgentAssetResolutionRelation::Independent,
            &[&target],
        );
        candidate.effective_state = AgentAssetState::Unknown;
        let mut declarations = vec![target, neighbor];
        let mut companions = vec![neighbor_draft];
        match mode {
            "policy" => {
                let mut policy = declaration_with_key(
                    &context(),
                    "main",
                    "server",
                    "policy",
                    AgentAssetCategory::Mcp,
                    AgentAssetDeclaredState::Enabled,
                );
                policy.role = AgentAssetDeclarationRole::PolicyOverlay;
                policy.native_payload = AgentAssetNativePayload::McpPolicy(
                    AgentMcpPolicyPayload::Excluded(BTreeSet::from(["server".to_owned()])),
                );
                candidate
                    .represented_declaration_ids
                    .push(policy.declaration_id.clone());
                candidate
                    .contributor_ids
                    .push(policy.declaration_id.clone());
                candidate.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
                candidate.resolution.control_source =
                    Some(AgentAssetPolicyReferenceDraft::Declaration {
                        declaration_id: policy.declaration_id.clone(),
                    });
                candidate.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
                    terminal: AgentAssetResolutionTerminal::PolicyBlocked,
                    cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                    evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                        declaration_id: policy.declaration_id.clone(),
                    }],
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
                candidate.effective_state = AgentAssetState::Blocked;
                if let AgentAssetDetails::Mcp {
                    effective_availability,
                    ..
                } = &mut candidate.details
                {
                    *effective_availability = AgentAssetEffectiveAvailability::PolicyBlocked;
                }
                declarations.push(policy);
            }
            "parent" | "parent-enabled" => {
                let parent = declaration(
                    &context(),
                    "main",
                    "bundle",
                    AgentAssetCategory::Extension,
                    if mode == "parent-enabled" {
                        AgentAssetDeclaredState::Enabled
                    } else {
                        AgentAssetDeclaredState::Disabled
                    },
                );
                companions.push(draft(
                    &parent,
                    "bundle",
                    AgentAssetResolutionRelation::Independent,
                    &[&parent],
                ));
                let reference = AgentAssetNativeRef {
                    category: AgentAssetCategory::Extension,
                    native_id: "bundle".to_owned(),
                    qualifier: Some("bundle".to_owned()),
                };
                declarations[0].provided_by = Some(reference.clone());
                fixture_input(&mut declarations[0], "parent_gate = true");
                candidate.provided_by = Some(reference.clone());
                candidate.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
                    parent: reference,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
                if mode == "parent" {
                    candidate.effective_state = AgentAssetState::Disabled;
                    if let AgentAssetDetails::Mcp {
                        effective_availability,
                        ..
                    } = &mut candidate.details
                    {
                        *effective_availability = AgentAssetEffectiveAvailability::Disabled;
                    }
                }
                declarations.push(parent);
            }
            _ => {}
        }
        let mut valid = companions.clone();
        valid.push(candidate.clone());
        let result = project(&sources, &declarations, valid);
        assert_eq!(
            result.records.len(),
            companions.len() + 1,
            "{mode}: {:?}",
            result.diagnostics
        );
        if mode == "parent-enabled" {
            let mut forged_availability = candidate.clone();
            if let AgentAssetDetails::Mcp {
                effective_availability,
                ..
            } = &mut forged_availability.details
            {
                *effective_availability = AgentAssetEffectiveAvailability::Unknown;
            }
            let mut candidates = companions.clone();
            candidates.push(forged_availability);
            let result = project(&sources, &declarations, candidates);
            assert_eq!(result.records.len(), companions.len());
            assert!(result
                .records
                .iter()
                .all(|asset| asset.native_id != "server"));
        }
        if let AgentAssetDetails::Mcp {
            approval_state,
            effective_availability,
            ..
        } = &mut candidate.details
        {
            *approval_state = AgentMcpApprovalState::Approved;
            if matches!(mode, "intrinsic" | "parent-enabled") {
                *effective_availability = AgentAssetEffectiveAvailability::Available;
                candidate.effective_state = AgentAssetState::Enabled;
            }
        }
        companions.push(candidate);
        let result = project(&sources, &declarations, companions);
        assert!(
            result
                .records
                .iter()
                .all(|asset| asset.native_id != "server"),
            "{mode}"
        );
        assert!(result
            .records
            .iter()
            .any(|asset| asset.native_id == "neighbor"));
    }
}

#[test]
fn same_source_controls_remain_distinct_and_require_complete_evidence() {
    let sources = [source("main", "main.json", &[AgentAssetCategory::Mcp])];
    let target = declaration(
        &context(),
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut declarations = vec![target.clone()];
    for key in ["control-one", "control-two"] {
        let mut control = declaration_with_key(
            &context(),
            "main",
            "global",
            key,
            AgentAssetCategory::Mcp,
            AgentAssetDeclaredState::Unknown,
        );
        control.role = AgentAssetDeclarationRole::StateOverlay;
        control.native_payload = AgentAssetNativePayload::InvalidControl(
            crate::services::agent_cli::contracts::AgentAssetInvalidControl::McpEnablement,
        );
        declarations.push(control);
    }
    let mut candidate = draft(
        &target,
        "server",
        AgentAssetResolutionRelation::Independent,
        &[&target],
    );
    candidate.effective_state = AgentAssetState::Unknown;
    candidate.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
    if let AgentAssetDetails::Mcp {
        effective_availability,
        ..
    } = &mut candidate.details
    {
        *effective_availability = AgentAssetEffectiveAvailability::Unknown;
    }
    candidate.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause: AgentAssetTerminalCauseDraft::InvalidControl,
        evidence: vec![AgentAssetStateEvidenceRefDraft::Source {
            source_key: "main".to_owned(),
        }],
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    assert_eq!(
        project(&sources, &declarations, vec![candidate.clone()])
            .records
            .len(),
        1
    );
    let mut forged_owner = candidate.clone();
    forged_owner.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Source {
        source_key: "main".to_owned(),
    });
    assert!(project(&sources, &declarations, vec![forged_owner])
        .records
        .is_empty());
    for evidence in [
        vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declarations[1].declaration_id.clone(),
        }],
        vec![
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: declarations[1].declaration_id.clone(),
            },
            AgentAssetStateEvidenceRefDraft::Source {
                source_key: "main".to_owned(),
            },
        ],
    ] {
        let mut incomplete = candidate.clone();
        if let AgentAssetEffectiveStateProofDraft::Terminal {
            evidence: actual, ..
        } = &mut incomplete.state_proof.effective
        {
            *actual = evidence;
        }
        assert!(project(&sources, &declarations, vec![incomplete])
            .records
            .is_empty());
    }
}
