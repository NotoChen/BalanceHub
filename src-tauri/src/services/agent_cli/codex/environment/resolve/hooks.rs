//! Independently verify native keys, normalized trust, user policy and the
//! selected plugin manifest. Workspace trust never grants Hook execution.
mod feature;
mod plugin;

use super::super::hooks::{self as schema, CodexHookPayload};
use super::*;
use crate::models::{AgentAssetInstallationOrigin, AgentAssetScope};
use std::path::Path;

pub(super) struct HookIndex<'a> {
    context: &'a crate::models::AgentConfigurationContext,
    sources: Vec<AgentAssetSourceSpec>,
    declarations: &'a [ParsedAgentAsset],
    feature_control: Result<Option<AgentAssetControlAssessment>, AgentAssetAssessmentFailure>,
}

impl<'a> HookIndex<'a> {
    pub(super) fn new(
        context: &'a crate::models::AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let mut index = Self {
            context,
            sources: sources.to_vec(),
            declarations,
            feature_control: Ok(None),
        };
        index.feature_control = feature::build(&index);
        index
    }

    fn source(
        &self,
        asset: &ParsedAgentAsset,
    ) -> Result<&AgentAssetSourceSpec, AgentAssetAssessmentFailure> {
        let mut matches = self
            .sources
            .iter()
            .filter(|source| source.native_source_key == asset.source_key);
        let source = matches
            .next()
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        if matches.next().is_some()
            || !declaration_matches_source(self.context, asset, source)
            || asset.logical_origin
                != crate::services::agent_cli::environment::physical_origin(source)
            || self
                .declarations
                .iter()
                .filter(|other| other.declaration_id == asset.declaration_id)
                .count()
                != 1
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(source)
    }

    fn policy(&self) -> Result<&'a ParsedAgentAsset, AgentAssetAssessmentFailure> {
        let mut policies = self.declarations.iter().filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::CodexHook(CodexHookPayload::Policy { .. })
            )
        });
        let policy = policies
            .next()
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        let source = self.source(policy)?;
        if policies.next().is_some()
            || policy.role != AgentAssetDeclarationRole::PolicyOverlay
            || policy.source_key != "config"
            || policy.native_id != "codex-hook-user-state"
            || policy.declaration_key != "codex-hook-user-state"
            || source.scope != AgentAssetScope::User
            || source.path != Path::new(&self.context.config_root).join("config.toml")
            || source.origin != AgentAssetInstallationOrigin::ConfigEntry
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(policy)
    }

    pub(super) fn decide(
        &self,
        bucket: &[&'a ParsedAgentAsset],
        target: &AgentAssetAssessmentTarget,
        assets: &assets::AssetIndex<'a>,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        if bucket.len() != 1 {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let mut decision = decide_definition_bucket(bucket, target, false)?;
        let anchor = decision.anchor;
        let AgentAssetNativePayload::CodexHook(CodexHookPayload::Definition {
            key,
            current_hash,
            managed,
            evidence,
        }) = &anchor.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let decoded = schema::decode_definition(evidence)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let native_default = *managed || decoded.builtin;
        let raw_state = if native_default {
            AgentAssetDeclaredState::Enabled
        } else {
            AgentAssetDeclaredState::Unknown
        };
        if decoded.current_hash != *current_hash
            || key != &anchor.native_id
            || key != &anchor.declaration_key
            || anchor.declared_state != raw_state
            || *managed
                != matches!(
                    anchor.logical_origin.scope,
                    AgentAssetScope::System | AgentAssetScope::Managed
                )
            || !anchor.explicitly_affected.is_empty()
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let binding = self.plugin(anchor, evidence, assets)?;
        if *key
            != schema::native_id(
                &binding.key_source,
                &evidence.event,
                evidence.group_index,
                evidence.handler_index,
            )
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        if anchor.trust_state
            != if native_default {
                AgentTrustState::Trusted
            } else {
                AgentTrustState::Unknown
            }
            || anchor.details
                != (AgentAssetDetails::Hook {
                    managed: *managed,
                    enabled: raw_state,
                    rule_count: Some(1),
                })
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        if !native_default {
            let policy = self.policy()?;
            let AgentAssetNativePayload::CodexHook(CodexHookPayload::Policy { states }) =
                &policy.native_payload
            else {
                unreachable!()
            };
            if let Some(states) = states {
                let native = states.get(key);
                let state = if native.and_then(|state| state.enabled) == Some(false) {
                    AgentAssetDeclaredState::Disabled
                } else {
                    AgentAssetDeclaredState::Enabled
                };
                let trust = if native.and_then(|state| state.trusted_hash.as_deref())
                    == Some(current_hash.as_str())
                {
                    AgentTrustState::Trusted
                } else {
                    AgentTrustState::Untrusted
                };
                let mut details = anchor.details.clone();
                if let AgentAssetDetails::Hook { enabled, .. } = &mut details {
                    *enabled = state;
                }
                let explicit = native.and_then(|state| state.enabled).is_some();
                decision.assessment = AgentAssetNativeAssessment {
                    declared_state: state,
                    declared: if explicit {
                        AgentAssetDeclaredStateProofDraft::Policy {
                            declaration_ids: vec![policy.declaration_id.clone()],
                            outcome: state,
                        }
                    } else {
                        AgentAssetDeclaredStateProofDraft::NativeDefault {
                            definition_id: anchor.declaration_id.clone(),
                            outcome: state,
                        }
                    },
                    declared_members: if explicit {
                        vec![member(policy, AgentAssetEvidenceKind::Policy)]
                    } else {
                        vec![member(anchor, AgentAssetEvidenceKind::Definition)]
                    },
                    intrinsic: intrinsic_basis(details, trust),
                    control: None,
                };
                decision.effective_proof = AgentAssetEffectiveStateProofDraft::Intrinsic;
                decision.resolution.terminal = None;
            }
        }
        if feature::apply(self, &mut decision)? {
            return Ok(decision);
        }
        let (effective, terminal_kind, parent_state) = assets::parent_route(
            anchor,
            decision.assessment.declared_state,
            decision.effective_proof,
            decision.resolution.terminal,
            binding.parent_state,
        )?;
        decision.effective_proof = effective;
        decision.resolution.terminal = terminal_kind;
        decision.parent_state = parent_state;
        Ok(decision)
    }
}
