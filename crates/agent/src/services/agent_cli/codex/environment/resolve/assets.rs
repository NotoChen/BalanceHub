mod mcp;
pub(super) use mcp::has_plugin_mcp;

use super::super::native;
use super::*;
use crate::models::{
    AgentAssetInstallState, AgentAssetSourceKind, AgentAssetState, AgentSkillInvocationPolicy,
};
use crate::services::agent_cli::contracts::{
    CodexAssetPayload, CodexSkillRule, CodexSkillSelector,
};
use std::path::Path;

pub(super) struct AssetIndex<'a> {
    context: &'a crate::models::AgentConfigurationContext,
    declarations: &'a [ParsedAgentAsset],
    sources: BTreeMap<String, AgentAssetSourceSpec>,
    rules: Result<(&'a ParsedAgentAsset, &'a [CodexSkillRule], bool), AgentAssetAssessmentFailure>,
    config_roots: Result<Vec<&'a ParsedAgentAsset>, AgentAssetAssessmentFailure>,
    plugins: BTreeMap<String, Result<NativeDecision<'a>, AgentAssetAssessmentFailure>>,
}

impl<'a> AssetIndex<'a> {
    pub(super) fn new(
        context: &'a crate::models::AgentConfigurationContext,
        declarations: &'a [ParsedAgentAsset],
        sources: &[AgentAssetSourceSpec],
    ) -> Self {
        let duplicate_sources = sources
            .iter()
            .map(|source| &source.native_source_key)
            .collect::<BTreeSet<_>>()
            .len()
            != sources.len();
        let sources = sources
            .iter()
            .map(|source| (source.native_source_key.clone(), source.clone()))
            .collect();
        let mut index = Self {
            context,
            declarations,
            sources,
            rules: Err(AgentAssetAssessmentFailure::IncompleteInput),
            config_roots: Err(AgentAssetAssessmentFailure::IncompleteInput),
            plugins: BTreeMap::new(),
        };
        if duplicate_sources {
            index.rules = Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            index.config_roots = Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        } else {
            index.rules = index.rule_index();
            index.config_roots = index.plugin_config_roots();
        }
        let mut buckets = BTreeMap::<String, Vec<&ParsedAgentAsset>>::new();
        for asset in declarations
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Plugin)
        {
            buckets
                .entry(asset.native_id.clone())
                .or_default()
                .push(asset);
        }
        for (id, mut bucket) in buckets {
            if !bucket.iter().any(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig { .. })
                )
            }) {
                continue;
            }
            bucket.sort_by_key(|asset| {
                (
                    asset.logical_origin.precedence,
                    &asset.source_key,
                    &asset.declaration_id,
                )
            });
            index.plugins.insert(id, index.plugin_decision(&bucket));
        }
        index
    }

    pub(super) fn configured_plugin(&self, id: &str) -> bool {
        self.plugins.contains_key(id)
    }

    pub(super) fn plugin(
        &self,
        id: &str,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        self.plugins
            .get(id)
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?
            .clone()
    }

    fn valid_source(&self, asset: &ParsedAgentAsset) -> bool {
        self.sources.get(&asset.source_key).is_some_and(|source| {
            declaration_matches_source(self.context, asset, source)
                && asset.logical_origin
                    == crate::services::agent_cli::environment::physical_origin(source)
        }) && self
            .declarations
            .iter()
            .filter(|other| other.declaration_id == asset.declaration_id)
            .count()
            == 1
    }

    fn rule_index(
        &self,
    ) -> Result<(&'a ParsedAgentAsset, &'a [CodexSkillRule], bool), AgentAssetAssessmentFailure>
    {
        let mut found = self.declarations.iter().filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillRules { .. })
            )
        });
        let rule = found
            .next()
            .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
        if found.next().is_some()
            || !self.valid_source(rule)
            || rule.source_key != "config"
            || rule.category != AgentAssetCategory::Skill
            || rule.role != AgentAssetDeclarationRole::PolicyOverlay
            || rule.participation != AgentAssetResolutionParticipation::Participates
            || rule.declaration_key != native::SKILL_RULES_KEY
            || rule.native_id != native::SKILL_RULES_ID
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillRules { rules, available }) =
            &rule.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        if rules.iter().enumerate().any(|(index, rule)| {
            rules[..index]
                .iter()
                .any(|other| other.selector == rule.selector)
        }) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok((rule, rules, *available))
    }

    fn plugin_config_roots(
        &self,
    ) -> Result<Vec<&'a ParsedAgentAsset>, AgentAssetAssessmentFailure> {
        let mut roots = Vec::new();
        for source in self.sources.values().filter(|source| {
            matches!(
                source.native_source_key.as_str(),
                "config" | "system-config" | "workspace-config"
            )
        }) {
            let in_source = self
                .declarations
                .iter()
                .filter(|asset| {
                    asset.source_key == source.native_source_key
                        && asset.category == AgentAssetCategory::Plugin
                })
                .collect::<Vec<_>>();
            let mut root = None;
            let mut count = 0;
            for asset in &in_source {
                if !self.valid_source(asset) {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
                match &asset.native_payload {
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfigRoot {
                        ..
                    }) if asset.declaration_key == native::PLUGIN_CONFIG_ROOT_KEY
                        && asset.native_id == native::PLUGIN_CONFIG_ROOT_ID
                        && asset.role == AgentAssetDeclarationRole::PolicyOverlay =>
                    {
                        if root.replace(*asset).is_some() {
                            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                        }
                    }
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig {
                        table,
                    }) if asset.declaration_key == format!("plugins.entry:{}", asset.native_id)
                        && asset.role == AgentAssetDeclarationRole::Definition =>
                    {
                        self.validate_mcp_policies(asset, table)?;
                        count += 1;
                    }
                    _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
                }
            }
            let root = root.ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
            let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfigRoot {
                entry_count,
                ..
            }) = root.native_payload
            else {
                unreachable!()
            };
            if count != entry_count {
                return Err(AgentAssetAssessmentFailure::IncompleteInput);
            }
            for policy in self.declarations.iter().filter(|asset| {
                asset.source_key == source.native_source_key
                    && asset.category == AgentAssetCategory::Mcp
                    && matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { .. })
                    )
            }) {
                if !in_source.iter().any(|config| {
                    matches!(
                        config.native_payload,
                        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig { .. })
                    ) && policy.declaration_key
                        == format!("plugins.mcp:{}:{}", config.native_id, policy.native_id)
                }) {
                    return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
                }
            }
            roots.push(root);
        }
        Ok(roots)
    }

    fn validate_mcp_policies(
        &self,
        config: &ParsedAgentAsset,
        table: &toml::value::Table,
    ) -> Result<(), AgentAssetAssessmentFailure> {
        let prefix = format!("plugins.mcp:{}:", config.native_id);
        let actual = self
            .declarations
            .iter()
            .filter(|asset| {
                asset.source_key == config.source_key
                    && asset.category == AgentAssetCategory::Mcp
                    && asset.declaration_key.starts_with(&prefix)
            })
            .collect::<Vec<_>>();
        let expected = table
            .get("mcp_servers")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flat_map(|servers| servers.iter())
            .filter_map(|(name, value)| value.as_table().map(|table| (name, table)))
            .collect::<Vec<_>>();
        if actual.len() != expected.len() {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        }
        for (name, table) in expected {
            let key = format!("{prefix}{name}");
            let mut matching = actual.iter().filter(|asset| asset.declaration_key == key);
            let asset = matching
                .next()
                .ok_or(AgentAssetAssessmentFailure::IncompleteInput)?;
            if matching.next().is_some()
                || !self.valid_source(asset)
                || asset.native_id != *name
                || asset.resolution_group_key != key
                || asset.role != AgentAssetDeclarationRole::PolicyOverlay
                || asset.participation != config.participation
                || asset.declared_state
                    != table
                        .get("enabled")
                        .and_then(toml::Value::as_bool)
                        .map_or(AgentAssetDeclaredState::Unknown, declared)
                || !matches!(&asset.native_payload,
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { table: raw }) if raw == table)
            {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
        }
        Ok(())
    }

    fn plugin_decision(
        &self,
        bucket: &[&'a ParsedAgentAsset],
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let config = bucket
            .iter()
            .copied()
            .filter(|asset| {
                asset.participation == AgentAssetResolutionParticipation::Participates
                    && matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig { .. })
                    )
            })
            .collect::<Vec<_>>();
        let anchor = *config
            .last()
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)?;
        let roots = self.config_roots.as_ref().map_err(|failure| *failure)?;
        let definitions = definitions(bucket, true);
        if definitions.iter().any(|asset| !self.valid_source(asset)) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let mut merged = toml::value::Table::new();
        for asset in &config {
            let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig { table }) =
                &asset.native_payload
            else {
                unreachable!()
            };
            super::super::merge_toml_table(&mut merged, table);
        }
        let installed = self.install_state(anchor.native_id.as_str(), &definitions)?;
        let members = config
            .iter()
            .map(|asset| member(asset, AgentAssetEvidenceKind::Definition))
            .collect::<Vec<_>>();
        let assessment = if let Some(decoded) = native::decode_plugin_config(&merged) {
            let state = declared(decoded.enabled);
            let selected = config.iter().copied().rev().find(|asset| matches!(&asset.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfig { table }) if table.get("enabled").is_some()));
            AgentAssetNativeAssessment {
                declared_state: state,
                declared: match selected {
                    Some(selected) => AgentAssetDeclaredStateProofDraft::Definition {
                        declaration_ids: sorted_ids(&members),
                        selected_id: selected.declaration_id.clone(),
                    },
                    None => AgentAssetDeclaredStateProofDraft::NativeDefault {
                        definition_id: anchor.declaration_id.clone(),
                        outcome: state,
                    },
                },
                declared_members: if selected.is_some() {
                    members
                } else {
                    vec![member(anchor, AgentAssetEvidenceKind::Definition)]
                },
                intrinsic: intrinsic_basis(plugin_details(state, installed), anchor.trust_state),
                control: None,
            }
        } else {
            unknown_assessment(
                AgentAssetDeclaredUnknownCauseDraft::InvalidNativeMerge,
                members,
                plugin_details(AgentAssetDeclaredState::Unknown, installed),
                anchor.trust_state,
            )
        };
        let invalid = roots
            .iter()
            .copied()
            .filter(|asset| {
                asset.participation == AgentAssetResolutionParticipation::Participates
                    && matches!(
                        asset.native_payload,
                        AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginConfigRoot {
                            valid: false,
                            ..
                        })
                    )
            })
            .collect::<Vec<_>>();
        let control = (!invalid.is_empty()).then(|| invalid_control(&invalid));
        let (terminal_kind, owner, effective) = effective_control(&assessment, control.as_ref());
        Ok(NativeDecision {
            anchor,
            projection_key: format!("plugin:{}", anchor.native_id),
            represented: bucket.to_vec(),
            contributors: definitions.clone(),
            resolution: resolution(
                if definitions.len() > 1 {
                    AgentAssetResolutionRelation::Merged
                } else {
                    AgentAssetResolutionRelation::Independent
                },
                terminal_kind,
                owner,
            ),
            assessment: AgentAssetNativeAssessment {
                control,
                ..assessment
            },
            effective_proof: effective,
            malformed: false,
            parent_state: None,
            relationship_contributors: None,
        })
    }

    fn install_state(
        &self,
        id: &str,
        definitions: &[&ParsedAgentAsset],
    ) -> Result<AgentAssetInstallState, AgentAssetAssessmentFailure> {
        let Some((name, marketplace)) = native::plugin_id_parts(id) else {
            return Ok(AgentAssetInstallState::Unknown);
        };
        let expected = Path::new(&self.context.config_root)
            .join("plugins/cache")
            .join(marketplace)
            .join(name);
        let versions = definitions
            .iter()
            .copied()
            .filter(|asset| {
                matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginVersions { .. })
                )
            })
            .collect::<Vec<_>>();
        if versions.is_empty() {
            return Ok(AgentAssetInstallState::Unknown);
        }
        if versions.len() != 1 {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let versions = versions[0];
        if versions.source_key != format!("{}{id}", native::PLUGIN_BASE_PREFIX)
            || versions.declaration_key != "cache:versions"
            || self.sources.get(&versions.source_key).is_none_or(|source| {
                source.path != expected || source.source_kind != AgentAssetSourceKind::Directory
            })
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginVersions {
            versions,
            complete,
        }) = &versions.native_payload
        else {
            unreachable!()
        };
        if !complete {
            return Ok(AgentAssetInstallState::Unknown);
        }
        if versions.iter().collect::<BTreeSet<_>>().len() != versions.len() {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let Some(version) = native::active_version(versions) else {
            return Ok(AgentAssetInstallState::NotInstalled);
        };
        let mut manifests = definitions.iter().filter(|asset| {
            matches!(
                asset.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest { .. })
            )
        });
        let Some(manifest) = manifests.next() else {
            return Ok(AgentAssetInstallState::Unknown);
        };
        if manifests.next().is_some() {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let source = self
            .sources
            .get(&manifest.source_key)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let Some((source_id, source_version, format)) = manifest
            .source_key
            .strip_prefix(native::PLUGIN_MANIFEST_PREFIX)
            .and_then(native::plugin_manifest_parts)
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let relative = match format {
            "agent" => "plugin.json",
            "codex" => ".codex-plugin/plugin.json",
            "claude" => ".claude-plugin/plugin.json",
            "cursor" => ".cursor-plugin/plugin.json",
            _ => unreachable!(),
        };
        if source_id != id
            || source_version != version
            || source.path != expected.join(version).join(relative)
            || manifest.declaration_key != "plugin.json"
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        Ok(
            if matches!(
                manifest.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest {
                    valid: true,
                    ..
                })
            ) {
                AgentAssetInstallState::Installed
            } else {
                AgentAssetInstallState::Unknown
            },
        )
    }

    pub(super) fn skill(
        &self,
        bucket: &[&'a ParsedAgentAsset],
        target: &AgentAssetAssessmentTarget,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let definitions = definitions(bucket, true);
        let anchor = match &target.subject {
            AgentAssetAssessmentSubject::Bucket if definitions.len() == 1 => definitions[0],
            AgentAssetAssessmentSubject::Definition(id) if definitions.len() > 1 => *definitions
                .iter()
                .find(|asset| asset.declaration_id == *id)
                .ok_or(AgentAssetAssessmentFailure::MissingTarget)?,
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::SkillDefinition {
            canonical_path,
            plugin_id,
        }) = &anchor.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        if !self.valid_source(anchor)
            || !canonical_path.is_absolute()
            || canonical_path
                .file_name()
                .is_none_or(|name| name != "SKILL.md")
            || anchor.declaration_key != "SKILL.md"
            || !anchor.source_key.starts_with(native::SKILL_PREFIX)
            || anchor.action_owner.is_some()
            || anchor.declared_state != AgentAssetDeclaredState::Unknown
            || !self.valid_skill_source(anchor, canonical_path, plugin_id.is_some())
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let parent = self.validate_skill_parent(anchor, canonical_path, plugin_id.as_deref())?;
        let (rules_asset, rules, available) = self.rules.as_ref().map_err(|failure| *failure)?;
        let matched = rules.iter().rev().find(|rule| match &rule.selector {
            CodexSkillSelector::Name(name) => name == &anchor.native_id,
            CodexSkillSelector::Path(path) => path == canonical_path,
        });
        let state = declared(matched.is_none_or(|rule| rule.enabled));
        let members = vec![member(
            if matched.is_some() {
                rules_asset
            } else {
                anchor
            },
            if matched.is_some() {
                AgentAssetEvidenceKind::Policy
            } else {
                AgentAssetEvidenceKind::Definition
            },
        )];
        let control = (!available).then(|| invalid_control(&[*rules_asset]));
        let assessment = AgentAssetNativeAssessment {
            declared_state: state,
            declared: if matched.is_some() {
                AgentAssetDeclaredStateProofDraft::Policy {
                    declaration_ids: sorted_ids(&members),
                    outcome: state,
                }
            } else {
                AgentAssetDeclaredStateProofDraft::NativeDefault {
                    definition_id: anchor.declaration_id.clone(),
                    outcome: state,
                }
            },
            declared_members: members,
            intrinsic: intrinsic_basis(
                AgentAssetDetails::Skill {
                    enabled: state,
                    invocation_policy: AgentSkillInvocationPolicy::Unknown,
                },
                anchor.trust_state,
            ),
            control,
        };
        let (terminal_kind, owner, effective) =
            effective_control(&assessment, assessment.control.as_ref());
        let (effective, terminal_kind, parent_state) =
            parent_route(anchor, state, effective, terminal_kind, parent)?;
        Ok(NativeDecision {
            anchor,
            projection_key: if definitions.len() > 1 {
                qualified_projection_key(anchor)
            } else {
                format!("skill:{}", anchor.native_id)
            },
            // Physical aliases remain complete native evidence after deduplication;
            // only this row's selected Definition supplies state and content.
            represented: bucket.to_vec(),
            contributors: vec![anchor],
            resolution: resolution(
                if definitions.len() > 1 {
                    AgentAssetResolutionRelation::Additive
                } else {
                    AgentAssetResolutionRelation::Independent
                },
                terminal_kind,
                owner,
            ),
            assessment,
            effective_proof: effective,
            malformed: false,
            parent_state,
            relationship_contributors: None,
        })
    }

    fn valid_skill_source(&self, skill: &ParsedAgentAsset, canonical: &Path, plugin: bool) -> bool {
        let Some((root_key, entry)) = native::skill_source_parts(&skill.source_key) else {
            return false;
        };
        if root_key.starts_with(native::PLUGIN_SKILLS_PREFIX) != plugin {
            return false;
        }
        let Some(root) = self.sources.get(root_key) else {
            return false;
        };
        let Some(source) = self.sources.get(&skill.source_key) else {
            return false;
        };
        let expected = if entry == "SKILL.md" {
            root.path.join("SKILL.md")
        } else {
            root.path.join(entry).join("SKILL.md")
        };
        root.source_kind == AgentAssetSourceKind::Directory
            && root.categories.contains(&AgentAssetCategory::Skill)
            && source.path == expected
            && source.definition_identity_path() == Some(canonical)
    }

    fn validate_skill_parent(
        &self,
        skill: &ParsedAgentAsset,
        canonical_path: &Path,
        id: Option<&str>,
    ) -> Result<Option<AgentAssetState>, AgentAssetAssessmentFailure> {
        let Some(id) = id else {
            return if skill.provided_by.is_none() {
                Ok(None)
            } else {
                Err(AgentAssetAssessmentFailure::InvalidNativeInput)
            };
        };
        if skill.provided_by != Some(parent_reference(id)) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let parent = self.plugin(id)?;
        if !matches!(
            parent.assessment.intrinsic.details,
            AgentAssetDetails::Plugin {
                install_state: AgentAssetInstallState::Installed,
                ..
            }
        ) {
            return Err(AgentAssetAssessmentFailure::IncompleteInput);
        }
        let manifest = parent
            .contributors
            .iter()
            .find_map(|asset| match &asset.native_payload {
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest {
                    namespace,
                    skill_roots,
                    valid: true,
                    ..
                }) => Some((*asset, namespace, skill_roots)),
                _ => None,
            })
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let source = self
            .sources
            .get(&skill.source_key)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let lexical_path = &source.path;
        let (_, manifest_version, manifest_format) = manifest
            .0
            .source_key
            .strip_prefix(native::PLUGIN_MANIFEST_PREFIX)
            .and_then(native::plugin_manifest_parts)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let (root_key, entry) = native::skill_source_parts(&skill.source_key)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let (source_id, version, namespace, root_index, agent) = root_key
            .strip_prefix(native::PLUGIN_SKILLS_PREFIX)
            .and_then(native::plugin_skill_source_parts)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let root = manifest
            .2
            .get(root_index)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let root_manifest = entry == "SKILL.md";
        let expected = if root_manifest {
            root.join("SKILL.md")
        } else {
            root.join(entry).join("SKILL.md")
        };
        if source_id != id
            || version != manifest_version
            || agent != (manifest_format == "agent")
            || (agent && root_manifest)
            || namespace != *manifest.1
            || !skill.native_id.starts_with(&format!("{}:", manifest.1))
            || *lexical_path != expected
            || source.definition_identity_path() != Some(canonical_path)
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        compose_effective_state(&parent.assessment.intrinsic, &parent.effective_proof, None)
            .map(|(state, _)| Some(state))
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)
    }
}

fn declared(enabled: bool) -> AgentAssetDeclaredState {
    if enabled {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Disabled
    }
}
fn plugin_details(
    enabled: AgentAssetDeclaredState,
    install_state: AgentAssetInstallState,
) -> AgentAssetDetails {
    AgentAssetDetails::Plugin {
        install_state,
        enabled,
        trusted: AgentTrustState::Unknown,
    }
}
fn sorted_ids(members: &[AgentAssetEvidenceMember]) -> Vec<String> {
    let mut ids = members
        .iter()
        .map(|member| member.declaration_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}
fn parent_reference(id: &str) -> AgentAssetNativeRef {
    AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: id.to_owned(),
        qualifier: Some(format!("plugin:{id}")),
    }
}
fn invalid_control(assets: &[&ParsedAgentAsset]) -> AgentAssetControlAssessment {
    AgentAssetControlAssessment {
        cause: AgentAssetTerminalCauseDraft::InvalidControl,
        members: assets
            .iter()
            .map(|asset| member(asset, AgentAssetEvidenceKind::InvalidControl))
            .collect(),
        authorities: assets
            .iter()
            .map(|asset| AgentAssetControlAuthority::Declaration(asset.declaration_id.clone()))
            .collect(),
    }
}
fn effective_control(
    assessment: &AgentAssetNativeAssessment,
    control: Option<&AgentAssetControlAssessment>,
) -> (
    Option<AgentAssetResolutionTerminal>,
    Option<AgentAssetPolicyReferenceDraft>,
    AgentAssetEffectiveStateProofDraft,
) {
    if assessment.declared_state == AgentAssetDeclaredState::Unknown {
        return (
            Some(AgentAssetResolutionTerminal::Unknown),
            None,
            terminal(AgentAssetTerminalCauseDraft::DeclaredUnknown, Vec::new()),
        );
    }
    let Some(control) = control else {
        return (None, None, AgentAssetEffectiveStateProofDraft::Intrinsic);
    };
    let owner = if control.authorities.len() == 1 {
        match control.authorities.first() {
            Some(AgentAssetControlAuthority::Declaration(id)) => {
                Some(AgentAssetPolicyReferenceDraft::Declaration {
                    declaration_id: id.clone(),
                })
            }
            Some(AgentAssetControlAuthority::SourceAggregate(key)) => {
                Some(AgentAssetPolicyReferenceDraft::Source {
                    source_key: key.clone(),
                })
            }
            None => None,
        }
    } else {
        None
    };
    (
        Some(
            if control.cause == AgentAssetTerminalCauseDraft::TypedPolicy {
                AgentAssetResolutionTerminal::PolicyBlocked
            } else {
                AgentAssetResolutionTerminal::Unknown
            },
        ),
        owner,
        terminal(control.cause, evidence(&control.members)),
    )
}

pub(super) fn parent_route(
    anchor: &ParsedAgentAsset,
    declared: AgentAssetDeclaredState,
    effective: AgentAssetEffectiveStateProofDraft,
    terminal_kind: Option<AgentAssetResolutionTerminal>,
    parent_state: Option<AgentAssetState>,
) -> Result<
    (
        AgentAssetEffectiveStateProofDraft,
        Option<AgentAssetResolutionTerminal>,
        Option<AgentAssetState>,
    ),
    AgentAssetAssessmentFailure,
> {
    let Some(parent_state) = parent_state else {
        return Ok((effective, terminal_kind, None));
    };
    if (declared == AgentAssetDeclaredState::Disabled
        && matches!(
            parent_state,
            AgentAssetState::Unknown | AgentAssetState::Blocked
        ))
        || terminal_kind.is_some()
    {
        return Ok((effective, terminal_kind, None));
    }
    let parent = anchor
        .provided_by
        .clone()
        .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
    let gate = AgentAssetEffectiveStateProofDraft::ParentGate {
        parent,
        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
    };
    if matches!(
        parent_state,
        AgentAssetState::Unknown | AgentAssetState::Blocked
    ) {
        Ok((
            AgentAssetEffectiveStateProofDraft::Terminal {
                terminal: AgentAssetResolutionTerminal::Unknown,
                cause: AgentAssetTerminalCauseDraft::ParentUnknown,
                evidence: Vec::new(),
                input: Box::new(gate),
            },
            Some(AgentAssetResolutionTerminal::Unknown),
            Some(parent_state),
        ))
    } else {
        Ok((gate, None, Some(parent_state)))
    }
}
