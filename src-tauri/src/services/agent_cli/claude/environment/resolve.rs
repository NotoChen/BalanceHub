//! A single pure native decision feeds both construction and independent assessment.
use super::{control::*, SETTINGS_AUTHORITIES};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetNativeRef, AgentAssetResolutionParticipation,
    AgentAssetResolutionRelation, AgentAssetResolutionTerminal, AgentAssetScope,
    AgentConfigurationContext, AgentMcpApprovalState, AgentMcpTransport, AgentStatusUiMode,
    AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentFailure, AgentAssetAssessmentIndex, AgentAssetAssessmentRequest,
    AgentAssetAssessmentResult, AgentAssetAssessmentSubject, AgentAssetAssessmentTarget,
    AgentAssetControlAssessment, AgentAssetControlAuthority, AgentAssetDeclaredStateProofDraft,
    AgentAssetDeclaredUnknownCauseDraft, AgentAssetEffectiveStateProofDraft,
    AgentAssetEvidenceKind, AgentAssetEvidenceMember, AgentAssetLogicalOrigin,
    AgentAssetNativeAssessment, AgentAssetNativePayload, AgentAssetPolicyReferenceDraft,
    AgentAssetProjectedDraft, AgentAssetProjectionAssessment, AgentAssetResolutionDraft,
    AgentAssetResolveRequest, AgentAssetResolvedDraftInput, AgentAssetSourceSpec,
    AgentAssetStateEvidenceRefDraft, AgentAssetStateOverlayScopeDraft, AgentAssetStateProofDraft,
    AgentAssetTerminalCauseDraft, AgentResolveOutput, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    compose_effective_state, finalize_projected_draft, intrinsic_basis, qualified_projection_key,
    select_draft_structure,
};
use std::collections::{BTreeMap, BTreeSet};

mod controls;
mod index;
mod mcp;
mod mcp_equivalence;
mod plugin;
#[cfg(test)]
mod tests;
use index::NativeIndex;
pub(in crate::services::agent_cli::claude) use plugin::definition_suppressions;

type BucketKey = (AgentAssetCategory, String, String);
type NativeResult<T> = Result<T, AgentAssetAssessmentFailure>;

struct NativeDecision<'a> {
    anchor: &'a ParsedAgentAsset,
    key: String,
    represented: Vec<&'a ParsedAgentAsset>,
    contributors: Vec<&'a ParsedAgentAsset>,
    resolution: AgentAssetResolutionDraft,
    assessment: AgentAssetNativeAssessment,
    effective: AgentAssetEffectiveStateProofDraft,
    parent_state: Option<crate::models::AgentAssetState>,
}
impl NativeDecision<'_> {
    fn into_assessment(self) -> NativeResult<AgentAssetAssessmentResult> {
        let structure = select_draft_structure(self.anchor, &self.represented, &self.contributors)
            .map_err(|_| AgentAssetAssessmentFailure::InvalidNativeInput)?;
        Ok(AgentAssetAssessmentResult::Assessed {
            state: self.assessment,
            projection: Box::new(AgentAssetProjectionAssessment {
                anchor_declaration_id: structure.anchor.declaration_id.clone(),
                relation: self.resolution.relation,
                effective: self.effective,
                relationships: structure.relationships,
            }),
        })
    }
    fn finalize(self) -> Result<AgentAssetProjectedDraft, AgentAssetDiagnostic> {
        let invalid = || AgentAssetDiagnostic::InvalidResolution {
            projection_key: self.key.clone(),
            resolution: self.resolution.relation,
        };
        let structure = select_draft_structure(self.anchor, &self.represented, &self.contributors)
            .map_err(|_| invalid())?;
        let (effective_state, details) = compose_effective_state(
            &self.assessment.intrinsic,
            &self.effective,
            self.parent_state,
        )
        .ok_or_else(invalid)?;
        finalize_projected_draft(AgentAssetResolvedDraftInput {
            anchor: structure.anchor,
            projection_key: self.key.clone(),
            represented: &structure.represented,
            contributors: &structure.contributors,
            declared_state: self.assessment.declared_state,
            effective_state,
            trust_state: self.assessment.intrinsic.trust_state,
            resolution: self.resolution.clone(),
            details,
            relationships: structure.relationships,
            state_proof: AgentAssetStateProofDraft {
                declared: self.assessment.declared,
                effective: self.effective,
            },
        })
        .map_err(|_| invalid())
    }
}

fn order(left: &ParsedAgentAsset, right: &ParsedAgentAsset) -> std::cmp::Ordering {
    if let Some(order) = plugin::native_order(left, right) {
        if order != std::cmp::Ordering::Equal {
            return order;
        }
    }
    right
        .logical_origin
        .precedence
        .cmp(&left.logical_origin.precedence)
        .then_with(|| left.source_key.cmp(&right.source_key))
        .then_with(|| left.declaration_id.cmp(&right.declaration_id))
}
fn definitions<'a>(bucket: &[&'a ParsedAgentAsset]) -> Vec<&'a ParsedAgentAsset> {
    bucket
        .iter()
        .copied()
        .filter(|asset| {
            asset.role == AgentAssetDeclarationRole::Definition
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .collect()
}
fn tied(definitions: &[&ParsedAgentAsset]) -> bool {
    definitions.len() > 1
        && definitions[0].logical_origin.precedence == definitions[1].logical_origin.precedence
        && plugin::native_order(definitions[0], definitions[1])
            .is_none_or(|order| order == std::cmp::Ordering::Equal)
}

impl<'a> NativeIndex<'a> {
    fn targets(&self) -> Vec<AgentAssetAssessmentTarget> {
        let mut targets = Vec::new();
        for ((category, group, native_id), bucket) in &self.buckets {
            let definitions = definitions(bucket);
            if definitions.is_empty()
                && !(*category == AgentAssetCategory::Mcp
                    && bucket
                        .iter()
                        .any(|asset| asset.role == AgentAssetDeclarationRole::Definition))
            {
                continue;
            }
            let target = AgentAssetAssessmentTarget {
                category: *category,
                resolution_group_key: group.clone(),
                exact_native_id: native_id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            };
            if *category == AgentAssetCategory::Hook && definitions.len() > 1 {
                for asset in &definitions {
                    targets.push(AgentAssetAssessmentTarget {
                        subject: AgentAssetAssessmentSubject::Definition(
                            asset.declaration_id.clone(),
                        ),
                        ..target.clone()
                    });
                }
            } else {
                targets.push(target.clone());
                if !tied(&definitions) {
                    for loser in definitions.iter().skip(1) {
                        targets.push(AgentAssetAssessmentTarget {
                            subject: AgentAssetAssessmentSubject::Definition(
                                loser.declaration_id.clone(),
                            ),
                            ..target.clone()
                        });
                    }
                }
            }
        }
        targets
    }

    fn decide(&self, target: &AgentAssetAssessmentTarget) -> NativeResult<NativeDecision<'a>> {
        let bucket = self
            .buckets
            .get(&(
                target.category,
                target.resolution_group_key.clone(),
                target.exact_native_id.clone(),
            ))
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        let definitions = definitions(bucket);
        if definitions.is_empty() {
            return trust_suppressed(bucket, target);
        }
        let first = definitions[0];
        let is_additive = target.category == AgentAssetCategory::Hook && definitions.len() > 1;
        let structural = !is_additive && tied(&definitions);
        let winner_key = format!("{}:{}", first.category.key(), first.native_id);
        let winner_ref = AgentAssetNativeRef {
            category: first.category,
            native_id: first.native_id.clone(),
            qualifier: Some(winner_key.clone()),
        };
        let (anchor, relation, key, represented, contributors) = match &target.subject {
            AgentAssetAssessmentSubject::Bucket if !is_additive => (
                first,
                if structural {
                    AgentAssetResolutionRelation::Unknown
                } else if definitions.len() > 1 {
                    AgentAssetResolutionRelation::ReplaceWinner
                } else {
                    AgentAssetResolutionRelation::Independent
                },
                if structural {
                    qualified_projection_key(first)
                } else {
                    winner_key
                },
                bucket.clone(),
                participating(bucket),
            ),
            AgentAssetAssessmentSubject::Definition(id) if is_additive => {
                let anchor = definitions
                    .iter()
                    .copied()
                    .find(|asset| asset.declaration_id == *id)
                    .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
                let contributors = bucket
                    .iter()
                    .copied()
                    .filter(|asset| {
                        asset.participation == AgentAssetResolutionParticipation::Participates
                            && (asset.role != AgentAssetDeclarationRole::Definition
                                || asset.declaration_id == *id)
                    })
                    .collect();
                (
                    anchor,
                    AgentAssetResolutionRelation::Additive,
                    qualified_projection_key(anchor),
                    bucket.clone(),
                    contributors,
                )
            }
            AgentAssetAssessmentSubject::Definition(id) if !structural => {
                let anchor = definitions
                    .iter()
                    .copied()
                    .skip(1)
                    .find(|asset| asset.declaration_id == *id)
                    .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
                (
                    anchor,
                    AgentAssetResolutionRelation::Replaced,
                    qualified_projection_key(anchor),
                    vec![anchor],
                    vec![anchor],
                )
            }
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        let mut assessment = if structural {
            let peers = definitions
                .iter()
                .copied()
                .filter(|asset| asset.logical_origin.precedence == first.logical_origin.precedence)
                .collect::<Vec<_>>();
            unknown_assessment(
                anchor,
                &peers,
                AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
                peers
                    .iter()
                    .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
                    .collect(),
            )
        } else {
            definition_assessment(anchor)
        };
        if target.category == AgentAssetCategory::Plugin {
            self.plugin_installation(anchor, &mut assessment)?;
        }
        let shadowed = relation == AgentAssetResolutionRelation::Replaced;
        let terminal = if shadowed {
            None
        } else {
            match target.category {
                AgentAssetCategory::Mcp => {
                    assessment = mcp::declared(self, anchor, structural, assessment)?;
                    assessment = mcp_equivalence::assess(self, anchor, assessment)?;
                    mcp::terminal(self, anchor, &assessment)?
                }
                AgentAssetCategory::Plugin => {
                    assessment = controls::plugin_declared(self, anchor, structural, assessment)?;
                    controls::plugin_terminal(self)?
                }
                AgentAssetCategory::Hook | AgentAssetCategory::StatusUi => {
                    controls::hook_terminal(self, anchor)?
                }
                AgentAssetCategory::Skill => {
                    if !structural
                        && self
                            .ambiguous_plugin_skill_ids
                            .contains(&anchor.declaration_id)
                    {
                        assessment = unknown_assessment(
                            anchor,
                            &[anchor],
                            AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                            vec![member(anchor, AgentAssetEvidenceKind::Definition)],
                        );
                    }
                    None
                }
                AgentAssetCategory::Extension => {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput)
                }
            }
            .or_else(|| {
                structural.then(|| {
                    NativeTerminal::unknown(AgentAssetTerminalCauseDraft::StructuralUnknown)
                })
            })
        };
        let effective = if shadowed {
            AgentAssetEffectiveStateProofDraft::Shadowed {
                winner: winner_ref.clone(),
                input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
            }
        } else if let Some(terminal) = &terminal {
            terminal.proof()
        } else {
            AgentAssetEffectiveStateProofDraft::Intrinsic
        };
        assessment.control = terminal
            .as_ref()
            .and_then(|terminal| terminal.control.clone());
        let collision = self.buckets.iter().any(|((category, group, id), values)| {
            *category == target.category
                && *group == target.resolution_group_key
                && *id != target.exact_native_id
                && values.iter().any(|asset| {
                    asset.role == AgentAssetDeclarationRole::Definition
                        && asset.participation == AgentAssetResolutionParticipation::Participates
                })
        });
        let mut decision = NativeDecision {
            anchor,
            key,
            represented,
            contributors,
            resolution: AgentAssetResolutionDraft {
                relation,
                qualified_collision: collision,
                terminal: terminal.as_ref().map(|terminal| terminal.kind),
                winner: matches!(
                    relation,
                    AgentAssetResolutionRelation::ReplaceWinner
                        | AgentAssetResolutionRelation::Replaced
                )
                .then_some(winner_ref),
                control_source: terminal.as_ref().and_then(NativeTerminal::owner),
            },
            assessment,
            effective,
            parent_state: None,
        };
        self.apply_plugin_parent(&mut decision)?;
        Ok(decision)
    }
}

fn participating<'a>(bucket: &[&'a ParsedAgentAsset]) -> Vec<&'a ParsedAgentAsset> {
    bucket
        .iter()
        .copied()
        .filter(|asset| asset.participation == AgentAssetResolutionParticipation::Participates)
        .collect()
}
fn member(asset: &ParsedAgentAsset, kind: AgentAssetEvidenceKind) -> AgentAssetEvidenceMember {
    AgentAssetEvidenceMember {
        declaration_id: asset.declaration_id.clone(),
        expected_role: asset.role,
        kind,
    }
}
fn ids(assets: &[&ParsedAgentAsset]) -> Vec<String> {
    let mut values = assets
        .iter()
        .map(|asset| asset.declaration_id.clone())
        .collect::<Vec<_>>();
    values.sort();
    values
}
fn refs(assets: &[&ParsedAgentAsset]) -> Vec<AgentAssetStateEvidenceRefDraft> {
    ids(assets)
        .into_iter()
        .map(|declaration_id| AgentAssetStateEvidenceRefDraft::Declaration { declaration_id })
        .collect()
}
fn set_declared(details: &mut AgentAssetDetails, state: AgentAssetDeclaredState) {
    match details {
        AgentAssetDetails::Mcp { declared_state, .. } => *declared_state = state,
        AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. }
        | AgentAssetDetails::Skill { enabled, .. }
        | AgentAssetDetails::Hook { enabled, .. } => *enabled = state,
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } if state == AgentAssetDeclaredState::Unknown => {
            *mode = AgentStatusUiMode::Unknown;
            *command_present = false;
        }
        AgentAssetDetails::StatusUi { .. } => {}
    }
}
fn unknown_assessment(
    anchor: &ParsedAgentAsset,
    evidence: &[&ParsedAgentAsset],
    cause: AgentAssetDeclaredUnknownCauseDraft,
    members: Vec<AgentAssetEvidenceMember>,
) -> AgentAssetNativeAssessment {
    let mut details = anchor.details.clone();
    set_declared(&mut details, AgentAssetDeclaredState::Unknown);
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Unknown,
        declared: AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: refs(evidence),
            cause,
        },
        declared_members: members,
        intrinsic: intrinsic_basis(details, anchor.trust_state),
        control: None,
    }
}
fn definition_assessment(anchor: &ParsedAgentAsset) -> AgentAssetNativeAssessment {
    let members = vec![member(anchor, AgentAssetEvidenceKind::Definition)];
    if anchor.declared_state == AgentAssetDeclaredState::Unknown {
        return unknown_assessment(
            anchor,
            &[anchor],
            AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            members,
        );
    }
    AgentAssetNativeAssessment {
        declared_state: anchor.declared_state,
        declared: AgentAssetDeclaredStateProofDraft::Definition {
            declaration_ids: vec![anchor.declaration_id.clone()],
            selected_id: anchor.declaration_id.clone(),
        },
        declared_members: members,
        intrinsic: intrinsic_basis(anchor.details.clone(), anchor.trust_state),
        control: None,
    }
}

fn trust_suppressed<'a>(
    bucket: &[&'a ParsedAgentAsset],
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    if target.category != AgentAssetCategory::Mcp
        || target.subject != AgentAssetAssessmentSubject::Bucket
    {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let definitions = bucket
        .iter()
        .copied()
        .filter(|asset| asset.role == AgentAssetDeclarationRole::Definition)
        .collect::<Vec<_>>();
    let anchor = definitions
        .iter()
        .copied()
        .min_by(|left, right| {
            left.source_key
                .cmp(&right.source_key)
                .then_with(|| left.declaration_id.cmp(&right.declaration_id))
        })
        .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
    if !matches!(
        anchor.trust_state,
        AgentTrustState::Required | AgentTrustState::Untrusted
    ) || !definitions.iter().all(|asset| {
        asset.trust_state == anchor.trust_state
            && matches!(
                asset.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }) {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let details = AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Unknown,
        approval_state: AgentMcpApprovalState::Unknown,
        effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
    };
    Ok(NativeDecision {
        anchor,
        key: format!("mcp:{}", anchor.native_id),
        represented: bucket.to_vec(),
        contributors: Vec::new(),
        resolution: AgentAssetResolutionDraft {
            relation: AgentAssetResolutionRelation::Unknown,
            qualified_collision: false,
            terminal: Some(AgentAssetResolutionTerminal::Unknown),
            winner: None,
            control_source: None,
        },
        assessment: AgentAssetNativeAssessment {
            declared_state: AgentAssetDeclaredState::Unknown,
            declared: AgentAssetDeclaredStateProofDraft::Unknown {
                evidence: refs(&definitions),
                cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            },
            declared_members: definitions
                .iter()
                .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
                .collect(),
            intrinsic: intrinsic_basis(details, anchor.trust_state),
            control: None,
        },
        effective: NativeTerminal::unknown(AgentAssetTerminalCauseDraft::TrustSuppressed).proof(),
        parent_state: None,
    })
}

struct NativeTerminal {
    kind: AgentAssetResolutionTerminal,
    cause: AgentAssetTerminalCauseDraft,
    control: Option<AgentAssetControlAssessment>,
}
impl NativeTerminal {
    fn unknown(cause: AgentAssetTerminalCauseDraft) -> Self {
        Self {
            kind: AgentAssetResolutionTerminal::Unknown,
            cause,
            control: None,
        }
    }
    fn controlled(control: AgentAssetControlAssessment) -> Self {
        Self {
            kind: if control.cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                AgentAssetResolutionTerminal::PolicyBlocked
            } else {
                AgentAssetResolutionTerminal::Unknown
            },
            cause: control.cause,
            control: Some(control),
        }
    }
    fn owner(&self) -> Option<AgentAssetPolicyReferenceDraft> {
        let authorities = &self.control.as_ref()?.authorities;
        if authorities.len() != 1 {
            return None;
        }
        authorities.first().map(|authority| match authority {
            AgentAssetControlAuthority::Declaration(declaration_id) => {
                AgentAssetPolicyReferenceDraft::Declaration {
                    declaration_id: declaration_id.clone(),
                }
            }
            AgentAssetControlAuthority::SourceAggregate(source_key) => {
                AgentAssetPolicyReferenceDraft::Source {
                    source_key: source_key.clone(),
                }
            }
        })
    }
    fn proof(&self) -> AgentAssetEffectiveStateProofDraft {
        let mut evidence = self
            .control
            .as_ref()
            .map(|control| {
                control
                    .members
                    .iter()
                    .map(|member| AgentAssetStateEvidenceRefDraft::Declaration {
                        declaration_id: member.declaration_id.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        evidence.sort();
        AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: self.kind,
            cause: self.cause,
            evidence,
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        }
    }
}

pub(in crate::services::agent_cli::claude) fn resolve_assets(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let index = NativeIndex::new(request.context, request.declarations, &sources);
    if !index.ambiguous_plugin_skill_ids.is_empty() || index.conflicting_plugin_skill_parents() {
        output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
            agent_kind: super::super::AGENT_KIND,
            category: AgentAssetCategory::Skill,
            reason: crate::models::AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved,
        });
    }
    for target in index.targets() {
        match index.decide(&target) {
            Ok(decision) => {
                if decision.anchor.category == AgentAssetCategory::Mcp
                    && matches!(
                        decision.anchor.native_payload,
                        AgentAssetNativePayload::ClaudePlugin(_)
                    )
                    && matches!(
                        decision.assessment.declared,
                        AgentAssetDeclaredStateProofDraft::Unknown {
                            cause: AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                            ..
                        }
                    )
                {
                    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                        agent_kind: super::super::AGENT_KIND,
                        category: AgentAssetCategory::Mcp,
                        reason: crate::models::AgentAssetDiscoveryIncompleteReason::NativeEquivalenceUnobserved,
                    });
                }
                match decision.finalize() {
                    Ok(draft) => {
                        if output.emit_draft(draft).is_break() {
                            return;
                        }
                    }
                    Err(diagnostic) => {
                        output.emit_diagnostic(diagnostic);
                    }
                }
            }
            Err(failure) => {
                output.emit_diagnostic(AgentAssetDiagnostic::InvalidResolution {
                    projection_key: format!(
                        "{}:{}:assessment:{failure:?}",
                        target.category.key(),
                        target.exact_native_id
                    ),
                    resolution: AgentAssetResolutionRelation::Unknown,
                });
            }
        }
    }
}

pub(in crate::services::agent_cli::claude) fn assess_assets(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let index = NativeIndex::new(request.context, request.declarations, request.sources);
    let mut result = AgentAssetAssessmentIndex::default();
    for target in request.targets {
        result.insert(
            target.clone(),
            match index
                .decide(target)
                .and_then(NativeDecision::into_assessment)
            {
                Ok(assessment) => assessment,
                Err(failure) => AgentAssetAssessmentResult::Unsupported(failure),
            },
        );
    }
    result
}
