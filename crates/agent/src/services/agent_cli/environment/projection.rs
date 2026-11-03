//! Common projection of adapter-owned declarations into the public asset model.
//!
//! Adapters decide native meaning and resolution.  This module only validates
//! their drafts, assigns stable IDs, resolves context-local references and
//! materializes the read-only action surface.  It must not inspect JSON/TOML
//! keys or infer precedence semantics.

mod state;

#[cfg(test)]
mod fixture;

#[cfg(test)]
pub(in crate::services::agent_cli) use fixture::{fixture_assessor, fixture_draft};

use state::{
    assessment_target, parent_gate_reference, validate_effective_state_source, EvidenceIndex,
};

use super::{
    adapter_support::state_from_declared,
    diagnostics::DiagnosticOwner,
    identity::{source_stable_id, stable_id},
    run::AgentInventoryRun,
    snapshot::revision_for_missing,
    state_projection::{expected_mcp_availability, state_from_availability},
};
use crate::{
    models::{
        AgentAssetAction, AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState,
        AgentAssetDetails, AgentAssetDiagnostic, AgentAssetEffectiveAvailability,
        AgentAssetNativeRef, AgentAssetPolicyReference, AgentAssetRecord, AgentAssetRelationKind,
        AgentAssetResolution, AgentAssetResolutionParticipation, AgentAssetResolutionRelation,
        AgentAssetSource, AgentAssetSourceKind, AgentAssetState, AgentConfigurationContext,
        AgentEnvironmentDescriptor, AgentStatusUiMode, AgentTrustState,
    },
    services::agent_cli::contracts::{
        AgentAssetAssessmentFailure, AgentAssetAssessmentRequest, AgentAssetAssessmentResult,
        AgentAssetDeclaredStateProofDraft, AgentAssetEffectiveStateProofDraft,
        AgentAssetIntrinsicBasis, AgentAssetPolicyReferenceDraft, AgentAssetProjectedDraft,
        AgentAssetSourceSpec, AgentAssetStateAssessor, AgentAssetStateEvidenceRefDraft,
        AgentAssetTerminalCauseDraft, ParsedAgentAsset,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub(crate) struct AgentAssetProjectionResult {
    pub records: Vec<AgentAssetRecord>,
}

#[derive(Debug)]
pub(crate) struct AgentAssetProjectionInput {
    pub drafts: Vec<AgentAssetProjectedDraft>,
    pub state_assessor: AgentAssetStateAssessor,
}

#[derive(Debug)]
struct ValidatedDraft {
    draft: AgentAssetProjectedDraft,
    represented: Vec<ParsedAgentAsset>,
    contributors: Vec<ParsedAgentAsset>,
    contributor_sources: Vec<AgentAssetSourceSpec>,
    inspection: AgentAssetSourceSpec,
    source_ids: Vec<String>,
    stable_id: String,
}

#[derive(Debug, Clone, Copy)]
enum DraftValidationError {
    Projection,
    Resolution,
}

type ProjectionKeyIndex = BTreeMap<String, usize>;
type NativeAssetIndex = BTreeMap<(AgentAssetCategory, String), Vec<usize>>;

struct RelationshipResolutionRequest<'a> {
    reference: Option<&'a AgentAssetNativeRef>,
    relation: AgentAssetRelationKind,
    item: &'a ValidatedDraft,
    validated: &'a [ValidatedDraft],
    by_projection: &'a ProjectionKeyIndex,
    by_native: &'a NativeAssetIndex,
    record: &'a mut AgentAssetRecord,
    run: &'a mut AgentInventoryRun,
}

/// Validate and project one adapter result.
pub(in crate::services::agent_cli) fn project_records(
    environment: &AgentEnvironmentDescriptor,
    context: &AgentConfigurationContext,
    sources: &[AgentAssetSourceSpec],
    declarations: &[ParsedAgentAsset],
    result: AgentAssetProjectionInput,
    source_map: &BTreeMap<String, AgentAssetSource>,
    run: &mut AgentInventoryRun,
) -> AgentAssetProjectionResult {
    let mut output = AgentAssetProjectionResult {
        records: Vec::new(),
    };

    // Declaration IDs are adapter-produced but common validation owns their
    // uniqueness.  A duplicate ID cannot be safely resolved by choosing one.
    let mut declarations_by_id = BTreeMap::<String, ParsedAgentAsset>::new();
    let mut duplicate_declarations = BTreeSet::new();
    let mut declaration_keys = BTreeMap::<(String, AgentAssetCategory, String), String>::new();
    for declaration in declarations {
        let source_valid = source_for_key(sources, &declaration.source_key).is_some_and(|source| {
            super::adapter_support::declaration_matches_source(context, declaration, source)
        });
        if !source_valid {
            duplicate_declarations.insert(declaration.declaration_id.clone());
            run.emit(
                DiagnosticOwner::Declaration(declaration.declaration_id.clone()),
                invalid_projection(&declaration.declaration_id),
            );
        }
        if declaration.declaration_id.trim().is_empty()
            || declaration.declaration_key.trim().is_empty()
            || declaration.declaration_key.chars().any(char::is_control)
        {
            duplicate_declarations.insert(declaration.declaration_id.clone());
        }
        if let Some(previous) =
            declarations_by_id.insert(declaration.declaration_id.clone(), declaration.clone())
        {
            let previous_id = previous.declaration_id.clone();
            duplicate_declarations.insert(previous_id.clone());
            duplicate_declarations.insert(declaration.declaration_id.clone());
            run.emit(
                DiagnosticOwner::Declaration(previous_id.clone()),
                invalid_projection(&previous_id),
            );
            run.emit(
                DiagnosticOwner::Declaration(declaration.declaration_id.clone()),
                invalid_projection(&declaration.declaration_id),
            );
        }
        let identity_key = (
            declaration.source_key.clone(),
            declaration.category,
            declaration.declaration_key.clone(),
        );
        if let Some(previous_id) =
            declaration_keys.insert(identity_key, declaration.declaration_id.clone())
        {
            duplicate_declarations.insert(previous_id);
            duplicate_declarations.insert(declaration.declaration_id.clone());
            run.emit(
                DiagnosticOwner::Declaration(declaration.declaration_id.clone()),
                invalid_projection(&declaration.declaration_id),
            );
        }
    }
    // Suppression belongs to the declaration even when its resolution group
    // intentionally produces no projected record. Emit it once after the
    // declaration index has established that the parsed declaration is valid.
    for declaration in declarations {
        if duplicate_declarations.contains(&declaration.declaration_id) {
            continue;
        }
        if let AgentAssetResolutionParticipation::Suppressed { reason } = declaration.participation
        {
            run.emit(
                DiagnosticOwner::Declaration(declaration.declaration_id.clone()),
                AgentAssetDiagnostic::DeclarationSuppressed { reason },
            );
        }
    }

    let state_assessor = result.state_assessor;

    let projection_key_counts =
        result
            .drafts
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, draft| {
                *counts.entry(draft.projection_key.clone()).or_default() += 1;
                counts
            });
    let duplicate_projection_keys = projection_key_counts
        .iter()
        .filter_map(|(key, count)| (*count > 1).then_some(key.clone()))
        .collect::<BTreeSet<_>>();
    for key in &duplicate_projection_keys {
        run.emit(
            DiagnosticOwner::Context(context.id.clone()),
            invalid_projection(key),
        );
    }

    let mut validated = Vec::new();
    for draft in result.drafts {
        let key = draft.projection_key.clone();
        if duplicate_projection_keys.contains(&key) {
            continue;
        }
        match validate_draft(
            &draft,
            context,
            sources,
            &declarations_by_id,
            &duplicate_declarations,
        ) {
            Ok(candidate) => {
                if candidate.draft.resolution.qualified_collision {
                    run.emit(
                        DiagnosticOwner::Resolution(candidate.stable_id.clone()),
                        AgentAssetDiagnostic::DuplicateNativeId {
                            category: candidate.draft.native_kind,
                            native_id: candidate.draft.native_id.clone(),
                        },
                    );
                }
                if candidate.draft.resolution.terminal
                    == Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked)
                {
                    run.emit(
                        DiagnosticOwner::Resolution(candidate.stable_id.clone()),
                        AgentAssetDiagnostic::PolicyBlocked,
                    );
                }
                validated.push(candidate);
            }
            Err(DraftValidationError::Projection) => {
                run.emit(
                    DiagnosticOwner::Context(context.id.clone()),
                    invalid_projection(&key),
                );
            }
            Err(DraftValidationError::Resolution) => {
                run.emit(
                    DiagnosticOwner::Context(context.id.clone()),
                    invalid_resolution(&key, draft.resolution.relation),
                );
            }
        }
    }

    let mut rejected = validate_resolution_groups(
        declarations,
        &duplicate_declarations,
        &validated,
        &context.id,
        run,
    );

    let targets_by_index = validated
        .iter()
        .enumerate()
        .filter(|(index, _)| !rejected.contains(index))
        .filter_map(|(index, item)| assessment_target(item).map(|target| (index, target)))
        .collect::<BTreeMap<_, _>>();
    let targets = targets_by_index
        .values()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let assessments = state_assessor(AgentAssetAssessmentRequest {
        context,
        targets: &targets,
        declarations,
        sources,
    });
    let evidence_index = EvidenceIndex::new(
        context,
        sources,
        &declarations_by_id,
        &duplicate_declarations,
    );
    let mut intrinsic_by_index = BTreeMap::<usize, AgentAssetIntrinsicBasis>::new();
    let mut anchors_by_index = BTreeMap::<usize, &ParsedAgentAsset>::new();
    for (index, item) in validated.iter().enumerate() {
        if rejected.contains(&index) {
            continue;
        }
        let assessment = targets_by_index
            .get(&index)
            .and_then(|target| assessments.get(target));
        let failure = match assessment {
            Some(AgentAssetAssessmentResult::Assessed { state, projection }) => {
                if let Some(anchor) = evidence_index.validate_native_assessment(
                    item,
                    &targets_by_index[&index],
                    state,
                    projection,
                ) {
                    intrinsic_by_index.insert(index, state.intrinsic.clone());
                    anchors_by_index.insert(index, anchor);
                    None
                } else {
                    Some(AgentAssetAssessmentFailure::InvalidNativeInput)
                }
            }
            Some(AgentAssetAssessmentResult::Unsupported(failure)) => Some(*failure),
            None => Some(AgentAssetAssessmentFailure::MissingTarget),
        };
        if let Some(failure) = failure {
            rejected.insert(index);
            run.emit(
                DiagnosticOwner::Resolution(item.stable_id.clone()),
                invalid_resolution(
                    &format!("{}:assessment:{failure:?}", item.draft.projection_key),
                    item.draft.resolution.relation,
                ),
            );
        }
    }

    // Graph proofs only see identities whose native assessment was admitted.
    let (by_projection, by_native) = projection_indexes(&validated, Some(&rejected));
    for (index, item) in validated.iter().enumerate() {
        if rejected.contains(&index) {
            continue;
        }
        if !validate_effective_state_source(
            index,
            &validated,
            &intrinsic_by_index,
            &by_projection,
            &by_native,
        ) {
            rejected.insert(index);
            run.emit(
                DiagnosticOwner::Resolution(item.stable_id.clone()),
                invalid_resolution(&item.draft.projection_key, item.draft.resolution.relation),
            );
        }
    }
    for (index, item) in validated.iter().enumerate() {
        if rejected.contains(&index) {
            continue;
        }
        let resolution = &item.draft.resolution;
        let winner = resolution.winner.as_ref().and_then(|reference| {
            resolve_native_ref_index(reference, &validated, &by_projection, &by_native)
        });
        // Terminal applicability, complete evidence and exact authority were
        // already validated against the independent assessment.
        let valid = match resolution.relation {
            AgentAssetResolutionRelation::ReplaceWinner => winner == Some(index),
            AgentAssetResolutionRelation::Replaced => winner.is_some_and(|winner_index| {
                winner_index != index
                    && validated[winner_index].draft.resolution.relation
                        == AgentAssetResolutionRelation::ReplaceWinner
                    && validated[winner_index].draft.native_kind == item.draft.native_kind
                    && validated[winner_index].draft.native_id == item.draft.native_id
            }),
            AgentAssetResolutionRelation::Unknown => winner.is_none(),
            _ => winner.is_none(),
        };
        if !valid {
            rejected.insert(index);
            run.emit(
                DiagnosticOwner::Resolution(item.stable_id.clone()),
                invalid_resolution(&item.draft.projection_key, resolution.relation),
            );
        }
    }

    // Rejection is transitive for state and required ownership references.
    // An earlier intrinsic gate (for example MCP trust) can make ParentGate
    // unnecessary without making the independently proved provider optional.
    loop {
        let mut changed = false;
        for (index, item) in validated.iter().enumerate() {
            if rejected.contains(&index) {
                continue;
            }
            let state_reference_rejected = item
                .draft
                .resolution
                .winner
                .as_ref()
                .into_iter()
                .chain(parent_gate_reference(&item.draft.state_proof.effective))
                .any(|reference| {
                    resolve_native_ref_index(reference, &validated, &by_projection, &by_native)
                        .is_none_or(|target| rejected.contains(&target))
                });
            let ownership_reference_rejected = item
                .draft
                .provided_by
                .as_ref()
                .into_iter()
                .chain(item.draft.action_owner.as_ref())
                .any(|reference| {
                    resolve_native_ref_index(reference, &validated, &by_projection, &by_native)
                        .is_none_or(|target| target == index || rejected.contains(&target))
                });
            if state_reference_rejected || ownership_reference_rejected {
                rejected.insert(index);
                run.emit(
                    DiagnosticOwner::Resolution(item.stable_id.clone()),
                    invalid_resolution(&item.draft.projection_key, item.draft.resolution.relation),
                );
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let (final_by_projection, final_by_native) = projection_indexes(&validated, Some(&rejected));
    let mut records = Vec::new();
    for (index, item) in validated.iter().enumerate() {
        if rejected.contains(&index) {
            continue;
        }
        let Some(anchor) = anchors_by_index.get(&index) else {
            run.emit(
                DiagnosticOwner::Resolution(item.stable_id.clone()),
                invalid_projection(&item.draft.projection_key),
            );
            continue;
        };
        let mut record = materialize_record(environment, context, item, anchor, source_map);
        record.resolution.winner_id = item
            .draft
            .resolution
            .winner
            .as_ref()
            .and_then(|reference| {
                resolve_native_ref_index(
                    reference,
                    &validated,
                    &final_by_projection,
                    &final_by_native,
                )
            })
            .map(|target| validated[target].stable_id.clone());
        record.resolution.control_source =
            item.draft
                .resolution
                .control_source
                .as_ref()
                .map(|reference| match reference {
                    AgentAssetPolicyReferenceDraft::Declaration { declaration_id } => {
                        AgentAssetPolicyReference::Declaration {
                            declaration_id: declaration_id.clone(),
                        }
                    }
                    AgentAssetPolicyReferenceDraft::Source { source_key } => {
                        AgentAssetPolicyReference::Source {
                            source_id: source_for_key(sources, source_key)
                                .map(|source| source_stable_id(context, &source.path))
                                .unwrap_or_else(|| source_key.clone()),
                        }
                    }
                });

        resolve_relationship(RelationshipResolutionRequest {
            reference: item.draft.provided_by.as_ref(),
            relation: AgentAssetRelationKind::ProvidedBy,
            item,
            validated: &validated,
            by_projection: &final_by_projection,
            by_native: &final_by_native,
            record: &mut record,
            run,
        });
        resolve_relationship(RelationshipResolutionRequest {
            reference: item.draft.action_owner.as_ref(),
            relation: AgentAssetRelationKind::ActionOwner,
            item,
            validated: &validated,
            by_projection: &final_by_projection,
            by_native: &final_by_native,
            record: &mut record,
            run,
        });
        // Whole-entry replacement keeps the complete bucket as structural
        // evidence, but only its native anchor supplies the resulting asset.
        // Merged/uncertain buckets retain all possible contributing origins.
        let definitions = match item.draft.resolution.relation {
            AgentAssetResolutionRelation::Merged | AgentAssetResolutionRelation::Unknown
                if !item.contributors.is_empty() =>
            {
                item.contributors
                    .iter()
                    .filter(|value| value.role == AgentAssetDeclarationRole::Definition)
                    .collect()
            }
            _ => vec![*anchor],
        };
        record.provenance = definitions
            .into_iter()
            .filter_map(|declaration| {
                let source = source_for_key(sources, &declaration.source_key)?;
                Some(super::provenance::definition_provenance(
                    source_stable_id(context, &source.path),
                    declaration.declaration_id.clone(),
                    declaration.logical_origin.scope,
                    source.origin,
                    source.provider,
                    declaration.provided_by.is_some(),
                    declaration.provided_by.is_some() && record.relationships.provided_by.is_some(),
                ))
            })
            .collect();
        record.provenance.sort();
        record.provenance.dedup();
        for reference in &item.draft.explicitly_affected {
            match resolve_native_ref_index(
                reference,
                &validated,
                &final_by_projection,
                &final_by_native,
            ) {
                Some(target) if validated[target].stable_id != record.stable_id => {
                    record
                        .relationships
                        .affected_asset_ids
                        .push(validated[target].stable_id.clone());
                }
                Some(_) | None => {
                    let diagnostic =
                        unresolved_relationship(AgentAssetRelationKind::ExplicitImpact, reference);
                    run.emit(DiagnosticOwner::Record(item.stable_id.clone()), diagnostic);
                }
            }
        }
        record.relationships.affected_asset_ids.sort();
        record.relationships.affected_asset_ids.dedup();
        record.diagnostics =
            run.take_diagnostics_for(DiagnosticOwner::Record(item.stable_id.clone()));
        record.resolution.diagnostics =
            run.take_diagnostics_for(DiagnosticOwner::Resolution(item.stable_id.clone()));
        records.push(record);
    }

    // The reverse impact relation is derived only from resolved stable IDs and
    // is deterministic even when provider and action owner differ.
    let index_by_id = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.stable_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let reverse_edges = records
        .iter()
        .flat_map(|child| {
            child
                .relationships
                .provided_by
                .iter()
                .chain(child.relationships.action_owner.iter())
                .map(move |parent| (parent.clone(), child.stable_id.clone()))
        })
        .collect::<Vec<_>>();
    for (parent, child) in reverse_edges {
        if let Some(index) = index_by_id.get(&parent) {
            let affected = &mut records[*index].relationships.affected_asset_ids;
            affected.push(child);
            affected.sort();
            affected.dedup();
        }
    }

    output.records = records;
    output
        .records
        .sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    output
}

fn validate_draft(
    draft: &AgentAssetProjectedDraft,
    context: &AgentConfigurationContext,
    sources: &[AgentAssetSourceSpec],
    declarations: &BTreeMap<String, ParsedAgentAsset>,
    duplicate_declarations: &BTreeSet<String>,
) -> Result<ValidatedDraft, DraftValidationError> {
    if draft.projection_key.trim().is_empty()
        || draft.resolution_group_key.trim().is_empty()
        || draft.native_id.trim().is_empty()
    {
        return Err(DraftValidationError::Projection);
    }
    if details_category(&draft.details) != draft.native_kind {
        return Err(DraftValidationError::Projection);
    }
    if !details_are_consistent(draft) {
        return Err(DraftValidationError::Resolution);
    }

    if draft.represented_declaration_ids.is_empty()
        || has_duplicates(&draft.represented_declaration_ids)
    {
        return Err(DraftValidationError::Projection);
    }

    let expected_definition_count = match draft.resolution.relation {
        AgentAssetResolutionRelation::Independent
        | AgentAssetResolutionRelation::Replaced
        | AgentAssetResolutionRelation::Additive => Some(1),
        AgentAssetResolutionRelation::ReplaceWinner | AgentAssetResolutionRelation::Merged => None,
        // An Unknown relation can either be an unresolved exact winner or a
        // terminal availability result.  The exact bucket validator below
        // checks the structural cardinality after all declarations are known.
        AgentAssetResolutionRelation::Unknown => None,
    };
    if has_duplicates(&draft.contributor_ids) {
        return Err(DraftValidationError::Resolution);
    }
    match draft.resolution.relation {
        AgentAssetResolutionRelation::Independent
        | AgentAssetResolutionRelation::Additive
        | AgentAssetResolutionRelation::Unknown => {
            if draft.resolution.winner.is_some()
                || (draft.resolution.terminal.is_none()
                    && draft.resolution.control_source.is_some())
            {
                return Err(DraftValidationError::Resolution);
            }
        }
        AgentAssetResolutionRelation::ReplaceWinner => {
            if draft.resolution.winner.is_none()
                || (draft.resolution.terminal.is_none()
                    && draft.resolution.control_source.is_some())
            {
                return Err(DraftValidationError::Resolution);
            }
        }
        AgentAssetResolutionRelation::Replaced => {
            if draft.resolution.winner.is_none()
                || draft.resolution.control_source.is_some()
                || draft.resolution.terminal.is_some()
            {
                return Err(DraftValidationError::Resolution);
            }
        }
        AgentAssetResolutionRelation::Merged => {
            if draft.resolution.winner.is_some()
                || (draft.resolution.terminal.is_none()
                    && draft.resolution.control_source.is_some())
            {
                return Err(DraftValidationError::Resolution);
            }
        }
    }
    if draft.resolution.relation == AgentAssetResolutionRelation::Replaced
        && draft.effective_state != AgentAssetState::Shadowed
    {
        return Err(DraftValidationError::Resolution);
    }
    if draft.resolution.relation == AgentAssetResolutionRelation::Unknown
        && draft.effective_state != AgentAssetState::Unknown
        && draft.effective_state != AgentAssetState::Blocked
    {
        return Err(DraftValidationError::Resolution);
    }
    if draft.resolution.relation == AgentAssetResolutionRelation::Additive
        && draft.projection_key == format!("{}:{}", draft.native_kind.key(), draft.native_id)
    {
        return Err(DraftValidationError::Resolution);
    }

    let mut contributors = Vec::with_capacity(draft.contributor_ids.len());
    let mut contributor_sources = Vec::with_capacity(draft.contributor_ids.len());
    let mut source_ids = Vec::new();
    let mut identity_source_ids = Vec::new();
    let mut represented = Vec::with_capacity(draft.represented_declaration_ids.len());
    for declaration_id in &draft.represented_declaration_ids {
        if duplicate_declarations.contains(declaration_id) {
            return Err(DraftValidationError::Projection);
        }
        let Some(declaration) = declarations.get(declaration_id) else {
            return Err(DraftValidationError::Projection);
        };
        if declaration.category != draft.native_kind
            || declaration.resolution_group_key != draft.resolution_group_key
        {
            return Err(DraftValidationError::Projection);
        }
        let Some(source) = source_for_key(sources, &declaration.source_key) else {
            return Err(DraftValidationError::Projection);
        };
        if !super::adapter_support::declaration_matches_source(context, declaration, source) {
            return Err(DraftValidationError::Projection);
        }
        represented.push(declaration.clone());
    }
    for declaration_id in &draft.contributor_ids {
        if duplicate_declarations.contains(declaration_id) {
            return Err(DraftValidationError::Projection);
        }
        let Some(declaration) = declarations.get(declaration_id) else {
            return Err(DraftValidationError::Projection);
        };
        if declaration.category != draft.native_kind
            || declaration.resolution_group_key != draft.resolution_group_key
            || !draft.represented_declaration_ids.contains(declaration_id)
            || !matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            )
        {
            return Err(DraftValidationError::Projection);
        }
        let Some(source) = source_for_key(sources, &declaration.source_key) else {
            return Err(DraftValidationError::Projection);
        };
        if !super::adapter_support::declaration_matches_source(context, declaration, source) {
            return Err(DraftValidationError::Projection);
        }
        source_ids.push(source_stable_id(context, &source.path));
        if declaration.role == crate::models::AgentAssetDeclarationRole::Definition {
            identity_source_ids.push(source_stable_id(context, &source.path));
        }
        contributor_sources.push(source.clone());
        contributors.push(declaration.clone());
    }
    let definition_count = contributors
        .iter()
        .filter(|declaration| {
            declaration.role == crate::models::AgentAssetDeclarationRole::Definition
        })
        .count();
    if expected_definition_count.is_some_and(|count| definition_count != count)
        || (draft.contributor_ids.is_empty()
            && draft.resolution.relation != AgentAssetResolutionRelation::Unknown)
        || (matches!(
            draft.resolution.relation,
            AgentAssetResolutionRelation::ReplaceWinner | AgentAssetResolutionRelation::Merged
        ) && definition_count < 2)
    {
        return Err(DraftValidationError::Resolution);
    }
    source_ids.sort();
    source_ids.dedup();
    let Some(inspection) = source_for_reference(context, sources, &draft.inspection_source_id)
    else {
        return Err(DraftValidationError::Projection);
    };
    let represented_source_ids = represented
        .iter()
        .filter_map(|declaration| source_for_key(sources, &declaration.source_key))
        .map(|source| source_stable_id(context, &source.path))
        .collect::<Vec<_>>();
    if source_ids.is_empty() && !represented_source_ids.is_empty() {
        source_ids = represented_source_ids;
    }
    source_ids.sort();
    source_ids.dedup();
    let trust_suppressed_exception =
        is_trust_suppressed_exception(draft, &represented, &contributors, inspection);
    let inspection_definition = contributors.iter().any(|declaration| {
        declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            && declaration.source_key == inspection.native_source_key
    }) || (trust_suppressed_exception
        && represented.iter().any(|declaration| {
            declaration.role == crate::models::AgentAssetDeclarationRole::Definition
                && declaration.source_key == inspection.native_source_key
        }));
    // A participating MCP can intrinsically require trust. Its availability
    // is proved from the independently assessed declared/approval/trust basis
    // below. Only an empty contributor set needs the strict suppression path.
    if contributors.is_empty() && !trust_suppressed_exception {
        return Err(DraftValidationError::Resolution);
    }
    if trust_suppressed_exception {
        identity_source_ids.extend(
            represented
                .iter()
                .filter(|declaration| {
                    declaration.role == crate::models::AgentAssetDeclarationRole::Definition
                })
                .filter_map(|declaration| source_for_key(sources, &declaration.source_key))
                .map(|source| source_stable_id(context, &source.path)),
        );
    }
    identity_source_ids.sort();
    identity_source_ids.dedup();
    if !draft.represented_declaration_ids.iter().all(|id| {
        declarations
            .get(id)
            .is_some_and(|declaration| declaration.category == draft.native_kind)
    }) || source_ids.is_empty()
        || identity_source_ids.is_empty()
        || !inspection.categories.contains(&draft.native_kind)
        || !inspection_definition
        || (!contributors
            .iter()
            .any(|declaration| declaration.native_id == draft.native_id)
            && !represented.iter().any(|declaration| {
                declaration.native_id == draft.native_id
                    && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            }))
    {
        return Err(DraftValidationError::Projection);
    }
    if !trust_suppressed_exception
        && matches!(
            draft.resolution.relation,
            AgentAssetResolutionRelation::Independent
                | AgentAssetResolutionRelation::Additive
                | AgentAssetResolutionRelation::Unknown
                | AgentAssetResolutionRelation::Replaced
        )
    {
        let Some(contributor) = contributors.iter().find(|declaration| {
            declaration.role == crate::models::AgentAssetDeclarationRole::Definition
        }) else {
            return Err(DraftValidationError::Resolution);
        };
        // Record trust can be derived from an independent native policy after
        // parsing the Definition. The native assessment validates that value;
        // this structural pass only binds the declaration identity.
        if contributor.native_id != draft.native_id {
            return Err(DraftValidationError::Resolution);
        }
    }
    let category_key = draft.native_kind.key();
    // Controls remain public evidence and mutation guards, but changing a
    // switch does not create a new logical asset with the same definitions.
    let source_component = identity_source_ids.join("|");
    let stable = stable_id(
        "asset",
        &[
            context.id.as_str(),
            context.agent_kind.key(),
            category_key.as_str(),
            draft.projection_key.as_str(),
            source_component.as_str(),
        ],
    );
    Ok(ValidatedDraft {
        draft: draft.clone(),
        represented,
        contributors,
        contributor_sources,
        inspection: inspection.clone(),
        source_ids,
        stable_id: stable,
    })
}

/// A suppressed MCP definition is still useful inventory evidence, but it is
/// not executable until the workspace is trusted.  Keep this escape hatch
/// deliberately narrow: an empty contributor set must never become a generic
/// Unknown asset merely because the adapter omitted a winner.
fn is_trust_suppressed_exception(
    draft: &AgentAssetProjectedDraft,
    represented: &[ParsedAgentAsset],
    contributors: &[ParsedAgentAsset],
    inspection: &AgentAssetSourceSpec,
) -> bool {
    if !contributors.is_empty()
        || draft.native_kind != AgentAssetCategory::Mcp
        || draft.resolution.relation != AgentAssetResolutionRelation::Unknown
        || draft.resolution.terminal != Some(crate::models::AgentAssetResolutionTerminal::Unknown)
        || draft.resolution.winner.is_some()
        || draft.resolution.control_source.is_some()
        || !matches!(
            draft.trust_state,
            AgentTrustState::Required | AgentTrustState::Untrusted
        )
        || draft.effective_state != AgentAssetState::Unknown
    {
        return false;
    }

    let AgentAssetDetails::Mcp {
        declared_state,
        effective_availability,
        ..
    } = &draft.details
    else {
        return false;
    };
    if *declared_state != AgentAssetDeclaredState::Unknown
        || *effective_availability != AgentAssetEffectiveAvailability::TrustRequired
    {
        return false;
    }

    let definitions = represented
        .iter()
        .filter(|declaration| declaration.role == AgentAssetDeclarationRole::Definition)
        .collect::<Vec<_>>();
    !definitions.is_empty()
        && definitions.iter().all(|declaration| {
            declaration.trust_state == draft.trust_state
                && matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                    }
                )
        })
        && definitions
            .iter()
            .any(|declaration| declaration.source_key == inspection.native_source_key)
}

fn validate_resolution_groups(
    declarations: &[ParsedAgentAsset],
    duplicate_declarations: &BTreeSet<String>,
    validated: &[ValidatedDraft],
    context_id: &str,
    run: &mut AgentInventoryRun,
) -> BTreeSet<usize> {
    type GroupKey = (AgentAssetCategory, String);

    let mut declaration_groups = BTreeMap::<GroupKey, Vec<&ParsedAgentAsset>>::new();
    let mut invalid_groups = BTreeSet::<GroupKey>::new();
    for declaration in declarations {
        let key = (
            declaration.category,
            declaration.resolution_group_key.clone(),
        );
        if declaration.resolution_group_key.trim().is_empty()
            || duplicate_declarations.contains(&declaration.declaration_id)
        {
            invalid_groups.insert(key.clone());
        }
        declaration_groups.entry(key).or_default().push(declaration);
    }
    for group in declaration_groups.values_mut() {
        sort_declarations(group);
    }

    let mut draft_groups = BTreeMap::<GroupKey, Vec<usize>>::new();
    for (index, item) in validated.iter().enumerate() {
        draft_groups
            .entry((
                item.draft.native_kind,
                item.draft.resolution_group_key.clone(),
            ))
            .or_default()
            .push(index);
    }

    let group_keys = declaration_groups
        .keys()
        .chain(draft_groups.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut rejected = BTreeSet::new();
    for key in group_keys {
        let declarations = declaration_groups
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let draft_indexes = draft_groups
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let declaration_only_overlay_evidence = draft_indexes.is_empty()
            && !declarations.is_empty()
            && declarations.iter().all(|declaration| {
                declaration.role == crate::models::AgentAssetDeclarationRole::StateOverlay
                    || declaration.role == crate::models::AgentAssetDeclarationRole::PolicyOverlay
            });
        let suppressed_definitions = declarations
            .iter()
            .filter(|declaration| {
                declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            })
            .collect::<Vec<_>>();
        let declaration_only_trust_suppression = draft_indexes.is_empty()
            && !suppressed_definitions.is_empty()
            && suppressed_definitions.iter().all(|declaration| {
                matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace
                    }
                )
            })
            && (key.0 != AgentAssetCategory::Mcp
                || suppressed_definitions
                    .iter()
                    .all(|declaration| declaration.trust_state == AgentTrustState::Unknown));
        // A losing parent can leave only raw co-source MCP or Hook evidence. Native
        // selection owns that suppression; it must not create a placeholder
        // record or be confused with the zero-contributor trust exception.
        let declaration_only_parent_suppression = draft_indexes.is_empty()
            && matches!(key.0, AgentAssetCategory::Mcp | AgentAssetCategory::Hook)
            && !suppressed_definitions.is_empty()
            && suppressed_definitions.iter().all(|declaration| {
                matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Suppressed {
                        reason: crate::models::AgentAssetSuppressionReason::ParentNotSelected
                    }
                )
            });
        let declaration_only_evidence = declaration_only_overlay_evidence
            || declaration_only_trust_suppression
            || declaration_only_parent_suppression;
        let valid = !invalid_groups.contains(&key)
            && (declaration_only_evidence
                || (!declarations.is_empty()
                    && !draft_indexes.is_empty()
                    && resolution_group_is_complete(declarations, draft_indexes, validated)));
        if valid {
            continue;
        }
        rejected.extend(draft_indexes.iter().copied());
        let (projection_key, kind) = draft_indexes
            .first()
            .map(|index| {
                (
                    validated[*index].draft.projection_key.clone(),
                    validated[*index].draft.resolution.relation,
                )
            })
            .unwrap_or_else(|| {
                (
                    format!("resolution-group:{}:{}", key.0.key(), key.1),
                    AgentAssetResolutionRelation::Unknown,
                )
            });
        run.emit(
            DiagnosticOwner::Context(context_id.to_string()),
            invalid_resolution(&projection_key, kind),
        );
    }
    rejected
}

fn resolution_group_is_complete(
    declarations: &[&ParsedAgentAsset],
    draft_indexes: &[usize],
    validated: &[ValidatedDraft],
) -> bool {
    let group_ids = declarations
        .iter()
        .map(|declaration| declaration.declaration_id.clone())
        .collect::<BTreeSet<_>>();
    let drafts = draft_indexes
        .iter()
        .map(|index| &validated[*index].draft)
        .collect::<Vec<_>>();

    let represented = drafts
        .iter()
        .flat_map(|draft| draft.represented_declaration_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if represented != group_ids {
        return false;
    }
    let participating = declarations
        .iter()
        .filter(|declaration| {
            matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Participates
            )
        })
        .map(|declaration| declaration.declaration_id.clone())
        .collect::<BTreeSet<_>>();
    let suppressed = group_ids
        .difference(&participating)
        .cloned()
        .collect::<BTreeSet<_>>();
    // An untrusted group may retain participating policy/state overlays as
    // evidence while every native Definition is suppressed.  Preserve that
    // evidence as one zero-contributor Unknown record, but only when its
    // inspection source is itself a represented suppressed Definition.
    let definitions = declarations
        .iter()
        .filter(|declaration| {
            declaration.role == crate::models::AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    if drafts.len() == 1
        && drafts[0].resolution.relation == AgentAssetResolutionRelation::Unknown
        && !definitions.is_empty()
        && definitions.iter().all(|declaration| {
            matches!(
                declaration.participation,
                AgentAssetResolutionParticipation::Suppressed { .. }
            )
        })
        && drafts[0].contributor_ids.is_empty()
        && definitions.iter().any(|declaration| {
            declaration.source_key == drafts[0].inspection_source_id
                && represented.contains(&declaration.declaration_id)
        })
    {
        return true;
    }
    let contributed = drafts
        .iter()
        .flat_map(|draft| draft.contributor_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if !participating.is_subset(&contributed) || !contributed.is_subset(&participating) {
        return false;
    }
    if drafts.iter().any(|draft| {
        draft
            .contributor_ids
            .iter()
            .any(|id| suppressed.contains(id))
    }) {
        return false;
    }

    // Normalized collisions are orthogonal to exact-key precedence.  Validate
    // each exact bucket independently so a replacement chain is not flattened
    // into a collision-only result.
    let exact_ids = declarations
        .iter()
        .filter(|declaration| {
            participating.contains(&declaration.declaration_id)
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .map(|declaration| declaration.native_id.clone())
        .collect::<BTreeSet<_>>();
    let expected_qualified_collision = exact_ids.len() > 1;
    if drafts
        .iter()
        .any(|draft| draft.resolution.qualified_collision != expected_qualified_collision)
    {
        return false;
    }
    if exact_ids.len() > 1 {
        if drafts
            .iter()
            .map(|draft| draft.native_id.clone())
            .collect::<BTreeSet<_>>()
            != exact_ids
        {
            return false;
        }
        for native_id in &exact_ids {
            let bucket_declarations = declarations
                .iter()
                .filter(|declaration| {
                    participating.contains(&declaration.declaration_id)
                        && declaration.role == AgentAssetDeclarationRole::Definition
                        && declaration.native_id == *native_id
                })
                .collect::<Vec<_>>();
            let bucket_drafts = drafts
                .iter()
                .filter(|draft| draft.native_id == *native_id)
                .collect::<Vec<_>>();
            let expected_definition_ids = bucket_declarations
                .iter()
                .map(|declaration| declaration.declaration_id.clone())
                .collect::<BTreeSet<_>>();
            let draft_definition_ids = |draft: &AgentAssetProjectedDraft| {
                draft
                    .contributor_ids
                    .iter()
                    .filter_map(|id| {
                        declarations.iter().find(|declaration| {
                            declaration.declaration_id == *id
                                && declaration.role == AgentAssetDeclarationRole::Definition
                        })
                    })
                    .map(|declaration| declaration.declaration_id.clone())
                    .collect::<BTreeSet<_>>()
            };
            // Structural Definition ownership is exact. Shared overlay
            // applicability is checked later against independent scope members.
            if bucket_drafts
                .iter()
                .any(|draft| !draft_definition_ids(draft).is_subset(&expected_definition_ids))
            {
                return false;
            }
            if bucket_declarations.len() == 1 {
                if bucket_drafts.len() != 1
                    || !matches!(
                        bucket_drafts[0].resolution.relation,
                        AgentAssetResolutionRelation::Independent
                            | AgentAssetResolutionRelation::Unknown
                    )
                    || draft_definition_ids(bucket_drafts[0]) != expected_definition_ids
                {
                    return false;
                }
            } else {
                let winners = bucket_drafts
                    .iter()
                    .filter(|draft| {
                        draft.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                    })
                    .count();
                let losers = bucket_drafts
                    .iter()
                    .filter(|draft| {
                        draft.resolution.relation == AgentAssetResolutionRelation::Replaced
                    })
                    .count();
                let unresolved = bucket_drafts.len() == 1
                    && bucket_drafts[0].resolution.relation
                        == AgentAssetResolutionRelation::Unknown;
                if (!unresolved && (winners != 1 || losers + 1 != bucket_declarations.len()))
                    || (unresolved && bucket_declarations.len() < 2)
                {
                    return false;
                }
                if unresolved {
                    if draft_definition_ids(bucket_drafts[0]) != expected_definition_ids {
                        return false;
                    }
                } else {
                    let Some(winner) = bucket_drafts.iter().find(|draft| {
                        draft.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
                    }) else {
                        return false;
                    };
                    if draft_definition_ids(winner) != expected_definition_ids
                        || bucket_drafts
                            .iter()
                            .filter(|draft| {
                                draft.resolution.relation == AgentAssetResolutionRelation::Replaced
                            })
                            .map(|draft| draft_definition_ids(draft))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .collect::<BTreeSet<_>>()
                            .len()
                            != losers
                        || bucket_drafts
                            .iter()
                            .filter(|draft| {
                                draft.resolution.relation == AgentAssetResolutionRelation::Replaced
                            })
                            .any(|draft| draft_definition_ids(draft).len() != 1)
                    {
                        return false;
                    }
                }
            }
        }
        return true;
    }

    let winner_drafts = drafts
        .iter()
        .copied()
        .filter(|draft| draft.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
        .collect::<Vec<_>>();
    let replaced_drafts = drafts
        .iter()
        .copied()
        .filter(|draft| draft.resolution.relation == AgentAssetResolutionRelation::Replaced)
        .collect::<Vec<_>>();
    if !winner_drafts.is_empty() || !replaced_drafts.is_empty() {
        let participating_definition_count = declarations
            .iter()
            .filter(|declaration| {
                participating.contains(&declaration.declaration_id)
                    && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            })
            .count();
        if participating_definition_count < 2
            || winner_drafts.len() != 1
            || replaced_drafts.len() + 1 != participating_definition_count
            || drafts.len() != participating_definition_count
            || drafts.iter().any(|draft| {
                !matches!(
                    draft.resolution.relation,
                    AgentAssetResolutionRelation::ReplaceWinner
                        | AgentAssetResolutionRelation::Replaced
                )
            })
        {
            return false;
        }
        let winner = winner_drafts[0];
        let actual_losers = replaced_drafts
            .iter()
            .filter_map(|draft| draft.contributor_ids.first().cloned())
            .collect::<BTreeSet<_>>();
        let winner_definition_ids = declarations
            .iter()
            .filter(|declaration| {
                declaration.role == crate::models::AgentAssetDeclarationRole::Definition
                    && participating.contains(&declaration.declaration_id)
                    && !actual_losers.contains(&declaration.declaration_id)
            })
            .map(|declaration| declaration.declaration_id.clone())
            .collect::<Vec<_>>();
        if winner_definition_ids.len() != 1 {
            return false;
        }
        let winner_declaration = declarations.iter().find(|declaration| {
            declaration.declaration_id == winner_definition_ids[0]
                && declaration.native_id == winner.native_id
        });
        let Some(winner_declaration) = winner_declaration else {
            return false;
        };
        if winner
            .contributor_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            != participating
            || winner.native_id != winner_declaration.native_id
            || winner.inspection_source_id != winner_declaration.source_key
        {
            return false;
        }
        let expected_losers = declarations
            .iter()
            .filter(|declaration| {
                declaration.role == crate::models::AgentAssetDeclarationRole::Definition
                    && participating.contains(&declaration.declaration_id)
                    && declaration.declaration_id != winner_declaration.declaration_id
            })
            .map(|declaration| declaration.declaration_id.clone())
            .collect::<BTreeSet<_>>();
        return actual_losers == expected_losers
            && replaced_drafts
                .iter()
                .all(|draft| draft.contributor_ids.len() == 1)
            && replaced_drafts
                .iter()
                .all(|draft| draft.resolution.winner == winner.resolution.winner);
    }

    let Some(kind) = drafts.first().map(|draft| draft.resolution.relation) else {
        return false;
    };
    if drafts.iter().any(|draft| draft.resolution.relation != kind) {
        return false;
    }
    let definition_count = declarations
        .iter()
        .filter(|declaration| {
            participating.contains(&declaration.declaration_id)
                && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
        })
        .count();
    let completion = match kind {
        AgentAssetResolutionRelation::Independent => {
            definition_count == 1
                && drafts.len() == 1
                && drafts[0]
                    .contributor_ids
                    .iter()
                    .filter(|id| {
                        declarations.iter().any(|declaration| {
                            declaration.declaration_id == **id
                                && declaration.role
                                    == crate::models::AgentAssetDeclarationRole::Definition
                        })
                    })
                    .count()
                    == 1
        }
        AgentAssetResolutionRelation::Merged => {
            definition_count >= 2
                && drafts.len() == 1
                && drafts[0]
                    .contributor_ids
                    .iter()
                    .filter(|id| {
                        declarations.iter().any(|declaration| {
                            declaration.declaration_id == **id
                                && declaration.role
                                    == crate::models::AgentAssetDeclarationRole::Definition
                        })
                    })
                    .count()
                    >= 2
        }
        AgentAssetResolutionRelation::Additive => {
            let definition_ids = declarations
                .iter()
                .filter(|declaration| {
                    participating.contains(&declaration.declaration_id)
                        && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
                })
                .map(|declaration| declaration.declaration_id.clone())
                .collect::<BTreeSet<_>>();
            if definition_ids.len() < 2 || drafts.len() != definition_ids.len() {
                return false;
            }
            let actual = drafts
                .iter()
                .flat_map(|draft| {
                    draft
                        .contributor_ids
                        .iter()
                        .filter_map(|id| definition_ids.contains(id).then_some(id.clone()))
                })
                .collect::<BTreeSet<_>>();
            actual == definition_ids
                && drafts.iter().all(|draft| {
                    draft
                        .contributor_ids
                        .iter()
                        .filter(|id| definition_ids.contains(*id))
                        .count()
                        == 1
                })
        }
        AgentAssetResolutionRelation::Unknown => {
            let has_participating_definition = declarations.iter().any(|declaration| {
                participating.contains(&declaration.declaration_id)
                    && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            });
            let has_suppressed_definition = declarations.iter().any(|declaration| {
                matches!(
                    declaration.participation,
                    AgentAssetResolutionParticipation::Suppressed { .. }
                ) && declaration.role == crate::models::AgentAssetDeclarationRole::Definition
            });
            let policy_only = !participating.is_empty()
                && !has_participating_definition
                && has_suppressed_definition
                && declarations
                    .iter()
                    .find(|declaration| {
                        participating.contains(&declaration.declaration_id)
                            && declaration.role
                                != crate::models::AgentAssetDeclarationRole::PolicyOverlay
                    })
                    .is_none();
            drafts.len() == 1
                && ((drafts[0].contributor_ids.is_empty() && participating.is_empty())
                    || has_participating_definition
                    || policy_only)
        }
        AgentAssetResolutionRelation::ReplaceWinner | AgentAssetResolutionRelation::Replaced => {
            false
        }
    };
    completion
}

fn sort_declarations(declarations: &mut Vec<&ParsedAgentAsset>) {
    declarations.sort_by(|left, right| {
        right
            .logical_origin
            .precedence
            .cmp(&left.logical_origin.precedence)
            .then_with(|| left.source_key.cmp(&right.source_key))
            .then_with(|| left.declaration_id.cmp(&right.declaration_id))
    });
}

fn details_are_consistent(draft: &AgentAssetProjectedDraft) -> bool {
    let details_effective_state = match &draft.details {
        AgentAssetDetails::Skill { enabled, .. } => {
            if *enabled != draft.declared_state {
                return false;
            }
            state_from_declared(*enabled)
        }
        AgentAssetDetails::Mcp {
            declared_state,
            approval_state,
            effective_availability,
            ..
        } => {
            if *declared_state != draft.declared_state {
                return false;
            }
            let expected_availability = match draft.resolution.terminal {
                Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
                    AgentAssetEffectiveAvailability::PolicyBlocked
                }
                Some(crate::models::AgentAssetResolutionTerminal::Unknown) => {
                    // TrustRequired is admitted here only after the full
                    // suppressed-definition evidence is checked below.
                    if *declared_state == AgentAssetDeclaredState::Unknown
                        && *effective_availability == AgentAssetEffectiveAvailability::TrustRequired
                        && draft.effective_state == AgentAssetState::Unknown
                        && matches!(
                            draft.trust_state,
                            AgentTrustState::Required | AgentTrustState::Untrusted
                        )
                    {
                        AgentAssetEffectiveAvailability::TrustRequired
                    } else {
                        AgentAssetEffectiveAvailability::Unknown
                    }
                }
                None => {
                    let declared_availability = expected_mcp_availability(
                        draft.declared_state,
                        *approval_state,
                        draft.trust_state,
                    );
                    // The same Unknown state can mean ApprovalRequired or a
                    // parent-induced Unknown. Only the independently assessed
                    // parent graph can determine the final availability; this
                    // early shape check must not reverse-infer it from state.
                    if matches!(
                        draft.state_proof.effective,
                        AgentAssetEffectiveStateProofDraft::ParentGate { .. }
                    ) {
                        *effective_availability
                    } else {
                        declared_availability
                    }
                }
            };
            if *effective_availability != expected_availability {
                return false;
            }
            state_from_availability(*effective_availability)
        }
        AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. } => {
            if *enabled != draft.declared_state {
                return false;
            }
            super::state_projection::intrinsic_state(&draft.details, draft.trust_state)
        }
        AgentAssetDetails::Hook { enabled, .. } => {
            if *enabled != draft.declared_state {
                return false;
            }
            super::state_projection::intrinsic_state(&draft.details, draft.trust_state)
        }
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } => {
            let declared_state = match mode {
                AgentStatusUiMode::BuiltIn | AgentStatusUiMode::Command => {
                    AgentAssetDeclaredState::Enabled
                }
                AgentStatusUiMode::Disabled => AgentAssetDeclaredState::Disabled,
                AgentStatusUiMode::Unknown => AgentAssetDeclaredState::Unknown,
            };
            if declared_state != draft.declared_state
                || *command_present != (*mode == AgentStatusUiMode::Command)
            {
                return false;
            }
            state_from_declared(declared_state)
        }
    };

    let expected_effective_state = match draft.resolution.terminal {
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
            AgentAssetState::Blocked
        }
        Some(crate::models::AgentAssetResolutionTerminal::Unknown) => AgentAssetState::Unknown,
        None if draft.resolution.relation == AgentAssetResolutionRelation::Replaced => {
            AgentAssetState::Shadowed
        }
        None => details_effective_state,
    };
    draft.effective_state == expected_effective_state
        || (matches!(
            draft.state_proof.effective,
            AgentAssetEffectiveStateProofDraft::ParentGate { .. }
        ) && draft.resolution.terminal.is_none())
}

fn materialize_record(
    environment: &AgentEnvironmentDescriptor,
    context: &AgentConfigurationContext,
    item: &ValidatedDraft,
    anchor: &ParsedAgentAsset,
    source_map: &BTreeMap<String, AgentAssetSource>,
) -> AgentAssetRecord {
    let declared_state = state_from_declared(item.draft.declared_state);
    let effective_state = match item.draft.resolution.terminal {
        Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
            AgentAssetState::Blocked
        }
        Some(crate::models::AgentAssetResolutionTerminal::Unknown) => AgentAssetState::Unknown,
        None if item.draft.resolution.relation == AgentAssetResolutionRelation::Replaced => {
            AgentAssetState::Shadowed
        }
        None => item.draft.effective_state,
    };
    let inspection_source_id = source_stable_id(context, &item.inspection.path);
    let revision = source_map
        .get(&inspection_source_id)
        .map(|source| source.revision.clone())
        .unwrap_or_else(|| revision_for_missing(&item.inspection.path));
    let diagnostics = Vec::new();
    let scope = anchor.logical_origin.scope;
    let precedence = anchor.logical_origin.precedence;
    let mut actions = super::read_only_action_set();
    let source_available = source_map.get(&inspection_source_id).is_some_and(|source| {
        !source.revision.is_missing
            && !source.revision.is_symlink
            && !source
                .diagnostics
                .iter()
                .any(source_diagnostic_blocks_access)
    });
    if !source_available {
        mark_read_action_unavailable(&mut actions);
    }
    if item.draft.resolution.relation == AgentAssetResolutionRelation::Replaced {
        mark_mutation_reason(
            &mut actions,
            crate::models::AgentAssetActionUnavailableReason::Shadowed,
        );
    } else {
        match item.draft.resolution.terminal {
            Some(crate::models::AgentAssetResolutionTerminal::PolicyBlocked) => {
                mark_mutation_reason(
                    &mut actions,
                    crate::models::AgentAssetActionUnavailableReason::PolicyBlocked,
                );
            }
            Some(crate::models::AgentAssetResolutionTerminal::Unknown) => {
                mark_mutation_reason(
                    &mut actions,
                    crate::models::AgentAssetActionUnavailableReason::Unknown,
                );
            }
            None => {}
        }
    }
    AgentAssetRecord {
        stable_id: item.stable_id.clone(),
        agent_kind: context.agent_kind,
        category: item.draft.native_kind,
        native_id: item.draft.native_id.clone(),
        label: item.draft.label.clone(),
        source_ids: item.source_ids.clone(),
        inspection_source_id: inspection_source_id.clone(),
        scope,
        provenance: Vec::new(),
        environment_id: environment.id.clone(),
        workspace_id: context.workspace_id.clone(),
        path: Some(item.inspection.path.to_string_lossy().into_owned()),
        precedence,
        writable: if item.contributors.is_empty() {
            item.inspection.writable
        } else {
            item.contributor_sources
                .iter()
                .any(|source| source.writable)
        },
        declared_state,
        effective_state,
        trust_state: item.draft.trust_state,
        diagnostics,
        revision,
        sensitive: if item.contributors.is_empty() {
            item.inspection.sensitive
        } else {
            item.contributor_sources
                .iter()
                .any(|source| source.sensitive)
        } || item.inspection.sensitive,
        is_directory: matches!(item.inspection.source_kind, AgentAssetSourceKind::Directory),
        context_id: context.id.clone(),
        represented_declaration_ids: item.draft.represented_declaration_ids.clone(),
        resolution: AgentAssetResolution {
            relation: item.draft.resolution.relation,
            qualified_collision: item.draft.resolution.qualified_collision,
            terminal: item.draft.resolution.terminal,
            contributor_ids: item.draft.contributor_ids.clone(),
            winner_id: None,
            control_source: None,
            diagnostics: Vec::new(),
        },
        relationships: Default::default(),
        actions,
        access: crate::models::AgentAssetAccess::default(),
        compatible_installation_ids: context.compatible_installation_ids.clone(),
        selected_action_installation_id: None,
        details: item.draft.details.clone(),
    }
}

fn source_for_key<'a>(
    sources: &'a [AgentAssetSourceSpec],
    key: &str,
) -> Option<&'a AgentAssetSourceSpec> {
    let mut matches = sources
        .iter()
        .filter(|source| source.native_source_key == key);
    let value = matches.next()?;
    matches.next().is_none().then_some(value)
}

fn source_for_reference<'a>(
    context: &AgentConfigurationContext,
    sources: &'a [AgentAssetSourceSpec],
    reference: &str,
) -> Option<&'a AgentAssetSourceSpec> {
    let mut matches = sources.iter().filter(|source| {
        source.native_source_key == reference
            || source_stable_id(context, &source.path) == reference
    });
    let value = matches.next()?;
    matches.next().is_none().then_some(value)
}

fn projection_indexes(
    validated: &[ValidatedDraft],
    rejected: Option<&BTreeSet<usize>>,
) -> (ProjectionKeyIndex, NativeAssetIndex) {
    let mut by_projection = BTreeMap::new();
    let mut by_native = BTreeMap::<(AgentAssetCategory, String), Vec<usize>>::new();
    for (index, item) in validated.iter().enumerate() {
        if rejected.is_some_and(|rejected| rejected.contains(&index)) {
            continue;
        }
        by_projection.insert(item.draft.projection_key.clone(), index);
        by_native
            .entry((item.draft.native_kind, item.draft.native_id.clone()))
            .or_default()
            .push(index);
    }
    for indexes in by_native.values_mut() {
        indexes.sort_by(|left, right| {
            validated[*left]
                .draft
                .projection_key
                .cmp(&validated[*right].draft.projection_key)
        });
    }
    (by_projection, by_native)
}

fn resolve_native_ref_index(
    reference: &AgentAssetNativeRef,
    validated: &[ValidatedDraft],
    by_projection: &ProjectionKeyIndex,
    by_native: &NativeAssetIndex,
) -> Option<usize> {
    if let Some(qualifier) = &reference.qualifier {
        let index = *by_projection.get(qualifier)?;
        let target = &validated[index].draft;
        return (target.native_kind == reference.category
            && target.native_id == reference.native_id)
            .then_some(index);
    }
    let candidates = by_native.get(&(reference.category, reference.native_id.clone()))?;
    (candidates.len() == 1).then_some(candidates[0])
}

fn resolve_relationship(request: RelationshipResolutionRequest<'_>) {
    let RelationshipResolutionRequest {
        reference,
        relation,
        item,
        validated,
        by_projection,
        by_native,
        record,
        run,
    } = request;
    let Some(reference) = reference else {
        return;
    };
    match resolve_native_ref_index(reference, validated, by_projection, by_native) {
        Some(target) if validated[target].stable_id != item.stable_id => {
            let stable_id = validated[target].stable_id.clone();
            match relation {
                AgentAssetRelationKind::ProvidedBy => {
                    record.relationships.provided_by = Some(stable_id);
                }
                AgentAssetRelationKind::ActionOwner => {
                    record.relationships.action_owner = Some(stable_id);
                    mark_child_actions(record.actions.as_mut_slice());
                }
                AgentAssetRelationKind::ExplicitImpact => {}
            }
        }
        Some(_) | None => {
            let diagnostic = unresolved_relationship(relation, reference);
            run.emit(DiagnosticOwner::Record(item.stable_id.clone()), diagnostic);
            if relation == AgentAssetRelationKind::ActionOwner {
                mark_child_actions(record.actions.as_mut_slice());
            }
        }
    }
}

fn unresolved_relationship(
    relation: AgentAssetRelationKind,
    reference: &AgentAssetNativeRef,
) -> AgentAssetDiagnostic {
    AgentAssetDiagnostic::UnresolvedRelationship {
        relation,
        native_id: reference.native_id.clone(),
    }
}

fn details_category(details: &AgentAssetDetails) -> AgentAssetCategory {
    match details {
        AgentAssetDetails::Skill { .. } => AgentAssetCategory::Skill,
        AgentAssetDetails::Mcp { .. } => AgentAssetCategory::Mcp,
        AgentAssetDetails::Plugin { .. } => AgentAssetCategory::Plugin,
        AgentAssetDetails::Extension { .. } => AgentAssetCategory::Extension,
        AgentAssetDetails::Hook { .. } => AgentAssetCategory::Hook,
        AgentAssetDetails::StatusUi { .. } => AgentAssetCategory::StatusUi,
    }
}

fn has_duplicates(values: &[String]) -> bool {
    let mut unique = BTreeSet::new();
    values.iter().any(|value| !unique.insert(value))
}

fn invalid_projection(projection_key: &str) -> AgentAssetDiagnostic {
    AgentAssetDiagnostic::InvalidProjection {
        projection_key: projection_key.to_string(),
    }
}

fn invalid_resolution(
    projection_key: &str,
    resolution: AgentAssetResolutionRelation,
) -> AgentAssetDiagnostic {
    AgentAssetDiagnostic::InvalidResolution {
        projection_key: projection_key.to_string(),
        resolution,
    }
}

fn mark_mutation_reason(
    actions: &mut [AgentAssetAction],
    reason: crate::models::AgentAssetActionUnavailableReason,
) {
    for action in actions.iter_mut().filter(|action| {
        matches!(
            action.action,
            crate::models::AgentAssetActionKind::Enable
                | crate::models::AgentAssetActionKind::Disable
        )
    }) {
        action.reason = Some(reason);
    }
}

fn mark_child_actions(actions: &mut [AgentAssetAction]) {
    mark_mutation_reason(
        actions,
        crate::models::AgentAssetActionUnavailableReason::ChildOwnedByParent,
    );
}

fn mark_read_action_unavailable(actions: &mut [AgentAssetAction]) {
    for action in actions.iter_mut().filter(|action| {
        matches!(
            action.action,
            crate::models::AgentAssetActionKind::Preview
                | crate::models::AgentAssetActionKind::Open
                | crate::models::AgentAssetActionKind::Reveal
        )
    }) {
        action.available = false;
        action.reason = Some(crate::models::AgentAssetActionUnavailableReason::SourceUnavailable);
    }
}

fn source_diagnostic_blocks_access(diagnostic: &AgentAssetDiagnostic) -> bool {
    matches!(
        diagnostic,
        AgentAssetDiagnostic::SymlinkRejected { .. }
            | AgentAssetDiagnostic::ReadFailed { .. }
            | AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. }
            | AgentAssetDiagnostic::SourceTypeMismatch { .. }
            | AgentAssetDiagnostic::BudgetExceeded { .. }
    )
}

#[cfg(test)]
mod tests;
