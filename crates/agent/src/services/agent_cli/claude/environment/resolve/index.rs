//! Completeness and identity are checked before any native absence/default rule.
use super::super::plugin::{source_bindings, SourceBinding, PREFIX};
use super::*;
use crate::services::agent_cli::environment::{declaration_matches_source, physical_origin};

pub(super) struct ControlList<'a> {
    pub(super) root: &'a ParsedAgentAsset,
    pub(super) state: ListState,
    pub(super) entries: Vec<&'a ParsedAgentAsset>,
}
impl<'a> ControlList<'a> {
    pub(super) fn members(&self) -> Vec<&'a ParsedAgentAsset> {
        std::iter::once(self.root)
            .chain(self.entries.iter().copied())
            .collect()
    }
}

pub(super) struct ControlSource<'a> {
    pub(super) spec: &'a AgentAssetSourceSpec,
    pub(super) root: &'a ParsedAgentAsset,
    pub(super) state: SourceState,
    pub(super) lists: BTreeMap<ListFamily, ControlList<'a>>,
    pub(super) booleans: BTreeMap<BooleanField, (&'a ParsedAgentAsset, BooleanValue)>,
    pub(super) plugins: Option<ControlList<'a>>,
    pub(super) namespace: Option<(&'a ParsedAgentAsset, &'a NamespaceState)>,
}

pub(super) struct NativeIndex<'a> {
    pub(super) context: &'a AgentConfigurationContext,
    pub(super) sources: &'a [AgentAssetSourceSpec],
    pub(super) buckets: BTreeMap<BucketKey, Vec<&'a ParsedAgentAsset>>,
    pub(super) ambiguous_plugin_skill_ids: BTreeSet<String>,
    pub(super) plugin_bindings: BTreeMap<&'a str, Option<Vec<SourceBinding>>>,
    controls: BTreeMap<AgentAssetCategory, Vec<ControlSource<'a>>>,
    failures: BTreeMap<AgentAssetCategory, AgentAssetAssessmentFailure>,
}

fn source_is_active(context: &AgentConfigurationContext, source: &AgentAssetSourceSpec) -> bool {
    context.trust_context == AgentTrustState::Trusted
        || !matches!(
            source.native_source_key.as_str(),
            "workspace-settings"
                | "workspace-local-settings"
                | "workspace-mcp"
                | "workspace-skills"
        )
}

fn controls_category(source: &AgentAssetSourceSpec, category: AgentAssetCategory) -> bool {
    source.categories.contains(&category)
        && (SETTINGS_AUTHORITIES.contains(&source.native_source_key.as_str())
            || category == AgentAssetCategory::Mcp
                && matches!(source.native_source_key.as_str(), "account" | "managed-mcp"))
}

impl<'a> NativeIndex<'a> {
    pub(super) fn new(
        context: &'a AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &'a [AgentAssetSourceSpec],
    ) -> Self {
        let mut buckets = BTreeMap::<BucketKey, Vec<&ParsedAgentAsset>>::new();
        let mut by_source = BTreeMap::<(&str, AgentAssetCategory), Vec<&ParsedAgentAsset>>::new();
        let mut ids = BTreeMap::<&str, usize>::new();
        let mut source_keys = BTreeMap::<&str, usize>::new();
        let mut failures = BTreeMap::new();
        for source in sources {
            *source_keys.entry(&source.native_source_key).or_default() += 1;
        }
        for declaration in declarations {
            *ids.entry(&declaration.declaration_id).or_default() += 1;
            buckets
                .entry((
                    declaration.category,
                    declaration.resolution_group_key.clone(),
                    declaration.native_id.clone(),
                ))
                .or_default()
                .push(declaration);
            if matches!(
                declaration.native_payload,
                AgentAssetNativePayload::ClaudeControl(_)
            ) {
                by_source
                    .entry((&declaration.source_key, declaration.category))
                    .or_default()
                    .push(declaration);
                if !sources.iter().any(|source| {
                    source.native_source_key == declaration.source_key
                        && controls_category(source, declaration.category)
                }) {
                    failures.insert(
                        declaration.category,
                        AgentAssetAssessmentFailure::InvalidNativeInput,
                    );
                }
            }
        }
        for bucket in buckets.values_mut() {
            bucket.sort_by(|left, right| order(left, right));
        }
        let mut controls = BTreeMap::<AgentAssetCategory, Vec<ControlSource<'a>>>::new();
        for source in sources {
            if !source_is_active(context, source) {
                continue;
            }
            for category in [
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Plugin,
                AgentAssetCategory::Hook,
                AgentAssetCategory::StatusUi,
            ] {
                if !controls_category(source, category) {
                    continue;
                }
                let members = by_source
                    .get(&(source.native_source_key.as_str(), category))
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let result = if source_keys.get(source.native_source_key.as_str()) != Some(&1) {
                    Err(AgentAssetAssessmentFailure::InvalidNativeInput)
                } else {
                    ControlSource::build(context, source, category, members, &ids)
                };
                match result {
                    Ok(value) => controls.entry(category).or_default().push(value),
                    Err(failure) => {
                        failures
                            .entry(category)
                            .and_modify(|previous| {
                                if failure == AgentAssetAssessmentFailure::InvalidNativeInput {
                                    *previous = failure;
                                }
                            })
                            .or_insert(failure);
                    }
                }
            }
        }
        let mut index = Self {
            context,
            sources,
            buckets,
            ambiguous_plugin_skill_ids: BTreeSet::new(),
            plugin_bindings: sources
                .iter()
                .filter(|source| source.native_source_key.starts_with(PREFIX))
                .map(|source| (source.native_source_key.as_str(), source_bindings(source)))
                .collect(),
            controls,
            failures,
        };
        // Parent decisions are independent of Skill deduplication. Evaluate the
        // physical candidate set once, then share it across both decision users.
        index.ambiguous_plugin_skill_ids = index.unordered_plugin_skill_duplicate_ids();
        index
    }

    pub(super) fn controls(
        &self,
        category: AgentAssetCategory,
    ) -> NativeResult<&[ControlSource<'a>]> {
        if let Some(failure) = self.failures.get(&category) {
            return Err(*failure);
        }
        Ok(self
            .controls
            .get(&category)
            .map(Vec::as_slice)
            .unwrap_or_default())
    }
}

fn payload(asset: &ParsedAgentAsset) -> NativeResult<&ClaudeControlPayload> {
    match &asset.native_payload {
        AgentAssetNativePayload::ClaudeControl(value) => Ok(value),
        _ => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    }
}

impl<'a> ControlSource<'a> {
    fn build(
        context: &AgentConfigurationContext,
        spec: &'a AgentAssetSourceSpec,
        category: AgentAssetCategory,
        members: &[&'a ParsedAgentAsset],
        ids: &BTreeMap<&str, usize>,
    ) -> NativeResult<Self> {
        for member in members {
            if ids.get(member.declaration_id.as_str()) != Some(&1)
                || !declaration_matches_source(context, member, spec)
                || !control_identity(context, member, spec, category)?
            {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
        }
        let roots = members
            .iter()
            .copied()
            .filter(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::ClaudeControl(ClaudeControlPayload::SourceRoot { .. })
                )
            })
            .collect::<Vec<_>>();
        let root = match roots.as_slice() {
            [] => return Err(AgentAssetAssessmentFailure::IncompleteInput),
            [root] => *root,
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        let ClaudeControlPayload::SourceRoot { state } = payload(root)? else {
            unreachable!()
        };
        if *state != SourceState::Readable {
            if members.len() != 1 {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            return Ok(Self {
                spec,
                root,
                state: *state,
                lists: BTreeMap::new(),
                booleans: BTreeMap::new(),
                plugins: None,
                namespace: None,
            });
        }
        let key = spec.native_source_key.as_str();
        let list_families: &[ListFamily] = if key == "account" {
            &[ListFamily::PersonalDisabled, ListFamily::PersonalEnabled]
        } else if category == AgentAssetCategory::Mcp && SETTINGS_AUTHORITIES.contains(&key) {
            &[
                ListFamily::Allowed,
                ListFamily::Denied,
                ListFamily::Approved,
                ListFamily::Rejected,
            ]
        } else {
            &[]
        };
        let boolean_fields: &[BooleanField] = match (category, key == "managed-settings") {
            (AgentAssetCategory::Mcp, true) => {
                &[BooleanField::ApproveAll, BooleanField::ManagedMcpOnly]
            }
            (AgentAssetCategory::Mcp, false) if SETTINGS_AUTHORITIES.contains(&key) => {
                &[BooleanField::ApproveAll]
            }
            (AgentAssetCategory::Hook, true) => &[
                BooleanField::DisableAllHooks,
                BooleanField::ManagedHooksOnly,
            ],
            (AgentAssetCategory::Hook | AgentAssetCategory::StatusUi, _) => {
                &[BooleanField::DisableAllHooks]
            }
            _ => &[],
        };
        let mut consumed = BTreeSet::from([root.declaration_id.as_str()]);
        let mut lists = BTreeMap::new();
        for family in list_families {
            let list_roots = members
                .iter()
                .copied()
                .filter(|asset| {
                    matches!(payload(asset),
                Ok(ClaudeControlPayload::ListRoot { family: actual, .. }) if actual == family)
                })
                .collect::<Vec<_>>();
            let list_root = unique(&list_roots)?;
            let ClaudeControlPayload::ListRoot {
                state,
                expected_entry_count,
                ..
            } = payload(list_root)?
            else {
                unreachable!()
            };
            let mut entries = members
                .iter()
                .copied()
                .filter(|asset| match payload(asset) {
                    Ok(
                        ClaudeControlPayload::RuleEntry { family: actual, .. }
                        | ClaudeControlPayload::NameEntry { family: actual, .. },
                    ) => actual == family,
                    _ => false,
                })
                .collect::<Vec<_>>();
            validate_count(entries.len(), *expected_entry_count)?;
            entries.sort_by_key(|asset| ordinal(asset).unwrap_or(usize::MAX));
            for (expected, asset) in entries.iter().enumerate() {
                if ordinal(asset) != Some(expected) {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
                if matches!(payload(asset)?, ClaudeControlPayload::RuleEntry { .. })
                    != family.policy()
                {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            let invalid_member = entries.iter().any(|asset| match payload(asset) {
                Ok(ClaudeControlPayload::RuleEntry { matcher, .. }) => matcher.is_none(),
                Ok(ClaudeControlPayload::NameEntry { name, .. }) => name.is_none(),
                _ => true,
            });
            let valid_shape = match state {
                ListState::Absent => entries.is_empty(),
                ListState::Valid => !invalid_member,
                ListState::Invalid => {
                    *family != ListFamily::Allowed && (entries.is_empty() || invalid_member)
                }
                ListState::NativeFailClosed => {
                    *family == ListFamily::Allowed && (entries.is_empty() || invalid_member)
                }
            };
            if !valid_shape {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            consumed.insert(list_root.declaration_id.as_str());
            consumed.extend(entries.iter().map(|asset| asset.declaration_id.as_str()));
            lists.insert(
                *family,
                ControlList {
                    root: list_root,
                    state: *state,
                    entries,
                },
            );
        }
        let mut booleans = BTreeMap::new();
        for field in boolean_fields {
            let entries = members
                .iter()
                .copied()
                .filter(|asset| {
                    matches!(payload(asset),
                Ok(ClaudeControlPayload::Boolean { field: actual, .. }) if actual == field)
                })
                .collect::<Vec<_>>();
            let asset = unique(&entries)?;
            let ClaudeControlPayload::Boolean { value, .. } = payload(asset)? else {
                unreachable!()
            };
            if *value == BooleanValue::NativeFailClosed && !field.managed()
                || *value == BooleanValue::Invalid && field.managed()
            {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            consumed.insert(asset.declaration_id.as_str());
            booleans.insert(*field, (asset, *value));
        }
        let plugins = if category == AgentAssetCategory::Plugin {
            let roots = members
                .iter()
                .copied()
                .filter(|asset| {
                    matches!(payload(asset), Ok(ClaudeControlPayload::PluginRoot { .. }))
                })
                .collect::<Vec<_>>();
            let plugin_root = unique(&roots)?;
            let ClaudeControlPayload::PluginRoot {
                state,
                expected_entry_count,
            } = payload(plugin_root)?
            else {
                unreachable!()
            };
            let entries = members
                .iter()
                .copied()
                .filter(|asset| {
                    matches!(payload(asset), Ok(ClaudeControlPayload::PluginEntry { .. }))
                })
                .collect::<Vec<_>>();
            validate_count(entries.len(), *expected_entry_count)?;
            if !matches!(state, ListState::Valid)
                && (!entries.is_empty() || *state == ListState::NativeFailClosed)
                || entries
                    .iter()
                    .map(|entry| &entry.native_id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != entries.len()
            {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            consumed.insert(plugin_root.declaration_id.as_str());
            consumed.extend(entries.iter().map(|entry| entry.declaration_id.as_str()));
            Some(ControlList {
                root: plugin_root,
                state: *state,
                entries,
            })
        } else {
            None
        };
        let namespace = if key == "managed-mcp" {
            let entries = members
                .iter()
                .copied()
                .filter(|asset| {
                    matches!(
                        payload(asset),
                        Ok(ClaudeControlPayload::ManagedNamespace { .. })
                    )
                })
                .collect::<Vec<_>>();
            let asset = unique(&entries)?;
            let ClaudeControlPayload::ManagedNamespace { state } = payload(asset)? else {
                unreachable!()
            };
            consumed.insert(asset.declaration_id.as_str());
            Some((asset, state))
        } else {
            None
        };
        if consumed.len() != members.len() {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(Self {
            spec,
            root,
            state: *state,
            lists,
            booleans,
            plugins,
            namespace,
        })
    }
}

fn unique<'a>(values: &[&'a ParsedAgentAsset]) -> NativeResult<&'a ParsedAgentAsset> {
    match values {
        [] => Err(AgentAssetAssessmentFailure::IncompleteInput),
        [value] => Ok(*value),
        _ => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
    }
}
fn validate_count(actual: usize, expected: usize) -> NativeResult<()> {
    match actual.cmp(&expected) {
        std::cmp::Ordering::Less => Err(AgentAssetAssessmentFailure::IncompleteInput),
        std::cmp::Ordering::Greater => Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        std::cmp::Ordering::Equal => Ok(()),
    }
}
fn ordinal(asset: &ParsedAgentAsset) -> Option<usize> {
    match payload(asset).ok()? {
        ClaudeControlPayload::RuleEntry { ordinal, .. }
        | ClaudeControlPayload::NameEntry { ordinal, .. } => Some(*ordinal),
        _ => None,
    }
}

fn control_identity(
    context: &AgentConfigurationContext,
    asset: &ParsedAgentAsset,
    spec: &AgentAssetSourceSpec,
    category: AgentAssetCategory,
) -> NativeResult<bool> {
    if asset.category != category || asset.resolution_group_key != asset.native_id {
        return Ok(false);
    }
    let mut origin = physical_origin(spec);
    let (role, key, native_id) = match payload(asset)? {
        ClaudeControlPayload::SourceRoot { .. } => {
            let key = format!("control:{}:source", category.key());
            (source_role(category), key.clone(), key)
        }
        ClaudeControlPayload::ListRoot { family, .. } => {
            if category != AgentAssetCategory::Mcp {
                return Ok(false);
            }
            if family.personal() {
                origin = AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::Local,
                    precedence: 30,
                };
            }
            let key = format!("control:mcp:{}:root", family.field());
            (family.role(), key.clone(), key)
        }
        ClaudeControlPayload::RuleEntry {
            family,
            ordinal,
            matcher,
        } => {
            if category != AgentAssetCategory::Mcp || !family.policy() {
                return Ok(false);
            }
            let id = match matcher {
                Some(PolicyMatcher::Name(name)) => name.clone(),
                _ => format!("control:{}:{ordinal}", family.field()),
            };
            (
                family.role(),
                format!("policy:{}:{ordinal}:{id}", family.field()),
                id,
            )
        }
        ClaudeControlPayload::NameEntry {
            family,
            ordinal,
            name,
        } => {
            if category != AgentAssetCategory::Mcp || family.policy() {
                return Ok(false);
            }
            if family.personal() {
                origin = AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::Local,
                    precedence: 30,
                };
            }
            let id = name
                .clone()
                .unwrap_or_else(|| format!("control:{}:{ordinal}", family.field()));
            let prefix = if family.personal() {
                "state"
            } else {
                "approval"
            };
            (
                family.role(),
                format!("{prefix}:{}:{ordinal}:{id}", family.field()),
                id,
            )
        }
        ClaudeControlPayload::Boolean { field, .. } => {
            let id = format!("control:{}:{}", category.key(), field.field());
            (AgentAssetDeclarationRole::PolicyOverlay, id.clone(), id)
        }
        ClaudeControlPayload::PluginRoot { .. } => {
            if category != AgentAssetCategory::Plugin {
                return Ok(false);
            }
            let id = "control:plugin:enabledPlugins:root".to_owned();
            (AgentAssetDeclarationRole::StateOverlay, id.clone(), id)
        }
        ClaudeControlPayload::PluginEntry { .. } => {
            if category != AgentAssetCategory::Plugin {
                return Ok(false);
            }
            (
                AgentAssetDeclarationRole::StateOverlay,
                format!("state:{}", asset.native_id),
                asset.native_id.clone(),
            )
        }
        ClaudeControlPayload::ManagedNamespace { .. } => {
            if category != AgentAssetCategory::Mcp {
                return Ok(false);
            }
            let id = "control:mcp:managed-namespace".to_owned();
            (AgentAssetDeclarationRole::PolicyOverlay, id.clone(), id)
        }
    };
    let personal = matches!(payload(asset)?, ClaudeControlPayload::ListRoot { family, .. } | ClaudeControlPayload::NameEntry { family, .. } if family.personal());
    let participation = if personal && context.trust_context != AgentTrustState::Trusted {
        AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        AgentAssetResolutionParticipation::Participates
    };
    Ok(asset.role == role
        && asset.logical_origin == origin
        && asset.declaration_key == key
        && asset.native_id == native_id
        && asset.participation == participation)
}
