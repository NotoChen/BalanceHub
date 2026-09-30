//! Pure native manifest and component decoding. No filesystem access.
use super::{clean_relative, FileRoles, SkillRole};
use crate::models::{AgentMcpTransport, AgentSkillInvocationPolicy};
use serde_json::{Map, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::services::agent_cli::grok) struct Descriptor {
    pub namespace: String,
    pub skills: Vec<String>,
    pub commands: Vec<String>,
    pub mcp_path: String,
    pub mcp_inline: Option<Value>,
    pub hooks_path: Option<String>,
    pub hooks_inline: Option<Value>,
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

pub(super) fn convention_name(directory: &str) -> Option<String> {
    let name = directory
        .chars()
        .map(|character| {
            let character = character.to_ascii_lowercase();
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    valid_name(&name).then_some(name)
}

pub(in crate::services::agent_cli::grok) fn convention(directory: &str) -> Option<Descriptor> {
    Some(Descriptor {
        namespace: convention_name(directory)?,
        skills: vec!["skills".to_owned()],
        commands: vec!["commands".to_owned()],
        mcp_path: ".mcp.json".to_owned(),
        mcp_inline: None,
        hooks_path: Some("hooks/hooks.json".to_owned()),
        hooks_inline: None,
    })
}

fn optional_string(value: Option<&Value>) -> bool {
    value.is_none_or(|value| value.is_null() || value.is_string())
}

fn paths(value: Option<&Value>, default: &str) -> Option<Vec<String>> {
    match value {
        None | Some(Value::Null) => Some(vec![default.to_owned()]),
        Some(Value::String(path)) => Some(vec![path.clone()]),
        Some(Value::Array(paths)) => paths
            .iter()
            .map(|path| path.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

pub(in crate::services::agent_cli::grok) fn decode(value: &Value) -> Option<Descriptor> {
    let object = value.as_object()?;
    let namespace = object.get("name")?.as_str()?;
    if !valid_name(namespace)
        || ![
            "version",
            "description",
            "homepage",
            "repository",
            "license",
        ]
        .iter()
        .all(|key| optional_string(object.get(*key)))
        || object.get("keywords").is_some_and(|value| {
            !value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        })
        || object.get("author").is_some_and(|author| {
            !author.is_null()
                && !author.as_object().is_some_and(|author| {
                    ["name", "email", "url"]
                        .iter()
                        .all(|key| optional_string(author.get(*key)))
                })
        })
    {
        return None;
    }
    // Optional metadata has the pinned native serde types; unknown fields are
    // forward compatible. Authorship text never determines source provenance.
    let skills = paths(object.get("skills"), "skills")?;
    let commands = paths(object.get("commands"), "commands")?;
    paths(object.get("agents"), "agents")?;
    let (mcp_path, mcp_inline) = match object.get("mcpServers") {
        Some(Value::String(path)) => (path.clone(), None),
        None | Some(Value::Null) => (".mcp.json".to_owned(), None),
        Some(value) => (".mcp.json".to_owned(), Some(value.clone())),
    };
    let (hooks_path, hooks_inline) = match object.get("hooks") {
        Some(Value::String(path)) => (Some(path.clone()), None),
        None | Some(Value::Null) => (Some("hooks/hooks.json".to_owned()), None),
        Some(value) => (None, Some(value.clone())),
    };
    Some(Descriptor {
        namespace: namespace.to_owned(),
        skills,
        commands,
        mcp_path,
        mcp_inline,
        hooks_path,
        hooks_inline,
    })
}

pub(in crate::services::agent_cli::grok) fn component_path(
    package: &Path,
    path: &str,
) -> Option<PathBuf> {
    let path = Path::new(path);
    clean_relative(if path.is_absolute() {
        path.strip_prefix(package).ok()?
    } else {
        path
    })
}

/// A no-follow file can have several native component roles. Derive all roles
/// before its first snapshot so role order never creates competing sources.
pub(super) fn file_roles(package: &Path, descriptor: &Descriptor, path: &Path) -> FileRoles {
    let mut skills = Vec::new();
    if let Some(directory) = path.parent() {
        for (command, roots) in [(false, &descriptor.skills), (true, &descriptor.commands)] {
            let mut seen = BTreeSet::new();
            for (root_index, raw) in roots.iter().enumerate() {
                let Some(root) = component_path(package, raw) else {
                    continue;
                };
                if !seen.insert(root.clone()) {
                    continue;
                }
                let Ok(relative) = directory.strip_prefix(root) else {
                    continue;
                };
                let eligible = if command {
                    relative.as_os_str().is_empty()
                        && path.extension().is_some_and(|extension| extension == "md")
                } else {
                    path.file_name().is_some_and(|name| name == "SKILL.md")
                        && relative.components().count() <= 6
                };
                if eligible {
                    skills.push(SkillRole {
                        command,
                        root_index,
                    });
                }
            }
        }
    }
    FileRoles {
        skills,
        mcp: component_path(package, &descriptor.mcp_path).as_deref() == Some(path),
        hooks: descriptor
            .hooks_path
            .as_deref()
            .and_then(|raw| component_path(package, raw))
            .as_deref()
            == Some(path),
    }
}

pub(in crate::services::agent_cli::grok) fn hook_namespace(
    namespace: &str,
    path: &Path,
    inline: bool,
) -> Option<String> {
    let stem = if inline {
        "plugin"
    } else {
        path.file_stem()?.to_str()?
    };
    Some(format!("plugin/{namespace}/{stem}"))
}

pub(super) fn skill(
    text: &str,
    basename: &str,
) -> Option<(String, String, AgentSkillInvocationPolicy)> {
    let metadata = super::super::skill::decode_metadata(text)?;
    let name = super::super::skill::normalized_name(basename)?;
    let display = metadata
        .name
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or(&name)
        .to_owned();
    Some((name, display, metadata.invocation_policy))
}

pub(super) fn mcp_entries(value: &Value, inline: bool) -> Option<&Map<String, Value>> {
    if inline && value.get("mcpServers").is_none() {
        value.as_object()
    } else {
        value.get("mcpServers")?.as_object()
    }
}

pub(super) fn mcp_transport(value: &Value) -> Option<AgentMcpTransport> {
    if !value.is_object() || !value.get("enabled").is_none_or(Value::is_boolean) {
        return None;
    }
    for field in ["startup_timeout_sec", "tool_timeout_sec"] {
        if !optional(value.get(field), |value| value.as_u64().is_some()) {
            return None;
        }
    }
    if !optional(value.get("expose_image_base64"), Value::is_boolean)
        || !optional(value.get("tool_timeouts"), |value| {
            value
                .as_object()
                .is_some_and(|values| values.values().all(|value| value.as_u64().is_some()))
        })
        || !optional(value.get("oauth"), oauth)
        || !optional(value.get("setup"), setup)
    {
        return None;
    }
    // Native Option fields accept null. The shared transport decoder predates
    // package JSON; remove only those absent-equivalent values before reuse.
    let mut transport_value = value.clone();
    let object = transport_value.as_object_mut()?;
    for field in [
        "env",
        "cwd",
        "type",
        "bearer_token_env_var",
        "headers",
        "oauth_client_id",
        "oauth_client_secret_env_var",
        "oauth_scopes",
    ] {
        if object.get(field).is_some_and(Value::is_null) {
            object.remove(field);
        }
    }
    let transport = super::super::parse::mcp_transport(&transport_value);
    (transport != AgentMcpTransport::Unknown).then_some(transport)
}

fn optional(value: Option<&Value>, valid: impl FnOnce(&Value) -> bool) -> bool {
    value.is_none_or(|value| value.is_null() || valid(value))
}

fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

fn oauth(value: &Value) -> bool {
    value.as_object().is_some_and(|value| {
        optional(value.get("clientId"), Value::is_string)
            && optional(value.get("clientSecretEnvVar"), Value::is_string)
            && optional(value.get("scopes"), strings)
            && optional(value.get("callbackPort"), |value| {
                value
                    .as_u64()
                    .is_some_and(|port| port <= u64::from(u16::MAX))
            })
    })
}

fn setup(value: &Value) -> bool {
    let Some(value) = value.as_object() else {
        return false;
    };
    let fields = value.get("fields").is_none_or(|fields| {
        fields.as_array().is_some_and(|fields| {
            fields.iter().all(|field| {
                field.as_object().is_some_and(|field| {
                    ["id", "label"]
                        .iter()
                        .all(|key| field.get(*key).is_some_and(Value::is_string))
                        && field.get("type").and_then(Value::as_str) == Some("select")
                        && field.get("required").is_none_or(Value::is_boolean)
                        && optional(field.get("default"), Value::is_string)
                        && field.get("options").is_none_or(|options| {
                            options.as_array().is_some_and(|options| {
                                options.iter().all(|option| {
                                    option.as_object().is_some_and(|option| {
                                        ["label", "value"].iter().all(|key| {
                                            option.get(*key).is_some_and(Value::is_string)
                                        })
                                    })
                                })
                            })
                        })
                })
            })
        })
    });
    let variables = value.get("variables").or_else(|| value.get("values"));
    fields
        && !(value.contains_key("variables") && value.contains_key("values"))
        && variables.is_none_or(|variables| {
            variables.as_object().is_some_and(|variables| {
                variables.values().all(|variable| {
                    variable.as_object().is_some_and(|variable| {
                        variable.get("from").is_some_and(Value::is_string)
                            && variable
                                .get("map")
                                .and_then(Value::as_object)
                                .is_some_and(|map| map.values().all(Value::is_string))
                    })
                })
            })
        })
}

pub(super) fn runtime_unobserved(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains('$') || value.contains("{{"),
        Value::Array(values) => values.iter().any(runtime_unobserved),
        Value::Object(values) => {
            values.contains_key("setup") || values.values().any(runtime_unobserved)
        }
        _ => false,
    }
}
