use super::*;

pub(in crate::services::agent_cli::codex::environment::resolve) fn has_plugin_mcp(
    bucket: &[&ParsedAgentAsset],
) -> bool {
    bucket.iter().any(|asset| {
        asset.role == AgentAssetDeclarationRole::Definition
            && matches!(
                asset.native_payload,
                AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { .. })
            )
    })
}

impl<'a> AssetIndex<'a> {
    pub(in crate::services::agent_cli::codex::environment::resolve) fn mcp_winner(
        &self,
        bucket: &[&'a ParsedAgentAsset],
    ) -> Result<&'a ParsedAgentAsset, AgentAssetAssessmentFailure> {
        let definitions = definitions(bucket, true);
        if let Some(config) = definitions
            .iter()
            .copied()
            .rev()
            .find(|asset| matches!(asset.native_payload, AgentAssetNativePayload::TomlTable(_)))
        {
            return Ok(config);
        }
        let mut plugins = definitions;
        plugins.sort_by_key(|asset| {
            asset
                .provided_by
                .as_ref()
                .map(|parent| parent.native_id.as_str())
        });
        for plugin in &plugins {
            let parent = plugin
                .provided_by
                .as_ref()
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            let decision = self.plugin(&parent.native_id)?;
            if decision.assessment.declared_state != AgentAssetDeclaredState::Disabled {
                return Ok(*plugin);
            }
        }
        plugins
            .first()
            .copied()
            .ok_or(AgentAssetAssessmentFailure::MissingTarget)
    }

    pub(in crate::services::agent_cli::codex::environment::resolve) fn mcp(
        &self,
        bucket: &[&'a ParsedAgentAsset],
        target: &AgentAssetAssessmentTarget,
        requirements: &Result<RequirementsIndex<'a>, AgentAssetAssessmentFailure>,
        context_trust: AgentTrustState,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        let winner = self.mcp_winner(bucket)?;
        let definitions = definitions(bucket, true);
        let selected = match &target.subject {
            AgentAssetAssessmentSubject::Bucket => winner,
            AgentAssetAssessmentSubject::Definition(id) if definitions.len() > 1 => *definitions
                .iter()
                .find(|asset| {
                    asset.declaration_id == *id && asset.declaration_id != winner.declaration_id
                })
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?,
            _ => return Err(AgentAssetAssessmentFailure::InvalidNativeInput),
        };
        let shadowed = selected.declaration_id != winner.declaration_id;
        let empty_requirements = Ok(RequirementsIndex::NoPolicy);
        let active_requirements = if shadowed {
            &empty_requirements
        } else {
            requirements
        };
        let base = if matches!(
            selected.native_payload,
            AgentAssetNativePayload::TomlTable(_)
        ) {
            let config = if shadowed {
                vec![selected]
            } else {
                bucket
                    .iter()
                    .copied()
                    .filter(|asset| {
                        matches!(asset.native_payload, AgentAssetNativePayload::TomlTable(_))
                    })
                    .collect()
            };
            decide_mcp(&config, active_requirements, context_trust)?
        } else {
            self.plugin_mcp(selected, active_requirements)?
        };
        if definitions.len() == 1 {
            return Ok(base);
        }
        let winner_ref = AgentAssetNativeRef {
            category: AgentAssetCategory::Mcp,
            native_id: winner.native_id.clone(),
            qualifier: Some(format!("mcp:{}", winner.native_id)),
        };
        if shadowed {
            return Ok(NativeDecision {
                anchor: selected,
                projection_key: qualified_projection_key(selected),
                represented: base.represented,
                contributors: base.contributors,
                resolution: AgentAssetResolutionDraft {
                    relation: AgentAssetResolutionRelation::Replaced,
                    qualified_collision: false,
                    terminal: None,
                    winner: Some(winner_ref.clone()),
                    control_source: None,
                },
                assessment: AgentAssetNativeAssessment {
                    control: None,
                    ..base.assessment
                },
                effective_proof: AgentAssetEffectiveStateProofDraft::Shadowed {
                    winner: winner_ref,
                    input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                },
                malformed: base.malformed,
                parent_state: None,
                relationship_contributors: None,
            });
        }
        Ok(NativeDecision {
            anchor: base.anchor,
            projection_key: format!("mcp:{}", selected.native_id),
            represented: bucket.to_vec(),
            contributors: bucket
                .iter()
                .copied()
                .filter(|asset| {
                    asset.participation == AgentAssetResolutionParticipation::Participates
                        && asset.role != AgentAssetDeclarationRole::PolicyOverlay
                })
                .collect(),
            resolution: AgentAssetResolutionDraft {
                relation: AgentAssetResolutionRelation::ReplaceWinner,
                winner: Some(winner_ref),
                ..base.resolution
            },
            relationship_contributors: Some(base.contributors),
            assessment: base.assessment,
            effective_proof: base.effective_proof,
            malformed: base.malformed,
            parent_state: base.parent_state,
        })
    }

    fn plugin_mcp(
        &self,
        asset: &'a ParsedAgentAsset,
        requirements: &Result<RequirementsIndex<'a>, AgentAssetAssessmentFailure>,
    ) -> Result<NativeDecision<'a>, AgentAssetAssessmentFailure> {
        if !self.valid_source(asset)
            || asset.category != AgentAssetCategory::Mcp
            || asset.role != AgentAssetDeclarationRole::Definition
            || asset.participation != AgentAssetResolutionParticipation::Participates
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let parent_ref = asset
            .provided_by
            .as_ref()
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        if *parent_ref != parent_reference(&parent_ref.native_id)
            || asset.action_owner.as_ref() != Some(parent_ref)
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let parent = self.plugin(&parent_ref.native_id)?;
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
            .find(|definition| {
                matches!(
                    definition.native_payload,
                    AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest {
                        valid: true,
                        ..
                    })
                )
            })
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginManifest {
            mcp_path, ..
        }) = &manifest.native_payload
        else {
            unreachable!()
        };
        let source = self
            .sources
            .get(&asset.source_key)
            .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        if asset.source_key != manifest.source_key {
            let (_, version, format) = manifest
                .source_key
                .strip_prefix(native::PLUGIN_MANIFEST_PREFIX)
                .and_then(native::plugin_manifest_parts)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            let (id, source_version, agent) = asset
                .source_key
                .strip_prefix(native::PLUGIN_MCP_PREFIX)
                .and_then(native::plugin_mcp_source_parts)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
            if mcp_path.as_ref() != Some(&source.path)
                || id != parent_ref.native_id
                || source_version != version
                || agent != (format == "agent")
            {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
        }
        if asset.declaration_key != format!("mcpServers.entry:{}", asset.native_id) {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { table }) =
            &asset.native_payload
        else {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        };
        let decoded = super::super::super::decode_mcp_config(table)
            .map_err(|_| AgentAssetAssessmentFailure::InvalidNativeInput)?;
        if asset.declared_state
            != table
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .map_or(AgentAssetDeclaredState::Unknown, declared)
        {
            return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
        }
        let policy_key = format!("plugins.mcp:{}:{}", parent_ref.native_id, asset.native_id);
        let mut policies = self
            .declarations
            .iter()
            .filter(|declaration| {
                declaration.category == AgentAssetCategory::Mcp
                    && declaration.declaration_key == policy_key
                    && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
                    && declaration.participation == AgentAssetResolutionParticipation::Participates
            })
            .collect::<Vec<_>>();
        policies.sort_by_key(|asset| (asset.logical_origin.precedence, &asset.source_key));
        let mut policy_table = toml::value::Table::new();
        for policy in &policies {
            if !self.valid_source(policy) {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            }
            let AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { table }) =
                &policy.native_payload
            else {
                return Err(AgentAssetAssessmentFailure::InvalidNativeInput);
            };
            super::super::super::merge_toml_table(&mut policy_table, table);
        }
        let native_state = declared(decoded.enabled);
        let policy_state = if policies.is_empty() {
            None
        } else {
            let parent_table = toml::value::Table::from_iter([(
                "mcp_servers".to_owned(),
                toml::Value::Table(toml::value::Table::from_iter([(
                    asset.native_id.clone(),
                    toml::Value::Table(policy_table),
                )])),
            )]);
            native::decode_plugin_config(&parent_table).and_then(|config| {
                config
                    .mcp_servers
                    .get(&asset.native_id)
                    .map(|policy| declared(policy.enabled))
            })
        };
        let state = policy_state.unwrap_or(native_state);
        let members = if policy_state.is_some() {
            policies
                .iter()
                .map(|policy| member(policy, AgentAssetEvidenceKind::Policy))
                .collect::<Vec<_>>()
        } else {
            vec![member(asset, AgentAssetEvidenceKind::Definition)]
        };
        let declared = if policy_state.is_some() {
            AgentAssetDeclaredStateProofDraft::Policy {
                declaration_ids: sorted_ids(&members),
                outcome: state,
            }
        } else if asset.declared_state != AgentAssetDeclaredState::Unknown {
            AgentAssetDeclaredStateProofDraft::Definition {
                declaration_ids: sorted_ids(&members),
                selected_id: asset.declaration_id.clone(),
            }
        } else {
            AgentAssetDeclaredStateProofDraft::NativeDefault {
                definition_id: asset.declaration_id.clone(),
                outcome: state,
            }
        };
        let control = if !policies.is_empty() && policy_state.is_none() {
            Some(invalid_control(&policies))
        } else {
            requirement_policy(
                &asset.native_id,
                &decoded,
                requirements.as_ref().map_err(|failure| *failure)?,
            )
        };
        let assessment = AgentAssetNativeAssessment {
            declared_state: state,
            declared,
            declared_members: members,
            intrinsic: intrinsic_basis(mcp_details(decoded.transport, state), asset.trust_state),
            control,
        };
        let (terminal_kind, owner, effective) =
            effective_control(&assessment, assessment.control.as_ref());
        let parent_state =
            compose_effective_state(&parent.assessment.intrinsic, &parent.effective_proof, None)
                .map(|(state, _)| state)
                .ok_or(AgentAssetAssessmentFailure::InvalidNativeInput)?;
        let (effective, terminal_kind, parent_state) =
            parent_route(asset, state, effective, terminal_kind, Some(parent_state))?;
        Ok(NativeDecision {
            anchor: asset,
            projection_key: format!("mcp:{}", asset.native_id),
            represented: vec![asset],
            contributors: vec![asset],
            resolution: resolution(
                AgentAssetResolutionRelation::Independent,
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
}
