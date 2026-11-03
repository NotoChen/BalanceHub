//! Resolve resource contents from native identities and their existing adapters.
use super::selection::{rejected, value_at, DocumentSelection};
use crate::{
    models::*,
    services::agent_cli::{
        self, catalog::projection::definition_anchor, environment::mutation::MutationInventory,
    },
};
use serde_json::Value;

pub(super) fn select(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
    root: &Value,
    format: AgentConfigurationFormat,
    facts: &mut Vec<AgentResourceContentFact>,
) -> Result<DocumentSelection, AgentConfigurationError> {
    let path = match asset.category {
        AgentAssetCategory::Mcp => {
            let declaration = definition_anchor(snapshot, asset)
                .ok_or_else(|| rejected("此 MCP 的定义来源尚未确认".to_owned()))?;
            let context = snapshot
                .inventory
                .contexts
                .iter()
                .find(|context| context.id == asset.context_id)
                .ok_or_else(|| rejected("MCP 配置范围已变化".to_owned()))?;
            let adapter = agent_cli::definition(asset.agent_kind)
                .environment
                .catalog_adapter()
                .ok_or_else(|| rejected("此 Agent 尚无 MCP 配置读取器".to_owned()))?;
            let native = adapter.declaration_value(root, declaration, context).ok();
            let path = native
                .and_then(|value| path_to(root, value, &mut Vec::new()))
                .or_else(|| plugin_mcp_path(root, declaration))
                .ok_or_else(|| rejected("未能唯一定位 MCP 条目，显示完整来源文件".to_owned()))?;
            if let Some(value) = value_at(root, &path) {
                add_string_fact(facts, "命令", value.get("command"));
                add_string_fact(facts, "地址", value.get("url"));
                add_string_fact(
                    facts,
                    "连接方式",
                    value.get("type").or_else(|| value.get("transport")),
                );
            }
            path
        }
        AgentAssetCategory::Hook => {
            let adapter = agent_cli::definition(asset.agent_kind)
                .environment
                .hook_adapter()
                .ok_or_else(|| rejected("此 Agent 尚无 Hook 配置读取器".to_owned()))?;
            let rule = (adapter.read_hook)(snapshot, asset).map_err(rejected)?;
            facts.extend(hook_facts(&rule.definition.event, &rule.definition.group));
            let mut candidates = Vec::new();
            find_hook_groups(
                root,
                &rule.anchor.event,
                rule.anchor.group_index,
                &rule.anchor.original_group,
                &mut Vec::new(),
                &mut candidates,
            );
            let [mut path] = <Vec<Vec<String>> as TryInto<[Vec<String>; 1]>>::try_into(candidates)
                .map_err(|_| rejected("Hook 所在位置不唯一，显示完整来源文件".to_owned()))?;
            path.extend(["hooks".to_owned(), rule.anchor.handler_index.to_string()]);
            value_at(root, &path)
                .filter(|value| value.is_object())
                .ok_or_else(|| rejected("Hook 执行配置已变化".to_owned()))?;
            path
        }
        _ => return Ok(DocumentSelection::Whole),
    };
    Ok(DocumentSelection::Structured { path, format })
}

pub(super) fn hook_facts(event: &str, group: &Value) -> Vec<AgentResourceContentFact> {
    let mut facts = vec![AgentResourceContentFact {
        label: "触发事件".to_owned(),
        value: event.to_owned(),
    }];
    for (key, value) in group
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != "hooks")
    {
        facts.push(AgentResourceContentFact {
            label: if key == "matcher" {
                "匹配条件".to_owned()
            } else {
                format!("规则字段 · {key}")
            },
            // Keep JSON types distinct, including null versus the string "null".
            value: value.to_string(),
        });
    }
    facts
}

pub(super) fn add_string_fact(
    facts: &mut Vec<AgentResourceContentFact>,
    label: &str,
    value: Option<&Value>,
) {
    if let Some(value) = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        facts.push(AgentResourceContentFact {
            label: label.to_owned(),
            value: value.to_owned(),
        });
    }
}

fn path_to(root: &Value, target: &Value, path: &mut Vec<String>) -> Option<Vec<String>> {
    if std::ptr::eq(root, target) {
        return Some(path.clone());
    }
    if path.len() >= 64 {
        return None;
    }
    let children: Vec<(String, &Value)> = match root {
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect(),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Vec::new(),
    };
    for (key, value) in children {
        path.push(key);
        let found = path_to(value, target, path);
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

fn plugin_mcp_path(root: &Value, declaration: &AgentAssetDeclaration) -> Option<Vec<String>> {
    declaration.provided_by.as_ref()?;
    let exact = declaration
        .declaration_key
        .strip_prefix("mcpServers.entry:");
    let claude = declaration
        .declaration_key
        .strip_prefix("plugin.mcp:")
        .and_then(|key| {
            let mut parts = key.splitn(3, ':');
            parts.next()?;
            Some((parts.next()?.parse::<usize>().ok()?, parts.next()?))
        });
    let mut candidates = Vec::new();
    let mut tables = vec![(Vec::new(), root)];
    if let Some(servers) = root.get("mcpServers") {
        if let Some(entries) = servers.as_array() {
            if let Some((ordinal, _)) = claude {
                if let Some(value) = ordinal
                    .checked_sub(1)
                    .and_then(|index| entries.get(index).map(|value| (index, value)))
                {
                    tables.push((vec!["mcpServers".to_owned(), value.0.to_string()], value.1));
                }
            }
        } else {
            tables.push((vec!["mcpServers".to_owned()], servers));
            if declaration.declaration_key == format!("mcpServers:{}", declaration.native_id) {
                if let Some(nested) = servers.get("mcpServers") {
                    tables.push((
                        vec!["mcpServers".to_owned(), "mcpServers".to_owned()],
                        nested,
                    ));
                }
            }
        }
    }
    for (prefix, table) in tables {
        for (name, value) in table.as_object().into_iter().flatten() {
            let matches = exact.map_or_else(
                || claude.map_or(declaration.native_id == *name, |(_, key)| key == name),
                |key| key == name,
            );
            if matches && value.is_object() {
                let mut path = prefix.clone();
                path.push(name.clone());
                candidates.push(path);
            }
        }
    }
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn find_hook_groups(
    root: &Value,
    event: &str,
    index: usize,
    group: &Value,
    path: &mut Vec<String>,
    found: &mut Vec<Vec<String>>,
) {
    if path.len() > 32 || found.len() > 1 {
        return;
    }
    match root {
        Value::Object(values) => {
            for (key, value) in values {
                path.push(key.clone());
                if key == event
                    && value.as_array().and_then(|values| values.get(index)) == Some(group)
                {
                    let mut selected = path.clone();
                    selected.push(index.to_string());
                    found.push(selected);
                }
                find_hook_groups(value, event, index, group, path, found);
                path.pop();
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                path.push(index.to_string());
                find_hook_groups(value, event, index, group, path, found);
                path.pop();
            }
        }
        _ => {}
    }
}
