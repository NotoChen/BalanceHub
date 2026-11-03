//! Local registered packages. Native identity and decoding stay in this adapter.
use crate::models::{
    AgentAssetCategory, AgentAssetDiagnostic, AgentAssetDiscoveryIncompleteReason,
    AgentAssetDocumentFormat, AgentAssetNativeRef, AgentAssetScope, AgentAssetSourceKind,
    AgentConfigurationContext,
};
use crate::services::agent_cli::contracts::{
    AgentAssetDirectoryEntry, AgentAssetLogicalOrigin, AgentAssetSnapshot, AgentDiagnosticOutput,
};
use crate::services::agent_cli::environment::{bounded_object_entries, parse_json, stable_id};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Component, Path, PathBuf};

mod discovery;
pub(in crate::services::agent_cli::claude) mod hooks;
mod parse;
mod source;
pub(super) use discovery::discover;
pub(super) use parse::{parse_source, skill_name, transport};
pub(super) use source::{physical_key, source_bindings, source_spec, SourceBinding, SourceRole};

pub(super) const PREFIX: &str = "claude-plugin:";

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct PackageKey {
    pub(super) id: String,
    pub(super) occurrence: String,
}

#[derive(Clone)]
pub(crate) struct RegisteredPlugin {
    pub(super) key: PackageKey,
    pub(super) path: PathBuf,
    pub(super) origin: AgentAssetLogicalOrigin,
    pub(super) version: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) enum McpComponent {
    File(PathBuf),
    Inline(Map<String, Value>),
    Unobserved,
    Invalid,
}

#[derive(Clone, PartialEq)]
pub(crate) struct Manifest {
    pub(super) namespace: String,
    pub(super) skills: Option<Vec<PathBuf>>,
    pub(super) commands: Option<Vec<PathBuf>>,
    pub(super) mcp: Vec<McpComponent>,
    pub(super) hooks: Vec<hooks::HookComponent>,
    pub(super) unsupported: Vec<&'static str>,
}

#[derive(Clone)]
pub(crate) enum ClaudePluginPayload {
    Registry(RegisteredPlugin),
    Root {
        key: PackageKey,
        entries: Option<Vec<AgentAssetDirectoryEntry>>,
        missing: bool,
    },
    Manifest {
        key: PackageKey,
        value: Option<Manifest>,
    },
    Skill {
        key: PackageKey,
        namespace: String,
        root_index: usize,
        mode: String,
        frontmatter_name: Option<String>,
        valid: bool,
    },
    Hook {
        key: PackageKey,
        namespace: String,
        ordinal: usize,
        definition: hooks::HookRule,
    },
    HookSource {
        key: PackageKey,
        ordinal: usize,
        document: Option<hooks::HookDocument>,
        missing: bool,
    },
    Mcp {
        key: PackageKey,
        namespace: String,
        ordinal: usize,
        value: Value,
        definition: crate::services::agent_cli::contracts::AgentMcpDefinitionPayload,
    },
}

pub(super) fn malformed(output: &mut dyn AgentDiagnosticOutput, field: &str) {
    output.emit_diagnostic(AgentAssetDiagnostic::Malformed {
        format: AgentAssetDocumentFormat::Manifest,
        location: Some(format!("plugin.{field}")),
    });
}

pub(super) fn unobserved(output: &mut dyn AgentDiagnosticOutput, field: &str) {
    let category = if field.starts_with("mcpServers") {
        AgentAssetCategory::Mcp
    } else if field.starts_with("skills") || field.starts_with("commands") {
        AgentAssetCategory::Skill
    } else if field.starts_with("hooks") {
        AgentAssetCategory::Hook
    } else {
        AgentAssetCategory::Plugin
    };
    let reason = if field.ends_with("symbolicLink") {
        AgentAssetDiscoveryIncompleteReason::SourceUnavailable
    } else if field.ends_with("runtimeExpansion") || field == "dependencyEnablement" {
        AgentAssetDiscoveryIncompleteReason::RuntimeStateUnobserved
    } else if field == "registry.ambiguousInstallation" {
        AgentAssetDiscoveryIncompleteReason::InstallationUnverified
    } else {
        AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint
    };
    output.emit_diagnostic(AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind: super::super::AGENT_KIND,
        category,
        reason,
    });
}

pub(super) fn json(
    snapshot: &AgentAssetSnapshot,
    output: &mut dyn AgentDiagnosticOutput,
) -> Option<Value> {
    let AgentAssetSnapshot::File { bytes, .. } = snapshot else {
        return None;
    };
    parse_json(bytes, None, output).ok()
}

pub(super) fn registry_entries(
    context: &AgentConfigurationContext,
    workspace_lexical: Option<&Path>,
    root: &Map<String, Value>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<RegisteredPlugin> {
    if root.get("version").and_then(Value::as_u64) != Some(2) {
        malformed(output, "registry.version");
        return Vec::new();
    }
    let Some(plugins) = root.get("plugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for (id, value) in bounded_object_entries(plugins) {
        let Some(entries) = value.as_array() else {
            malformed(output, "registry.entries");
            continue;
        };
        for value in entries {
            let Some(entry) = value.as_object() else {
                malformed(output, "registry.entry");
                continue;
            };
            let scope = entry.get("scope").and_then(Value::as_str);
            let origin = match scope {
                Some("managed") => AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::Managed,
                    precedence: 40,
                },
                Some("user") => AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 10,
                },
                Some("project") => AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::Workspace,
                    precedence: 20,
                },
                Some("local") => AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::Local,
                    precedence: 30,
                },
                _ => {
                    malformed(output, "registry.scope");
                    continue;
                }
            };
            let Some(path) = entry
                .get("installPath")
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.trim().is_empty()
                        && Path::new(value).is_absolute()
                        && !Path::new(value)
                            .components()
                            .any(|part| part == Component::ParentDir)
                })
            else {
                malformed(output, "registry.installPath");
                continue;
            };
            let project = match entry.get("projectPath") {
                Some(Value::String(value)) if !value.trim().is_empty() => Some(value.as_str()),
                None if !matches!(scope, Some("project" | "local")) => None,
                _ => {
                    malformed(output, "registry.projectPath");
                    continue;
                }
            };
            if matches!(scope, Some("project" | "local"))
                && !project.is_some_and(|project| {
                    context.workspace_id.as_deref() == Some(project)
                        || workspace_lexical.is_some_and(|path| path.to_string_lossy() == project)
                })
            {
                continue;
            }
            result.push(RegisteredPlugin {
                key: PackageKey {
                    id: id.to_owned(),
                    occurrence: stable_id(
                        "plugin-entry",
                        &[
                            id,
                            scope.unwrap_or_default(),
                            path,
                            project.unwrap_or_default(),
                        ],
                    ),
                },
                path: PathBuf::from(path),
                origin,
                version: entry
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
    }
    result
}

pub(super) fn parent_ref(key: &PackageKey) -> AgentAssetNativeRef {
    AgentAssetNativeRef {
        category: AgentAssetCategory::Plugin,
        native_id: key.id.clone(),
        qualifier: Some(format!("plugin:{}", key.id)),
    }
}

pub(super) fn relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    ((value == "." || value.starts_with("./"))
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)))
    .then(|| path.to_path_buf())
}

fn paths(value: &Value) -> Option<Vec<PathBuf>> {
    match value {
        Value::String(value) => Some(vec![relative_path(value)?]),
        Value::Array(values) => values
            .iter()
            .map(|value| relative_path(value.as_str()?))
            .collect(),
        _ => None,
    }
}

pub(super) fn decode_manifest(value: Option<&Value>, key: &PackageKey) -> Option<Manifest> {
    let fallback = key
        .id
        .rsplit_once('@')
        .map_or(key.id.as_str(), |(name, _)| name);
    let mut manifest = Manifest {
        namespace: fallback.to_owned(),
        skills: None,
        commands: None,
        mcp: Vec::new(),
        hooks: Vec::new(),
        unsupported: Vec::new(),
    };
    let Some(value) = value else {
        return valid_namespace(fallback).then_some(manifest);
    };
    let root = value.as_object()?;
    let name = root.get("name")?.as_str()?;
    if !valid_namespace(name) {
        return None;
    }
    manifest.namespace = name.to_owned();
    if let Some(value) = root.get("skills") {
        manifest.skills = Some(paths(value)?);
    }
    if let Some(value) = root.get("commands") {
        if value.is_object() {
            manifest.commands = Some(Vec::new());
            manifest.unsupported.push("commands.inline");
        } else {
            manifest.commands = Some(paths(value)?);
        }
    }
    if root.contains_key("dependencies") || root.contains_key("defaultEnabled") {
        manifest.unsupported.push("dependencyEnablement");
    }
    if let Some(value) = root.get("mcpServers") {
        let values = value
            .as_array()
            .map_or_else(|| vec![value], |values| values.iter().collect());
        for value in values {
            manifest.mcp.push(match value {
                Value::Object(object) => match object.get("mcpServers") {
                    Some(Value::Object(servers)) => McpComponent::Inline(servers.clone()),
                    Some(_) => McpComponent::Invalid,
                    None => McpComponent::Inline(object.clone()),
                },
                Value::String(path)
                    if path.ends_with(".mcpb")
                        || path.ends_with(".dxt")
                        || path.contains("://") =>
                {
                    manifest.unsupported.push("mcpServers.bundleOrRemote");
                    McpComponent::Unobserved
                }
                Value::String(path) => relative_path(path)
                    .map(McpComponent::File)
                    .unwrap_or(McpComponent::Invalid),
                _ => McpComponent::Invalid,
            });
        }
    }
    manifest.hooks = hooks::components(root.get("hooks"));
    Some(manifest)
}

fn valid_namespace(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(' ')
        && !name.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub(super) fn ordinary(
    entries: &[AgentAssetDirectoryEntry],
    name: &str,
    kind: AgentAssetSourceKind,
) -> bool {
    entries
        .iter()
        .any(|entry| entry.name == name && entry.source_kind == kind && !entry.is_symlink)
}

pub(super) fn skill_roots(
    root: &Path,
    entries: &[AgentAssetDirectoryEntry],
    manifest: &Manifest,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let default = ordinary(entries, "skills", AgentAssetSourceKind::Directory);
    if default {
        roots.push(root.join("skills"));
    }
    if let Some(paths) = &manifest.skills {
        for path in paths {
            let path = root.join(path);
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    } else if !default && ordinary(entries, "SKILL.md", AgentAssetSourceKind::File) {
        roots.push(root.to_path_buf());
    }
    roots
}

pub(super) fn normalize_skill(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

pub(super) fn runtime_expansion(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains("${"),
        Value::Array(values) => values.iter().any(runtime_expansion),
        Value::Object(values) => values.values().any(runtime_expansion),
        _ => false,
    }
}
