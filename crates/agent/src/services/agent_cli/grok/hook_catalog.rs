//! Grok native Hook editing and enablement with source and policy validation.
use super::environment::hooks;
use crate::{
    models::*,
    services::agent_cli::{
        catalog::native::{
            hook_codec::{self, HookDocument, HookDocumentFormat, HookLocation, HookSlot},
            hooks::*,
        },
        environment::mutation::{GuardedFile, MutationInventory},
        native_agent_kinds::grok::AGENT_KIND,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod plugin;

struct InspectedSource {
    rules: Vec<HookNativeRule>,
    namespace: String,
    read_source_ids: BTreeSet<String>,
}

pub(super) static ADAPTER: NativeHookAdapter = NativeHookAdapter {
    switch_mode: HookNativeSwitchMode::Native,
    agent_kind: AGENT_KIND,
    parser_version: super::environment::PARSER_VERSION,
    default_role: "config",
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
) -> Option<(String, String, HookDocumentFormat)> {
    let root = Path::new(&context.config_root);
    for (role, filename, namespace) in [
        ("config", "config.toml", "user"),
        ("hook-user-managed", "managed_config.toml", "managed"),
        (
            "hook-user-requirements",
            "requirements.toml",
            "requirements/user",
        ),
    ] {
        if hook_codec::exact_source(
            source,
            context,
            AgentAssetScope::User,
            &root.join(filename),
            AgentAssetInstallationOrigin::ConfigEntry,
        ) {
            return Some((
                role.to_owned(),
                namespace.to_owned(),
                HookDocumentFormat::Toml,
            ));
        }
    }
    let path = Path::new(&source.path);
    if path.extension().and_then(|value| value.to_str()) != Some("json")
        || path.file_name()?.to_str()?.starts_with('.')
    {
        return None;
    }
    let directory = match source.scope {
        AgentAssetScope::User => root.join("hooks"),
        AgentAssetScope::Workspace => {
            Path::new(context.workspace_id.as_deref()?).join(".grok/hooks")
        }
        _ => return None,
    };
    if path.parent()? != directory
        || !hook_codec::exact_source(
            source,
            context,
            source.scope,
            path,
            AgentAssetInstallationOrigin::LocalFiles,
        )
    {
        return None;
    }
    Some((
        "hook-file".to_owned(),
        hooks::file_namespace(path, source.scope)?,
        HookDocumentFormat::Json,
    ))
}

fn hook_targets(
    inventory: &AgentEnvironmentInventory,
    context: &AgentConfigurationContext,
) -> Vec<HookNativeDestination> {
    inventory
        .sources
        .iter()
        .filter_map(|source| {
            role(source, context).and_then(|(role, _, _)| hook_codec::destination(source, &role))
        })
        .collect()
}

fn policy_source<'a>(
    snapshot: &'a MutationInventory,
    context: &AgentConfigurationContext,
) -> Result<&'a AgentAssetSource, String> {
    let path = Path::new(&context.config_root).join("disabled-hooks");
    let matches = snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| {
            hook_codec::exact_source(
                source,
                context,
                AgentAssetScope::User,
                &path,
                AgentAssetInstallationOrigin::ConfigEntry,
            )
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [source] => Ok(*source),
        _ => Err("Grok Hook 禁用策略来源缺失或不唯一".to_owned()),
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
    Ok(inspect_complete(snapshot, source_id)?.rules)
}

fn inspect_complete(
    snapshot: &MutationInventory,
    source_id: &str,
) -> Result<InspectedSource, String> {
    let source = hook_codec::source(snapshot, source_id)?;
    let context = hook_codec::context(snapshot, source)?;
    let mut inspected = if let Some((_, namespace, format)) = role(source, context) {
        let document = HookDocument::observe(snapshot, source_id, format)?;
        validate_source(&document.root)?;
        let rules = source_rules(
            snapshot,
            source,
            &document.root,
            &namespace,
            matches!(format, HookDocumentFormat::Json),
        )?;
        InspectedSource {
            rules,
            namespace,
            read_source_ids: BTreeSet::from([source.id.clone()]),
        }
    } else {
        plugin::inspect(snapshot, source, context)?
    };
    let policy = policy_source(snapshot, context)?;
    let guard = GuardedFile::capture(policy, &snapshot.source_anchors)
        .map_err(|_| "Grok Hook 禁用策略无法安全读取")?;
    guard.revalidate().map_err(|_| "Grok Hook 禁用策略已变化")?;
    let disabled = hooks::decode_disabled(
        std::str::from_utf8(guard.bytes().unwrap_or_default())
            .map_err(|_| "Grok disabled-hooks 不是 UTF-8")?,
    );
    for rule in &mut inspected.rules {
        let name = hooks::native_id(
            &inspected.namespace,
            &rule.anchor.event,
            rule.anchor.group_index,
            rule.anchor.handler_index,
        );
        if disabled.contains(&name) {
            rule.enabled = AgentAssetDeclaredState::Disabled;
        }
    }
    inspected.read_source_ids.insert(policy.id.clone());
    Ok(inspected)
}

fn source_rules(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    value: &Value,
    namespace: &str,
    standalone: bool,
) -> Result<Vec<HookNativeRule>, String> {
    let root = value.as_object().ok_or("Grok Hook 配置不是对象")?;
    let mut slots = Vec::new();
    if !hooks::walk_slots(root, standalone, |event, group, handler| {
        slots.push(HookSlot {
            native_id: hooks::native_id(namespace, event, group, handler),
            event: event.to_owned(),
            group_index: group,
            handler_index: handler,
        });
        true
    }) {
        return Err("Grok Hook 配置含无法精确定位的事件别名、matcher 或 handler".to_owned());
    }
    hook_codec::rules_from_slots(
        snapshot,
        source,
        value,
        slots,
        AgentAssetDeclaredState::Enabled,
    )
}

fn validate_source(root: &Value) -> Result<(), String> {
    if root
        .get("version_overrides")
        .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
    {
        return Err("此来源包含 Grok 原生版本覆盖，不能仅修改基础 Hook 配置".to_owned());
    }
    Ok(())
}

fn validate_definition(definition: &HookNativeDefinition) -> Result<(), String> {
    if !hooks::valid_event(&definition.event)
        || !definition
            .group
            .as_object()
            .is_some_and(hooks::valid_group_metadata)
        || !hooks::valid_handler(hook_codec::selected_handler(definition)?)
        || !hooks::valid_matcher(&definition.event, &definition.group)
    {
        return Err("Hook 不符合固定 Grok 源码的可加载原生 schema".to_owned());
    }
    Ok(())
}

fn validate_adoption(rule: &HookNativeRule) -> Result<(), String> {
    validate_definition(&rule.definition)
}

fn all_names(root: &Value, namespace: &str) -> BTreeSet<String> {
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
                        .map(move |(handler, _)| hooks::native_id(namespace, event, group, handler))
                })
        })
        .collect()
}

fn rekey_disabled(
    text: &str,
    namespace: &str,
    root: &Value,
    locations: &BTreeMap<HookLocation, Option<HookLocation>>,
) -> String {
    let moves = locations
        .iter()
        .map(|(old, new)| {
            (
                hooks::native_id(namespace, &old.event, old.group, old.handler),
                new.as_ref()
                    .map(|new| hooks::native_id(namespace, &new.event, new.group, new.handler)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let final_names = all_names(root, namespace);
    let mut result = String::new();
    for line in text.split_inclusive('\n') {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            result.push_str(line);
            continue;
        }
        match moves.get(name) {
            Some(Some(next)) => {
                let prefix = line.len() - line.trim_start().len();
                let suffix = line.trim_end().len();
                result.push_str(&line[..prefix]);
                result.push_str(next);
                result.push_str(&line[suffix..]);
            }
            Some(None) => (),
            None if final_names.contains(name) => (), // stale state must not attach to a new occupant
            None => result.push_str(line),
        }
    }
    result
}

fn set_enabled(text: &str, name: &str, enabled: bool) -> String {
    if enabled {
        return text
            .split_inclusive('\n')
            .filter(|line| line.trim() != name)
            .collect();
    }
    if hooks::decode_disabled(text).contains(name) {
        return text.to_owned();
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut result = text.to_owned();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push_str(newline);
    }
    result.push_str(name);
    result.push_str(newline);
    result
}

fn prepare_hook_edits(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    edits: &[HookNativeEdit],
) -> Result<HookNativePrepared, String> {
    hook_codec::require_destination(snapshot, destination, hook_targets)?;
    let source = hook_codec::source(snapshot, &destination.source_id)?;
    let context = hook_codec::context(snapshot, source)?;
    let (_, namespace, format) = role(source, context).ok_or("Grok Hook 目标角色已变化")?;
    let document = HookDocument::capture(snapshot, &source.id, format)?;
    validate_source(&document.root)?;
    let structural = edits
        .iter()
        .filter(|edit| !matches!(edit, HookNativeEdit::SetEnabled { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let changed = hook_codec::edit_document_tracked(&document, &structural, validate_definition)?;
    if !changed.root.as_object().is_some_and(|root| {
        hooks::walk_slots(
            root,
            matches!(format, HookDocumentFormat::Json),
            |_, _, _| true,
        )
    }) {
        return Err("修改后的 Grok Hook 名称存在别名歧义或原生 schema 无效".to_owned());
    }
    let policy = policy_source(snapshot, context)?;
    let guard = GuardedFile::capture(policy, &snapshot.source_anchors)
        .map_err(|_| "Grok Hook 禁用策略已变化或不可写")?;
    let original_policy = std::str::from_utf8(guard.bytes().unwrap_or_default())
        .map_err(|_| "Grok disabled-hooks 不是 UTF-8")?;
    let mut policy_text = if structural.is_empty() {
        original_policy.to_owned()
    } else {
        rekey_disabled(
            original_policy,
            &namespace,
            &changed.root,
            &changed.locations,
        )
    };
    let mut read_source_ids = BTreeSet::from([source.id.clone(), policy.id.clone()]);
    let mut affected_asset_ids = hook_codec::affected_source_assets(snapshot, &source.id)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut expectations = changed.expectations;
    for edit in edits {
        if let HookNativeEdit::SetEnabled { original, enabled } = edit {
            let original_source = hook_codec::source(snapshot, &original.source_id)?;
            if original_source.context_id != context.id
                || matches!(
                    original_source.scope,
                    AgentAssetScope::Managed | AgentAssetScope::System
                )
            {
                return Err("托管或其他上下文的 Grok Hook 不可由用户开关覆盖".to_owned());
            }
            let inspected = inspect_complete(snapshot, &original.source_id)?;
            let original_namespace = inspected.namespace;
            read_source_ids.extend(inspected.read_source_ids);
            let matches = inspected
                .rules
                .into_iter()
                .filter(|rule| {
                    rule.definition == original.definition
                        && rule.anchor.original_group == original.anchor.original_group
                })
                .collect::<Vec<_>>();
            let [current] = matches.as_slice() else {
                return Err("Grok Hook 原始绑定已变化或存在歧义".to_owned());
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
                    .ok_or("Grok Hook 位置证据已失效")?
                    .as_ref()
                    .ok_or("已移除的 Hook 不能切换状态")?
            } else {
                &location
            };
            let name = hooks::native_id(
                &original_namespace,
                &mapped.event,
                mapped.group,
                mapped.handler,
            );
            policy_text = set_enabled(&policy_text, &name, *enabled);
            read_source_ids.insert(original.source_id.clone());
            affected_asset_ids.extend(
                snapshot
                    .inventory
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.context_id == context.id
                            && asset.category == AgentAssetCategory::Hook
                            && asset.native_id == name
                    })
                    .map(|asset| asset.stable_id.clone()),
            );
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
    let mut writes = document
        .render(&changed.root)?
        .into_iter()
        .collect::<Vec<_>>();
    let mut required_capabilities = vec![HookNativeCapability::Configuration];
    if policy_text != original_policy
        || edits
            .iter()
            .any(|edit| matches!(edit, HookNativeEdit::SetEnabled { .. }))
    {
        required_capabilities.push(HookNativeCapability::NativeSwitch);
    }
    if policy_text != original_policy {
        writes.push(HookNativeWrite {
            source_id: policy.id.clone(),
            bytes: policy_text.into_bytes(),
        });
    }
    Ok(HookNativePrepared { required_capabilities, read_source_ids: read_source_ids.into_iter().collect(), writes, expectations, affected_asset_ids: affected_asset_ids.into_iter().collect(), notes: vec!["使用 Grok 原生 disabled-hooks 名称开关；保留项目可信状态。此格式按固定源码校验，不能据此证明其他安装构建的运行行为。".to_owned()] })
}
