//! Resolve declared primary-chat model settings in Gemini's native order:
//! alias depth, match specificity, declaration order. Runtime-only defaults
//! remain unknown when they are not present in the readable configuration.
//! Reference: google-gemini/gemini-cli, packages/core/src/services/modelConfigService.ts.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::services::agent_cli::settings) type Aliases<'a> =
    BTreeMap<String, Vec<(&'a sources::Layer, &'a Value)>>;

pub(in crate::services::agent_cli::settings) fn aliases(snapshot: &NativeSnapshot) -> Aliases<'_> {
    let mut result = BTreeMap::new();
    for key in ["aliases", "customAliases"] {
        let mut group: Aliases<'_> = BTreeMap::new();
        for layer in snapshot.layers.iter().filter(|layer| !layer.requirements) {
            if let Some(items) = layer
                .value
                .pointer(&format!("/modelConfigs/{key}"))
                .and_then(Value::as_object)
            {
                for (name, alias) in items {
                    group.entry(name.clone()).or_default().push((layer, alias));
                }
            }
        }
        // A custom alias replaces the built-in alias with that name.
        result.extend(group);
    }
    result
}

struct Resolved {
    value: Value,
    sources: BTreeSet<String>,
    managed: bool,
}

impl Resolved {
    fn apply(&mut self, layer: &sources::Layer, config: &Value, field: &str) {
        let Some(value) = config
            .get("generateContentConfig")
            .filter(|value| value.is_object())
        else {
            return;
        };
        super::super::apply::merge(&mut self.value, value);
        let mut cursor = value;
        let mut affected = true;
        for part in field.split('.') {
            if !cursor.is_object() {
                break;
            }
            let Some(next) = cursor.get(part) else {
                affected = false;
                break;
            };
            cursor = next;
        }
        if affected {
            self.sources.insert(
                crate::platform::paths::compatible_path(&layer.path)
                    .to_string_lossy()
                    .into_owned(),
            );
            self.managed |= layer.role != "system-defaults"
                && matches!(
                    layer.scope,
                    AgentAssetScope::Managed | AgentAssetScope::System
                );
        }
    }
}

pub(in crate::services::agent_cli::settings) fn read(
    snapshot: &NativeSnapshot,
    model: &str,
    field: &str,
) -> Option<Observed> {
    if model.is_empty() {
        return None;
    }
    let aliases = aliases(snapshot);
    let named_alias = aliases.contains_key(model);
    let mut current = if named_alias {
        Some(model)
    } else {
        aliases.contains_key("chat-base").then_some("chat-base")
    };
    let mut chain = Vec::new();
    let mut visited = BTreeSet::new();
    while let Some(name) = current {
        if visited.len() >= 100 || !visited.insert(name) {
            return None;
        }
        chain.push(name);
        current = aliases
            .get(name)
            .and_then(|parts| {
                parts
                    .iter()
                    .rev()
                    .find_map(|(_, alias)| alias.get("extends"))
            })
            .and_then(Value::as_str);
    }
    chain.reverse();
    let mut resolved = Resolved {
        value: json!({}),
        sources: BTreeSet::new(),
        managed: false,
    };
    let mut base_model = None;
    for name in &chain {
        if let Some(parts) = aliases.get(*name) {
            for (layer, alias) in parts {
                if let Some(config) = alias.get("modelConfig") {
                    base_model = config.get("model").and_then(Value::as_str).or(base_model);
                    resolved.apply(layer, config, field);
                }
            }
        }
    }
    if !named_alias {
        chain.push(model);
        base_model = Some(model);
    }
    let mut levels = BTreeMap::new();
    if let Some(model) = base_model {
        levels.insert(model, 0);
    }
    for (index, name) in chain.iter().enumerate() {
        levels.insert(*name, index + 1);
    }
    let mut overrides = Vec::new();
    for key in ["overrides", "customOverrides"] {
        for layer in snapshot.layers.iter().filter(|layer| !layer.requirements) {
            let Some(items) = layer
                .value
                .pointer(&format!("/modelConfigs/{key}"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for item in items {
                let Some(criteria) = item
                    .get("match")
                    .and_then(Value::as_object)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let mut level = 0;
                let matches = criteria.iter().all(|(key, value)| match key.as_str() {
                    "model" => value
                        .as_str()
                        .and_then(|model| levels.get(model))
                        .is_some_and(|matched| {
                            level = *matched;
                            true
                        }),
                    "isChatModel" => value == &json!(true),
                    "overrideScope" => value.as_str() == Some("core"),
                    // Retry and subagent-specific values are not the baseline
                    // for the primary conversation shown by this editor.
                    _ => false,
                });
                if matches {
                    if let Some(config) = item.get("modelConfig") {
                        overrides.push((level, criteria.len(), overrides.len(), layer, config));
                    }
                }
            }
        }
    }
    overrides.sort_by_key(|(level, specificity, index, _, _)| (*level, *specificity, *index));
    for (_, _, _, layer, config) in overrides {
        resolved.apply(layer, config, field);
    }
    let value = resolved
        .value
        .pointer(&format!("/{}", field.replace('.', "/")))?
        .clone();
    (!value.is_null()).then(|| Observed {
        value,
        source: resolved.sources.into_iter().collect::<Vec<_>>().join("\n"),
        managed: resolved.managed,
        state: AgentSettingValueState::Configured,
    })
}
