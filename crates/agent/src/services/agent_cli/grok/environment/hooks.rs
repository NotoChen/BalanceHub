//! Bounded discovery and configured rule counts for Grok's direct Hook JSON files.

use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDiagnostic, AgentAssetDocumentFormat, AgentAssetInstallationOrigin,
    AgentAssetResolutionParticipation, AgentAssetScope, AgentAssetSourceKind, AgentTrustState,
};
use crate::services::agent_cli::{
    contracts::{
        AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot, AgentAssetSourceSpec,
        AgentFollowUpSourceDiscoveryRequest, AgentFollowUpSourceSpec, AgentFollowUpSourceTarget,
        AgentParseOutput, FollowUpSourceOutput,
    },
    environment::{
        config_document, parsed_asset, physical_origin, source, ParsedAssetInput, SourceInput,
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const FILE_PREFIX: &str = "grok-hook-file:";
const WORKSPACE_FILE: &str = "hooks.json";

pub(super) fn workspace_file_source(workspace: &Path) -> AgentAssetSourceSpec {
    source(SourceInput {
        origin: AgentAssetInstallationOrigin::LocalFiles,
        native_source_key: "grok-hook-file:workspace-hooks:hooks.json",
        label: "Grok 项目 Hook 配置",
        path: workspace.join(".grok/hooks").join(WORKSPACE_FILE),
        allowed_root: workspace,
        scope: AgentAssetScope::Workspace,
        precedence: 20,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[AgentAssetCategory::Hook],
    })
}

pub(super) fn discover_sources(
    request: AgentFollowUpSourceDiscoveryRequest<'_>,
    output: &mut dyn FollowUpSourceOutput,
) {
    for entry in request.manifest {
        // The fixed project destination is registered even when missing so a
        // first rule can be created. Keep that same source identity after the
        // file appears instead of discovering a second copy from the directory.
        if request.parent.native_source_key == "workspace-hooks" && entry.name == WORKSPACE_FILE {
            continue;
        }
        // Matches the pinned native direct JSON filename filter. Snapshot
        // admission still owns symlink, containment and source-kind checks.
        if !entry.name.ends_with(".json") || entry.name.len() <= 5 || entry.name.starts_with('.') {
            continue;
        }
        if entry.source_kind != AgentAssetSourceKind::File && !entry.is_symlink {
            continue;
        }
        if entry.is_symlink {
            output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
                agent_kind: super::super::AGENT_KIND,
                category: AgentAssetCategory::Hook,
                reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            });
        }
        if output
            .emit_follow_up(AgentFollowUpSourceSpec {
                parent_source_key: request.parent.native_source_key.clone(),
                target: AgentFollowUpSourceTarget::ManifestFile {
                    entry_name: entry.name.clone(),
                },
                native_source_key: format!(
                    "{FILE_PREFIX}{}:{}",
                    request.parent.native_source_key, entry.name
                ),
                label: format!("Grok Hook 配置：{}", entry.name),
                scope: request.parent.scope,
                precedence: request.parent.precedence,
                sensitive: true,
                source_kind: AgentAssetSourceKind::File,
                categories: vec![AgentAssetCategory::Hook],
            })
            .is_break()
        {
            return;
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum GrokHookPayload {
    Definition { name: String, managed: bool },
    Disabled { names: Option<BTreeSet<String>> },
}

pub(in crate::services::agent_cli::grok) const POLICY_KEY: &str = "hook-disabled-state";

pub(in crate::services::agent_cli::grok) fn decode_disabled(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

pub(in crate::services::agent_cli::grok) fn source_namespace(
    source: &AgentAssetSourceSpec,
) -> Option<String> {
    match source.native_source_key.as_str() {
        "config" if source.scope == AgentAssetScope::User => Some("user".to_owned()),
        "hook-user-managed" => Some("managed".to_owned()),
        "hook-user-requirements" => Some("requirements/user".to_owned()),
        key if key.starts_with(FILE_PREFIX) => file_namespace(&source.path, source.scope),
        _ => None,
    }
}

pub(in crate::services::agent_cli::grok) fn file_namespace(
    path: &Path,
    scope: AgentAssetScope,
) -> Option<String> {
    let prefix = match scope {
        AgentAssetScope::User => "global",
        AgentAssetScope::Workspace => "project",
        _ => return None,
    };
    Some(format!("{prefix}/{}", path.file_stem()?.to_str()?))
}

pub(super) fn parse_source(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) -> bool {
    if request.source.native_source_key == POLICY_KEY {
        parse_policy(request, output);
        return true;
    }
    let Some(namespace) = source_namespace(request.source) else {
        return false;
    };
    let standalone = request.source.native_source_key.starts_with(FILE_PREFIX);
    let handled = request.source.native_source_key != "config";
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return handled;
    };
    let format = if standalone {
        config_document::ConfigDocumentFormat::Json
    } else {
        config_document::ConfigDocumentFormat::Toml
    };
    let Some(value) = config_document::parse(bytes, format) else {
        malformed(output, standalone);
        return handled;
    };
    let Some(root) = value.as_object() else {
        malformed(output, standalone);
        return handled;
    };
    if root
        .get("version_overrides")
        .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
    {
        incomplete(output);
        return handled;
    }
    let complete = walk_slots(root, standalone, |event, group, handler| {
        let id = native_id(&namespace, event, group, handler);
        let mut asset = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &id,
                resolution_group_key: &id,
                category: AgentAssetCategory::Hook,
                native_id: &id,
                label: &format!("Grok Hook：{event}"),
                logical_origin: physical_origin(request.source),
                declared_state: AgentAssetDeclaredState::Enabled,
                trust_state: if request.source.scope == AgentAssetScope::Workspace {
                    request.context.trust_context
                } else {
                    AgentTrustState::Trusted
                },
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Hook {
                    managed: false,
                    enabled: AgentAssetDeclaredState::Enabled,
                    rule_count: Some(1),
                },
                facts: BTreeMap::new(),
            },
        );
        asset.native_payload = AgentAssetNativePayload::GrokHook(GrokHookPayload::Definition {
            name: id,
            managed: false,
        });
        output.emit_declaration(asset).is_continue()
    });
    if !complete {
        malformed(output, standalone);
    }
    handled
}

fn parse_policy(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    let names = match request.snapshot {
        AgentAssetSnapshot::Missing { .. } => Some(BTreeSet::new()),
        AgentAssetSnapshot::File { bytes, .. } => {
            std::str::from_utf8(bytes).ok().map(decode_disabled)
        }
        _ => None,
    };
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: POLICY_KEY,
            resolution_group_key: POLICY_KEY,
            category: AgentAssetCategory::Hook,
            native_id: POLICY_KEY,
            label: "Grok Hook 用户禁用策略",
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
    asset.native_payload = AgentAssetNativePayload::GrokHook(GrokHookPayload::Disabled { names });
    let _ = output.emit_declaration(asset);
}

fn incomplete(output: &mut dyn AgentParseOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: super::super::AGENT_KIND,
        category: AgentAssetCategory::Hook,
        reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
    });
}

fn malformed(output: &mut dyn AgentParseOutput, json: bool) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: if json {
            AgentAssetDocumentFormat::Json
        } else {
            AgentAssetDocumentFormat::Toml
        },
        location: Some("hooks".to_owned()),
    });
    incomplete(output);
}

pub(in crate::services::agent_cli::grok) fn event_key(event: &str) -> Option<&'static str> {
    Some(match event {
        "SessionStart" | "session_start" | "sessionStart" => "session_start",
        "UserPromptSubmit" | "user_prompt_submit" | "beforeSubmitPrompt" => "user_prompt_submit",
        "PreToolUse"
        | "pre_tool_use"
        | "preToolUse"
        | "beforeShellExecution"
        | "beforeMCPExecution"
        | "beforeReadFile" => "pre_tool_use",
        "PostToolUse"
        | "post_tool_use"
        | "postToolUse"
        | "afterShellExecution"
        | "afterMCPExecution"
        | "afterFileEdit"
        | "afterAgentResponse"
        | "afterAgentThought" => "post_tool_use",
        "PostToolUseFailure" | "post_tool_use_failure" | "postToolUseFailure" => {
            "post_tool_use_failure"
        }
        "PermissionDenied" | "permission_denied" | "permissionDenied" => "permission_denied",
        "Stop" | "stop" => "stop",
        "StopFailure" | "stop_failure" | "stopFailure" => "stop_failure",
        "StopCancelled" | "stop_cancelled" | "stopCancelled" => "stop_cancelled",
        "Notification" | "notification" => "notification",
        "SubagentStart" | "subagent_start" | "subagentStart" => "subagent_start",
        "SubagentStop" | "subagent_stop" | "subagentStop" => "subagent_stop",
        "SubagentEnd" | "subagent_end" | "subagentEnd" => "subagent_stop",
        "PreCompact" | "pre_compact" | "preCompact" => "pre_compact",
        "PostCompact" | "post_compact" | "postCompact" => "post_compact",
        "SessionEnd" | "session_end" | "sessionEnd" => "session_end",
        _ => return None,
    })
}

pub(in crate::services::agent_cli::grok) fn valid_event(event: &str) -> bool {
    event_key(event).is_some()
}

pub(in crate::services::agent_cli::grok) fn native_id(
    namespace: &str,
    event: &str,
    group: usize,
    handler: usize,
) -> String {
    format!(
        "{namespace}:{}[{group}].hooks[{handler}]",
        event_key(event).unwrap_or(event)
    )
}

pub(in crate::services::agent_cli::grok) fn valid_group_metadata(
    group: &Map<String, Value>,
) -> bool {
    group
        .get("matcher")
        .is_none_or(|value| value.is_null() || value.is_string())
        && group.get("hooks").is_some_and(Value::is_array)
}

fn schema_handler(value: &Value) -> bool {
    let Some(handler) = value.as_object() else {
        return false;
    };
    handler.get("type").is_some_and(Value::is_string)
        && handler
            .get("timeout")
            .is_none_or(|value| value.is_null() || value.as_u64().is_some())
        && handler.get("env").is_none_or(|value| {
            value.is_null()
                || value
                    .as_object()
                    .is_some_and(|values| values.values().all(Value::is_string))
        })
        && ["command", "url"].iter().all(|key| {
            handler
                .get(*key)
                .is_none_or(|value| value.is_null() || value.is_string())
        })
}

pub(in crate::services::agent_cli::grok) fn valid_handler(value: &Value) -> bool {
    schema_handler(value)
        && match value.get("type").and_then(Value::as_str) {
            Some("command") => value.get("command").is_some_and(Value::is_string),
            Some("http") => value.get("url").is_some_and(Value::is_string),
            _ => false,
        }
}

pub(in crate::services::agent_cli::grok) fn valid_matcher(event: &str, group: &Value) -> bool {
    if matches!(event_key(event), Some("stop" | "user_prompt_submit")) {
        return true;
    }
    group
        .get("matcher")
        .and_then(Value::as_str)
        .is_none_or(|matcher| {
            matcher.is_empty()
                || matcher == "*"
                || matcher
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '|'))
                || regex::Regex::new(matcher).is_ok()
        })
}

fn valid_groups(value: &Value) -> bool {
    value.as_array().is_some_and(|groups| {
        groups.iter().all(|group| {
            group.as_object().is_some_and(valid_group_metadata)
                && group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|handlers| handlers.iter().all(schema_handler))
        })
    })
}

pub(in crate::services::agent_cli::grok) fn walk_slots(
    root: &Map<String, Value>,
    standalone: bool,
    mut emit: impl FnMut(&str, usize, usize) -> bool,
) -> bool {
    let Some(hooks) = root.get("hooks") else {
        return true;
    };
    let Some(events) = hooks.as_object() else {
        return false;
    };
    if standalone
        && events
            .iter()
            .any(|(event, groups)| valid_event(event) && !valid_groups(groups))
    {
        return false;
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for event in events.keys().filter_map(|event| event_key(event)) {
        *counts.entry(event).or_default() += 1;
    }
    // Native alias groups merge through a HashMap. With multiple spellings the
    // persisted group indices have no stable source anchor; do not invent one.
    let mut complete = true;
    for (event, groups) in events.iter().filter(|(event, _)| valid_event(event)) {
        if counts.get(event_key(event).unwrap_or_default()) != Some(&1) || !valid_groups(groups) {
            complete = false;
            continue;
        }
        for (group, value) in groups.as_array().into_iter().flatten().enumerate() {
            if !valid_matcher(event, value) {
                complete = false;
                continue;
            }
            for (handler, value) in value
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                if !valid_handler(value) {
                    complete = false;
                    continue;
                }
                if !emit(event, group, handler) {
                    return false;
                }
            }
        }
    }
    complete
}
