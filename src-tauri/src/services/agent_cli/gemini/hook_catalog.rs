//! Exact Gemini Hook definitions and persistent native disabled-list operations.

use super::environment::{hook_policy as policy, hooks};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::native::{
            hook_codec::{self, HookDocument, HookDocumentFormat, HookSlot},
            hooks::*,
        },
        environment::mutation::MutationInventory,
        native_agent_kinds::gemini::AGENT_KIND,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

pub(super) static ADAPTER: NativeHookAdapter = NativeHookAdapter {
    switch_mode: HookNativeSwitchMode::Native,
    agent_kind: AGENT_KIND,
    parser_version: super::environment::PARSER_VERSION,
    default_role: "settings",
    read_hook,
    validate_adoption,
    hook_targets: policy::targets,
    inspect_source,
    validate_definition,
    prepare_hook_edits,
};

fn extension_name<'a>(
    source: &'a AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Option<&'a str> {
    let root = Path::new(&context.config_root);
    let relative = Path::new(&source.path)
        .strip_prefix(root.join("extensions"))
        .ok()?;
    let mut components = relative.components();
    let Component::Normal(name) = components.next()? else {
        return None;
    };
    let name = name.to_str()?;
    (context.agent_kind == AGENT_KIND
        && !name.is_empty()
        && components.as_path() == Path::new("hooks/hooks.json")
        && Path::new(&source.allowed_root) == root
        && hook_codec::exact_source(
            source,
            context,
            AgentAssetScope::User,
            Path::new(&source.path),
            AgentAssetInstallationOrigin::NativePackage,
        ))
    .then_some(name)
}

fn read_hook(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
) -> Result<HookNativeRule, String> {
    hook_codec::read_rule(snapshot, asset, inspect_source)
}

fn rules_from_document(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
    document: &HookDocument,
) -> Result<Vec<HookNativeRule>, String> {
    let extension = extension_name(source, context);
    if extension.is_none() && policy::settings_role(source, context).is_none() {
        return Err("来源不是 Gemini 原生 Hook 设置或 Extension 定义".to_owned());
    }
    let Some(events) = document.root.get("hooks") else {
        return Ok(Vec::new());
    };
    let events = events.as_object().ok_or("Gemini Hook 事件表无效")?;
    let mut slots = Vec::new();
    for (event, groups) in events {
        if !hooks::valid_event(event) {
            continue;
        }
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for (handler_index, handler) in handlers.iter().enumerate() {
                if hooks::disable_identity(handler).is_none() {
                    continue;
                }
                let native_id = match extension {
                    Some(extension) => {
                        format!("{extension}:{event}:{group_index}:{handler_index}")
                    }
                    None => format!("{event}:{group_index}:{handler_index}"),
                };
                slots.push(HookSlot {
                    native_id,
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
        AgentAssetDeclaredState::Unknown,
    )
}

fn inspect_source(
    snapshot: &MutationInventory,
    source_id: &str,
) -> Result<Vec<HookNativeRule>, String> {
    let source = hook_codec::source(snapshot, source_id)?;
    let context = hook_codec::context(snapshot, source)?;
    let settings = policy::load_settings(snapshot, context)?;
    let document = observe_document(snapshot, source, context)?;
    let mut rules = rules_from_document(snapshot, source, context, &document)?;
    for rule in &mut rules {
        rule.enabled = policy::configured_state(&settings, &BTreeMap::new(), rule_identity(rule)?);
    }
    Ok(rules)
}

fn observe_document(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Result<HookDocument, String> {
    let format = if extension_name(source, context).is_some() {
        HookDocumentFormat::Json
    } else if policy::settings_role(source, context).is_some() {
        HookDocumentFormat::Jsonc
    } else {
        return Err("来源不是 Gemini 原生 Hook 设置或 Extension 定义".to_owned());
    };
    HookDocument::observe(snapshot, &source.id, format)
}

fn rule_identity(rule: &HookNativeRule) -> Result<&str, String> {
    hooks::disable_identity(hook_codec::selected_handler(&rule.definition)?)
        .ok_or_else(|| "Gemini Hook 缺少有效的原生禁用身份".to_owned())
}

fn validate_definition(definition: &HookNativeDefinition) -> Result<(), String> {
    if !hooks::valid_event(&definition.event) || !hooks::valid_group(&definition.group) {
        return Err("Hook 事件或 matcher group 不符合 Gemini 原生 schema".to_owned());
    }
    let handler = hook_codec::selected_handler(definition)?;
    if handler.get("type").and_then(Value::as_str) != Some("command") {
        return Err(
            "Gemini 共享定义仅支持 command；plugin/runtime 依赖原生注册，不能独立复制".to_owned(),
        );
    }
    if hooks::disable_identity(handler).is_none()
        || handler
            .get("description")
            .is_some_and(|value| !value.is_string())
        || handler
            .get("timeout")
            .is_some_and(|value| !value.is_number())
    {
        return Err("Hook handler 不符合 Gemini command schema".to_owned());
    }
    Ok(())
}

fn validate_adoption(rule: &HookNativeRule) -> Result<(), String> {
    if hooks::sequential_siblings(&rule.anchor.original_group) {
        return Err(
            "此 Gemini Hook 依赖同组顺序执行，不能将单条 handler 独立收录或复制".to_owned(),
        );
    }
    validate_definition(&rule.definition)
}

fn require_in_place_group(
    original: &HookNativeRule,
    definition: &HookNativeDefinition,
) -> Result<(), String> {
    if hooks::sequential_siblings(&original.anchor.original_group)
        && (definition.event != original.definition.event
            || !hooks::same_group_metadata(&original.definition.group, &definition.group))
    {
        return Err(
            "顺序执行组只能原位修改选定 handler，不能改变事件或拆分 matcher group".to_owned(),
        );
    }
    Ok(())
}

fn edit_original(edit: &HookNativeEdit) -> Option<&HookNativeRule> {
    match edit {
        HookNativeEdit::Add { .. } => None,
        HookNativeEdit::Replace { original, .. }
        | HookNativeEdit::Remove { original }
        | HookNativeEdit::SetEnabled { original, .. }
        | HookNativeEdit::Restore { original, .. } => Some(original),
    }
}

fn extension_dependencies(
    snapshot: &MutationInventory,
    context: &AgentConfigurationContext,
    read_source_ids: &BTreeSet<String>,
    include_membership: bool,
) -> Result<Vec<String>, String> {
    let root = Path::new(&context.config_root);
    let mut paths = Vec::new();
    for source in snapshot
        .inventory
        .sources
        .iter()
        .filter(|source| read_source_ids.contains(&source.id))
    {
        if let Some(name) = extension_name(source, context) {
            paths.push((
                root.join("extensions")
                    .join(name)
                    .join("gemini-extension.json"),
                AgentAssetInstallationOrigin::NativePackage,
                AgentAssetSourceKind::File,
            ));
        }
    }
    if !paths.is_empty() {
        paths.push((
            root.join("extensions/extension-enablement.json"),
            AgentAssetInstallationOrigin::ConfigEntry,
            AgentAssetSourceKind::File,
        ));
    }
    if include_membership {
        paths.push((
            root.join("extensions"),
            AgentAssetInstallationOrigin::NativePackage,
            AgentAssetSourceKind::Directory,
        ));
    }
    paths
        .into_iter()
        .map(|(path, origin, kind)| {
            let mut sources = snapshot.inventory.sources.iter().filter(|source| {
                source.context_id == context.id
                    && source.environment_id == context.environment_id
                    && source.scope == AgentAssetScope::User
                    && source.origin == origin
                    && source.source_kind == kind
                    && source.categories.contains(&AgentAssetCategory::Extension)
                    && Path::new(&source.allowed_root) == root
                    && Path::new(&source.path) == path
            });
            let source = sources
                .next()
                .ok_or("Gemini Extension Hook 的父级依赖来源不完整")?;
            if sources.next().is_some() {
                return Err("Gemini Extension Hook 的父级依赖来源存在歧义".to_owned());
            }
            Ok(source.id.clone())
        })
        .collect()
}

fn prepare_hook_edits(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    edits: &[HookNativeEdit],
) -> Result<HookNativePrepared, String> {
    hook_codec::require_destination(snapshot, destination, policy::targets)?;
    let target = hook_codec::source(snapshot, &destination.source_id)?;
    let context = hook_codec::context(snapshot, target)?;
    let settings = policy::load_settings(snapshot, context)?;
    for document in &settings {
        policy::disabled_set(&document.document.root)?;
    }
    let mut documents = BTreeMap::new();
    let mut rules = Vec::new();
    let selected_sources = edits
        .iter()
        .filter_map(edit_original)
        .map(|original| original.source_id.as_str())
        .chain(std::iter::once(destination.source_id.as_str()))
        .collect::<BTreeSet<_>>();
    for source in snapshot.inventory.sources.iter().filter(|source| {
        policy::settings_role(source, context).is_some()
            || selected_sources.contains(source.id.as_str())
    }) {
        let document = observe_document(snapshot, source, context)?;
        rules.extend(rules_from_document(snapshot, source, context, &document)?);
        documents.insert(source.id.clone(), document);
    }
    let mut structural = Vec::new();
    let mut switches = BTreeMap::new();
    for edit in edits {
        if let Some(original) = edit_original(edit) {
            if !matches!(edit, HookNativeEdit::Restore { .. }) {
                let matches = rules
                    .iter()
                    .filter(|rule| {
                        rule.source_id == original.source_id
                            && rule.definition == original.definition
                            && rule.anchor.original_group == original.anchor.original_group
                    })
                    .count();
                if matches != 1 {
                    return Err("Gemini Hook 原始节点已变化或存在歧义".to_owned());
                }
            }
        }
        match edit {
            HookNativeEdit::SetEnabled { original, enabled } => {
                policy::request_switch(
                    &mut switches,
                    rule_identity(original)?.to_owned(),
                    *enabled,
                )?;
            }
            HookNativeEdit::Replace {
                original,
                definition,
            }
            | HookNativeEdit::Restore {
                original,
                definition,
            } => {
                require_in_place_group(original, definition)?;
                let state = if matches!(edit, HookNativeEdit::Restore { .. }) {
                    original.enabled
                } else {
                    policy::configured_state(&settings, &BTreeMap::new(), rule_identity(original)?)
                };
                if state == AgentAssetDeclaredState::Disabled {
                    let identity =
                        hooks::disable_identity(hook_codec::selected_handler(definition)?)
                            .ok_or("Gemini Hook 缺少有效的原生禁用身份")?;
                    if policy::configured_state(&settings, &BTreeMap::new(), identity)
                        != AgentAssetDeclaredState::Disabled
                    {
                        policy::request_switch(&mut switches, identity.to_owned(), false)?;
                    }
                } else if state != AgentAssetDeclaredState::Enabled {
                    return Err("Gemini Hook 原有启停状态未知，不能应用新定义".to_owned());
                }
                structural.push(edit.clone());
            }
            _ => structural.push(edit.clone()),
        }
    }
    // Only native identity-policy changes can affect hooks in other packages.
    // A separate invalid Extension must not block a proven in-place definition edit.
    if !switches.is_empty() {
        if !snapshot
            .inventory
            .hook_rule_counts
            .iter()
            .any(|count| count.agent_kind == AGENT_KIND && count.rule_count.is_some())
        {
            return Err(
                "Gemini Hook 盘点不完整，无法确认同身份策略的全部影响，请重新盘点后再试".to_owned(),
            );
        }
        for source in snapshot.inventory.sources.iter().filter(|source| {
            extension_name(source, context).is_some()
                && !selected_sources.contains(source.id.as_str())
        }) {
            let document = observe_document(snapshot, source, context)?;
            rules.extend(rules_from_document(snapshot, source, context, &document)?);
            documents.insert(source.id.clone(), document);
        }
    }
    let document = documents
        .get(&destination.source_id)
        .ok_or("Gemini Hook 目标来源不完整")?;
    let (root, mut expectations) =
        hook_codec::edit_document(document, &structural, validate_definition)?;
    let mut desired = BTreeMap::from([(destination.source_id.clone(), root)]);
    for (identity, enabled) in &switches {
        if *enabled {
            for policy in &settings {
                let names = policy::disabled_set(&policy.document.root)?;
                if !names.contains(identity) {
                    continue;
                }
                if policy.destination.is_none() {
                    if policy.participates {
                        return Err(
                            "选定 Gemini Hook 仍被只读配置禁用，不能仅修改用户设置就将其启用"
                                .to_owned(),
                        );
                    }
                    continue;
                }
                let root = desired
                    .entry(policy.document.source_id.clone())
                    .or_insert_with(|| policy.document.root.clone());
                policy::edit_disabled(root, identity, true)?;
            }
        } else {
            policy::edit_disabled(
                desired
                    .get_mut(&destination.source_id)
                    .ok_or("Gemini Hook 禁用策略目标已失效")?,
                identity,
                false,
            )?;
        }
    }
    let mut affected = BTreeSet::new();
    if !structural.is_empty() {
        affected.extend(hook_codec::affected_source_assets(
            snapshot,
            &destination.source_id,
        ));
    }
    let mut policy_affected = BTreeSet::new();
    for rule in &rules {
        if !switches.contains_key(rule_identity(rule)?) {
            continue;
        }
        let asset_id = rule
            .native_asset_id
            .as_ref()
            .ok_or("Gemini Hook 同身份规则缺少完整盘点，无法确认全部影响")?;
        policy_affected.insert(asset_id.clone());
        let root = desired
            .get(&rule.source_id)
            .unwrap_or(&documents[&rule.source_id].root);
        if !expectations.iter().any(|expectation| {
            expectation.source_id == rule.source_id && expectation.definition == rule.definition
        }) {
            expectations.push(HookNativeExpectation {
                source_id: rule.source_id.clone(),
                definition: rule.definition.clone(),
                occurrences: hook_codec::occurrences(root, &rule.definition)?,
                enabled: None,
            });
        }
    }
    affected.extend(policy_affected.iter().cloned());
    for expectation in &mut expectations {
        if expectation.occurrences > 0 {
            let identity =
                hooks::disable_identity(hook_codec::selected_handler(&expectation.definition)?)
                    .ok_or("Gemini Hook 验证定义无效")?;
            expectation.enabled = Some(policy::configured_state(&settings, &desired, identity));
        }
    }
    let mut writes = Vec::new();
    for (source_id, root) in &desired {
        if root == &documents[source_id].root {
            continue;
        }
        // A policy document is observable even when readonly. Each actual write
        // separately requires the exact writable settings role and file guard.
        let source = hook_codec::source(snapshot, source_id)?;
        let role =
            policy::settings_role(source, context).ok_or("Gemini Hook 写入来源角色已变化")?;
        if hook_codec::destination(source, role).is_none() {
            return Err("Gemini Hook 策略来源不可写".to_owned());
        }
        let document = HookDocument::capture(snapshot, source_id, HookDocumentFormat::Jsonc)?;
        writes.extend(document.render(root)?);
    }
    let mut notes = vec![
        "保留 hooksConfig.enabled、全局信任和其他策略；已运行的 Gemini 会话可能需要重新加载配置"
            .to_owned(),
    ];
    if !switches.is_empty() {
        notes.push(format!(
            "原生禁用身份按大小写精确匹配；本次策略同时涉及 {} 条已观察 Hook，包含同身份的设置与 Extension 规则",
            policy_affected.len()
        ));
    }
    if expectations.iter().any(|expectation| {
        expectation.occurrences > 0
            && expectation.enabled == Some(AgentAssetDeclaredState::Disabled)
    }) {
        notes.push("已有或新身份匹配的禁用策略继续保留，应用定义不会自动启用 Hook".to_owned());
    }
    let mut read_source_ids = documents.into_keys().collect::<BTreeSet<_>>();
    read_source_ids.extend(policy::trust_source_id(snapshot, context)?);
    let dependencies =
        extension_dependencies(snapshot, context, &read_source_ids, !switches.is_empty())?;
    read_source_ids.extend(dependencies);
    let mut required_capabilities = vec![HookNativeCapability::Configuration];
    if !switches.is_empty() {
        required_capabilities.push(HookNativeCapability::NativeSwitch);
    }
    Ok(HookNativePrepared {
        required_capabilities,
        read_source_ids: read_source_ids.into_iter().collect(),
        writes,
        expectations,
        affected_asset_ids: affected.into_iter().collect(),
        notes,
    })
}
