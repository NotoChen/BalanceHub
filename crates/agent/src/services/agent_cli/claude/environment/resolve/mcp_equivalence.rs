//! Endpoint suppression is runtime-dependent and crosses native-name buckets.
//! Keep a visible Unknown boundary instead of inventing a replacement relation.
use super::*;
use crate::services::agent_cli::contracts::AgentMcpMatcherIdentity;

pub(super) fn assess(
    index: &NativeIndex<'_>,
    anchor: &ParsedAgentAsset,
    assessment: AgentAssetNativeAssessment,
) -> NativeResult<AgentAssetNativeAssessment> {
    if assessment.declared_state != AgentAssetDeclaredState::Enabled
        || !matches!(
            anchor.native_payload,
            AgentAssetNativePayload::ClaudePlugin(
                super::super::plugin::ClaudePluginPayload::Mcp { .. }
            )
        )
    {
        return Ok(assessment);
    }
    let Some(anchor_identity) = identity(anchor) else {
        return Ok(assessment);
    };
    for ((category, _, native_id), bucket) in &index.buckets {
        if *category != AgentAssetCategory::Mcp || *native_id == anchor.native_id {
            continue;
        }
        let definitions = definitions(bucket);
        let candidates = if tied(&definitions) {
            definitions.as_slice()
        } else {
            &definitions[..definitions.len().min(1)]
        };
        for peer in candidates {
            if !identity(peer).is_some_and(|candidate| {
                super::super::policy::possibly_equal_mcp_identity(anchor_identity, candidate)
            }) {
                continue;
            }
            let peer_assessment = mcp::declared(index, peer, false, definition_assessment(peer))?;
            if peer_assessment.declared_state == AgentAssetDeclaredState::Disabled
                || mcp::terminal(index, peer, &peer_assessment)?.is_some_and(|terminal| {
                    terminal.kind == AgentAssetResolutionTerminal::PolicyBlocked
                })
                || matches!(
                    peer_assessment.intrinsic.details,
                    AgentAssetDetails::Mcp {
                        approval_state: AgentMcpApprovalState::Pending
                            | AgentMcpApprovalState::Rejected,
                        ..
                    }
                )
            {
                continue;
            }
            if let Some(entry) = index.validate_plugin_child(peer)? {
                let parent = index.decide(&AgentAssetAssessmentTarget {
                    category: AgentAssetCategory::Plugin,
                    resolution_group_key: entry.key.id.clone(),
                    exact_native_id: entry.key.id.clone(),
                    subject: AgentAssetAssessmentSubject::Bucket,
                })?;
                let (state, _) = compose_effective_state(
                    &parent.assessment.intrinsic,
                    &parent.effective,
                    parent.parent_state,
                )
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
                if matches!(
                    state,
                    crate::models::AgentAssetState::Disabled
                        | crate::models::AgentAssetState::NotInstalled
                ) {
                    continue;
                }
            }
            let mut unknown = unknown_assessment(
                anchor,
                &[anchor],
                AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                vec![member(anchor, AgentAssetEvidenceKind::Definition)],
            );
            let mut details = assessment.intrinsic.details;
            set_declared(&mut details, AgentAssetDeclaredState::Unknown);
            unknown.intrinsic = intrinsic_basis(details, assessment.intrinsic.trust_state);
            return Ok(unknown);
        }
    }
    Ok(assessment)
}

fn identity(asset: &ParsedAgentAsset) -> Option<&AgentMcpMatcherIdentity> {
    match &asset.native_payload {
        AgentAssetNativePayload::McpDefinition(payload) => Some(&payload.identity),
        AgentAssetNativePayload::ClaudePlugin(super::super::plugin::ClaudePluginPayload::Mcp {
            definition,
            ..
        }) => Some(&definition.identity),
        _ => None,
    }
}
