//! Codex's one native decision path. The resolver finalizes its decisions;
//! the independent assessor returns the same decisions' typed witnesses.

mod assets;
mod hooks;

#[cfg(test)]
mod tests;

use super::{merge_and_decode_mcp, CodexMcpServerConfig};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetEffectiveAvailability,
    AgentAssetNativeRef, AgentAssetResolutionParticipation, AgentAssetResolutionRelation,
    AgentAssetResolutionTerminal, AgentAssetSuppressionReason, AgentMcpApprovalState,
    AgentMcpTransport, AgentStatusUiMode, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentFailure, AgentAssetAssessmentIndex, AgentAssetAssessmentRequest,
    AgentAssetAssessmentResult, AgentAssetAssessmentSubject, AgentAssetAssessmentTarget,
    AgentAssetControlAssessment, AgentAssetControlAuthority, AgentAssetDeclaredStateProofDraft,
    AgentAssetDeclaredUnknownCauseDraft, AgentAssetEffectiveStateProofDraft,
    AgentAssetEvidenceKind, AgentAssetEvidenceMember, AgentAssetNativeAssessment,
    AgentAssetNativePayload, AgentAssetPolicyReferenceDraft, AgentAssetProjectedDraft,
    AgentAssetProjectionAssessment, AgentAssetResolutionDraft, AgentAssetResolveRequest,
    AgentAssetResolvedDraftInput, AgentAssetSourceSpec, AgentAssetStateEvidenceRefDraft,
    AgentAssetStateOverlayScopeDraft, AgentAssetStateProofDraft, AgentAssetTerminalCauseDraft,
    AgentResolveOutput, CodexMcpRequirement, CodexRequirementsPayload, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    compose_effective_state, declaration_matches_source, finalize_projected_draft, intrinsic_basis,
    qualified_projection_key, select_draft_structure,
};
use std::collections::{BTreeMap, BTreeSet};

type BucketKey = (AgentAssetCategory, String, String);

struct NativeIndex<'a> {
    context: &'a crate::models::AgentConfigurationContext,
    buckets: BTreeMap<BucketKey, Vec<&'a ParsedAgentAsset>>,
    requirements: Result<RequirementsIndex<'a>, AgentAssetAssessmentFailure>,
    assets: assets::AssetIndex<'a>,
    hooks: hooks::HookIndex<'a>,
}

enum RequirementsIndex<'a> {
    NoPolicy,
    Invalid(&'a ParsedAgentAsset),
    Allowlist {
        root: &'a ParsedAgentAsset,
        entries: BTreeMap<&'a str, (&'a ParsedAgentAsset, &'a CodexMcpRequirement)>,
    },
}

#[derive(Clone)]
struct NativeDecision<'a> {
    anchor: &'a ParsedAgentAsset,
    projection_key: String,
    represented: Vec<&'a ParsedAgentAsset>,
    contributors: Vec<&'a ParsedAgentAsset>,
    resolution: AgentAssetResolutionDraft,
    assessment: AgentAssetNativeAssessment,
    effective_proof: AgentAssetEffectiveStateProofDraft,
    malformed: bool,
    parent_state: Option<crate::models::AgentAssetState>,
    relationship_contributors: Option<Vec<&'a ParsedAgentAsset>>,
}

impl NativeDecision<'_> {
    fn into_assessment(self) -> AgentAssetAssessmentResult {
        let Ok(structure) = select_draft_structure(
            self.anchor,
            &self.represented,
            self.relationship_contributors
                .as_deref()
                .unwrap_or(&self.contributors),
        ) else {
            return AgentAssetAssessmentResult::Unsupported(
                AgentAssetAssessmentFailure::InvalidNativeInput,
            );
        };
        AgentAssetAssessmentResult::Assessed {
            state: self.assessment,
            projection: Box::new(AgentAssetProjectionAssessment {
                anchor_declaration_id: structure.anchor.declaration_id.clone(),
                relation: self.resolution.relation,
                effective: self.effective_proof,
                relationships: structure.relationships,
            }),
        }
    }

    fn finalize(self) -> Result<AgentAssetProjectedDraft, AgentAssetDiagnostic> {
        let projection_key = self.projection_key.clone();
        let relation = self.resolution.relation;
        let invalid = || AgentAssetDiagnostic::InvalidResolution {
            projection_key: projection_key.clone(),
            resolution: relation,
        };
        let structure = select_draft_structure(
            self.anchor,
            &self.represented,
            self.relationship_contributors
                .as_deref()
                .unwrap_or(&self.contributors),
        )
        .map_err(|_| invalid())?;
        let (effective_state, details) = compose_effective_state(
            &self.assessment.intrinsic,
            &self.effective_proof,
            self.parent_state,
        )
        .ok_or_else(invalid)?;
        finalize_projected_draft(AgentAssetResolvedDraftInput {
            anchor: structure.anchor,
            projection_key: self.projection_key.clone(),
            represented: &self.represented,
            contributors: &self.contributors,
            declared_state: self.assessment.declared_state,
            effective_state,
            trust_state: self.assessment.intrinsic.trust_state,
            resolution: self.resolution.clone(),
            details,
            relationships: structure.relationships,
            state_proof: AgentAssetStateProofDraft {
                declared: self.assessment.declared,
                effective: self.effective_proof,
            },
        })
        .map_err(|_| invalid())
    }
}

impl<'a> NativeIndex<'a> {
    fn new(
        context: &'a crate::models::AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let mut buckets = BTreeMap::<BucketKey, Vec<&ParsedAgentAsset>>::new();
        for declaration in declarations {
            buckets
                .entry((
                    declaration.category,
                    declaration.resolution_group_key.clone(),
                    declaration.native_id.clone(),
                ))
                .or_default()
                .push(declaration);
        }
        for bucket in buckets.values_mut() {
            bucket.sort_by(|left, right| {
                left.logical_origin
                    .precedence
                    .cmp(&right.logical_origin.precedence)
                    .then_with(|| left.source_key.cmp(&right.source_key))
                    .then_with(|| left.declaration_id.cmp(&right.declaration_id))
            });
        }
        Self {
            context,
            buckets,
            requirements: requirements_index(context, declarations, sources),
            assets: assets::AssetIndex::new(context, declarations, sources),
            hooks: hooks::HookIndex::new(context, declarations, sources),
        }
    }

    fn targets(&self) -> Vec<AgentAssetAssessmentTarget> {
        let mut targets = Vec::new();
        for ((category, group, native_id), bucket) in &self.buckets {
            let definitions = definitions(bucket, true);
            if definitions.is_empty() {
                // Unknown trust remains declaration-only. Explicitly required
                // or untrusted workspaces retain the strict read-only row.
                let suppressed = definitions_for_trust_suppression(bucket);
                if *category != AgentAssetCategory::Mcp
                    || suppressed.is_none()
                    || !matches!(
                        self.context.trust_context,
                        AgentTrustState::Required | AgentTrustState::Untrusted
                    )
                {
                    continue;
                }
            }
            if *category == AgentAssetCategory::Plugin && !self.assets.configured_plugin(native_id)
            {
                continue;
            }
            let target = AgentAssetAssessmentTarget {
                category: *category,
                resolution_group_key: group.clone(),
                exact_native_id: native_id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            };
            if *category == AgentAssetCategory::Skill && definitions.len() > 1 {
                for definition in &definitions {
                    targets.push(AgentAssetAssessmentTarget {
                        subject: AgentAssetAssessmentSubject::Definition(
                            definition.declaration_id.clone(),
                        ),
                        ..target.clone()
                    });
                }
                continue;
            }
            targets.push(target.clone());
            if *category == AgentAssetCategory::Mcp && assets::has_plugin_mcp(bucket) {
                if let Ok(winner) = self.assets.mcp_winner(bucket) {
                    for definition in &definitions {
                        if definition.declaration_id != winner.declaration_id {
                            targets.push(AgentAssetAssessmentTarget {
                                subject: AgentAssetAssessmentSubject::Definition(
                                    definition.declaration_id.clone(),
                                ),
                                ..target.clone()
                            });
                        }
                    }
                }
            }
            if *category == AgentAssetCategory::StatusUi
                && replacement_winner(&definitions).is_some()
                && definitions.len() > 1
            {
                for definition in definitions.iter().take(definitions.len() - 1) {
                    targets.push(AgentAssetAssessmentTarget {
                        subject: AgentAssetAssessmentSubject::Definition(
                            definition.declaration_id.clone(),
                        ),
                        ..target.clone()
                    });
                }
            }
        }
        targets
    }

    fn decide(
        &self,
        target: &AgentAssetAssessmentTarget,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let key = (
            target.category,
            target.resolution_group_key.clone(),
            target.exact_native_id.clone(),
        );
        let bucket = self
            .buckets
            .get(&key)
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        match target.category {
            AgentAssetCategory::Mcp => {
                if assets::has_plugin_mcp(bucket) {
                    return self.assets.mcp(
                        bucket,
                        target,
                        &self.requirements,
                        self.context.trust_context,
                    );
                }
                if target.subject != AgentAssetAssessmentSubject::Bucket {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
                decide_mcp(bucket, &self.requirements, self.context.trust_context)
            }
            AgentAssetCategory::StatusUi => decide_definition_bucket(bucket, target, true),
            AgentAssetCategory::Hook => self.hooks.decide(bucket, target, &self.assets),
            AgentAssetCategory::Skill => self.assets.skill(bucket, target),
            AgentAssetCategory::Plugin if target.subject == AgentAssetAssessmentSubject::Bucket => {
                self.assets.plugin(&target.exact_native_id)
            }
            AgentAssetCategory::Plugin => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
            AgentAssetCategory::Extension => decide_definition_bucket(bucket, target, false),
        }
    }
}

pub(in crate::services::agent_cli::codex) fn resolve_assets(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let sources = request
        .sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let index = NativeIndex::new(request.context, request.declarations, &sources);
    for target in index.targets() {
        match index.decide(&target) {
            Ok(decision) => {
                if decision.malformed {
                    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
                        format: AgentAssetDocumentFormat::Toml,
                        location: Some(format!("mcp_servers.{}", target.exact_native_id)),
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

pub(in crate::services::agent_cli::codex) fn assess_assets(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let index = NativeIndex::new(request.context, request.declarations, request.sources);
    let mut assessments = AgentAssetAssessmentIndex::default();
    for target in request.targets {
        let result = match index.decide(target) {
            Ok(decision) => decision.into_assessment(),
            Err(failure) => AgentAssetAssessmentResult::Unsupported(failure),
        };
        assessments.insert(target.clone(), result);
    }
    assessments
}

fn definitions<'a>(
    bucket: &[&'a ParsedAgentAsset],
    participating: bool,
) -> Vec<&'a ParsedAgentAsset> {
    bucket
        .iter()
        .copied()
        .filter(|asset| {
            asset.role == AgentAssetDeclarationRole::Definition
                && (!participating
                    || asset.participation == AgentAssetResolutionParticipation::Participates)
        })
        .collect()
}

fn definitions_for_trust_suppression<'a>(
    bucket: &[&'a ParsedAgentAsset],
) -> Option<Vec<&'a ParsedAgentAsset>> {
    let definitions = definitions(bucket, false);
    let trust = definitions.first()?.trust_state;
    (matches!(
        trust,
        AgentTrustState::Required | AgentTrustState::Untrusted
    ) && definitions.iter().all(|definition| {
        definition.trust_state == trust
            && matches!(
                definition.participation,
                AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }))
    .then_some(definitions)
}

fn replacement_winner<'a>(definitions: &[&'a ParsedAgentAsset]) -> Option<&'a ParsedAgentAsset> {
    let winner = *definitions.last()?;
    if definitions
        .iter()
        .rev()
        .nth(1)
        .is_some_and(|peer| peer.logical_origin.precedence == winner.logical_origin.precedence)
    {
        return None;
    }
    Some(winner)
}

fn member(asset: &ParsedAgentAsset, kind: AgentAssetEvidenceKind) -> AgentAssetEvidenceMember {
    let expected_role = match kind {
        AgentAssetEvidenceKind::Definition => AgentAssetDeclarationRole::Definition,
        AgentAssetEvidenceKind::Overlay { .. }
        | AgentAssetEvidenceKind::InvalidStateControl { .. } => {
            AgentAssetDeclarationRole::StateOverlay
        }
        AgentAssetEvidenceKind::Policy | AgentAssetEvidenceKind::InvalidControl => {
            AgentAssetDeclarationRole::PolicyOverlay
        }
    };
    AgentAssetEvidenceMember {
        declaration_id: asset.declaration_id.clone(),
        expected_role,
        kind,
    }
}

fn config_member(
    asset: &ParsedAgentAsset,
) -> Result<AgentAssetEvidenceMember, AgentAssetAssessmentFailure> {
    let AgentAssetNativePayload::TomlTable(table) = &asset.native_payload else {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    };
    Ok(member(
        asset,
        match asset.role {
            AgentAssetDeclarationRole::Definition => AgentAssetEvidenceKind::Definition,
            AgentAssetDeclarationRole::StateOverlay
                if table
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .is_some() =>
            {
                AgentAssetEvidenceKind::Overlay {
                    scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
                }
            }
            AgentAssetDeclarationRole::StateOverlay => {
                AgentAssetEvidenceKind::InvalidStateControl {
                    scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
                }
            }
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        },
    ))
}

fn evidence(members: &[AgentAssetEvidenceMember]) -> Vec<AgentAssetStateEvidenceRefDraft> {
    let mut evidence = members
        .iter()
        .map(|member| AgentAssetStateEvidenceRefDraft::Declaration {
            declaration_id: member.declaration_id.clone(),
        })
        .collect::<Vec<_>>();
    evidence.sort();
    evidence
}

fn unknown_assessment(
    cause: AgentAssetDeclaredUnknownCauseDraft,
    members: Vec<AgentAssetEvidenceMember>,
    details: AgentAssetDetails,
    trust_state: AgentTrustState,
) -> AgentAssetNativeAssessment {
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Unknown,
        declared: AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: evidence(&members),
            cause,
        },
        declared_members: members,
        intrinsic: intrinsic_basis(details, trust_state),
        control: None,
    }
}

fn terminal(
    cause: AgentAssetTerminalCauseDraft,
    evidence: Vec<AgentAssetStateEvidenceRefDraft>,
) -> AgentAssetEffectiveStateProofDraft {
    AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: if cause == AgentAssetTerminalCauseDraft::TypedPolicy {
            AgentAssetResolutionTerminal::PolicyBlocked
        } else {
            AgentAssetResolutionTerminal::Unknown
        },
        cause,
        evidence,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    }
}

fn decide_mcp<'a>(
    bucket: &[&'a ParsedAgentAsset],
    requirements: &Result<RequirementsIndex<'a>, AgentAssetAssessmentFailure>,
    context_trust: AgentTrustState,
) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
    let definitions = definitions(bucket, true);
    if definitions.is_empty() {
        if !matches!(
            context_trust,
            AgentTrustState::Required | AgentTrustState::Untrusted
        ) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let suppressed = definitions_for_trust_suppression(bucket)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let anchor = *suppressed
            .last()
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        return Ok(NativeDecision {
            anchor,
            projection_key: format!("mcp:{}", anchor.native_id),
            represented: bucket.to_vec(),
            contributors: Vec::new(),
            resolution: resolution(
                AgentAssetResolutionRelation::Unknown,
                Some(AgentAssetResolutionTerminal::Unknown),
                None,
            ),
            assessment: unknown_assessment(
                AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
                suppressed
                    .iter()
                    .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
                    .collect(),
                mcp_details(AgentMcpTransport::Unknown, AgentAssetDeclaredState::Unknown),
                anchor.trust_state,
            ),
            effective_proof: terminal(AgentAssetTerminalCauseDraft::TrustSuppressed, Vec::new()),
            malformed: false,
            parent_state: None,
            relationship_contributors: None,
        });
    }
    let anchor = *definitions
        .last()
        .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
    let contributors = bucket
        .iter()
        .copied()
        .filter(|asset| asset.participation == AgentAssetResolutionParticipation::Participates)
        .collect::<Vec<_>>();
    let config = contributors
        .iter()
        .copied()
        .filter(|asset| asset.role != AgentAssetDeclarationRole::PolicyOverlay)
        .collect::<Vec<_>>();
    let config_members = config
        .iter()
        .map(|asset| config_member(asset))
        .collect::<Result<Vec<_>, _>>()?;
    let relation = if definitions.len() > 1 {
        AgentAssetResolutionRelation::Merged
    } else {
        AgentAssetResolutionRelation::Independent
    };
    let trust_state = config
        .iter()
        .map(|asset| asset.trust_state)
        .find(|trust| *trust != AgentTrustState::Unknown)
        .unwrap_or(AgentTrustState::Unknown);
    let (_, decoded) = match merge_and_decode_mcp(&config) {
        Ok(decoded) => decoded,
        Err(()) => {
            return Ok(NativeDecision {
                anchor,
                projection_key: format!("mcp:{}", anchor.native_id),
                represented: bucket.to_vec(),
                contributors,
                resolution: resolution(relation, Some(AgentAssetResolutionTerminal::Unknown), None),
                assessment: unknown_assessment(
                    AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                    config_members,
                    mcp_details(AgentMcpTransport::Unknown, AgentAssetDeclaredState::Unknown),
                    trust_state,
                ),
                effective_proof: terminal(
                    AgentAssetTerminalCauseDraft::DeclaredUnknown,
                    Vec::new(),
                ),
                malformed: true,
                parent_state: None,
                relationship_contributors: None,
            })
        }
    };
    let assessment = declared_mcp_assessment(&config, &decoded, trust_state)?;
    if assessment.declared_state == AgentAssetDeclaredState::Unknown {
        return Ok(NativeDecision {
            anchor,
            projection_key: format!("mcp:{}", anchor.native_id),
            represented: bucket.to_vec(),
            contributors,
            resolution: resolution(relation, Some(AgentAssetResolutionTerminal::Unknown), None),
            assessment,
            effective_proof: terminal(AgentAssetTerminalCauseDraft::DeclaredUnknown, Vec::new()),
            malformed: false,
            parent_state: None,
            relationship_contributors: None,
        });
    }
    let control = requirement_policy(
        anchor.native_id.as_str(),
        &decoded,
        requirements.as_ref().map_err(|failure| *failure)?,
    );
    let (terminal_kind, control_source, effective_proof) = match &control {
        None => (None, None, AgentAssetEffectiveStateProofDraft::Intrinsic),
        Some(control) => {
            let terminal_kind = if control.cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                AgentAssetResolutionTerminal::PolicyBlocked
            } else {
                AgentAssetResolutionTerminal::Unknown
            };
            let owner = match control.authorities.first() {
                Some(AgentAssetControlAuthority::Declaration(id)) => {
                    Some(AgentAssetPolicyReferenceDraft::Declaration {
                        declaration_id: id.clone(),
                    })
                }
                Some(AgentAssetControlAuthority::SourceAggregate(key)) => {
                    Some(AgentAssetPolicyReferenceDraft::Source {
                        source_key: key.clone(),
                    })
                }
                None => None,
            };
            let terminal_evidence = match control.authorities.first() {
                Some(AgentAssetControlAuthority::SourceAggregate(key))
                    if control.authorities.len() == 1 =>
                {
                    vec![AgentAssetStateEvidenceRefDraft::Source {
                        source_key: key.clone(),
                    }]
                }
                _ => evidence(&control.members),
            };
            (
                Some(terminal_kind),
                owner,
                terminal(control.cause, terminal_evidence),
            )
        }
    };
    Ok(NativeDecision {
        anchor,
        projection_key: format!("mcp:{}", anchor.native_id),
        represented: bucket.to_vec(),
        contributors,
        resolution: resolution(relation, terminal_kind, control_source),
        assessment: AgentAssetNativeAssessment {
            control,
            ..assessment
        },
        effective_proof,
        malformed: false,
        parent_state: None,
        relationship_contributors: None,
    })
}

fn declared_mcp_assessment(
    config: &[&ParsedAgentAsset],
    decoded: &CodexMcpServerConfig,
    trust_state: AgentTrustState,
) -> Result<AgentAssetNativeAssessment, AgentAssetAssessmentFailure> {
    let state = if decoded.enabled {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Disabled
    };
    let explicit = config.iter().copied().filter(|asset| matches!(&asset.native_payload, AgentAssetNativePayload::TomlTable(table) if table.contains_key("enabled"))).collect::<Vec<_>>();
    let Some(selected) = explicit.last().copied() else {
        let definition = config
            .iter()
            .rev()
            .copied()
            .find(|asset| asset.role == AgentAssetDeclarationRole::Definition)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        return Ok(AgentAssetNativeAssessment {
            declared_state: state,
            declared: AgentAssetDeclaredStateProofDraft::NativeDefault {
                definition_id: definition.declaration_id.clone(),
                outcome: state,
            },
            declared_members: vec![member(definition, AgentAssetEvidenceKind::Definition)],
            intrinsic: intrinsic_basis(mcp_details(decoded.transport, state), trust_state),
            control: None,
        });
    };
    if selected.role == AgentAssetDeclarationRole::StateOverlay {
        let peers = explicit
            .iter()
            .copied()
            .filter(|asset| {
                asset.role == AgentAssetDeclarationRole::StateOverlay
                    && asset.logical_origin.precedence == selected.logical_origin.precedence
            })
            .collect::<Vec<_>>();
        let members = peers
            .iter()
            .map(|asset| config_member(asset))
            .collect::<Result<Vec<_>, _>>()?;
        if peers.iter().any(|peer| peer.declared_state != state) {
            return Ok(unknown_assessment(
                AgentAssetDeclaredUnknownCauseDraft::OverlayConflict,
                members,
                mcp_details(decoded.transport, AgentAssetDeclaredState::Unknown),
                trust_state,
            ));
        }
        let mut ids = members
            .iter()
            .map(|member| member.declaration_id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        return Ok(AgentAssetNativeAssessment {
            declared_state: state,
            declared: AgentAssetDeclaredStateProofDraft::Overlay {
                declaration_ids: ids,
                scope: AgentAssetStateOverlayScopeDraft::ExactNativeId,
                outcome: state,
            },
            declared_members: members,
            intrinsic: intrinsic_basis(mcp_details(decoded.transport, state), trust_state),
            control: None,
        });
    }
    let members = config
        .iter()
        .filter(|asset| asset.role == AgentAssetDeclarationRole::Definition)
        .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
        .collect::<Vec<_>>();
    let mut ids = members
        .iter()
        .map(|member| member.declaration_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    Ok(AgentAssetNativeAssessment {
        declared_state: state,
        declared: AgentAssetDeclaredStateProofDraft::Definition {
            declaration_ids: ids,
            selected_id: selected.declaration_id.clone(),
        },
        declared_members: members,
        intrinsic: intrinsic_basis(mcp_details(decoded.transport, state), trust_state),
        control: None,
    })
}

fn requirements_index<'a>(
    context: &crate::models::AgentConfigurationContext,
    declarations: &'a [ParsedAgentAsset],
    sources: &[AgentAssetSourceSpec],
) -> Result<RequirementsIndex<'a>, AgentAssetAssessmentFailure> {
    let mut sources = sources
        .iter()
        .filter(|source| source.native_source_key == "system-requirements");
    let source = sources
        .next()
        .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
    if sources.next().is_some() {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let mut root = None;
    let mut entries = BTreeMap::new();
    let mut identity_counts = BTreeMap::<&str, usize>::new();
    for declaration in declarations {
        *identity_counts
            .entry(&declaration.declaration_id)
            .or_default() += 1;
    }
    for declaration in declarations
        .iter()
        .filter(|declaration| declaration.source_key == source.native_source_key)
    {
        let AgentAssetNativePayload::CodexRequirements(payload) = &declaration.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        if !declaration_matches_source(context, declaration, source)
            || identity_counts.get(declaration.declaration_id.as_str()) != Some(&1)
            || declaration.category != AgentAssetCategory::Mcp
            || declaration.role != AgentAssetDeclarationRole::PolicyOverlay
            || declaration.participation != AgentAssetResolutionParticipation::Participates
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        match payload {
            CodexRequirementsPayload::Entry(requirement) => {
                if entries
                    .insert(declaration.native_id.as_str(), (declaration, requirement))
                    .is_some()
                {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            _ => {
                if root.replace((declaration, payload)).is_some() {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
        }
    }
    let (root, payload) = root.ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
    match payload {
        CodexRequirementsPayload::MissingFile | CodexRequirementsPayload::NoPolicy
            if entries.is_empty() =>
        {
            Ok(RequirementsIndex::NoPolicy)
        }
        CodexRequirementsPayload::InvalidRequirements if entries.is_empty() => {
            Ok(RequirementsIndex::Invalid(root))
        }
        CodexRequirementsPayload::AllowlistRoot { entry_count } => {
            match entries.len().cmp(entry_count) {
                std::cmp::Ordering::Equal => Ok(RequirementsIndex::Allowlist { root, entries }),
                std::cmp::Ordering::Less => Err(AgentAssetAssessmentFailure::IncompleteInput),
                std::cmp::Ordering::Greater => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
            }
        }
        _ => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    }
}

fn requirement_policy(
    native_id: &str,
    config: &CodexMcpServerConfig,
    requirements: &RequirementsIndex<'_>,
) -> Option<AgentAssetControlAssessment> {
    match requirements {
        RequirementsIndex::NoPolicy => None,
        RequirementsIndex::Invalid(root) => Some(AgentAssetControlAssessment {
            cause: AgentAssetTerminalCauseDraft::InvalidControl,
            members: vec![member(root, AgentAssetEvidenceKind::InvalidControl)],
            authorities: BTreeSet::from([AgentAssetControlAuthority::Declaration(
                root.declaration_id.clone(),
            )]),
        }),
        RequirementsIndex::Allowlist { root, entries } => match entries.get(native_id) {
            Some((_, requirement)) if requirement.matches(config) => None,
            Some((entry, _)) => Some(AgentAssetControlAssessment {
                cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                members: vec![member(entry, AgentAssetEvidenceKind::Policy)],
                authorities: BTreeSet::from([AgentAssetControlAuthority::Declaration(
                    entry.declaration_id.clone(),
                )]),
            }),
            None => {
                let members = std::iter::once(*root)
                    .chain(entries.values().map(|(entry, _)| *entry))
                    .map(|entry| member(entry, AgentAssetEvidenceKind::Policy))
                    .collect();
                Some(AgentAssetControlAssessment {
                    cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                    members,
                    authorities: BTreeSet::from([AgentAssetControlAuthority::SourceAggregate(
                        root.source_key.clone(),
                    )]),
                })
            }
        },
    }
}

fn decide_definition_bucket<'a>(
    bucket: &[&'a ParsedAgentAsset],
    target: &AgentAssetAssessmentTarget,
    replacement: bool,
) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
    let definitions = definitions(bucket, true);
    let first = *definitions
        .first()
        .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
    if bucket
        .iter()
        .any(|asset| asset.role != AgentAssetDeclarationRole::Definition)
    {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let winner = if replacement {
        replacement_winner(&definitions)
    } else if definitions.len() == 1 {
        Some(first)
    } else {
        None
    };
    let Some(winner) = winner else {
        if target.subject != AgentAssetAssessmentSubject::Bucket {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let peers = if replacement {
            let precedence = definitions
                .last()
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?
                .logical_origin
                .precedence;
            definitions
                .iter()
                .copied()
                .filter(|definition| definition.logical_origin.precedence == precedence)
                .collect::<Vec<_>>()
        } else {
            definitions.clone()
        };
        return Ok(NativeDecision {
            anchor: first,
            projection_key: qualified_projection_key(first),
            represented: bucket.to_vec(),
            contributors: definitions,
            resolution: resolution(
                AgentAssetResolutionRelation::Unknown,
                Some(AgentAssetResolutionTerminal::Unknown),
                None,
            ),
            assessment: unknown_assessment(
                AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
                peers
                    .iter()
                    .map(|peer| member(peer, AgentAssetEvidenceKind::Definition))
                    .collect(),
                unknown_details(&first.details),
                first.trust_state,
            ),
            effective_proof: terminal(AgentAssetTerminalCauseDraft::StructuralUnknown, Vec::new()),
            malformed: false,
            parent_state: None,
            relationship_contributors: None,
        });
    };
    let winner_key = format!("{}:{}", winner.category.key(), winner.native_id);
    let winner_reference = AgentAssetNativeRef {
        category: winner.category,
        native_id: winner.native_id.clone(),
        qualifier: Some(winner_key.clone()),
    };
    let (anchor, represented, contributors, relation, projection_key) = match &target.subject {
        AgentAssetAssessmentSubject::Bucket => (
            winner,
            bucket.to_vec(),
            definitions.clone(),
            if definitions.len() == 1 {
                AgentAssetResolutionRelation::Independent
            } else {
                AgentAssetResolutionRelation::ReplaceWinner
            },
            winner_key,
        ),
        AgentAssetAssessmentSubject::Definition(id) if replacement && definitions.len() > 1 => {
            let loser = definitions
                .iter()
                .copied()
                .find(|definition| {
                    definition.declaration_id == *id
                        && definition.declaration_id != winner.declaration_id
                })
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            (
                loser,
                vec![loser],
                vec![loser],
                AgentAssetResolutionRelation::Replaced,
                qualified_projection_key(loser),
            )
        }
        _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    };
    let members = vec![member(anchor, AgentAssetEvidenceKind::Definition)];
    let assessment = if anchor.declared_state == AgentAssetDeclaredState::Unknown {
        unknown_assessment(
            AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            members,
            anchor.details.clone(),
            anchor.trust_state,
        )
    } else {
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
    };
    let (terminal_kind, effective_proof) = if relation == AgentAssetResolutionRelation::Replaced {
        (
            None,
            AgentAssetEffectiveStateProofDraft::Shadowed {
                winner: winner_reference.clone(),
                input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
            },
        )
    } else if anchor.declared_state == AgentAssetDeclaredState::Unknown {
        (
            Some(AgentAssetResolutionTerminal::Unknown),
            terminal(AgentAssetTerminalCauseDraft::DeclaredUnknown, Vec::new()),
        )
    } else {
        (None, AgentAssetEffectiveStateProofDraft::Intrinsic)
    };
    Ok(NativeDecision {
        anchor,
        projection_key,
        represented,
        contributors,
        resolution: AgentAssetResolutionDraft {
            relation,
            qualified_collision: false,
            terminal: terminal_kind,
            winner: (definitions.len() > 1).then_some(winner_reference),
            control_source: None,
        },
        assessment,
        effective_proof,
        malformed: false,
        parent_state: None,
        relationship_contributors: None,
    })
}

fn resolution(
    relation: AgentAssetResolutionRelation,
    terminal: Option<AgentAssetResolutionTerminal>,
    control_source: Option<AgentAssetPolicyReferenceDraft>,
) -> AgentAssetResolutionDraft {
    AgentAssetResolutionDraft {
        relation,
        qualified_collision: false,
        terminal,
        winner: None,
        control_source,
    }
}

fn mcp_details(
    transport: AgentMcpTransport,
    declared_state: AgentAssetDeclaredState,
) -> AgentAssetDetails {
    AgentAssetDetails::Mcp {
        transport,
        declared_state,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Unknown,
    }
}

fn unknown_details(details: &AgentAssetDetails) -> AgentAssetDetails {
    match details {
        AgentAssetDetails::Skill {
            invocation_policy, ..
        } => AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Unknown,
            invocation_policy: *invocation_policy,
        },
        AgentAssetDetails::Plugin {
            install_state,
            trusted,
            ..
        } => AgentAssetDetails::Plugin {
            install_state: *install_state,
            enabled: AgentAssetDeclaredState::Unknown,
            trusted: *trusted,
        },
        AgentAssetDetails::Extension {
            install_state,
            trusted,
            ..
        } => AgentAssetDetails::Extension {
            install_state: *install_state,
            enabled: AgentAssetDeclaredState::Unknown,
            trusted: *trusted,
        },
        AgentAssetDetails::Hook {
            managed,
            rule_count,
            ..
        } => AgentAssetDetails::Hook {
            enabled: AgentAssetDeclaredState::Unknown,
            managed: *managed,
            rule_count: *rule_count,
        },
        AgentAssetDetails::StatusUi { .. } => AgentAssetDetails::StatusUi {
            mode: AgentStatusUiMode::Unknown,
            command_present: false,
        },
        AgentAssetDetails::Mcp { .. } => {
            mcp_details(AgentMcpTransport::Unknown, AgentAssetDeclaredState::Unknown)
        }
    }
}
