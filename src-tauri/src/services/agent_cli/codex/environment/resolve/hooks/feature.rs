//! Native config-layer merge, independent of per-rule state and Hook trust.
use super::*;
use crate::models::{AgentAssetPresence, AgentAssetSourceKind};
use crate::services::agent_cli::codex::environment::hooks::feature::{
    self as feature_schema, HookFeatureLayer, POLICY_KEY,
};
use crate::services::agent_cli::contracts::AgentAssetSourcePathPolicy;

pub(super) fn build(
    index: &HookIndex<'_>,
) -> Result<Option<AgentAssetControlAssessment>, AgentAssetAssessmentFailure> {
    let observations = index
        .declarations
        .iter()
        .filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy { .. })
            )
        })
        .collect::<Vec<_>>();
    if observations
        .iter()
        .any(|asset| !feature_schema::is_config_source(&asset.source_key))
    {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    let mut layers = Vec::new();
    for source in index
        .sources
        .iter()
        .filter(|source| feature_schema::is_config_source(&source.native_source_key))
    {
        let matches = observations
            .iter()
            .copied()
            .filter(|asset| asset.source_key == source.native_source_key)
            .collect::<Vec<_>>();
        let [asset] = matches.as_slice() else {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        };
        let asset = *asset;
        validate(index, asset)?;
        if asset.participation == AgentAssetResolutionParticipation::Participates {
            let AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy { layer }) =
                &asset.native_payload
            else {
                unreachable!()
            };
            layers.push((asset, *layer));
        }
    }
    if observations.iter().any(|asset| {
        !index
            .sources
            .iter()
            .any(|source| source.native_source_key == asset.source_key)
    }) {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    layers.sort_by_key(|(asset, _)| asset.logical_origin.precedence);
    let mut merged = HookFeatureLayer::Absent;
    let mut selected = None;
    let mut unavailable = Vec::new();
    for (asset, layer) in layers {
        match layer {
            HookFeatureLayer::Unavailable => unavailable.push(asset),
            HookFeatureLayer::Absent => {}
            HookFeatureLayer::TableWithoutHook if merged != HookFeatureLayer::InvalidTable => {}
            _ => {
                merged = layer;
                selected = Some(asset);
            }
        }
    }
    if !unavailable.is_empty() {
        return Ok(Some(control(
            AgentAssetTerminalCauseDraft::InvalidControl,
            &unavailable,
        )));
    }
    let cause = match merged {
        HookFeatureLayer::Enabled(false) => AgentAssetTerminalCauseDraft::TypedPolicy,
        HookFeatureLayer::InvalidHook | HookFeatureLayer::InvalidTable => {
            AgentAssetTerminalCauseDraft::InvalidControl
        }
        HookFeatureLayer::Absent
        | HookFeatureLayer::TableWithoutHook
        | HookFeatureLayer::Enabled(true) => return Ok(None),
        HookFeatureLayer::Unavailable => unreachable!(),
    };
    let selected = selected.ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
    Ok(Some(control(cause, &[selected])))
}

fn validate(
    index: &HookIndex<'_>,
    asset: &ParsedAgentAsset,
) -> Result<(), AgentAssetAssessmentFailure> {
    let source = index.source(asset)?;
    let (scope, precedence, expected_path) = match source.native_source_key.as_str() {
        "config" => (
            AgentAssetScope::User,
            10,
            Path::new(&index.context.config_root).join("config.toml"),
        ),
        "system-config" => (
            AgentAssetScope::System,
            5,
            source.allowed_root.join("config.toml"),
        ),
        "workspace-config" => (
            AgentAssetScope::Workspace,
            20,
            Path::new(
                index
                    .context
                    .workspace_id
                    .as_deref()
                    .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?,
            )
            .join(".codex/config.toml"),
        ),
        _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    };
    if source.scope != scope
        || source.precedence != precedence
        || source.path != expected_path
        || source.source_kind != AgentAssetSourceKind::File
        || source.origin != AgentAssetInstallationOrigin::ConfigEntry
        || source.path_policy != AgentAssetSourcePathPolicy::NoFollow
        || asset.category != AgentAssetCategory::Hook
        || asset.role != AgentAssetDeclarationRole::PolicyOverlay
        || asset.declaration_key != POLICY_KEY
        || asset.resolution_group_key != POLICY_KEY
        || asset.native_id != POLICY_KEY
        || asset.presence != AgentAssetPresence::Present
        || asset.declared_state != AgentAssetDeclaredState::Unknown
        || asset.trust_state != AgentTrustState::Unknown
        || asset.participation != feature_schema::participation(index.context, source)
        || asset.provided_by.is_some()
        || asset.action_owner.is_some()
        || !asset.explicitly_affected.is_empty()
        || asset.details
            != (AgentAssetDetails::Hook {
                managed: matches!(scope, AgentAssetScope::System | AgentAssetScope::Managed),
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count: Some(0),
            })
    {
        return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
    }
    Ok(())
}

fn control(
    cause: AgentAssetTerminalCauseDraft,
    assets: &[&ParsedAgentAsset],
) -> AgentAssetControlAssessment {
    AgentAssetControlAssessment {
        cause,
        members: assets
            .iter()
            .map(|asset| {
                member(
                    asset,
                    if cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                        AgentAssetEvidenceKind::Policy
                    } else {
                        AgentAssetEvidenceKind::InvalidControl
                    },
                )
            })
            .collect(),
        authorities: assets
            .iter()
            .map(|asset| AgentAssetControlAuthority::Declaration(asset.declaration_id.clone()))
            .collect(),
    }
}

pub(super) fn apply(
    index: &HookIndex<'_>,
    decision: &mut NativeDecision<'_>,
) -> Result<bool, AgentAssetAssessmentFailure> {
    let Some(control) = index.feature_control.as_ref().map_err(|failure| *failure)? else {
        return Ok(false);
    };
    decision.resolution.terminal = Some(
        if control.cause == AgentAssetTerminalCauseDraft::TypedPolicy {
            AgentAssetResolutionTerminal::PolicyBlocked
        } else {
            AgentAssetResolutionTerminal::Unknown
        },
    );
    decision.resolution.control_source = if control.authorities.len() == 1 {
        match control.authorities.first() {
            Some(AgentAssetControlAuthority::Declaration(id)) => {
                Some(AgentAssetPolicyReferenceDraft::Declaration {
                    declaration_id: id.clone(),
                })
            }
            _ => None,
        }
    } else {
        None
    };
    decision.effective_proof = terminal(control.cause, evidence(&control.members));
    decision.assessment.control = Some(control.clone());
    Ok(true)
}
