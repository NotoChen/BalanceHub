//! Local native order: complete input, allowed intersection, excluded union,
//! persistent declared state, then a still-material Extension parent gate.
use super::*;

pub(super) fn decide<'a>(
    index: &NativeIndex<'a>,
    target: &AgentAssetAssessmentTarget,
) -> NativeResult<NativeDecision<'a>> {
    let mut decision = remaining::base(index, target)?;
    if decision.resolution.relation == AgentAssetResolutionRelation::Replaced
        || decision.contributors.is_empty()
    {
        return Ok(decision);
    }
    let structural = decision.resolution.relation == AgentAssetResolutionRelation::Unknown;
    if !structural {
        let overlays = index
            .overlays
            .get(&(target.category, target.resolution_group_key.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        if let Some(assessment) = overlay_assessment(decision.anchor, overlays) {
            decision.assessment = assessment;
        }
    }
    if let Some(terminal) = invalid_control(index) {
        decision.terminal(terminal);
    } else if !structural {
        if let Some(terminal) = policy_control(index, decision.anchor) {
            decision.terminal(terminal);
        } else {
            remaining::apply_parent(&mut decision)?;
        }
    }
    Ok(decision)
}

fn invalid_control(index: &NativeIndex<'_>) -> Option<NativeTerminal> {
    let mut members = index.invalid_mcp_policy.clone();
    members.extend(index.invalid_mcp_enablement.iter().copied());
    if members.is_empty() {
        return None;
    }
    members.sort_by(|left, right| left.declaration_id.cmp(&right.declaration_id));
    members.dedup_by_key(|asset| asset.declaration_id.clone());
    Some(NativeTerminal::declarations(
        AgentAssetTerminalCauseDraft::InvalidControl,
        &members,
    ))
}

fn policy_control(index: &NativeIndex<'_>, anchor: &ParsedAgentAsset) -> Option<NativeTerminal> {
    if index
        .allowed_intersection
        .as_ref()
        .is_some_and(|set| !extension_alias_matches(set, anchor))
    {
        let witnesses = index
            .allowed
            .iter()
            .map(|(asset, _)| *asset)
            .collect::<Vec<_>>();
        return Some(NativeTerminal::declarations(
            AgentAssetTerminalCauseDraft::TypedPolicy,
            &witnesses,
        ));
    }
    let excluded = index
        .excluded
        .iter()
        .filter_map(|(asset, set)| extension_alias_matches(set, anchor).then_some(*asset))
        .collect::<Vec<_>>();
    (!excluded.is_empty())
        .then(|| NativeTerminal::declarations(AgentAssetTerminalCauseDraft::TypedPolicy, &excluded))
}

fn extension_alias_matches(values: &BTreeSet<String>, anchor: &ParsedAgentAsset) -> bool {
    if values.contains(&anchor.resolution_group_key) {
        return true;
    }
    anchor.provided_by.as_ref().is_some_and(|parent| {
        values.contains(
            &format!("ext:{}:{}", parent.native_id, anchor.native_id)
                .trim()
                .to_ascii_lowercase(),
        )
    })
}
