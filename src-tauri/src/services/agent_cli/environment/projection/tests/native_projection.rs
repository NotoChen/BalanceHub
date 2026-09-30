use super::assessment::{alter_assessments, project_using};
use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentIndex, AgentAssetAssessmentSubject,
};

fn native_reference(asset: &ParsedAgentAsset) -> AgentAssetNativeRef {
    AgentAssetNativeRef {
        category: asset.category,
        native_id: asset.native_id.clone(),
        qualifier: Some(format!("{}:{}", asset.category.key(), asset.native_id)),
    }
}

fn replacement_pair(
    winner: &ParsedAgentAsset,
    loser: &ParsedAgentAsset,
) -> [AgentAssetProjectedDraft; 2] {
    let reference = native_reference(winner);
    let mut winner_draft = draft(
        winner,
        reference.qualifier.as_deref().unwrap(),
        AgentAssetResolutionRelation::ReplaceWinner,
        &[winner, loser],
    );
    winner_draft.resolution.winner = Some(reference.clone());
    let mut loser_draft = draft(
        loser,
        &format!(
            "{}:{}@{}",
            loser.category.key(),
            loser.native_id,
            loser.source_key
        ),
        AgentAssetResolutionRelation::Replaced,
        &[loser],
    );
    loser_draft.resolution.winner = Some(reference.clone());
    loser_draft.state_proof.effective = AgentAssetEffectiveStateProofDraft::Shadowed {
        winner: reference,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    [winner_draft, loser_draft]
}

#[test]
fn native_anchor_controls_metadata_across_hash_order_and_source_permutations() {
    for merge in [false, true] {
        let mut anchor_orders = BTreeSet::new();
        for (winner_source, loser_source) in [("one", "two"), ("two", "one")] {
            let mut sources = vec![
                source(
                    "one",
                    "one.json",
                    &[AgentAssetCategory::Mcp, AgentAssetCategory::Skill],
                ),
                source("two", "two.json", &[AgentAssetCategory::Mcp]),
            ];
            let mut winner = declaration(
                &context(),
                winner_source,
                "server",
                AgentAssetCategory::Mcp,
                AgentAssetDeclaredState::Enabled,
            );
            let mut loser = declaration(
                &context(),
                loser_source,
                "server",
                AgentAssetCategory::Mcp,
                AgentAssetDeclaredState::Enabled,
            );
            winner.logical_origin.scope = AgentAssetScope::Workspace;
            prefer_definition(&mut sources, &mut winner);
            if merge {
                fixture_input(&mut winner, "fixture_mode = 'merge'");
                fixture_input(&mut loser, "fixture_mode = 'merge'");
            }
            anchor_orders.insert(winner.declaration_id < loser.declaration_id);
            let neighbor = declaration(
                &context(),
                "one",
                "neighbor",
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            );
            let neighbor_draft = draft(
                &neighbor,
                "skill:neighbor",
                AgentAssetResolutionRelation::Independent,
                &[&neighbor],
            );
            let candidates = if merge {
                vec![
                    draft(
                        &winner,
                        "mcp:server",
                        AgentAssetResolutionRelation::Merged,
                        &[&winner, &loser],
                    ),
                    neighbor_draft.clone(),
                ]
            } else {
                let pair = replacement_pair(&winner, &loser);
                vec![pair[0].clone(), pair[1].clone(), neighbor_draft.clone()]
            };
            let declarations = vec![winner.clone(), loser.clone(), neighbor];
            for reversed in [false, true] {
                let mut sources = sources.clone();
                let mut declarations = declarations.clone();
                let mut candidates = candidates.clone();
                if reversed {
                    sources.reverse();
                    declarations.reverse();
                    candidates.reverse();
                }
                let result = project(&sources, &declarations, candidates);
                assert_rows(
                    &result,
                    if merge {
                        &["server", "neighbor"]
                    } else {
                        &["server", "server", "neighbor"]
                    },
                );
                let selected = result
                    .records
                    .iter()
                    .find(|record| {
                        record.native_id == "server"
                            && record.effective_state != AgentAssetState::Shadowed
                    })
                    .unwrap();
                assert_eq!(selected.scope, AgentAssetScope::Workspace);
                assert_eq!(selected.precedence, 20);
                assert_eq!(
                    selected.path,
                    Some(format!("/tmp/balancehub-fixture/{winner_source}.json"))
                );
                assert_eq!(
                    selected.inspection_source_id,
                    source_stable_id(
                        &context(),
                        &PathBuf::from(selected.path.as_deref().unwrap())
                    )
                );
                if !merge {
                    let shadowed = result
                        .records
                        .iter()
                        .find(|record| record.effective_state == AgentAssetState::Shadowed)
                        .unwrap();
                    assert_eq!(shadowed.scope, AgentAssetScope::User);
                    assert_eq!(shadowed.precedence, 10);
                    assert_eq!(
                        shadowed.path,
                        Some(format!("/tmp/balancehub-fixture/{loser_source}.json"))
                    );
                }
            }
            let mut wrong_inspection = candidates.clone();
            wrong_inspection[0].inspection_source_id = loser.source_key.clone();
            assert_rejected_with_neighbor(&project(&sources, &declarations, wrong_inspection));
            if !merge {
                let mut forged_merge = candidates[0].clone();
                forged_merge.resolution.relation = AgentAssetResolutionRelation::Merged;
                forged_merge.resolution.winner = None;
                assert_rejected_with_neighbor(&project(
                    &sources,
                    &declarations,
                    vec![forged_merge, neighbor_draft],
                ));
            }
        }
        assert_eq!(anchor_orders, BTreeSet::from([false, true]));
    }
}

#[test]
fn merged_and_collision_winner_cannot_forge_record_trust() {
    for trust in [
        AgentTrustState::Unknown,
        AgentTrustState::Required,
        AgentTrustState::Untrusted,
    ] {
        for collision in [false, true] {
            let mut sources = vec![
                source("one", "one.json", &[AgentAssetCategory::Skill]),
                source("two", "two.json", &[AgentAssetCategory::Skill]),
            ];
            let native_id = if collision { "Server" } else { "server" };
            let mut winner = declaration(
                &context(),
                "one",
                native_id,
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            );
            let mut loser = declaration(
                &context(),
                "two",
                native_id,
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            );
            winner.trust_state = trust;
            loser.trust_state = trust;
            prefer_definition(&mut sources, &mut winner);
            winner.resolution_group_key = "server".to_owned();
            loser.resolution_group_key = "server".to_owned();
            if !collision {
                fixture_input(&mut winner, "fixture_mode = 'merge'");
                fixture_input(&mut loser, "fixture_mode = 'merge'");
            }
            let neighbor = declaration(
                &context(),
                "one",
                "neighbor",
                AgentAssetCategory::Skill,
                AgentAssetDeclaredState::Enabled,
            );
            let neighbor_draft = draft(
                &neighbor,
                "skill:neighbor",
                AgentAssetResolutionRelation::Independent,
                &[&neighbor],
            );
            let mut drafts = if collision {
                replacement_pair(&winner, &loser)
                    .into_iter()
                    .collect::<Vec<_>>()
            } else {
                vec![draft(
                    &winner,
                    "skill:server",
                    AgentAssetResolutionRelation::Merged,
                    &[&winner, &loser],
                )]
            };
            let mut declarations = vec![winner, loser, neighbor];
            if collision {
                let mut exact_neighbor = declaration(
                    &context(),
                    "one",
                    "server",
                    AgentAssetCategory::Skill,
                    AgentAssetDeclaredState::Enabled,
                );
                exact_neighbor.resolution_group_key = "server".to_owned();
                drafts.push(draft(
                    &exact_neighbor,
                    "skill:server",
                    AgentAssetResolutionRelation::Independent,
                    &[&exact_neighbor],
                ));
                declarations.push(exact_neighbor);
                for draft in &mut drafts {
                    draft.resolution.qualified_collision = true;
                }
            }
            drafts.push(neighbor_draft);
            assert_rows(
                &project(&sources, &declarations, drafts.clone()),
                if collision {
                    &["Server", "Server", "server", "neighbor"]
                } else {
                    &["server", "neighbor"]
                },
            );
            drafts[0].trust_state = AgentTrustState::Trusted;
            let result = project(&sources, &declarations, drafts);
            assert_rows(
                &result,
                if collision {
                    &["server", "neighbor"]
                } else {
                    &["neighbor"]
                },
            );
            assert!(result.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidResolution { .. }
            )));
        }
    }
}

fn pending_mcp(
    source_key: &str,
    approval: AgentMcpApprovalState,
    transport: AgentMcpTransport,
) -> ParsedAgentAsset {
    let mut asset = declaration(
        &context(),
        source_key,
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    asset.details = AgentAssetDetails::Mcp {
        transport,
        declared_state: AgentAssetDeclaredState::Enabled,
        approval_state: approval,
        effective_availability: if approval == AgentMcpApprovalState::Pending {
            AgentAssetEffectiveAvailability::ApprovalRequired
        } else {
            AgentAssetEffectiveAvailability::Available
        },
    };
    asset
}

#[test]
fn replaced_and_additive_subjects_keep_their_own_intrinsic_basis() {
    for additive in [false, true] {
        let mut sources = vec![
            source(
                "one",
                "one.json",
                &[AgentAssetCategory::Mcp, AgentAssetCategory::Skill],
            ),
            source("two", "two.json", &[AgentAssetCategory::Mcp]),
        ];
        let mut approved = pending_mcp(
            "one",
            AgentMcpApprovalState::Approved,
            AgentMcpTransport::Http,
        );
        let mut pending = pending_mcp(
            "two",
            AgentMcpApprovalState::Pending,
            AgentMcpTransport::Stdio,
        );
        let neighbor = declaration(
            &context(),
            "one",
            "neighbor",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        );
        if additive {
            fixture_input(&mut approved, "fixture_mode = 'additive'");
            fixture_input(&mut pending, "fixture_mode = 'additive'");
        } else {
            prefer_definition(&mut sources, &mut approved);
        }
        let mut drafts = if additive {
            let available = draft(
                &approved,
                "mcp:server@one",
                AgentAssetResolutionRelation::Additive,
                &[&approved],
            );
            let mut needs_approval = draft(
                &pending,
                "mcp:server@two",
                AgentAssetResolutionRelation::Additive,
                &[&pending],
            );
            needs_approval.effective_state = AgentAssetState::Unknown;
            vec![available, needs_approval]
        } else {
            replacement_pair(&approved, &pending)
                .into_iter()
                .collect::<Vec<_>>()
        };
        drafts.push(draft(
            &neighbor,
            "skill:neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&neighbor],
        ));
        let declarations = vec![approved, pending, neighbor];
        let baseline = project(&sources, &declarations, drafts.clone());
        assert_rows(&baseline, &["server", "server", "neighbor"]);
        let second_source = source_stable_id(&context(), &sources[1].path);
        let pending_record = baseline
            .records
            .iter()
            .find(|record| record.inspection_source_id == second_source)
            .unwrap();
        assert!(matches!(
            pending_record.details,
            AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Stdio,
                approval_state: AgentMcpApprovalState::Pending,
                effective_availability: AgentAssetEffectiveAvailability::ApprovalRequired,
                ..
            }
        ));
        assert_eq!(
            pending_record.effective_state,
            if additive {
                AgentAssetState::Unknown
            } else {
                AgentAssetState::Shadowed
            }
        );
        drafts[1].details = declarations[0].details.clone();
        if additive {
            drafts[1].effective_state = AgentAssetState::Enabled;
        }
        let result = project(&sources, &declarations, drafts);
        assert_rows(&result, &["server", "neighbor"]);
        assert!(result
            .records
            .iter()
            .all(|record| record.inspection_source_id != second_source));
    }
}

fn gated_inputs(
    parent_state: AgentAssetDeclaredState,
) -> (
    Vec<AgentAssetSourceSpec>,
    Vec<ParsedAgentAsset>,
    Vec<AgentAssetProjectedDraft>,
) {
    let sources = vec![source(
        "main",
        "main.json",
        &[
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Extension,
            AgentAssetCategory::Skill,
        ],
    )];
    let parent = declaration(
        &context(),
        "main",
        "bundle",
        AgentAssetCategory::Extension,
        parent_state,
    );
    let mut child = pending_mcp(
        "main",
        AgentMcpApprovalState::Approved,
        AgentMcpTransport::Stdio,
    );
    child.provided_by = Some(native_reference(&parent));
    fixture_input(&mut child, "parent_gate = true");
    let neighbor = declaration(
        &context(),
        "main",
        "neighbor",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let parent_draft = draft(
        &parent,
        "extension:bundle",
        AgentAssetResolutionRelation::Independent,
        &[&parent],
    );
    let mut child_draft = draft(
        &child,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&child],
    );
    child_draft.state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
        parent: child.provided_by.clone().unwrap(),
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    if parent_state != AgentAssetDeclaredState::Enabled {
        child_draft.effective_state = if parent_state == AgentAssetDeclaredState::Disabled {
            AgentAssetState::Disabled
        } else {
            AgentAssetState::Unknown
        };
        if let AgentAssetDetails::Mcp {
            effective_availability,
            ..
        } = &mut child_draft.details
        {
            *effective_availability = if parent_state == AgentAssetDeclaredState::Disabled {
                AgentAssetEffectiveAvailability::Disabled
            } else {
                AgentAssetEffectiveAvailability::Unknown
            };
        }
    }
    let neighbor_draft = draft(
        &neighbor,
        "skill:neighbor",
        AgentAssetResolutionRelation::Independent,
        &[&neighbor],
    );
    (
        sources,
        vec![parent, child, neighbor],
        vec![parent_draft, child_draft, neighbor_draft],
    )
}

#[test]
fn native_projection_rejects_omitted_or_added_parent_gates_and_unrelated_relationships() {
    let (sources, mut declarations, mut drafts) = gated_inputs(AgentAssetDeclaredState::Disabled);
    let standalone = declaration(
        &context(),
        "main",
        "standalone",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    drafts.push(draft(
        &standalone,
        "skill:standalone",
        AgentAssetResolutionRelation::Independent,
        &[&standalone],
    ));
    declarations.push(standalone);
    assert_rows(
        &project(&sources, &declarations, drafts.clone()),
        &["bundle", "server", "neighbor", "standalone"],
    );
    let mut omitted = drafts.clone();
    omitted[1].state_proof.effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
    omitted[1].effective_state = AgentAssetState::Enabled;
    if let AgentAssetDetails::Mcp {
        effective_availability,
        ..
    } = &mut omitted[1].details
    {
        *effective_availability = AgentAssetEffectiveAvailability::Available;
    }
    assert_rows(
        &project(&sources, &declarations, omitted),
        &["bundle", "neighbor", "standalone"],
    );

    for attack in ["provider", "action-owner", "impact", "add-gate"] {
        let mut forged = drafts.clone();
        let parent = native_reference(&declarations[0]);
        match attack {
            "provider" => forged[3].provided_by = Some(parent),
            "action-owner" => forged[3].action_owner = Some(parent),
            "impact" => forged[3].explicitly_affected.push(parent),
            "add-gate" => {
                forged[3].provided_by = Some(parent.clone());
                forged[3].state_proof.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
                    parent,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
                forged[3].effective_state = AgentAssetState::Disabled;
            }
            _ => unreachable!(),
        }
        assert_rows(
            &project(&sources, &declarations, forged),
            &["bundle", "server", "neighbor"],
        );
    }
}

fn missing_parent_basis(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    let targets = request
        .targets
        .iter()
        .filter(|target| target.exact_native_id != "bundle")
        .cloned()
        .collect::<Vec<_>>();
    fixture_assessor(AgentAssetAssessmentRequest {
        targets: &targets,
        ..request
    })
}

fn invalid_parent_basis(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    alter_assessments(request, |target, result| {
        if target.exact_native_id == "bundle" {
            if let AgentAssetAssessmentResult::Assessed { state, .. } = result {
                state.intrinsic.details = AgentAssetDetails::Skill {
                    enabled: state.declared_state,
                    invocation_policy: AgentSkillInvocationPolicy::ManualOnly,
                };
            }
        }
    })
}

#[test]
fn assessor_missing_or_invalid_parent_basis_rejects_parent_and_dependents_only() {
    let (sources, declarations, drafts) = gated_inputs(AgentAssetDeclaredState::Disabled);
    assert_rows(
        &project(&sources, &declarations, drafts.clone()),
        &["bundle", "server", "neighbor"],
    );
    for assessor in [
        missing_parent_basis as AgentAssetStateAssessor,
        invalid_parent_basis,
    ] {
        assert_rejected_with_neighbor(&project_using(
            &sources,
            &declarations,
            drafts.clone(),
            assessor,
        ));
    }
}

#[test]
fn forged_parent_and_child_availability_cannot_replace_native_parent_basis() {
    let (sources, declarations, mut drafts) = gated_inputs(AgentAssetDeclaredState::Disabled);
    assert_rows(
        &project(&sources, &declarations, drafts.clone()),
        &["bundle", "server", "neighbor"],
    );
    drafts[0].declared_state = AgentAssetDeclaredState::Enabled;
    drafts[0].effective_state = AgentAssetState::Enabled;
    if let AgentAssetDetails::Extension { enabled, .. } = &mut drafts[0].details {
        *enabled = AgentAssetDeclaredState::Enabled;
    }
    drafts[1].effective_state = AgentAssetState::Enabled;
    if let AgentAssetDetails::Mcp {
        effective_availability,
        ..
    } = &mut drafts[1].details
    {
        *effective_availability = AgentAssetEffectiveAvailability::Available;
    }
    assert_rejected_with_neighbor(&project(&sources, &declarations, drafts));
}

fn invalid_anchor<const CASE: usize>(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let key = match CASE {
        0 => None,
        1 => Some("policy-anchor"),
        2 => Some("side"),
        3 => Some("other"),
        4 => Some("suppressed"),
        5 => Some("unrepresented"),
        _ => unreachable!(),
    };
    let forged_id = key
        .and_then(|key| {
            request
                .declarations
                .iter()
                .find(|asset| asset.declaration_key == key)
        })
        .map(|asset| asset.declaration_id.clone())
        .unwrap_or_else(|| "declaration:missing".to_owned());
    alter_assessments(request, |target, result| {
        if target.category == AgentAssetCategory::Mcp
            && target.exact_native_id == "server"
            && target.subject == AgentAssetAssessmentSubject::Bucket
        {
            if let AgentAssetAssessmentResult::Assessed { projection, .. } = result {
                projection.anchor_declaration_id = forged_id.clone();
            }
        }
    })
}

#[test]
fn assessor_anchor_requires_valid_definition_identity_scope_participation_and_membership() {
    let sources = vec![
        source(
            "main",
            "main.json",
            &[AgentAssetCategory::Mcp, AgentAssetCategory::Skill],
        ),
        source("off", "off.json", &[AgentAssetCategory::Mcp]),
    ];
    let target = declaration(
        &context(),
        "main",
        "server",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let neighbor = declaration(
        &context(),
        "main",
        "neighbor",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let side = declaration(
        &context(),
        "main",
        "side",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let other = declaration(
        &context(),
        "main",
        "other",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    let mut policy = declaration_with_key(
        &context(),
        "main",
        "server",
        "policy-anchor",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Enabled,
    );
    policy.role = AgentAssetDeclarationRole::PolicyOverlay;
    let mut suppressed = declaration_with_key(
        &context(),
        "off",
        "server",
        "suppressed",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    suppressed.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
    };
    let mut unrepresented = declaration_with_key(
        &context(),
        "off",
        "server",
        "unrepresented",
        AgentAssetCategory::Mcp,
        AgentAssetDeclaredState::Unknown,
    );
    unrepresented.participation = suppressed.participation;
    unrepresented.resolution_group_key = "unrepresented-group".to_owned();
    let mut target_draft = draft(
        &target,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&target, &policy],
    );
    target_draft
        .represented_declaration_ids
        .push(suppressed.declaration_id.clone());
    let drafts = vec![
        target_draft,
        draft(
            &neighbor,
            "skill:neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&neighbor],
        ),
        draft(
            &side,
            "skill:side",
            AgentAssetResolutionRelation::Independent,
            &[&side],
        ),
        draft(
            &other,
            "mcp:other",
            AgentAssetResolutionRelation::Independent,
            &[&other],
        ),
    ];
    let declarations = vec![
        target,
        neighbor,
        side,
        other,
        policy,
        suppressed,
        unrepresented,
    ];
    assert_rows(
        &project(&sources, &declarations, drafts.clone()),
        &["server", "neighbor", "side", "other"],
    );
    for assessor in [
        invalid_anchor::<0> as AgentAssetStateAssessor,
        invalid_anchor::<1>,
        invalid_anchor::<2>,
        invalid_anchor::<3>,
        invalid_anchor::<4>,
        invalid_anchor::<5>,
    ] {
        assert_rows(
            &project_using(&sources, &declarations, drafts.clone(), assessor),
            &["neighbor", "side", "other"],
        );
    }
    let mut suppressed_inspection = drafts;
    suppressed_inspection[0].inspection_source_id = "off".to_owned();
    assert_rows(
        &project(&sources, &declarations, suppressed_inspection),
        &["neighbor", "side", "other"],
    );
}
