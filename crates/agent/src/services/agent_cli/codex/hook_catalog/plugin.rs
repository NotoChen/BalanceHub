//! Package Hook contents are readable/adoptable through their verified native
//! parent, but are never exposed as writable library destinations.
use super::super::environment::plugin_hooks::{self, Component};
use super::*;

pub(super) fn rules(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    context: &AgentConfigurationContext,
) -> Result<Vec<HookNativeRule>, String> {
    if source.origin != AgentAssetInstallationOrigin::NativePackage
        || source.writable
        || source.revision.is_symlink
        || source.source_kind != AgentAssetSourceKind::File
        || source.allowed_root != context.config_root
    {
        return Err("来源不是已验证的 Codex Plugin Hook 文件".to_owned());
    }
    let parent_ids = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.category == AgentAssetCategory::Hook
                && asset.context_id == source.context_id
                && asset.inspection_source_id == source.id
        })
        .filter_map(|asset| asset.relationships.provided_by.as_deref())
        .collect::<BTreeSet<_>>();
    let policy = policy_source(snapshot, context)?;
    let states = hooks::decode_states(
        &HookDocument::observe(snapshot, &policy.id, HookDocumentFormat::Toml)?.root,
    );
    let document = HookDocument::observe(snapshot, &source.id, HookDocumentFormat::Json)?;
    let mut result = Vec::new();
    for parent_id in parent_ids {
        let parent = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| {
                asset.stable_id == parent_id
                    && asset.context_id == context.id
                    && asset.category == AgentAssetCategory::Plugin
            })
            .ok_or("Codex Plugin Hook 父级已变化")?;
        let root = plugin_hooks::package_root(
            Path::new(&context.config_root),
            &parent.native_id,
            Path::new(&source.path),
        )
        .ok_or("Codex Plugin Hook 安装目录无效")?;
        let manifests = snapshot
            .inventory
            .sources
            .iter()
            .filter_map(|candidate| {
                if !parent.source_ids.contains(&candidate.id)
                    || candidate.context_id != context.id
                    || candidate.revision.is_missing
                    || candidate.origin != AgentAssetInstallationOrigin::NativePackage
                {
                    return None;
                }
                [
                    ("codex", ".codex-plugin/plugin.json"),
                    ("claude", ".claude-plugin/plugin.json"),
                    ("cursor", ".cursor-plugin/plugin.json"),
                ]
                .into_iter()
                .find_map(|(format, relative)| {
                    (Path::new(&candidate.path) == root.join(relative))
                        .then_some((candidate, format))
                })
            })
            .collect::<Vec<_>>();
        let [(manifest_source, format)] = manifests.as_slice() else {
            return Err("Codex Plugin Hook 清单缺失或不唯一".to_owned());
        };
        let manifest =
            HookDocument::observe(snapshot, &manifest_source.id, HookDocumentFormat::Json)?;
        let components = plugin_hooks::manifest_components(&manifest.root, &root, format)
            .ok_or("Codex Plugin Hook 清单已变化")?;
        for (index, component) in components.iter().enumerate() {
            let raw = match component {
                Component::File(path) if Path::new(&source.path) == path => &document.root,
                Component::Inline(value) if source.id == manifest_source.id => value,
                _ => continue,
            };
            if components
                .iter()
                .filter(|candidate| *candidate == component)
                .count()
                != 1
                && matches!(component, Component::File(_))
            {
                return Err("Codex Plugin Hook 文件重复声明，不能唯一定位".to_owned());
            }
            let raw_root = raw
                .as_object()
                .filter(|root| hooks::valid_document(root, true))
                .ok_or("Codex Plugin Hook 原生文档无效")?;
            let key_source = plugin_hooks::key_source(&parent.native_id, &root, component, index)
                .ok_or("Codex Plugin Hook 原生键无效")?;
            let mut slots = Vec::new();
            if !hooks::walk_slots(raw_root, |event, group, handler| {
                slots.push(HookSlot {
                    native_id: hooks::native_id(&key_source, event, group, handler),
                    event: event.to_owned(),
                    group_index: group,
                    handler_index: handler,
                });
                true
            }) {
                return Err("Codex Plugin Hook 包含不能完整解码的规则".to_owned());
            }
            for mut rule in hook_codec::rules_from_slots(
                snapshot,
                source,
                raw,
                slots,
                AgentAssetDeclaredState::Enabled,
            )? {
                if rule.native_asset_id.is_none() {
                    continue;
                }
                let key = hooks::native_id(
                    &key_source,
                    &rule.anchor.event,
                    rule.anchor.group_index,
                    rule.anchor.handler_index,
                );
                let handler = hook_codec::selected_handler(&rule.definition)?;
                if !plugin_hooks::builtin(
                    &parent.native_id,
                    &rule.anchor.event,
                    &rule.anchor.original_group,
                    handler,
                ) && states.get(&key).and_then(|state| state.enabled) == Some(false)
                {
                    rule.enabled = AgentAssetDeclaredState::Disabled;
                }
                result.push(rule);
            }
        }
    }
    Ok(result)
}
