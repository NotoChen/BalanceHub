//! One bounded typed index and one native decision serve both public exits.
use crate::models::*;
use crate::services::agent_cli::contracts::*;
use crate::services::agent_cli::environment::{
    aggregate_relationships, compose_effective_state, declaration_matches_source,
    details_declared_state, finalize_projected_draft, intrinsic_basis, physical_origin,
    qualified_projection_key,
};
use std::collections::{BTreeMap, BTreeSet};

mod draft;
mod extension;
mod hook;
mod index;
mod mcp;
mod remaining;
mod skill;
mod validation;

use draft::*;
use index::NativeIndex;

type NativeResult<T> = Result<T, AgentAssetAssessmentFailure>;
type BucketKey = (AgentAssetCategory, String, String);

pub(super) fn resolve_assets(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let AgentAssetResolveRequest {
        context,
        declarations,
        sources,
    } = request;
    let source_specs = sources
        .iter()
        .map(|source| source.spec.clone())
        .collect::<Vec<_>>();
    let index = NativeIndex::new(context, declarations, &source_specs);
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

pub(super) fn assess_assets(request: AgentAssetAssessmentRequest<'_>) -> AgentAssetAssessmentIndex {
    let AgentAssetAssessmentRequest {
        context,
        declarations,
        sources,
        targets,
    } = request;
    let index = NativeIndex::new(context, declarations, sources);
    let mut result = AgentAssetAssessmentIndex::default();
    for target in targets {
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
