//! The persisted disabled-hooks list is a user policy, independently of the
//! definition source and any native project-trust gate.
use super::super::hooks::{GrokHookPayload, POLICY_KEY};
use super::*;
use crate::{
    models::AgentAssetInstallationOrigin,
    services::agent_cli::environment::declaration_matches_source,
};
use std::path::Path;

pub(super) struct HookIndex<'a> {
    context: AgentConfigurationContext,
    sources: Vec<AgentAssetSourceSpec>,
    policies: Vec<&'a ParsedAgentAsset>,
}

impl<'a> HookIndex<'a> {
    pub(super) fn new(
        context: &AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        Self {
            context: context.clone(),
            sources: sources.to_vec(),
            policies: declarations
                .iter()
                .filter(|asset| {
                    matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::GrokHook(GrokHookPayload::Disabled { .. })
                    )
                })
                .collect(),
        }
    }

    fn source_valid(&self, asset: &ParsedAgentAsset) -> bool {
        let matches = self
            .sources
            .iter()
            .filter(|source| source.native_source_key == asset.source_key)
            .collect::<Vec<_>>();
        matches.len() == 1 && declaration_matches_source(&self.context, asset, matches[0])
    }

    pub(super) fn apply(
        &self,
        mut decision: NativeDecision<'a>,
        plugins: &super::super::plugin::PluginIndex<'a>,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let anchor = decision.anchor;
        let (name, managed) = match &anchor.native_payload {
            AgentAssetNativePayload::GrokHook(GrokHookPayload::Definition { name, managed })
                if self.sources.iter().any(|source| {
                    source.native_source_key == anchor.source_key
                        && super::super::hooks::source_namespace(source).is_some()
                }) =>
            {
                (name, *managed)
            }
            AgentAssetNativePayload::GrokPlugin(_)
                if plugins.valid(anchor) && plugins.parent(anchor).is_some() =>
            {
                (&anchor.native_id, false)
            }
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        if !self.source_valid(anchor) || name != &anchor.native_id {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        if decision.resolution.relation == AgentAssetResolutionRelation::Unknown {
            return Ok(decision);
        }
        if managed {
            return Ok(decision);
        }
        let [policy] = self.policies.as_slice() else {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        };
        if !self.source_valid(policy)
            || policy.role != AgentAssetDeclarationRole::PolicyOverlay
            || policy.native_id != POLICY_KEY
            || self
                .sources
                .iter()
                .find(|source| source.native_source_key == policy.source_key)
                .is_none_or(|source| {
                    source.native_source_key != POLICY_KEY
                        || source.scope != AgentAssetScope::User
                        || source.path
                            != Path::new(&self.context.config_root).join("disabled-hooks")
                        || source.origin != AgentAssetInstallationOrigin::ConfigEntry
                        || source.hook_definition_source
                })
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let policy = *policy;
        let AgentAssetNativePayload::GrokHook(GrokHookPayload::Disabled { names }) =
            &policy.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let Some(names) = names else {
            if decision.resolution.relation == AgentAssetResolutionRelation::Replaced {
                return Ok(decision);
            }
            decision.assessment.control = Some(AgentAssetControlAssessment {
                cause: AgentAssetTerminalCauseDraft::InvalidControl,
                members: vec![member(policy, AgentAssetEvidenceKind::InvalidControl)],
                authorities: BTreeSet::from([AgentAssetControlAuthority::Declaration(
                    policy.declaration_id.clone(),
                )]),
            });
            decision.effective = terminal(
                AgentAssetTerminalCauseDraft::InvalidControl,
                refs(&[policy]),
            );
            decision.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
            decision.resolution.control_source =
                Some(AgentAssetPolicyReferenceDraft::Declaration {
                    declaration_id: policy.declaration_id.clone(),
                });
            return Ok(decision);
        };
        let disabled = names.contains(name);
        let state = if disabled {
            AgentAssetDeclaredState::Disabled
        } else {
            AgentAssetDeclaredState::Enabled
        };
        let mut details = anchor.details.clone();
        set_declared(&mut details, state);
        decision.assessment = AgentAssetNativeAssessment {
            declared_state: state,
            declared: if disabled {
                AgentAssetDeclaredStateProofDraft::Policy {
                    declaration_ids: vec![policy.declaration_id.clone()],
                    outcome: state,
                }
            } else {
                AgentAssetDeclaredStateProofDraft::Definition {
                    declaration_ids: vec![anchor.declaration_id.clone()],
                    selected_id: anchor.declaration_id.clone(),
                }
            },
            declared_members: if disabled {
                vec![member(policy, AgentAssetEvidenceKind::Policy)]
            } else {
                vec![member(anchor, AgentAssetEvidenceKind::Definition)]
            },
            intrinsic: intrinsic_basis(details, anchor.trust_state),
            control: None,
        };
        if disabled
            && matches!(
                decision.effective,
                AgentAssetEffectiveStateProofDraft::ParentGate { .. }
                    | AgentAssetEffectiveStateProofDraft::Terminal {
                        cause: AgentAssetTerminalCauseDraft::ParentUnknown,
                        ..
                    }
            )
        {
            decision.effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
            decision.resolution.terminal = None;
        }
        Ok(decision)
    }
}
