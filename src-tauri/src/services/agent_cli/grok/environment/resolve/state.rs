//! Shared state derivation for native construction and independent assessment.
use super::*;

pub(super) fn decisive_overlays<'a>(
    bucket: &[&'a ParsedAgentAsset],
    category: AgentAssetCategory,
) -> Vec<&'a ParsedAgentAsset> {
    if category == AgentAssetCategory::Plugin {
        // These lists are retained as evidence only until exact-version
        // inspect establishes their enablement semantics.
        return Vec::new();
    }
    let controls = bucket
        .iter()
        .copied()
        .filter(|asset| {
            asset.role == AgentAssetDeclarationRole::StateOverlay
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .collect::<Vec<_>>();
    controls
        .into_iter()
        .filter(|asset| match asset.native_payload {
            AgentAssetNativePayload::GrokStateControl(GrokStateControl::McpDisabled) => {
                category == AgentAssetCategory::Mcp
                    && asset.logical_origin.scope == AgentAssetScope::User
            }
            AgentAssetNativePayload::GrokStateControl(GrokStateControl::SkillDisabled) => {
                category == AgentAssetCategory::Skill
                    && asset.logical_origin.scope == AgentAssetScope::User
            }
            _ => false,
        })
        .collect()
}

pub(super) fn intrinsic_assessment(
    anchor: &ParsedAgentAsset,
    plugins: &super::super::plugin::PluginIndex<'_>,
) -> Result<AgentAssetNativeAssessment, AgentAssetAssessmentFailure> {
    let mut state = anchor.declared_state;
    let mut details = anchor.details.clone();
    let mut default = None;
    match anchor.category {
        AgentAssetCategory::Mcp if plugins.child_state(anchor).is_some() => {
            state = plugins
                .child_state(anchor)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            if anchor.declared_state == AgentAssetDeclaredState::Unknown {
                default = Some(state);
            }
        }
        AgentAssetCategory::Mcp => match anchor.native_payload {
            AgentAssetNativePayload::GrokMcpDefinition {
                enabled,
                valid: true,
            } => {
                if enabled.is_none() {
                    state = AgentAssetDeclaredState::Enabled;
                    default = Some(state);
                }
            }
            AgentAssetNativePayload::GrokMcpDefinition { valid: false, .. } => {
                state = AgentAssetDeclaredState::Unknown
            }
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        },
        AgentAssetCategory::Plugin => {
            state = AgentAssetDeclaredState::Unknown;
        }
        AgentAssetCategory::Skill
            if plugins.child_state(anchor).is_some()
                || matches!(
                    anchor.native_payload,
                    AgentAssetNativePayload::GrokSkillDefinition
                ) =>
        {
            state = AgentAssetDeclaredState::Enabled;
            default = Some(state);
        }
        _ => {}
    }
    set_declared(&mut details, state);
    if state == AgentAssetDeclaredState::Unknown {
        return Ok(unknown(
            anchor,
            details,
            &[anchor],
            AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
        ));
    }
    let declared = if let Some(outcome) = default {
        AgentAssetDeclaredStateProofDraft::NativeDefault {
            definition_id: anchor.declaration_id.clone(),
            outcome,
        }
    } else {
        AgentAssetDeclaredStateProofDraft::Definition {
            declaration_ids: vec![anchor.declaration_id.clone()],
            selected_id: anchor.declaration_id.clone(),
        }
    };
    Ok(AgentAssetNativeAssessment {
        declared_state: state,
        declared,
        declared_members: vec![member(anchor, AgentAssetEvidenceKind::Definition)],
        intrinsic: intrinsic_basis(details, anchor.trust_state),
        control: None,
    })
}

pub(super) fn set_declared(details: &mut AgentAssetDetails, state: AgentAssetDeclaredState) {
    match details {
        AgentAssetDetails::Mcp { declared_state, .. } => *declared_state = state,
        AgentAssetDetails::Plugin { enabled, .. }
        | AgentAssetDetails::Extension { enabled, .. }
        | AgentAssetDetails::Skill { enabled, .. }
        | AgentAssetDetails::Hook { enabled, .. } => *enabled = state,
        AgentAssetDetails::StatusUi {
            mode,
            command_present,
        } if state == AgentAssetDeclaredState::Unknown => {
            *mode = AgentStatusUiMode::Unknown;
            *command_present = false;
        }
        AgentAssetDetails::StatusUi { .. } => {}
    }
}

pub(super) fn member(
    asset: &ParsedAgentAsset,
    kind: AgentAssetEvidenceKind,
) -> AgentAssetEvidenceMember {
    AgentAssetEvidenceMember {
        declaration_id: asset.declaration_id.clone(),
        expected_role: asset.role,
        kind,
    }
}

pub(super) fn ids(assets: &[&ParsedAgentAsset]) -> Vec<String> {
    let mut ids = assets
        .iter()
        .map(|asset| asset.declaration_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}
pub(super) fn refs(assets: &[&ParsedAgentAsset]) -> Vec<AgentAssetStateEvidenceRefDraft> {
    ids(assets)
        .into_iter()
        .map(|declaration_id| AgentAssetStateEvidenceRefDraft::Declaration { declaration_id })
        .collect()
}
pub(super) fn unknown(
    anchor: &ParsedAgentAsset,
    details: AgentAssetDetails,
    assets: &[&ParsedAgentAsset],
    cause: AgentAssetDeclaredUnknownCauseDraft,
) -> AgentAssetNativeAssessment {
    AgentAssetNativeAssessment {
        declared_state: AgentAssetDeclaredState::Unknown,
        declared: AgentAssetDeclaredStateProofDraft::Unknown {
            evidence: refs(assets),
            cause,
        },
        declared_members: assets
            .iter()
            .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
            .collect(),
        intrinsic: intrinsic_basis(details, anchor.trust_state),
        control: None,
    }
}
pub(super) fn terminal(
    cause: AgentAssetTerminalCauseDraft,
    evidence: Vec<AgentAssetStateEvidenceRefDraft>,
) -> AgentAssetEffectiveStateProofDraft {
    AgentAssetEffectiveStateProofDraft::Terminal {
        terminal: AgentAssetResolutionTerminal::Unknown,
        cause,
        evidence,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    }
}
