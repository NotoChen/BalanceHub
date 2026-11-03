use super::{control::*, SETTINGS_AUTHORITIES, SKILL_PREFIX};
use crate::models::{
    AgentAssetCategory, AgentAssetDeclaredState, AgentAssetDetails, AgentAssetDiagnostic,
    AgentAssetDocumentFormat, AgentAssetEffectiveAvailability, AgentAssetScope,
    AgentAssetSourceKind, AgentMcpApprovalState, AgentMcpTransport, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetLogicalOrigin, AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot,
    AgentDiagnosticOutput, AgentOutputStop, AgentParseOutput,
};
use crate::services::agent_cli::environment::{
    availability_from_declared, bounded_object_entries, declaration_trust_for_participation,
    emit_unknown_fields, parse_json, parsed_asset, physical_origin, ParsedAssetInput,
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, ops::ControlFlow, path::Path};

mod controls;
pub(in crate::services::agent_cli::claude) mod hooks;
mod registry;
pub(super) mod skill;
use controls::parse_control_sources;
use hooks::{parse_hooks, parse_status};
use registry::parse_registry;
use skill::parse_skill;

pub(super) fn emit_malformed(output: &mut dyn AgentDiagnosticOutput, location: &str) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Json,
        location: Some(location.to_owned()),
    });
}

fn emit_hook_incomplete(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: super::super::AGENT_KIND,
        category: AgentAssetCategory::Hook,
        reason: crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
    });
}

fn snapshot_root(
    snapshot: &AgentAssetSnapshot,
    output: &mut dyn AgentDiagnosticOutput,
) -> Option<Map<String, Value>> {
    let AgentAssetSnapshot::File { bytes, .. } = snapshot else {
        return None;
    };
    let Ok(value) = parse_json(bytes, None, output) else {
        emit_malformed(output, "root");
        return None;
    };
    let Some(root) = value.as_object() else {
        emit_malformed(output, "root");
        return None;
    };
    Some(root.clone())
}

fn suppressed(
    request: AgentAssetParseRequest<'_>,
) -> crate::models::AgentAssetResolutionParticipation {
    if (matches!(
        request.source.native_source_key.as_str(),
        "workspace-settings" | "workspace-local-settings" | "workspace-mcp" | "workspace-skills"
    ) || request
        .source
        .native_source_key
        .starts_with("skill-manifest:workspace-skills:"))
        && request.context.trust_context != AgentTrustState::Trusted
    {
        crate::models::AgentAssetResolutionParticipation::Suppressed {
            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        crate::models::AgentAssetResolutionParticipation::Participates
    }
}

fn mcp_definition_origin(request: AgentAssetParseRequest<'_>) -> AgentAssetLogicalOrigin {
    match request.source.native_source_key.as_str() {
        "workspace-mcp" => AgentAssetLogicalOrigin {
            scope: AgentAssetScope::Workspace,
            precedence: 20,
        },
        "managed-mcp" => AgentAssetLogicalOrigin {
            scope: AgentAssetScope::Managed,
            precedence: 40,
        },
        _ => physical_origin(request.source),
    }
}

pub(super) fn strict_transport(value: &Value) -> AgentMcpTransport {
    crate::services::agent_cli::catalog::native::CLAUDE
        .decode(value)
        .map_or(AgentMcpTransport::Unknown, |definition| {
            definition.transport
        })
}

fn emit_mcp(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    native_id: &str,
    value: &Value,
    declaration_key: &str,
    participation: crate::models::AgentAssetResolutionParticipation,
    logical_origin: AgentAssetLogicalOrigin,
) -> ControlFlow<AgentOutputStop> {
    let transport = strict_transport(value);
    let state = if transport == AgentMcpTransport::Unknown {
        AgentAssetDeclaredState::Unknown
    } else {
        AgentAssetDeclaredState::Enabled
    };
    let details = AgentAssetDetails::Mcp {
        transport,
        declared_state: state,
        approval_state: if request.source.native_source_key == "workspace-mcp" {
            AgentMcpApprovalState::Pending
        } else {
            AgentMcpApprovalState::NotRequired
        },
        effective_availability: availability_from_declared(state),
    };
    let mut facts = BTreeMap::new();
    facts.insert("transport".to_owned(), format!("{transport:?}"));
    if let Some(object) = value.as_object() {
        emit_unknown_fields(
            object,
            &[
                "type",
                "transport",
                "command",
                "args",
                "env",
                "url",
                "headers",
                "headersHelper",
                "oauth",
                "alwaysLoad",
                "timeout",
            ],
            &format!("mcpServers.{native_id}"),
            output,
        );
    }
    let mut declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key,
            resolution_group_key: native_id,
            category: AgentAssetCategory::Mcp,
            native_id,
            label: native_id,
            logical_origin,
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
            details,
            facts,
        },
    );
    declaration.native_payload = native_mcp_payload(value, transport);
    output.emit_declaration(declaration)
}

pub(super) fn native_mcp_payload(
    value: &Value,
    transport: AgentMcpTransport,
) -> AgentAssetNativePayload {
    match transport {
        AgentMcpTransport::Stdio => value
            .get("command")
            .and_then(Value::as_str)
            .filter(|command| !command.trim().is_empty())
            .map(|command| {
                let mut argv = vec![command.to_owned()];
                if let Some(args) = value.get("args").and_then(Value::as_array) {
                    argv.extend(args.iter().filter_map(Value::as_str).map(str::to_owned));
                }
                AgentAssetNativePayload::McpDefinition(
                    crate::services::agent_cli::contracts::AgentMcpDefinitionPayload {
                        identity: crate::services::agent_cli::contracts::AgentMcpMatcherIdentity::Stdio { argv },
                        origin: crate::services::agent_cli::contracts::AgentMcpDefinitionOrigin::Declared,
                    },
                )
            })
            .unwrap_or_default(),
        AgentMcpTransport::Http | AgentMcpTransport::Sse | AgentMcpTransport::WebSocket => value
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
            .map(|url| {
                AgentAssetNativePayload::McpDefinition(
                    crate::services::agent_cli::contracts::AgentMcpDefinitionPayload {
                        identity: crate::services::agent_cli::contracts::AgentMcpMatcherIdentity::Remote {
                            url: url.to_owned(),
                        },
                        origin: crate::services::agent_cli::contracts::AgentMcpDefinitionOrigin::Declared,
                    },
                )
            })
            .unwrap_or_default(),
        AgentMcpTransport::Unknown => AgentAssetNativePayload::None,
    }
}

fn parse_mcp_map(
    request: AgentAssetParseRequest<'_>,
    map: &Map<String, Value>,
    prefix: &str,
    participation: crate::models::AgentAssetResolutionParticipation,
    logical_origin: AgentAssetLogicalOrigin,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for (native_id, value) in bounded_object_entries(map) {
        if emit_mcp(
            request,
            output,
            native_id,
            value,
            &format!("{prefix}:{native_id}"),
            participation,
            logical_origin,
        )
        .is_break()
        {
            return ControlFlow::Break(AgentOutputStop::EntryLimit);
        }
    }
    ControlFlow::Continue(())
}

struct OverlayInput<'a> {
    category: AgentAssetCategory,
    native_id: &'a str,
    label: &'a str,
    declaration_key: &'a str,
    state: AgentAssetDeclaredState,
    role: crate::models::AgentAssetDeclarationRole,
    details: AgentAssetDetails,
    participation: crate::models::AgentAssetResolutionParticipation,
}

fn emit_overlay(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
    input: OverlayInput<'_>,
) -> ControlFlow<AgentOutputStop> {
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: input.declaration_key,
            resolution_group_key: input.native_id,
            category: input.category,
            native_id: input.native_id,
            label: input.label,
            logical_origin: physical_origin(request.source),
            declared_state: input.state,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                input.participation,
            ),
            role: input.role,
            participation: input.participation,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: input.details,
            facts: BTreeMap::new(),
        },
    ))
}

pub(super) enum ProjectSelection<'a> {
    Missing,
    Unique(&'a Map<String, Value>),
    Ambiguous,
    Malformed,
}

pub(super) fn select_exact_project<'a>(
    projects: &'a Map<String, Value>,
    canonical: &Path,
    lexical: Option<&Path>,
) -> ProjectSelection<'a> {
    let canonical = canonical.to_string_lossy();
    let lexical = lexical.map(|path| path.to_string_lossy());
    let mut matches = projects
        .iter()
        .filter(|(key, _)| {
            key.as_str() == canonical.as_ref()
                || lexical
                    .as_deref()
                    .is_some_and(|value| key.as_str() == value)
        })
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return ProjectSelection::Missing;
    }
    if matches.len() != 1 {
        return ProjectSelection::Ambiguous;
    }
    matches
        .pop()
        .and_then(Value::as_object)
        .map(ProjectSelection::Unique)
        .unwrap_or(ProjectSelection::Malformed)
}

fn parse_account(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    match root.get("mcpServers") {
        Some(Value::Object(servers)) => {
            match parse_mcp_map(
                request,
                servers,
                "user",
                crate::models::AgentAssetResolutionParticipation::Participates,
                AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 10,
                },
                output,
            ) {
                ControlFlow::Break(stop) => return ControlFlow::Break(stop),
                ControlFlow::Continue(()) => {}
            }
        }
        Some(_) => emit_malformed(output, "mcpServers"),
        None => {}
    }
    if let Some(workspace) = request.context.workspace_id.as_deref() {
        let selection = match root.get("projects") {
            None => ProjectSelection::Missing,
            Some(Value::Object(projects)) => {
                select_exact_project(projects, Path::new(workspace), request.workspace_lexical)
            }
            Some(_) => ProjectSelection::Malformed,
        };
        if let ProjectSelection::Unique(project) = selection {
            if let Some(value) = project.get("mcpServers") {
                if let Some(servers) = value.as_object() {
                    let participation = if request.context.trust_context == AgentTrustState::Trusted
                    {
                        crate::models::AgentAssetResolutionParticipation::Participates
                    } else {
                        crate::models::AgentAssetResolutionParticipation::Suppressed {
                            reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
                        }
                    };
                    if parse_mcp_map(
                        request,
                        servers,
                        "project",
                        participation,
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Local,
                            precedence: 30,
                        },
                        output,
                    )
                    .is_break()
                    {
                        return ControlFlow::Break(AgentOutputStop::EntryLimit);
                    }
                } else {
                    emit_malformed(output, "projects.mcpServers");
                }
            }
        }
    }
    ControlFlow::Continue(())
}

fn validate_settings_schema(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) {
    if !SETTINGS_AUTHORITIES.contains(&request.source.native_source_key.as_str()) {
        return;
    }
    if !request
        .source
        .categories
        .contains(&AgentAssetCategory::Hook)
        && root.get("hooks").is_some_and(|value| !value.is_object())
    {
        emit_malformed(output, "hooks");
    }
    // The typed control decoder owns boolean validation and diagnostics.
    // This field is only meaningful in the managed settings source.
    if root.contains_key("allowManagedHooksOnly")
        && request.source.native_source_key != "managed-settings"
    {
        output.emit_diagnostic(AgentAssetDiagnostic::UnknownField {
            field_path: "allowManagedHooksOnly".to_owned(),
        });
    }
}

pub(in crate::services::agent_cli::claude) fn parse_assets(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    if request.source.categories.is_empty() {
        return;
    }
    if super::plugin::parse_source(request, output) {
        return;
    }
    if request.source.native_source_key.starts_with(SKILL_PREFIX) {
        parse_skill(request, output);
        return;
    }
    if request.source.source_kind == AgentAssetSourceKind::Directory {
        return;
    }
    let decoded = snapshot_root(request.snapshot, output);
    let state = match request.snapshot {
        AgentAssetSnapshot::File { .. } if decoded.is_some() => SourceState::Readable,
        AgentAssetSnapshot::File { .. } => SourceState::Malformed,
        AgentAssetSnapshot::Blocked { .. } => SourceState::Blocked,
        AgentAssetSnapshot::Missing { .. } | AgentAssetSnapshot::DirectoryManifest { .. } => {
            SourceState::Missing
        }
    };
    if request.source.categories.is_empty() {
        return;
    }
    let Some(root) = decoded.as_ref() else {
        if matches!(state, SourceState::Malformed | SourceState::Blocked)
            && request
                .source
                .categories
                .contains(&AgentAssetCategory::Hook)
        {
            emit_hook_incomplete(output);
        }
        let _ = parse_control_sources(request, None, state, output);
        return;
    };
    validate_settings_schema(request, root, output);
    if request.source.native_source_key == "account" {
        if parse_account(request, root, output).is_break() {
            return;
        }
        let _ = parse_control_sources(request, Some(root), state, output);
        return;
    }
    if request.source.native_source_key == "plugin-registry" {
        let _ = parse_registry(request, root, output);
        return;
    }
    if parse_control_sources(request, Some(root), state, output).is_break() {
        return;
    }
    if request.source.categories.contains(&AgentAssetCategory::Mcp) {
        if matches!(
            request.source.native_source_key.as_str(),
            "workspace-mcp" | "managed-mcp"
        ) {
            match root.get("mcpServers") {
                Some(Value::Object(servers))
                    if parse_mcp_map(
                        request,
                        servers,
                        "definition",
                        suppressed(request),
                        mcp_definition_origin(request),
                        output,
                    )
                    .is_break() =>
                {
                    return
                }
                Some(Value::Object(_)) | None => {}
                Some(_) => emit_malformed(output, "mcpServers"),
            }
        }
        if request.source.native_source_key == "managed-settings" {
            match root.get("managedMcpServers") {
                Some(Value::Object(servers))
                    if parse_mcp_map(
                        request,
                        servers,
                        "managed",
                        crate::models::AgentAssetResolutionParticipation::Participates,
                        AgentAssetLogicalOrigin {
                            scope: AgentAssetScope::Managed,
                            precedence: 40,
                        },
                        output,
                    )
                    .is_break() =>
                {
                    return
                }
                Some(Value::Object(_)) | None => {}
                Some(_) => emit_malformed(output, "managedMcpServers"),
            }
        }
    }
    if request
        .source
        .categories
        .contains(&AgentAssetCategory::Hook)
        && parse_hooks(request, root, output).is_break()
    {
        return;
    }
    if request
        .source
        .categories
        .contains(&AgentAssetCategory::StatusUi)
    {
        let _ = parse_status(request, root, output);
    }
}
