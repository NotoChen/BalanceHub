pub(super) mod hooks;
pub(crate) use hooks::CodexHookPayload;
mod native;
mod resolve;

pub(super) use native::plugin_hooks;
pub(super) use native::{definition_suppressions, discover_follow_up_sources};
pub(super) mod system_paths;

pub(super) use resolve::{assess_assets, resolve_assets};

use super::config;
use crate::{
    models::{
        AgentAssetCategory, AgentAssetDeclaredState, AgentAssetDetails, AgentAssetDiagnostic,
        AgentAssetDiscoveryIncompleteReason, AgentAssetDocumentFormat, AgentAssetScope,
        AgentAssetSourceKind, AgentMcpApprovalState, AgentMcpTransport, AgentStatusUiMode,
        AgentTrustState,
    },
    services::agent_cli::{
        contracts::{
            AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot,
            AgentContextDiscoveryRequest, AgentDiagnosticOutput, AgentOutputStop, AgentParseOutput,
            AgentSourceDiscoveryRequest, AgentWorkspaceTrustResolveRequest,
            AgentWorkspaceTrustSourceRequest, CodexMcpRequirement, CodexMcpServerValueMatcher,
            CodexRawMcpServerIdentity, CodexRequirementsPayload, InitialSourceOutput,
            ParsedAgentAsset,
        },
        environment::{
            declaration_trust_for_participation, default_context, emit_unknown_fields,
            parsed_asset, physical_origin, source, ParsedAssetInput, SourceInput,
        },
    },
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, fmt, num::NonZeroUsize, ops::ControlFlow, time::Duration};
use toml::value::Table as TomlTable;

pub(super) const PARSER_VERSION: u32 = 3;

pub(super) fn readonly_external_roots(
    _installations: &[crate::models::AgentInstallation],
) -> Vec<std::path::PathBuf> {
    #[cfg(unix)]
    {
        vec![system_paths::system_root().join("skills")]
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

pub(super) fn discover_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let root = config::config_dir_for_home(request.home);
    default_context(request, root, PARSER_VERSION)
}

pub(super) fn discover_workspace_trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    if !request.workspace.is_absolute() {
        return;
    }
    #[cfg(unix)]
    let system_root = system_paths::system_root();
    #[cfg(unix)]
    if output
        .emit_initial(source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
            native_source_key: "system-config",
            label: "Codex 系统配置",
            path: system_root.join("config.toml"),
            allowed_root: &system_root,
            scope: AgentAssetScope::System,
            precedence: 5,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[
                AgentAssetCategory::Mcp,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Plugin,
                AgentAssetCategory::StatusUi,
                AgentAssetCategory::Hook,
            ],
        }))
        .is_break()
    {
        return;
    }
    let _ = request.home;
    let _ = output.emit_initial(source(SourceInput {
        origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: "config",
        label: "Codex 配置",
        path: request.config_root.join("config.toml"),
        allowed_root: request.config_root,
        scope: AgentAssetScope::User,
        precedence: 10,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Skill,
            AgentAssetCategory::Plugin,
            AgentAssetCategory::StatusUi,
            AgentAssetCategory::Hook,
        ],
    }));
}

pub(super) fn resolve_workspace_trust(
    request: AgentWorkspaceTrustResolveRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> AgentTrustState {
    let mut ordered = request.sources.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|source| source.spec.precedence);
    let mut merged = TomlTable::new();
    for source in ordered {
        match source.snapshot {
            AgentAssetSnapshot::Missing { .. } => continue,
            AgentAssetSnapshot::Blocked { .. } => return AgentTrustState::Unknown,
            AgentAssetSnapshot::DirectoryManifest { .. } => return AgentTrustState::Unknown,
            AgentAssetSnapshot::File { bytes, .. } => {
                let Ok(text) = std::str::from_utf8(bytes) else {
                    emit_trust_malformed(output);
                    return AgentTrustState::Unknown;
                };
                let Ok(value) = text.parse::<toml::Value>() else {
                    emit_trust_malformed(output);
                    return AgentTrustState::Unknown;
                };
                let Some(table) = value.as_table() else {
                    emit_trust_malformed(output);
                    return AgentTrustState::Unknown;
                };
                merge_toml_table(&mut merged, table);
            }
        }
    }
    let Some(projects) = merged.get("projects") else {
        return AgentTrustState::Unknown;
    };
    let Some(projects) = projects.as_table() else {
        emit_trust_malformed(output);
        return AgentTrustState::Unknown;
    };
    let canonical_key = request.workspace.to_string_lossy().into_owned();
    let lexical_key = request
        .workspace_lexical
        .unwrap_or(request.workspace)
        .to_string_lossy()
        .into_owned();
    let matched = projects
        .iter()
        .filter(|(key, _)| {
            [canonical_key.as_str(), lexical_key.as_str()]
                .iter()
                .any(|candidate| workspace_key_matches(key, candidate, cfg!(windows)))
        })
        .collect::<Vec<_>>();
    if matched.len() > 1 {
        return AgentTrustState::Unknown;
    }
    let Some((_, value)) = matched.into_iter().next() else {
        return AgentTrustState::Unknown;
    };
    let Some(entry) = value.as_table() else {
        emit_trust_malformed(output);
        return AgentTrustState::Unknown;
    };
    let Some(level) = entry.get("trust_level") else {
        return AgentTrustState::Unknown;
    };
    let Some(level) = level.as_str() else {
        emit_trust_malformed(output);
        return AgentTrustState::Unknown;
    };
    match level {
        "trusted" => AgentTrustState::Trusted,
        "untrusted" => AgentTrustState::Untrusted,
        _ => AgentTrustState::Unknown,
    }
}

fn workspace_key_matches(key: &str, candidate: &str, windows: bool) -> bool {
    if windows {
        key.eq_ignore_ascii_case(candidate)
    } else {
        key == candidate
    }
}

fn emit_trust_malformed(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Toml,
        location: Some("projects".to_owned()),
    });
}

pub(super) fn discover_resource_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    native::discover_sources(request, output);
}

pub(super) fn parse_assets(request: AgentAssetParseRequest<'_>, output: &mut dyn AgentParseOutput) {
    if request.source.native_source_key == "system-requirements" {
        parse_requirements_servers(request, output);
        return;
    }
    if native::parse_source(request, output) {
        return;
    }
    if request.source.source_kind == AgentAssetSourceKind::Directory {
        return;
    }
    let config_source = matches!(
        request.source.native_source_key.as_str(),
        "config" | "system-config" | "workspace-config"
    );
    if config_source && !matches!(request.snapshot, AgentAssetSnapshot::File { .. }) {
        if matches!(request.snapshot, AgentAssetSnapshot::Blocked { .. }) {
            hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
        if !hooks::parse_policy(request, None, output) {
            return;
        }
        let _ = native::parse_config(request, None, output);
        return;
    }

    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    if matches!(
        request.source.native_source_key.as_str(),
        "hooks" | "workspace-hooks"
    ) {
        if parse_hook_file(request, bytes, output).is_break() {
            return;
        }
        return;
    }
    if !request.source.categories.contains(&AgentAssetCategory::Mcp) {
        return;
    }

    let toml = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<toml::Value>().ok());
    let root = toml.as_ref().and_then(toml::Value::as_table);
    if config_source {
        let hook_root = toml
            .as_ref()
            .and_then(|value| serde_json::to_value(value).ok());
        if !hooks::parse_policy(request, hook_root.as_ref(), output) {
            return;
        }
        if let Some(root) = hook_root.as_ref().and_then(Value::as_object) {
            if !hooks::parse_document(request, root, false, output) {
                return;
            }
        } else {
            hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
    }
    if config_source && native::parse_config(request, root, output).is_break() {
        return;
    }
    let Some(root) = root else {
        malformed_toml(output);
        return;
    };
    if parse_mcp_servers(request, root, output).is_break() {
        return;
    }
    let json_root = match serde_json::to_value(toml)
        .ok()
        .and_then(|value| value.as_object().cloned())
    {
        Some(value) => value,
        None => return,
    };
    let _terminal_result = parse_status_ui(request, &json_root, output);
}

fn malformed_toml(output: &mut dyn AgentDiagnosticOutput) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Toml,
        location: None,
    });
}

fn parse_mcp_servers(
    request: AgentAssetParseRequest<'_>,
    root: &TomlTable,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(value) = root.get("mcp_servers") else {
        return ControlFlow::Continue(());
    };
    let Some(servers) = value.as_table() else {
        malformed_toml(output);
        return ControlFlow::Continue(());
    };
    for (native_id, value) in servers {
        let Some(table) = value.as_table() else {
            malformed_toml(output);
            continue;
        };
        let explicit_state = table.get("enabled").and_then(toml::Value::as_bool);
        let declared_state = match explicit_state {
            Some(true) => AgentAssetDeclaredState::Enabled,
            Some(false) => AgentAssetDeclaredState::Disabled,
            None => AgentAssetDeclaredState::Unknown,
        };
        let role = if table.len() == 1 && table.contains_key("enabled") {
            crate::models::AgentAssetDeclarationRole::StateOverlay
        } else {
            crate::models::AgentAssetDeclarationRole::Definition
        };
        if let Ok(json_value) = serde_json::to_value(table) {
            if let Some(object) = json_value.as_object() {
                emit_unknown_fields(
                    object,
                    &[
                        "enabled",
                        "command",
                        "args",
                        "env",
                        "env_vars",
                        "cwd",
                        "url",
                        "bearer_token",
                        "bearer_token_env_var",
                        "http_headers",
                        "env_http_headers",
                        "startup_timeout_sec",
                        "startup_timeout_ms",
                        "tool_timeout_sec",
                        "enabled_tools",
                        "disabled_tools",
                        "tools",
                        "environment_id",
                        "auth",
                        "required",
                        "supports_parallel_tool_calls",
                        "omit_tools_from",
                        "default_tools_approval_mode",
                        "scopes",
                        "oauth",
                        "oauth_resource",
                        "http_headers_helper",
                        "name",
                    ],
                    &format!("mcp_servers.{native_id}"),
                    output,
                );
            }
        }
        let participation = if request.source.native_source_key == "workspace-config"
            && request.context.trust_context != AgentTrustState::Trusted
        {
            crate::models::AgentAssetResolutionParticipation::Suppressed {
                reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
            }
        } else {
            crate::models::AgentAssetResolutionParticipation::Participates
        };
        let mut declaration = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: native_id,
                resolution_group_key: native_id,
                category: AgentAssetCategory::Mcp,
                native_id,
                label: native_id,
                logical_origin: physical_origin(request.source),
                declared_state,
                trust_state: declaration_trust_for_participation(
                    request.context.trust_context,
                    participation,
                ),
                role,
                participation,
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Unknown,
                    declared_state,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
                },
                facts: BTreeMap::new(),
            },
        );
        declaration.native_payload = AgentAssetNativePayload::TomlTable(table.clone());
        if let ControlFlow::Break(reason) = output.emit_declaration(declaration) {
            return ControlFlow::Break(reason);
        }
    }
    ControlFlow::Continue(())
}

fn parse_requirements_servers(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    match request.snapshot {
        AgentAssetSnapshot::File { bytes, .. } => {
            use crate::services::agent_cli::environment::config_document::{
                self, ConfigDocumentFormat,
            };
            match config_document::parse(bytes, ConfigDocumentFormat::Toml) {
                Some(root) if root.get("hooks").is_some_and(hooks::configured_source) => {
                    hooks::incomplete(
                        output,
                        AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
                    );
                }
                None => hooks::incomplete(
                    output,
                    AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
                ),
                _ => {}
            }
        }
        AgentAssetSnapshot::Blocked { .. } => {
            hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
        }
        _ => {}
    }
    let servers = match parse_requirements_snapshot(request.snapshot) {
        Ok(RequirementsSnapshot::Allowlist(servers)) => servers,
        Ok(RequirementsSnapshot::Missing) => {
            let _ = emit_requirement_policy(
                request,
                REQUIREMENTS_POLICY_SENTINEL,
                CodexRequirementsPayload::MissingFile,
                output,
            );
            return;
        }
        Ok(RequirementsSnapshot::NoPolicy) => {
            let _ = emit_requirement_policy(
                request,
                REQUIREMENTS_POLICY_SENTINEL,
                CodexRequirementsPayload::NoPolicy,
                output,
            );
            return;
        }
        Err(()) => {
            if matches!(request.snapshot, AgentAssetSnapshot::File { .. }) {
                malformed_toml(output);
            }
            let _ = emit_requirement_policy(
                request,
                REQUIREMENTS_POLICY_SENTINEL,
                CodexRequirementsPayload::InvalidRequirements,
                output,
            );
            return;
        }
    };
    let entry_count = servers.len();
    for (native_id, requirement) in servers {
        if emit_requirement_policy(
            request,
            &native_id,
            CodexRequirementsPayload::Entry(requirement),
            output,
        )
        .is_break()
        {
            return;
        }
    }
    let _ = emit_requirement_policy(
        request,
        REQUIREMENTS_POLICY_SENTINEL,
        CodexRequirementsPayload::AllowlistRoot { entry_count },
        output,
    );
}

const REQUIREMENTS_POLICY_SENTINEL: &str = "__balancehub_codex_requirements_policy__";

fn emit_requirement_policy(
    request: AgentAssetParseRequest<'_>,
    native_id: &str,
    requirements: CodexRequirementsPayload,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let declaration = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: &match &requirements {
                CodexRequirementsPayload::Entry(_) => format!("policy:requirements:{native_id}"),
                _ => "policy:requirements-root".to_owned(),
            },
            resolution_group_key: native_id,
            category: AgentAssetCategory::Mcp,
            native_id,
            label: native_id,
            logical_origin: crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::Managed,
                precedence: request.source.precedence,
            },
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::PolicyOverlay,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Mcp {
                transport: AgentMcpTransport::Unknown,
                declared_state: AgentAssetDeclaredState::Unknown,
                approval_state: AgentMcpApprovalState::NotRequired,
                effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    let mut declaration = declaration;
    declaration.native_payload = AgentAssetNativePayload::CodexRequirements(requirements);
    output.emit_declaration(declaration)
}

fn parse_status_ui(
    request: AgentAssetParseRequest<'_>,
    root: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let value = root
        .get("tui")
        .and_then(Value::as_object)
        .and_then(|tui| tui.get("status_line").or_else(|| tui.get("statusLine")))
        .or_else(|| root.get("status_line"));
    let Some(value) = value else {
        return ControlFlow::Continue(());
    };
    let mode = match value {
        Value::Null | Value::Bool(false) => AgentStatusUiMode::Disabled,
        Value::String(value) if !value.trim().is_empty() => AgentStatusUiMode::Command,
        Value::Array(_) | Value::Object(_) => AgentStatusUiMode::BuiltIn,
        _ => AgentStatusUiMode::Unknown,
    };
    let state = match mode {
        AgentStatusUiMode::Disabled => AgentAssetDeclaredState::Disabled,
        AgentStatusUiMode::BuiltIn | AgentStatusUiMode::Command => AgentAssetDeclaredState::Enabled,
        AgentStatusUiMode::Unknown => AgentAssetDeclaredState::Unknown,
    };
    output.emit_declaration(parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: "status-line",
            resolution_group_key: "status-line",
            category: AgentAssetCategory::StatusUi,
            native_id: "status-line",
            label: "Codex Status UI",
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: request.context.trust_context,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
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

fn parse_hook_file(
    request: AgentAssetParseRequest<'_>,
    bytes: &[u8],
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let value = match crate::services::agent_cli::environment::config_document::parse(
        bytes,
        crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
    ) {
        Some(value) => value,
        None => {
            output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Json,
                location: None,
            });
            hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
            return ControlFlow::Continue(());
        }
    };
    let Some(root) = value.as_object() else {
        hooks::incomplete(
            output,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
        return ControlFlow::Continue(());
    };
    let _ = hooks::parse_document(request, root, true, output);
    ControlFlow::Continue(())
}

fn merge_and_decode_mcp(
    assets: &[&ParsedAgentAsset],
) -> Result<(TomlTable, CodexMcpServerConfig), ()> {
    let mut merged = TomlTable::new();
    for asset in assets {
        let AgentAssetNativePayload::TomlTable(table) = &asset.native_payload else {
            continue;
        };
        merge_toml_table(&mut merged, table);
    }
    let decoded = decode_mcp_config(&merged)?;
    Ok((merged, decoded))
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CodexFixtureMcpFacts {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    pub search_approval: Option<String>,
    pub search_output_token_limit: Option<usize>,
    pub enabled: bool,
}

/// Test-only view over the production merge and decoder. This keeps the
/// cross-module inventory fixtures able to assert native values without
/// exposing configuration payloads in the public inventory model.
#[cfg(test)]
pub(crate) fn decode_merged_mcp_fixture(layers: &[&str]) -> Result<CodexFixtureMcpFacts, ()> {
    let mut merged = TomlTable::new();
    for layer in layers {
        let value = layer.parse::<toml::Value>().map_err(|_| ())?;
        let table = value.as_table().ok_or(())?;
        merge_toml_table(&mut merged, table);
    }
    let server = merged
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|servers| servers.get("same"))
        .and_then(toml::Value::as_table)
        .ok_or(())?;
    let decoded = decode_mcp_config(server)?;
    let search = decoded.tools.get("search");
    Ok(CodexFixtureMcpFacts {
        command: decoded.command,
        args: decoded.args,
        enabled_tools: decoded.enabled_tools,
        disabled_tools: decoded.disabled_tools,
        search_approval: search
            .and_then(|tool| tool.approval_mode)
            .map(|value| format!("{value:?}").to_lowercase()),
        search_output_token_limit: search
            .and_then(|tool| tool.output_token_limit)
            .map(NonZeroUsize::get),
        enabled: decoded.enabled,
    })
}

fn merge_toml_table(target: &mut TomlTable, layer: &TomlTable) {
    for (key, value) in layer {
        match (target.get_mut(key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(incoming)) => {
                merge_toml_table(existing, incoming);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CodexAppToolApproval {
    Auto,
    Prompt,
    Writes,
    Approve,
}

#[derive(Debug, Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CodexToolExposureSurface {
    CodeMode,
    Deferred,
    Direct,
}

#[derive(Clone, Deserialize, serde::Serialize)]
struct CodexMcpServerToolConfig {
    pub(super) approval_mode: Option<CodexAppToolApproval>,
    pub(super) output_token_limit: Option<NonZeroUsize>,
}

impl fmt::Debug for CodexMcpServerToolConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexMcpServerToolConfig")
            .field("has_approval_mode", &self.approval_mode.is_some())
            .field("has_output_token_limit", &self.output_token_limit.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(untagged)]
enum CodexMcpServerEnvVar {
    Name(String),
    Descriptor(CodexMcpServerEnvVarDescriptor),
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CodexMcpServerEnvVarDescriptor {
    name: String,
    source: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct CodexMcpServerOAuthConfig {
    pub(super) client_id: Option<String>,
    pub(super) callback_url: Option<String>,
    pub(super) callback_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CodexMcpServerAuth {
    #[default]
    Oauth,
    Chatgpt,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct CodexRawMcpServerConfig {
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    env_vars: Option<Vec<CodexMcpServerEnvVar>>,
    cwd: Option<String>,
    url: Option<String>,
    bearer_token: Option<String>,
    bearer_token_env_var: Option<String>,
    http_headers: Option<BTreeMap<String, String>>,
    env_http_headers: Option<BTreeMap<String, String>>,
    environment_id: Option<String>,
    experimental_environment: Option<String>,
    auth: Option<CodexMcpServerAuth>,
    startup_timeout_sec: Option<f64>,
    startup_timeout_ms: Option<u64>,
    tool_timeout_sec: Option<f64>,
    enabled: Option<bool>,
    required: Option<bool>,
    supports_parallel_tool_calls: Option<bool>,
    omit_tools_from: Option<Vec<CodexToolExposureSurface>>,
    default_tools_approval_mode: Option<CodexAppToolApproval>,
    enabled_tools: Option<Vec<String>>,
    disabled_tools: Option<Vec<String>>,
    scopes: Option<Vec<String>>,
    oauth: Option<CodexMcpServerOAuthConfig>,
    oauth_resource: Option<String>,
    name: Option<String>,
    tools: Option<BTreeMap<String, CodexMcpServerToolConfig>>,
    http_headers_helper: Option<String>,
}

struct CodexMcpServerConfig {
    pub(super) transport: AgentMcpTransport,
    pub(super) command: Option<String>,
    pub(super) url: Option<String>,
    pub(super) enabled: bool,
    pub(super) environment_id: String,
    pub(super) auth: CodexMcpServerAuth,
    pub(super) args: Vec<String>,
    pub(super) env_vars: Vec<CodexMcpServerEnvVar>,
    pub(super) enabled_tools: Option<Vec<String>>,
    pub(super) disabled_tools: Option<Vec<String>>,
    pub(super) tools: BTreeMap<String, CodexMcpServerToolConfig>,
    pub(super) omit_tools_from: Option<Vec<CodexToolExposureSurface>>,
    pub(super) default_tools_approval_mode: Option<CodexAppToolApproval>,
    pub(super) scopes: Option<Vec<String>>,
    pub(super) startup_timeout: Option<Duration>,
    pub(super) tool_timeout: Option<Duration>,
    pub(super) required: bool,
    pub(super) supports_parallel_tool_calls: bool,
}

impl fmt::Debug for CodexMcpServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexMcpServerConfig")
            .field("transport", &self.transport)
            .field("has_command", &self.command.is_some())
            .field("has_url", &self.url.is_some())
            .field("enabled", &self.enabled)
            .field("environment_id", &self.environment_id)
            .field("auth", &self.auth)
            .field("arg_count", &self.args.len())
            .field("env_var_count", &self.env_vars.len())
            .field(
                "enabled_tool_count",
                &self.enabled_tools.as_ref().map(Vec::len),
            )
            .field(
                "disabled_tool_count",
                &self.disabled_tools.as_ref().map(Vec::len),
            )
            .field("tool_config_count", &self.tools.len())
            .field(
                "omit_tools_from",
                &self.omit_tools_from.as_ref().map(Vec::len),
            )
            .field(
                "default_tools_approval_mode",
                &self.default_tools_approval_mode.is_some(),
            )
            .field("scope_count", &self.scopes.as_ref().map(Vec::len))
            .field("has_startup_timeout", &self.startup_timeout.is_some())
            .field("has_tool_timeout", &self.tool_timeout.is_some())
            .field("required", &self.required)
            .field(
                "supports_parallel_tool_calls",
                &self.supports_parallel_tool_calls,
            )
            .finish()
    }
}

fn duration_from_secs(value: Option<f64>) -> Result<Option<Duration>, ()> {
    value
        .map(|seconds| {
            (seconds.is_finite() && seconds >= 0.0)
                .then(|| Duration::try_from_secs_f64(seconds).map_err(|_| ()))
                .ok_or(())?
        })
        .transpose()
}

impl TryFrom<CodexRawMcpServerConfig> for CodexMcpServerConfig {
    type Error = ();

    fn try_from(raw: CodexRawMcpServerConfig) -> Result<Self, Self::Error> {
        let connection = crate::services::agent_cli::catalog::native::CODEX
            .decode(&serde_json::to_value(&raw).map_err(|_| ())?)
            .map_err(|_| ())?;
        let transport = if connection.transport == AgentMcpTransport::Stdio {
            if raw.url.is_some()
                || raw.bearer_token.is_some()
                || raw.bearer_token_env_var.is_some()
                || raw.http_headers.is_some()
                || raw.env_http_headers.is_some()
                || raw.http_headers_helper.is_some()
                || raw.oauth.is_some()
                || raw.oauth_resource.is_some()
                || raw.auth.is_some()
            {
                return Err(());
            }
            if raw.env_vars.as_ref().is_some_and(|vars| {
                vars.iter().any(|var| match var {
                    CodexMcpServerEnvVar::Name(name) => {
                        let _ = name;
                        false
                    }
                    CodexMcpServerEnvVar::Descriptor(CodexMcpServerEnvVarDescriptor {
                        name,
                        source,
                        ..
                    }) => {
                        let _ = name;
                        source
                            .as_deref()
                            .is_some_and(|source| !matches!(source, "local" | "remote"))
                    }
                })
            }) {
                return Err(());
            }
            AgentMcpTransport::Stdio
        } else if connection.transport == AgentMcpTransport::Http {
            if raw.args.is_some()
                || raw.env.is_some()
                || raw.env_vars.is_some()
                || raw.cwd.is_some()
                || raw.bearer_token.is_some()
            {
                return Err(());
            }
            if raw
                .http_headers_helper
                .as_deref()
                .is_some_and(|helper| helper.trim().is_empty())
            {
                return Err(());
            }
            if raw.http_headers_helper.is_some()
                && raw
                    .experimental_environment
                    .as_deref()
                    .or(raw.environment_id.as_deref())
                    .unwrap_or("local")
                    != "local"
            {
                return Err(());
            }
            AgentMcpTransport::Http
        } else {
            return Err(());
        };
        // These fields are intentionally carried through deserialization to
        // mirror the pinned native type, even though inventory does not expose
        // their sensitive values or impose extra validation on them.
        let _ = &raw.name;
        if let Some(oauth) = raw.oauth.as_ref() {
            let _ = (&oauth.client_id, &oauth.callback_url, &oauth.callback_port);
        }
        let startup_timeout = match raw.startup_timeout_sec {
            Some(seconds) => duration_from_secs(Some(seconds))?,
            None => raw.startup_timeout_ms.map(Duration::from_millis),
        };
        Ok(Self {
            transport,
            command: raw.command,
            url: raw.url,
            enabled: raw.enabled.unwrap_or(true),
            environment_id: raw
                .experimental_environment
                .or(raw.environment_id)
                .unwrap_or_else(|| "local".to_owned()),
            auth: raw.auth.unwrap_or_default(),
            args: raw.args.unwrap_or_default(),
            env_vars: raw.env_vars.unwrap_or_default(),
            enabled_tools: raw.enabled_tools,
            disabled_tools: raw.disabled_tools,
            tools: raw.tools.unwrap_or_default(),
            omit_tools_from: raw.omit_tools_from,
            default_tools_approval_mode: raw.default_tools_approval_mode,
            scopes: raw.scopes,
            startup_timeout,
            tool_timeout: duration_from_secs(raw.tool_timeout_sec)?,
            required: raw.required.unwrap_or(false),
            supports_parallel_tool_calls: raw.supports_parallel_tool_calls.unwrap_or(false),
        })
    }
}

fn decode_mcp_config(table: &TomlTable) -> Result<CodexMcpServerConfig, ()> {
    let raw: CodexRawMcpServerConfig = toml::Value::Table(table.clone())
        .try_into()
        .map_err(|_| ())?;
    raw.try_into()
}

enum RequirementsSnapshot {
    Missing,
    NoPolicy,
    Allowlist(CodexMcpRequirementsTable),
}

type CodexMcpRequirementsTable = BTreeMap<String, CodexMcpRequirement>;

fn parse_requirements_snapshot(snapshot: &AgentAssetSnapshot) -> Result<RequirementsSnapshot, ()> {
    let AgentAssetSnapshot::File { bytes, .. } = snapshot else {
        return match snapshot {
            AgentAssetSnapshot::Missing { .. } => Ok(RequirementsSnapshot::Missing),
            _ => Err(()),
        };
    };
    let root = std::str::from_utf8(bytes)
        .map_err(|_| ())?
        .parse::<toml::Value>()
        .map_err(|_| ())?;
    let Some(table) = root.as_table() else {
        return Err(());
    };
    match decode_requirements_table(table)? {
        Some(servers) => Ok(RequirementsSnapshot::Allowlist(servers)),
        None => Ok(RequirementsSnapshot::NoPolicy),
    }
}

impl CodexMcpRequirement {
    fn transport(&self) -> AgentMcpTransport {
        match self.identity {
            CodexRawMcpServerIdentity::LegacyCommand { .. }
            | CodexRawMcpServerIdentity::CommandMatcher(_) => AgentMcpTransport::Stdio,
            CodexRawMcpServerIdentity::LegacyUrl { .. }
            | CodexRawMcpServerIdentity::UrlMatcher(_) => AgentMcpTransport::Http,
        }
    }

    fn matches(&self, config: &CodexMcpServerConfig) -> bool {
        if self.transport() != config.transport {
            return false;
        }
        match (&self.identity, config.transport) {
            (CodexRawMcpServerIdentity::LegacyCommand { command }, AgentMcpTransport::Stdio) => {
                config.command.as_deref() == Some(command.as_str())
            }
            (CodexRawMcpServerIdentity::LegacyUrl { url }, AgentMcpTransport::Http) => {
                config.url.as_deref() == Some(url.as_str())
            }
            (CodexRawMcpServerIdentity::CommandMatcher(command), AgentMcpTransport::Stdio) => {
                config.command.as_deref() == Some(command.command.executable.as_str())
                    && command.command.args.len() == config.args.len()
                    && command
                        .command
                        .args
                        .iter()
                        .zip(&config.args)
                        .all(|(expected, actual)| expected.matches(actual))
            }
            (CodexRawMcpServerIdentity::UrlMatcher(url), AgentMcpTransport::Http) => config
                .url
                .as_deref()
                .is_some_and(|actual| url.url.matches(actual)),
            _ => false,
        }
    }
}

impl CodexMcpServerValueMatcher {
    fn matches(&self, actual: &str) -> bool {
        match self {
            Self::Exact { value } => actual == value,
            Self::Prefix { value } => actual.starts_with(value),
            Self::Regex { expression } => regex_lite::Regex::new(&format!(r"\A(?:{expression})\z"))
                .is_ok_and(|regex| regex.is_match(actual)),
        }
    }

    fn validate(&self) -> bool {
        match self {
            Self::Regex { expression } => {
                regex_lite::Regex::new(expression).is_ok()
                    && regex_lite::Regex::new(&format!(r"\A(?:{expression})\z")).is_ok()
            }
            Self::Exact { .. } | Self::Prefix { .. } => true,
        }
    }
}

fn decode_requirements_table(root: &TomlTable) -> Result<Option<CodexMcpRequirementsTable>, ()> {
    let Some(value) = root.get("mcp_servers") else {
        return Ok(None);
    };
    let Some(servers) = value.as_table() else {
        return Err(());
    };
    let mut decoded = BTreeMap::new();
    for (native_id, value) in servers {
        if native_id.trim().is_empty() || native_id.chars().any(char::is_control) {
            return Err(());
        }
        let Some(_) = value.as_table() else {
            return Err(());
        };
        let requirement: CodexMcpRequirement = value.clone().try_into().map_err(|_| ())?;
        let valid = match &requirement.identity {
            CodexRawMcpServerIdentity::LegacyCommand { .. }
            | CodexRawMcpServerIdentity::LegacyUrl { .. } => true,
            CodexRawMcpServerIdentity::CommandMatcher(command) => command
                .command
                .args
                .iter()
                .all(CodexMcpServerValueMatcher::validate),
            CodexRawMcpServerIdentity::UrlMatcher(url) => url.url.validate(),
        };
        if !valid {
            return Err(());
        }
        decoded.insert(native_id.clone(), requirement);
    }
    Ok(Some(decoded))
}

/// General settings and resource inventory consume the same native declarations.
pub(super) fn discover_sources(
    request: crate::services::agent_cli::contracts::AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn crate::services::agent_cli::contracts::InitialSourceOutput,
) {
    crate::services::agent_cli::configuration::native_support::discover_inventory_sources(
        discover_resource_sources,
        super::configuration::discover_additional,
        request,
        output,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentAssetResolutionRelation, AgentAssetState};
    use crate::services::agent_cli::contracts::{
        AgentAssetEffectiveStateProofDraft, AgentAssetResolveRequest, AgentAssetResolveSource,
        AgentAssetStateEvidenceRefDraft, AgentAssetTerminalCauseDraft, AgentResolveOutput,
    };
    use std::path::Path;

    #[derive(Default)]
    struct StopAfterFirst {
        declarations: usize,
    }

    impl AgentDiagnosticOutput for StopAfterFirst {
        fn has_regular_capacity(&self) -> bool {
            true
        }

        fn emit_diagnostic(
            &mut self,
            _value: AgentAssetDiagnostic,
        ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        }
    }

    impl AgentParseOutput for StopAfterFirst {
        fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
            if matches!(
                value.category,
                AgentAssetCategory::Skill | AgentAssetCategory::Plugin
            ) {
                return ControlFlow::Continue(());
            }
            self.declarations += 1;
            ControlFlow::Break(AgentOutputStop::EntryLimit)
        }
    }

    #[test]
    fn parser_stops_before_status_after_mcp_break() {
        let context = crate::models::AgentConfigurationContext {
            id: "codex-context".to_owned(),
            environment_id: "native".to_owned(),
            agent_kind: crate::models::AgentCliKind::Codex,
            config_root: "/tmp/codex".to_owned(),
            profile: "default".to_owned(),
            workspace_id: None,
            trust_context: AgentTrustState::Unknown,
            parser_version: PARSER_VERSION,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        };
        let source = crate::services::agent_cli::contracts::AgentAssetSourceSpec {
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            verified_physical_path: None,
            hook_definition_source: true,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            native_source_key: "config".to_owned(),
            label: "config".to_owned(),
            scope: AgentAssetScope::User,
            path: "/tmp/codex/config.toml".into(),
            allowed_root: "/tmp/codex".into(),
            precedence: 10,
            writable: true,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![
                AgentAssetCategory::Mcp,
                AgentAssetCategory::StatusUi,
                AgentAssetCategory::Skill,
                AgentAssetCategory::Plugin,
            ],
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 10,
                },
            ],
        };
        let snapshot = AgentAssetSnapshot::File {
            bytes: br#"[mcp_servers.first]
command = "server"
[tui]
status_line = "status"
"#
            .to_vec(),
            revision: Default::default(),
        };
        let mut output = StopAfterFirst::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &source,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut output,
        );
        assert_eq!(output.declarations, 1);
    }

    #[test]
    fn toml_duplicate_server_is_malformed_instead_of_last_write_wins() {
        let source = r#"
[mcp_servers.same]
command = "one"
[mcp_servers.same]
command = "two"
"#;
        assert!(source.parse::<toml::Value>().is_err());
    }

    #[derive(Default)]
    struct ParseCollector {
        values: Vec<ParsedAgentAsset>,
    }

    impl AgentDiagnosticOutput for ParseCollector {
        fn has_regular_capacity(&self) -> bool {
            true
        }
        fn emit_diagnostic(
            &mut self,
            _value: AgentAssetDiagnostic,
        ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        }
    }

    impl AgentParseOutput for ParseCollector {
        fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
            self.values.push(value);
            ControlFlow::Continue(())
        }
    }

    #[derive(Default)]
    struct ResolveCollector {
        values: Vec<crate::services::agent_cli::contracts::AgentAssetProjectedDraft>,
        diagnostics: Vec<AgentAssetDiagnostic>,
    }

    impl AgentDiagnosticOutput for ResolveCollector {
        fn has_regular_capacity(&self) -> bool {
            true
        }
        fn emit_diagnostic(
            &mut self,
            value: AgentAssetDiagnostic,
        ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
            self.diagnostics.push(value);
            crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
        }
    }

    impl AgentResolveOutput for ResolveCollector {
        fn emit_draft(
            &mut self,
            value: crate::services::agent_cli::contracts::AgentAssetProjectedDraft,
        ) -> ControlFlow<AgentOutputStop> {
            self.values.push(value);
            ControlFlow::Continue(())
        }
    }

    fn fixture_context(trust_context: AgentTrustState) -> crate::models::AgentConfigurationContext {
        crate::models::AgentConfigurationContext {
            id: "codex-fixture".to_owned(),
            environment_id: "native".to_owned(),
            agent_kind: crate::models::AgentCliKind::Codex,
            config_root: "/tmp/codex-fixture".to_owned(),
            profile: "default".to_owned(),
            workspace_id: Some("/tmp/project".to_owned()),
            trust_context,
            parser_version: PARSER_VERSION,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        }
    }

    fn fixture_source(
        key: &str,
        precedence: u32,
        path: &str,
    ) -> crate::services::agent_cli::contracts::AgentAssetSourceSpec {
        crate::services::agent_cli::contracts::AgentAssetSourceSpec {
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            verified_physical_path: None,
            hook_definition_source: true,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            native_source_key: key.to_owned(),
            label: key.to_owned(),
            scope: match key {
                "workspace-config" => AgentAssetScope::Workspace,
                "system-requirements" => AgentAssetScope::Managed,
                _ => AgentAssetScope::User,
            },
            path: path.into(),
            allowed_root: "/tmp".into(),
            precedence,
            writable: true,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: if matches!(key, "config" | "workspace-config" | "system-config") {
                vec![
                    AgentAssetCategory::Mcp,
                    AgentAssetCategory::Skill,
                    AgentAssetCategory::Plugin,
                    AgentAssetCategory::StatusUi,
                ]
            } else {
                vec![AgentAssetCategory::Mcp]
            },
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: match key {
                        "workspace-config" => AgentAssetScope::Workspace,
                        "system-requirements" => AgentAssetScope::Managed,
                        _ => AgentAssetScope::User,
                    },
                    precedence,
                },
            ],
        }
    }

    fn fixture_snapshot(value: &str) -> AgentAssetSnapshot {
        AgentAssetSnapshot::File {
            bytes: value.as_bytes().to_vec(),
            revision: Default::default(),
        }
    }

    #[test]
    fn production_codex_merge_keeps_tables_and_replaces_lists() {
        let context = fixture_context(AgentTrustState::Trusted);
        let user = fixture_source("config", 10, "/tmp/config.toml");
        let workspace = fixture_source("workspace-config", 20, "/tmp/project/.codex/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let user_snapshot = fixture_snapshot(
            "[mcp_servers.demo]\ncommand = \"runner\"\nargs = [\"base\"]\nenabled_tools = [\"read\", \"write\"]\n[mcp_servers.demo.tools.search]\napproval_mode = \"auto\"\n",
        );
        let project_snapshot = fixture_snapshot(
            "[mcp_servers.demo]\nenabled = false\nargs = [\"project\"]\nenabled_tools = [\"b\", \"c\"]\ndisabled_tools = [\"b\"]\n[mcp_servers.demo.tools.search]\noutput_token_limit = 10\n",
        );
        let missing_snapshot = AgentAssetSnapshot::Missing {
            revision: Default::default(),
        };
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &user,
                snapshot: &user_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &workspace,
                snapshot: &project_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        assert_eq!(parsed.values.len(), 8);
        assert_eq!(
            parsed
                .values
                .iter()
                .filter(|value| matches!(
                    value.native_payload,
                    AgentAssetNativePayload::CodexHook(
                        hooks::CodexHookPayload::FeaturePolicy { .. }
                    )
                ))
                .count(),
            2
        );
        assert_eq!(
            parsed
                .values
                .iter()
                .filter(|value| matches!(
                    value.native_payload,
                    AgentAssetNativePayload::CodexHook(hooks::CodexHookPayload::Policy { .. })
                ))
                .count(),
            1
        );
        let config_layers = parsed
            .values
            .iter()
            .filter(|value| matches!(value.native_payload, AgentAssetNativePayload::TomlTable(_)))
            .collect::<Vec<_>>();
        assert_eq!(config_layers.len(), 2);
        let (_, decoded) = merge_and_decode_mcp(&config_layers).unwrap();
        assert_eq!(decoded.args, vec!["project"]);
        assert_eq!(
            decoded.enabled_tools,
            Some(vec!["b".to_owned(), "c".to_owned()])
        );
        assert_eq!(decoded.disabled_tools, Some(vec!["b".to_owned()]));
        assert!(decoded
            .enabled_tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(|tool| tool == "b")));
        assert!(decoded
            .disabled_tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(|tool| tool == "b")));
        assert_eq!(decoded.command.as_deref(), Some("runner"));
        assert_eq!(decoded.tools.len(), 1);
        assert!(matches!(
            decoded.tools["search"].approval_mode,
            Some(CodexAppToolApproval::Auto)
        ));
        assert_eq!(
            decoded.tools["search"].output_token_limit,
            NonZeroUsize::new(10)
        );
        let mut resolved = ResolveCollector::default();
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &user,
                snapshot: &user_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &workspace,
                snapshot: &project_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &missing_snapshot,
            },
        ];
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &missing_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        let draft = &resolved.values[0];
        assert_eq!(
            draft.resolution.relation,
            crate::models::AgentAssetResolutionRelation::Merged
        );
        assert_eq!(draft.declared_state, AgentAssetDeclaredState::Disabled);
        assert_eq!(draft.inspection_source_id, "workspace-config");
        assert_eq!(draft.contributor_ids.len(), 2);
    }

    #[test]
    fn production_codex_deep_merge_fixture_observes_inherited_values() {
        let facts = decode_merged_mcp_fixture(&[
            "[mcp_servers.same]\ncommand = \"runner\"\nargs = [\"base\"]\nenabled_tools = [\"read\", \"write\"]\n[mcp_servers.same.tools.search]\napproval_mode = \"auto\"\n",
            "[mcp_servers.same]\nargs = [\"workspace\"]\nenabled_tools = [\"workspace\"]\n[mcp_servers.same.tools.search]\noutput_token_limit = 10\n",
        ]);
        assert_eq!(
            facts,
            Ok(CodexFixtureMcpFacts {
                command: Some("runner".to_owned()),
                args: vec!["workspace".to_owned()],
                enabled_tools: Some(vec!["workspace".to_owned()]),
                disabled_tools: None,
                search_approval: Some("auto".to_owned()),
                search_output_token_limit: Some(10),
                enabled: true,
            })
        );
    }

    #[test]
    fn pinned_codex_accepts_empty_optional_mcp_values() {
        let stdio: TomlTable = "command = \"runner\"\nenv_vars = [\"\", { name = \"\", source = \"local\" }]\nname = \"\"\n"
            .parse::<toml::Value>()
            .expect("pinned stdio fixture")
            .as_table()
            .cloned()
            .expect("pinned stdio table");
        let decoded = decode_mcp_config(&stdio).expect("pinned stdio values remain valid");
        assert_eq!(decoded.transport, AgentMcpTransport::Stdio);
        assert_eq!(decoded.env_vars.len(), 2);

        let http: TomlTable = "url = \"https://example.invalid\"\nname = \"\"\n[oauth]\nclient_id = \"\"\ncallback_url = \"\"\ncallback_port = 0\n"
            .parse::<toml::Value>()
            .expect("pinned http fixture")
            .as_table()
            .cloned()
            .expect("pinned http table");
        let decoded = decode_mcp_config(&http).expect("pinned oauth values remain valid");
        assert_eq!(decoded.transport, AgentMcpTransport::Http);
    }

    #[test]
    fn workspace_trust_uses_canonical_and_lexical_keys_and_fails_closed_on_ambiguity() {
        let source = fixture_source("config", 10, "/tmp/codex/config.toml");
        let snapshot =
            fixture_snapshot("[projects.\"/tmp/workspace/./\"]\ntrust_level = \"trusted\"\n");
        let sources = [AgentAssetResolveSource {
            spec: &source,
            snapshot: &snapshot,
        }];
        let mut output = ResolveCollector::default();
        assert_eq!(
            resolve_workspace_trust(
                AgentWorkspaceTrustResolveRequest {
                    workspace: Path::new("/tmp/workspace"),
                    workspace_lexical: Some(Path::new("/tmp/workspace/./")),
                    sources: &sources,
                },
                &mut output,
            ),
            AgentTrustState::Trusted
        );

        let ambiguous = fixture_snapshot(
            "[projects.\"/tmp/workspace\"]\ntrust_level = \"trusted\"\n[projects.\"/tmp/workspace/./\"]\n",
        );
        let sources = [AgentAssetResolveSource {
            spec: &source,
            snapshot: &ambiguous,
        }];
        assert_eq!(
            resolve_workspace_trust(
                AgentWorkspaceTrustResolveRequest {
                    workspace: Path::new("/tmp/workspace"),
                    workspace_lexical: Some(Path::new("/tmp/workspace/./")),
                    sources: &sources,
                },
                &mut output,
            ),
            AgentTrustState::Unknown
        );
        let missing_level = fixture_snapshot(
            "[projects.\"/tmp/workspace\"]\n[projects.\"/tmp/workspace/./\"]\ntrust_level = \"trusted\"\n",
        );
        let sources = [AgentAssetResolveSource {
            spec: &source,
            snapshot: &missing_level,
        }];
        assert_eq!(
            resolve_workspace_trust(
                AgentWorkspaceTrustResolveRequest {
                    workspace: Path::new("/tmp/workspace"),
                    workspace_lexical: Some(Path::new("/tmp/workspace/./")),
                    sources: &sources,
                },
                &mut output,
            ),
            AgentTrustState::Unknown
        );
        assert!(workspace_key_matches("C:/Work", "c:/work", true));
        assert!(workspace_key_matches(
            "C:/Work/Project",
            "c:/work/project",
            true
        ));
        assert!(!workspace_key_matches(
            "/tmp/workspace",
            "/tmp/Workspace",
            false
        ));
    }

    #[test]
    fn production_codex_requirements_missing_id_uses_source_policy_reference() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let config_snapshot = fixture_snapshot("[mcp_servers.demo]\ncommand = \"runner\"\n");
        let requirements_snapshot =
            fixture_snapshot("[mcp_servers.other.identity]\ncommand = \"other\"\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &config_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let mut resolved = ResolveCollector::default();
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &config_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        let draft = &resolved.values[0];
        assert_eq!(
            draft.resolution.relation,
            crate::models::AgentAssetResolutionRelation::Independent
        );
        assert!(matches!(
            draft.resolution.control_source,
            Some(
                crate::services::agent_cli::contracts::AgentAssetPolicyReferenceDraft::Source { .. }
            )
        ));
        assert_eq!(draft.contributor_ids.len(), 1);
        assert!(matches!(
            &draft.state_proof.effective,
            AgentAssetEffectiveStateProofDraft::Terminal {
                cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                evidence,
                ..
            } if matches!(evidence.as_slice(), [AgentAssetStateEvidenceRefDraft::Source { source_key }]
                if source_key == &requirements.native_source_key)
        ));
        assert!(parsed
            .values
            .iter()
            .all(|asset| asset.native_id != "requirements"));
    }

    #[test]
    fn codex_requirements_declaration_ref_contributes_matching_overlay() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let config_snapshot = fixture_snapshot("[mcp_servers.demo]\ncommand = \"runner\"\n");
        let requirements_snapshot =
            fixture_snapshot("[mcp_servers.demo.identity]\ncommand = \"runner\"\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &config_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &config_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        assert_eq!(
            resolved.values[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(resolved.values[0].contributor_ids.len(), 2);
        assert!(resolved.values[0].contributor_ids.iter().any(|id| {
            parsed.values.iter().any(|asset| {
                asset.declaration_id == *id
                    && asset.role == crate::models::AgentAssetDeclarationRole::PolicyOverlay
            })
        }));
    }

    #[test]
    fn codex_requirements_absent_mcp_servers_means_no_policy() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let config_snapshot = fixture_snapshot("[mcp_servers.demo]\ncommand = \"runner\"\n");
        let requirements_snapshot = fixture_snapshot("[other]\nvalue = true\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &config_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &config_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        assert_eq!(
            resolved.values[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(resolved.values[0].effective_state, AgentAssetState::Enabled);
    }

    #[test]
    fn codex_requirements_empty_allowlist_blocks_with_source_reference() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let config_snapshot = fixture_snapshot("[mcp_servers.demo]\ncommand = \"runner\"\n");
        let requirements_snapshot = fixture_snapshot("mcp_servers = {}\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &config_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &config_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        assert_eq!(
            resolved.values[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(matches!(
            resolved.values[0].resolution.control_source,
            Some(
                crate::services::agent_cli::contracts::AgentAssetPolicyReferenceDraft::Source { .. }
            )
        ));
        assert!(matches!(
            &resolved.values[0].state_proof.effective,
            AgentAssetEffectiveStateProofDraft::Terminal {
                cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                evidence,
                ..
            } if matches!(evidence.as_slice(), [AgentAssetStateEvidenceRefDraft::Source { source_key }]
                if source_key == &requirements.native_source_key)
        ));
    }

    #[test]
    fn codex_requirements_malformed_snapshot_is_unknown_and_read_only() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let config_snapshot = fixture_snapshot("[mcp_servers.demo]\ncommand = \"runner\"\n");
        let requirements_snapshot = fixture_snapshot("mcp_servers = \"not-a-table\"\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &config_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &config_snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        assert_eq!(
            resolved.values[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(resolved.values[0].effective_state, AgentAssetState::Unknown);
    }

    #[test]
    fn codex_all_suppressed_workspace_group_remains_unknown_evidence() {
        let context = fixture_context(AgentTrustState::Unknown);
        let workspace = fixture_source("workspace-config", 20, "/tmp/project/.codex/config.toml");
        let snapshot = fixture_snapshot("[mcp_servers.project]\ncommand = \"runner\"\n");
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &workspace,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        assert_eq!(parsed.values.len(), 3);
        assert_eq!(
            parsed
                .values
                .iter()
                .filter(|asset| matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy { .. })
                ))
                .count(),
            1
        );
        let mcp = parsed
            .values
            .iter()
            .find(|value| value.category == AgentAssetCategory::Mcp)
            .unwrap();
        assert!(matches!(
            mcp.participation,
            crate::models::AgentAssetResolutionParticipation::Suppressed { .. }
        ));
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &workspace,
                snapshot: &snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert!(resolved.values.is_empty());
        assert!(resolved.diagnostics.iter().all(|diagnostic| !matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidResolution { .. }
        )));
    }

    #[test]
    fn codex_invalid_merged_schema_never_defaults_to_enabled() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let snapshot = fixture_snapshot(
            "[mcp_servers.invalid]\ncommand = \"runner\"\nenabled = \"false\"\nargs = 1\n",
        );
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 1);
        assert_eq!(
            resolved.values[0].resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert_eq!(resolved.values[0].effective_state, AgentAssetState::Unknown);
        assert!(resolved
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
    }

    #[test]
    fn codex_different_ids_are_additive_set() {
        let context = fixture_context(AgentTrustState::Trusted);
        let config = fixture_source("config", 10, "/tmp/config.toml");
        let requirements = fixture_source("system-requirements", 100, "/tmp/requirements.toml");
        let snapshot = fixture_snapshot(
            "[mcp_servers.first]\ncommand = \"one\"\n[mcp_servers.second]\nurl = \"https://two.invalid\"\n",
        );
        let requirements_snapshot = AgentAssetSnapshot::Missing {
            revision: Default::default(),
        };
        let mut parsed = ParseCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &config,
                snapshot: &snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        let sources = [
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &config,
                snapshot: &snapshot,
            },
            crate::services::agent_cli::contracts::AgentAssetResolveSource {
                spec: &requirements,
                snapshot: &requirements_snapshot,
            },
        ];
        let mut resolved = ResolveCollector::default();
        parse_assets(
            AgentAssetParseRequest {
                native_home: None,
                context: &context,
                source: &requirements,
                snapshot: &requirements_snapshot,
                workspace_canonical: None,
                workspace_lexical: None,
            },
            &mut parsed,
        );
        resolve_assets(
            AgentAssetResolveRequest {
                context: &context,
                declarations: &parsed.values,
                sources: &sources,
            },
            &mut resolved,
        );
        assert_eq!(resolved.values.len(), 2);
        assert!(resolved
            .values
            .iter()
            .all(|draft| draft.resolution.relation == AgentAssetResolutionRelation::Independent));
        assert_ne!(
            resolved.values[0].projection_key,
            resolved.values[1].projection_key
        );
    }

    #[test]
    fn codex_requirements_matchers_require_tagged_exact_shapes() {
        assert!(inline_value("\"bare\"")
            .try_into::<CodexMcpServerValueMatcher>()
            .is_err());
        assert!(inline_value("{ value = \"missing-tag\" }")
            .try_into::<CodexMcpServerValueMatcher>()
            .is_err());
        assert!(
            inline_value("{ match = \"exact\", value = \"x\", extra = true }")
                .try_into::<CodexMcpServerValueMatcher>()
                .is_err()
        );
        assert!(matches!(
            inline_value("{ match = \"prefix\", value = \"x\" }")
                .try_into::<CodexMcpServerValueMatcher>(),
            Ok(CodexMcpServerValueMatcher::Prefix { .. })
        ));
        assert!(matches!(
            inline_value("{ match = \"regex\", expression = \"x[0-9]+\" }")
                .try_into::<CodexMcpServerValueMatcher>(),
            Ok(CodexMcpServerValueMatcher::Regex { .. })
        ));
        let invalid_regex = inline_value("{ match = \"regex\", expression = \"[\" }")
            .try_into::<CodexMcpServerValueMatcher>()
            .unwrap();
        assert!(!invalid_regex.validate());
        let legacy_url = inline_value("{ identity = { url = \"https://example.invalid\" } }");
        let requirement: CodexMcpRequirement = legacy_url.try_into().unwrap();
        assert_eq!(requirement.transport(), AgentMcpTransport::Http);
    }

    fn inline_value(input: &str) -> toml::Value {
        format!("value = {input}")
            .parse::<toml::Value>()
            .unwrap()
            .as_table()
            .and_then(|table| table.get("value"))
            .cloned()
            .unwrap()
    }

    #[test]
    fn codex_native_payload_debug_never_contains_configuration_values() {
        let table = "command = \"command-secret\"\nargs = [\"arg-secret\"]\nurl = \"url-secret\"\n"
            .parse::<toml::Value>()
            .unwrap()
            .as_table()
            .unwrap()
            .clone();
        let debug = format!("{:?}", AgentAssetNativePayload::TomlTable(table));
        assert!(!debug.contains("command-secret"));
        assert!(!debug.contains("arg-secret"));
        assert!(!debug.contains("url-secret"));
        assert!(debug.contains("field_count"));
    }
}
