use super::super::super::{hooks::CodexHookDefinitionEvidence, native};
use super::*;
use crate::models::{AgentAssetInstallState, AgentAssetSourceKind, AgentAssetState};
use crate::services::agent_cli::contracts::{AgentAssetSourcePathPolicy, CodexAssetPayload};

pub(super) struct PluginBinding {
    pub(super) key_source: String,
    pub(super) parent_state: Option<AgentAssetState>,
}

impl<'a> HookIndex<'a> {
    pub(super) fn plugin(
        &self,
        anchor: &'a ParsedAgentAsset,
        evidence: &CodexHookDefinitionEvidence,
        assets: &assets::AssetIndex<'a>,
    ) -> Result<PluginBinding, AgentAssetAssessmentFailure> {
        let source = self.source(anchor)?;
        let Some(binding) = &evidence.plugin else {
            return self.standalone(anchor, source);
        };
        let reference = AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: binding.plugin_id.clone(),
            qualifier: Some(format!("plugin:{}", binding.plugin_id)),
        };
        if anchor.provided_by.as_ref() != Some(&reference)
            || anchor.action_owner.as_ref() != Some(&reference)
            || source.origin != AgentAssetInstallationOrigin::NativePackage
            || source.writable
            || source.source_kind != AgentAssetSourceKind::File
            || source.path_policy != AgentAssetSourcePathPolicy::NoFollow
            || source.allowed_root != Path::new(&self.context.config_root)
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let parent = assets.plugin(&binding.plugin_id)?;
        if !matches!(
            parent.assessment.intrinsic.details,
            AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                ..
            }
        ) {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        }
        let manifests = parent
            .contributors
            .iter()
            .copied()
            .filter(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest {
                        valid: true,
                        ..
                    })
                )
            })
            .collect::<Vec<_>>();
        let [manifest] = manifests.as_slice() else {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        };
        if manifest.source_key != binding.manifest_key {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let (id, _, format) = binding
            .manifest_key
            .strip_prefix(native::PLUGIN_MANIFEST_PREFIX)
            .and_then(native::plugin_manifest_parts)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        if id != binding.plugin_id || format == "agent" {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let manifest_source = self.source(manifest)?;
        let root = manifest_source
            .path
            .parent()
            .and_then(Path::parent)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let observations = self
            .declarations
            .iter()
            .filter(|asset| {
                asset.source_key == binding.manifest_key
                    && matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::CodexHook(CodexHookPayload::PluginManifest { .. })
                    )
            })
            .collect::<Vec<_>>();
        let [observation] = observations.as_slice() else {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        };
        let observation = *observation;
        self.source(observation)?;
        if observation.category != AgentAssetCategory::Hook
            || observation.role != AgentAssetDeclarationRole::StateOverlay
            || observation.declaration_key != native::plugin_hooks::MANIFEST_KEY
            || observation.resolution_group_key != native::plugin_hooks::MANIFEST_KEY
            || observation.native_id != native::plugin_hooks::MANIFEST_KEY
            || observation.declared_state != AgentAssetDeclaredState::Unknown
            || observation.trust_state != AgentTrustState::Unknown
            || observation.participation != AgentAssetResolutionParticipation::Participates
            || observation.provided_by.is_some()
            || observation.action_owner.is_some()
            || !observation.explicitly_affected.is_empty()
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let AgentAssetNativePayload::CodexHook(payload) = &observation.native_payload else {
            unreachable!()
        };
        let selected = native::plugin_hooks::select_component(payload, root, evidence)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        match selected.location {
            native::plugin_hooks::HookComponentLocation::File { path, source_key } => {
                if source.path != path || source.native_source_key != source_key {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            native::plugin_hooks::HookComponentLocation::Manifest => {
                if source.native_source_key != manifest_source.native_source_key {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
        }
        let parent_state =
            compose_effective_state(&parent.assessment.intrinsic, &parent.effective_proof, None)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?
                .0;
        Ok(PluginBinding {
            key_source: selected.key_source,
            parent_state: Some(parent_state),
        })
    }

    fn standalone(
        &self,
        anchor: &ParsedAgentAsset,
        source: &AgentAssetSourceSpec,
    ) -> Result<PluginBinding, AgentAssetAssessmentFailure> {
        if anchor.provided_by.is_some()
            || anchor.action_owner.is_some()
            || source.origin != AgentAssetInstallationOrigin::ConfigEntry
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let (expected, scope) = match source.native_source_key.as_str() {
            "hooks" => (
                Path::new(&self.context.config_root).join("hooks.json"),
                AgentAssetScope::User,
            ),
            "config" => (
                Path::new(&self.context.config_root).join("config.toml"),
                AgentAssetScope::User,
            ),
            "workspace-hooks" | "workspace-config" => (
                Path::new(
                    self.context
                        .workspace_id
                        .as_deref()
                        .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?,
                )
                .join(".codex")
                .join(if source.native_source_key == "workspace-hooks" {
                    "hooks.json"
                } else {
                    "config.toml"
                }),
                AgentAssetScope::Workspace,
            ),
            "system-config" => (
                super::super::super::system_paths::system_root().join("config.toml"),
                AgentAssetScope::System,
            ),
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        if source.path != expected || source.scope != scope {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(PluginBinding {
            key_source: source.path.to_string_lossy().into_owned(),
            parent_state: None,
        })
    }
}
