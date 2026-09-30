use super::*;
use crate::models::AgentStatusUiMode;

fn emit_hook_malformed(output: &mut dyn AgentDiagnosticOutput, location: &str) {
    emit_malformed(output, location);
    emit_hook_incomplete(output);
}

pub(super) fn parse_hooks(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("hooks") else {
        return ControlFlow::Continue(());
    };
    let Some(events) = value.as_object() else {
        emit_hook_malformed(output, "hooks");
        return ControlFlow::Continue(());
    };
    for (event, groups) in bounded_object_entries(events) {
        if !valid_hook_event(event) {
            emit_hook_malformed(output, "hooks.event");
            continue;
        }
        let label = format!("Claude Code Hook：{event}");
        if guard_event_invalid(event, groups) {
            emit_hook_malformed(output, &format!("hooks.{event}"));
            let (group_index, matcher_shape) = groups
                .as_array()
                .and_then(|groups| {
                    groups.iter().enumerate().find_map(|(index, group)| {
                        let object = group.as_object();
                        let invalid = object.is_none_or(|object| {
                            object.get("hooks").and_then(Value::as_array).is_none()
                                || !valid_hook_matcher(object)
                                || object.get("hooks").and_then(Value::as_array).is_some_and(
                                    |entries| entries.iter().any(|entry| !valid_hook_entry(entry)),
                                )
                        });
                        invalid.then(|| (Some(index), hook_matcher_shape(object)))
                    })
                })
                .unwrap_or((None, "invalid"));
            let id = hook_identity(
                request.source.native_source_key.as_str(),
                event,
                matcher_shape,
                group_index,
                None,
            );
            if emit_hook_unknown(request, output, &id, &label).is_break() {
                return ControlFlow::Break(AgentOutputStop::EntryLimit);
            }
            continue;
        }
        let Some(groups) = groups.as_array() else {
            emit_hook_malformed(output, &format!("hooks.{event}"));
            let id = hook_identity(
                request.source.native_source_key.as_str(),
                event,
                "invalid",
                None,
                None,
            );
            if emit_overlay(
                request,
                output,
                OverlayInput {
                    category: AgentAssetCategory::Hook,
                    native_id: &id,
                    label: &label,
                    declaration_key: &id,
                    state: AgentAssetDeclaredState::Unknown,
                    role: crate::models::AgentAssetDeclarationRole::Definition,
                    details: AgentAssetDetails::Hook {
                        managed: request.source.scope == AgentAssetScope::Managed,
                        enabled: AgentAssetDeclaredState::Unknown,
                        rule_count: None,
                    },
                    participation: suppressed(request),
                },
            )
            .is_break()
            {
                return ControlFlow::Break(AgentOutputStop::EntryLimit);
            }
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(group) = group.as_object() else {
                emit_hook_malformed(output, &format!("hooks.{event}"));
                let id = hook_identity(
                    request.source.native_source_key.as_str(),
                    event,
                    "invalid",
                    Some(group_index),
                    None,
                );
                if emit_overlay(
                    request,
                    output,
                    OverlayInput {
                        category: AgentAssetCategory::Hook,
                        native_id: &id,
                        label: &label,
                        declaration_key: &id,
                        state: AgentAssetDeclaredState::Unknown,
                        role: crate::models::AgentAssetDeclarationRole::Definition,
                        details: AgentAssetDetails::Hook {
                            managed: request.source.scope == AgentAssetScope::Managed,
                            enabled: AgentAssetDeclaredState::Unknown,
                            rule_count: None,
                        },
                        participation: suppressed(request),
                    },
                )
                .is_break()
                {
                    return ControlFlow::Break(AgentOutputStop::EntryLimit);
                }
                continue;
            };
            let Some(entries) = group.get("hooks").and_then(Value::as_array) else {
                emit_hook_malformed(output, &format!("hooks.{event}"));
                let id = hook_identity(
                    request.source.native_source_key.as_str(),
                    event,
                    hook_matcher_shape(Some(group)),
                    Some(group_index),
                    None,
                );
                if emit_overlay(
                    request,
                    output,
                    OverlayInput {
                        category: AgentAssetCategory::Hook,
                        native_id: &id,
                        label: &label,
                        declaration_key: &id,
                        state: AgentAssetDeclaredState::Unknown,
                        role: crate::models::AgentAssetDeclarationRole::Definition,
                        details: AgentAssetDetails::Hook {
                            managed: request.source.scope == AgentAssetScope::Managed,
                            enabled: AgentAssetDeclaredState::Unknown,
                            rule_count: None,
                        },
                        participation: suppressed(request),
                    },
                )
                .is_break()
                {
                    return ControlFlow::Break(AgentOutputStop::EntryLimit);
                }
                continue;
            };
            if !valid_hook_matcher(group) {
                emit_hook_malformed(output, &format!("hooks.{event}"));
                let id = hook_identity(
                    request.source.native_source_key.as_str(),
                    event,
                    hook_matcher_shape(Some(group)),
                    Some(group_index),
                    None,
                );
                if emit_overlay(
                    request,
                    output,
                    OverlayInput {
                        category: AgentAssetCategory::Hook,
                        native_id: &id,
                        label: &label,
                        declaration_key: &id,
                        state: AgentAssetDeclaredState::Unknown,
                        role: crate::models::AgentAssetDeclarationRole::Definition,
                        details: AgentAssetDetails::Hook {
                            managed: request.source.scope == AgentAssetScope::Managed,
                            enabled: AgentAssetDeclaredState::Unknown,
                            rule_count: None,
                        },
                        participation: suppressed(request),
                    },
                )
                .is_break()
                {
                    return ControlFlow::Break(AgentOutputStop::EntryLimit);
                }
                continue;
            }
            for (hook_index, value) in entries.iter().enumerate() {
                let id = hook_identity(
                    request.source.native_source_key.as_str(),
                    event,
                    hook_matcher_shape(Some(group)),
                    Some(group_index),
                    Some(hook_index),
                );
                if !valid_hook_entry(value) {
                    emit_hook_malformed(output, &format!("hooks.{event}"));
                    if emit_hook_unknown(request, output, &id, &label).is_break() {
                        return ControlFlow::Break(AgentOutputStop::EntryLimit);
                    }
                    continue;
                }
                let details = AgentAssetDetails::Hook {
                    managed: request.source.scope == AgentAssetScope::Managed,
                    enabled: AgentAssetDeclaredState::Enabled,
                    rule_count: Some(1),
                };
                if emit_overlay(
                    request,
                    output,
                    OverlayInput {
                        category: AgentAssetCategory::Hook,
                        native_id: &id,
                        label: &label,
                        declaration_key: &id,
                        state: AgentAssetDeclaredState::Enabled,
                        role: crate::models::AgentAssetDeclarationRole::Definition,
                        details,
                        participation: suppressed(request),
                    },
                )
                .is_break()
                {
                    return ControlFlow::Break(AgentOutputStop::EntryLimit);
                }
            }
        }
    }
    ControlFlow::Continue(())
}

pub(super) fn parse_status(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("statusLine") else {
        return ControlFlow::Continue(());
    };
    let (mode, state, command_present) = match value {
        Value::Object(object) if valid_status_object(object) => (
            AgentStatusUiMode::Command,
            AgentAssetDeclaredState::Enabled,
            true,
        ),
        _ => (
            AgentStatusUiMode::Unknown,
            AgentAssetDeclaredState::Unknown,
            false,
        ),
    };
    if state == AgentAssetDeclaredState::Unknown {
        emit_malformed(output, "statusLine");
    }
    let participation = suppressed(request);
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "status-line",
            resolution_group_key: "status-line",
            category: AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Claude Code Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                participation,
            ),
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation,
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

pub(in crate::services::agent_cli::claude) fn valid_hook_matcher(
    group: &Map<String, Value>,
) -> bool {
    group.get("matcher").is_none_or(Value::is_string)
}

pub(in crate::services::agent_cli::claude) fn guard_event_invalid(
    event: &str,
    groups: &Value,
) -> bool {
    matches!(event, "PreToolUse" | "PermissionRequest")
        && groups.as_array().is_none_or(|groups| {
            groups.iter().any(|group| {
                let Some(group) = group.as_object() else {
                    return true;
                };
                let Some(entries) = group.get("hooks").and_then(Value::as_array) else {
                    return true;
                };
                !valid_hook_matcher(group) || entries.iter().any(|entry| !valid_hook_entry(entry))
            })
        })
}

pub(in crate::services::agent_cli::claude) fn hook_matcher_shape(
    group: Option<&Map<String, Value>>,
) -> &'static str {
    match group.and_then(|group| group.get("matcher")) {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::String(_)) => "string-present",
        Some(_) => "invalid",
    }
}

pub(in crate::services::agent_cli::claude) fn hook_identity(
    source_key: &str,
    event: &str,
    matcher_shape: &str,
    group_index: Option<usize>,
    hook_index: Option<usize>,
) -> String {
    format!(
        "{source_key}:{event}:matcher-{matcher_shape}:group-{}:hook-{}",
        group_index
            .map(|index| index.to_string())
            .unwrap_or_else(|| "invalid".to_owned()),
        hook_index
            .map(|index| index.to_string())
            .unwrap_or_else(|| "invalid".to_owned())
    )
}

pub(in crate::services::agent_cli::claude) const HOOK_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "SessionStart",
    "SessionEnd",
    "Stop",
    "StopFailure",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "PermissionRequest",
    "PermissionDenied",
    "Setup",
    "TeammateIdle",
    "TaskCreated",
    "TaskCompleted",
    "Elicitation",
    "ElicitationResult",
    "ConfigChange",
    "WorktreeCreate",
    "WorktreeRemove",
    "InstructionsLoaded",
    "CwdChanged",
    "FileChanged",
    "DirectoryAdded",
    "MessageDisplay",
];

pub(in crate::services::agent_cli::claude) fn valid_hook_event(event: &str) -> bool {
    HOOK_EVENTS.contains(&event)
}

/// Claude Code 2.1.270 Td()/UOe()/Rt() schemas. Known optional fields are
/// validated by handler type; unknown keys are retained without inventing
/// runtime control semantics for fields the native schema strips.
pub(in crate::services::agent_cli::claude) fn valid_hook_entry(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(kind) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    let optional_string = |field: &str| object.get(field).is_none_or(Value::is_string);
    let optional_bool = |field: &str| object.get(field).is_none_or(Value::is_boolean);
    let string_array = |field: &str| {
        object.get(field).is_none_or(|value| {
            value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        })
    };
    if !optional_string("if")
        || !optional_string("statusMessage")
        || !optional_bool("once")
        || object.get("timeout").is_some_and(|value| {
            !value
                .as_f64()
                .is_some_and(|value| value.is_finite() && value > 0.0)
        })
    {
        return false;
    }
    match kind {
        "command" => {
            object.get("command").is_some_and(Value::is_string)
                && string_array("args")
                && optional_bool("async")
                && optional_bool("asyncRewake")
                && object
                    .get("shell")
                    .is_none_or(|value| matches!(value.as_str(), Some("bash" | "powershell")))
                && ["rewakeMessage", "rewakeSummary"].iter().all(|field| {
                    object
                        .get(*field)
                        .is_none_or(|value| value.as_str().is_some_and(|value| !value.is_empty()))
                })
        }
        "prompt" | "agent" => {
            object.get("prompt").is_some_and(Value::is_string)
                && optional_string("model")
                && (kind != "prompt" || optional_bool("continueOnBlock"))
        }
        "mcp_tool" => {
            object.get("server").is_some_and(Value::is_string)
                && object.get("tool").is_some_and(Value::is_string)
                && object.get("input").is_none_or(Value::is_object)
        }
        "http" => {
            object
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| reqwest::Url::parse(url).is_ok())
                && object.get("headers").is_none_or(|value| {
                    value
                        .as_object()
                        .is_some_and(|headers| headers.values().all(Value::is_string))
                })
                && string_array("allowedEnvVars")
        }
        _ => false,
    }
}

fn emit_hook_unknown(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    id: &str,
    label: &str,
) -> ControlFlow<AgentOutputStop> {
    emit_overlay(
        request,
        output,
        OverlayInput {
            category: AgentAssetCategory::Hook,
            native_id: id,
            label,
            declaration_key: id,
            state: AgentAssetDeclaredState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            details: AgentAssetDetails::Hook {
                managed: request.source.scope == AgentAssetScope::Managed,
                enabled: AgentAssetDeclaredState::Unknown,
                rule_count: None,
            },
            participation: suppressed(request),
        },
    )
}

fn valid_status_object(object: &Map<String, Value>) -> bool {
    // Claude 2.1.270 accepts any command string, strips unknown object keys,
    // and catches invalid refreshInterval values as undefined.
    object.get("type").and_then(Value::as_str) == Some("command")
        && object.get("command").is_some_and(Value::is_string)
        && object.get("padding").is_none_or(Value::is_number)
        && object
            .get("hideVimModeIndicator")
            .is_none_or(Value::is_boolean)
}
