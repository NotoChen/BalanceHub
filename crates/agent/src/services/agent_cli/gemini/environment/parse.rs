use super::*;
use crate::{
    models::*,
    services::agent_cli::{contracts::*, environment::*, native_agent_kinds::gemini::AGENT_KIND},
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
};

pub(super) fn parse_settings(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if request.source.categories.contains(&AgentAssetCategory::Mcp) {
        if root.contains_key("mcp") && !root.get("mcp").is_some_and(Value::is_object) {
            emit_malformed(output, "mcp");
            emit_mcp_policy_marker(
                request,
                output,
                "mcp.allowed",
                AgentMcpPolicyPayload::InvalidAllowed,
            )?;
            emit_mcp_policy_marker(
                request,
                output,
                "mcp.excluded",
                AgentMcpPolicyPayload::InvalidExcluded,
            )?;
        }
        if let Some(mcp) = root.get("mcp").and_then(Value::as_object) {
            if let Some(values) = mcp.get("allowed") {
                emit_policy(request, output, "mcp.allowed", values, true)?;
            }
            if let Some(values) = mcp.get("excluded") {
                emit_policy(request, output, "mcp.excluded", values, false)?;
            }
        }
        // Gemini 0.59 replaces the entire local admin object with schema
        // defaults and authenticated session policy. Neither is a local source.
        parse_mcp_servers(request, root, output)?;
    }
    if request
        .source
        .categories
        .contains(&AgentAssetCategory::Skill)
    {
        super::skill::parse_disabled_policy(request, root, output)?;
    }
    if request
        .source
        .categories
        .contains(&AgentAssetCategory::Hook)
    {
        emit_hook_policy(request, root, output)?;
        parse_hooks(request, root, output)?;
    }
    if request
        .source
        .categories
        .contains(&AgentAssetCategory::StatusUi)
    {
        parse_footer(request, root, output)?;
    }
    ControlFlow::Continue(())
}

pub(super) fn unknown_mcp_details() -> AgentAssetDetails {
    AgentAssetDetails::Mcp {
        transport: AgentMcpTransport::Unknown,
        declared_state: AgentAssetDeclaredState::Unknown,
        approval_state: AgentMcpApprovalState::NotRequired,
        effective_availability: AgentAssetEffectiveAvailability::Unknown,
    }
}

fn emit_policy(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    id: &str,
    value: &Value,
    allowed: bool,
) -> ControlFlow<AgentOutputStop> {
    let Some(values) = value.as_array() else {
        emit_malformed(output, id);
        return emit_mcp_policy_marker(
            request,
            output,
            id,
            if allowed {
                AgentMcpPolicyPayload::InvalidAllowed
            } else {
                AgentMcpPolicyPayload::InvalidExcluded
            },
        );
    };
    let Some(members) = values
        .iter()
        .map(Value::as_str)
        .map(|value| value.map(|item| item.trim().to_ascii_lowercase()))
        .collect::<Option<BTreeSet<_>>>()
    else {
        emit_malformed(output, id);
        return emit_mcp_policy_marker(
            request,
            output,
            id,
            if allowed {
                AgentMcpPolicyPayload::InvalidAllowed
            } else {
                AgentMcpPolicyPayload::InvalidExcluded
            },
        );
    };
    let payload = if allowed {
        AgentMcpPolicyPayload::Allowed(members)
    } else {
        AgentMcpPolicyPayload::Excluded(members)
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: id,
            resolution_group_key: id,
            category: AgentAssetCategory::Mcp,
            native_id: id,
            label: id,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::PolicyOverlay,
            participation: mcp_participation(request),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: unknown_mcp_details(),
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::McpPolicy(payload);
    output.emit_declaration(declaration)
}

pub(super) fn emit_mcp_policy_marker(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    id: &str,
    payload: AgentMcpPolicyPayload,
) -> ControlFlow<AgentOutputStop> {
    let declaration_key = match &payload {
        AgentMcpPolicyPayload::InvalidAllowed | AgentMcpPolicyPayload::InvalidExcluded => {
            format!("{id}.invalid")
        }
        _ => id.to_owned(),
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &declaration_key,
            resolution_group_key: id,
            category: AgentAssetCategory::Mcp,
            native_id: id,
            label: id,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::PolicyOverlay,
            participation: mcp_participation(request),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: unknown_mcp_details(),
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::McpPolicy(payload);
    output.emit_declaration(declaration)
}

/// Each override entry is validated in full before context evaluation.
pub(super) fn parse_extension_enablement(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) {
    let evaluation_paths = extension_evaluation_paths(request);
    for (name, value) in bounded_object_entries(root) {
        let rules = value
            .as_object()
            .and_then(|config| config.get("overrides"))
            .and_then(Value::as_array)
            .and_then(|overrides| {
                overrides
                    .iter()
                    .map(|value| value.as_str().and_then(extension_override_pattern))
                    .collect::<Option<Vec<_>>>()
            });
        let (state, unknown) = match rules {
            None => {
                emit_malformed(output, name);
                (
                    AgentAssetDeclaredState::Unknown,
                    Some(GeminiExtensionEnablementUnknownCause::InvalidEntry),
                )
            }
            Some(rules) if !rules.is_empty() && evaluation_paths.is_empty() => (
                AgentAssetDeclaredState::Unknown,
                Some(GeminiExtensionEnablementUnknownCause::WorkspaceUnavailable),
            ),
            Some(rules) => {
                let mut state = AgentAssetDeclaredState::Enabled;
                for (disabled, pattern) in rules {
                    if evaluation_paths
                        .iter()
                        .any(|workspace| glob_matches(&pattern, workspace))
                    {
                        state = if disabled {
                            AgentAssetDeclaredState::Disabled
                        } else {
                            AgentAssetDeclaredState::Enabled
                        };
                    }
                }
                (state, None)
            }
        };
        if emit_extension_state(request, output, name, state, unknown).is_break() {
            return;
        }
    }
}

fn extension_evaluation_paths(request: AgentAssetParseRequest<'_>) -> Vec<String> {
    let workspace_paths = [request.workspace_canonical, request.workspace_lexical];
    // Gemini's native listing computes userEnabled at homedir(). This does not
    // turn the default context into a trusted or discovered workspace.
    let native_home =
        if request.context.workspace_id.is_none() && workspace_paths.iter().all(Option::is_none) {
            request.native_home.filter(|home| home.is_absolute())
        } else {
            None
        };
    workspace_paths
        .into_iter()
        .chain([native_home])
        .flatten()
        .map(normalize_workspace_path)
        .fold(Vec::new(), |mut forms, value| {
            if !forms.contains(&value) {
                forms.push(value);
            }
            forms
        })
}

fn normalize_workspace_path(path: &std::path::Path) -> String {
    normalize_gemini_slashes(&path.to_string_lossy())
}

fn emit_extension_state(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    name: &str,
    state: AgentAssetDeclaredState,
    unknown: Option<GeminiExtensionEnablementUnknownCause>,
) -> ControlFlow<AgentOutputStop> {
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: name,
            resolution_group_key: name,
            category: AgentAssetCategory::Extension,
            native_id: name,
            label: name,
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: AgentAssetDeclarationRole::StateOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Extension {
                install_state: AgentAssetInstallState::Installed,
                enabled: state,
                trusted: AgentTrustState::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    if let Some(cause) = unknown {
        declaration.native_payload =
            AgentAssetNativePayload::GeminiExtensionEnablementUnknown(cause);
    }
    output.emit_declaration(declaration)
}

fn extension_override_pattern(rule: &str) -> Option<(bool, String)> {
    if rule.is_empty() || rule == "!" || rule.chars().any(char::is_control) || rule.contains('\0') {
        return None;
    }
    let disabled = rule.starts_with('!');
    let mut pattern = if disabled { &rule[1..] } else { rule };
    let include_subdirs = pattern.ends_with('*');
    if include_subdirs {
        pattern = &pattern[..pattern.len() - 1];
    }
    let mut pattern = normalize_gemini_slashes(pattern);
    if include_subdirs {
        pattern.push('*');
    }
    Some((disabled, pattern))
}

fn normalize_gemini_slashes(value: &str) -> String {
    let mut value = value.replace('\\', "/");
    if !value.starts_with('/') {
        value.insert(0, '/');
    }
    if !value.ends_with('/') {
        value.push('/');
    }
    value
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for &token in pattern {
        let mut current = vec![false; value.len() + 1];
        if token == b'*' {
            current[0] = previous[0];
            for index in 1..=value.len() {
                current[index] = previous[index] || current[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                current[index] = previous[index - 1] && token == value[index - 1];
            }
        }
        previous = current;
    }
    previous[value.len()]
}
pub(super) fn parse_extension_manifest(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let value = match serde_json::from_slice::<Value>(bytes) {
        Ok(value) => value,
        Err(_) => {
            malformed_extension_manifest(output);
            return;
        }
    };
    let Some(root) = value.as_object() else {
        malformed_extension_manifest(output);
        return;
    };
    let Some(name) = root.get("name").and_then(Value::as_str) else {
        malformed_extension_manifest(output);
        return;
    };
    let Some(version) = root.get("version").and_then(Value::as_str).map(str::trim) else {
        malformed_extension_manifest(output);
        return;
    };
    if !is_valid_extension_name(name) || version.is_empty() {
        malformed_extension_manifest(output);
        return;
    }

    let Some(extension_entry) = request
        .source
        .native_source_key
        .strip_prefix(EXTENSION_MANIFEST_PREFIX)
    else {
        malformed_extension_manifest(output);
        return;
    };
    let parent_ref = extension_ref(extension_entry);
    let mut children = Vec::new();
    if let Some(servers) = root.get("mcpServers").and_then(Value::as_object) {
        for (native_id, value) in bounded_object_entries(servers) {
            children.push(mcp_ref(native_id));
            if emit_mcp(
                request,
                output,
                native_id,
                value,
                &format!("extension:{extension_entry}:{native_id}"),
                mcp_participation(request),
                Some(parent_ref.clone()),
            )
            .is_break()
            {
                return;
            }
        }
    }
    let declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: extension_entry,
            resolution_group_key: extension_entry,
            category: AgentAssetCategory::Extension,
            native_id: extension_entry,
            label: name,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: children,
            details: AgentAssetDetails::Extension {
                install_state: crate::models::AgentAssetInstallState::Installed,
                enabled: AgentAssetDeclaredState::Unknown,
                trusted: AgentTrustState::Unknown,
            },
            facts: BTreeMap::from([("version".to_owned(), version.to_owned())]),
        },
    );
    let _ = output.emit_declaration(declaration);
}

fn malformed_extension_manifest(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Manifest,
        location: None,
    });
}

fn is_valid_extension_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(super) fn parse_mcp_servers(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    if root.contains_key("mcpServers") && !root.get("mcpServers").is_some_and(Value::is_object) {
        emit_malformed(output, "mcpServers");
        return ControlFlow::Continue(());
    }
    let Some(servers) = root.get("mcpServers").and_then(Value::as_object) else {
        return ControlFlow::Continue(());
    };
    for (native_id, value) in bounded_object_entries(servers) {
        if emit_mcp(
            request,
            output,
            native_id,
            value,
            &format!("mcpServers.{native_id}"),
            mcp_participation(request),
            None,
        )
        .is_break()
        {
            return ControlFlow::Break(AgentOutputStop::EntryLimit);
        }
    }
    ControlFlow::Continue(())
}

pub(super) fn extension_ref(extension: &str) -> crate::models::AgentAssetNativeRef {
    crate::models::AgentAssetNativeRef {
        category: AgentAssetCategory::Extension,
        native_id: extension.to_owned(),
        qualifier: Some(format!("extension:{extension}")),
    }
}

fn mcp_ref(native_id: &str) -> crate::models::AgentAssetNativeRef {
    crate::models::AgentAssetNativeRef {
        category: AgentAssetCategory::Mcp,
        native_id: native_id.to_owned(),
        qualifier: None,
    }
}

struct DecodedGeminiMcpTransport {
    transport: AgentMcpTransport,
    identity: AgentMcpMatcherIdentity,
}

fn gemini_transport(
    value: &Value,
    output: &mut dyn AgentDiagnosticOutput,
    location: &str,
) -> Option<DecodedGeminiMcpTransport> {
    let definition = match crate::services::agent_cli::catalog::native::GEMINI.decode(value) {
        Ok(definition) => definition,
        Err(_) => {
            emit_malformed(output, location);
            return None;
        }
    };
    let identity = if let Some(command) = definition.command {
        let mut argv = vec![command];
        argv.extend(definition.args);
        AgentMcpMatcherIdentity::Stdio { argv }
    } else {
        AgentMcpMatcherIdentity::Remote {
            url: definition.url?,
        }
    };
    Some(DecodedGeminiMcpTransport {
        transport: definition.transport,
        identity,
    })
}

fn emit_mcp(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    native_id: &str,
    value: &Value,
    declaration_key: &str,
    participation: crate::models::AgentAssetResolutionParticipation,
    parent: Option<crate::models::AgentAssetNativeRef>,
) -> ControlFlow<AgentOutputStop> {
    let decoded = gemini_transport(value, output, &format!("mcpServers.{native_id}"));
    if let Some(object) = value.as_object() {
        emit_unknown_fields(
            object,
            &[
                "command",
                "args",
                "env",
                "cwd",
                "url",
                "httpUrl",
                "type",
                "transport",
                "authProviderType",
                "targetAudience",
                "targetServiceAccount",
                "headers",
                "timeout",
                "trust",
                "includeTools",
                "excludeTools",
                "oauth",
            ],
            &format!("mcpServers.{native_id}"),
            output,
        );
    }
    output.emit_declaration(decoded_mcp(
        request,
        native_id,
        declaration_key,
        participation,
        parent,
        decoded,
    ))
}

fn decoded_mcp(
    request: AgentAssetParseRequest<'_>,
    native_id: &str,
    declaration_key: &str,
    participation: crate::models::AgentAssetResolutionParticipation,
    parent: Option<crate::models::AgentAssetNativeRef>,
    decoded: Option<DecodedGeminiMcpTransport>,
) -> ParsedAgentAsset {
    let transport = decoded
        .as_ref()
        .map_or(AgentMcpTransport::Unknown, |value| value.transport);
    let payload = decoded
        .map(|value| {
            AgentAssetNativePayload::McpDefinition(AgentMcpDefinitionPayload {
                identity: value.identity,
                origin: AgentMcpDefinitionOrigin::Declared,
            })
        })
        .unwrap_or_default();
    let state = if transport == AgentMcpTransport::Unknown {
        AgentAssetDeclaredState::Unknown
    } else {
        AgentAssetDeclaredState::Enabled
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key,
            resolution_group_key: &native_id.trim().to_ascii_lowercase(),
            category: AgentAssetCategory::Mcp,
            native_id,
            label: native_id,
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation,
            provided_by: parent.clone(),
            action_owner: parent,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Mcp {
                transport,
                declared_state: state,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: if state == AgentAssetDeclaredState::Unknown {
                    AgentAssetEffectiveAvailability::Unknown
                } else {
                    AgentAssetEffectiveAvailability::Available
                },
            },
            facts: BTreeMap::from([("transport".to_owned(), format!("{transport:?}"))]),
        },
    );
    declaration.native_payload = payload;
    declaration
}

pub(super) fn parse_enablement(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for (native_id, value) in bounded_object_entries(root) {
        let enabled = value
            .as_object()
            .and_then(|object| object.get("enabled"))
            .and_then(Value::as_bool);
        let Some(enabled) = enabled else {
            emit_malformed(output, native_id);
            if emit_enablement_state(request, output, native_id, AgentAssetDeclaredState::Unknown)
                .is_break()
            {
                return ControlFlow::Break(AgentOutputStop::EntryLimit);
            }
            continue;
        };
        let declared_state = if enabled {
            AgentAssetDeclaredState::Enabled
        } else {
            AgentAssetDeclaredState::Disabled
        };
        if let ControlFlow::Break(reason) =
            emit_enablement_state(request, output, native_id, declared_state)
        {
            return ControlFlow::Break(reason);
        }
    }
    ControlFlow::Continue(())
}

fn emit_enablement_state(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    native_id: &str,
    declared_state: AgentAssetDeclaredState,
) -> ControlFlow<AgentOutputStop> {
    let normalized_id = native_id.trim().to_ascii_lowercase();
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &format!("enablement.entry:{native_id}"),
            resolution_group_key: &normalized_id,
            category: AgentAssetCategory::Mcp,
            native_id: &normalized_id,
            label: native_id,
            logical_origin: physical_origin(request.source),
            declared_state,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::StateOverlay,
            participation: mcp_participation(request),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Unknown,
                declared_state,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: availability_from_declared(declared_state),
            },
            facts: BTreeMap::new(),
        },
    );
    if declared_state == AgentAssetDeclaredState::Unknown {
        declaration.native_payload = AgentAssetNativePayload::GeminiMcpEnablementInvalid;
    }
    output.emit_declaration(declaration)
}

pub(super) fn hook_incomplete(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: AGENT_KIND,
        category: AgentAssetCategory::Hook,
        reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
    });
}

fn emit_hook_malformed(output: &mut dyn AgentDiagnosticOutput, location: &str) {
    emit_malformed(output, location);
    hook_incomplete(output);
}

fn parse_hooks(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("hooks") else {
        return ControlFlow::Continue(());
    };
    let Some(hooks) = value.as_object() else {
        emit_hook_malformed(output, "hooks");
        return ControlFlow::Continue(());
    };
    emit_hook_events(request, hooks, None, output)
}

fn emit_hook_policy(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(config) = root.get("hooksConfig") else {
        return ControlFlow::Continue(());
    };
    let Some(config) = config.as_object() else {
        emit_malformed(output, "hooksConfig");
        return emit_invalid_hook_policy(request, output);
    };
    if let Some(value) = config.get("enabled") {
        let enabled = value.as_bool();
        let key = if enabled.is_some() {
            "hooksConfig.enabled"
        } else {
            emit_malformed(output, "hooksConfig.enabled");
            "hooksConfig.enabled.invalid"
        };
        emit_hook_policy_declaration(
            request,
            AgentHookPolicyPayload::GlobalEnabled(enabled),
            key,
            output,
        )?;
    }
    let payload = match hooks::disabled_names(root) {
        Ok(None) => return ControlFlow::Continue(()),
        Ok(Some(values)) => AgentHookPolicyPayload::DisabledSet(
            values
                .into_iter()
                .map(|value| AgentHookDisableMatcherIdentity::new(value.to_owned()))
                .collect(),
        ),
        Err(location) => {
            emit_malformed(output, location);
            AgentHookPolicyPayload::InvalidDisabledSet
        }
    };
    emit_hook_policy_declaration(request, payload, "hooksConfig.disabled", output)
}

fn emit_hook_policy_declaration(
    request: AgentAssetParseRequest<'_>,
    payload: AgentHookPolicyPayload,
    key: &str,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let (native_id, label) = if matches!(payload, AgentHookPolicyPayload::GlobalEnabled(_)) {
        ("hooksConfig.enabled", "Gemini CLI Hook 全局开关")
    } else {
        ("hooksConfig.disabled", "Gemini CLI Hook 禁用策略")
    };
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: key,
            resolution_group_key: native_id,
            category: AgentAssetCategory::Hook,
            native_id,
            label,
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::PolicyOverlay,
            participation: participation(request, AgentAssetCategory::Hook),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Hook {
                managed: false,
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count: Some(0),
            },
            facts: BTreeMap::new(),
        },
    );
    declaration.native_payload = AgentAssetNativePayload::HookPolicy(payload);
    output.emit_declaration(declaration)
}

pub(super) fn emit_invalid_hook_policy(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    emit_hook_policy_declaration(
        request,
        AgentHookPolicyPayload::InvalidDisabledSet,
        "hooksConfig.disabled.invalid",
        output,
    )?;
    emit_hook_policy_declaration(
        request,
        AgentHookPolicyPayload::GlobalEnabled(None),
        "hooksConfig.enabled.invalid",
        output,
    )
}

fn extension_entry_from_source<'a>(source_key: &'a str, prefix: &str) -> Option<&'a str> {
    source_key
        .strip_prefix(prefix)
        .filter(|value| !value.is_empty())
}

pub(super) fn parse_extension_hooks(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let Some(extension) =
        extension_entry_from_source(&request.source.native_source_key, EXTENSION_HOOKS_PREFIX)
    else {
        return;
    };
    let Some(value) = parse_strict_json(bytes, output, AgentAssetCategory::Hook) else {
        hook_incomplete(output);
        return;
    };
    let Some(root) = value.as_object() else {
        emit_hook_malformed(output, "hooks");
        return;
    };
    let Some(events) = root.get("hooks").and_then(Value::as_object) else {
        emit_hook_malformed(output, "hooks");
        return;
    };
    let parent = extension_ref(extension);
    let _ = emit_hook_events(request, events, Some(parent), output);
}

fn emit_hook_events(
    request: AgentAssetParseRequest<'_>,
    events: &Map<String, Value>,
    parent: Option<crate::models::AgentAssetNativeRef>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for (event, definitions) in bounded_object_entries(events) {
        if matches!(event, "enabled" | "disabled" | "notifications") {
            continue;
        }
        if !hooks::valid_event(event) {
            emit_hook_malformed(output, event);
            continue;
        }
        let Some(definitions) = definitions.as_array() else {
            emit_hook_malformed(output, event);
            continue;
        };
        for (definition_index, definition) in definitions.iter().enumerate() {
            let Some(definition) = definition.as_object() else {
                emit_hook_malformed(output, &format!("{event}.{definition_index}"));
                continue;
            };
            let Some(slots) = definition.get("hooks").and_then(Value::as_array) else {
                emit_hook_malformed(output, &format!("{event}.{definition_index}"));
                continue;
            };
            for (hook_index, hook) in slots.iter().enumerate() {
                let location = format!("{event}.{definition_index}.{hook_index}");
                let Some(disable_identity) = hooks::disable_identity(hook) else {
                    emit_hook_malformed(output, &location);
                    continue;
                };
                let native_id = match &parent {
                    Some(parent) => format!(
                        "{}:{event}:{definition_index}:{hook_index}",
                        parent.native_id
                    ),
                    None => format!("{event}:{definition_index}:{hook_index}"),
                };
                let mut declaration = parsed_asset(
                    request,
                    ParsedAssetInput {
                        declaration_key: &native_id,
                        resolution_group_key: &native_id,
                        category: AgentAssetCategory::Hook,
                        native_id: &native_id,
                        label: &format!("Gemini Hook：{event}"),
                        logical_origin: physical_origin(request.source),
                        declared_state: AgentAssetDeclaredState::Unknown,
                        trust_state: request.context.trust_context,
                        role: crate::models::AgentAssetDeclarationRole::Definition,
                        participation: if parent.is_some() {
                            crate::models::AgentAssetResolutionParticipation::Participates
                        } else {
                            participation(request, AgentAssetCategory::Hook)
                        },
                        provided_by: parent.clone(),
                        action_owner: parent.clone(),
                        explicitly_affected: Vec::new(),
                        details: AgentAssetDetails::Hook {
                            managed: false,
                            enabled: AgentAssetDeclaredState::Unknown,
                            rule_count: Some(1),
                        },
                        facts: BTreeMap::new(),
                    },
                );
                declaration.native_payload = AgentAssetNativePayload::HookDefinition(
                    AgentHookDisableMatcherIdentity::new(disable_identity.to_owned()),
                );
                if let ControlFlow::Break(reason) = output.emit_declaration(declaration) {
                    return ControlFlow::Break(reason);
                }
            }
        }
    }
    ControlFlow::Continue(())
}

fn parse_footer(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let value = root
        .get("ui")
        .and_then(Value::as_object)
        .and_then(|ui| ui.get("footer"))
        .or_else(|| root.get("footer"));
    let Some(value) = value else {
        return ControlFlow::Continue(());
    };
    let mode = match value {
        Value::Null | Value::Bool(false) => AgentStatusUiMode::Disabled,
        Value::String(value) if !value.trim().is_empty() => AgentStatusUiMode::Command,
        Value::Object(_) | Value::Bool(true) => AgentStatusUiMode::BuiltIn,
        _ => AgentStatusUiMode::Unknown,
    };
    let state = match mode {
        AgentStatusUiMode::Disabled => AgentAssetDeclaredState::Disabled,
        AgentStatusUiMode::Unknown => AgentAssetDeclaredState::Unknown,
        AgentStatusUiMode::BuiltIn | AgentStatusUiMode::Command => AgentAssetDeclaredState::Enabled,
    };
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "footer",
            resolution_group_key: "footer",
            category: AgentAssetCategory::StatusUi,
            native_id: "footer",
            label: "Gemini CLI Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: participation(request, AgentAssetCategory::StatusUi),
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::StatusUi {
                mode,
                command_present: mode == AgentStatusUiMode::Command,
            },
            facts: BTreeMap::new(),
        },
    ))
}
