//! Skill names stay exact; Gemini matches its union disable policy without case.
use super::*;

pub(super) fn decide<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    let mut decision = remaining::base(index, target)?;
    if decision.resolution.relation == AgentAssetResolutionRelation::Unknown {
        return Ok(decision);
    }
    decision.assessment = native_default(decision.anchor);
    if decision.resolution.relation == AgentAssetResolutionRelation::Replaced {
        return Ok(decision);
    }
    let name = decision.anchor.native_id.to_lowercase();
    let matches = index
        .skill_disabled
        .iter()
        .filter_map(|(asset, names)| names.contains(&name).then_some(*asset))
        .collect::<Vec<_>>();
    if !matches.is_empty() {
        decision.assessment = disabled_policy_assessment(decision.anchor, &matches);
    }
    if matches.is_empty()
        && !index.invalid_skill_policy.is_empty()
        && decision.parent_state != Some(AgentAssetState::Disabled)
    {
        decision.terminal(NativeTerminal::declarations(
            AgentAssetTerminalCauseDraft::InvalidControl,
            &index.invalid_skill_policy,
        ));
    } else {
        remaining::apply_parent(&mut decision)?;
    }
    Ok(decision)
}
