//! Structural operations shared by native Hook adapters. Native modules own
//! event/handler schemas, source roles, trust and enablement policy.

use super::hooks::*;
use crate::models::*;
use crate::services::agent_cli::{
    catalog::source_index::{definition_anchor, DefinitionIndex},
    config_support::{parse_jsonc_document, rewrite_json_document},
    environment::{
        config_document::{self, ConfigDocumentFormat},
        mutation::{GuardedFile, MutationInventory},
        verified_path::reopen_verified_path,
    },
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

mod toml_source;
use toml_source::render_toml;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) enum HookDocumentFormat {
    Json,
    Jsonc,
    Toml,
}

pub(crate) struct HookDocument {
    pub source_id: String,
    pub root: Value,
    bytes: Vec<u8>,
    format: HookDocumentFormat,
}

impl HookDocument {
    pub(crate) fn capture(
        snapshot: &MutationInventory,
        source_id: &str,
        format: HookDocumentFormat,
    ) -> Result<Self, String> {
        let source = source(snapshot, source_id)?;
        let guard = GuardedFile::capture(source, &snapshot.source_anchors)
            .map_err(|_| "Hook 来源已变化或不能安全读取")?;
        Self::from_bytes(source_id, guard.bytes().map(<[u8]>::to_vec), format)
    }

    pub(crate) fn observe(
        snapshot: &MutationInventory,
        source_id: &str,
        format: HookDocumentFormat,
    ) -> Result<Self, String> {
        Self::from_bytes(source_id, observe_bytes(snapshot, source_id)?, format)
    }

    fn from_bytes(
        source_id: &str,
        bytes: Option<Vec<u8>>,
        format: HookDocumentFormat,
    ) -> Result<Self, String> {
        let bytes = bytes.unwrap_or_else(|| match format {
            HookDocumentFormat::Toml => Vec::new(),
            _ => b"{}\n".to_vec(),
        });
        let root = match format {
            HookDocumentFormat::Json => config_document::parse(&bytes, ConfigDocumentFormat::Json)
                .ok_or("Hook JSON 无效或存在重复字段")?,
            HookDocumentFormat::Jsonc => parse_jsonc_document(
                std::str::from_utf8(&bytes).map_err(|_| "Hook 配置不是 UTF-8")?,
            )?,
            HookDocumentFormat::Toml => config_document::parse(&bytes, ConfigDocumentFormat::Toml)
                .ok_or("Hook TOML 无效或存在重复字段")?,
        };
        Ok(Self {
            source_id: source_id.to_owned(),
            root,
            bytes,
            format,
        })
    }

    pub(crate) fn render(&self, desired: &Value) -> Result<Option<HookNativeWrite>, String> {
        if desired == &self.root {
            return Ok(None);
        }
        let text = std::str::from_utf8(&self.bytes).map_err(|_| "Hook 配置不是 UTF-8")?;
        let bytes = match self.format {
            HookDocumentFormat::Json | HookDocumentFormat::Jsonc => rewrite_json_document(
                text,
                desired,
                matches!(self.format, HookDocumentFormat::Jsonc),
            )?
            .into_bytes(),
            HookDocumentFormat::Toml => render_toml(text, &self.root, desired)?,
        };
        Ok(Some(HookNativeWrite {
            source_id: self.source_id.clone(),
            bytes,
        }))
    }
}

/// Read a bounded verified snapshot without assigning any configuration syntax.
/// Dedicated line-based native policy files use the same access authority.
pub(crate) fn observe_bytes(
    snapshot: &MutationInventory,
    source_id: &str,
) -> Result<Option<Vec<u8>>, String> {
    let source = source(snapshot, source_id)?;
    if source.revision.is_missing {
        return Ok(None);
    }
    let anchor = snapshot
        .source_anchors
        .get(source_id)
        .ok_or("Hook 来源缺少已验证的读取证据")?;
    if anchor.revision().identity != source.revision.identity {
        return Err("Hook 来源在盘点后发生变化".to_owned());
    }
    let guard = reopen_verified_path(anchor).map_err(|_| "Hook 来源在盘点后发生变化")?;
    let bytes = guard
        .read_file_bounded(AgentAssetLimits::DEFAULT.bytes_per_source)
        .map_err(|_| "Hook 来源不能安全读取")?;
    if !anchor.matches_bytes(&bytes) {
        return Err("Hook 来源内容已经变化".to_owned());
    }
    guard
        .revalidate()
        .map_err(|_| "Hook 来源在读取期间发生变化")?;
    Ok(Some(bytes))
}

pub(crate) fn source<'a>(
    snapshot: &'a MutationInventory,
    id: &str,
) -> Result<&'a AgentAssetSource, String> {
    snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == id)
        .ok_or_else(|| "Hook 来源已失效".to_owned())
}

pub(crate) fn context<'a>(
    snapshot: &'a MutationInventory,
    source: &AgentAssetSource,
) -> Result<&'a AgentConfigurationContext, String> {
    snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.id == source.context_id)
        .ok_or_else(|| "Hook 配置上下文已失效".to_owned())
}

/// Structural admission is shared; only the adapter supplies the full native
/// role path. A package containing the same basename cannot become a target.
pub(crate) fn exact_source(
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
    scope: AgentAssetScope,
    path: &Path,
    origin: AgentAssetInstallationOrigin,
) -> bool {
    source.context_id == context.id
        && source.environment_id == context.environment_id
        && source.scope == scope
        && source.origin == origin
        && Path::new(&source.path) == path
        && source.source_kind == AgentAssetSourceKind::File
        && source.categories.contains(&AgentAssetCategory::Hook)
        && !source.revision.is_symlink
}

pub(crate) fn destination(source: &AgentAssetSource, role: &str) -> Option<HookNativeDestination> {
    (source.writable
        && matches!(
            source.scope,
            AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
        ))
    .then(|| HookNativeDestination {
        source_id: source.id.clone(),
        scope: source.scope,
        role: role.to_owned(),
    })
}

pub(crate) fn require_destination(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    targets: fn(
        &AgentEnvironmentInventory,
        &AgentConfigurationContext,
    ) -> Vec<HookNativeDestination>,
) -> Result<(), String> {
    let source = source(snapshot, &destination.source_id)?;
    let context = context(snapshot, source)?;
    if !targets(&snapshot.inventory, context).contains(destination) {
        return Err("Hook 目标不是该上下文的原生可写配置位置".to_owned());
    }
    Ok(())
}

pub(crate) struct HookSlot {
    pub native_id: String,
    pub event: String,
    pub group_index: usize,
    pub handler_index: usize,
}

pub(crate) fn read_rule(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
    inspect: fn(&MutationInventory, &str) -> Result<Vec<HookNativeRule>, String>,
) -> Result<HookNativeRule, String> {
    let declaration = definition_anchor(snapshot, asset)
        .filter(|declaration| declaration.native_kind == AgentAssetCategory::Hook)
        .ok_or("Hook 没有唯一且完整的定义来源")?;
    let mut rules = inspect(snapshot, &declaration.source_id)?
        .into_iter()
        .filter(|rule| rule.native_asset_id.as_deref() == Some(asset.stable_id.as_str()));
    let rule = rules.next().ok_or("Hook 原生定义已变化或不能精确定位")?;
    if rules.next().is_some() {
        return Err("Hook 原生定义存在歧义".to_owned());
    }
    Ok(rule)
}

pub(crate) fn rules_from_slots(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    root: &Value,
    slots: Vec<HookSlot>,
    default_enabled: AgentAssetDeclaredState,
) -> Result<Vec<HookNativeRule>, String> {
    let definitions = DefinitionIndex::new(snapshot);
    let mut assets = std::collections::BTreeMap::new();
    for asset in snapshot.inventory.assets.iter().filter(|asset| {
        asset.category == AgentAssetCategory::Hook
            && asset.context_id == source.context_id
            && asset.inspection_source_id == source.id
    }) {
        if definitions
            .anchor(asset)
            .is_some_and(|declaration| declaration.declaration_key == asset.native_id)
        {
            assets
                .entry(asset.native_id.as_str())
                .and_modify(|asset| *asset = None)
                .or_insert(Some(asset));
        }
    }
    let mut rules = Vec::new();
    for slot in slots {
        let group = root
            .get("hooks")
            .and_then(|events| events.get(&slot.event))
            .and_then(Value::as_array)
            .and_then(|groups| groups.get(slot.group_index))
            .ok_or("Hook matcher group 已变化")?;
        let handler = group
            .get("hooks")
            .and_then(Value::as_array)
            .and_then(|handlers| handlers.get(slot.handler_index))
            .ok_or("Hook handler 已变化")?;
        let mut selected = group
            .as_object()
            .ok_or("Hook matcher group 无效")?
            .iter()
            .filter(|(key, _)| key.as_str() != "hooks")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>();
        selected.insert("hooks".to_owned(), Value::Array(vec![handler.clone()]));
        let native_asset_id = assets
            .get(slot.native_id.as_str())
            .copied()
            .flatten()
            .map(|asset| asset.stable_id.clone());
        rules.push(HookNativeRule {
            source_id: source.id.clone(),
            native_asset_id,
            definition: HookNativeDefinition {
                event: slot.event.clone(),
                group: Value::Object(selected),
            },
            anchor: HookNativeAnchor {
                event: slot.event,
                group_index: slot.group_index,
                handler_index: slot.handler_index,
                original_group: group.clone(),
            },
            enabled: default_enabled,
        });
    }
    Ok(rules)
}

pub(crate) fn selected_handler(definition: &HookNativeDefinition) -> Result<&Value, String> {
    let handlers = definition
        .group
        .as_object()
        .and_then(|group| group.get("hooks"))
        .and_then(Value::as_array)
        .ok_or("Hook 定义必须包含原生 hooks 数组")?;
    if handlers.len() != 1 {
        return Err("共享 Hook 定义必须只包含所选的一条 handler".to_owned());
    }
    handlers
        .first()
        .ok_or_else(|| "Hook 定义没有 handler".to_owned())
}

fn metadata(group: &Value) -> Result<Map<String, Value>, String> {
    let mut metadata = group
        .as_object()
        .cloned()
        .ok_or("Hook matcher group 无效")?;
    metadata.remove("hooks");
    Ok(metadata)
}

fn matching_slots(
    root: &Value,
    definition: &HookNativeDefinition,
) -> Result<Vec<(usize, usize)>, String> {
    let Some(groups) = root
        .get("hooks")
        .and_then(|events| events.get(&definition.event))
    else {
        return Ok(Vec::new());
    };
    let groups = groups.as_array().ok_or("Hook 事件不是数组")?;
    let desired_meta = metadata(&definition.group)?;
    let handler = selected_handler(definition)?;
    let mut result = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        if metadata(group)? != desired_meta {
            continue;
        }
        let handlers = group
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or("Hook group 缺少数组")?;
        result.extend(
            handlers
                .iter()
                .enumerate()
                .filter(|(_, value)| *value == handler)
                .map(|(index, _)| (group_index, index)),
        );
    }
    Ok(result)
}

pub(crate) fn occurrences(
    root: &Value,
    definition: &HookNativeDefinition,
) -> Result<usize, String> {
    Ok(matching_slots(root, definition)?.len())
}

fn locate(root: &Value, original: &HookNativeRule) -> Result<(usize, usize), String> {
    if original.anchor.event != original.definition.event {
        return Err("Hook 恢复证据与定义不匹配".to_owned());
    }
    let slots = matching_slots(root, &original.definition)?;
    let exact_groups = slots
        .iter()
        .copied()
        .filter(|(group, _)| {
            root["hooks"][&original.anchor.event][*group] == original.anchor.original_group
        })
        .collect::<Vec<_>>();
    let candidates = if exact_groups.is_empty() {
        &slots
    } else {
        &exact_groups
    };
    match candidates.as_slice() {
        [slot] => Ok(*slot),
        [] => Err("Hook 原始节点已不存在，拒绝按旧索引修改".to_owned()),
        _ => Err("Hook 存在多个相同节点，不能精确确定修改目标".to_owned()),
    }
}

/// Native policy keys are array-position based in some Agents. Carry old slot
/// identity through the edit itself, so even identical siblings retain their
/// own policy and an index can never silently acquire another handler's trust.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HookLocation {
    pub event: String,
    pub group: usize,
    pub handler: usize,
}

pub(crate) struct HookDocumentEdit {
    pub root: Value,
    pub expectations: Vec<HookNativeExpectation>,
    pub locations: BTreeMap<HookLocation, Option<HookLocation>>,
}

type SlotKey = (String, usize, usize);
type TrackedGroups = BTreeMap<String, Vec<Vec<Option<HookLocation>>>>;

fn tracked_groups(root: &Value) -> TrackedGroups {
    root.get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(event, value)| {
            value.as_array().map(|groups| {
                (
                    event.clone(),
                    groups
                        .iter()
                        .enumerate()
                        .map(|(group, value)| {
                            value
                                .get("hooks")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .enumerate()
                                .map(|(handler, _)| {
                                    Some(HookLocation {
                                        event: event.clone(),
                                        group,
                                        handler,
                                    })
                                })
                                .collect()
                        })
                        .collect(),
                )
            })
        })
        .collect()
}

fn track_insert(
    groups: &mut TrackedGroups,
    location: &HookLocation,
    origin: Option<HookLocation>,
) -> Result<(), String> {
    let event = groups.entry(location.event.clone()).or_default();
    if location.group == event.len() {
        event.push(Vec::new());
    }
    let group = event
        .get_mut(location.group)
        .ok_or("Hook 插入位置缺少结构证据")?;
    if location.handler > group.len() {
        return Err("Hook 插入位置已变化".to_owned());
    }
    group.insert(location.handler, origin);
    Ok(())
}

/// Resolve all original nodes before changing any array, then mutate in reverse
/// index order. One source is rendered once even when several siblings change.
pub(crate) fn edit_document(
    document: &HookDocument,
    edits: &[HookNativeEdit],
    validate: fn(&HookNativeDefinition) -> Result<(), String>,
) -> Result<(Value, Vec<HookNativeExpectation>), String> {
    let result = edit_document_tracked(document, edits, validate)?;
    Ok((result.root, result.expectations))
}

pub(crate) fn edit_document_tracked(
    document: &HookDocument,
    edits: &[HookNativeEdit],
    validate: fn(&HookNativeDefinition) -> Result<(), String>,
) -> Result<HookDocumentEdit, String> {
    let mut root = document.root.clone();
    let mut tracked = tracked_groups(&root);
    let mut locations = tracked
        .values()
        .flatten()
        .flatten()
        .flatten()
        .cloned()
        .map(|location| (location, None))
        .collect::<BTreeMap<_, _>>();
    let mut changes = BTreeMap::<SlotKey, Option<HookNativeDefinition>>::new();
    let mut additions = Vec::new();
    let mut restorations = Vec::new();
    let mut expected = Vec::<HookNativeDefinition>::new();
    for edit in edits {
        let (original, replacement) = match edit {
            HookNativeEdit::Add { definition } => {
                validate(definition)?;
                additions.push((definition.clone(), None));
                expected.push(definition.clone());
                continue;
            }
            HookNativeEdit::Restore {
                original,
                definition,
            } => {
                validate(definition)?;
                require_original_source(document, original)?;
                restorations.push((original, definition));
                expected.push(definition.clone());
                continue;
            }
            HookNativeEdit::Remove { original } => (original, None),
            HookNativeEdit::Replace {
                original,
                definition,
            } => {
                validate(definition)?;
                expected.push(definition.clone());
                (original, Some(definition.clone()))
            }
            HookNativeEdit::SetEnabled { .. } => {
                return Err("原生 Hook 开关必须由对应 Agent 的策略编辑器处理".to_owned())
            }
        };
        require_original_source(document, original)?;
        let (group, handler) = locate(&root, original)?;
        let key = (original.definition.event.clone(), group, handler);
        if changes.insert(key, replacement).is_some() {
            return Err("同一 Hook 在本次批处理中存在相互冲突的修改".to_owned());
        }
        expected.push(original.definition.clone());
    }
    let mut moved = Vec::new();
    for ((event, group_index, handler_index), replacement) in changes.into_iter().rev() {
        let group = root
            .get_mut("hooks")
            .and_then(|events| events.get_mut(&event))
            .and_then(Value::as_array_mut)
            .and_then(|groups| groups.get_mut(group_index))
            .ok_or("Hook group 已变化")?;
        let keep_group = replacement.as_ref().is_some_and(|replacement| {
            replacement.event == event && metadata(group).ok() == metadata(&replacement.group).ok()
        });
        let handlers = group
            .get_mut("hooks")
            .and_then(Value::as_array_mut)
            .ok_or("Hook handler 数组已变化")?;
        if keep_group {
            handlers[handler_index] =
                selected_handler(replacement.as_ref().ok_or("Hook 替换定义缺失")?)?.clone();
        } else {
            handlers.remove(handler_index);
            let origin = tracked
                .get_mut(&event)
                .and_then(|groups| groups.get_mut(group_index))
                .ok_or("Hook 缺少位置证据")?
                .remove(handler_index);
            if let Some(replacement) = replacement {
                moved.push((replacement, origin));
            }
        }
    }
    // Keep the old relative order when changed metadata creates new groups.
    additions.extend(moved.into_iter().rev());
    for (original, definition) in restorations {
        if let Some(location) = restore(&mut root, original, definition)? {
            track_insert(&mut tracked, &location, None)?;
        }
    }
    for (definition, origin) in additions {
        match add(&mut root, &definition)? {
            Some(location) => track_insert(&mut tracked, &location, origin)?,
            None if origin.is_some() => {
                return Err("替换后的 Hook 与另一绑定重复，不能合并两条规则的原生状态".to_owned())
            }
            None => (),
        }
    }
    for (event, groups) in tracked {
        for (group, handlers) in groups.into_iter().enumerate() {
            for (handler, origin) in handlers.into_iter().enumerate() {
                if let Some(origin) = origin {
                    locations.insert(
                        origin,
                        Some(HookLocation {
                            event: event.clone(),
                            group,
                            handler,
                        }),
                    );
                }
            }
        }
    }
    let mut expectations = Vec::new();
    for definition in expected {
        if expectations
            .iter()
            .any(|expected: &HookNativeExpectation| expected.definition == definition)
        {
            continue;
        }
        expectations.push(HookNativeExpectation {
            source_id: document.source_id.clone(),
            occurrences: occurrences(&root, &definition)?,
            definition,
            enabled: None,
        });
    }
    Ok(HookDocumentEdit {
        root,
        expectations,
        locations,
    })
}

fn require_original_source(
    document: &HookDocument,
    original: &HookNativeRule,
) -> Result<(), String> {
    if original.source_id != document.source_id {
        return Err("Hook 编辑证据属于其他来源".to_owned());
    }
    let original_handler = original
        .anchor
        .original_group
        .get("hooks")
        .and_then(Value::as_array)
        .and_then(|handlers| handlers.get(original.anchor.handler_index))
        .ok_or("Hook 原始节点证据不完整")?;
    if original_handler != selected_handler(&original.definition)?
        || metadata(&original.anchor.original_group)? != metadata(&original.definition.group)?
    {
        return Err("Hook 原始节点与恢复副本不匹配".to_owned());
    }
    Ok(())
}

fn add(
    root: &mut Value,
    definition: &HookNativeDefinition,
) -> Result<Option<HookLocation>, String> {
    match occurrences(root, definition)? {
        1 => return Ok(None),
        0 => (),
        _ => return Err("目标已有多条相同 Hook，需先明确原生绑定".to_owned()),
    }
    let events = root
        .as_object_mut()
        .ok_or("Hook 配置根节点无效")?
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("Hook 事件表无效")?;
    let groups = events
        .entry(&definition.event)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("Hook 事件数组无效")?;
    let group = groups.len();
    groups.push(definition.group.clone());
    Ok(Some(HookLocation {
        event: definition.event.clone(),
        group,
        handler: 0,
    }))
}

fn restore(
    root: &mut Value,
    original: &HookNativeRule,
    definition: &HookNativeDefinition,
) -> Result<Option<HookLocation>, String> {
    match occurrences(root, definition)? {
        1 => return Ok(None),
        0 => (),
        _ => return Err("恢复目标存在多个相同 Hook，已保留副本且未写入".to_owned()),
    }
    if occurrences(root, &original.definition)? != 0 {
        return Err("原 Hook 已被恢复或由外部修改，不能重复插入新版本".to_owned());
    }
    let groups = root
        .get("hooks")
        .and_then(|events| events.get(&original.anchor.event))
        .and_then(Value::as_array)
        .ok_or("原 Hook 的事件已移除，不能确定恢复位置")?;
    let original_meta = metadata(&original.anchor.original_group)?;
    let original_handlers = original
        .anchor
        .original_group
        .get("hooks")
        .and_then(Value::as_array)
        .ok_or("Hook 恢复证据无效")?;
    let mut positions = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        if metadata(group)? != original_meta {
            continue;
        }
        let handlers = group
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or("Hook group 无效")?;
        if let Some(position) =
            restore_position(original_handlers, original.anchor.handler_index, handlers)
        {
            positions.push((group_index, position));
        }
    }
    let [(group_index, position)] = positions.as_slice() else {
        return Err("原 Hook 的邻项证据已变化或存在歧义，恢复副本已保留".to_owned());
    };
    if definition.event != original.anchor.event || metadata(&definition.group)? != original_meta {
        return add(root, definition);
    }
    let handlers = root
        .get_mut("hooks")
        .and_then(|events| events.get_mut(&original.anchor.event))
        .and_then(Value::as_array_mut)
        .and_then(|groups| groups.get_mut(*group_index))
        .and_then(|group| group.get_mut("hooks"))
        .and_then(Value::as_array_mut)
        .ok_or("Hook 恢复目标已变化")?;
    handlers.insert(*position, selected_handler(definition)?.clone());
    Ok(Some(HookLocation {
        event: definition.event.clone(),
        group: *group_index,
        handler: *position,
    }))
}

fn restore_position(original: &[Value], selected: usize, current: &[Value]) -> Option<usize> {
    if current.is_empty() {
        return Some(0);
    }
    let mut previous = None;
    let mut before = None;
    let mut after = None;
    for (index, sibling) in original
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != selected)
    {
        let positions = current
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == sibling)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if positions.len() > 1 {
            return None;
        }
        let Some(position) = positions.first().copied() else {
            continue;
        };
        if previous.is_some_and(|previous| position <= previous) {
            return None;
        }
        previous = Some(position);
        if index < selected {
            before = Some(position);
        } else if after.is_none() {
            after = Some(position);
        }
    }
    before.map(|before| before + 1).or(after)
}

pub(crate) fn affected_source_assets(snapshot: &MutationInventory, source_id: &str) -> Vec<String> {
    snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Hook && asset.inspection_source_id == source_id
        })
        .map(|asset| asset.stable_id.clone())
        .collect()
}

pub(crate) fn prepare_structural(
    snapshot: &MutationInventory,
    destination: &HookNativeDestination,
    edits: &[HookNativeEdit],
    format: HookDocumentFormat,
    validate: fn(&HookNativeDefinition) -> Result<(), String>,
) -> Result<HookNativePrepared, String> {
    let document = HookDocument::capture(snapshot, &destination.source_id, format)?;
    let (root, expectations) = edit_document(&document, edits, validate)?;
    Ok(HookNativePrepared {
        required_capabilities: vec![HookNativeCapability::Configuration],
        read_source_ids: vec![destination.source_id.clone()],
        writes: document.render(&root)?.into_iter().collect(),
        expectations,
        affected_asset_ids: affected_source_assets(snapshot, &destination.source_id),
        notes: vec!["仅修改选定 Hook；既有进程可能需要重新加载配置".to_owned()],
    })
}
