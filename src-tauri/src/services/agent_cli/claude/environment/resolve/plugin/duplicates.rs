//! Native physical Skill deduplication never changes public alias paths.
use super::*;
use crate::services::agent_cli::contracts::{
    AgentDefinitionSelectionRequest, AgentDefinitionSuppression,
};

impl NativeIndex<'_> {
    pub(in crate::services::agent_cli::claude::environment::resolve) fn conflicting_plugin_skill_parents(
        &self,
    ) -> bool {
        self.buckets.iter().any(|((category, _, _), bucket)| {
            if *category != AgentAssetCategory::Skill {
                return false;
            }
            let mut parents = BTreeSet::new();
            for asset in definitions(bucket) {
                let AgentAssetNativePayload::ClaudePlugin(Payload::Skill { key, .. }) =
                    &asset.native_payload
                else {
                    continue;
                };
                if asset.declared_state == AgentAssetDeclaredState::Enabled
                    && self
                        .validate_plugin_child(asset)
                        .is_ok_and(|parent| parent.is_some())
                {
                    parents.insert(key.id.as_str());
                }
            }
            parents.len() > 1
        })
    }

    pub(in crate::services::agent_cli::claude::environment::resolve) fn unordered_plugin_skill_duplicate_ids(
        &self,
    ) -> BTreeSet<String> {
        let mut candidates = BTreeMap::<&Path, BTreeMap<&PackageKey, Vec<&str>>>::new();
        for asset in self.buckets.values().flatten().copied() {
            let AgentAssetNativePayload::ClaudePlugin(Payload::Skill { key, .. }) =
                &asset.native_payload
            else {
                continue;
            };
            if asset.declared_state != AgentAssetDeclaredState::Enabled
                || self.validate_plugin_child(asset).is_err()
            {
                continue;
            }
            let Ok(parent) = self.decide(&AgentAssetAssessmentTarget {
                category: AgentAssetCategory::Plugin,
                resolution_group_key: key.id.clone(),
                exact_native_id: key.id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            }) else {
                continue;
            };
            if compose_effective_state(
                &parent.assessment.intrinsic,
                &parent.effective,
                parent.parent_state,
            )
            .is_none_or(|(state, _)| state != AgentAssetState::Enabled)
            {
                continue;
            }
            let Some(path) = self
                .sources
                .iter()
                .find(|source| source.native_source_key == asset.source_key)
                .and_then(|source| source.verified_physical_path.as_deref())
            else {
                continue;
            };
            candidates
                .entry(path)
                .or_default()
                .entry(key)
                .or_default()
                .push(&asset.declaration_id);
        }
        candidates
            .into_values()
            .filter(|parents| parents.len() > 1)
            .flat_map(BTreeMap::into_values)
            .flatten()
            .map(str::to_owned)
            .collect()
    }
}

pub(in crate::services::agent_cli::claude) fn definition_suppressions(
    request: AgentDefinitionSelectionRequest<'_>,
) -> Vec<AgentDefinitionSuppression> {
    if request.context.agent_kind != crate::services::agent_cli::claude::AGENT_KIND {
        return Vec::new();
    }
    let sources = request
        .sources
        .iter()
        .map(|source| (source.native_source_key.as_str(), source))
        .collect::<BTreeMap<_, _>>();
    if sources.len() != request.sources.len()
        || request
            .declarations
            .iter()
            .map(|asset| &asset.declaration_id)
            .collect::<BTreeSet<_>>()
            .len()
            != request.declarations.len()
    {
        return Vec::new();
    }
    let native = NativeIndex::new(request.context, request.declarations, request.sources);
    let standalone = request
        .declarations
        .iter()
        .filter_map(|asset| {
            let source = *sources.get(asset.source_key.as_str())?;
            let (root_key, entry) = asset
                .source_key
                .strip_prefix(super::super::super::SKILL_PREFIX)?
                .split_once(':')?;
            let root = *sources.get(root_key)?;
            (matches!(root_key, "skills" | "workspace-skills")
                && !entry.is_empty()
                && asset.category == AgentAssetCategory::Skill
                && asset.declaration_key == entry
                && asset.native_id == entry
                && asset.resolution_group_key == entry
                && asset.role == AgentAssetDeclarationRole::Definition
                && asset.participation == AgentAssetResolutionParticipation::Participates
                && asset.presence == AgentAssetPresence::Present
                && asset.declared_state == AgentAssetDeclaredState::Enabled
                && matches!(
                    asset.details,
                    AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Enabled,
                        ..
                    }
                )
                && asset.provided_by.is_none()
                && asset.action_owner.is_none()
                && matches!(asset.native_payload, AgentAssetNativePayload::None)
                && declaration_matches_source(request.context, asset, source)
                && asset.logical_origin == physical_origin(source)
                && source.path == root.path.join(entry).join("SKILL.md")
                && source.source_kind == AgentAssetSourceKind::File
                && root.source_kind == AgentAssetSourceKind::Directory
                && source.scope == root.scope
                && source.precedence == root.precedence)
                .then_some(source.verified_physical_path.as_deref())
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    let mut candidates = request
        .declarations
        .iter()
        .filter_map(|asset| {
            let AgentAssetNativePayload::ClaudePlugin(Payload::Skill { key, .. }) =
                &asset.native_payload
            else {
                return None;
            };
            if asset.declared_state != AgentAssetDeclaredState::Enabled {
                return None;
            }
            native.validate_plugin_child(asset).ok().flatten()?;
            let parent = native
                .decide(&AgentAssetAssessmentTarget {
                    category: AgentAssetCategory::Plugin,
                    resolution_group_key: key.id.clone(),
                    exact_native_id: key.id.clone(),
                    subject: AgentAssetAssessmentSubject::Bucket,
                })
                .ok()?;
            let (state, _) = compose_effective_state(
                &parent.assessment.intrinsic,
                &parent.effective,
                parent.parent_state,
            )?;
            if state != AgentAssetState::Enabled {
                return None;
            }
            let source = *sources.get(asset.source_key.as_str())?;
            Some((key, source.verified_physical_path.as_deref()?, asset))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left_key, _, left), (right_key, _, right)| {
        left_key.cmp(right_key).then_with(|| {
            native_order(left, right)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.source_key.cmp(&right.source_key))
        })
    });
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|&(key, path, _)| standalone.contains(path) || !seen.insert((key, path)))
        .map(|(_, _, asset)| AgentDefinitionSuppression {
            declaration_id: asset.declaration_id.clone(),
            reason: crate::models::AgentAssetSuppressionReason::DuplicatePhysicalSource,
        })
        .collect()
}
