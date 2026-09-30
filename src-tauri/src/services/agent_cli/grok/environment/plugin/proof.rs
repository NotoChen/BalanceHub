//! Independently reconstruct package ownership from source and parser evidence.
mod hooks;
use super::{
    discovery, manifest, parse, Evidence, GrokPluginPayload, Parent, ParentEvidence, SourceKey,
    SourceKind, PROBE_ID,
};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetInstallState, AgentAssetInstallationOrigin, AgentAssetPresence,
    AgentAssetProviderOrigin, AgentAssetResolutionParticipation, AgentAssetScope,
    AgentAssetSourceKind, AgentAssetSuppressionReason, AgentConfigurationContext,
    AgentMcpApprovalState, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetNativePayload, AgentAssetSourcePathPolicy, AgentAssetSourceSpec,
    AgentDefinitionSelectionRequest, AgentDefinitionSuppression, ParsedAgentAsset,
};
use crate::services::agent_cli::environment::{
    availability_from_declared, declaration_matches_source, physical_origin,
};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(in crate::services::agent_cli::grok::environment) fn is_plugin_payload(
    asset: &ParsedAgentAsset,
) -> bool {
    matches!(asset.native_payload, AgentAssetNativePayload::GrokPlugin(_))
}

fn payload(asset: &ParsedAgentAsset) -> Option<&GrokPluginPayload> {
    match &asset.native_payload {
        AgentAssetNativePayload::GrokPlugin(payload) => Some(payload),
        _ => None,
    }
}

struct ParentRecord<'a> {
    asset: &'a ParsedAgentAsset,
    parent: Parent,
    descriptor: manifest::Descriptor,
    path: PathBuf,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ChildOrder {
    // Inactive JSON definitions cannot claim a native MCP merge name.
    disabled: bool,
    precedence: std::cmp::Reverse<u32>,
    namespace: String,
    command_or_inline: bool,
    root_index: usize,
    relative: PathBuf,
}

pub(in crate::services::agent_cli::grok::environment) struct PluginIndex<'a> {
    valid: BTreeSet<&'a str>,
    parents: BTreeMap<&'a str, &'a ParsedAgentAsset>,
    parent_paths: BTreeMap<&'a str, PathBuf>,
    child_order: BTreeMap<&'a str, ChildOrder>,
    child_states: BTreeMap<&'a str, AgentAssetDeclaredState>,
    parent_suppressions: BTreeSet<&'a str>,
    incomplete_mcp: BTreeSet<String>,
    incomplete_hooks: BTreeSet<String>,
}

struct Input<'a, 's> {
    context: &'s AgentConfigurationContext,
    sources: BTreeMap<&'s str, Vec<&'s AgentAssetSourceSpec>>,
    by_source: BTreeMap<&'a str, Vec<&'a ParsedAgentAsset>>,
    declaration_counts: BTreeMap<&'a str, usize>,
}

impl<'a, 's> Input<'a, 's> {
    fn source(&self, key: &str) -> Option<&'s AgentAssetSourceSpec> {
        let sources = self.sources.get(key)?;
        (sources.len() == 1).then(|| sources[0])
    }

    fn valid_source(
        &self,
        asset: &ParsedAgentAsset,
    ) -> Option<(SourceKey, &'s AgentAssetSourceSpec)> {
        let source = self.source(&asset.source_key)?;
        let key = SourceKey::decode(&asset.source_key)?;
        let base = self.source(&key.package.source)?;
        let expected = discovery::spec(base, &key)?;
        let expected_allowed_root = if key.scope() == AgentAssetScope::User {
            Path::new(&self.context.config_root)
        } else {
            Path::new(self.context.workspace_id.as_deref()?)
        };
        let expected_base_path =
            expected_allowed_root.join(if key.scope() == AgentAssetScope::User {
                "plugins"
            } else {
                ".grok/plugins"
            });
        (self.declaration_counts.get(asset.declaration_id.as_str()) == Some(&1)
            && declaration_matches_source(self.context, asset, source)
            && asset.logical_origin == physical_origin(source)
            && asset.explicitly_affected.is_empty()
            && asset.facts.is_empty()
            && base.path == expected_base_path
            && base.allowed_root == expected_allowed_root
            && base.source_kind == AgentAssetSourceKind::Directory
            && base.origin == AgentAssetInstallationOrigin::NativePackage
            && base.provider == AgentAssetProviderOrigin::Unknown
            && !base.writable
            && base.categories == [AgentAssetCategory::Plugin]
            && base.scope == key.scope()
            && base.precedence == key.precedence()
            && matches!(base.path_policy, AgentAssetSourcePathPolicy::NoFollow)
            && source.path == expected.path
            && source.allowed_root == expected.allowed_root
            && source.source_kind == expected.source_kind
            && source.origin == AgentAssetInstallationOrigin::NativePackage
            && source.provider == AgentAssetProviderOrigin::Unknown
            && source.scope == expected.scope
            && source.precedence == expected.precedence
            && source.allowed_logical_origins == expected.allowed_logical_origins
            && source.categories == expected.categories
            && !source.writable
            && source.sensitive == expected.sensitive
            && matches!(source.path_policy, AgentAssetSourcePathPolicy::NoFollow))
        .then_some((key, source))
    }

    fn probe(&self, key: &SourceKey, directory: bool) -> Option<&'a ParsedAgentAsset> {
        let declarations = self.by_source.get(key.encode().as_str())?;
        let [asset] = declarations.as_slice() else {
            return None;
        };
        let (actual, _) = self.valid_source(asset)?;
        let native = payload(asset)?;
        (*key == actual
            && asset.category == AgentAssetCategory::Plugin
            && asset.role == AgentAssetDeclarationRole::PolicyOverlay
            && asset.participation == AgentAssetResolutionParticipation::Participates
            && asset.native_id == PROBE_ID
            && asset.resolution_group_key == PROBE_ID
            && asset.label == "Grok Plugin 发现证据"
            && asset.declared_state == AgentAssetDeclaredState::Unknown
            && asset.trust_state == AgentTrustState::Unknown
            && asset.provided_by.is_none()
            && asset.action_owner.is_none()
            && asset.details
                == (AgentAssetDetails::Plugin {
                    install_state: AgentAssetInstallState::Unknown,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted: AgentTrustState::Unknown,
                })
            && if directory {
                matches!(actual.kind, SourceKind::Package)
                    && matches!(native.evidence, Evidence::PackageDirectory)
                    && asset.presence == AgentAssetPresence::Present
                    && asset.declaration_key == "package-directory"
            } else {
                matches!(actual.kind, SourceKind::Manifest { .. })
                    && matches!(native.evidence, Evidence::MissingManifest)
                    && asset.presence == AgentAssetPresence::Missing
                    && asset.declaration_key == "manifest-missing"
            })
        .then_some(*asset)
    }

    fn parent(&self, asset: &'a ParsedAgentAsset) -> Option<ParentRecord<'a>> {
        let (key, source) = self.valid_source(asset)?;
        let native = payload(asset)?;
        let descriptor = match (&key.kind, &native.evidence) {
            (SourceKind::Manifest { .. }, Evidence::Manifest(value)) => manifest::decode(value)?,
            (SourceKind::Convention { .. }, Evidence::Convention(_)) => {
                manifest::convention(&key.package.directory)?
            }
            _ => return None,
        };
        let parent = parse::parent_from_package(&key, &native.revision, &descriptor)?;
        let package_key = SourceKey {
            package: key.package.clone(),
            kind: SourceKind::Package,
        };
        let directory = self.probe(&package_key, true)?;
        if payload(directory)?.revision != parent.package_revision {
            return None;
        }
        let prior_count = match parent.evidence {
            ParentEvidence::Manifest(rank) => usize::from(rank),
            ParentEvidence::Convention(_) => 3,
        };
        for rank in 0..prior_count {
            self.probe(
                &SourceKey {
                    package: key.package.clone(),
                    kind: SourceKind::Manifest {
                        rank: rank as u8,
                        package_revision: parent.package_revision.clone(),
                    },
                },
                false,
            )?;
        }
        let trusted = parse::package_trust(key.scope(), self.context.trust_context);
        if asset.category != AgentAssetCategory::Plugin
            || asset.role != AgentAssetDeclarationRole::Definition
            || asset.participation != AgentAssetResolutionParticipation::Participates
            || asset.presence != AgentAssetPresence::Present
            || asset.declaration_key != "plugin"
            || asset.native_id != descriptor.namespace
            || asset.resolution_group_key != descriptor.namespace
            || asset.label != descriptor.namespace
            || asset.declared_state != AgentAssetDeclaredState::Unknown
            || asset.trust_state != trusted
            || asset.provided_by.is_some()
            || asset.action_owner.is_some()
            || asset.details
                != (AgentAssetDetails::Plugin {
                    install_state: AgentAssetInstallState::Installed,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted,
                })
        {
            return None;
        }
        let path = self
            .source(&directory.source_key)?
            .definition_identity_path()?
            .to_owned();
        if source.definition_identity_path()? != path.join(key.relative_path()?) {
            return None;
        }
        if matches!(&native.evidence, Evidence::Convention(Some(_)))
            && (source.source_kind != AgentAssetSourceKind::File
                || (manifest::component_path(&source.allowed_root, &descriptor.mcp_path)
                    != key.relative_path()
                    && descriptor
                        .hooks_path
                        .as_deref()
                        .and_then(|path| manifest::component_path(&source.allowed_root, path))
                        != key.relative_path()))
        {
            return None;
        }
        Some(ParentRecord {
            asset,
            parent,
            descriptor,
            path,
        })
    }

    fn child(
        &self,
        asset: &ParsedAgentAsset,
        parents: &BTreeMap<String, ParentRecord<'a>>,
    ) -> Option<(&'a ParsedAgentAsset, ChildOrder, AgentAssetDeclaredState)> {
        let (key, source) = self.valid_source(asset)?;
        let native = payload(asset)?;
        let record = match &key.kind {
            SourceKind::ComponentFile { parent, .. } => {
                let record = parents.get(&parent.source_key().encode())?;
                if *parent != record.parent {
                    return None;
                }
                record
            }
            SourceKind::Manifest { .. } | SourceKind::Convention { .. } => {
                parents.get(&asset.source_key)?
            }
            _ => return None,
        };
        let parent = &record.parent;
        if key.package != parent.package
            || asset.role != AgentAssetDeclarationRole::Definition
            || asset.presence != AgentAssetPresence::Present
            || asset.provided_by.as_ref() != Some(&parent.reference())
            || asset.action_owner.as_ref() != asset.provided_by.as_ref()
        {
            return None;
        }
        let base = self.source(&key.package.source)?;
        let package = base.path.join(&key.package.directory);
        if let SourceKind::ComponentFile { path, roles, .. } = &key.kind {
            if *roles != manifest::file_roles(&package, &record.descriptor, path)
                || (roles.skills.is_empty() && !roles.mcp && !roles.hooks)
            {
                return None;
            }
        }
        let mut order = ChildOrder {
            disabled: false,
            precedence: std::cmp::Reverse(key.precedence()),
            namespace: parent.namespace.clone(),
            command_or_inline: false,
            root_index: 0,
            relative: PathBuf::new(),
        };
        let state = match (&key.kind, &native.evidence) {
            (SourceKind::ComponentFile { path, roles, .. }, Evidence::Skill(text)) => {
                let role = roles
                    .skills
                    .iter()
                    .find(|role| asset.declaration_key == role.declaration_key())?;
                let roots = if role.command {
                    &record.descriptor.commands
                } else {
                    &record.descriptor.skills
                };
                let root = manifest::component_path(&package, roots.get(role.root_index)?)?;
                let directory = path.parent()?;
                let relative = directory.strip_prefix(&root).ok()?;
                let basename = role.basename(path, &key.package.directory);
                let (bare, label, invocation_policy) = manifest::skill(text, basename)?;
                if asset.category != AgentAssetCategory::Skill
                    || asset.declared_state != AgentAssetDeclaredState::Unknown
                    || asset.native_id != format!("{}:{bare}", parent.namespace)
                    || asset.resolution_group_key != bare
                    || asset.label != label
                    || asset.trust_state != self.context.trust_context
                    || asset.details
                        != (AgentAssetDetails::Skill {
                            enabled: AgentAssetDeclaredState::Unknown,
                            invocation_policy,
                        })
                {
                    return None;
                }
                order.command_or_inline = role.command;
                order.root_index = role.root_index;
                order.relative = if role.command {
                    path.clone()
                } else {
                    relative.to_owned()
                };
                AgentAssetDeclaredState::Enabled
            }
            (_, Evidence::Mcp(raw)) => {
                let colocated = key == parent.source_key();
                let parent_raw = match &payload(record.asset)?.evidence {
                    Evidence::Manifest(value) => Some(value),
                    Evidence::Convention(Some(value)) => Some(value.as_ref()),
                    _ => None,
                };
                if source.source_kind != AgentAssetSourceKind::File
                    || (!colocated
                        && !matches!(&key.kind, SourceKind::ComponentFile { roles, .. } if roles.mcp))
                    || (colocated && native.revision != parent.revision)
                    || (colocated && parent_raw != Some(raw.as_ref()))
                {
                    return None;
                }
                let inline = colocated
                    && matches!(key.kind, SourceKind::Manifest { .. })
                    && record.descriptor.mcp_inline.is_some();
                let value = if inline {
                    record.descriptor.mcp_inline.as_ref()?
                } else {
                    if key.relative_path()?
                        != manifest::component_path(&package, &record.descriptor.mcp_path)?
                    {
                        return None;
                    }
                    raw.as_ref()
                };
                let entries = manifest::mcp_entries(value, inline)?;
                if inline
                    && entries
                        .values()
                        .any(|entry| manifest::mcp_transport(entry).is_none())
                {
                    return None;
                }
                let entry = entries.get(&asset.native_id)?;
                let transport = manifest::mcp_transport(entry)?;
                let declared = entry.get("enabled").and_then(Value::as_bool).map_or(
                    AgentAssetDeclaredState::Unknown,
                    |enabled| {
                        if enabled {
                            AgentAssetDeclaredState::Enabled
                        } else {
                            AgentAssetDeclaredState::Disabled
                        }
                    },
                );
                if asset.category != AgentAssetCategory::Mcp
                    || asset.declared_state != declared
                    || asset.native_id.trim().is_empty()
                    || asset.resolution_group_key != asset.native_id
                    || asset.label != asset.native_id
                    || asset.declaration_key != format!("mcpServers:{}", asset.native_id)
                    || asset.trust_state
                        != parse::package_trust(key.scope(), self.context.trust_context)
                    || asset.details
                        != (AgentAssetDetails::Mcp {
                            transport,
                            declared_state: declared,
                            approval_state: AgentMcpApprovalState::NotRequired,
                            effective_availability: availability_from_declared(declared),
                        })
                {
                    return None;
                }
                order.command_or_inline = inline;
                order.disabled = entry.get("enabled") == Some(&Value::Bool(false));
                if order.disabled {
                    AgentAssetDeclaredState::Disabled
                } else {
                    AgentAssetDeclaredState::Enabled
                }
            }
            (
                _,
                Evidence::Hook {
                    raw,
                    event,
                    group,
                    handler,
                },
            ) => {
                order.command_or_inline =
                    self.hook_child(asset, &key, source, record, raw, (event, *group, *handler))?;
                AgentAssetDeclaredState::Enabled
            }
            _ => return None,
        };
        // File paths and manifest paths are tied to the same snapshot-owned
        // package identity; an alias cannot borrow a same-name parent's edge.
        if source.definition_identity_path()? != record.path.join(key.relative_path()?) {
            return None;
        }
        Some((record.asset, order, state))
    }

    fn colocated_mcp_entries<'p>(
        &self,
        record: &'p ParentRecord<'a>,
    ) -> Option<&'p serde_json::Map<String, Value>> {
        let source = self.source(&record.asset.source_key)?;
        let inline = matches!(record.parent.evidence, ParentEvidence::Manifest(_))
            && record.descriptor.mcp_inline.is_some();
        let value = if inline {
            record.descriptor.mcp_inline.as_ref()?
        } else {
            if source.source_kind != AgentAssetSourceKind::File
                || manifest::component_path(&source.allowed_root, &record.descriptor.mcp_path)
                    != record.parent.source_key().relative_path()
            {
                return None;
            }
            match &payload(record.asset)?.evidence {
                Evidence::Manifest(value) => value,
                Evidence::Convention(Some(value)) => value.as_ref(),
                _ => return None,
            }
        };
        let entries = manifest::mcp_entries(value, inline)?;
        if inline
            && entries
                .values()
                .any(|entry| manifest::mcp_transport(entry).is_none())
        {
            return None;
        }
        Some(entries)
    }

    fn incomplete_colocated_mcp(
        &self,
        parents: &BTreeMap<String, ParentRecord<'a>>,
    ) -> BTreeSet<String> {
        let mut incomplete = BTreeSet::new();
        for record in parents.values() {
            let mut actual = BTreeMap::<&str, Vec<&ParsedAgentAsset>>::new();
            for asset in self
                .by_source
                .get(record.asset.source_key.as_str())
                .into_iter()
                .flatten()
                .filter(|asset| asset.category == AgentAssetCategory::Mcp)
            {
                actual.entry(&asset.native_id).or_default().push(asset);
            }
            if let Some(entries) = self.colocated_mcp_entries(record) {
                for (native_id, entry) in entries {
                    if native_id.trim().is_empty() || manifest::mcp_transport(entry).is_none() {
                        continue;
                    }
                    let complete = actual.remove(native_id.as_str()).is_some_and(|assets| {
                        assets.len() == 1 && self.child(assets[0], parents).is_some()
                    });
                    if !complete {
                        incomplete.insert(native_id.clone());
                    }
                }
            }
            // Extra declarations are not evidence for a component entry, even
            // if their content would otherwise decode as a valid MCP.
            incomplete.extend(actual.into_keys().map(str::to_owned));
        }
        incomplete
    }
}

impl<'a> PluginIndex<'a> {
    pub(in crate::services::agent_cli::grok::environment) fn new(
        context: &AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let mut input = Input {
            context,
            sources: BTreeMap::new(),
            by_source: BTreeMap::new(),
            declaration_counts: BTreeMap::new(),
        };
        for source in sources {
            input
                .sources
                .entry(&source.native_source_key)
                .or_default()
                .push(source);
        }
        for asset in declarations {
            input
                .by_source
                .entry(&asset.source_key)
                .or_default()
                .push(asset);
            *input
                .declaration_counts
                .entry(&asset.declaration_id)
                .or_default() += 1;
        }
        let mut index = Self {
            valid: BTreeSet::new(),
            parents: BTreeMap::new(),
            parent_paths: BTreeMap::new(),
            child_order: BTreeMap::new(),
            child_states: BTreeMap::new(),
            parent_suppressions: BTreeSet::new(),
            incomplete_mcp: BTreeSet::new(),
            incomplete_hooks: BTreeSet::new(),
        };
        let mut parents = BTreeMap::new();
        for asset in declarations.iter().filter(|asset| is_plugin_payload(asset)) {
            if let Some(parent) = input.parent(asset) {
                index.valid.insert(&asset.declaration_id);
                index
                    .parent_paths
                    .insert(&asset.declaration_id, parent.path.clone());
                parents.insert(asset.source_key.clone(), parent);
            } else if let Some(key) = SourceKey::decode(&asset.source_key) {
                let directory = matches!(key.kind, SourceKind::Package);
                if input.probe(&key, directory).is_some() {
                    index.valid.insert(&asset.declaration_id);
                }
            }
        }
        let mut candidates = parents.values().collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .asset
                .logical_origin
                .precedence
                .cmp(&left.asset.logical_origin.precedence)
                .then_with(|| left.path.cmp(&right.path))
        });
        let mut selected = BTreeMap::new();
        for candidate in candidates {
            selected
                .entry(candidate.parent.namespace.clone())
                .or_insert(candidate);
        }
        index.incomplete_mcp = input.incomplete_colocated_mcp(&parents);
        index.incomplete_hooks = input.incomplete_colocated_hooks(&parents);
        for asset in declarations.iter().filter(|asset| is_plugin_payload(asset)) {
            if (asset.category == AgentAssetCategory::Mcp
                && index.incomplete_mcp.contains(&asset.native_id))
                || (asset.category == AgentAssetCategory::Hook
                    && index.incomplete_hooks.contains(&asset.native_id))
            {
                continue;
            }
            if let Some((parent, order, state)) = input.child(asset, &parents) {
                let parent_selected = selected
                    .get(&parent.native_id)
                    .is_some_and(|record| record.asset.declaration_id == parent.declaration_id);
                if parent_selected {
                    if asset.participation != AgentAssetResolutionParticipation::Participates {
                        continue;
                    }
                    index.valid.insert(&asset.declaration_id);
                    index.parents.insert(&asset.declaration_id, parent);
                    index.child_order.insert(&asset.declaration_id, order);
                    index.child_states.insert(&asset.declaration_id, state);
                } else if asset.source_key == parent.source_key
                    && matches!(
                        asset.category,
                        AgentAssetCategory::Mcp | AgentAssetCategory::Hook
                    )
                {
                    // Parsing a co-located document keeps its raw child
                    // declarations. Only complete native parent selection can
                    // exclude them; the assessor rechecks the exact same proof.
                    index.parent_suppressions.insert(&asset.declaration_id);
                    if asset.participation
                        == (AgentAssetResolutionParticipation::Suppressed {
                            reason: AgentAssetSuppressionReason::ParentNotSelected,
                        })
                    {
                        index.valid.insert(&asset.declaration_id);
                    }
                }
            }
        }
        index
    }

    pub(in crate::services::agent_cli::grok::environment) fn valid(
        &self,
        asset: &ParsedAgentAsset,
    ) -> bool {
        self.valid.contains(asset.declaration_id.as_str())
    }

    pub(in crate::services::agent_cli::grok::environment) fn incomplete_mcp(
        &self,
        native_id: &str,
    ) -> bool {
        self.incomplete_mcp.contains(native_id)
    }

    pub(in crate::services::agent_cli::grok::environment) fn incomplete_hooks(
        &self,
        native_id: &str,
    ) -> bool {
        self.incomplete_hooks.contains(native_id)
    }

    pub(in crate::services::agent_cli::grok::environment) fn parent(
        &self,
        asset: &ParsedAgentAsset,
    ) -> Option<&'a ParsedAgentAsset> {
        self.parents.get(asset.declaration_id.as_str()).copied()
    }

    pub(in crate::services::agent_cli::grok::environment) fn child_state(
        &self,
        asset: &ParsedAgentAsset,
    ) -> Option<AgentAssetDeclaredState> {
        self.child_states
            .get(asset.declaration_id.as_str())
            .copied()
    }

    pub(in crate::services::agent_cli::grok::environment) fn compare(
        &self,
        left: &ParsedAgentAsset,
        right: &ParsedAgentAsset,
    ) -> Option<Ordering> {
        if left.role != AgentAssetDeclarationRole::Definition
            || right.role != AgentAssetDeclarationRole::Definition
        {
            return None;
        }
        if left.category == AgentAssetCategory::Plugin {
            let left_path = self.parent_paths.get(left.declaration_id.as_str())?;
            let right_path = self.parent_paths.get(right.declaration_id.as_str())?;
            return Some(
                right
                    .logical_origin
                    .precedence
                    .cmp(&left.logical_origin.precedence)
                    .then_with(|| left_path.cmp(right_path)),
            );
        }
        match (
            self.child_order.get(left.declaration_id.as_str()),
            self.child_order.get(right.declaration_id.as_str()),
        ) {
            (Some(left), Some(right)) => Some(left.cmp(right)),
            (Some(_), None) if left.category == AgentAssetCategory::Mcp => Some(Ordering::Greater),
            (None, Some(_)) if left.category == AgentAssetCategory::Mcp => Some(Ordering::Less),
            _ => None,
        }
    }

    pub(in crate::services::agent_cli::grok::environment) fn ordered_winner(
        &self,
        definitions: &[&ParsedAgentAsset],
    ) -> bool {
        let Some(first) = definitions.first() else {
            return false;
        };
        self.parent_paths
            .contains_key(first.declaration_id.as_str())
            || self.child_order.contains_key(first.declaration_id.as_str())
            || (first.category == AgentAssetCategory::Mcp
                && definitions.get(1).is_some_and(|next| {
                    self.child_order.contains_key(next.declaration_id.as_str())
                }))
    }
}

pub(in crate::services::agent_cli::grok) fn definition_suppressions(
    request: AgentDefinitionSelectionRequest<'_>,
) -> Vec<AgentDefinitionSuppression> {
    PluginIndex::new(request.context, request.declarations, request.sources)
        .parent_suppressions
        .into_iter()
        .map(|declaration_id| AgentDefinitionSuppression {
            declaration_id: declaration_id.to_owned(),
            reason: AgentAssetSuppressionReason::ParentNotSelected,
        })
        .collect()
}
