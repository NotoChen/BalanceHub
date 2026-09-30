//! Independently bind Hook handlers to the complete set of plugin Hook files.
use super::*;
use plugin::hooks::HookComponent;
use std::{collections::BTreeSet, path::PathBuf};

fn hook_path(entry: &RegisteredPlugin, manifest: &Manifest, ordinal: usize) -> Option<PathBuf> {
    if ordinal == 0 {
        return Some(entry.path.join("hooks/hooks.json"));
    }
    match manifest.hooks.get(ordinal - 1)? {
        HookComponent::File(relative) => Some(entry.path.join(relative)),
        HookComponent::Inline(_) | HookComponent::Invalid => {
            Some(entry.path.join(".claude-plugin/plugin.json"))
        }
    }
}

impl<'a> NativeIndex<'a> {
    pub(super) fn hook_observations(
        &self,
        entry: &RegisteredPlugin,
        manifest: &Manifest,
    ) -> NativeResult<Vec<&'a ParsedAgentAsset>> {
        let mut ordinals = vec![0];
        let mut paths = BTreeSet::from([plugin::physical_key(
            &entry.path.join("hooks/hooks.json"),
            &entry.path,
            AgentAssetSourceKind::File,
        )]);
        for (index, component) in manifest.hooks.iter().enumerate() {
            if let HookComponent::File(relative) = component {
                if !paths.insert(plugin::physical_key(
                    &entry.path.join(relative),
                    &entry.path,
                    AgentAssetSourceKind::File,
                )) {
                    continue;
                }
            }
            ordinals.push(index + 1);
        }
        ordinals
            .into_iter()
            .map(|ordinal| self.hook_observation(entry, manifest, ordinal))
            .collect()
    }

    fn hook_observation(
        &self,
        entry: &RegisteredPlugin,
        manifest: &Manifest,
        ordinal: usize,
    ) -> NativeResult<&'a ParsedAgentAsset> {
        let id = plugin::hooks::source_id(&entry.key.id, ordinal);
        let mut matches =
            self.buckets.values().flatten().copied().filter(|asset| {
                asset.category == AgentAssetCategory::Hook && asset.native_id == id
            });
        let observation = matches
            .next()
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        let source = self.package_source(observation)?;
        let AgentAssetNativePayload::ClaudePlugin(Payload::HookSource {
            key,
            ordinal: actual,
            document,
            missing,
        }) = &observation.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let role = SourceRole::Hook {
            namespace: manifest.namespace.clone(),
            ordinal,
        };
        let source_binding_valid = self
            .plugin_bindings
            .get(source.native_source_key.as_str())
            .and_then(Option::as_deref)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.key == entry.key
                        && binding.origin() == entry.origin
                        && (binding.role == role
                            || (ordinal > 0
                                && binding.role == SourceRole::Manifest
                                && source.path == entry.path.join(".claude-plugin/plugin.json")))
                })
            });
        if matches.next().is_some()
            || key != &entry.key
            || *actual != ordinal
            || !source_binding_valid
            || hook_path(entry, manifest, ordinal).as_ref() != Some(&source.path)
            || source.allowed_root != entry.path
            || source.origin != AgentAssetInstallationOrigin::NativePackage
            || source.writable
            || source.path_policy != AgentAssetSourcePathPolicy::NoFollow
            || source.source_kind != AgentAssetSourceKind::File
            || observation.declaration_key != id
            || observation.resolution_group_key != id
            || observation.logical_origin != entry.origin
            || observation.role != AgentAssetDeclarationRole::StateOverlay
            || observation.participation != AgentAssetResolutionParticipation::Participates
            || observation.declared_state != AgentAssetDeclaredState::Unknown
            || observation.trust_state != AgentTrustState::Unknown
            || observation.provided_by.is_some()
            || observation.action_owner.is_some()
            || !observation.explicitly_affected.is_empty()
            || (*missing
                && (document.is_some() || observation.presence != AgentAssetPresence::Missing))
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        if !*missing && document.as_ref().is_none_or(|document| !document.loadable) {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        }
        if let Some(HookComponent::Inline(expected)) = ordinal
            .checked_sub(1)
            .and_then(|index| manifest.hooks.get(index))
        {
            if document.as_ref() != Some(expected) {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
        }
        Ok(observation)
    }

    pub(super) fn validate_hook_child(
        &self,
        asset: &ParsedAgentAsset,
        entry: &RegisteredPlugin,
        manifest: &Manifest,
    ) -> NativeResult<()> {
        let observations = self.hook_observations(entry, manifest)?;
        let AgentAssetNativePayload::ClaudePlugin(Payload::Hook {
            ordinal,
            definition,
            ..
        }) = &asset.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let observation = observations.iter().find(|observation| matches!(&observation.native_payload, AgentAssetNativePayload::ClaudePlugin(Payload::HookSource { ordinal: actual, .. }) if actual == ordinal)).ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        let AgentAssetNativePayload::ClaudePlugin(Payload::HookSource {
            document: Some(document),
            missing: false,
            ..
        }) = &observation.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        };
        if asset.source_key != observation.source_key
            || !document.rules.contains(definition)
            || asset.declared_state != AgentAssetDeclaredState::Enabled
            || asset.native_id
                != plugin::hooks::native_id(
                    &entry.key.id,
                    *ordinal,
                    &definition.event,
                    definition.group_index,
                    definition.handler_index,
                )
            || asset.declaration_key != asset.native_id
            || !matches!(asset.details, AgentAssetDetails::Hook { managed, enabled: AgentAssetDeclaredState::Enabled, rule_count: Some(1) } if managed == (entry.origin.scope == AgentAssetScope::Managed))
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(())
    }
}
