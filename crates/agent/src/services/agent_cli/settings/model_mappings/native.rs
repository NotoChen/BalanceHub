use super::*;

pub(super) struct Paths {
    pub model: String,
    pub display_name: Option<String>,
    pub parent: Option<String>,
}

pub(super) fn paths(kind: AgentCliKind, id: &str) -> Paths {
    let id_path = native_model_pointer(id);
    match kind {
        AgentCliKind::ClaudeCode => {
            let key = if id == "subagent" {
                "CLAUDE_CODE_SUBAGENT_MODEL".into()
            } else {
                format!("ANTHROPIC_DEFAULT_{}_MODEL", id.to_ascii_uppercase())
            };
            Paths {
                model: format!("/env/{key}"),
                display_name: (id != "subagent").then(|| format!("/env/{key}_NAME")),
                parent: None,
            }
        }
        AgentCliKind::Gemini => Paths {
            model: format!("/modelConfigs/customAliases/{id_path}/modelConfig/model"),
            display_name: None,
            parent: Some(format!("/modelConfigs/customAliases/{id_path}/extends")),
        },
        AgentCliKind::Grok => Paths {
            model: format!("/model/{id_path}/model"),
            display_name: Some(format!("/model/{id_path}/name")),
            parent: None,
        },
        AgentCliKind::Codex => unreachable!("Codex has purpose-specific model settings"),
    }
}

fn claude_alias(key: &str) -> Option<String> {
    let name = key
        .strip_prefix("ANTHROPIC_DEFAULT_")?
        .strip_suffix("_NAME")
        .unwrap_or_else(|| key.trim_start_matches("ANTHROPIC_DEFAULT_"));
    let name = name.strip_suffix("_MODEL")?;
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
    .then(|| name.to_ascii_lowercase())
}

pub(super) fn declared(kind: AgentCliKind, root: &Value) -> BTreeSet<String> {
    let pointer = match kind {
        AgentCliKind::ClaudeCode => "/env",
        AgentCliKind::Gemini => "/modelConfigs/customAliases",
        AgentCliKind::Grok => "/model",
        AgentCliKind::Codex => return BTreeSet::new(),
    };
    root.pointer(pointer)
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|items| items.keys())
        .filter_map(|key| {
            if kind == AgentCliKind::ClaudeCode {
                if key == "CLAUDE_CODE_SUBAGENT_MODEL" {
                    Some("subagent".into())
                } else {
                    claude_alias(key)
                }
            } else {
                Some(key.clone())
            }
        })
        .collect()
}

pub(super) fn ids(snapshot: &NativeSnapshot) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    if snapshot.kind == AgentCliKind::ClaudeCode {
        ids.extend(["sonnet", "opus", "haiku", "fable", "subagent"].map(String::from));
        if snapshot.include_process_environment {
            ids.extend(
                std::env::vars_os().filter_map(|(key, _)| key.to_str().and_then(claude_alias)),
            );
        }
    }
    if snapshot.kind == AgentCliKind::Gemini {
        ids.extend(gemini::aliases(snapshot).into_keys());
    }
    for layer in snapshot.layers.iter().filter(|layer| !layer.requirements) {
        if snapshot.kind != AgentCliKind::Grok || layer.role != "workspace-config" {
            ids.extend(declared(snapshot.kind, &layer.value));
        }
    }
    ids
}

fn text(value: Option<&Value>, key: &str) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(Value::String(value)) => Ok((!value.trim().is_empty()).then(|| value.clone())),
        Some(_) => Err(format!("{key} 必须是文本，请在原文中核对")),
    }
}

pub(super) fn from_root(kind: AgentCliKind, root: &Value) -> Result<Mappings, String> {
    let mut result = Mappings::new();
    for id in declared(kind, root) {
        let paths = paths(kind, &id);
        let value = AgentModelMappingValue {
            model: text(root.pointer(&paths.model), &paths.model)?,
            display_name: paths
                .display_name
                .as_ref()
                .map(|path| text(root.pointer(path), path))
                .transpose()?
                .flatten(),
            extends: paths
                .parent
                .as_ref()
                .map(|path| text(root.pointer(path), path))
                .transpose()?
                .flatten(),
        };
        if value != AgentModelMappingValue::default() {
            result.insert(id, value);
        }
    }
    Ok(result)
}

pub(super) fn effective(snapshot: &NativeSnapshot) -> Result<Mappings, String> {
    if snapshot.kind == AgentCliKind::Gemini {
        return gemini::aliases(snapshot)
            .into_iter()
            .map(|(id, parts)| {
                let mut alias = json!({});
                for (_, part) in parts {
                    super::super::apply::merge(&mut alias, part);
                }
                Ok((
                    id.clone(),
                    AgentModelMappingValue {
                        model: text(alias.pointer("/modelConfig/model"), &id)?,
                        extends: text(alias.get("extends"), &id)?,
                        display_name: None,
                    },
                ))
            })
            .collect();
    }
    ids(snapshot)
        .into_iter()
        .map(|id| {
            let paths = paths(snapshot.kind, &id);
            let get = |path: &str| {
                let observed = if snapshot.kind == AgentCliKind::ClaudeCode {
                    snapshot.env(path.trim_start_matches("/env/"), Some(path))
                } else {
                    snapshot.get(path)
                };
                text(observed.as_ref().map(|value| &value.value), path)
            };
            Ok((
                id,
                AgentModelMappingValue {
                    model: get(&paths.model)?,
                    display_name: paths
                        .display_name
                        .as_deref()
                        .map(get)
                        .transpose()?
                        .flatten(),
                    extends: None,
                },
            ))
        })
        .collect()
}

pub(super) fn resolved(
    kind: AgentCliKind,
    values: &Mappings,
    id: &str,
    main: &str,
) -> Result<Option<String>, String> {
    let mut current = id;
    let mut visited = BTreeSet::new();
    let mut model = None;
    loop {
        if !visited.insert(current) {
            return Err(format!("模型别名 {id} 存在循环继承，请调整继承配置"));
        }
        let Some(value) = values.get(current) else {
            break;
        };
        if model.is_none() {
            model = value.model.clone();
        }
        match value.extends.as_deref() {
            Some(parent) if kind == AgentCliKind::Gemini => current = parent,
            _ => break,
        }
    }
    Ok(model.or_else(|| match kind {
        AgentCliKind::ClaudeCode if id == "subagent" && !main.is_empty() => Some(main.into()),
        _ => None,
    }))
}

pub(super) fn managed(snapshot: &NativeSnapshot, id: &str) -> bool {
    let paths = paths(snapshot.kind, id);
    snapshot.layers.iter().any(|layer| {
        layer.role != "system-defaults"
            && matches!(
                layer.scope,
                AgentAssetScope::Managed | AgentAssetScope::System
            )
            && ((snapshot.kind == AgentCliKind::Gemini
                && layer.value.get("modelConfigs").is_some())
                || [&paths.model]
                    .into_iter()
                    .chain(paths.display_name.as_ref())
                    .chain(paths.parent.as_ref())
                    .any(|path| layer.value.pointer(path).is_some()))
    })
}

pub(super) fn source(snapshot: &NativeSnapshot, id: &str) -> String {
    let paths = paths(snapshot.kind, id);
    let observed = if snapshot.kind == AgentCliKind::ClaudeCode {
        snapshot.env(paths.model.trim_start_matches("/env/"), Some(&paths.model))
    } else if snapshot.kind == AgentCliKind::Gemini {
        return gemini::aliases(snapshot)
            .get(id)
            .and_then(|parts| {
                parts
                    .iter()
                    .rev()
                    .find(|(_, part)| {
                        part.pointer("/modelConfig/model").is_some()
                            || part.get("extends").is_some()
                    })
                    .or_else(|| parts.last())
            })
            .map(|(layer, _)| {
                crate::platform::paths::compatible_path(&layer.path)
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_default();
    } else {
        snapshot.get(&paths.model)
    };
    observed.map(|value| value.source).unwrap_or_default()
}
