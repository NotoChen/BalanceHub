use crate::{
    models::{
        AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
        AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetResolutionParticipation,
        AgentAssetScope, AgentAssetSourceKind, AgentMcpApprovalState, AgentMcpTransport,
        AgentSkillInvocationPolicy, AgentStatusUiMode, AgentTrustState,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot,
            AgentDiagnosticOutput, AgentOutputStop, AgentParseOutput, GrokStateControl,
            ParsedAgentAsset,
        },
        environment::{
            availability_from_declared, bounded_object_entries, emit_unknown_fields, parsed_asset,
            physical_origin, ParsedAssetInput,
        },
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
};

#[cfg(test)]
mod tests;

pub(in crate::services::agent_cli::grok) fn parse_assets(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    if request.source.categories.is_empty() {
        return;
    }
    if super::plugin::parse_source(request, output) {
        return;
    }
    if super::hooks::parse_source(request, output) {
        return;
    }
    if request.source.source_kind == AgentAssetSourceKind::Directory {
        // Directory entries require their bounded native file snapshots.
        return;
    }
    if request.source.native_source_key == "auth"
        || super::trust::is_authority_source(&request.source.native_source_key)
    {
        return;
    }
    if request
        .source
        .native_source_key
        .starts_with(super::skill::MANIFEST_PREFIX)
    {
        super::skill::parse_manifest(request, output);
        return;
    }
    let root = match request.snapshot {
        AgentAssetSnapshot::Missing { .. } => return,
        AgentAssetSnapshot::File { bytes, .. } => std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| text.parse::<toml::Value>().ok())
            .and_then(|value| serde_json::to_value(value).ok())
            .and_then(|value| value.as_object().cloned()),
        _ => None,
    };
    let Some(root) = root else {
        if matches!(
            request.source.native_source_key.as_str(),
            "config" | "workspace-config"
        ) {
            output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: super::super::AGENT_KIND,
                category: AgentAssetCategory::Hook,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            });
        }
        if matches!(request.snapshot, AgentAssetSnapshot::File { .. }) {
            malformed_toml(output);
        }
        if emit_invalid(request, GrokStateControl::InvalidMcp, "source", output).is_break() {
            return;
        }
        if emit_invalid(request, GrokStateControl::InvalidPlugin, "source", output).is_break() {
            return;
        }
        if request.source.scope == AgentAssetScope::User
            && request
                .source
                .categories
                .contains(&AgentAssetCategory::Skill)
        {
            let _ = emit_invalid(request, GrokStateControl::InvalidSkill, "source", output);
        }
        return;
    };
    report_uncovered_entries(&root, output);
    if parse_mcp_servers(request, &root, output).is_break() {
        return;
    }
    if parse_plugins(request, &root, output).is_break() {
        return;
    }
    // Project config only supplies the native project sections; status UI and
    // personal MCP disablement belong to user config.
    if request.source.scope != AgentAssetScope::Workspace {
        if parse_skills(request, &root, output).is_break() {
            return;
        }
        let _ = parse_status_ui(request, &root, output);
    }
}

fn report_uncovered_entries(root: &Map<String, Value>, output: &mut dyn AgentDiagnosticOutput) {
    let configured = |value: &Value| match value {
        Value::Array(values) => !values.is_empty(),
        Value::String(value) => !value.trim().is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Bool(value) => *value,
        Value::Null => false,
        _ => true,
    };
    for (section, category) in [
        ("skills", AgentAssetCategory::Skill),
        ("plugins", AgentAssetCategory::Plugin),
    ] {
        if root
            .get(section)
            .and_then(Value::as_object)
            .is_some_and(|section| {
                ["paths", "extra_paths", "extraPaths"]
                    .iter()
                    .any(|key| section.get(*key).is_some_and(configured))
            })
        {
            output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: super::super::AGENT_KIND,
                category,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
            });
        }
    }
    if root.get("compat").is_some_and(configured) {
        for category in [
            AgentAssetCategory::Skill,
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Hook,
        ] {
            output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: super::super::AGENT_KIND,
                category,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
            });
        }
    }
}

fn malformed_toml(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Toml,
        location: None,
    });
}

pub(super) fn mcp_transport(value: &Value) -> AgentMcpTransport {
    crate::services::agent_cli::catalog::native::GROK
        .decode(value)
        .map_or(AgentMcpTransport::Unknown, |definition| {
            definition.transport
        })
}

fn parse_mcp_servers(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if let Some(value) = root.get("mcp_servers") {
        if let Some(servers) = value.as_object() {
            for (native_id, value) in bounded_object_entries(servers) {
                // Native server names must remain disjoint from list-control
                // declaration keys in the same source and category.
                let declaration_key = format!("mcp_servers.{native_id}");
                let enabled = value.get("enabled").and_then(Value::as_bool);
                let transport = mcp_transport(value);
                let valid = value.is_object()
                    && transport != AgentMcpTransport::Unknown
                    && value.get("enabled").is_none_or(Value::is_boolean);
                let state = if valid {
                    enabled
                        .map(|value| {
                            if value {
                                AgentAssetDeclaredState::Enabled
                            } else {
                                AgentAssetDeclaredState::Disabled
                            }
                        })
                        .unwrap_or(AgentAssetDeclaredState::Unknown)
                } else {
                    AgentAssetDeclaredState::Unknown
                };
                if !valid {
                    malformed_toml(output);
                }
                if let Some(object) = value.as_object() {
                    emit_unknown_fields(
                        object,
                        &[
                            "enabled",
                            "command",
                            "args",
                            "env",
                            "cwd",
                            "url",
                            "urlTemplate",
                            "url_template",
                            "bearer_token_env_var",
                            "headers",
                            "oauth_client_id",
                            "oauth_client_secret_env_var",
                            "oauth_scopes",
                            "oauth",
                            "setup",
                            "startup_timeout_sec",
                            "tool_timeout_sec",
                            "tool_timeouts",
                            "expose_image_base64",
                            "type",
                        ],
                        &declaration_key,
                        output,
                    );
                }
                let mut asset = parsed_asset(
                    request,
                    ParsedAssetInput {
                        declaration_key: &declaration_key,
                        resolution_group_key: native_id,
                        category: AgentAssetCategory::Mcp,
                        native_id,
                        label: native_id,
                        logical_origin: physical_origin(request.source),
                        declared_state: state,
                        trust_state: request.context.trust_context,
                        role: AgentAssetDeclarationRole::Definition,
                        participation: AgentAssetResolutionParticipation::Participates,
                        provided_by: None,
                        action_owner: None,
                        explicitly_affected: Vec::new(),
                        details: AgentAssetDetails::Mcp {
                            transport,
                            declared_state: state,
                            approval_state: AgentMcpApprovalState::NotRequired,
                            effective_availability: availability_from_declared(state),
                        },
                        facts: BTreeMap::new(),
                    },
                );
                asset.native_payload =
                    AgentAssetNativePayload::GrokMcpDefinition { enabled, valid };
                output.emit_declaration(asset)?;
            }
        } else {
            emit_invalid(request, GrokStateControl::InvalidMcp, "mcp_servers", output)?;
        }
    }
    if request.source.scope != AgentAssetScope::Workspace {
        parse_control_list(
            request,
            root.get("disabled_mcp_servers"),
            GrokStateControl::McpDisabled,
            "disabled_mcp_servers",
            output,
        )?;
    }
    ControlFlow::Continue(())
}

fn parse_plugins(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("plugins") else {
        return ControlFlow::Continue(());
    };
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: super::super::AGENT_KIND,
        category: AgentAssetCategory::Plugin,
        reason: crate::models::AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved,
    });
    let Some(plugins) = value.as_object() else {
        return emit_invalid(request, GrokStateControl::InvalidPlugin, "plugins", output);
    };
    if request.source.scope != AgentAssetScope::Workspace {
        parse_control_list(
            request,
            plugins.get("enabled"),
            GrokStateControl::PluginEnabled,
            "plugins.enabled",
            output,
        )?;
    }
    parse_control_list(
        request,
        plugins.get("disabled"),
        GrokStateControl::PluginDisabled,
        "plugins.disabled",
        output,
    )
}

fn parse_control_list(
    request: AgentAssetParseRequest<'_>,
    value: Option<&Value>,
    control: GrokStateControl,
    key: &str,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = value else {
        return ControlFlow::Continue(());
    };
    let invalid = match control {
        GrokStateControl::McpDisabled => GrokStateControl::InvalidMcp,
        GrokStateControl::SkillDisabled => GrokStateControl::InvalidSkill,
        _ => GrokStateControl::InvalidPlugin,
    };
    let Some(values) = value.as_array() else {
        return emit_invalid(request, invalid, key, output);
    };
    if values.iter().any(|value| value.as_str().is_none()) {
        if control == GrokStateControl::SkillDisabled {
            // Skill state comes from a complete native string list. A partial
            // malformed list cannot certify any of its proposed memberships.
            return emit_invalid(request, invalid, key, output);
        }
        emit_invalid(request, invalid, key, output)?;
    }
    // Native lists describe membership, so a repeated normalized ID is one
    // control. Offering duplicate declaration IDs would make the common sink
    // reject the control entirely and could incorrectly restore a default.
    let native_ids = values
        .iter()
        .filter_map(Value::as_str)
        .map(|id| {
            if control == GrokStateControl::SkillDisabled {
                id
            } else {
                id.trim()
            }
        })
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>();
    for native_id in native_ids {
        output.emit_declaration(control_asset(
            request,
            control,
            &format!("{key}:{native_id}"),
            native_id,
        ))?;
    }
    ControlFlow::Continue(())
}

fn parse_skills(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if !request
        .source
        .categories
        .contains(&AgentAssetCategory::Skill)
    {
        return ControlFlow::Continue(());
    }
    let Some(skills) = root.get("skills") else {
        return ControlFlow::Continue(());
    };
    let Some(skills) = skills.as_object() else {
        return emit_invalid(request, GrokStateControl::InvalidSkill, "skills", output);
    };
    parse_control_list(
        request,
        skills.get("disabled"),
        GrokStateControl::SkillDisabled,
        "skills.disabled",
        output,
    )
}

fn emit_invalid(
    request: AgentAssetParseRequest<'_>,
    control: GrokStateControl,
    key: &str,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    malformed_toml(output);
    output.emit_declaration(control_asset(
        request,
        control,
        &format!("invalid:{key}"),
        "__grok_invalid_control__",
    ))
}

fn control_asset(
    request: AgentAssetParseRequest<'_>,
    control: GrokStateControl,
    key: &str,
    native_id: &str,
) -> ParsedAgentAsset {
    let category = match control {
        GrokStateControl::McpDisabled | GrokStateControl::InvalidMcp => AgentAssetCategory::Mcp,
        GrokStateControl::SkillDisabled | GrokStateControl::InvalidSkill => {
            AgentAssetCategory::Skill
        }
        GrokStateControl::PluginEnabled
        | GrokStateControl::PluginDisabled
        | GrokStateControl::InvalidPlugin => AgentAssetCategory::Plugin,
    };
    let state = match control {
        GrokStateControl::McpDisabled | GrokStateControl::SkillDisabled => {
            AgentAssetDeclaredState::Disabled
        }
        // 1.0.24 inspect did not establish these switches. Preserve list
        // membership in the private payload without claiming enabled state.
        GrokStateControl::PluginEnabled | GrokStateControl::PluginDisabled => {
            AgentAssetDeclaredState::Unknown
        }
        _ => AgentAssetDeclaredState::Unknown,
    };
    let details = if category == AgentAssetCategory::Mcp {
        AgentAssetDetails::Mcp {
            transport: AgentMcpTransport::Unknown,
            declared_state: state,
            approval_state: AgentMcpApprovalState::NotRequired,
            effective_availability: availability_from_declared(state),
        }
    } else if category == AgentAssetCategory::Skill {
        AgentAssetDetails::Skill {
            enabled: state,
            invocation_policy: AgentSkillInvocationPolicy::Unknown,
        }
    } else {
        AgentAssetDetails::Plugin {
            install_state: crate::models::AgentAssetInstallState::Unknown,
            enabled: state,
            trusted: AgentTrustState::Unknown,
        }
    };
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: key,
            resolution_group_key: native_id,
            category,
            native_id,
            label: native_id,
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::StateOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details,
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload = AgentAssetNativePayload::GrokStateControl(control);
    asset
}

fn parse_status_ui(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("status_line").or_else(|| root.get("statusLine")) else {
        return ControlFlow::Continue(());
    };
    let command_present = matches!(value, Value::String(text) if !text.trim().is_empty())
        || value
            .as_object()
            .and_then(|object| object.get("command"))
            .is_some();
    let mode = match value {
        Value::Null | Value::Bool(false) => AgentStatusUiMode::Disabled,
        _ if command_present => AgentStatusUiMode::Command,
        Value::Object(_) | Value::Bool(true) => AgentStatusUiMode::BuiltIn,
        _ => AgentStatusUiMode::Unknown,
    };
    let state = match mode {
        AgentStatusUiMode::Disabled => AgentAssetDeclaredState::Disabled,
        AgentStatusUiMode::Unknown => AgentAssetDeclaredState::Unknown,
        _ => AgentAssetDeclaredState::Enabled,
    };
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "status-line",
            resolution_group_key: "status-line",
            category: AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Grok Build Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::Definition,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::StatusUi {
                mode,
                command_present,
            },
            facts: BTreeMap::new(),
        },
    ))
}
