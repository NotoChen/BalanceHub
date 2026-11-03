//! One native decision path serves construction and independent assessment.
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetNativeRef, AgentAssetResolutionParticipation,
    AgentAssetResolutionRelation, AgentAssetResolutionTerminal, AgentAssetScope, AgentAssetState,
    AgentConfigurationContext, AgentStatusUiMode,
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
    AgentResolveOutput, GrokStateControl, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    compose_effective_state, finalize_projected_draft, intrinsic_basis, qualified_projection_key,
    select_draft_structure,
};
use std::collections::{BTreeMap, BTreeSet};

mod hooks;
mod state;
use state::{
    decisive_overlays, ids, intrinsic_assessment, member, refs, set_declared, terminal, unknown,
};

#[cfg(test)]
mod tests;

type BucketKey = (AgentAssetCategory, String, String);

struct NativeIndex<'a> {
    buckets: BTreeMap<BucketKey, Vec<&'a ParsedAgentAsset>>,
    invalid_controls: BTreeMap<AgentAssetCategory, Vec<&'a ParsedAgentAsset>>,
    collision_groups: BTreeSet<(AgentAssetCategory, String)>,
    invalid_targets: BTreeSet<BucketKey>,
    invalid_skill_control: bool,
    plugins: super::plugin::PluginIndex<'a>,
    hooks: hooks::HookIndex<'a>,
}

struct NativeDecision<'a> {
    anchor: &'a ParsedAgentAsset,
    key: String,
    represented: Vec<&'a ParsedAgentAsset>,
    contributors: Vec<&'a ParsedAgentAsset>,
    resolution: AgentAssetResolutionDraft,
    assessment: AgentAssetNativeAssessment,
    effective: AgentAssetEffectiveStateProofDraft,
    parent_state: Option<AgentAssetState>,
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
                effective: self.effective,
                relationships: structure.relationships,
            }),
        }
    }

    fn finalize(self) -> Result<AgentAssetProjectedDraft, AgentAssetDiagnostic> {
        let invalid = || AgentAssetDiagnostic::InvalidResolution {
            projection_key: self.key.clone(),
            resolution: self.resolution.relation,
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
            &self.effective,
            self.parent_state,
        )
        .ok_or_else(invalid)?;
        finalize_projected_draft(AgentAssetResolvedDraftInput {
            anchor: structure.anchor,
            projection_key: self.key.clone(),
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
                effective: self.effective,
            },
        })
        .map_err(|_| invalid())
    }
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

fn replacement(category: AgentAssetCategory) -> bool {
    matches!(
        category,
        AgentAssetCategory::Mcp
            | AgentAssetCategory::Plugin
            | AgentAssetCategory::StatusUi
            | AgentAssetCategory::Skill
    )
}

fn winner<'a>(definitions: &[&'a ParsedAgentAsset]) -> Option<&'a ParsedAgentAsset> {
    let first = *definitions.first()?;
    if definitions.len() == 1
        || (replacement(first.category)
            && definitions[1].logical_origin.precedence < first.logical_origin.precedence)
    {
        Some(first)
    } else {
        None
    }
}

impl<'a> NativeIndex<'a> {
    fn new(
        context: &AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let mut buckets = BTreeMap::<BucketKey, Vec<&ParsedAgentAsset>>::new();
        let mut invalid_controls = BTreeMap::<AgentAssetCategory, Vec<&ParsedAgentAsset>>::new();
        let mut source_index = BTreeMap::<&str, Vec<&AgentAssetSourceSpec>>::new();
        for source in sources {
            source_index
                .entry(&source.native_source_key)
                .or_default()
                .push(source);
        }
        let mut declaration_ids = BTreeMap::<&str, usize>::new();
        for asset in declarations {
            *declaration_ids.entry(&asset.declaration_id).or_default() += 1;
        }
        let mut invalid_targets = BTreeSet::new();
        let mut invalid_skill_control = false;
        let plugins = super::plugin::PluginIndex::new(context, declarations, sources);
        for asset in declarations {
            buckets
                .entry((
                    asset.category,
                    asset.resolution_group_key.clone(),
                    asset.native_id.clone(),
                ))
                .or_default()
                .push(asset);
            if asset.category == AgentAssetCategory::Skill
                && !super::plugin::is_plugin_payload(asset)
                && !(declaration_ids.get(asset.declaration_id.as_str()) == Some(&1)
                    && source_index
                        .get(asset.source_key.as_str())
                        .is_some_and(|sources| {
                            sources.len() == 1
                                && super::skill::valid_declaration(context, asset, sources[0])
                        }))
            {
                invalid_targets.insert((
                    asset.category,
                    asset.resolution_group_key.clone(),
                    asset.native_id.clone(),
                ));
                invalid_skill_control |= asset.role == AgentAssetDeclarationRole::StateOverlay;
            }
            if (super::plugin::is_plugin_payload(asset)
                || (asset.category == AgentAssetCategory::Plugin
                    && asset.role == AgentAssetDeclarationRole::Definition))
                && !plugins.valid(asset)
            {
                invalid_targets.insert((
                    asset.category,
                    asset.resolution_group_key.clone(),
                    asset.native_id.clone(),
                ));
            }
            if asset.role == AgentAssetDeclarationRole::StateOverlay
                && asset.participation == AgentAssetResolutionParticipation::Participates
                && matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::GrokStateControl(
                        GrokStateControl::InvalidMcp
                            | GrokStateControl::InvalidPlugin
                            | GrokStateControl::InvalidSkill
                    )
                )
            {
                invalid_controls
                    .entry(asset.category)
                    .or_default()
                    .push(asset);
            }
        }
        for bucket in buckets.values_mut() {
            bucket.sort_by(|left, right| {
                (left.role != AgentAssetDeclarationRole::Definition)
                    .cmp(&(right.role != AgentAssetDeclarationRole::Definition))
                    .then_with(|| {
                        plugins.compare(left, right).unwrap_or_else(|| {
                            right
                                .logical_origin
                                .precedence
                                .cmp(&left.logical_origin.precedence)
                                .then_with(|| left.source_key.cmp(&right.source_key))
                                .then_with(|| left.declaration_id.cmp(&right.declaration_id))
                        })
                    })
            });
        }
        let mut definition_counts = BTreeMap::<(AgentAssetCategory, String), usize>::new();
        for ((category, group, _), bucket) in &buckets {
            if bucket
                .iter()
                .any(|asset| asset.role == AgentAssetDeclarationRole::Definition)
            {
                *definition_counts
                    .entry((*category, group.clone()))
                    .or_default() += 1;
            }
        }
        let collision_groups = definition_counts
            .into_iter()
            .filter_map(|(group, count)| (count > 1).then_some(group))
            .collect();
        Self {
            buckets,
            invalid_controls,
            collision_groups,
            invalid_targets,
            invalid_skill_control,
            plugins,
            hooks: hooks::HookIndex::new(context, declarations, sources),
        }
    }

    fn targets(&self) -> Vec<AgentAssetAssessmentTarget> {
        let mut targets = Vec::new();
        for ((category, group, native_id), bucket) in &self.buckets {
            let definitions = definitions(bucket);
            if definitions.is_empty() {
                continue;
            }
            let target = AgentAssetAssessmentTarget {
                category: *category,
                resolution_group_key: group.clone(),
                exact_native_id: native_id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            };
            targets.push(target.clone());
            if self.winner(&definitions).is_some() {
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
        targets
    }

    fn decide(
        &self,
        target: &AgentAssetAssessmentTarget,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let decision = self.decide_base(target)?;
        if target.category == AgentAssetCategory::Hook {
            self.hooks.apply(decision, &self.plugins)
        } else {
            Ok(decision)
        }
    }

    fn decide_base(
        &self,
        target: &AgentAssetAssessmentTarget,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        if (target.category == AgentAssetCategory::Skill && self.invalid_skill_control)
            || (target.category == AgentAssetCategory::Mcp
                && self.plugins.incomplete_mcp(&target.exact_native_id))
            || (target.category == AgentAssetCategory::Hook
                && self.plugins.incomplete_hooks(&target.exact_native_id))
            || self.invalid_targets.contains(&(
                target.category,
                target.resolution_group_key.clone(),
                target.exact_native_id.clone(),
            ))
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let mut bucket = self
            .buckets
            .get(&(
                target.category,
                target.resolution_group_key.clone(),
                target.exact_native_id.clone(),
            ))
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?
            .clone();
        if target.category == AgentAssetCategory::Skill
            && target.exact_native_id != target.resolution_group_key
        {
            if let Some(controls) = self.buckets.get(&(
                AgentAssetCategory::Skill,
                target.resolution_group_key.clone(),
                target.resolution_group_key.clone(),
            )) {
                bucket.extend(
                    controls
                        .iter()
                        .copied()
                        .filter(|asset| asset.role == AgentAssetDeclarationRole::StateOverlay),
                );
            }
        }
        let definitions = definitions(&bucket);
        let first = *definitions
            .first()
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        let collision = self
            .collision_groups
            .contains(&(target.category, target.resolution_group_key.clone()));
        let Some(selected) = self.winner(&definitions) else {
            if target.subject != AgentAssetAssessmentSubject::Bucket {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            let peers = if replacement(target.category) {
                definitions
                    .iter()
                    .copied()
                    .filter(|asset| {
                        asset.logical_origin.precedence == first.logical_origin.precedence
                    })
                    .collect::<Vec<_>>()
            } else {
                definitions.clone()
            };
            let mut details = first.details.clone();
            set_declared(&mut details, AgentAssetDeclaredState::Unknown);
            let assessment = unknown(
                first,
                details,
                &peers,
                AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
            );
            return Ok(NativeDecision {
                anchor: first,
                key: qualified_projection_key(first),
                represented: bucket.to_vec(),
                contributors: bucket
                    .iter()
                    .copied()
                    .filter(|asset| {
                        asset.participation == AgentAssetResolutionParticipation::Participates
                    })
                    .collect(),
                resolution: AgentAssetResolutionDraft {
                    relation: AgentAssetResolutionRelation::Unknown,
                    qualified_collision: collision,
                    terminal: Some(AgentAssetResolutionTerminal::Unknown),
                    winner: None,
                    control_source: None,
                },
                assessment,
                effective: terminal(AgentAssetTerminalCauseDraft::StructuralUnknown, Vec::new()),
                parent_state: None,
                relationship_contributors: None,
            });
        };
        let winner_key = format!("{}:{}", selected.category.key(), selected.native_id);
        let winner_ref = AgentAssetNativeRef {
            category: selected.category,
            native_id: selected.native_id.clone(),
            qualifier: Some(winner_key.clone()),
        };
        let (anchor, represented, contributors, relation, key) = match &target.subject {
            AgentAssetAssessmentSubject::Bucket => (
                selected,
                bucket.to_vec(),
                bucket
                    .iter()
                    .copied()
                    .filter(|asset| {
                        asset.participation == AgentAssetResolutionParticipation::Participates
                    })
                    .collect(),
                if definitions.len() == 1 {
                    AgentAssetResolutionRelation::Independent
                } else {
                    AgentAssetResolutionRelation::ReplaceWinner
                },
                winner_key,
            ),
            AgentAssetAssessmentSubject::Definition(id) => {
                let loser = definitions
                    .iter()
                    .copied()
                    .skip(1)
                    .find(|asset| asset.declaration_id == *id)
                    .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
                (
                    loser,
                    vec![loser],
                    vec![loser],
                    AgentAssetResolutionRelation::Replaced,
                    qualified_projection_key(loser),
                )
            }
        };
        let mut assessment = intrinsic_assessment(anchor, &self.plugins)?;
        let mut effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
        let mut terminal_kind = None;
        let mut control_source = None;
        if relation == AgentAssetResolutionRelation::Replaced {
            effective = AgentAssetEffectiveStateProofDraft::Shadowed {
                winner: winner_ref.clone(),
                input: Box::new(effective),
            };
        } else {
            let overlays = decisive_overlays(&bucket, target.category);
            if !overlays.is_empty() {
                let state = overlays[0].declared_state;
                let mut details = assessment.intrinsic.details.clone();
                set_declared(&mut details, state);
                let overlay_scope = if target.category == AgentAssetCategory::Skill
                    && anchor.native_id != anchor.resolution_group_key
                {
                    AgentAssetStateOverlayScopeDraft::ResolutionGroup
                } else {
                    AgentAssetStateOverlayScopeDraft::ExactNativeId
                };
                let members = overlays
                    .iter()
                    .map(|asset| {
                        member(
                            asset,
                            AgentAssetEvidenceKind::Overlay {
                                scope: overlay_scope,
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                assessment = AgentAssetNativeAssessment {
                    declared_state: state,
                    declared: AgentAssetDeclaredStateProofDraft::Overlay {
                        declaration_ids: ids(&overlays),
                        scope: overlay_scope,
                        outcome: state,
                    },
                    declared_members: members,
                    intrinsic: intrinsic_basis(details, anchor.trust_state),
                    control: None,
                };
            }
            if let Some(invalid) = self
                .invalid_controls
                .get(&target.category)
                .filter(|_| target.category != AgentAssetCategory::Skill || overlays.is_empty())
            {
                let control = AgentAssetControlAssessment {
                    cause: AgentAssetTerminalCauseDraft::InvalidControl,
                    members: invalid
                        .iter()
                        .map(|asset| member(asset, AgentAssetEvidenceKind::InvalidControl))
                        .collect(),
                    authorities: invalid
                        .iter()
                        .map(|asset| {
                            AgentAssetControlAuthority::Declaration(asset.declaration_id.clone())
                        })
                        .collect(),
                };
                if invalid.len() == 1 {
                    control_source = Some(AgentAssetPolicyReferenceDraft::Declaration {
                        declaration_id: invalid[0].declaration_id.clone(),
                    });
                }
                terminal_kind = Some(AgentAssetResolutionTerminal::Unknown);
                effective = terminal(AgentAssetTerminalCauseDraft::InvalidControl, refs(invalid));
                assessment.control = Some(control);
            }
        }
        let parent_state = if let Some(parent) = self.plugins.parent(anchor) {
            let parent_decision = self.decide(&AgentAssetAssessmentTarget {
                category: AgentAssetCategory::Plugin,
                resolution_group_key: parent.resolution_group_key.clone(),
                exact_native_id: parent.native_id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            })?;
            if parent_decision.anchor.declaration_id != parent.declaration_id {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            Some(
                compose_effective_state(
                    &parent_decision.assessment.intrinsic,
                    &parent_decision.effective,
                    None,
                )
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?
                .0,
            )
        } else {
            None
        };
        if relation != AgentAssetResolutionRelation::Replaced
            && matches!(effective, AgentAssetEffectiveStateProofDraft::Intrinsic)
            && compose_effective_state(&assessment.intrinsic, &effective, None)
                .is_some_and(|(state, _)| state == AgentAssetState::Enabled)
        {
            if let Some(parent) = anchor.provided_by.clone() {
                let gate = AgentAssetEffectiveStateProofDraft::ParentGate {
                    parent,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                };
                effective = match parent_state {
                    Some(
                        AgentAssetState::Enabled
                        | AgentAssetState::Disabled
                        | AgentAssetState::NotInstalled,
                    ) => gate,
                    Some(AgentAssetState::Unknown | AgentAssetState::Blocked) => {
                        terminal_kind = Some(AgentAssetResolutionTerminal::Unknown);
                        AgentAssetEffectiveStateProofDraft::Terminal {
                            terminal: AgentAssetResolutionTerminal::Unknown,
                            cause: AgentAssetTerminalCauseDraft::ParentUnknown,
                            evidence: Vec::new(),
                            input: Box::new(gate),
                        }
                    }
                    _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
                };
            }
        }
        let relationship_contributors = definitions
            .iter()
            .any(|asset| asset.provided_by.is_some())
            .then(|| vec![anchor]);
        Ok(NativeDecision {
            anchor,
            key,
            represented,
            contributors,
            resolution: AgentAssetResolutionDraft {
                relation,
                qualified_collision: collision,
                terminal: terminal_kind,
                winner: (definitions.len() > 1).then_some(winner_ref),
                control_source,
            },
            assessment,
            effective,
            parent_state,
            relationship_contributors,
        })
    }

    fn winner(&self, definitions: &[&'a ParsedAgentAsset]) -> Option<&'a ParsedAgentAsset> {
        if self.plugins.ordered_winner(definitions) {
            definitions.first().copied()
        } else {
            winner(definitions)
        }
    }
}

pub(in crate::services::agent_cli::grok) fn resolve_assets(
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
            Ok(decision) => match decision.finalize() {
                Ok(draft) => {
                    if output.emit_draft(draft).is_break() {
                        return;
                    }
                }
                Err(diagnostic) => {
                    output.emit_diagnostic(diagnostic);
                }
            },
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

pub(in crate::services::agent_cli::grok) fn assess_assets(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let index = NativeIndex::new(request.context, request.declarations, request.sources);
    let mut result = AgentAssetAssessmentIndex::default();
    for target in request.targets {
        result.insert(
            target.clone(),
            match index.decide(target) {
                Ok(decision) => decision.into_assessment(),
                Err(failure) => AgentAssetAssessmentResult::Unsupported(failure),
            },
        );
    }
    result
}
