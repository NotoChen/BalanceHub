//! Package observations are load evidence, never an enabledPlugins switch.
use super::super::plugin::{
    self, ClaudePluginPayload as Payload, Manifest, PackageKey, RegisteredPlugin, SourceRole,
};
use super::*;
use crate::models::{
    AgentAssetInstallState, AgentAssetInstallationOrigin, AgentAssetPresence, AgentAssetSourceKind,
    AgentAssetState,
};
use crate::services::agent_cli::contracts::{AgentAssetDirectoryEntry, AgentAssetSourcePathPolicy};
use crate::services::agent_cli::environment::{declaration_matches_source, physical_origin};
use std::path::Path;

mod duplicates;
mod hooks;
pub(in crate::services::agent_cli::claude) use duplicates::definition_suppressions;

pub(super) fn native_order(
    left: &ParsedAgentAsset,
    right: &ParsedAgentAsset,
) -> Option<std::cmp::Ordering> {
    match (&left.native_payload, &right.native_payload) {
        (
            AgentAssetNativePayload::ClaudePlugin(Payload::Mcp {
                key: a, ordinal: x, ..
            }),
            AgentAssetNativePayload::ClaudePlugin(Payload::Mcp {
                key: b, ordinal: y, ..
            }),
        ) if a == b => Some(y.cmp(x)),
        (
            AgentAssetNativePayload::ClaudePlugin(Payload::Skill {
                key: a,
                root_index: x,
                mode: m,
                ..
            }),
            AgentAssetNativePayload::ClaudePlugin(Payload::Skill {
                key: b,
                root_index: y,
                mode: n,
                ..
            }),
        ) if a == b => {
            let (command_left, ordinal_left) = skill_mode_order(m);
            let (command_right, ordinal_right) = skill_mode_order(n);
            Some((command_left, x, ordinal_left).cmp(&(command_right, y, ordinal_right)))
        }
        _ => None,
    }
}

fn skill_mode_order(mode: &str) -> (bool, usize) {
    let command = mode.starts_with("command");
    let ordinal = mode
        .split_once('-')
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    (command, ordinal)
}

impl<'a> NativeIndex<'a> {
    fn package_source(&self, asset: &ParsedAgentAsset) -> NativeResult<&'a AgentAssetSourceSpec> {
        let mut sources = self
            .sources
            .iter()
            .filter(|source| source.native_source_key == asset.source_key);
        let source = sources
            .next()
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        if sources.next().is_some() || !declaration_matches_source(self.context, asset, source) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(source)
    }

    fn registry_entry(
        &self,
        key: &PackageKey,
    ) -> NativeResult<(&'a ParsedAgentAsset, &'a RegisteredPlugin)> {
        let bucket = self
            .buckets
            .get(&(AgentAssetCategory::Plugin, key.id.clone(), key.id.clone()))
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        let definitions = definitions(bucket);
        if tied(&definitions) {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        }
        let anchor = *definitions
            .first()
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        let AgentAssetNativePayload::ClaudePlugin(Payload::Registry(entry)) =
            &anchor.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let source = self.package_source(anchor)?;
        if entry.key != *key
            || anchor.source_key != "plugin-registry"
            || anchor.declaration_key != format!("registry:{}", key.occurrence)
            || anchor.native_id != key.id
            || anchor.logical_origin != entry.origin
            || source.path
                != Path::new(&self.context.config_root).join("plugins/installed_plugins.json")
            || anchor.provided_by.is_some()
            || anchor.action_owner.is_some()
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok((anchor, entry))
    }

    fn metadata(
        &self,
        entry: &RegisteredPlugin,
        role: &str,
    ) -> NativeResult<Option<&'a ParsedAgentAsset>> {
        let declaration_key = format!("package.{role}:{}", entry.key.occurrence);
        let mut matches = self.buckets.values().flatten().copied().filter(|asset| {
            asset.declaration_key == declaration_key
                && asset.native_id == entry.key.id
                && asset.category == AgentAssetCategory::Plugin
        });
        let Some(asset) = matches.next() else {
            return Ok(None);
        };
        let source = self.package_source(asset)?;
        let path = if role == "root" {
            entry.path.clone()
        } else {
            entry.path.join(".claude-plugin/plugin.json")
        };
        let expected_role = if role == "root" {
            SourceRole::Root
        } else {
            SourceRole::Manifest
        };
        let binding_valid = self
            .plugin_bindings
            .get(source.native_source_key.as_str())
            .and_then(Option::as_deref)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.key == entry.key
                        && binding.origin() == entry.origin
                        && binding.role == expected_role
                })
            });
        if matches.next().is_some()
            || !binding_valid
            || source.path != path
            || source.allowed_root != entry.path
            || source.writable
            || source.path_policy != AgentAssetSourcePathPolicy::NoFollow
            || source.origin != AgentAssetInstallationOrigin::NativePackage
            || source.source_kind
                != if role == "root" {
                    AgentAssetSourceKind::Directory
                } else {
                    AgentAssetSourceKind::File
                }
            || !source.allows(entry.origin)
            || asset.logical_origin != entry.origin
            || asset.role != AgentAssetDeclarationRole::StateOverlay
            || asset.participation != AgentAssetResolutionParticipation::Participates
            || asset.declared_state != AgentAssetDeclaredState::Unknown
            || asset.native_id != entry.key.id
            || asset.resolution_group_key != entry.key.id
            || asset.declaration_key != format!("package.{role}:{}", entry.key.occurrence)
            || asset.provided_by.is_some()
            || asset.action_owner.is_some()
            || asset.trust_state != AgentTrustState::Unknown
            || !asset.explicitly_affected.is_empty()
            || !matches!(
                asset.details,
                AgentAssetDetails::Plugin {
                    install_state: AgentAssetInstallState::Unknown,
                    enabled: AgentAssetDeclaredState::Unknown,
                    trusted: AgentTrustState::Unknown,
                }
            )
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let valid_payload = match (&asset.native_payload, role) {
            (
                AgentAssetNativePayload::ClaudePlugin(Payload::Root {
                    key,
                    entries,
                    missing,
                }),
                "root",
            ) => {
                *key == entry.key
                    && if *missing {
                        entries.is_none() && asset.presence == AgentAssetPresence::Missing
                    } else {
                        asset.presence != AgentAssetPresence::Missing
                            && (entries.is_none() || asset.presence == AgentAssetPresence::Present)
                    }
            }
            (
                AgentAssetNativePayload::ClaudePlugin(Payload::Manifest { key, value }),
                "manifest",
            ) => {
                *key == entry.key
                    && (value.is_none()
                        || matches!(
                            asset.presence,
                            AgentAssetPresence::Present | AgentAssetPresence::Missing
                        ))
            }
            _ => false,
        };
        if !valid_payload {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(Some(asset))
    }

    fn package(
        &self,
        key: &PackageKey,
    ) -> NativeResult<(
        &'a RegisteredPlugin,
        &'a [AgentAssetDirectoryEntry],
        &'a Manifest,
    )> {
        let (_, entry) = self.registry_entry(key)?;
        let root = self
            .metadata(entry, "root")?
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        let manifest = self
            .metadata(entry, "manifest")?
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        match (&root.native_payload, &manifest.native_payload) {
            (
                AgentAssetNativePayload::ClaudePlugin(Payload::Root {
                    key: root_key,
                    entries: Some(entries),
                    missing: false,
                }),
                AgentAssetNativePayload::ClaudePlugin(Payload::Manifest {
                    key: manifest_key,
                    value: Some(descriptor),
                }),
            ) if root_key == key
                && manifest_key == key
                && root.presence == AgentAssetPresence::Present
                && matches!(
                    manifest.presence,
                    AgentAssetPresence::Present | AgentAssetPresence::Missing
                ) =>
            {
                Ok((entry, entries, descriptor))
            }
            _ => Err(AgentAssetAssessmentFailure::IncompleteInput),
        }
    }

    pub(super) fn plugin_installation(
        &self,
        anchor: &ParsedAgentAsset,
        assessment: &mut AgentAssetNativeAssessment,
    ) -> NativeResult<()> {
        let AgentAssetNativePayload::ClaudePlugin(Payload::Registry(entry)) =
            &anchor.native_payload
        else {
            return Ok(());
        };
        let source = self.package_source(anchor)?;
        if anchor.source_key != "plugin-registry"
            || source.path
                != Path::new(&self.context.config_root).join("plugins/installed_plugins.json")
            || source.source_kind != AgentAssetSourceKind::File
            || anchor.role != AgentAssetDeclarationRole::Definition
            || anchor.declaration_key != format!("registry:{}", entry.key.occurrence)
            || anchor.native_id != entry.key.id
            || anchor.resolution_group_key != entry.key.id
            || anchor.logical_origin != entry.origin
            || anchor.provided_by.is_some()
            || anchor.action_owner.is_some()
            || anchor.declared_state != AgentAssetDeclaredState::Unknown
            || anchor.trust_state != AgentTrustState::Unknown
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let installed = match self.metadata(entry, "root")? {
            Some(root) => match &root.native_payload {
                AgentAssetNativePayload::ClaudePlugin(Payload::Root {
                    key,
                    entries: Some(_),
                    missing: false,
                }) if *key == entry.key && root.presence == AgentAssetPresence::Present => {
                    match self.metadata(entry, "manifest")? {
                        Some(manifest) if matches!(&manifest.native_payload, AgentAssetNativePayload::ClaudePlugin(Payload::Manifest { key, value: Some(_) }) if *key == entry.key) => {
                            AgentAssetInstallState::Installed
                        }
                        _ => AgentAssetInstallState::Unknown,
                    }
                }
                AgentAssetNativePayload::ClaudePlugin(Payload::Root {
                    key,
                    missing: true,
                    entries: None,
                }) if *key == entry.key && root.presence == AgentAssetPresence::Missing => {
                    AgentAssetInstallState::NotInstalled
                }
                _ => AgentAssetInstallState::Unknown,
            },
            None => AgentAssetInstallState::Unknown,
        };
        let mut details = assessment.intrinsic.details.clone();
        if let AgentAssetDetails::Plugin { install_state, .. } = &mut details {
            *install_state = installed;
        }
        assessment.intrinsic = intrinsic_basis(details, assessment.intrinsic.trust_state);
        Ok(())
    }

    pub(super) fn validate_plugin_child(
        &self,
        asset: &ParsedAgentAsset,
    ) -> NativeResult<Option<&'a RegisteredPlugin>> {
        let (key, namespace) = match &asset.native_payload {
            AgentAssetNativePayload::ClaudePlugin(
                Payload::Skill { key, namespace, .. }
                | Payload::Mcp { key, namespace, .. }
                | Payload::Hook { key, namespace, .. },
            ) => (key, namespace),
            _ => {
                return if asset.provided_by.is_some() || asset.action_owner.is_some() {
                    Err(AgentAssetAssessmentFailure::InvalidNativeInput)
                } else {
                    Ok(None)
                }
            }
        };
        let (entry, entries, manifest) = self.package(key)?;
        let source = self.package_source(asset)?;
        let parent = plugin::parent_ref(key);
        if namespace != &manifest.namespace
            || source.allowed_root != entry.path
            || source.writable
            || source.path_policy != AgentAssetSourcePathPolicy::NoFollow
            || source.origin != AgentAssetInstallationOrigin::NativePackage
            || source.source_kind != AgentAssetSourceKind::File
            || asset.logical_origin != entry.origin
            || !source.allows(entry.origin)
            || asset.role != AgentAssetDeclarationRole::Definition
            || asset.presence != AgentAssetPresence::Present
            || asset.participation != AgentAssetResolutionParticipation::Participates
            || asset.trust_state != self.context.trust_context
            || asset.provided_by.as_ref() != Some(&parent)
            || asset.action_owner.as_ref() != Some(&parent)
            || !asset.explicitly_affected.is_empty()
            || asset.resolution_group_key != asset.native_id
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let bindings = self
            .plugin_bindings
            .get(source.native_source_key.as_str())
            .and_then(Option::as_deref)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let binding_matches = |role: &SourceRole| {
            bindings.iter().any(|binding| {
                binding.key == *key && binding.origin() == entry.origin && &binding.role == role
            })
        };
        match &asset.native_payload {
            AgentAssetNativePayload::ClaudePlugin(Payload::Skill {
                root_index,
                mode,
                frontmatter_name,
                valid,
                ..
            }) => {
                let role = SourceRole::Skill {
                    namespace: namespace.clone(),
                    root_index: *root_index,
                    mode: mode.clone(),
                };
                let valid_path = if mode.starts_with("command") {
                    let commands = manifest
                        .commands
                        .clone()
                        .unwrap_or_else(|| vec!["commands".into()]);
                    commands.get(*root_index).is_some_and(|relative| {
                        let path = entry.path.join(relative);
                        if path.extension().is_some_and(|extension| extension == "md") {
                            mode == "command" && path == source.path
                        } else {
                            mode.starts_with("command-")
                                && source.path.parent() == Some(path.as_path())
                                && source
                                    .path
                                    .extension()
                                    .is_some_and(|extension| extension == "md")
                        }
                    })
                } else {
                    plugin::skill_roots(&entry.path, entries, manifest)
                        .get(*root_index)
                        .is_some_and(|root| {
                            (mode == "root" && source.path == root.join("SKILL.md"))
                                || (mode.starts_with("child-")
                                    && source.path.parent().and_then(Path::parent)
                                        == Some(root.as_path())
                                    && source
                                        .path
                                        .file_name()
                                        .is_some_and(|name| name == "SKILL.md"))
                        })
                };
                let state = if *valid {
                    AgentAssetDeclaredState::Enabled
                } else {
                    AgentAssetDeclaredState::Unknown
                };
                if !binding_matches(&role)
                    || !valid_path
                    || asset.category != AgentAssetCategory::Skill
                    || plugin::skill_name(
                        namespace,
                        &source.path,
                        mode,
                        frontmatter_name.as_deref(),
                    )
                    .as_deref()
                        != Some(asset.native_id.as_str())
                    || asset.declaration_key
                        != format!("plugin.skill:{}:{root_index}:{mode}", key.occurrence)
                    || asset.declared_state != state
                    || !matches!(asset.details, AgentAssetDetails::Skill { enabled, .. } if enabled == state)
                {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            AgentAssetNativePayload::ClaudePlugin(Payload::Mcp {
                ordinal,
                value,
                definition,
                ..
            }) => {
                let expected_path = if *ordinal == 0 {
                    Some(entry.path.join(".mcp.json"))
                } else {
                    manifest
                        .mcp
                        .get(ordinal - 1)
                        .and_then(|component| match component {
                            plugin::McpComponent::File(path) => Some(entry.path.join(path)),
                            plugin::McpComponent::Inline(_) => {
                                Some(entry.path.join(".claude-plugin/plugin.json"))
                            }
                            plugin::McpComponent::Unobserved | plugin::McpComponent::Invalid => {
                                None
                            }
                        })
                };
                let prefix = format!("plugin:{namespace}:");
                let name = asset
                    .native_id
                    .strip_prefix(&prefix)
                    .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
                let transport = plugin::transport(value);
                let state = if transport == AgentMcpTransport::Unknown
                    || plugin::runtime_expansion(value)
                {
                    AgentAssetDeclaredState::Unknown
                } else {
                    AgentAssetDeclaredState::Enabled
                };
                let manifest_source = binding_matches(&SourceRole::Manifest);
                let valid_manifest_component = manifest_source
                    && *ordinal > 0
                    && manifest
                        .mcp
                        .get(ordinal - 1)
                        .is_some_and(|component| match component {
                            plugin::McpComponent::Inline(servers) => {
                                servers.get(name) == Some(value)
                            }
                            plugin::McpComponent::File(_) => {
                                expected_path.as_ref() == Some(&source.path)
                            }
                            plugin::McpComponent::Unobserved | plugin::McpComponent::Invalid => {
                                false
                            }
                        });
                let role = SourceRole::Mcp {
                    namespace: namespace.clone(),
                    ordinal: *ordinal,
                };
                if expected_path.as_ref() != Some(&source.path)
                    || !(valid_manifest_component || binding_matches(&role))
                    || transport == AgentMcpTransport::Unknown
                    || !matches!(super::super::parse::native_mcp_payload(value, transport),
                        AgentAssetNativePayload::McpDefinition(expected) if expected == *definition)
                    || asset.category != AgentAssetCategory::Mcp
                    || asset.declaration_key
                        != format!("plugin.mcp:{}:{ordinal}:{name}", key.occurrence)
                    || asset.declared_state != state
                    || !matches!(asset.details, AgentAssetDetails::Mcp { transport: actual, declared_state, .. } if actual == transport && declared_state == state)
                {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            AgentAssetNativePayload::ClaudePlugin(Payload::Hook { .. }) => {
                self.validate_hook_child(asset, entry, manifest)?;
            }
            _ => unreachable!(),
        }
        Ok(Some(entry))
    }

    pub(super) fn apply_plugin_parent(
        &self,
        decision: &mut NativeDecision<'a>,
    ) -> NativeResult<()> {
        let Some(entry) = self.validate_plugin_child(decision.anchor)? else {
            return Ok(());
        };
        if decision.resolution.relation == AgentAssetResolutionRelation::Replaced
            || decision.assessment.declared_state != AgentAssetDeclaredState::Enabled
            || !matches!(
                decision.effective,
                AgentAssetEffectiveStateProofDraft::Intrinsic
            )
        {
            return Ok(());
        }
        let target = AgentAssetAssessmentTarget {
            category: AgentAssetCategory::Plugin,
            resolution_group_key: entry.key.id.clone(),
            exact_native_id: entry.key.id.clone(),
            subject: AgentAssetAssessmentSubject::Bucket,
        };
        let parent = self.decide(&target)?;
        let (state, _) = compose_effective_state(
            &parent.assessment.intrinsic,
            &parent.effective,
            parent.parent_state,
        )
        .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let gate = AgentAssetEffectiveStateProofDraft::ParentGate {
            parent: plugin::parent_ref(&entry.key),
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        decision.parent_state = Some(state);
        if matches!(state, AgentAssetState::Unknown | AgentAssetState::Blocked) {
            decision.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
            decision.effective = AgentAssetEffectiveStateProofDraft::Terminal {
                terminal: AgentAssetResolutionTerminal::Unknown,
                cause: AgentAssetTerminalCauseDraft::ParentUnknown,
                evidence: Vec::new(),
                input: Box::new(gate),
            };
        } else {
            decision.effective = gate;
        }
        Ok(())
    }
}
