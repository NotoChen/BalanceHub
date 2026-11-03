use super::assessment::{alter_assessments, project_using};
use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentIndex, AgentAssetStateOverlayScopeDraft,
};

fn policy_input(
    source_key: &str,
    key: &str,
    category: AgentAssetCategory,
    input: &str,
) -> ParsedAgentAsset {
    let mut policy = declaration_with_key(
        &context(),
        source_key,
        key,
        key,
        category,
        AgentAssetDeclaredState::Enabled,
    );
    policy.role = AgentAssetDeclarationRole::PolicyOverlay;
    fixture_input(&mut policy, input);
    policy
}

#[test]
fn known_declared_policy_accepts_cross_group_complete_peers_and_rejects_draft_substitutions() {
    let sources = [source(
        "main",
        "main.json",
        &[
            AgentAssetCategory::Hook,
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Skill,
        ],
    )];
    let target = declaration(
        &context(),
        "main",
        "hook",
        AgentAssetCategory::Hook,
        AgentAssetDeclaredState::Enabled,
    );
    let neighbor = declaration(
        &context(),
        "main",
        "neighbor",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let one = policy_input(
        "main",
        "global-one",
        AgentAssetCategory::Hook,
        "declared_disable = true\npolicy_targets = ['hook']",
    );
    let two = policy_input(
        "main",
        "global-two",
        AgentAssetCategory::Hook,
        "declared_disable = true\npolicy_targets = ['hook']",
    );
    let wrong_scope = policy_input(
        "main",
        "other-target",
        AgentAssetCategory::Hook,
        "declared_disable = true\npolicy_targets = ['different']",
    );
    let mut wrong_role = policy_input(
        "main",
        "wrong-role",
        AgentAssetCategory::Hook,
        "declared_disable = true\npolicy_targets = ['hook']",
    );
    wrong_role.role = AgentAssetDeclarationRole::StateOverlay;
    let wrong_category = policy_input(
        "main",
        "wrong-category",
        AgentAssetCategory::Mcp,
        "declared_disable = true\npolicy_targets = ['hook']",
    );
    let mut suppressed = policy_input(
        "main",
        "suppressed",
        AgentAssetCategory::Hook,
        "declared_disable = true\npolicy_targets = ['hook']",
    );
    suppressed.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UnsupportedContext,
    };
    let mut candidate = draft(
        &target,
        "hook:hook",
        AgentAssetResolutionRelation::Independent,
        &[&target],
    );
    candidate.declared_state = AgentAssetDeclaredState::Disabled;
    candidate.effective_state = AgentAssetState::Disabled;
    candidate.details = AgentAssetDetails::Hook {
        managed: false,
        enabled: AgentAssetDeclaredState::Disabled,
        rule_count: None,
    };
    let mut policy_ids = vec![one.declaration_id.clone(), two.declaration_id.clone()];
    policy_ids.sort();
    candidate.state_proof.declared = AgentAssetDeclaredStateProofDraft::Policy {
        declaration_ids: policy_ids.clone(),
        outcome: AgentAssetDeclaredState::Disabled,
    };
    let neighbor_draft = draft(
        &neighbor,
        "skill:neighbor",
        AgentAssetResolutionRelation::Independent,
        &[&neighbor],
    );
    let decoy_ids = [&wrong_scope, &wrong_role, &wrong_category, &suppressed]
        .map(|asset| asset.declaration_id.clone());
    let declarations = vec![
        target,
        neighbor,
        one,
        two,
        wrong_scope,
        wrong_role,
        wrong_category,
        suppressed,
    ];
    let baseline = project(
        &sources,
        &declarations,
        vec![candidate.clone(), neighbor_draft.clone()],
    );
    assert_rows(&baseline, &["hook", "neighbor"]);
    let hook = baseline
        .records
        .iter()
        .find(|record| record.native_id == "hook")
        .unwrap();
    assert_eq!(
        hook.resolution.contributor_ids,
        vec![declarations[0].declaration_id.clone()]
    );
    assert_eq!(
        hook.represented_declaration_ids,
        vec![declarations[0].declaration_id.clone()]
    );
    assert_eq!(hook.effective_state, AgentAssetState::Disabled);

    let mut attacks = vec![
        vec![policy_ids[0].clone()],
        vec![
            policy_ids[0].clone(),
            policy_ids[0].clone(),
            policy_ids[1].clone(),
        ],
    ];
    for decoy in decoy_ids
        .into_iter()
        .chain(["declaration:other-context".to_owned()])
    {
        attacks.push(vec![policy_ids[0].clone(), decoy]);
    }
    for mut ids in attacks {
        ids.sort();
        let mut forged = candidate.clone();
        forged.state_proof.declared = AgentAssetDeclaredStateProofDraft::Policy {
            declaration_ids: ids,
            outcome: AgentAssetDeclaredState::Disabled,
        };
        assert_rejected_with_neighbor(&project(
            &sources,
            &declarations,
            vec![forged, neighbor_draft.clone()],
        ));
    }
    for use_unknown_cause in [false, true] {
        let mut forged = candidate.clone();
        forged.declared_state = AgentAssetDeclaredState::Unknown;
        forged.effective_state = AgentAssetState::Unknown;
        forged.details = AgentAssetDetails::Hook {
            managed: false,
            enabled: AgentAssetDeclaredState::Unknown,
            rule_count: None,
        };
        forged.state_proof.declared = if use_unknown_cause {
            AgentAssetDeclaredStateProofDraft::Unknown {
                evidence: policy_ids
                    .iter()
                    .map(|id| AgentAssetStateEvidenceRefDraft::Declaration {
                        declaration_id: id.clone(),
                    })
                    .collect(),
                cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            }
        } else {
            AgentAssetDeclaredStateProofDraft::Policy {
                declaration_ids: policy_ids.clone(),
                outcome: AgentAssetDeclaredState::Unknown,
            }
        };
        assert_rejected_with_neighbor(&project(
            &sources,
            &declarations,
            vec![forged, neighbor_draft.clone()],
        ));
    }
}

#[test]
fn context_unavailable_requires_complete_scoped_peers_and_cannot_claim_invalid_input() {
    for invalid in [false, true] {
        let sources = [source(
            "main",
            "main.json",
            &[AgentAssetCategory::Extension, AgentAssetCategory::Skill],
        )];
        let mut target = declaration(
            &context(),
            "main",
            "Bundle",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Enabled,
        );
        target.resolution_group_key = "bundle".to_owned();
        let neighbor = declaration(
            &context(),
            "main",
            "neighbor",
            AgentAssetCategory::Skill,
            AgentAssetDeclaredState::Enabled,
        );
        let mut one = declaration_with_key(
            &context(),
            "main",
            "bundle",
            "overlay-one",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Unknown,
        );
        one.role = AgentAssetDeclarationRole::StateOverlay;
        one.resolution_group_key = "bundle".to_owned();
        fixture_input(&mut one, "group_scope = true\nrequires_workspace = true");
        if invalid {
            fixture_input(&mut one, "invalid_overlay = true");
        }
        let mut two = declaration_with_key(
            &context(),
            "main",
            "BUNDLE",
            "overlay-two",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Enabled,
        );
        two.role = AgentAssetDeclarationRole::StateOverlay;
        two.resolution_group_key = "bundle".to_owned();
        group_scoped_overlay(&mut two);
        let mut foreign = declaration_with_key(
            &context(),
            "main",
            "elsewhere",
            "overlay-other",
            AgentAssetCategory::Extension,
            AgentAssetDeclaredState::Unknown,
        );
        foreign.role = AgentAssetDeclarationRole::StateOverlay;
        fixture_input(
            &mut foreign,
            "group_scope = true\nrequires_workspace = true",
        );
        let mut candidate = draft(
            &target,
            "extension:Bundle",
            AgentAssetResolutionRelation::Independent,
            &[&target, &one, &two],
        );
        candidate.declared_state = AgentAssetDeclaredState::Unknown;
        candidate.effective_state = AgentAssetState::Unknown;
        candidate.details = AgentAssetDetails::Extension {
            install_state: crate::models::AgentAssetInstallState::Installed,
            enabled: AgentAssetDeclaredState::Unknown,
            trusted: AgentTrustState::Unknown,
        };
        let mut evidence = vec![
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: one.declaration_id.clone(),
            },
            AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: two.declaration_id.clone(),
            },
        ];
        evidence.sort();
        let cause = if invalid {
            AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl
        } else {
            AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable
        };
        candidate.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: evidence.clone(),
            cause,
        };
        let neighbor_draft = draft(
            &neighbor,
            "skill:neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&neighbor],
        );
        let declarations = vec![target, neighbor, one, two, foreign];
        let baseline = project(
            &sources,
            &declarations,
            vec![candidate.clone(), neighbor_draft.clone()],
        );
        assert_rows(&baseline, &["Bundle", "neighbor"]);
        assert!(baseline
            .records
            .iter()
            .find(|record| record.native_id == "Bundle")
            .unwrap()
            .resolution
            .terminal
            .is_none());
        for attack in [
            "missing-peer",
            "wrong-group",
            "alias-overlap",
            "wrong-cause",
            "false-terminal",
        ] {
            let mut forged = candidate.clone();
            match attack {
                "missing-peer" => {
                    forged.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
                        evidence: vec![evidence[0].clone()],
                        cause,
                    }
                }
                "wrong-group" => {
                    let mut references = vec![
                        evidence[0].clone(),
                        AgentAssetStateEvidenceRefDraft::Declaration {
                            declaration_id: declarations[4].declaration_id.clone(),
                        },
                    ];
                    references.sort();
                    forged.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
                        evidence: references,
                        cause,
                    };
                }
                "alias-overlap" => {
                    let mut references = evidence.clone();
                    references.push(AgentAssetStateEvidenceRefDraft::Source {
                        source_key: "main".to_owned(),
                    });
                    references.sort();
                    forged.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
                        evidence: references,
                        cause,
                    };
                }
                "wrong-cause" => {
                    forged.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
                        evidence: evidence.clone(),
                        cause: if invalid {
                            AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable
                        } else {
                            AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl
                        },
                    }
                }
                "false-terminal" => {
                    forged.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
                    forged.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
                        terminal: AgentAssetResolutionTerminal::Unknown,
                        cause: AgentAssetTerminalCauseDraft::DeclaredUnknown,
                        evidence: Vec::new(),
                        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                    };
                }
                _ => unreachable!(),
            }
            assert_rejected_with_neighbor(&project(
                &sources,
                &declarations,
                vec![forged, neighbor_draft.clone()],
            ));
        }
    }
}

#[test]
fn context_unavailable_cannot_replace_a_known_overlay_result() {
    let sources = [source(
        "main",
        "main.json",
        &[AgentAssetCategory::Extension, AgentAssetCategory::Skill],
    )];
    let target = declaration(
        &context(),
        "main",
        "bundle",
        AgentAssetCategory::Extension,
        AgentAssetDeclaredState::Enabled,
    );
    let neighbor = declaration(
        &context(),
        "main",
        "neighbor",
        AgentAssetCategory::Skill,
        AgentAssetDeclaredState::Enabled,
    );
    let mut overlay = declaration_with_key(
        &context(),
        "main",
        "bundle",
        "state",
        AgentAssetCategory::Extension,
        AgentAssetDeclaredState::Enabled,
    );
    overlay.role = AgentAssetDeclarationRole::StateOverlay;
    fixture_input(
        &mut overlay,
        "group_scope = true\nrequires_workspace = true",
    );
    let mut candidate = draft(
        &target,
        "extension:bundle",
        AgentAssetResolutionRelation::Independent,
        &[&target, &overlay],
    );
    candidate.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
        declaration_ids: vec![overlay.declaration_id.clone()],
        scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup,
        outcome: AgentAssetDeclaredState::Enabled,
    };
    let neighbor_draft = draft(
        &neighbor,
        "skill:neighbor",
        AgentAssetResolutionRelation::Independent,
        &[&neighbor],
    );
    let declarations = vec![target, neighbor, overlay];
    assert_rows(
        &project(
            &sources,
            &declarations,
            vec![candidate.clone(), neighbor_draft.clone()],
        ),
        &["bundle", "neighbor"],
    );
    candidate.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: declarations[2].declaration_id.clone(),
        }],
        cause: AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable,
    };
    candidate.declared_state = AgentAssetDeclaredState::Unknown;
    candidate.effective_state = AgentAssetState::Unknown;
    if let AgentAssetDetails::Extension { enabled, .. } = &mut candidate.details {
        *enabled = AgentAssetDeclaredState::Unknown;
    }
    assert_rejected_with_neighbor(&project(
        &sources,
        &declarations,
        vec![candidate, neighbor_draft],
    ));
}

fn without_policy_authorities(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    alter_assessments(request, |target, result| {
        if target.exact_native_id == "server" {
            if let AgentAssetAssessmentResult::Assessed { state, .. } = result {
                state.control.as_mut().unwrap().authorities.clear();
            }
        }
    })
}

#[test]
fn typed_policy_preserves_declaration_and_source_aggregate_authority_cardinality() {
    for (label, policy_sources, aggregates) in [
        (
            "same-source-declarations",
            vec!["one", "one"],
            vec![false, false],
        ),
        (
            "cross-source-declarations",
            vec!["one", "two"],
            vec![false, false],
        ),
        (
            "cross-source-aggregates",
            vec!["one", "two"],
            vec![true, true],
        ),
        ("cross-source-mixed", vec!["one", "two"], vec![false, true]),
        (
            "whole-source-aggregate",
            vec!["one", "one"],
            vec![true, true],
        ),
        ("single-declaration", vec!["one"], vec![false]),
    ] {
        let sources = [
            source(
                "main",
                "main.json",
                &[AgentAssetCategory::Mcp, AgentAssetCategory::Skill],
            ),
            source("one", "one.json", &[AgentAssetCategory::Mcp]),
            source("two", "two.json", &[AgentAssetCategory::Mcp]),
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
        let mut policies = Vec::new();
        for (index, (&source_key, aggregate)) in policy_sources.iter().zip(&aggregates).enumerate()
        {
            let mut policy = policy_input(
                source_key,
                &format!("policy-{index}"),
                AgentAssetCategory::Mcp,
                "block_targets = ['server']",
            );
            if *aggregate {
                fixture_input(&mut policy, "source_aggregate = true");
            }
            policies.push(policy);
        }
        let owner = if label == "single-declaration" {
            Some(AgentAssetPolicyReferenceDraft::Declaration {
                declaration_id: policies[0].declaration_id.clone(),
            })
        } else if label == "whole-source-aggregate" {
            Some(AgentAssetPolicyReferenceDraft::Source {
                source_key: "one".to_owned(),
            })
        } else {
            None
        };
        let mut evidence = policies
            .iter()
            .map(|policy| AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: policy.declaration_id.clone(),
            })
            .collect::<Vec<_>>();
        evidence.sort();
        let mut candidate = draft(
            &target,
            "mcp:server",
            AgentAssetResolutionRelation::Independent,
            &[&target],
        );
        candidate.effective_state = AgentAssetState::Blocked;
        candidate.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
        candidate.resolution.control_source = owner.clone();
        if let AgentAssetDetails::Mcp {
            effective_availability,
            ..
        } = &mut candidate.details
        {
            *effective_availability = AgentAssetEffectiveAvailability::PolicyBlocked;
        }
        candidate.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: AgentAssetResolutionTerminal::PolicyBlocked,
            cause: AgentAssetTerminalCauseDraft::TypedPolicy,
            evidence: evidence.clone(),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        let neighbor_draft = draft(
            &neighbor,
            "skill:neighbor",
            AgentAssetResolutionRelation::Independent,
            &[&neighbor],
        );
        let mut declarations = vec![target, neighbor];
        declarations.extend(policies);
        let baseline = project(
            &sources,
            &declarations,
            vec![candidate.clone(), neighbor_draft.clone()],
        );
        assert_rows(&baseline, &["server", "neighbor"]);
        let server = baseline
            .records
            .iter()
            .find(|record| record.native_id == "server")
            .unwrap();
        assert_eq!(server.effective_state, AgentAssetState::Blocked, "{label}");
        assert_eq!(
            server.resolution.control_source.is_some(),
            owner.is_some(),
            "{label}"
        );
        let mut source_evidence = candidate.clone();
        if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
            &mut source_evidence.state_proof.effective
        {
            *evidence = policy_sources
                .iter()
                .map(|source| AgentAssetStateEvidenceRefDraft::Source {
                    source_key: (*source).to_owned(),
                })
                .collect();
            evidence.sort();
            evidence.dedup();
        }
        assert_rows(
            &project(
                &sources,
                &declarations,
                vec![source_evidence, neighbor_draft.clone()],
            ),
            &["server", "neighbor"],
        );

        let forged_owners = if owner.is_none() {
            vec![
                Some(AgentAssetPolicyReferenceDraft::Declaration {
                    declaration_id: declarations[2].declaration_id.clone(),
                }),
                Some(AgentAssetPolicyReferenceDraft::Source {
                    source_key: "one".to_owned(),
                }),
            ]
        } else {
            vec![None]
        };
        for forged_owner in forged_owners {
            let mut forged = candidate.clone();
            forged.resolution.control_source = forged_owner;
            assert_rejected_with_neighbor(&project(
                &sources,
                &declarations,
                vec![forged, neighbor_draft.clone()],
            ));
        }
        let mut missing = candidate.clone();
        if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
            &mut missing.state_proof.effective
        {
            evidence.pop();
        }
        assert_rejected_with_neighbor(&project(
            &sources,
            &declarations,
            vec![missing, neighbor_draft.clone()],
        ));
        let mut overlapping = candidate.clone();
        if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
            &mut overlapping.state_proof.effective
        {
            evidence.push(AgentAssetStateEvidenceRefDraft::Source {
                source_key: "one".to_owned(),
            });
            evidence.sort();
        }
        assert_rejected_with_neighbor(&project(
            &sources,
            &declarations,
            vec![overlapping, neighbor_draft.clone()],
        ));
    }
}

#[test]
fn assessor_typed_policy_without_authority_rejects_only_its_target() {
    let sources = [source(
        "main",
        "main.json",
        &[AgentAssetCategory::Mcp, AgentAssetCategory::Skill],
    )];
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
    let policy = policy_input(
        "main",
        "blocking-policy",
        AgentAssetCategory::Mcp,
        "block_targets = ['server']",
    );
    let mut candidate = draft(
        &target,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&target],
    );
    candidate.effective_state = AgentAssetState::Blocked;
    candidate.resolution.terminal = Some(AgentAssetResolutionTerminal::PolicyBlocked);
    candidate.resolution.control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
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
    if let AgentAssetDetails::Mcp {
        effective_availability,
        ..
    } = &mut candidate.details
    {
        *effective_availability = AgentAssetEffectiveAvailability::PolicyBlocked;
    }
    let neighbor_draft = draft(
        &neighbor,
        "skill:neighbor",
        AgentAssetResolutionRelation::Independent,
        &[&neighbor],
    );
    let declarations = vec![target, neighbor, policy];
    let drafts = vec![candidate, neighbor_draft];
    assert_rows(
        &project(&sources, &declarations, drafts.clone()),
        &["server", "neighbor"],
    );
    assert_rejected_with_neighbor(&project_using(
        &sources,
        &declarations,
        drafts.clone(),
        without_policy_authorities,
    ));
    let mut ownerless = drafts;
    ownerless[0].resolution.control_source = None;
    assert_rejected_with_neighbor(&project_using(
        &sources,
        &declarations,
        ownerless,
        without_policy_authorities,
    ));
}
