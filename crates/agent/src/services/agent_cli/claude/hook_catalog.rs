//! Claude native Hook configuration: one handler per shared binding.

use super::environment::parse::hooks;
use crate::{
    models::*,
    services::agent_cli::{
        catalog::native::{
            hook_codec::{self, HookDocument, HookDocumentFormat, HookSlot},
            hooks::*,
        },
        environment::mutation::MutationInventory,
        native_agent_kinds::claude::AGENT_KIND,
    },
};
use serde_json::Value;
use std::path::Path;

pub(super) static ADAPTER: NativeHookAdapter = NativeHookAdapter {
    switch_mode: HookNativeSwitchMode::Suspend,
    agent_kind: AGENT_KIND,
    parser_version: super::environment::PARSER_VERSION,
    default_role: "settings",
    read_hook,
    hook_targets,
    inspect_source,
    validate_definition,
    validate_adoption,
    prepare_hook_edits,
};

fn validate_adoption(rule: &HookNativeRule) -> Result<(), String> {
    validate_definition(&rule.definition)?;
    let serialized = serde_json::to_string(&rule.definition.group).map_err(|_| "Hook 定义无效")?;
    if ["CLAUDE_PLUGIN_ROOT", "CLAUDE_PLUGIN_DATA"]
        .iter()
        .any(|name| serialized.contains(name))
    {
        return Err(
            "此 Hook 依赖所属 Plugin 的运行环境，需先将共享定义中的依赖改为独立配置".to_owned(),
        );
    }
    Ok(())
}

fn settings_role(
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Option<&'static str> {
    let root = Path::new(&context.config_root);
    let mut roles = vec![(
        "settings",
        AgentAssetScope::User,
        root.join("settings.json"),
    )];
    if let Some(workspace) = context.workspace_id.as_deref() {
        let workspace = Path::new(workspace).join(".claude");
        roles.extend([
            (
                "workspace-settings",
                AgentAssetScope::Workspace,
                workspace.join("settings.json"),
            ),
            (
                "workspace-local-settings",
                AgentAssetScope::Local,
                workspace.join("settings.local.json"),
            ),
        ]);
    }
    let managed = super::environment::discovery::managed_root();
    roles.push((
        "managed-settings",
        AgentAssetScope::Managed,
        managed.join("managed-settings.json"),
    ));
    roles.into_iter().find_map(|(role, scope, path)| {
        hook_codec::exact_source(
            source,
            context,
            scope,
            &path,
            AgentAssetInstallationOrigin::ConfigEntry,
        )
        .then_some(role)
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
            settings_role(source, context).and_then(|role| hook_codec::destination(source, role))
        })
        .collect()
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
    let document = HookDocument::observe(snapshot, source_id, HookDocumentFormat::Json)?;
    let Some(role) = settings_role(source, context) else {
        return plugin_rules(snapshot, source, &document.root);
    };
    let Some(events) = document.root.get("hooks") else {
        return Ok(Vec::new());
    };
    let events = events.as_object().ok_or("Claude Hook 事件表无效")?;
    let mut slots = Vec::new();
    for (event, groups) in events {
        if !hooks::valid_hook_event(event) {
            continue;
        }
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(object) = group
                .as_object()
                .filter(|object| hooks::valid_hook_matcher(object))
            else {
                continue;
            };
            let Some(handlers) = object.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for (handler_index, handler) in handlers.iter().enumerate() {
                if !hooks::valid_hook_entry(handler) {
                    continue;
                }
                slots.push(HookSlot {
                    native_id: hooks::hook_identity(
                        role,
                        event,
                        hooks::hook_matcher_shape(Some(object)),
                        Some(group_index),
                        Some(handler_index),
                    ),
                    event: event.clone(),
                    group_index,
                    handler_index,
                });
            }
        }
    }
    hook_codec::rules_from_slots(
        snapshot,
        source,
        &document.root,
        slots,
        AgentAssetDeclaredState::Enabled,
    )
}

fn plugin_rules(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    root: &Value,
) -> Result<Vec<HookNativeRule>, String> {
    if source.origin != AgentAssetInstallationOrigin::NativePackage
        || source.writable
        || source.revision.is_symlink
        || source.source_kind != AgentAssetSourceKind::File
        || !Path::new(&source.path).starts_with(&source.allowed_root)
    {
        return Err("来源不是已验证的 Claude Plugin Hook 文件".to_owned());
    }
    let mut views = std::collections::BTreeSet::new();
    for asset in &snapshot.inventory.assets {
        if asset.category != AgentAssetCategory::Hook
            || asset.inspection_source_id != source.id
            || asset.context_id != source.context_id
        {
            continue;
        }
        let Some(parent) = asset.relationships.provided_by.as_deref().and_then(|id| {
            snapshot.inventory.assets.iter().find(|parent| {
                parent.stable_id == id
                    && parent.context_id == asset.context_id
                    && parent.category == AgentAssetCategory::Plugin
            })
        }) else {
            continue;
        };
        let prefix = format!("plugin:{}:hook:", parent.native_id);
        let ordinal = asset
            .native_id
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.split_once(':'))
            .and_then(|(ordinal, _)| ordinal.parse::<usize>().ok())
            .ok_or("Plugin Hook 原生绑定角色无效")?;
        views.insert((parent.native_id.clone(), ordinal));
    }
    let manifest = Path::new(&source.path)
        == Path::new(&source.allowed_root).join(".claude-plugin/plugin.json");
    let mut rules = Vec::new();
    for (parent, ordinal) in views {
        let document = if manifest {
            if ordinal == 0 {
                return Err("Plugin Hook 内联位置无效".to_owned());
            }
            let components = root.get("hooks").ok_or("Plugin Hook 内联配置已变化")?;
            let component = if let Some(components) = components.as_array() {
                components.get(ordinal - 1)
            } else {
                (ordinal == 1).then_some(components)
            }
            .ok_or("Plugin Hook 内联位置已变化")?;
            if !component.is_object() {
                return Err("Plugin Hook 内联定义已变化".to_owned());
            }
            serde_json::json!({"hooks":component})
        } else {
            root.clone()
        };
        let mut slots = Vec::new();
        for (event, groups) in document
            .get("hooks")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if !hooks::valid_hook_event(event) {
                continue;
            }
            for (group_index, group) in groups.as_array().into_iter().flatten().enumerate() {
                let Some(group) = group
                    .as_object()
                    .filter(|group| hooks::valid_hook_matcher(group))
                else {
                    continue;
                };
                for (handler_index, handler) in group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if hooks::valid_hook_entry(handler) {
                        slots.push(HookSlot {
                            native_id: super::environment::plugin::hooks::native_id(
                                &parent,
                                ordinal,
                                event,
                                group_index,
                                handler_index,
                            ),
                            event: event.clone(),
                            group_index,
                            handler_index,
                        });
                    }
                }
            }
        }
        rules.extend(
            hook_codec::rules_from_slots(
                snapshot,
                source,
                &document,
                slots,
                AgentAssetDeclaredState::Enabled,
            )?
            .into_iter()
            .filter(|rule| rule.native_asset_id.is_some()),
        );
    }
    Ok(rules)
}

fn validate_definition(definition: &HookNativeDefinition) -> Result<(), String> {
    if !hooks::valid_hook_event(&definition.event) {
        return Err("Claude Code 不支持此原生 Hook 事件".to_owned());
    }
    if !definition
        .group
        .as_object()
        .is_some_and(hooks::valid_hook_matcher)
        || !hooks::valid_hook_entry(hook_codec::selected_handler(definition)?)
    {
        return Err("Hook matcher/handler 不符合 Claude Code 原生 schema".to_owned());
    }
    Ok(())
}

fn prepare_hook_edits(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    edits: &[HookNativeEdit],
) -> Result<HookNativePrepared, String> {
    hook_codec::require_destination(snapshot, destination, hook_targets)?;
    hook_codec::prepare_structural(
        snapshot,
        destination,
        edits,
        HookDocumentFormat::Json,
        validate_definition,
    )
}
