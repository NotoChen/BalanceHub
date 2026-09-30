//! Build bounded typed buckets, complete policy witnesses and parent plans once.
use super::*;

pub(super) struct NativeIndex<'a> {
    pub(super) buckets: BTreeMap<BucketKey, Vec<&'a ParsedAgentAsset>>,
    pub(super) overlays: BTreeMap<(AgentAssetCategory, String), Vec<&'a ParsedAgentAsset>>,
    pub(super) allowed: Vec<(&'a ParsedAgentAsset, &'a BTreeSet<String>)>,
    pub(super) allowed_intersection: Option<BTreeSet<String>>,
    pub(super) excluded: Vec<(&'a ParsedAgentAsset, &'a BTreeSet<String>)>,
    pub(super) invalid_mcp_policy: Vec<&'a ParsedAgentAsset>,
    pub(super) invalid_mcp_enablement: Vec<&'a ParsedAgentAsset>,
    pub(super) invalid_extension_enablement: Vec<&'a ParsedAgentAsset>,
    pub(super) hook_disabled: Vec<(
        &'a ParsedAgentAsset,
        &'a BTreeSet<AgentHookDisableMatcherIdentity>,
    )>,
    pub(super) invalid_hook_policy: Vec<&'a ParsedAgentAsset>,
    pub(super) hook_global: Vec<(&'a ParsedAgentAsset, Option<bool>)>,
    pub(super) skill_disabled: Vec<(&'a ParsedAgentAsset, BTreeSet<String>)>,
    pub(super) invalid_skill_policy: Vec<&'a ParsedAgentAsset>,
    pub(super) parents: BTreeMap<String, NativeResult<NativeDecision<'a>>>,
    failures: BTreeMap<BucketKey, AgentAssetAssessmentFailure>,
    control_failures: BTreeMap<AgentAssetCategory, AgentAssetAssessmentFailure>,
    collision_groups: BTreeSet<(AgentAssetCategory, String)>,
}

pub(super) fn order(left: &ParsedAgentAsset, right: &ParsedAgentAsset) -> std::cmp::Ordering {
    right
        .logical_origin
        .precedence
        .cmp(&left.logical_origin.precedence)
        .then_with(|| left.source_key.cmp(&right.source_key))
        .then_with(|| left.declaration_key.cmp(&right.declaration_key))
        .then_with(|| left.declaration_id.cmp(&right.declaration_id))
}

pub(super) fn definitions<'a>(bucket: &[&'a ParsedAgentAsset]) -> Vec<&'a ParsedAgentAsset> {
    bucket
        .iter()
        .copied()
        .filter(|asset| {
            asset.role == AgentAssetDeclarationRole::Definition
                && asset.participation == AgentAssetResolutionParticipation::Participates
        })
        .collect()
}

pub(super) fn native_ref(asset: &ParsedAgentAsset) -> AgentAssetNativeRef {
    AgentAssetNativeRef {
        category: asset.category,
        native_id: asset.native_id.clone(),
        qualifier: Some(format!("{}:{}", asset.category.key(), asset.native_id)),
    }
}

pub(super) struct Selection<'a> {
    pub(super) anchor: &'a ParsedAgentAsset,
    pub(super) conflicts: Vec<&'a ParsedAgentAsset>,
}

impl<'a> NativeIndex<'a> {
    pub(super) fn new(
        context: &AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let mut result = Self {
            buckets: BTreeMap::new(),
            overlays: BTreeMap::new(),
            allowed: Vec::new(),
            allowed_intersection: None,
            excluded: Vec::new(),
            invalid_mcp_policy: Vec::new(),
            invalid_mcp_enablement: Vec::new(),
            invalid_extension_enablement: Vec::new(),
            hook_disabled: Vec::new(),
            invalid_hook_policy: Vec::new(),
            hook_global: Vec::new(),
            skill_disabled: Vec::new(),
            invalid_skill_policy: Vec::new(),
            parents: BTreeMap::new(),
            failures: BTreeMap::new(),
            control_failures: BTreeMap::new(),
            collision_groups: BTreeSet::new(),
        };
        let mut source_index = BTreeMap::<&str, Vec<&AgentAssetSourceSpec>>::new();
        for source in sources {
            source_index
                .entry(&source.native_source_key)
                .or_default()
                .push(source);
        }
        let mut ids = BTreeMap::<&str, usize>::new();
        for asset in declarations {
            *ids.entry(&asset.declaration_id).or_default() += 1;
        }
        for asset in declarations {
            let key = (
                asset.category,
                asset.resolution_group_key.clone(),
                asset.native_id.clone(),
            );
            if !matches!(
                asset.native_payload,
                AgentAssetNativePayload::SkillPolicy(_)
            ) {
                result.buckets.entry(key.clone()).or_default().push(asset);
            }
            let valid = ids.get(asset.declaration_id.as_str()) == Some(&1)
                && source_index
                    .get(asset.source_key.as_str())
                    .is_some_and(|sources| {
                        sources.len() == 1 && validation::valid(context, asset, sources[0])
                    });
            if !valid {
                result
                    .failures
                    .insert(key, AgentAssetAssessmentFailure::InvalidNativeInput);
                if let Some(category) = validation::control_category(asset) {
                    result
                        .control_failures
                        .insert(category, AgentAssetAssessmentFailure::InvalidNativeInput);
                }
                // An invalid target overlay invalidates its whole normalized group.
                if asset.role == AgentAssetDeclarationRole::StateOverlay
                    && validation::control_category(asset).is_none()
                {
                    result
                        .overlays
                        .entry((asset.category, asset.resolution_group_key.clone()))
                        .or_default()
                        .push(asset);
                }
                continue;
            }
            if asset.participation != AgentAssetResolutionParticipation::Participates {
                continue;
            }
            match &asset.native_payload {
                AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::Allowed(set)) => {
                    result.allowed.push((asset, set))
                }
                AgentAssetNativePayload::McpPolicy(AgentMcpPolicyPayload::Excluded(set)) => {
                    result.excluded.push((asset, set))
                }
                AgentAssetNativePayload::McpPolicy(
                    AgentMcpPolicyPayload::InvalidAllowed | AgentMcpPolicyPayload::InvalidExcluded,
                ) => result.invalid_mcp_policy.push(asset),
                AgentAssetNativePayload::InvalidControl(
                    AgentAssetInvalidControl::McpEnablement,
                ) => result.invalid_mcp_enablement.push(asset),
                AgentAssetNativePayload::InvalidControl(
                    AgentAssetInvalidControl::ExtensionEnablement,
                ) => result.invalid_extension_enablement.push(asset),
                AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::DisabledSet(set)) => {
                    result.hook_disabled.push((asset, set))
                }
                AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::InvalidDisabledSet) => {
                    result.invalid_hook_policy.push(asset)
                }
                AgentAssetNativePayload::HookPolicy(AgentHookPolicyPayload::GlobalEnabled(
                    enabled,
                )) => result.hook_global.push((asset, *enabled)),
                AgentAssetNativePayload::SkillPolicy(AgentSkillPolicyPayload::DisabledSet(set)) => {
                    result
                        .skill_disabled
                        .push((asset, set.iter().map(|name| name.to_lowercase()).collect()))
                }
                AgentAssetNativePayload::SkillPolicy(
                    AgentSkillPolicyPayload::InvalidDisabledSet,
                ) => result.invalid_skill_policy.push(asset),
                _ if asset.role == AgentAssetDeclarationRole::StateOverlay => result
                    .overlays
                    .entry((asset.category, asset.resolution_group_key.clone()))
                    .or_default()
                    .push(asset),
                _ => {}
            }
        }
        for bucket in result.buckets.values_mut() {
            bucket.sort_by(|left, right| order(left, right));
        }
        for overlays in result.overlays.values_mut() {
            overlays.sort_by(|left, right| order(left, right));
        }
        for values in [
            &mut result.invalid_mcp_policy,
            &mut result.invalid_mcp_enablement,
            &mut result.invalid_extension_enablement,
            &mut result.invalid_hook_policy,
            &mut result.invalid_skill_policy,
        ] {
            values.sort_by(|left, right| order(left, right));
        }
        result.allowed.sort_by(|left, right| order(left.0, right.0));
        result
            .hook_global
            .sort_by(|left, right| order(left.0, right.0));
        result
            .excluded
            .sort_by(|left, right| order(left.0, right.0));
        result
            .hook_disabled
            .sort_by(|left, right| order(left.0, right.0));
        result
            .skill_disabled
            .sort_by(|left, right| order(left.0, right.0));
        for (_, set) in &result.allowed {
            result.allowed_intersection = Some(match result.allowed_intersection.take() {
                None => (*set).clone(),
                Some(previous) => previous.intersection(set).cloned().collect(),
            });
        }
        let mut exact_ids = BTreeMap::<(AgentAssetCategory, String), BTreeSet<String>>::new();
        for ((category, group, id), bucket) in &result.buckets {
            if !definitions(bucket).is_empty() {
                exact_ids
                    .entry((*category, group.clone()))
                    .or_default()
                    .insert(id.clone());
            }
        }
        result.collision_groups = exact_ids
            .into_iter()
            .filter_map(|(key, ids)| (ids.len() > 1).then_some(key))
            .collect();
        let parent_targets = result
            .buckets
            .iter()
            .filter(|((category, _, _), values)| {
                *category == AgentAssetCategory::Extension && !definitions(values).is_empty()
            })
            .map(|((category, group, id), _)| AgentAssetAssessmentTarget {
                category: *category,
                resolution_group_key: group.clone(),
                exact_native_id: id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            })
            .collect::<Vec<_>>();
        for target in parent_targets {
            let decision = extension::decide(&result, &target);
            result
                .parents
                .insert(target.exact_native_id.clone(), decision);
        }
        result
    }

    pub(super) fn check(&self, target: &AgentAssetAssessmentTarget) -> NativeResult<()> {
        if let Some(failure) = self.control_failures.get(&target.category) {
            return Err(*failure);
        }
        if let Some(failure) = self.failures.get(&(
            target.category,
            target.resolution_group_key.clone(),
            target.exact_native_id.clone(),
        )) {
            return Err(*failure);
        }
        if self
            .overlays
            .get(&(target.category, target.resolution_group_key.clone()))
            .is_some_and(|overlays| {
                overlays.iter().any(|asset| {
                    self.failures.contains_key(&(
                        asset.category,
                        asset.resolution_group_key.clone(),
                        asset.native_id.clone(),
                    ))
                })
            })
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(())
    }

    pub(super) fn select(
        &self,
        definitions: &[&'a ParsedAgentAsset],
    ) -> NativeResult<Selection<'a>> {
        let mut candidates = definitions.to_vec();
        let first = *candidates
            .first()
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        if first.category == AgentAssetCategory::Mcp
            && candidates.iter().any(|asset| asset.provided_by.is_none())
        {
            candidates.retain(|asset| asset.provided_by.is_none());
        }
        candidates.sort_by(|left, right| order(left, right));
        let anchor = candidates[0];
        let peers = candidates
            .iter()
            .copied()
            .take_while(|asset| asset.logical_origin.precedence == anchor.logical_origin.precedence)
            .collect::<Vec<_>>();
        Ok(Selection {
            anchor,
            conflicts: if peers.len() > 1 { peers } else { Vec::new() },
        })
    }

    pub(super) fn targets(&self) -> Vec<AgentAssetAssessmentTarget> {
        let mut result = Vec::new();
        for ((category, group, id), bucket) in &self.buckets {
            let definitions = definitions(bucket);
            if definitions.is_empty()
                && !(*category == AgentAssetCategory::Mcp
                    && bucket
                        .iter()
                        .any(|asset| asset.role == AgentAssetDeclarationRole::Definition))
            {
                continue;
            }
            let target = AgentAssetAssessmentTarget {
                category: *category,
                resolution_group_key: group.clone(),
                exact_native_id: id.clone(),
                subject: AgentAssetAssessmentSubject::Bucket,
            };
            if *category == AgentAssetCategory::Hook && definitions.len() > 1 {
                result.extend(definitions.iter().map(|asset| AgentAssetAssessmentTarget {
                    subject: AgentAssetAssessmentSubject::Definition(asset.declaration_id.clone()),
                    ..target.clone()
                }));
            } else {
                result.push(target.clone());
                if let Ok(selection) = self.select(&definitions) {
                    if selection.conflicts.is_empty() {
                        result.extend(
                            definitions
                                .iter()
                                .filter(|asset| {
                                    asset.declaration_id != selection.anchor.declaration_id
                                })
                                .map(|asset| AgentAssetAssessmentTarget {
                                    subject: AgentAssetAssessmentSubject::Definition(
                                        asset.declaration_id.clone(),
                                    ),
                                    ..target.clone()
                                }),
                        );
                    }
                }
            }
        }
        result
    }

    pub(super) fn decide(
        &self,
        target: &AgentAssetAssessmentTarget,
    ) -> NativeResult<NativeDecision<'a>> {
        match target.category {
            AgentAssetCategory::Mcp => mcp::decide(self, target),
            AgentAssetCategory::Extension => {
                if target.subject == AgentAssetAssessmentSubject::Bucket {
                    self.parents
                        .get(&target.exact_native_id)
                        .cloned()
                        .unwrap_or(Err(AgentAssetAssessmentFailure::MissingTarget))
                } else {
                    extension::decide(self, target)
                }
            }
            AgentAssetCategory::Hook => hook::decide(self, target),
            AgentAssetCategory::Skill => skill::decide(self, target),
            AgentAssetCategory::StatusUi => remaining::decide(self, target),
            AgentAssetCategory::Plugin => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        }
    }

    pub(super) fn collision(&self, target: &AgentAssetAssessmentTarget) -> bool {
        self.collision_groups
            .contains(&(target.category, target.resolution_group_key.clone()))
    }

    pub(super) fn parent_state(
        &self,
        reference: &AgentAssetNativeRef,
    ) -> NativeResult<AgentAssetState> {
        if reference.category != AgentAssetCategory::Extension
            || reference.qualifier.as_deref()
                != Some(format!("extension:{}", reference.native_id).as_str())
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let decision = self
            .parents
            .get(&reference.native_id)
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?
            .as_ref()
            .map_err(|failure| *failure)?;
        if decision.resolution.relation == AgentAssetResolutionRelation::Unknown {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        decision.state()
    }
}
