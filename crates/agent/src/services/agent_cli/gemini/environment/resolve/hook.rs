//! Per-rule disabled sets union; the independent global switch uses precedence.
use super::*;

pub(super) fn decide<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    let mut decision = remaining::base(index, target)?;
    let AgentAssetNativePayload::HookDefinition(matcher) = &decision.anchor.native_payload else {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    };
    let matches = index
        .hook_disabled
        .iter()
        .filter_map(|(asset, set)| set.contains(matcher).then_some(*asset))
        .collect::<Vec<_>>();
    decision.assessment = if matches.is_empty() {
        native_default(decision.anchor)
    } else {
        disabled_policy_assessment(decision.anchor, &matches)
    };
    if let Some(terminal) = global_terminal(index) {
        decision.terminal(terminal);
        return Ok(decision);
    }
    if matches.is_empty()
        && !index.invalid_hook_policy.is_empty()
        && decision.parent_state != Some(AgentAssetState::Disabled)
    {
        decision.terminal(NativeTerminal::declarations(
            AgentAssetTerminalCauseDraft::InvalidControl,
            &index.invalid_hook_policy,
        ));
    } else {
        remaining::apply_parent(&mut decision)?;
    }
    Ok(decision)
}

fn global_terminal(index: &NativeIndex<'_>) -> Option<NativeTerminal> {
    let (first, enabled) = index.hook_global.first()?;
    let peers = index
        .hook_global
        .iter()
        .take_while(|(asset, _)| asset.logical_origin.precedence == first.logical_origin.precedence)
        .collect::<Vec<_>>();
    let cause = if enabled.is_none() || peers.iter().any(|(_, value)| value != enabled) {
        AgentAssetTerminalCauseDraft::InvalidControl
    } else if *enabled == Some(false) {
        AgentAssetTerminalCauseDraft::TypedPolicy
    } else {
        return None;
    };
    Some(NativeTerminal::declarations(
        cause,
        &peers
            .into_iter()
            .map(|(asset, _)| *asset)
            .collect::<Vec<_>>(),
    ))
}
