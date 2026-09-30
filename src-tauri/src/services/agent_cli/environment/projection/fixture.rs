//! Input-driven native fixture rules. No candidate draft enters this module.
//!
//! The default is greatest-precedence replacement. Tests that model native
//! merge, additive or unresolved structure set `fixture_mode` in a Definition's
//! TOML payload before constructing any draft. Relationships come from parser
//! declarations; only `parent_gate = true` gives a relationship state effects.
//! `source_aggregate` is a fixture-only whole-list policy, never a legacy native
//! admin-root payload. Its explicit target list determines applicability.

use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentIndex, AgentAssetAssessmentSubject, AgentAssetAssessmentTarget,
    AgentAssetControlAssessment, AgentAssetControlAuthority, AgentAssetDeclaredUnknownCauseDraft,
    AgentAssetEvidenceKind, AgentAssetEvidenceMember, AgentAssetNativeAssessment,
    AgentAssetNativePayload, AgentAssetProjectionAssessment, AgentAssetResolutionDraft,
    AgentAssetResolveRequest, AgentAssetResolvedDraftInput, AgentAssetStateOverlayScopeDraft,
    AgentAssetStateProofDraft, AgentMcpPolicyPayload,
};
use crate::services::agent_cli::environment::{
    adapter_support::{aggregate_relationships, declaration_matches_source},
    compose_effective_state, finalize_projected_draft, intrinsic_basis,
};

struct FixtureDecision<'a> {
    anchor: &'a ParsedAgentAsset,
    represented: Vec<&'a ParsedAgentAsset>,
    contributors: Vec<&'a ParsedAgentAsset>,
    state: AgentAssetNativeAssessment,
    projection: AgentAssetProjectionAssessment,
}

impl FixtureDecision<'_> {
    fn into_assessment(self) -> AgentAssetAssessmentResult {
        AgentAssetAssessmentResult::Assessed {
            state: self.state,
            projection: Box::new(self.projection),
        }
    }
}

pub(in crate::services::agent_cli) fn fixture_assessor(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let mut result = AgentAssetAssessmentIndex::default();
    for target in request.targets {
        let assessment = assess(target, &request)
            .map(FixtureDecision::into_assessment)
            .unwrap_or(AgentAssetAssessmentResult::Unsupported(
                AgentAssetAssessmentFailure::IncompleteInput,
            ));
        result.insert(target.clone(), assessment);
    }
    result
}

/// The generic inventory fixtures offer independent buckets in parser order.
/// Advanced structural/parent fixtures use the same decision through the
/// assessor, while deliberately constructing their candidate proof separately.
pub(in crate::services::agent_cli) fn fixture_draft(
    request: &AgentAssetResolveRequest<'_>,
    declaration: &ParsedAgentAsset,
) -> Option<AgentAssetProjectedDraft> {
    let target = AgentAssetAssessmentTarget {
        category: declaration.category,
        resolution_group_key: declaration.resolution_group_key.clone(),
        exact_native_id: declaration.native_id.clone(),
        subject: AgentAssetAssessmentSubject::Bucket,
    };
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let decision = assess(
        &target,
        &AgentAssetAssessmentRequest {
            context: request.context,
            targets: std::slice::from_ref(&target),
            declarations: request.declarations,
            sources: &sources,
        },
    )?;
    // These resolver fixtures have one Definition per bucket and do not model
    // material parent gates. Reject accidental use for a different structure.
    if decision.projection.relation != AgentAssetResolutionRelation::Independent {
        return None;
    }
    let (effective_state, details) = compose_effective_state(
        &decision.state.intrinsic,
        &decision.projection.effective,
        None,
    )?;
    finalize_projected_draft(AgentAssetResolvedDraftInput {
        anchor: decision.anchor,
        projection_key: format!("{}:{}", declaration.category.key(), declaration.native_id),
        represented: &decision.represented,
        contributors: &decision.contributors,
        declared_state: decision.state.declared_state,
        effective_state,
        trust_state: decision.state.intrinsic.trust_state,
        resolution: AgentAssetResolutionDraft {
            relation: decision.projection.relation,
            qualified_collision: false,
            terminal: None,
            winner: None,
            control_source: None,
        },
        details,
        relationships: decision.projection.relationships,
        state_proof: AgentAssetStateProofDraft {
            declared: decision.state.declared,
            effective: decision.projection.effective,
        },
    })
    .ok()
}

fn table(asset: &ParsedAgentAsset) -> Option<&toml::map::Map<String, toml::Value>> {
    match &asset.native_payload {
        AgentAssetNativePayload::TomlTable(table) => Some(table),
        _ => None,
    }
}

fn flag(asset: &ParsedAgentAsset, key: &str) -> bool {
    table(asset)
        .and_then(|table| table.get(key))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn mode(asset: &ParsedAgentAsset) -> &str {
    table(asset)
        .and_then(|table| table.get("fixture_mode"))
        .and_then(toml::Value::as_str)
        .unwrap_or("replace")
}

fn targets(asset: &ParsedAgentAsset, key: &str, target: &AgentAssetAssessmentTarget) -> bool {
    table(asset)
        .and_then(|table| table.get(key))
        .and_then(toml::Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .any(|value| value.as_str() == Some(&target.exact_native_id))
        })
}

fn evidence(assets: &[&ParsedAgentAsset]) -> Vec<AgentAssetStateEvidenceRefDraft> {
    let mut result = assets
        .iter()
        .map(|asset| AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: asset.declaration_id.clone(),
        })
        .collect::<Vec<_>>();
    result.sort();
    result
}

fn members(
    assets: &[&ParsedAgentAsset],
    kind: AgentAssetEvidenceKind,
) -> Vec<AgentAssetEvidenceMember> {
    assets
        .iter()
        .map(|asset| AgentAssetEvidenceMember {
            declaration_id: asset.declaration_id.clone(),
            expected_role: asset.role,
            kind,
        })
        .collect()
}

fn overlay_scope(asset: &ParsedAgentAsset) -> AgentAssetStateOverlayScopeDraft {
    if flag(asset, "group_scope") {
        AgentAssetStateOverlayScopeDraft::ResolutionGroup
    } else {
        AgentAssetStateOverlayScopeDraft::ExactNativeId
    }
}

fn set_declared(details: &mut AgentAssetDetails, state: AgentAssetDeclaredState) {
    match details {
        AgentAssetDetails::Skill { enabled, .. }
        | AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. }
        | AgentAssetDetails::Hook { enabled, .. } => *enabled = state,
        AgentAssetDetails::Mcp { declared_state, .. } => *declared_state = state,
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } => {
            *mode = match state {
                AgentAssetDeclaredState::Enabled => AgentStatusUiMode::BuiltIn,
                AgentAssetDeclaredState::Disabled => AgentStatusUiMode::Disabled,
                _ => AgentStatusUiMode::Unknown,
            };
            *command_present = false;
        }
    }
}

fn valid_declaration(asset: &ParsedAgentAsset, request: &AgentAssetAssessmentRequest<'_>) -> bool {
    let mut sources = request
        .sources
        .iter()
        .filter(|source| source.native_source_key == asset.source_key);
    let Some(source) = sources.next() else {
        return false;
    };
    sources.next().is_none()
        && declaration_matches_source(request.context, asset, source)
        && request
            .declarations
            .iter()
            .filter(|other| other.declaration_id == asset.declaration_id)
            .count()
            == 1
}

fn assess<'a>(
    target: &AgentAssetAssessmentTarget,
    request: &AgentAssetAssessmentRequest<'a>,
) -> Option<FixtureDecision<'a>> {
    let declarations = request.declarations;
    let mut definitions = declarations
        .iter()
        .filter(|asset| {
            asset.category == target.category
                && asset.resolution_group_key == target.resolution_group_key
                && asset.native_id == target.exact_native_id
                && asset.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    let suppressed = !definitions
        .iter()
        .any(|asset| asset.participation == AgentAssetResolutionParticipation::Participates);
    if !suppressed {
        definitions
            .retain(|asset| asset.participation == AgentAssetResolutionParticipation::Participates);
    }
    definitions.sort_by(|a, b| {
        b.logical_origin
            .precedence
            .cmp(&a.logical_origin.precedence)
            .then_with(|| a.source_key.cmp(&b.source_key))
            .then_with(|| a.declaration_id.cmp(&b.declaration_id))
    });
    if definitions
        .iter()
        .any(|asset| !valid_declaration(asset, request))
    {
        return None;
    }
    let bucket_anchor = *definitions.first()?;
    let peers = definitions
        .iter()
        .copied()
        .filter(|asset| asset.logical_origin.precedence == bucket_anchor.logical_origin.precedence)
        .collect::<Vec<_>>();
    let is_bucket = target.subject == AgentAssetAssessmentSubject::Bucket;
    let anchor = match &target.subject {
        AgentAssetAssessmentSubject::Bucket => bucket_anchor,
        AgentAssetAssessmentSubject::Definition(id) => *definitions
            .iter()
            .find(|asset| asset.declaration_id == *id)?,
    };
    let relation = if suppressed {
        if !is_bucket {
            return None;
        }
        AgentAssetResolutionRelation::Unknown
    } else {
        match (mode(bucket_anchor), &target.subject) {
            ("additive", AgentAssetAssessmentSubject::Definition(_)) => {
                AgentAssetResolutionRelation::Additive
            }
            ("merge", AgentAssetAssessmentSubject::Bucket) => AgentAssetResolutionRelation::Merged,
            ("unresolved", AgentAssetAssessmentSubject::Bucket) => {
                AgentAssetResolutionRelation::Unknown
            }
            ("replace", AgentAssetAssessmentSubject::Bucket) if definitions.len() == 1 => {
                AgentAssetResolutionRelation::Independent
            }
            ("replace", AgentAssetAssessmentSubject::Bucket) if peers.len() == 1 => {
                AgentAssetResolutionRelation::ReplaceWinner
            }
            ("replace", AgentAssetAssessmentSubject::Bucket) => {
                AgentAssetResolutionRelation::Unknown
            }
            ("replace", AgentAssetAssessmentSubject::Definition(_))
                if peers.len() == 1 && anchor.declaration_id != bucket_anchor.declaration_id =>
            {
                AgentAssetResolutionRelation::Replaced
            }
            _ => return None,
        }
    };
    let structural_conflict =
        is_bucket && !suppressed && mode(bucket_anchor) == "replace" && peers.len() > 1;
    let mut details = anchor.details.clone();
    let mut state = anchor.declared_state;
    let (mut proof, mut declared_members) = if structural_conflict {
        state = AgentAssetDeclaredState::Unknown;
        set_declared(&mut details, state);
        (
            AgentAssetDeclaredStateProofDraft::Unknown {
                evidence: evidence(&peers),
                cause: AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
            },
            members(&peers, AgentAssetEvidenceKind::Definition),
        )
    } else if suppressed || state == AgentAssetDeclaredState::Unknown {
        let relevant = if suppressed {
            definitions.clone()
        } else {
            vec![anchor]
        };
        (
            AgentAssetDeclaredStateProofDraft::Unknown {
                evidence: evidence(&relevant),
                cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            },
            members(&relevant, AgentAssetEvidenceKind::Definition),
        )
    } else {
        (
            AgentAssetDeclaredStateProofDraft::Definition {
                declaration_ids: vec![anchor.declaration_id.clone()],
                selected_id: anchor.declaration_id.clone(),
            },
            members(&[anchor], AgentAssetEvidenceKind::Definition),
        )
    };
    if is_bucket && !suppressed && !structural_conflict {
        let mut overlays = declarations
            .iter()
            .filter(|asset| {
                asset.category == target.category
                    && asset.role == AgentAssetDeclarationRole::StateOverlay
                    && asset.participation == AgentAssetResolutionParticipation::Participates
                    && asset.resolution_group_key == target.resolution_group_key
                    && (asset.native_id == target.exact_native_id
                        || overlay_scope(asset)
                            == AgentAssetStateOverlayScopeDraft::ResolutionGroup)
            })
            .collect::<Vec<_>>();
        if let Some(precedence) = overlays
            .iter()
            .map(|asset| asset.logical_origin.precedence)
            .max()
        {
            overlays.retain(|asset| asset.logical_origin.precedence == precedence);
            overlays.sort_by_key(|asset| &asset.declaration_id);
            if overlays
                .iter()
                .any(|asset| !valid_declaration(asset, request))
            {
                return None;
            }
            let scope = overlay_scope(overlays[0]);
            state = overlays[0].declared_state;
            let invalid = overlays.iter().any(|asset| {
                flag(asset, "invalid_overlay")
                    || matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::InvalidControl(_)
                    )
            });
            let unavailable = request.context.workspace_id.is_none()
                && overlays.iter().any(|asset| {
                    flag(asset, "requires_workspace")
                        && asset.declared_state == AgentAssetDeclaredState::Unknown
                });
            let conflict = overlays.iter().any(|asset| asset.declared_state != state);
            if invalid || unavailable || conflict {
                state = AgentAssetDeclaredState::Unknown;
                proof = AgentAssetDeclaredStateProofDraft::Unknown {
                    evidence: evidence(&overlays),
                    cause: if invalid {
                        AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl
                    } else if unavailable {
                        AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable
                    } else {
                        AgentAssetDeclaredUnknownCauseDraft::OverlayConflict
                    },
                };
            } else {
                proof = AgentAssetDeclaredStateProofDraft::Overlay {
                    declaration_ids: overlays
                        .iter()
                        .map(|asset| asset.declaration_id.clone())
                        .collect(),
                    scope,
                    outcome: state,
                };
            }
            declared_members = overlays
                .iter()
                .map(|asset| AgentAssetEvidenceMember {
                    declaration_id: asset.declaration_id.clone(),
                    expected_role: asset.role,
                    kind: if flag(asset, "invalid_overlay")
                        || matches!(
                            asset.native_payload,
                            AgentAssetNativePayload::InvalidControl(_)
                        ) {
                        AgentAssetEvidenceKind::InvalidStateControl {
                            scope: overlay_scope(asset),
                        }
                    } else {
                        AgentAssetEvidenceKind::Overlay {
                            scope: overlay_scope(asset),
                        }
                    },
                })
                .collect();
            set_declared(&mut details, state);
        }
        let policies = declarations
            .iter()
            .filter(|asset| {
                asset.category == target.category
                    && asset.role == AgentAssetDeclarationRole::PolicyOverlay
                    && asset.participation == AgentAssetResolutionParticipation::Participates
                    && flag(asset, "declared_disable")
                    && targets(asset, "policy_targets", target)
            })
            .collect::<Vec<_>>();
        if !policies.is_empty() {
            if policies
                .iter()
                .any(|asset| !valid_declaration(asset, request))
            {
                return None;
            }
            state = AgentAssetDeclaredState::Disabled;
            let mut ids = policies
                .iter()
                .map(|asset| asset.declaration_id.clone())
                .collect::<Vec<_>>();
            ids.sort();
            proof = AgentAssetDeclaredStateProofDraft::Policy {
                declaration_ids: ids,
                outcome: state,
            };
            declared_members = members(&policies, AgentAssetEvidenceKind::Policy);
            set_declared(&mut details, state);
        }
    }
    let control = (is_bucket && !suppressed)
        .then(|| controls(target, declarations))
        .flatten();
    if control.as_ref().is_some_and(|control| {
        control.members.iter().any(|member| {
            declarations
                .iter()
                .find(|asset| asset.declaration_id == member.declaration_id)
                .is_none_or(|asset| !valid_declaration(asset, request))
        })
    }) {
        return None;
    }
    let relationship_assets = if is_bucket {
        definitions.clone()
    } else {
        vec![anchor]
    };
    let relationships = aggregate_relationships(&relationship_assets).ok()?;
    let input = if flag(anchor, "parent_gate") {
        AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: relationships.provided_by.clone()?,
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        }
    } else {
        AgentAssetEffectiveStateProofDraft::Intrinsic
    };
    let effective = if relation == AgentAssetResolutionRelation::Replaced {
        AgentAssetEffectiveStateProofDraft::Shadowed {
            winner: AgentAssetNativeRef {
                category: bucket_anchor.category,
                native_id: bucket_anchor.native_id.clone(),
                qualifier: Some(format!(
                    "{}:{}",
                    bucket_anchor.category.key(),
                    bucket_anchor.native_id
                )),
            },
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        }
    } else if suppressed {
        terminal(
            AgentAssetTerminalCauseDraft::TrustSuppressed,
            Vec::new(),
            input,
        )
    } else if let Some(control) = &control {
        let mut evidence = control
            .members
            .iter()
            .map(|member| AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: member.declaration_id.clone(),
            })
            .collect::<Vec<_>>();
        evidence.sort();
        terminal(control.cause, evidence, input)
    } else if structural_conflict {
        terminal(
            AgentAssetTerminalCauseDraft::StructuralUnknown,
            Vec::new(),
            input,
        )
    } else {
        input
    };
    let mut represented = if is_bucket { definitions } else { vec![anchor] };
    if is_bucket && !suppressed {
        represented.extend(declarations.iter().filter(|asset| {
            asset.role == AgentAssetDeclarationRole::StateOverlay
                && asset.category == target.category
                && asset.resolution_group_key == target.resolution_group_key
                && asset.participation == AgentAssetResolutionParticipation::Participates
                && (asset.native_id == target.exact_native_id
                    || overlay_scope(asset) == AgentAssetStateOverlayScopeDraft::ResolutionGroup)
        }));
    }
    let contributors = if suppressed {
        Vec::new()
    } else {
        represented.clone()
    };
    Some(FixtureDecision {
        anchor,
        represented,
        contributors,
        state: AgentAssetNativeAssessment {
            declared_state: state,
            declared: proof,
            declared_members,
            intrinsic: intrinsic_basis(details, anchor.trust_state),
            control,
        },
        projection: AgentAssetProjectionAssessment {
            anchor_declaration_id: anchor.declaration_id.clone(),
            relation,
            effective,
            relationships,
        },
    })
}

fn terminal(
    cause: AgentAssetTerminalCauseDraft,
    evidence: Vec<AgentAssetStateEvidenceRefDraft>,
    input: AgentAssetEffectiveStateProofDraft,
) -> AgentAssetEffectiveStateProofDraft {
    AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: if cause == AgentAssetTerminalCauseDraft::TypedPolicy {
            crate::models::AgentAssetResolutionTerminal::PolicyBlocked
        } else {
            crate::models::AgentAssetResolutionTerminal::Unknown
        },
        cause,
        evidence,
        input: Box::new(input),
    }
}

fn controls(
    target: &AgentAssetAssessmentTarget,
    declarations: &[ParsedAgentAsset],
) -> Option<AgentAssetControlAssessment> {
    let applicable = declarations
        .iter()
        .filter(|asset| {
            asset.category == target.category
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .collect::<Vec<_>>();
    let invalid = applicable
        .iter()
        .copied()
        .filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::InvalidControl(_)
            )
        })
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Some(AgentAssetControlAssessment {
            cause: AgentAssetTerminalCauseDraft::InvalidControl,
            members: members(&invalid, AgentAssetEvidenceKind::InvalidControl),
            authorities: invalid
                .iter()
                .map(|asset| AgentAssetControlAuthority::Declaration(asset.declaration_id.clone()))
                .collect(),
        });
    }
    let policies = applicable
        .into_iter()
        .filter(|asset| {
            asset.role == AgentAssetDeclarationRole::PolicyOverlay
                && match &asset.native_payload {
                    AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::Excluded(ids)) => {
                        ids.contains(&target.exact_native_id)
                    }
                    AgentAssetNativePayload::TomlTable(_) => {
                        targets(asset, "block_targets", target)
                    }
                    _ => false,
                }
        })
        .collect::<Vec<_>>();
    (!policies.is_empty()).then(|| AgentAssetControlAssessment {
        cause: AgentAssetTerminalCauseDraft::TypedPolicy,
        members: members(&policies, AgentAssetEvidenceKind::Policy),
        authorities: policies
            .iter()
            .map(|asset| {
                if flag(asset, "source_aggregate") {
                    AgentAssetControlAuthority::SourceAggregate(asset.source_key.clone())
                } else {
                    AgentAssetControlAuthority::Declaration(asset.declaration_id.clone())
                }
            })
            .collect(),
    })
}
