//! A complete Extension decision is cached as the unique parent plan.
use super::*;

pub(super) fn decide<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    let mut decision = remaining::base(index, target)?;
    if decision.resolution.relation == AgentAssetResolutionRelation::Replaced {
        return Ok(decision);
    }
    if decision.resolution.relation != AgentAssetResolutionRelation::Unknown {
        let overlays = index
            .overlays
            .get(&(target.category, target.resolution_group_key.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        decision.assessment = overlay_assessment(decision.anchor, overlays)
            .unwrap_or_else(|| native_default(decision.anchor));
    }
    if !index.invalid_extension_enablement.is_empty() {
        decision.terminal(NativeTerminal::declarations(
            AgentAssetTerminalCauseDraft::InvalidControl,
            &index.invalid_extension_enablement,
        ));
    }
    Ok(decision)
}
