//! Hook presentation uses the same complete native rules as catalog identity.
use super::{
    definition::DefinitionPayload,
    identity::CurrentDefinition,
    native::{hook_codec::selected_handler, hooks::HookNativeDefinition},
    repository::Entry,
};
use crate::models::*;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
}

// Display only: this never evaluates shell text or supplies an execution target.
fn leading_word(command: &str) -> (&str, &str) {
    let command = command.trim_start();
    if let Some(quote @ ('\'' | '"')) = command.chars().next() {
        let rest = &command[quote.len_utf8()..];
        if let Some(end) = rest.find(quote) {
            return (&rest[..end], &rest[end + quote.len_utf8()..]);
        }
    }
    let end = command.find(char::is_whitespace).unwrap_or(command.len());
    (&command[..end], &command[end..])
}

fn command_name(command: &str) -> String {
    let (program, rest) = leading_word(command);
    let basename = |value: &str| value.rsplit(['/', '\\']).next().unwrap_or(value).to_owned();
    let name = basename(program);
    if matches!(
        name.as_str(),
        "python" | "python3" | "node" | "bash" | "sh" | "zsh" | "pwsh" | "powershell"
    ) {
        let (script, _) = leading_word(rest);
        if !script.is_empty() && !script.starts_with('-') {
            return basename(script);
        }
    }
    if name.is_empty() {
        "命令 Hook".to_owned()
    } else {
        name
    }
}

pub(super) fn rule(definition: &HookNativeDefinition) -> Option<AgentCatalogHookRule> {
    let handler = selected_handler(definition).ok()?;
    let (fallback, execution) = match text(handler, "type") {
        Some("mcp_tool") => {
            let execution = format!("{}.{}", text(handler, "server")?, text(handler, "tool")?);
            (execution.clone(), execution)
        }
        Some("command") => {
            let command = if cfg!(windows) {
                text(handler, "commandWindows")
                    .or_else(|| text(handler, "command_windows"))
                    .or_else(|| text(handler, "command"))?
            } else {
                text(handler, "command")?
            };
            (command_name(command), command.to_owned())
        }
        Some("prompt") => ("提示词 Hook".to_owned(), "提示词".to_owned()),
        Some("agent") => ("Agent Hook".to_owned(), "Agent 调用".to_owned()),
        Some("http") => (
            "HTTP Hook".to_owned(),
            text(handler, "url").unwrap_or("HTTP 请求").to_owned(),
        ),
        Some(kind) => (kind.to_owned(), kind.to_owned()),
        None => return None,
    };
    Some(AgentCatalogHookRule {
        name: text(handler, "name")
            .or_else(|| text(&definition.group, "name"))
            .map(str::to_owned)
            .unwrap_or(fallback),
        event: definition.event.clone(),
        matcher: text(&definition.group, "matcher").map(str::to_owned),
        execution,
    })
}

fn file_label(scope: AgentAssetScope, path: Option<&str>) -> String {
    let scope = match scope {
        AgentAssetScope::User => "用户配置",
        AgentAssetScope::Workspace => "项目配置",
        AgentAssetScope::Local => "本地配置",
        AgentAssetScope::Managed => "受管配置",
        AgentAssetScope::System => "系统配置",
    };
    let name = path
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str());
    name.map_or_else(|| scope.to_owned(), |name| format!("{scope} · {name}"))
}

pub(super) fn project(
    item: &mut AgentCatalogAsset,
    entry: &Entry,
    observations: &BTreeMap<String, CurrentDefinition>,
    inventory: &AgentEnvironmentInventory,
    preserve_name: bool,
) {
    if item.category != AgentAssetCategory::Hook {
        return;
    }
    let mut rules = Vec::new();
    let mut add_rule = |rule| {
        if let Some(rule) = rule {
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    };
    if let Some(DefinitionPayload::Hook(variants)) = entry.current().map(|value| &value.payload) {
        for definition in variants.values() {
            add_rule(rule(definition));
        }
    }
    let mut sources = Vec::new();
    for binding in &item.bindings {
        add_rule(
            observations
                .get(&binding.id)
                .and_then(|observation| observation.hook.clone()),
        );
        let parent_id = binding.native.relationships.provided_by.as_ref().or(binding
            .native
            .relationships
            .action_owner
            .as_ref());
        let parent =
            parent_id.and_then(|id| inventory.assets.iter().find(|asset| &asset.stable_id == id));
        let path = observations
            .get(&binding.id)
            .and_then(|observation| observation.source_path.clone())
            .or_else(|| binding.native.path.clone());
        sources.push(AgentCatalogHookSource {
            binding_id: binding.id.clone(),
            label: parent
                .map(|asset| asset.label.clone())
                .unwrap_or_else(|| file_label(binding.native.scope, path.as_deref())),
            parent_native_asset_id: parent.map(|asset| asset.stable_id.clone()),
            path,
        });
    }
    for target in &item.unresolved_targets {
        let receipt = entry.receipts.get(&target.target_id);
        add_rule(
            receipt
                .and_then(|receipt| receipt.hook.as_ref())
                .and_then(|hook| rule(&hook.rule.definition)),
        );
        let path = receipt.map(|receipt| receipt.path.clone());
        sources.push(AgentCatalogHookSource {
            binding_id: target.target_id.clone(),
            label: file_label(target.scope, path.as_deref()),
            parent_native_asset_id: None,
            path,
        });
    }
    if item.ownership == AgentCatalogOwnership::Observed && !preserve_name && !rules.is_empty() {
        let mut names = Vec::new();
        for rule in &rules {
            if !names.contains(&rule.name) {
                names.push(rule.name.clone());
            }
        }
        item.name = names.join(" / ");
    }
    sources.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then(left.binding_id.cmp(&right.binding_id))
    });
    item.hook = Some(AgentCatalogHookSummary { rules, sources });
}
