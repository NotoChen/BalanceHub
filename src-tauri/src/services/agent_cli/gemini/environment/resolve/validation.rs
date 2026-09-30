//! Native shape validation supplements shared source and declaration identities.
use super::*;

pub(super) fn settings_source(source: &AgentAssetSourceSpec) -> bool {
    source.source_kind == AgentAssetSourceKind::File
        && matches!(
            source.native_source_key.as_str(),
            "settings" | "workspace-settings" | "system-defaults" | "system-settings"
        )
}

pub(super) fn control_category(asset: &ParsedAgentAsset) -> Option<AgentAssetCategory> {
    match asset.native_payload {
        AgentAssetNativePayload::McpPolicy(_) => Some(AgentAssetCategory::Mcp),
        AgentAssetNativePayload::HookPolicy(_) => Some(AgentAssetCategory::Hook),
        AgentAssetNativePayload::SkillPolicy(_) => Some(AgentAssetCategory::Skill),
        AgentAssetNativePayload::InvalidControl(AgentAssetInvalidControl::McpEnablement) => {
            Some(AgentAssetCategory::Mcp)
        }
        AgentAssetNativePayload::InvalidControl(AgentAssetInvalidControl::ExtensionEnablement) => {
            Some(AgentAssetCategory::Extension)
        }
        _ => None,
    }
}

fn expected_participation(
    context: &AgentConfigurationContext,
    source: &AgentAssetSourceSpec,
    category: AgentAssetCategory,
) -> AgentAssetResolutionParticipation {
    if context.workspace_id.is_some()
        && context.trust_context != AgentTrustState::Trusted
        && (category == AgentAssetCategory::Mcp || super::super::sources::workspace_source(source))
    {
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        AgentAssetResolutionParticipation::Participates
    }
}

fn mcp_transport(asset: &ParsedAgentAsset, identity: &AgentMcpMatcherIdentity) -> bool {
    let AgentAssetDetails::Mcp {
        transport,
        declared_state,
        approval_state,
        effective_availability,
    } = &asset.details
    else {
        return false;
    };
    *declared_state == AgentAssetDeclaredState::Enabled
        && asset.declared_state == *declared_state
        && *approval_state == AgentMcpApprovalState::NotRequired
        && *effective_availability == AgentAssetEffectiveAvailability::Available
        && match (identity, transport) {
            (AgentMcpMatcherIdentity::Stdio { argv }, AgentMcpTransport::Stdio) => argv
                .first()
                .is_some_and(|command| !command.trim().is_empty()),
            (
                AgentMcpMatcherIdentity::Remote { url },
                AgentMcpTransport::Http | AgentMcpTransport::Sse,
            ) => !url.trim().is_empty(),
            _ => false,
        }
}

fn mcp_unknown_details(asset: &ParsedAgentAsset) -> bool {
    matches!(
        asset.details,
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state: AgentAssetDeclaredState::Unknown,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: AgentAssetEffectiveAvailability::Unknown,
        }
    )
}

fn no_parent(asset: &ParsedAgentAsset) -> bool {
    asset.provided_by.is_none() && asset.action_owner.is_none()
}

fn target_shape(asset: &ParsedAgentAsset) -> bool {
    asset.category != AgentAssetCategory::Mcp
        || asset.resolution_group_key == asset.native_id.trim().to_ascii_lowercase()
}

fn definition_key(asset: &ParsedAgentAsset) -> bool {
    if let Some(parent) = asset.provided_by.as_ref() {
        parent.category == AgentAssetCategory::Extension
            && parent.qualifier.as_deref()
                == Some(format!("extension:{}", parent.native_id).as_str())
            && asset.action_owner.as_ref() == Some(parent)
            && asset.declaration_key
                == format!("extension:{}:{}", parent.native_id, asset.native_id)
    } else {
        asset.declaration_key == format!("mcpServers.{}", asset.native_id)
            && asset.action_owner.is_none()
    }
}

pub(super) fn valid(
    context: &AgentConfigurationContext,
    asset: &ParsedAgentAsset,
    source: &AgentAssetSourceSpec,
) -> bool {
    if !declaration_matches_source(context, asset, source)
        || asset.logical_origin != physical_origin(source)
        || source.source_kind != AgentAssetSourceKind::File
        || asset.presence != AgentAssetPresence::Present
        || asset.native_id.trim().is_empty()
        || asset.participation != expected_participation(context, source, asset.category)
    {
        return false;
    }
    match &asset.native_payload {
        AgentAssetNativePayload::McpPolicy(policy) => {
            if asset.category != AgentAssetCategory::Mcp
                || asset.role != AgentAssetDeclarationRole::PolicyOverlay
                || !no_parent(asset)
            {
                return false;
            }
            let (id, key) = match policy {
                AgentMcpPolicyPayload::Allowed(set) | AgentMcpPolicyPayload::Excluded(set) => {
                    if set.iter().any(|id| *id != id.trim().to_ascii_lowercase())
                        || asset.declared_state != AgentAssetDeclaredState::Enabled
                    {
                        return false;
                    }
                    let id = if matches!(policy, AgentMcpPolicyPayload::Allowed(_)) {
                        "mcp.allowed"
                    } else {
                        "mcp.excluded"
                    };
                    (id.to_owned(), id.to_owned())
                }
                AgentMcpPolicyPayload::InvalidAllowed => {
                    ("mcp.allowed".to_owned(), "mcp.allowed.invalid".to_owned())
                }
                AgentMcpPolicyPayload::InvalidExcluded => {
                    ("mcp.excluded".to_owned(), "mcp.excluded.invalid".to_owned())
                }
            };
            let declared = if matches!(
                policy,
                AgentMcpPolicyPayload::Allowed(_) | AgentMcpPolicyPayload::Excluded(_)
            ) {
                AgentAssetDeclaredState::Enabled
            } else {
                AgentAssetDeclaredState::Unknown
            };
            asset.native_id == id
                && asset.resolution_group_key == id
                && asset.declaration_key == key
                && asset.declared_state == declared
                && mcp_unknown_details(asset)
                && asset.explicitly_affected.is_empty()
        }
        AgentAssetNativePayload::McpDefinition(payload) => {
            asset.category == AgentAssetCategory::Mcp
                && asset.role == AgentAssetDeclarationRole::Definition
                && target_shape(asset)
                && mcp_transport(asset, &payload.identity)
                && definition_key(asset)
        }
        AgentAssetNativePayload::GeminiMcpEnablementInvalid => valid_mcp_overlay(asset, true),
        AgentAssetNativePayload::GeminiExtensionEnablementUnknown(_) => {
            valid_extension_overlay(asset, true)
        }
        AgentAssetNativePayload::InvalidControl(control) => {
            let (category, id) = match control {
                AgentAssetInvalidControl::McpEnablement => {
                    (AgentAssetCategory::Mcp, "mcp-enablement")
                }
                AgentAssetInvalidControl::ExtensionEnablement => {
                    (AgentAssetCategory::Extension, "extension-enablement")
                }
            };
            asset.category == category
                && asset.role == AgentAssetDeclarationRole::StateOverlay
                && asset.declaration_key == "invalid-control"
                && asset.native_id == id
                && asset.resolution_group_key == id
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && no_parent(asset)
                && details_declared_state(&asset.details) == AgentAssetDeclaredState::Unknown
                && matches!(
                    (category, &asset.details),
                    (AgentAssetCategory::Mcp, AgentAssetDetails::Mcp { .. })
                        | (
                            AgentAssetCategory::Extension,
                            AgentAssetDetails::Extension { .. }
                        )
                )
        }
        AgentAssetNativePayload::HookPolicy(policy) => {
            let (native_id, valid_key) = match policy {
                AgentHookPolicyPayload::GlobalEnabled(enabled) => (
                    "hooksConfig.enabled",
                    asset.declaration_key
                        == if enabled.is_some() {
                            "hooksConfig.enabled"
                        } else {
                            "hooksConfig.enabled.invalid"
                        },
                ),
                _ => (
                    "hooksConfig.disabled",
                    matches!(
                        asset.declaration_key.as_str(),
                        "hooksConfig.disabled" | "hooksConfig.disabled.invalid"
                    ),
                ),
            };
            settings_source(source)
                && asset.category == AgentAssetCategory::Hook
                && asset.role == AgentAssetDeclarationRole::PolicyOverlay
                && valid_key
                && asset.native_id == native_id
                && asset.resolution_group_key == asset.native_id
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && matches!(
                    asset.details,
                    AgentAssetDetails::Hook {
                        managed: false,
                        enabled: AgentAssetDeclaredState::Unknown,
                        rule_count: Some(0),
                    }
                )
                && no_parent(asset)
        }
        AgentAssetNativePayload::HookDefinition(_) => {
            asset.category == AgentAssetCategory::Hook
                && asset.role == AgentAssetDeclarationRole::Definition
                && asset.declaration_key == asset.native_id
                && asset.resolution_group_key == asset.native_id
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && matches!(
                    asset.details,
                    AgentAssetDetails::Hook {
                        managed: false,
                        enabled: AgentAssetDeclaredState::Unknown,
                        rule_count: Some(1),
                    }
                )
                && asset.action_owner == asset.provided_by
        }
        AgentAssetNativePayload::SkillPolicy(policy) => {
            let key = match policy {
                AgentSkillPolicyPayload::DisabledSet(_) => "skills.disabled",
                AgentSkillPolicyPayload::InvalidDisabledSet => "skills.disabled.invalid",
            };
            settings_source(source)
                && asset.category == AgentAssetCategory::Skill
                && asset.role == AgentAssetDeclarationRole::PolicyOverlay
                && asset.declaration_key == key
                && asset.native_id == "skills.disabled"
                && asset.resolution_group_key == asset.native_id
                && asset.declared_state == AgentAssetDeclaredState::Unknown
                && matches!(
                    asset.details,
                    AgentAssetDetails::Skill {
                        enabled: AgentAssetDeclaredState::Unknown,
                        invocation_policy: AgentSkillInvocationPolicy::Unknown,
                    }
                )
                && no_parent(asset)
                && asset.explicitly_affected.is_empty()
        }
        AgentAssetNativePayload::None => match (asset.category, asset.role) {
            (AgentAssetCategory::Mcp, AgentAssetDeclarationRole::StateOverlay) => {
                valid_mcp_overlay(asset, false)
            }
            (AgentAssetCategory::Mcp, AgentAssetDeclarationRole::Definition) => {
                asset.declared_state == AgentAssetDeclaredState::Unknown
                    && mcp_unknown_details(asset)
                    && target_shape(asset)
                    && definition_key(asset)
            }
            (AgentAssetCategory::Extension, AgentAssetDeclarationRole::StateOverlay) => {
                valid_extension_overlay(asset, false)
            }
            (AgentAssetCategory::Extension, AgentAssetDeclarationRole::Definition) => {
                source
                    .native_source_key
                    .starts_with(super::super::EXTENSION_MANIFEST_PREFIX)
                    && asset.declaration_key == asset.native_id
                    && asset.resolution_group_key == asset.native_id
                    && asset.declared_state == AgentAssetDeclaredState::Unknown
                    && no_parent(asset)
                    && matches!(
                        asset.details,
                        AgentAssetDetails::Extension {
                            install_state: AgentAssetInstallState::Installed,
                            enabled: AgentAssetDeclaredState::Unknown,
                            ..
                        }
                    )
            }
            (AgentAssetCategory::Skill, AgentAssetDeclarationRole::Definition) => {
                source.source_kind == AgentAssetSourceKind::File
                    && (source
                        .native_source_key
                        .starts_with(super::super::SKILL_MANIFEST_PREFIX)
                        || source
                            .native_source_key
                            .starts_with(super::super::EXTENSION_SKILL_PREFIX))
                    && asset.declaration_key == asset.source_key
                    && asset.resolution_group_key == asset.native_id
                    && asset.declared_state == AgentAssetDeclaredState::Unknown
                    && matches!(
                        asset.details,
                        AgentAssetDetails::Skill {
                            enabled: AgentAssetDeclaredState::Unknown,
                            invocation_policy: AgentSkillInvocationPolicy::Unknown,
                        }
                    )
                    && asset.action_owner == asset.provided_by
            }
            (AgentAssetCategory::StatusUi, AgentAssetDeclarationRole::Definition) => {
                asset.native_id == "footer"
                    && asset.resolution_group_key == "footer"
                    && matches!(
                        asset.declaration_key.as_str(),
                        "footer" | "footer.invalid-control"
                    )
                    && no_parent(asset)
                    && details_declared_state(&asset.details) == asset.declared_state
                    && matches!(asset.details, AgentAssetDetails::StatusUi { mode, command_present } if command_present == (mode == AgentStatusUiMode::Command))
            }
            _ => false,
        },
        _ => false,
    }
}

fn valid_mcp_overlay(asset: &ParsedAgentAsset, unknown: bool) -> bool {
    asset.category == AgentAssetCategory::Mcp
        && asset.role == AgentAssetDeclarationRole::StateOverlay
        && target_shape(asset)
        && asset.native_id == asset.resolution_group_key
        && asset
            .declaration_key
            .strip_prefix("enablement.entry:")
            .is_some_and(|raw_key| raw_key.trim().to_ascii_lowercase() == asset.native_id)
        && no_parent(asset)
        && if unknown {
            asset.declared_state == AgentAssetDeclaredState::Unknown
        } else {
            matches!(
                asset.declared_state,
                AgentAssetDeclaredState::Enabled | AgentAssetDeclaredState::Disabled
            )
        }
        && matches!(asset.details, AgentAssetDetails::Mcp { transport: AgentMcpTransport::Unknown, declared_state, approval_state: AgentMcpApprovalState::NotRequired, .. } if declared_state == asset.declared_state)
}

fn valid_extension_overlay(asset: &ParsedAgentAsset, unknown: bool) -> bool {
    asset.category == AgentAssetCategory::Extension
        && asset.role == AgentAssetDeclarationRole::StateOverlay
        && asset.resolution_group_key == asset.native_id
        && asset.declaration_key == asset.native_id
        && no_parent(asset)
        && if unknown {
            asset.declared_state == AgentAssetDeclaredState::Unknown
        } else {
            matches!(
                asset.declared_state,
                AgentAssetDeclaredState::Enabled | AgentAssetDeclaredState::Disabled
            )
        }
        && matches!(asset.details, AgentAssetDetails::Extension { enabled, .. } if enabled == asset.declared_state)
}
