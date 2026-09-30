//! Local legacy plugin Hooks in Codex rust-v0.154.0. AgentPlugin format uses
//! executor-provided hooks and deliberately contributes no local Hook source.
use super::{sources, Manifest};
use crate::models::*;
use crate::services::agent_cli::{
    codex::environment::hooks::{
        self as native_hooks, CodexHookDefinitionEvidence, CodexHookPayload, CodexPluginHookBinding,
    },
    contracts::{
        AgentAssetNativePayload, AgentAssetParseRequest, AgentAssetSnapshot, AgentParseOutput,
        InitialSourceOutput,
    },
    environment::{
        config_document::{self, ConfigDocumentFormat},
        parsed_asset, physical_origin, ParsedAssetInput,
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub(in crate::services::agent_cli::codex::environment) const SOURCE_PREFIX: &str =
    "codex-plugin-hook:";
pub(in crate::services::agent_cli::codex::environment) const MANIFEST_KEY: &str =
    "codex-plugin-hook-manifest";

#[derive(Clone, PartialEq, Eq)]
pub(in crate::services::agent_cli::codex) enum Component {
    File(PathBuf),
    Inline(Value),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct HookManifest {
    components: Vec<Component>,
}

pub(super) fn components(object: &Map<String, Value>, root: &Path) -> Vec<Component> {
    let valid_inline = |value: &Value| {
        value
            .as_object()
            .is_some_and(|root| native_hooks::valid_document(root, true))
    };
    let components = match object.get("hooks") {
        Some(Value::String(path)) => super::manifest_path(root, path)
            .map(Component::File)
            .into_iter()
            .collect(),
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => values
            .iter()
            .filter_map(|value| super::manifest_path(root, value.as_str()?))
            .map(Component::File)
            .collect(),
        Some(value @ Value::Object(_)) if valid_inline(value) => {
            vec![Component::Inline(value.clone())]
        }
        Some(Value::Array(values)) if values.iter().all(valid_inline) => {
            values.iter().cloned().map(Component::Inline).collect()
        }
        _ => Vec::new(),
    };
    if components.is_empty() {
        vec![Component::File(root.join("hooks/hooks.json"))]
    } else {
        components
    }
}

pub(in crate::services::agent_cli::codex) fn manifest_components(
    value: &Value,
    root: &Path,
    format: &str,
) -> Option<Vec<Component>> {
    super::decode_manifest_object(value.as_object()?, root, format == "agent")
        .map(|manifest| manifest.hook_sources)
}

pub(in crate::services::agent_cli::codex::environment) enum HookComponentLocation {
    File { path: PathBuf, source_key: String },
    Manifest,
}

pub(in crate::services::agent_cli::codex::environment) struct SelectedHookComponent {
    pub key_source: String,
    pub location: HookComponentLocation,
}

pub(in crate::services::agent_cli::codex::environment) fn select_component(
    payload: &CodexHookPayload,
    root: &Path,
    evidence: &CodexHookDefinitionEvidence,
) -> Option<SelectedHookComponent> {
    let CodexHookPayload::PluginManifest { manifest } = payload else {
        return None;
    };
    let binding = evidence.plugin.as_ref()?;
    let (id, version, format) = binding
        .manifest_key
        .strip_prefix(super::PLUGIN_MANIFEST_PREFIX)
        .and_then(sources::plugin_manifest_parts)?;
    let components = &manifest.components;
    let component = components.get(binding.component_index)?;
    let location = match component {
        Component::File(path) => {
            if components
                .iter()
                .filter(|candidate| *candidate == component)
                .count()
                != 1
            {
                return None;
            }
            HookComponentLocation::File {
                path: path.clone(),
                source_key: source_key(id, version, format, binding.component_index),
            }
        }
        Component::Inline(value) => {
            if value
                .get("hooks")
                .and_then(|events| events.get(&evidence.event))
                .and_then(|groups| groups.get(evidence.group_index))
                != Some(&evidence.group)
            {
                return None;
            }
            HookComponentLocation::Manifest
        }
    };
    Some(SelectedHookComponent {
        key_source: key_source(id, root, component, binding.component_index)?,
        location,
    })
}

pub(in crate::services::agent_cli::codex::environment) fn source_parts(
    key: &str,
) -> Option<(&str, &str, &str, usize)> {
    let mut parts = key.strip_prefix(SOURCE_PREFIX)?.split(':');
    let (id, version, format, index) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?.parse().ok()?,
    );
    (parts.next().is_none()
        && super::plugin_id_parts(id).is_some()
        && super::valid_version(version)
        && matches!(format, "codex" | "claude" | "cursor"))
    .then_some((id, version, format, index))
}

pub(in crate::services::agent_cli::codex::environment) fn source_key(
    id: &str,
    version: &str,
    format: &str,
    index: usize,
) -> String {
    format!("{SOURCE_PREFIX}{id}:{version}:{format}:{index}")
}

pub(in crate::services::agent_cli::codex) fn key_source(
    id: &str,
    root: &Path,
    component: &Component,
    index: usize,
) -> Option<String> {
    let relative = match component {
        Component::Inline(_) => format!("plugin.json#hooks[{index}]"),
        Component::File(path) => path
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    };
    Some(format!("{id}:{relative}"))
}

pub(in crate::services::agent_cli::codex) fn package_root(
    config_root: &Path,
    id: &str,
    path: &Path,
) -> Option<PathBuf> {
    let (name, marketplace) = super::plugin_id_parts(id)?;
    let base = config_root
        .join("plugins/cache")
        .join(marketplace)
        .join(name);
    let relative = path.strip_prefix(&base).ok()?;
    let version = relative.components().next()?.as_os_str().to_str()?;
    super::valid_version(version).then(|| base.join(version))
}

pub(in crate::services::agent_cli::codex::environment::native) fn discover(
    root: &Path,
    id: &str,
    version: &str,
    format: &str,
    manifest_path: &Path,
    manifest: &Manifest,
    output: &mut dyn InitialSourceOutput,
) -> bool {
    let mut physical = BTreeSet::new();
    for (index, component) in manifest.hook_sources.iter().enumerate() {
        let Component::File(path) = component else {
            continue;
        };
        // A repeated path repeats native keys. Keep one read and disclose the
        // ambiguous cardinality; never silently merge two native identities.
        if !physical.insert(path) {
            continue;
        }
        if manifest
            .hook_sources
            .iter()
            .filter(|candidate| *candidate == component)
            .count()
            != 1
            || path == manifest_path
            || manifest.mcp_path.as_ref() == Some(path)
        {
            native_hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
            continue;
        }
        if output
            .emit_initial(sources::plugin_source(
                root,
                &source_key(id, version, format, index),
                path.clone(),
                AgentAssetSourceKind::File,
                &[AgentAssetCategory::Hook],
            ))
            .is_break()
        {
            return false;
        }
    }
    true
}

pub(super) fn parse_manifest(
    request: AgentAssetParseRequest<'_>,
    manifest: &Manifest,
    output: &mut dyn AgentParseOutput,
) {
    let Some((id, _, format)) = request
        .source
        .native_source_key
        .strip_prefix(super::PLUGIN_MANIFEST_PREFIX)
        .and_then(sources::plugin_manifest_parts)
    else {
        return;
    };
    if format == "agent" {
        return;
    }
    let Some(root) = sources::plugin_manifest_root(&request.source.path, format) else {
        return;
    };
    let mut observation = parsed_asset(
        request,
        ParsedAssetInput {
            declaration_key: MANIFEST_KEY,
            resolution_group_key: MANIFEST_KEY,
            native_id: MANIFEST_KEY,
            category: AgentAssetCategory::Hook,
            label: "Codex Plugin Hook 来源",
            logical_origin: physical_origin(request.source),
            declared_state: AgentAssetDeclaredState::Unknown,
            trust_state: AgentTrustState::Unknown,
            role: AgentAssetDeclarationRole::StateOverlay,
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
    observation.native_payload =
        AgentAssetNativePayload::CodexHook(CodexHookPayload::PluginManifest {
            manifest: HookManifest {
                components: manifest.hook_sources.clone(),
            },
        });
    if output.emit_declaration(observation).is_break() {
        return;
    }
    for (index, component) in manifest.hook_sources.iter().enumerate() {
        let Component::Inline(value) = component else {
            continue;
        };
        let Some(key_source) = key_source(id, root, component, index) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        if !native_hooks::parse_with_origin(
            request,
            object,
            true,
            &key_source,
            Some(CodexPluginHookBinding {
                plugin_id: id.to_owned(),
                manifest_key: request.source.native_source_key.clone(),
                component_index: index,
            }),
            output,
        ) {
            return;
        }
    }
}

pub(in crate::services::agent_cli::codex::environment) fn parse_file(
    request: AgentAssetParseRequest<'_>,
    output: &mut dyn AgentParseOutput,
) {
    let Some((id, version, format, index)) = source_parts(&request.source.native_source_key) else {
        return;
    };
    let Some((name, marketplace)) = super::plugin_id_parts(id) else {
        return;
    };
    let root = Path::new(&request.context.config_root)
        .join("plugins/cache")
        .join(marketplace)
        .join(name)
        .join(version);
    let component = Component::File(request.source.path.clone());
    let Some(key_source) = key_source(id, &root, &component, index) else {
        return;
    };
    let bytes = match request.snapshot {
        AgentAssetSnapshot::File { bytes, .. } => bytes,
        AgentAssetSnapshot::Missing { .. } => return,
        _ => {
            native_hooks::incomplete(
                output,
                AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
            );
            return;
        }
    };
    let value = config_document::parse(bytes, ConfigDocumentFormat::Json);
    let Some(object) = value.as_ref().and_then(Value::as_object) else {
        native_hooks::incomplete(
            output,
            AgentAssetDiscoveryIncompleteReason::SourceUnavailable,
        );
        return;
    };
    native_hooks::parse_with_origin(
        request,
        object,
        true,
        &key_source,
        Some(CodexPluginHookBinding {
            plugin_id: id.to_owned(),
            manifest_key: format!("{}{id}:{version}:{format}", super::PLUGIN_MANIFEST_PREFIX),
            component_index: index,
        }),
        output,
    );
}

pub(in crate::services::agent_cli::codex) fn builtin(
    id: &str,
    event: &str,
    group: &Value,
    handler: &Value,
) -> bool {
    let server = match id {
        "browser@openai-bundled"
        | "chrome@openai-bundled"
        | "chrome-dev@openai-bundled"
        | "chrome-internal@openai-bundled"
        | "computer-use@openai-bundled" => "node_repl",
        "unified-computer-use@openai-bundled" => "cua_repl",
        _ => return false,
    };
    matches!(event, "Stop" | "Interrupt" | "SubagentStop")
        && group.get("matcher").is_none_or(Value::is_null)
        && handler.get("type").and_then(Value::as_str) == Some("mcp_tool")
        && handler.get("server").and_then(Value::as_str) == Some(server)
        && handler.get("tool").and_then(Value::as_str) == Some("turn_ended")
}
