use super::{FoundResource, HookDefinition, HOOK_EVENTS};
use crate::models::AgentHookOwnership;
use serde_json::{json, Map, Value};
use std::path::Path;

pub(super) fn definitions(helper_path: &Path, spool_root: &Path) -> Vec<HookDefinition> {
    HOOK_EVENTS
        .iter()
        .map(|event_name| {
            let identity = format!("balancehub:codex:{}:v1", event_name.to_ascii_lowercase());
            let command = helper_command(helper_path, spool_root, &identity);
            HookDefinition {
                event_name,
                identity,
                handler: json!({"type":"command","command":command}),
            }
        })
        .collect()
}

fn helper_command(executable: &Path, spool_root: &Path, identity: &str) -> String {
    #[cfg(windows)]
    {
        format!(
            "{} --balancehub-hook-ingest --agent codex --balancehub-hook-node={} --spool-root {}",
            windows_quote(&executable.to_string_lossy()),
            identity,
            windows_quote(&spool_root.to_string_lossy())
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "{} --balancehub-hook-ingest --agent codex --balancehub-hook-node={} --spool-root {}",
            unix_quote(&executable.to_string_lossy()),
            identity,
            unix_quote(&spool_root.to_string_lossy())
        )
    }
}

pub(super) fn add_definition(root: &mut Value, definition: &HookDefinition) -> Result<(), String> {
    let object = root
        .as_object_mut()
        .ok_or_else(|| "Codex hooks.json 顶层必须是 JSON 对象".to_string())?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Codex hooks.json 的 hooks 必须是对象".to_string())?;
    let event = hooks
        .entry(definition.event_name)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| format!("Codex Hook {} 必须是数组", definition.event_name))?;
    let identity = definition.identity.as_str();
    if event.iter().any(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|handlers| {
                handlers.iter().any(|handler| {
                    handler
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|command| command.contains(identity))
                })
            })
    }) {
        let expected_group = definition_group(definition);
        let exact = event.iter().any(|group| group == &expected_group);
        if !exact {
            return Err(format!(
                "Codex Hook {} 已被修改，无法安全覆盖",
                definition.event_name
            ));
        }
        return Ok(());
    }
    event.push(definition_group(definition));
    Ok(())
}

pub(super) fn remove_owned_definitions(
    root: &mut Value,
    ownership: &AgentHookOwnership,
) -> Result<(), String> {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for resource in &ownership.resources {
        let Some(groups) = hooks
            .get_mut(&resource.event_name)
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        groups.retain(|group| {
            !(group_identity(group).as_deref() == Some(&resource.structural_identity)
                && fingerprint(group) == resource.content_fingerprint)
        });
    }
    Ok(())
}

pub(super) fn find_resources(root: &Value) -> Vec<FoundResource> {
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for event_name in HOOK_EVENTS {
        let Some(groups) = hooks.get(event_name).and_then(Value::as_array) else {
            continue;
        };
        for group in groups {
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for handler in handlers {
                let Some(identity) = handler_identity(handler) else {
                    continue;
                };
                if identity.starts_with("balancehub:codex:") {
                    found.push(FoundResource {
                        event_name: event_name.to_string(),
                        structural_identity: identity,
                        fingerprint: fingerprint(group),
                    });
                }
            }
        }
    }
    found
}

fn handler_identity(handler: &Value) -> Option<String> {
    let command = handler.get("command")?.as_str()?;
    let tail = command.split("--balancehub-hook-node=").nth(1)?;
    let value = tail.split_whitespace().next()?.trim_matches(['\'', '"']);
    (!value.is_empty()).then(|| value.to_string())
}

fn group_identity(group: &Value) -> Option<String> {
    let handlers = group.get("hooks")?.as_array()?;
    (handlers.len() == 1)
        .then(|| handler_identity(&handlers[0]))
        .flatten()
}

pub(super) fn definition_group(definition: &HookDefinition) -> Value {
    json!({"matcher":"*","hooks":[definition.handler.clone()]})
}

pub(super) fn definition_fingerprint(definition: &HookDefinition) -> String {
    fingerprint(&definition_group(definition))
}

pub(super) fn fingerprint(value: &Value) -> String {
    super::ownership::revision_for_bytes(canonical_json(value).as_bytes())
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let values = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(value)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", values.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(not(windows))]
fn unix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn windows_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}
