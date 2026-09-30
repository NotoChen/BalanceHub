//! State witnesses are compared with a native assessment of the complete
//! parser index. This module knows identities, roles and proof composition;
//! it never decodes an adapter payload or infers native applicability.

use super::super::state_projection::{
    compose_effective_state, details_declared_state, intrinsic_basis_is_consistent,
};
use super::*;
use crate::models::AgentAssetResolutionTerminal;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentSubject, AgentAssetAssessmentTarget, AgentAssetControlAssessment,
    AgentAssetControlAuthority, AgentAssetDeclaredUnknownCauseDraft, AgentAssetEvidenceKind,
    AgentAssetEvidenceMember, AgentAssetNativeAssessment, AgentAssetProjectionAssessment,
    AgentAssetStateOverlayScopeDraft,
};

pub(super) fn assessment_target(item: &ValidatedDraft) -> Option<AgentAssetAssessmentTarget> {
    // Only group-validated additive/replaced rows can name a single Definition.
    // The subject is taken from their structural contributor set, never proof.
    let subject = match item.draft.resolution.relation {
        AgentAssetResolutionRelation::Additive | AgentAssetResolutionRelation::Replaced => {
            let mut definitions = item
                .contributors
                .iter()
                .filter(|declaration| declaration.role == AgentAssetDeclarationRole::Definition);
            let definition = definitions.next()?;
            if definitions.next().is_some() {
                return None;
            }
            AgentAssetAssessmentSubject::Definition(definition.declaration_id.clone())
        }
        _ => AgentAssetAssessmentSubject::Bucket,
    };
    let definition = item.represented.iter().find(|declaration| {
        declaration.role == AgentAssetDeclarationRole::Definition
            && declaration.native_id == item.draft.native_id
    })?;
    Some(AgentAssetAssessmentTarget {
        category: definition.category,
        resolution_group_key: definition.resolution_group_key.clone(),
        exact_native_id: definition.native_id.clone(),
        subject,
    })
}

pub(super) struct EvidenceIndex<'a> {
    context: &'a AgentConfigurationContext,
    sources: &'a [AgentAssetSourceSpec],
    declarations: &'a BTreeMap<String, ParsedAgentAsset>,
    invalid_ids: &'a BTreeSet<String>,
}

impl<'a> EvidenceIndex<'a> {
    pub(super) fn new(
        context: &'a AgentConfigurationContext,
        sources: &'a [AgentAssetSourceSpec],
        declarations: &'a BTreeMap<String, ParsedAgentAsset>,
        invalid_ids: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            context,
            sources,
            declarations,
            invalid_ids,
        }
    }

    fn declaration(&self, id: &str) -> Option<&'a ParsedAgentAsset> {
        if self.invalid_ids.contains(id) {
            return None;
        }
        let declaration = self.declarations.get(id)?;
        let source = source_for_key(self.sources, &declaration.source_key)?;
        let expected_id = stable_id(
            "declaration",
            &[
                self.context.id.as_str(),
                declaration.source_key.as_str(),
                declaration.category.key().as_str(),
                declaration.declaration_key.as_str(),
            ],
        );
        (declaration.declaration_id == expected_id
            && source.categories.contains(&declaration.category)
            && source.allows(declaration.logical_origin))
        .then_some(declaration)
    }

    fn members(
        &self,
        members: &[AgentAssetEvidenceMember],
        target: &AgentAssetAssessmentTarget,
        item: &ValidatedDraft,
        declared: bool,
        trust_suppressed: bool,
    ) -> Option<BTreeMap<String, &'a ParsedAgentAsset>> {
        let mut result = BTreeMap::new();
        for member in members {
            let declaration = self.declaration(&member.declaration_id)?;
            if declaration.category != target.category
                || declaration.role != member.expected_role
                || !kind_matches_role(member.kind, declaration.role, declared)
                || result
                    .insert(member.declaration_id.clone(), declaration)
                    .is_some()
            {
                return None;
            }
            if trust_suppressed {
                if member.kind != AgentAssetEvidenceKind::Definition
                    || !matches!(
                        declaration.participation,
                        AgentAssetResolutionParticipation::Suppressed {
                            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                        }
                    )
                {
                    return None;
                }
            } else if declaration.participation != AgentAssetResolutionParticipation::Participates {
                return None;
            }
            // A known native policy can control a different structural bucket
            // without becoming that bucket's represented/contributing member.
            // Its exact applicability and full membership are independently
            // supplied by the assessor, then checked below by set equality.
            let external_declared_policy = declared
                && member.kind == AgentAssetEvidenceKind::Policy
                && matches!(
                    item.draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::Policy { outcome, .. }
                        if outcome != AgentAssetDeclaredState::Unknown
                );
            if declared && !external_declared_policy {
                if !item
                    .draft
                    .represented_declaration_ids
                    .contains(&member.declaration_id)
                    || (!trust_suppressed
                        && !item.draft.contributor_ids.contains(&member.declaration_id))
                    || !member_is_in_declared_scope(member, declaration, target)
                {
                    return None;
                }
                if let AgentAssetAssessmentSubject::Definition(subject) = &target.subject {
                    if member.kind == AgentAssetEvidenceKind::Definition
                        && member.declaration_id != *subject
                    {
                        return None;
                    }
                }
            }
        }
        (!result.is_empty()).then_some(result)
    }

    /// A Source reference expands only to this target/cause's independently
    /// assessed members. Overlapping aliases are rejected after expansion.
    fn expand(
        &self,
        evidence: &[AgentAssetStateEvidenceRefDraft],
        members: &BTreeMap<String, &ParsedAgentAsset>,
    ) -> Option<BTreeSet<String>> {
        if evidence.is_empty() || evidence.windows(2).any(|pair| pair[0] >= pair[1]) {
            return None;
        }
        let mut ids = BTreeSet::new();
        for reference in evidence {
            match reference {
                AgentAssetStateEvidenceRefDraft::Declaration { declaration_id } => {
                    self.declaration(declaration_id)?;
                    if !members.contains_key(declaration_id) || !ids.insert(declaration_id.clone())
                    {
                        return None;
                    }
                }
                AgentAssetStateEvidenceRefDraft::Source { source_key } => {
                    source_for_key(self.sources, source_key)?;
                    let mut source_members = members
                        .iter()
                        .filter(|(_, declaration)| declaration.source_key == *source_key)
                        .peekable();
                    source_members.peek()?;
                    for (id, _) in source_members {
                        if !ids.insert(id.clone()) {
                            return None;
                        }
                    }
                }
            }
        }
        Some(ids)
    }
}

fn kind_matches_role(
    kind: AgentAssetEvidenceKind,
    role: AgentAssetDeclarationRole,
    declared: bool,
) -> bool {
    match kind {
        AgentAssetEvidenceKind::Definition => role == AgentAssetDeclarationRole::Definition,
        AgentAssetEvidenceKind::Overlay { .. }
        | AgentAssetEvidenceKind::InvalidStateControl { .. } => {
            role == AgentAssetDeclarationRole::StateOverlay
        }
        AgentAssetEvidenceKind::Policy => role == AgentAssetDeclarationRole::PolicyOverlay,
        AgentAssetEvidenceKind::InvalidControl => {
            !declared
                && matches!(
                    role,
                    AgentAssetDeclarationRole::StateOverlay
                        | AgentAssetDeclarationRole::PolicyOverlay
                )
        }
    }
}

fn member_is_in_declared_scope(
    member: &AgentAssetEvidenceMember,
    declaration: &ParsedAgentAsset,
    target: &AgentAssetAssessmentTarget,
) -> bool {
    if declaration.resolution_group_key != target.resolution_group_key {
        return false;
    }
    match member.kind {
        AgentAssetEvidenceKind::Definition => declaration.native_id == target.exact_native_id,
        AgentAssetEvidenceKind::Overlay { scope }
        | AgentAssetEvidenceKind::InvalidStateControl { scope } => {
            scope == AgentAssetStateOverlayScopeDraft::ResolutionGroup
                || declaration.native_id == target.exact_native_id
        }
        AgentAssetEvidenceKind::Policy => true,
        AgentAssetEvidenceKind::InvalidControl => false,
    }
}

fn proof_evidence(
    proof: &AgentAssetDeclaredStateProofDraft,
) -> Vec<AgentAssetStateEvidenceRefDraft> {
    match proof {
        AgentAssetDeclaredStateProofDraft::Definition {
            declaration_ids, ..
        }
        | AgentAssetDeclaredStateProofDraft::Overlay {
            declaration_ids, ..
        }
        | AgentAssetDeclaredStateProofDraft::Policy {
            declaration_ids, ..
        } => declaration_ids
            .iter()
            .cloned()
            .map(|declaration_id| AgentAssetStateEvidenceRefDraft::Declaration { declaration_id })
            .collect(),
        AgentAssetDeclaredStateProofDraft::NativeDefault { definition_id, .. } => {
            vec![AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: definition_id.clone(),
            }]
        }
        AgentAssetDeclaredStateProofDraft::Unknown { evidence, .. } => evidence.clone(),
    }
}

fn proof_decision_matches(
    actual: &AgentAssetDeclaredStateProofDraft,
    expected: &AgentAssetDeclaredStateProofDraft,
) -> bool {
    match (actual, expected) {
        (
            AgentAssetDeclaredStateProofDraft::Definition {
                selected_id: actual,
                ..
            },
            AgentAssetDeclaredStateProofDraft::Definition {
                selected_id: expected,
                ..
            },
        ) => actual == expected,
        (
            AgentAssetDeclaredStateProofDraft::NativeDefault {
                definition_id: a_id,
                outcome: a_outcome,
            },
            AgentAssetDeclaredStateProofDraft::NativeDefault {
                definition_id: e_id,
                outcome: e_outcome,
            },
        ) => a_id == e_id && a_outcome == e_outcome,
        (
            AgentAssetDeclaredStateProofDraft::Overlay {
                scope: a_scope,
                outcome: a_outcome,
                ..
            },
            AgentAssetDeclaredStateProofDraft::Overlay {
                scope: e_scope,
                outcome: e_outcome,
                ..
            },
        ) => a_scope == e_scope && a_outcome == e_outcome,
        (
            AgentAssetDeclaredStateProofDraft::Policy {
                outcome: actual, ..
            },
            AgentAssetDeclaredStateProofDraft::Policy {
                outcome: expected, ..
            },
        ) => actual == expected,
        (
            AgentAssetDeclaredStateProofDraft::Unknown { cause: actual, .. },
            AgentAssetDeclaredStateProofDraft::Unknown {
                cause: expected, ..
            },
        ) => actual == expected,
        _ => false,
    }
}

impl<'a> EvidenceIndex<'a> {
    pub(super) fn validate_native_assessment(
        &self,
        item: &ValidatedDraft,
        target: &AgentAssetAssessmentTarget,
        assessment: &AgentAssetNativeAssessment,
        projection: &AgentAssetProjectionAssessment,
    ) -> Option<&'a ParsedAgentAsset> {
        let trust_suppressed = is_trust_suppressed_exception(
            &item.draft,
            &item.represented,
            &item.contributors,
            &item.inspection,
        );
        let anchor = self.declaration(&projection.anchor_declaration_id)?;
        if anchor.role != AgentAssetDeclarationRole::Definition
            || anchor.category != target.category
            || anchor.resolution_group_key != target.resolution_group_key
            || anchor.native_id != target.exact_native_id
            || !item
                .draft
                .represented_declaration_ids
                .contains(&anchor.declaration_id)
            || (!trust_suppressed
                && (anchor.participation != AgentAssetResolutionParticipation::Participates
                    || !item.draft.contributor_ids.contains(&anchor.declaration_id)))
            || (trust_suppressed
                && !matches!(
                    anchor.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                    }
                ))
            || matches!(&target.subject, AgentAssetAssessmentSubject::Definition(id) if *id != anchor.declaration_id)
            || item.inspection.native_source_key != anchor.source_key
            || item.draft.label != anchor.label
            || item.draft.resolution.relation != projection.relation
            || item.draft.provided_by != projection.relationships.provided_by
            || item.draft.action_owner != projection.relationships.action_owner
            || item.draft.explicitly_affected != projection.relationships.explicitly_affected
            || !effective_structure_matches(
                &item.draft.state_proof.effective,
                &projection.effective,
            )
        {
            return None;
        }
        if assessment.declared_state != item.draft.declared_state
            || !proof_decision_matches(&item.draft.state_proof.declared, &assessment.declared)
            || details_category(&assessment.intrinsic.details) != target.category
            || details_declared_state(&assessment.intrinsic.details) != assessment.declared_state
            || !intrinsic_basis_is_consistent(&assessment.intrinsic)
            || assessment.intrinsic.trust_state != item.draft.trust_state
        {
            return None;
        }
        let members = self.members(
            &assessment.declared_members,
            target,
            item,
            true,
            trust_suppressed,
        )?;
        let expected_ids = members.keys().cloned().collect::<BTreeSet<_>>();
        if self.expand(&proof_evidence(&item.draft.state_proof.declared), &members)
            != Some(expected_ids.clone())
            || self.expand(&proof_evidence(&assessment.declared), &members) != Some(expected_ids)
            || !validate_declared_shape(item, assessment, &members, trust_suppressed)
        {
            return None;
        }
        // The structural group pass cannot authorize cross-exact overlays. Every
        // such contributor must have an independently validated group scope.
        if item.contributors.iter().any(|declaration| {
            declaration.native_id != target.exact_native_id
                && !assessment.declared_members.iter().any(|member| {
                    member.declaration_id == declaration.declaration_id
                        && matches!(
                            member.kind,
                            AgentAssetEvidenceKind::Overlay {
                                scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup
                            } | AgentAssetEvidenceKind::InvalidStateControl {
                                scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup
                            }
                        )
                })
        }) {
            return None;
        }
        let valid_control = match (&assessment.control, &item.draft.state_proof.effective) {
            (
                Some(control),
                AgentAssetEffectiveStateProofDraft::Terminal {
                    cause, evidence, ..
                },
            ) if *cause == control.cause => {
                let AgentAssetEffectiveStateProofDraft::Terminal {
                    evidence: native_evidence,
                    ..
                } = &projection.effective
                else {
                    return None;
                };
                validate_control(self, item, target, control, evidence)
                    && validate_control(self, item, target, control, native_evidence)
            }
            (
                None,
                AgentAssetEffectiveStateProofDraft::Terminal {
                    cause:
                        AgentAssetTerminalCauseDraft::TypedPolicy
                        | AgentAssetTerminalCauseDraft::InvalidControl,
                    ..
                },
            ) => false,
            (None, _) => true,
            _ => false,
        };
        valid_control.then_some(anchor)
    }
}

/// Terminal references may use equivalent Source/Declaration forms; their
/// complete expanded sets are checked independently of this native route.
fn effective_structure_matches(
    actual: &AgentAssetEffectiveStateProofDraft,
    expected: &AgentAssetEffectiveStateProofDraft,
) -> bool {
    use AgentAssetEffectiveStateProofDraft as Proof;
    match (actual, expected) {
        (Proof::Intrinsic, Proof::Intrinsic) => true,
        (
            Proof::ParentGate {
                parent: actual_parent,
                input: actual_input,
            },
            Proof::ParentGate {
                parent: expected_parent,
                input: expected_input,
            },
        ) => {
            actual_parent == expected_parent
                && effective_structure_matches(actual_input, expected_input)
        }
        (
            Proof::Terminal {
                terminal: actual_terminal,
                cause: actual_cause,
                input: actual_input,
                ..
            },
            Proof::Terminal {
                terminal: expected_terminal,
                cause: expected_cause,
                input: expected_input,
                ..
            },
        ) => {
            actual_terminal == expected_terminal
                && actual_cause == expected_cause
                && effective_structure_matches(actual_input, expected_input)
        }
        (
            Proof::Shadowed {
                winner: actual_winner,
                input: actual_input,
            },
            Proof::Shadowed {
                winner: expected_winner,
                input: expected_input,
            },
        ) => {
            actual_winner == expected_winner
                && effective_structure_matches(actual_input, expected_input)
        }
        _ => false,
    }
}

fn validate_declared_shape(
    item: &ValidatedDraft,
    assessment: &AgentAssetNativeAssessment,
    declarations: &BTreeMap<String, &ParsedAgentAsset>,
    trust_suppressed: bool,
) -> bool {
    let members = &assessment.declared_members;
    let all_definitions = members
        .iter()
        .all(|member| member.kind == AgentAssetEvidenceKind::Definition);
    match &item.draft.state_proof.declared {
        AgentAssetDeclaredStateProofDraft::Definition { selected_id, .. } => {
            all_definitions
                && !trust_suppressed
                && item.draft.declared_state != AgentAssetDeclaredState::Unknown
                && declarations
                    .get(selected_id)
                    .is_some_and(|selected| selected.declared_state == item.draft.declared_state)
        }
        AgentAssetDeclaredStateProofDraft::NativeDefault {
            definition_id,
            outcome,
        } => {
            all_definitions
                && members.len() == 1
                && !trust_suppressed
                && matches!(
                    outcome,
                    AgentAssetDeclaredState::Enabled | AgentAssetDeclaredState::Disabled
                )
                && *outcome == item.draft.declared_state
                && declarations.get(definition_id).is_some_and(|definition| {
                    definition.declared_state == AgentAssetDeclaredState::Unknown
                })
        }
        AgentAssetDeclaredStateProofDraft::Overlay { scope, outcome, .. } => {
            !trust_suppressed
                && *outcome == item.draft.declared_state
                && *outcome != AgentAssetDeclaredState::Unknown
                && members
                    .iter()
                    .all(|member| member.kind == AgentAssetEvidenceKind::Overlay { scope: *scope })
                && declarations
                    .values()
                    .all(|declaration| declaration.declared_state == *outcome)
        }
        AgentAssetDeclaredStateProofDraft::Policy { outcome, .. } => {
            !trust_suppressed
                && *outcome != AgentAssetDeclaredState::Unknown
                && *outcome == item.draft.declared_state
                && members
                    .iter()
                    .all(|member| member.kind == AgentAssetEvidenceKind::Policy)
        }
        AgentAssetDeclaredStateProofDraft::Unknown { cause, .. } => {
            if item.draft.declared_state != AgentAssetDeclaredState::Unknown {
                return false;
            }
            if trust_suppressed {
                let represented_definitions = item
                    .represented
                    .iter()
                    .filter(|declaration| declaration.role == AgentAssetDeclarationRole::Definition)
                    .map(|declaration| declaration.declaration_id.as_str())
                    .collect::<BTreeSet<_>>();
                return *cause == AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown
                    && all_definitions
                    && represented_definitions
                        == declarations.keys().map(String::as_str).collect();
            }
            match cause {
                AgentAssetDeclaredUnknownCauseDraft::StructuralConflict => {
                    all_definitions
                        && members.len() >= 2
                        && item.draft.resolution.relation == AgentAssetResolutionRelation::Unknown
                        && item.draft.resolution.winner.is_none()
                }
                AgentAssetDeclaredUnknownCauseDraft::OverlayConflict => {
                    members.len() >= 2
                        && members.iter().all(|member| {
                            matches!(member.kind, AgentAssetEvidenceKind::Overlay { .. })
                        })
                        && declarations
                            .values()
                            .map(|declaration| declaration.logical_origin.precedence)
                            .collect::<BTreeSet<_>>()
                            .len()
                            == 1
                        && declarations.values().all(|declaration| {
                            declaration.declared_state != AgentAssetDeclaredState::Unknown
                        })
                        && declarations.values().any(|declaration| {
                            declarations.values().next().is_some_and(|first| {
                                declaration.declared_state != first.declared_state
                            })
                        })
                }
                AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl => {
                    members.iter().any(|member| {
                        matches!(
                            member.kind,
                            AgentAssetEvidenceKind::InvalidStateControl { .. }
                        )
                    }) && members.iter().all(|member| {
                        matches!(
                            member.kind,
                            AgentAssetEvidenceKind::Overlay { .. }
                                | AgentAssetEvidenceKind::InvalidStateControl { .. }
                        )
                    })
                }
                AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge => {
                    members
                        .iter()
                        .any(|member| member.kind == AgentAssetEvidenceKind::Definition)
                        && members.iter().all(|member| {
                            matches!(
                                member.kind,
                                AgentAssetEvidenceKind::Definition
                                    | AgentAssetEvidenceKind::Overlay { .. }
                                    | AgentAssetEvidenceKind::InvalidStateControl { .. }
                            )
                        })
                }
                AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown => {
                    all_definitions
                        && declarations.values().all(|declaration| {
                            declaration.declared_state == AgentAssetDeclaredState::Unknown
                        })
                }
                AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable => {
                    members
                        .iter()
                        .all(|member| matches!(member.kind, AgentAssetEvidenceKind::Overlay { .. }))
                        && declarations.values().any(|declaration| {
                            declaration.declared_state == AgentAssetDeclaredState::Unknown
                        })
                }
            }
        }
    }
}

fn validate_control(
    index: &EvidenceIndex<'_>,
    item: &ValidatedDraft,
    target: &AgentAssetAssessmentTarget,
    control: &AgentAssetControlAssessment,
    evidence: &[AgentAssetStateEvidenceRefDraft],
) -> bool {
    let Some(members) = index.members(&control.members, target, item, false, false) else {
        return false;
    };
    if index.expand(evidence, &members) != Some(members.keys().cloned().collect()) {
        return false;
    }
    match control.cause {
        AgentAssetTerminalCauseDraft::TypedPolicy => {
            if item.draft.resolution.terminal != Some(AgentAssetResolutionTerminal::PolicyBlocked)
                || control
                    .members
                    .iter()
                    .any(|member| member.kind != AgentAssetEvidenceKind::Policy)
                || control.authorities.is_empty()
            {
                return false;
            }
        }
        AgentAssetTerminalCauseDraft::InvalidControl => {
            if item.draft.resolution.terminal != Some(AgentAssetResolutionTerminal::Unknown)
                || !control
                    .members
                    .iter()
                    .any(|member| member.kind == AgentAssetEvidenceKind::InvalidControl)
                || control
                    .members
                    .iter()
                    .any(|member| member.kind == AgentAssetEvidenceKind::Definition)
            {
                return false;
            }
        }
        _ => return false,
    }
    for authority in &control.authorities {
        match authority {
            AgentAssetControlAuthority::Declaration(id) if members.contains_key(id) => {}
            AgentAssetControlAuthority::SourceAggregate(key)
                if source_for_key(index.sources, key).is_some()
                    && members
                        .values()
                        .any(|declaration| declaration.source_key == *key) => {}
            _ => return false,
        }
    }
    match (
        control.authorities.len(),
        item.draft.resolution.control_source.as_ref(),
    ) {
        (1, Some(reference)) => match (control.authorities.first(), reference) {
            (
                Some(AgentAssetControlAuthority::Declaration(expected)),
                AgentAssetPolicyReferenceDraft::Declaration { declaration_id },
            ) => expected == declaration_id,
            (
                Some(AgentAssetControlAuthority::SourceAggregate(expected)),
                AgentAssetPolicyReferenceDraft::Source { source_key },
            ) => {
                expected == source_key
                    && members
                        .values()
                        .all(|declaration| declaration.source_key == *source_key)
            }
            _ => false,
        },
        (0, None) => control.cause == AgentAssetTerminalCauseDraft::InvalidControl,
        (2.., None) => true,
        _ => false,
    }
}

pub(super) fn validate_effective_state_source(
    index: usize,
    validated: &[ValidatedDraft],
    intrinsic_by_index: &BTreeMap<usize, AgentAssetIntrinsicBasis>,
    by_projection: &ProjectionKeyIndex,
    by_native: &NativeAssetIndex,
) -> bool {
    evaluate_effective(
        index,
        validated,
        intrinsic_by_index,
        by_projection,
        by_native,
        &mut BTreeSet::new(),
    )
    .is_some()
}

fn evaluate_effective(
    index: usize,
    validated: &[ValidatedDraft],
    intrinsic_by_index: &BTreeMap<usize, AgentAssetIntrinsicBasis>,
    by_projection: &ProjectionKeyIndex,
    by_native: &NativeAssetIndex,
    visiting: &mut BTreeSet<usize>,
) -> Option<AgentAssetState> {
    if !visiting.insert(index) {
        return None;
    }
    let item = &validated[index];
    let draft = &item.draft;
    let basis = intrinsic_by_index.get(&index)?;
    if !super::super::adapter_support::effective_proof_shape_is_valid(
        &draft.state_proof.effective,
        &draft.resolution,
        draft.provided_by.as_ref(),
    ) {
        return None;
    }
    let parent_state = if let Some(parent) = parent_gate_reference(&draft.state_proof.effective) {
        if !matches!(
            parent.category,
            AgentAssetCategory::Plugin | AgentAssetCategory::Extension
        ) || draft.provided_by.as_ref() != Some(parent)
        {
            return None;
        }
        let parent_index = resolve_native_ref_index(parent, validated, by_projection, by_native)?;
        let state = evaluate_effective(
            parent_index,
            validated,
            intrinsic_by_index,
            by_projection,
            by_native,
            visiting,
        )?;
        if !matches!(
            state,
            AgentAssetState::Enabled
                | AgentAssetState::Disabled
                | AgentAssetState::NotInstalled
                | AgentAssetState::Unknown
                | AgentAssetState::Blocked
        ) {
            return None;
        }
        Some(state)
    } else {
        None
    };
    let valid = match &draft.state_proof.effective {
        AgentAssetEffectiveStateProofDraft::Intrinsic
        | AgentAssetEffectiveStateProofDraft::ParentGate { .. } => true,
        AgentAssetEffectiveStateProofDraft::Shadowed { winner, .. } => {
            let winner_index =
                resolve_native_ref_index(winner, validated, by_projection, by_native)?;
            let winner_draft = &validated[winner_index].draft;
            winner_index != index
                && winner_draft.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                && winner_draft.native_kind == draft.native_kind
                && winner_draft.native_id == draft.native_id
                && winner_draft.resolution_group_key == draft.resolution_group_key
        }
        AgentAssetEffectiveStateProofDraft::Terminal {
            terminal, cause, ..
        } => match cause {
            AgentAssetTerminalCauseDraft::TypedPolicy => {
                *terminal == AgentAssetResolutionTerminal::PolicyBlocked
            }
            AgentAssetTerminalCauseDraft::InvalidControl => {
                *terminal == AgentAssetResolutionTerminal::Unknown
            }
            AgentAssetTerminalCauseDraft::ParentUnknown => matches!(
                parent_state,
                Some(AgentAssetState::Unknown | AgentAssetState::Blocked)
            ),
            AgentAssetTerminalCauseDraft::DeclaredUnknown => matches!(
                draft.state_proof.declared,
                AgentAssetDeclaredStateProofDraft::Unknown {
                    cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown
                        | AgentAssetDeclaredUnknownCauseDraft::OverlayConflict
                        | AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                    ..
                }
            ),
            AgentAssetTerminalCauseDraft::StructuralUnknown => {
                draft.resolution.relation == AgentAssetResolutionRelation::Unknown
                    && matches!(
                        draft.state_proof.declared,
                        AgentAssetDeclaredStateProofDraft::Unknown {
                            cause: AgentAssetDeclaredUnknownCauseDraft::StructuralConflict,
                            ..
                        }
                    )
            }
            AgentAssetTerminalCauseDraft::TrustSuppressed => {
                is_trust_suppressed_exception(
                    draft,
                    &item.represented,
                    &item.contributors,
                    &item.inspection,
                ) && matches!(
                    draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::Unknown {
                        cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
                        ..
                    }
                )
            }
        },
    };
    let (state, details) =
        compose_effective_state(basis, &draft.state_proof.effective, parent_state)?;
    visiting.remove(&index);
    (valid
        && draft.effective_state == state
        && draft.details == details
        && draft.trust_state == basis.trust_state)
        .then_some(state)
}

pub(super) fn parent_gate_reference(
    proof: &AgentAssetEffectiveStateProofDraft,
) -> Option<&AgentAssetNativeRef> {
    match proof {
        AgentAssetEffectiveStateProofDraft::ParentGate { parent, .. } => Some(parent),
        AgentAssetEffectiveStateProofDraft::Terminal { input, .. } => parent_gate_reference(input),
        _ => None,
    }
}
