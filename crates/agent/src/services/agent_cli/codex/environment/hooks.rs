//! Exact passive schema and persisted user state for Codex rust-v0.154.0.
//! Hook trust is independent of workspace trust; neither reader grants it.
mod decoded;
pub(super) mod feature;
mod hash;
pub(super) use decoded::decode_definition;
pub(super) use hash::normalized_hash;

use crate::models::*;
use crate::services::agent_cli::{
    contracts::{
        AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot, AgentDiagnosticOutput,
        AgentParseOutput,
    },
    environment::{parsed_asset, physical_origin, ParsedAssetInput},
    native_agent_kinds::codex::AGENT_KIND,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CodexHookState {
    pub enabled: Option<bool>,
    pub trusted_hash: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CodexPluginHookBinding {
    pub plugin_id: String,
    pub manifest_key: String,
    pub component_index: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CodexHookDefinitionEvidence {
    pub event: String,
    pub group_index: usize,
    pub handler_index: usize,
    pub group: Value,
    pub plugin: Option<CodexPluginHookBinding>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CodexHookPayload {
    Definition {
        key: String,
        current_hash: String,
        managed: bool,
        evidence: Box<CodexHookDefinitionEvidence>,
    },
    /// None means the user state source could not be completely observed.
    Policy {
        states: Option<BTreeMap<String, CodexHookState>>,
    },
    FeaturePolicy {
        layer: feature::HookFeatureLayer,
    },
    PluginManifest {
        manifest: super::native::plugin_hooks::HookManifest,
    },
}

pub(in crate::services::agent_cli::codex) fn incomplete(
    output: &mut dyn AgentDiagnosticOutput,
    reason: AgentAssetDiscoveryIncompleteReason,
) {
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: AGENT_KIND,
        category: AgentAssetCategory::Hook,
        reason,
    });
}

pub(super) fn configured_source(value: &Value) -> bool {
    match value {
        Value::Object(object) => !object.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

pub(in crate::services::agent_cli::codex) fn event_key(event: &str) -> Option<&'static str> {
    Some(match event {
        "PreToolUse" => "pre_tool_use",
        "PermissionRequest" => "permission_request",
        "PostToolUse" => "post_tool_use",
        "PreCompact" => "pre_compact",
        "PostCompact" => "post_compact",
        "SessionStart" => "session_start",
        "SessionEnd" => "session_end",
        "UserPromptSubmit" => "user_prompt_submit",
        "SubagentStart" => "subagent_start",
        "SubagentStop" => "subagent_stop",
        "Stop" => "stop",
        "Interrupt" => "interrupt",
        _ => return None,
    })
}

pub(in crate::services::agent_cli::codex) fn valid_event(event: &str) -> bool {
    event_key(event).is_some()
}

pub(in crate::services::agent_cli::codex) fn native_id(
    source: &str,
    event: &str,
    group: usize,
    handler: usize,
) -> String {
    format!(
        "{source}:{}:{group}:{handler}",
        event_key(event).unwrap_or(event)
    )
}

pub(in crate::services::agent_cli::codex) fn valid_group_metadata(
    group: &Map<String, Value>,
) -> bool {
    group
        .get("matcher")
        .is_none_or(|value| value.is_null() || value.is_string())
        && group.get("hooks").is_none_or(Value::is_array)
}

fn optional_string(handler: &Map<String, Value>, key: &str) -> bool {
    handler
        .get(key)
        .is_none_or(|value| value.is_null() || value.is_string())
}

fn schema_handler(value: &Value) -> bool {
    let Some(handler) = value.as_object() else {
        return false;
    };
    match handler.get("type").and_then(Value::as_str) {
        Some("prompt" | "agent") => true,
        Some(kind @ ("command" | "mcp_tool")) => {
            if !optional_string(handler, "statusMessage")
                || handler
                    .get("timeout")
                    .is_some_and(|value| !value.is_null() && value.as_u64().is_none())
            {
                return false;
            }
            if kind == "command" {
                handler.get("command").is_some_and(Value::is_string)
                    && optional_string(handler, "commandWindows")
                    && optional_string(handler, "command_windows")
                    && !(handler.contains_key("commandWindows")
                        && handler.contains_key("command_windows"))
                    && handler.get("async").is_none_or(Value::is_boolean)
                    && handler.get("additionalContextLimit").is_none_or(|value| {
                        value.is_null()
                            || value
                                .as_u64()
                                .and_then(|value| usize::try_from(value).ok())
                                .is_some()
                    })
            } else {
                handler.get("server").is_some_and(Value::is_string)
                    && handler.get("tool").is_some_and(Value::is_string)
                    && handler.get("input").is_none_or(|value| {
                        value.is_object() && toml::Value::try_from(value).is_ok()
                    })
            }
        }
        _ => false,
    }
}

pub(in crate::services::agent_cli::codex) fn valid_handler(value: &Value) -> bool {
    if !schema_handler(value) {
        return false;
    }
    let nonempty = |key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    };
    match value.get("type").and_then(Value::as_str) {
        Some("command") => {
            if cfg!(windows)
                && value
                    .get("commandWindows")
                    .or_else(|| value.get("command_windows"))
                    .is_some_and(Value::is_string)
            {
                value
                    .get("commandWindows")
                    .or_else(|| value.get("command_windows"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            } else {
                nonempty("command")
            }
        }
        Some("mcp_tool") => nonempty("server") && nonempty("tool"),
        _ => false,
    }
}

pub(in crate::services::agent_cli::codex) fn effective_matcher<'a>(
    event: &str,
    group: &'a Value,
) -> Option<&'a str> {
    if matches!(event, "UserPromptSubmit" | "Stop" | "Interrupt") {
        None
    } else {
        group.get("matcher").and_then(Value::as_str)
    }
}

pub(in crate::services::agent_cli::codex) fn valid_matcher(event: &str, group: &Value) -> bool {
    effective_matcher(event, group).is_none_or(|matcher| regex::Regex::new(matcher).is_ok())
}

pub(in crate::services::agent_cli::codex) fn valid_document(
    root: &Map<String, Value>,
    standalone: bool,
) -> bool {
    if standalone
        && (root
            .keys()
            .any(|key| !matches!(key.as_str(), "description" | "hooks"))
            || root
                .get("description")
                .is_some_and(|value| !value.is_null() && !value.is_string()))
    {
        return false;
    }
    let Some(hooks) = root.get("hooks") else {
        return true;
    };
    let Some(events) = hooks.as_object() else {
        return false;
    };
    events
        .iter()
        .filter(|(event, _)| valid_event(event))
        .all(|(_, groups)| {
            groups
                .as_array()
                .is_some_and(|groups| groups.iter().all(valid_group))
        })
}

pub(super) fn valid_group(group: &Value) -> bool {
    group.as_object().is_some_and(valid_group_metadata)
        && group.get("hooks").is_none_or(|handlers| {
            handlers
                .as_array()
                .is_some_and(|handlers| handlers.iter().all(schema_handler))
        })
}

pub(in crate::services::agent_cli::codex) fn walk_slots(
    root: &Map<String, Value>,
    mut emit: impl FnMut(&str, usize, usize) -> bool,
) -> bool {
    if !valid_document(root, false) {
        return false;
    }
    let Some(events) = root.get("hooks").and_then(Value::as_object) else {
        return true;
    };
    let mut complete = true;
    for (event, groups) in events.iter().filter(|(event, _)| valid_event(event)) {
        for (group_index, group) in groups.as_array().into_iter().flatten().enumerate() {
            if !valid_matcher(event, group) {
                complete = false;
                continue;
            }
            for (handler_index, handler) in group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if !valid_handler(handler)
                    || (event == "SessionEnd"
                        && handler.get("type").and_then(Value::as_str) == Some("mcp_tool"))
                {
                    complete = false;
                    continue;
                }
                if !emit(event, group_index, handler_index) {
                    return false;
                }
            }
        }
    }
    complete
}

/// The native loader skips malformed state entries and merges trimmed aliases
/// field-by-field. Project and system state tables never override user state.
pub(in crate::services::agent_cli::codex) fn decode_states(
    root: &Value,
) -> BTreeMap<String, CodexHookState> {
    let mut result = BTreeMap::<String, CodexHookState>::new();
    let Some(states) = root
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(Value::as_object)
    else {
        return result;
    };
    for (key, state) in states {
        let key = key.trim();
        let Some(state) = state.as_object() else {
            continue;
        };
        if key.is_empty()
            || state
                .get("enabled")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            || !optional_string(state, "trusted_hash")
        {
            continue;
        }
        let entry = result.entry(key.to_owned()).or_insert(CodexHookState {
            enabled: None,
            trusted_hash: None,
        });
        if let Some(enabled) = state.get("enabled").and_then(Value::as_bool) {
            entry.enabled = Some(enabled);
        }
        if let Some(hash) = state.get("trusted_hash").and_then(Value::as_str) {
            entry.trusted_hash = Some(hash.to_owned());
        }
    }
    result
}

pub(super) fn parse_policy(
    request: AgentAssetParseRequest<'_>,
    root: Option<&Value>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if !feature::parse(request, root, output) {
        return false;
    }
    if request.source.native_source_key != "config" || request.source.scope != AgentAssetScope::User
    {
        return true;
    }
    let states = match request.snapshot {
        AgentAssetSnapshot::Missing { .. } => Some(BTreeMap::new()),
        _ => root.map(decode_states),
    };
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "codex-hook-user-state",
            resolution_group_key: "codex-hook-user-state",
            native_id: "codex-hook-user-state",
            category: AgentAssetCategory::Hook,
            label: "Codex Hook 用户状态",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::PolicyOverlay,
            participation: AgentAssetResolutionParticipation::Participates,
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
    asset.native_payload = AgentAssetNativePayload::CodexHook(CodexHookPayload::Policy { states });
    output.emit_declaration(asset).is_continue()
}

pub(super) fn parse_document(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    standalone: bool,
    output: &mut dyn AgentParseOutput,
) -> bool {
    parse_with_origin(
        request,
        root,
        standalone,
        &request.source.path.to_string_lossy(),
        None,
        output,
    )
}

pub(super) fn parse_with_origin(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    standalone: bool,
    key_source: &str,
    plugin: Option<CodexPluginHookBinding>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if !valid_document(root, standalone) {
        incomplete(
            output,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
        return true;
    }
    let mut stopped = false;
    let complete = walk_slots(root, |event, group, handler| {
        let key = native_id(key_source, event, group, handler);
        let managed = matches!(
            request.source.scope,
            AgentAssetScope::Managed | AgentAssetScope::System
        );
        let parent = plugin.as_ref().map(|plugin| AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: plugin.plugin_id.clone(),
            qualifier: Some(format!("plugin:{}", plugin.plugin_id)),
        });
        let group_value = &root["hooks"][event][group];
        let evidence = CodexHookDefinitionEvidence {
            event: event.to_owned(),
            group_index: group,
            handler_index: handler,
            group: group_value.clone(),
            plugin: plugin.clone(),
        };
        let Some(decoded) = decode_definition(&evidence) else {
            return false;
        };
        let declared_state = if managed || decoded.builtin {
            AgentAssetDeclaredState::Enabled
        } else {
            // The per-rule switch lives in the separate user state table.
            AgentAssetDeclaredState::Unknown
        };
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &key,
                resolution_group_key: &key,
                category: AgentAssetCategory::Hook,
                native_id: &key,
                label: &format!("Codex Hook：{event}"),
                logical_origin: physical_origin(request.source),
                declared_state,
                trust_state: if managed || decoded.builtin {
                    AgentTrustState::Trusted
                } else {
                    AgentTrustState::Unknown
                },
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: parent.clone(),
                action_owner: parent,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Hook {
                    managed,
                    enabled: declared_state,
                    rule_count: Some(1),
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = AgentAssetNativePayload::CodexHook(CodexHookPayload::Definition {
            key,
            current_hash: decoded.current_hash,
            managed,
            evidence: Box::new(evidence),
        });
        stopped = output.emit_declaration(asset).is_break();
        !stopped
    });
    if !complete && !stopped {
        incomplete(
            output,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
    }
    !stopped
}
