//! Codex native Hook JSON/inline TOML and persisted user enablement. Trust is
//! preserved byte-for-byte as policy data, never granted by management actions.
use super::environment::hooks;
use crate::{
    models::*,
    services::agent_cli::{
        catalog::native::{
            hook_codec::{self, HookDocument, HookDocumentFormat, HookLocation, HookSlot},
            hooks::*,
        },
        environment::mutation::MutationInventory,
        native_agent_kinds::codex::AGENT_KIND,
    },
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod plugin;
#[cfg(test)]
mod tests;

pub(super) static ADAPTER: NativeHookAdapter = NativeHookAdapter {
    switch_mode: HookNativeSwitchMode::Native,
    agent_kind: AGENT_KIND,
    parser_version: super::environment::PARSER_VERSION,
    default_role: "hooks",
    read_hook,
    hook_targets,
    inspect_source,
    validate_definition,
    validate_adoption,
    prepare_hook_edits,
};

fn role(
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Option<(&'static str, HookDocumentFormat)> {
    let root = Path::new(&context.config_root);
    let mut roles = vec![
        (
            "hooks",
            AgentAssetScope::User,
            root.join("hooks.json"),
            HookDocumentFormat::Json,
        ),
        (
            "config",
            AgentAssetScope::User,
            root.join("config.toml"),
            HookDocumentFormat::Toml,
        ),
    ];
    if let Some(workspace) = context.workspace_id.as_deref() {
        let root = Path::new(workspace).join(".codex");
        roles.extend([
            (
                "workspace-hooks",
                AgentAssetScope::Workspace,
                root.join("hooks.json"),
                HookDocumentFormat::Json,
            ),
            (
                "workspace-config",
                AgentAssetScope::Workspace,
                root.join("config.toml"),
                HookDocumentFormat::Toml,
            ),
        ]);
    }
    #[cfg(unix)]
    roles.push((
        "system-config",
        AgentAssetScope::System,
        super::environment::system_paths::system_root().join("config.toml"),
        HookDocumentFormat::Toml,
    ));
    roles.into_iter().find_map(|(role, scope, path, format)| {
        hook_codec::exact_source(
            source,
            context,
            scope,
            &path,
            AgentAssetInstallationOrigin::ConfigEntry,
        )
        .then_some((role, format))
    })
}

fn hook_targets(
    inventory: &AgentEnvironmentInventory,
    context: &AgentConfigurationContext,
) -> Vec<HookNativeDestination> {
    inventory
        .sources
        .iter()
        .filter_map(|source| {
            role(source, context).and_then(|(role, _)| hook_codec::destination(source, role))
        })
        .collect()
}

fn policy_source<'a>(
    snapshot: &'a MutationInventory,
    context: &AgentConfigurationContext,
) -> Result<&'a AgentAssetSource, String> {
    let candidates = snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| role(source, context).is_some_and(|(role, _)| role == "config"))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [source] => Ok(*source),
        _ => Err("Codex 用户 Hook 状态来源缺失或不唯一".to_owned()),
    }
}

fn read_hook(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
) -> Result<HookNativeRule, String> {
    hook_codec::read_rule(snapshot, asset, inspect_source)
}

fn inspect_source(
    snapshot: &MutationInventory,
    source_id: &str,
) -> Result<Vec<HookNativeRule>, String> {
    let source = hook_codec::source(snapshot, source_id)?;
    let context = hook_codec::context(snapshot, source)?;
    let Some((_, format)) = role(source, context) else {
        return plugin::rules(snapshot, source, context);
    };
    let document = HookDocument::observe(snapshot, source_id, format)?;
    let root = document
        .root
        .as_object()
        .ok_or("Codex Hook 配置根节点无效")?;
    if !hooks::valid_document(root, matches!(format, HookDocumentFormat::Json)) {
        return Err("Codex Hook 配置不符合固定原生 schema".to_owned());
    }
    let mut slots = Vec::new();
    let complete = hooks::walk_slots(root, |event, group, handler| {
        slots.push(HookSlot {
            native_id: hooks::native_id(&source.path, event, group, handler),
            event: event.to_owned(),
            group_index: group,
            handler_index: handler,
        });
        true
    });
    if !complete {
        return Err("Codex Hook 配置含原生不加载的 handler 或 matcher".to_owned());
    }
    let mut rules = hook_codec::rules_from_slots(
        snapshot,
        source,
        &document.root,
        slots,
        AgentAssetDeclaredState::Enabled,
    )?;
    if !matches!(
        source.scope,
        AgentAssetScope::System | AgentAssetScope::Managed
    ) {
        let policy = policy_source(snapshot, context)?;
        let policy = HookDocument::observe(snapshot, &policy.id, HookDocumentFormat::Toml)?;
        let states = hooks::decode_states(&policy.root);
        for rule in &mut rules {
            let key = hooks::native_id(
                &source.path,
                &rule.anchor.event,
                rule.anchor.group_index,
                rule.anchor.handler_index,
            );
            if states.get(&key).and_then(|state| state.enabled) == Some(false) {
                rule.enabled = AgentAssetDeclaredState::Disabled;
            }
        }
    }
    Ok(rules)
}

fn validate_definition(definition: &HookNativeDefinition) -> Result<(), String> {
    let handler = hook_codec::selected_handler(definition)?;
    if !hooks::valid_event(&definition.event)
        || !definition
            .group
            .as_object()
            .is_some_and(hooks::valid_group_metadata)
        || !hooks::valid_handler(handler)
        || !hooks::valid_matcher(&definition.event, &definition.group)
        || (definition.event == "SessionEnd"
            && handler.get("type").and_then(Value::as_str) == Some("mcp_tool"))
    {
        return Err("Hook 不符合 Codex rust-v0.154.0 可加载的原生 schema".to_owned());
    }
    Ok(())
}

fn validate_adoption(rule: &HookNativeRule) -> Result<(), String> {
    validate_definition(&rule.definition)?;
    let text = serde_json::to_string(&rule.definition.group).map_err(|_| "Hook 定义无效")?;
    if [
        "PLUGIN_ROOT",
        "PLUGIN_DATA",
        "CLAUDE_PLUGIN_ROOT",
        "CLAUDE_PLUGIN_DATA",
    ]
    .iter()
    .any(|name| text.contains(name))
    {
        return Err(
            "此 Hook 依赖所属 Plugin 的运行环境，需先将共享定义中的依赖改为独立配置".to_owned(),
        );
    }
    Ok(())
}

fn state_table(root: &mut Value) -> Result<&mut Map<String, Value>, String> {
    root.as_object_mut()
        .ok_or("Codex 配置根节点无效")?
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("Codex hooks 配置不是表")?
        .entry("state")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Codex hooks.state 不是原生状态表".to_owned())
}

fn slot_keys(root: &Value, source: &str) -> BTreeSet<String> {
    root.get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(event, _)| hooks::valid_event(event))
        .flat_map(|(event, groups)| {
            groups
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .flat_map(move |(group, value)| {
                    value
                        .get("hooks")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .enumerate()
                        .map(move |(handler, _)| hooks::native_id(source, event, group, handler))
                })
        })
        .collect()
}

fn rekey_states(
    root: &mut Value,
    source: &str,
    final_document: &Value,
    locations: &BTreeMap<HookLocation, Option<HookLocation>>,
) -> Result<(), String> {
    if root
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .is_none()
    {
        return Ok(());
    }
    let moves = locations
        .iter()
        .map(|(old, new)| {
            (
                hooks::native_id(source, &old.event, old.group, old.handler),
                new.as_ref()
                    .map(|new| hooks::native_id(source, &new.event, new.group, new.handler)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let final_keys = slot_keys(final_document, source);
    let table = state_table(root)?;
    let mut retained = Vec::new();
    for (key, value) in table.iter() {
        if let Some(Some(new)) = moves.get(key.trim()) {
            // Preserve trimmed aliases and all unknown state fields, including
            // trusted_hash. A content edit must become modified, not trusted.
            let prefix = key.len() - key.trim_start().len();
            let suffix = key.trim_end().len();
            retained.push((
                format!("{}{}{}", &key[..prefix], new, &key[suffix..]),
                value.clone(),
            ));
        }
    }
    table.retain(|key, _| !moves.contains_key(key.trim()) && !final_keys.contains(key.trim()));
    for (key, value) in retained {
        if table.insert(key, value).is_some() {
            return Err("Codex Hook 状态重排与既有绑定冲突".to_owned());
        }
    }
    Ok(())
}

fn set_enabled(root: &mut Value, key: &str, enabled: bool) -> Result<(), String> {
    let table = state_table(root)?;
    for (candidate, state) in table
        .iter_mut()
        .filter(|(candidate, _)| candidate.trim() == key)
    {
        let _ = candidate;
        if let Some(state) = state.as_object_mut() {
            state.remove("enabled");
        }
    }
    table
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("Codex Hook 状态条目不是表")?
        .insert("enabled".to_owned(), Value::Bool(enabled));
    if hooks::decode_states(root)
        .get(key)
        .and_then(|state| state.enabled)
        != Some(enabled)
    {
        return Err("Codex Hook 状态包含无效字段，不能确认开关已生效".to_owned());
    }
    Ok(())
}

fn prepare_hook_edits(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    edits: &[HookNativeEdit],
) -> Result<HookNativePrepared, String> {
    hook_codec::require_destination(snapshot, destination, hook_targets)?;
    let source = hook_codec::source(snapshot, &destination.source_id)?;
    let context = hook_codec::context(snapshot, source)?;
    let (_, format) = role(source, context).ok_or("Codex Hook 目标角色已变化")?;
    let document = HookDocument::capture(snapshot, &source.id, format)?;
    if !document
        .root
        .as_object()
        .is_some_and(|root| hooks::valid_document(root, matches!(format, HookDocumentFormat::Json)))
    {
        return Err("Codex 原始 Hook 配置不能完整解码".to_owned());
    }
    let structural = edits
        .iter()
        .filter(|edit| !matches!(edit, HookNativeEdit::SetEnabled { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let changed = hook_codec::edit_document_tracked(&document, &structural, validate_definition)?;
    let policy_source = policy_source(snapshot, context)?;
    let policy = HookDocument::capture(snapshot, &policy_source.id, HookDocumentFormat::Toml)?;
    let mut policy_root = if policy.source_id == document.source_id {
        changed.root.clone()
    } else {
        policy.root.clone()
    };
    if !structural.is_empty() {
        rekey_states(
            &mut policy_root,
            &source.path,
            &changed.root,
            &changed.locations,
        )?;
    }
    let mut read_source_ids = BTreeSet::from([source.id.clone(), policy_source.id.clone()]);
    let mut affected_asset_ids = hook_codec::affected_source_assets(snapshot, &source.id)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut expectations = changed.expectations;
    for edit in edits {
        if let HookNativeEdit::SetEnabled { original, enabled } = edit {
            let original_source = hook_codec::source(snapshot, &original.source_id)?;
            if original_source.context_id != context.id
                || original_source.origin == AgentAssetInstallationOrigin::NativePackage
                || matches!(
                    original_source.scope,
                    AgentAssetScope::Managed | AgentAssetScope::System
                )
            {
                return Err("托管或其他上下文的 Codex Hook 不能由用户开关覆盖".to_owned());
            }
            let matches = inspect_source(snapshot, &original.source_id)?
                .into_iter()
                .filter(|rule| {
                    rule.definition == original.definition
                        && rule.anchor.original_group == original.anchor.original_group
                })
                .collect::<Vec<_>>();
            let [current] = matches.as_slice() else {
                return Err("Codex Hook 原始绑定已变化或存在歧义".to_owned());
            };
            let location = HookLocation {
                event: current.anchor.event.clone(),
                group: current.anchor.group_index,
                handler: current.anchor.handler_index,
            };
            let mapped = if original.source_id == source.id {
                changed
                    .locations
                    .get(&location)
                    .ok_or("Codex Hook 位置证据已失效")?
                    .as_ref()
                    .ok_or("已移除的 Hook 不能切换状态")?
            } else {
                &location
            };
            let key = hooks::native_id(
                &original_source.path,
                &mapped.event,
                mapped.group,
                mapped.handler,
            );
            set_enabled(&mut policy_root, &key, *enabled)?;
            read_source_ids.insert(original.source_id.clone());
            affected_asset_ids.extend(hook_codec::affected_source_assets(
                snapshot,
                &original.source_id,
            ));
            expectations.push(HookNativeExpectation {
                source_id: original.source_id.clone(),
                definition: original.definition.clone(),
                occurrences: 1,
                enabled: Some(if *enabled {
                    AgentAssetDeclaredState::Enabled
                } else {
                    AgentAssetDeclaredState::Disabled
                }),
            });
        }
    }
    let mut writes = Vec::new();
    let mut required_capabilities = vec![HookNativeCapability::Configuration];
    if policy
        .root
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        != policy_root
            .get("hooks")
            .and_then(|hooks| hooks.get("state"))
        || edits
            .iter()
            .any(|edit| matches!(edit, HookNativeEdit::SetEnabled { .. }))
    {
        required_capabilities.push(HookNativeCapability::NativeSwitch);
    }
    if policy.source_id != document.source_id {
        writes.extend(document.render(&changed.root)?);
    }
    writes.extend(policy.render(&policy_root)?);
    Ok(HookNativePrepared { required_capabilities, read_source_ids: read_source_ids.into_iter().collect(), writes, expectations, affected_asset_ids: affected_asset_ids.into_iter().collect(), notes: vec!["使用 Codex 原生 hooks.state.enabled；保留信任记录且不授权 Hook。会话临时参数可能覆盖持久配置，既有会话需重新加载。".to_owned()] })
}
