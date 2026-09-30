//! Native structure and the unchanged Skill/Status definitions.
use super::*;

pub(super) fn base<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    index.check(target)?;
    let bucket = index
        .buckets
        .get(&(
            target.category,
            target.resolution_group_key.clone(),
            target.exact_native_id.clone(),
        ))
        .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
    let definitions = index::definitions(bucket);
    if definitions.is_empty() {
        return trust_suppressed(bucket, target);
    }
    let selection = index.select(&definitions)?;
    let additive = target.category == AgentAssetCategory::Hook && definitions.len() > 1;
    let structural = !additive && !selection.conflicts.is_empty();
    let winner_ref = index::native_ref(selection.anchor);
    let overlays = index
        .overlays
        .get(&(target.category, target.resolution_group_key.clone()))
        .cloned()
        .unwrap_or_default();
    let (anchor, relation, key, represented, contributors) = match &target.subject {
        AgentAssetAssessmentSubject::Bucket if !additive => {
            let mut represented = bucket.clone();
            represented.extend(overlays.iter().copied().filter(|asset| {
                !bucket
                    .iter()
                    .any(|value| value.declaration_id == asset.declaration_id)
            }));
            // Same-key rewrite witnesses remain part of the complete native
            // bucket even when a required Definition supplies the final
            // transport. Selection and structural evidence are separate.
            let mut contributors = bucket
                .iter()
                .copied()
                .filter(|asset| {
                    asset.participation == AgentAssetResolutionParticipation::Participates
                })
                .collect::<Vec<_>>();
            contributors.extend(overlays.into_iter().filter(|asset| {
                !bucket
                    .iter()
                    .any(|member| member.declaration_id == asset.declaration_id)
            }));
            (
                selection.anchor,
                if structural {
                    AgentAssetResolutionRelation::Unknown
                } else if definitions.len() > 1 {
                    AgentAssetResolutionRelation::ReplaceWinner
                } else {
                    AgentAssetResolutionRelation::Independent
                },
                if structural {
                    qualified_projection_key(selection.anchor)
                } else {
                    format!("{}:{}", target.category.key(), target.exact_native_id)
                },
                represented,
                contributors,
            )
        }
        AgentAssetAssessmentSubject::Definition(id) if additive => {
            let anchor = definitions
                .iter()
                .copied()
                .find(|asset| asset.declaration_id == *id)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            (
                anchor,
                AgentAssetResolutionRelation::Additive,
                qualified_projection_key(anchor),
                bucket.clone(),
                vec![anchor],
            )
        }
        AgentAssetAssessmentSubject::Definition(id) if !structural => {
            let anchor = definitions
                .iter()
                .copied()
                .find(|asset| {
                    asset.declaration_id == *id
                        && asset.declaration_id != selection.anchor.declaration_id
                })
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
    let assessment = if structural {
        unknown_assessment(
            anchor,
            &selection.conflicts,
            AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
            selection
                .conflicts
                .iter()
                .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
                .collect(),
        )
    } else {
        definition_assessment(anchor)
    };
    // Replacement is whole-entry: lower Extension providers do not own a
    // settings winner. Conflicting provider identities have no unique edge.
    let relationships = if structural
        && selection.conflicts.iter().any(|asset| {
            asset.provided_by != anchor.provided_by || asset.action_owner != anchor.action_owner
        }) {
        AgentAssetResolvedRelationships::default()
    } else {
        aggregate_relationships(&[anchor])
            .map_err(|_| AgentAssetAssessmentFailure::InvalidNativeInput)?
    };
    let parent_state = relationships
        .provided_by
        .as_ref()
        .map(|parent| index.parent_state(parent))
        .transpose()?;
    let shadowed = relation == AgentAssetResolutionRelation::Replaced;
    let mut decision = NativeDecision {
        anchor,
        key,
        represented,
        contributors,
        resolution: AgentAssetResolutionDraft {
            relation,
            qualified_collision: index.collision(target),
            terminal: None,
            winner: matches!(
                relation,
                AgentAssetResolutionRelation::ReplaceWinner
                    | AgentAssetResolutionRelation::Replaced
            )
            .then_some(winner_ref.clone()),
            control_source: None,
        },
        assessment,
        effective: if shadowed {
            AgentAssetEffectiveStateProofDraft::Shadowed {
                winner: winner_ref,
                input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
            }
        } else {
            AgentAssetEffectiveStateProofDraft::Intrinsic
        },
        relationships,
        parent_state,
    };
    if structural {
        decision.terminal(NativeTerminal::unknown(
            AgentAssetTerminalCauseDraft::StructuralUnknown,
        ));
    }
    Ok(decision)
}

pub(super) fn apply_parent(decision: &mut NativeDecision<'_>) -> NativeResult<()> {
    if decision.resolution.relation == AgentAssetResolutionRelation::Replaced
        || !matches!(
            decision.effective,
            AgentAssetEffectiveStateProofDraft::Intrinsic
        )
        || decision.state()? != AgentAssetState::Enabled
    {
        return Ok(());
    }
    let Some(parent) = decision.relationships.provided_by.clone() else {
        return Ok(());
    };
    match decision.parent_state {
        Some(AgentAssetState::Enabled | AgentAssetState::Disabled) => {
            // Enabled preserves the intrinsic state, but the dependency must
            // still be proved against the surviving parent candidate.
            decision.effective = AgentAssetEffectiveStateProofDraft::ParentGate {
                parent,
                input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
            };
        }
        Some(AgentAssetState::Unknown | AgentAssetState::Blocked) => {
            decision.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
            decision.effective = NativeTerminal::unknown(
                AgentAssetTerminalCauseDraft::ParentUnknown,
            )
            .proof(AgentAssetEffectiveStateProofDraft::ParentGate {
                parent,
                input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
            });
        }
        _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    }
    Ok(())
}

pub(super) fn decide<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    let mut decision = base(index, target)?;
    apply_parent(&mut decision)?;
    Ok(decision)
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
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace
                }
            )
    }) {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let mut decision = NativeDecision {
        anchor,
        key: format!("mcp:{}", anchor.native_id),
        represented: bucket.to_vec(),
        contributors: Vec::new(),
        resolution: AgentAssetResolutionDraft {
            relation: AgentAssetResolutionRelation::Unknown,
            qualified_collision: false,
            terminal: None,
            winner: None,
            control_source: None,
        },
        assessment: unknown_assessment(
            anchor,
            &definitions,
            AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
            definitions
                .iter()
                .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
                .collect(),
        ),
        effective: AgentAssetEffectiveStateProofDraft::Intrinsic,
        relationships: AgentAssetResolvedRelationships::default(),
        parent_state: None,
    };
    decision.terminal(NativeTerminal::unknown(
        AgentAssetTerminalCauseDraft::TrustSuppressed,
    ));
    Ok(decision)
}
