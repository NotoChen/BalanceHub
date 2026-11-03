use super::{
    malformed, mcp, sources, PLUGIN_BASE_PREFIX, PLUGIN_MANIFEST_PREFIX, PLUGIN_MCP_PREFIX,
};
pub(in crate::services::agent_cli::codex) mod hooks;
use crate::models::{
    AgentAssetCategory, AgentAssetDeclarationRole, AgentAssetDeclaredState, AgentAssetDetails,
    AgentAssetDocumentFormat, AgentAssetInstallState, AgentAssetNativeRef,
    AgentAssetResolutionParticipation, AgentAssetSourceKind, AgentAssetSuppressionReason,
    AgentMcpApprovalState, AgentMcpTransport, AgentTrustState,
};
use crate::services::agent_cli::contracts::{
    AgentAssetDirectoryEntry, AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot,
    AgentOutputStop, AgentParseOutput, CodexAssetPayload,
};
use crate::services::agent_cli::environment::{
    config_document::{self, ConfigDocumentFormat},
    declaration_trust_for_participation, parsed_asset, physical_origin, ParsedAssetInput,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    path::{Component, Path, PathBuf},
};

pub(in crate::services::agent_cli::codex::environment) const CONFIG_ROOT_KEY: &str = "plugins:root";
pub(in crate::services::agent_cli::codex::environment) const CONFIG_ROOT_ID: &str =
    "__balancehub_codex_plugin_config__";
pub(super) const AGENT_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
pub(super) const AGENT_SCHEMA_PREFIX: &str = "https://agent-plugins.org/schemas/";

pub(in crate::services::agent_cli::codex::environment) fn plugin_id_parts(
    id: &str,
) -> Option<(&str, &str)> {
    let (plugin, marketplace) = id.rsplit_once('@')?;
    let valid = |part: &str, dots: bool| {
        !part.is_empty()
            && (!dots || (!part.starts_with('.') && !part.ends_with('.') && !part.contains("..")))
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'_')
                    || (dots && byte == b'.')
            })
    };
    (valid(plugin, true) && valid(marketplace, false)).then_some((plugin, marketplace))
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && !matches!(version, "." | "..")
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

pub(in crate::services::agent_cli::codex::environment) fn active_version(
    versions: &[String],
) -> Option<&str> {
    if versions.iter().any(|version| version == "local") {
        return Some("local");
    }
    versions
        .iter()
        .filter(|version| valid_version(version))
        .max_by(
            |left, right| match (semver::Version::parse(left), semver::Version::parse(right)) {
                (Ok(left), Ok(right)) => left.cmp(&right),
                _ => left.cmp(right),
            },
        )
        .map(String::as_str)
}

pub(super) fn versions(entries: &[AgentAssetDirectoryEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| {
            !entry.is_symlink
                && entry.source_kind == AgentAssetSourceKind::Directory
                && valid_version(&entry.name)
        })
        .map(|entry| entry.name.clone())
        .collect()
}

#[derive(Deserialize)]
pub(in crate::services::agent_cli::codex::environment) struct PluginConfig {
    #[serde(default = "enabled_default")]
    pub(in crate::services::agent_cli::codex::environment) enabled: bool,
    #[serde(default)]
    pub(in crate::services::agent_cli::codex::environment) mcp_servers:
        BTreeMap<String, PluginMcpPolicy>,
}

#[derive(Deserialize)]
pub(in crate::services::agent_cli::codex::environment) struct PluginMcpPolicy {
    #[serde(default = "enabled_default")]
    pub(in crate::services::agent_cli::codex::environment) enabled: bool,
    #[serde(default)]
    default_tools_approval_mode: Option<super::super::CodexAppToolApproval>,
    #[serde(default)]
    enabled_tools: Option<Vec<String>>,
    #[serde(default)]
    disabled_tools: Option<Vec<String>>,
    #[serde(default)]
    tools: BTreeMap<String, super::super::CodexMcpServerToolConfig>,
}

fn enabled_default() -> bool {
    true
}

pub(in crate::services::agent_cli::codex::environment) fn decode_plugin_config(
    table: &toml::value::Table,
) -> Option<PluginConfig> {
    let config: PluginConfig = toml::Value::Table(table.clone()).try_into().ok()?;
    // All policy fields are decoded with their native types; inventory only
    // needs the enablement result and never publishes tool configuration.
    for policy in config.mcp_servers.values() {
        let _ = (
            &policy.default_tools_approval_mode,
            &policy.enabled_tools,
            &policy.disabled_tools,
            &policy.tools,
        );
    }
    Some(config)
}

pub(super) fn parse_config(
    request: AgentAssetParseRequest<'_>,
    root: Option<&toml::value::Table>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let plugin_value = root.and_then(|root| root.get("plugins"));
    let plugins = plugin_value.and_then(toml::Value::as_table);
    let valid = (root.is_some() || matches!(request.snapshot, AgentAssetSnapshot::Missing { .. }))
        && plugin_value.is_none_or(toml::Value::is_table)
        && plugins.is_none_or(|entries| entries.values().all(toml::Value::is_table));
    let mut count = 0;
    if let Some(plugins) = plugins {
        for (id, value) in plugins {
            let Some(table) = value.as_table() else {
                continue;
            };
            count += 1;
            let state = table
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .map_or(AgentAssetDeclaredState::Unknown, declared);
            emit_plugin(
                request,
                id,
                &format!("plugins.entry:{id}"),
                state,
                (
                    AgentAssetInstallState::Unknown,
                    AgentAssetDeclarationRole::Definition,
                ),
                CodexAssetPayload::PluginConfig {
                    table: table.clone(),
                },
                output,
            )?;
            parse_mcp_policy(request, id, table, output)?;
        }
    }
    if !valid && matches!(request.snapshot, AgentAssetSnapshot::File { .. }) {
        malformed(output, AgentAssetDocumentFormat::Toml);
    }
    emit_plugin(
        request,
        CONFIG_ROOT_ID,
        CONFIG_ROOT_KEY,
        AgentAssetDeclaredState::Unknown,
        (
            AgentAssetInstallState::Unknown,
            AgentAssetDeclarationRole::PolicyOverlay,
        ),
        CodexAssetPayload::PluginConfigRoot {
            entry_count: count,
            valid,
        },
        output,
    )
}

pub(super) fn declared(enabled: bool) -> AgentAssetDeclaredState {
    if enabled {
        AgentAssetDeclaredState::Enabled
    } else {
        AgentAssetDeclaredState::Disabled
    }
}

fn parse_mcp_policy(
    request: AgentAssetParseRequest<'_>,
    id: &str,
    table: &toml::value::Table,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let Some(servers) = table.get("mcp_servers").and_then(toml::Value::as_table) else {
        return ControlFlow::Continue(());
    };
    for (name, value) in servers {
        let Some(table) = value.as_table() else {
            continue;
        };
        let participation = if request.source.native_source_key == "workspace-config"
            && request.context.trust_context != AgentTrustState::Trusted
        {
            AgentAssetResolutionParticipation::Suppressed {
                reason: AgentAssetSuppressionReason::UntrustedWorkspace,
            }
        } else {
            AgentAssetResolutionParticipation::Participates
        };
        let state = table
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .map_or(AgentAssetDeclaredState::Unknown, declared);
        let policy_key = format!("plugins.mcp:{id}:{name}");
        let mut declaration = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &policy_key,
                resolution_group_key: &policy_key,
                category: AgentAssetCategory::Mcp,
                native_id: name,
                label: name,
                logical_origin: physical_origin(request.source),
                declared_state: state,
                trust_state: declaration_trust_for_participation(
                    request.context.trust_context,
                    participation,
                ),
                role: AgentAssetDeclarationRole::PolicyOverlay,
                participation,
                provided_by: None,
                action_owner: None,
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Mcp {
                    transport: AgentMcpTransport::Unknown,
                    declared_state: state,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
                },
                facts: BTreeMap::new(),
            },
        );
        declaration.native_payload =
            AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp {
                table: table.clone(),
            });
        output.emit_declaration(declaration)?;
    }
    ControlFlow::Continue(())
}

fn emit_plugin(
    request: AgentAssetParseRequest<'_>,
    id: &str,
    key: &str,
    state: AgentAssetDeclaredState,
    structure: (AgentAssetInstallState, AgentAssetDeclarationRole),
    payload: CodexAssetPayload,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    let (install_state, role) = structure;
    let participation = if request.source.native_source_key == "workspace-config"
        && request.context.trust_context != AgentTrustState::Trusted
    {
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    } else {
        AgentAssetResolutionParticipation::Participates
    };
    let mut asset = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: key,
            resolution_group_key: id,
            category: AgentAssetCategory::Plugin,
            native_id: id,
            label: id,
            logical_origin: physical_origin(request.source),
            declared_state: state,
            trust_state: declaration_trust_for_participation(
                request.context.trust_context,
                participation,
            ),
            role,
            participation,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Plugin {
                install_state,
                enabled: state,
                trusted: AgentTrustState::Unknown,
            },
            facts: BTreeMap::new(),
        },
    );
    asset.native_payload = AgentAssetNativePayload::CodexAsset(payload);
    output.emit_declaration(asset)
}

pub(super) fn parse_versions(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let Some(id) = request
        .source
        .native_source_key
        .strip_prefix(PLUGIN_BASE_PREFIX)
    else {
        return;
    };
    let (versions, complete) = match request.snapshot {
        AgentAssetSnapshot::DirectoryManifest {
            entries, complete, ..
        } => (versions(entries), *complete),
        AgentAssetSnapshot::Missing { .. } => (Vec::new(), true),
        _ => (Vec::new(), false),
    };
    let install_state = if complete && versions.is_empty() {
        AgentAssetInstallState::NotInstalled
    } else {
        AgentAssetInstallState::Unknown
    };
    let _ = emit_plugin(
        request,
        id,
        "cache:versions",
        AgentAssetDeclaredState::Unknown,
        (install_state, AgentAssetDeclarationRole::Definition),
        CodexAssetPayload::PluginVersions { versions, complete },
        output,
    );
}

pub(super) struct Manifest {
    pub(super) namespace: String,
    pub(super) skill_roots: Vec<PathBuf>,
    pub(super) mcp_path: Option<PathBuf>,
    pub(super) inline_mcp: Option<Map<String, Value>>,
    pub(super) hook_sources: Vec<hooks::Component>,
}

pub(super) fn decode_manifest(bytes: &[u8], root: &Path, agent_format: bool) -> Option<Manifest> {
    let value = config_document::parse(bytes, ConfigDocumentFormat::Json)?;
    decode_manifest_object(value.as_object()?, root, agent_format)
}

fn decode_manifest_object(
    object: &Map<String, Value>,
    root: &Path,
    agent_format: bool,
) -> Option<Manifest> {
    let raw_name = match object.get("name") {
        Some(Value::String(name)) => name.as_str(),
        None if !agent_format => "",
        _ => return None,
    };
    let namespace = if agent_format {
        if object.get("$schema")?.as_str()? != AGENT_SCHEMA || !valid_agent_name(raw_name) {
            return None;
        }
        raw_name.to_owned()
    } else if raw_name.trim().is_empty() {
        root.file_name()?.to_str()?.to_owned()
    } else {
        raw_name.to_owned()
    };
    for field in ["version", "description"] {
        if object
            .get(field)
            .is_some_and(|value| !value.is_string() && (agent_format || !value.is_null()))
        {
            return None;
        }
    }
    if object.get("keywords").is_some_and(|value| {
        !value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string))
    }) {
        return None;
    }
    if agent_format {
        for field in ["homepage", "repository", "license"] {
            if object.get(field).is_some_and(|value| !value.is_string()) {
                return None;
            }
        }
        if let Some(author) = object.get("author") {
            let author = author.as_object()?;
            if author.iter().any(|(key, value)| {
                !matches!(key.as_str(), "name" | "email" | "url") || !value.is_string()
            }) {
                return None;
            }
        }
        if let Some(extension) = object
            .get("extensions")
            .and_then(Value::as_object)
            .and_then(|extensions| extensions.get("com.openai"))
            .filter(|value| value.is_object())
        {
            decode_manifest_object(extension.as_object()?, root, false)?;
        }
        return Some(Manifest {
            namespace,
            skill_roots: vec![root.join("skills")],
            mcp_path: Some(root.join("mcp.json")),
            inline_mcp: None,
            // Local AgentPlugin packages use remote executor hooks instead.
            hook_sources: Vec::new(),
        });
    }
    if !valid_legacy_metadata(object) {
        return None;
    }
    let mut skill_roots = match object.get("skills") {
        Some(Value::String(path)) => manifest_path(root, path).into_iter().collect(),
        Some(Value::Array(paths)) if paths.iter().all(Value::is_string) => paths
            .iter()
            .filter_map(|path| manifest_path(root, path.as_str()?))
            .collect(),
        _ => Vec::new(),
    };
    if skill_roots.is_empty() {
        skill_roots.push(root.join("skills"));
    }
    skill_roots.push(root.join(".codex-plugin/migrated-command-skills"));
    skill_roots.sort();
    skill_roots.dedup();
    let (mcp_path, inline_mcp) = match object.get("mcpServers") {
        Some(Value::Object(servers)) => (None, Some(servers.clone())),
        Some(Value::String(path)) => (
            manifest_path(root, path).or_else(|| Some(root.join(".mcp.json"))),
            None,
        ),
        _ => (Some(root.join(".mcp.json")), None),
    };
    Some(Manifest {
        namespace,
        skill_roots,
        mcp_path,
        inline_mcp,
        hook_sources: hooks::components(object, root),
    })
}

fn valid_legacy_metadata(object: &Map<String, Value>) -> bool {
    if object
        .get("apps")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return false;
    }
    let Some(interface) = object.get("interface").filter(|value| !value.is_null()) else {
        return true;
    };
    let Some(interface) = interface.as_object() else {
        return false;
    };
    let strings = [
        "displayName",
        "shortDescription",
        "longDescription",
        "developerName",
        "category",
        "websiteUrl",
        "websiteURL",
        "privacyPolicyUrl",
        "privacyPolicyURL",
        "termsOfServiceUrl",
        "termsOfServiceURL",
        "brandColor",
        "composerIcon",
        "logo",
        "logoDark",
    ];
    if strings.iter().any(|field| {
        interface
            .get(*field)
            .is_some_and(|value| !value.is_null() && !value.is_string())
    }) {
        return false;
    }
    for (field, alias) in [
        ("websiteUrl", "websiteURL"),
        ("privacyPolicyUrl", "privacyPolicyURL"),
        ("termsOfServiceUrl", "termsOfServiceURL"),
    ] {
        if interface.contains_key(field) && interface.contains_key(alias) {
            return false;
        }
    }
    ["capabilities", "screenshots"].iter().all(|field| {
        interface.get(*field).is_none_or(|value| {
            value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_string))
        })
    })
}

fn valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains("--")
        && !name.contains("..")
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn manifest_path(root: &Path, value: &str) -> Option<PathBuf> {
    let relative = Path::new(value.strip_prefix("./")?);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(root.join(relative))
}

pub(super) fn parse_manifest(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let Some((id, _, format)) = request
        .source
        .native_source_key
        .strip_prefix(PLUGIN_MANIFEST_PREFIX)
        .and_then(sources::plugin_manifest_parts)
    else {
        return;
    };
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let decoded = config_document::parse(bytes, ConfigDocumentFormat::Json);
    if format == "agent" && !is_agent_manifest(bytes) {
        return;
    }
    let Some(root) = sources::plugin_manifest_root(&request.source.path, format) else {
        return;
    };
    let manifest = decoded
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|object| decode_manifest_object(object, root, format == "agent"));
    let valid = manifest.is_some();
    if !valid {
        malformed(output, AgentAssetDocumentFormat::Json);
    }
    let (namespace, roots, mcp_path) = manifest
        .as_ref()
        .map(|manifest| {
            (
                manifest.namespace.clone(),
                manifest.skill_roots.clone(),
                manifest.mcp_path.clone(),
            )
        })
        .unwrap_or_default();
    if emit_plugin(
        request,
        id,
        "plugin.json",
        AgentAssetDeclaredState::Unknown,
        (
            if valid {
                AgentAssetInstallState::Installed
            } else {
                AgentAssetInstallState::Unknown
            },
            AgentAssetDeclarationRole::Definition,
        ),
        CodexAssetPayload::PluginManifest {
            namespace,
            valid,
            skill_roots: roots,
            mcp_path,
        },
        output,
    )
    .is_break()
    {
        return;
    }
    if let Some(servers) = manifest
        .as_ref()
        .and_then(|manifest| manifest.inline_mcp.as_ref())
    {
        if parse_mcp_entries(request, id, root, false, servers, output).is_break() {
            return;
        }
    }
    if let Some(manifest) = manifest {
        hooks::parse_manifest(request, &manifest, output);
    } else {
        super::super::hooks::incomplete(
            output,
            crate::models::AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
    }
}

pub(super) fn is_agent_manifest(bytes: &[u8]) -> bool {
    config_document::parse(bytes, ConfigDocumentFormat::Json)
        .and_then(|value| {
            value
                .get("$schema")
                .and_then(Value::as_str)
                .map(|schema| schema.starts_with(AGENT_SCHEMA_PREFIX))
        })
        .unwrap_or(false)
}

pub(super) fn parse_mcp_file(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let Some((id, version, agent)) = request
        .source
        .native_source_key
        .strip_prefix(PLUGIN_MCP_PREFIX)
        .and_then(sources::plugin_mcp_source_parts)
    else {
        return;
    };
    let AgentAssetSnapshot::File { bytes, .. } = request.snapshot else {
        return;
    };
    let Some(value) = config_document::parse(bytes, ConfigDocumentFormat::Json) else {
        malformed(output, AgentAssetDocumentFormat::Json);
        return;
    };
    let Some(servers) = mcp::file_servers(value, agent) else {
        malformed(output, AgentAssetDocumentFormat::Json);
        return;
    };
    let Some((name, marketplace)) = plugin_id_parts(id) else {
        return;
    };
    let root = Path::new(&request.context.config_root)
        .join("plugins/cache")
        .join(marketplace)
        .join(name)
        .join(version);
    let _ = parse_mcp_entries(request, id, &root, agent, &servers, output);
}

fn parse_mcp_entries(
    request: AgentAssetParseRequest<'_>,
    id: &str,
    root: &Path,
    agent: bool,
    servers: &Map<String, Value>,
    output: &mut dyn AgentParseOutput,
) -> ControlFlow<AgentOutputStop> {
    for (name, value) in servers {
        let Some((table, decoded)) = mcp::normalize(
            value.clone(),
            root,
            Path::new(&request.context.config_root),
            id,
            agent,
        ) else {
            malformed(output, AgentAssetDocumentFormat::Json);
            continue;
        };
        let state = table
            .get("enabled")
            .and_then(toml::Value::as_bool)
            .map_or(AgentAssetDeclaredState::Unknown, declared);
        let parent = AgentAssetNativeRef {
            category: AgentAssetCategory::Plugin,
            native_id: id.to_owned(),
            qualifier: Some(format!("plugin:{id}")),
        };
        let mut declaration = parsed_asset(
            request,
            ParsedAssetInput {
                declaration_key: &format!("mcpServers.entry:{name}"),
                resolution_group_key: name,
                category: AgentAssetCategory::Mcp,
                native_id: name,
                label: name,
                logical_origin: physical_origin(request.source),
                declared_state: state,
                trust_state: declaration_trust_for_participation(
                    request.context.trust_context,
                    AgentAssetResolutionParticipation::Participates,
                ),
                role: AgentAssetDeclarationRole::Definition,
                participation: AgentAssetResolutionParticipation::Participates,
                provided_by: Some(parent.clone()),
                action_owner: Some(parent),
                explicitly_affected: Vec::new(),
                details: AgentAssetDetails::Mcp {
                    transport: decoded.transport,
                    declared_state: state,
                    approval_state: AgentMcpApprovalState::NotRequired,
                    effective_availability: crate::models::AgentAssetEffectiveAvailability::Unknown,
                },
                facts: BTreeMap::new(),
            },
        );
        declaration.native_payload =
            AgentAssetNativePayload::CodexAsset(CodexAssetPayload::PluginMcp { table });
        output.emit_declaration(declaration)?;
    }
    ControlFlow::Continue(())
}
