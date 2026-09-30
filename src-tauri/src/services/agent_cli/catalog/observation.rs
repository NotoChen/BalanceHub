//! One observation transaction owns its native reads. Bindings select from the
//! same source document/rule list/package instead of reopening it per row.
use super::{
    definition::DefinitionPayload,
    identity::CurrentDefinition,
    native::hooks::{HookNativeRule, NativeHookAdapter},
    package,
    source_index::DefinitionIndex,
};
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::{
            mutation::{GuardedFile, MutationInventory},
            verified_path::{reopen_verified_path, VerifiedPathAnchor},
        },
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type PackageKey = (PathBuf, PathBuf);
type Payload = Result<Arc<DefinitionPayload>, String>;

pub(super) struct DefinitionReader<'a> {
    pub(super) snapshot: &'a MutationInventory,
    pub(super) index: DefinitionIndex<'a>,
    mcp: BTreeMap<String, Result<Value, String>>,
    hooks: BTreeMap<(AgentCliKind, String), Result<Arc<HookSource>, String>>,
    packages: BTreeMap<PackageKey, Payload>,
    package_anchors: Vec<VerifiedPathAnchor>,
    package_cache: Option<&'a Mutex<super::package_cache::PackageCache>>,
    document_bytes: usize,
    deadline: Instant,
}

pub(super) struct HookSource {
    pub rules: Vec<HookNativeRule>,
    bindings: BTreeMap<String, Option<usize>>,
}

impl<'a> DefinitionReader<'a> {
    pub(super) fn new(snapshot: &'a MutationInventory) -> Self {
        Self {
            snapshot,
            index: DefinitionIndex::new(snapshot),
            mcp: BTreeMap::new(),
            hooks: BTreeMap::new(),
            packages: BTreeMap::new(),
            package_anchors: Vec::new(),
            package_cache: None,
            document_bytes: 0,
            deadline: Instant::now() + Duration::from_secs(10),
        }
    }

    pub(super) fn with_package_cache(
        snapshot: &'a MutationInventory,
        cache: &'a Mutex<super::package_cache::PackageCache>,
    ) -> Self {
        Self {
            package_cache: Some(cache),
            ..Self::new(snapshot)
        }
    }

    pub(super) fn package_anchors(&self) -> &[VerifiedPathAnchor] {
        &self.package_anchors
    }

    pub(super) fn hook_source(
        &mut self,
        kind: AgentCliKind,
        id: &str,
    ) -> Result<Arc<HookSource>, String> {
        let key = (kind, id.to_owned());
        if !self.hooks.contains_key(&key) {
            let result = (|| {
                if Instant::now() >= self.deadline {
                    return Err("完整定义读取达到 10 秒预算".to_owned());
                }
                let adapter = definition(kind)
                    .environment
                    .hook_adapter()
                    .ok_or("该 Agent 尚未注册原生 Hook 读取")?;
                let rules = (adapter.inspect_source)(self.snapshot, id)?;
                let mut bindings = BTreeMap::new();
                for (index, rule) in rules.iter().enumerate() {
                    if let Some(id) = &rule.native_asset_id {
                        bindings
                            .entry(id.clone())
                            .and_modify(|index| *index = None)
                            .or_insert(Some(index));
                    }
                }
                Ok(Arc::new(HookSource { rules, bindings }))
            })();
            self.hooks.insert(key.clone(), result);
        }
        self.hooks[&key].clone()
    }

    pub(super) fn hook(&mut self, asset: &AgentAssetRecord) -> Result<HookNativeRule, String> {
        let declaration = self
            .index
            .anchor(asset)
            .filter(|item| item.native_kind == AgentAssetCategory::Hook)
            .ok_or("Hook 没有唯一且完整的定义来源")?;
        let source = self.hook_source(asset.agent_kind, &declaration.source_id)?;
        let index = source
            .bindings
            .get(&asset.stable_id)
            .ok_or("Hook 原生定义已变化或不能精确定位")?
            .ok_or("Hook 原生定义存在歧义")?;
        Ok(source.rules[index].clone())
    }

    pub(super) fn payload(&mut self, asset: &AgentAssetRecord) -> Payload {
        if asset.relationships.provided_by.is_some() && asset.category != AgentAssetCategory::Hook {
            return Err("资源由插件提供，请通过父插件管理".to_owned());
        }
        match asset.category {
            AgentAssetCategory::Hook => {
                let rule = self.hook(asset)?;
                adoptable_hook_payload(asset, &rule).map(Arc::new)
            }
            AgentAssetCategory::Mcp => {
                let adapter = definition(asset.agent_kind)
                    .environment
                    .catalog_adapter()
                    .ok_or("该生态没有共享映射")?;
                let declaration = self
                    .index
                    .anchor(asset)
                    .ok_or("MCP 没有唯一且匹配作用域的完整定义，不能收录")?;
                let context = self
                    .index
                    .context(&asset.context_id)
                    .ok_or("MCP 配置上下文已失效")?;
                let source = self.index.source(asset).ok_or("MCP 来源已失效")?;
                if !self.mcp.contains_key(&source.id) {
                    let result = (|| {
                        let file = GuardedFile::capture(source, &self.snapshot.source_anchors)
                            .map_err(|_| "MCP 文件已变化或不能安全读取")?;
                        let bytes = file.bytes().ok_or("MCP 来源不存在")?;
                        self.document_bytes = self.document_bytes.saturating_add(bytes.len());
                        if self.document_bytes > 64 * 1024 * 1024 {
                            return Err("配置文件读取达到 64 MiB 预算".to_owned());
                        }
                        adapter
                            .parse_document(bytes)
                            .ok_or_else(|| "MCP 配置无效或含重复字段".to_owned())
                    })();
                    self.mcp.insert(source.id.clone(), result);
                }
                let root = self.mcp[&source.id].as_ref().map_err(Clone::clone)?;
                adapter
                    .decode(adapter.declaration_value(root, declaration, context)?)
                    .map(|value| Arc::new(DefinitionPayload::Mcp(value)))
            }
            AgentAssetCategory::Skill => self.skill(asset),
            _ => Err("保留原生生态与来源；不提供跨生态分发".to_owned()),
        }
    }

    fn skill(&mut self, asset: &AgentAssetRecord) -> Payload {
        let source = self
            .index
            .source(asset)
            .ok_or("Skill 没有完整原生来源证据")?;
        let anchor = self
            .snapshot
            .source_anchors
            .get(&source.id)
            .ok_or("Skill 来源没有通过路径校验")?;
        // Each alias still proves its own link and allowed root before sharing
        // the physical package. Paths alone never authorize a reused payload.
        let guard = reopen_verified_path(anchor).map_err(|_| "Skill 来源已变化")?;
        let (path, allowed_root) = guard.verified_read_path();
        let root = if path.file_name().is_some_and(|name| name == "SKILL.md") {
            path.parent().ok_or("Skill 包目录无效")?
        } else if source.source_kind == AgentAssetSourceKind::Directory {
            path
        } else {
            return Err("Skill 没有完整包目录".to_owned());
        };
        let key = (root.to_owned(), allowed_root.to_owned());
        if !self.packages.contains_key(&key) {
            let package = match self.package_cache {
                Some(cache) => cache
                    .lock()
                    .map_err(|_| "Skill 读取缓存不可用".to_owned())?
                    .read(root, allowed_root),
                None => package::read_package(root, allowed_root).map(Arc::new),
            };
            let result = package.map(|package| {
                self.package_anchors.extend(package.anchors.iter().cloned());
                Arc::new(DefinitionPayload::Skill(package.files.clone()))
            });
            self.packages.insert(key.clone(), result);
        }
        let payload = self.packages[&key].clone()?;
        if source.source_kind == AgentAssetSourceKind::File {
            let DefinitionPayload::Skill(files) = payload.as_ref() else {
                unreachable!()
            };
            if !files
                .get("SKILL.md")
                .is_some_and(|file| anchor.matches_bytes(&file.bytes))
            {
                return Err("Skill 来源已变化".to_owned());
            }
        }
        guard
            .revalidate()
            .map_err(|_| "Skill 来源在读取期间发生变化")?;
        Ok(payload)
    }

    pub(super) fn physical_key(&self, asset: &AgentAssetRecord) -> Option<String> {
        let snapshot = self.snapshot;
        let definition = self.index.anchor(asset)?;
        let source = self.index.source(asset)?;
        if source.id != definition.source_id {
            return None;
        }
        let anchor = snapshot.source_anchors.get(&source.id)?;
        let guard = reopen_verified_path(anchor).ok()?;
        let (mut path, allowed_root) = guard.verified_read_path();
        let physical = if asset.category == AgentAssetCategory::Skill {
            if path.file_name().is_some_and(|name| name == "SKILL.md") {
                path = path.parent()?;
            }
            let directory =
                crate::services::agent_cli::environment::verified_path::inspect_verified_path(
                    &[allowed_root],
                    allowed_root,
                    path,
                    AgentAssetSourceKind::Directory,
                )
                .ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = directory.metadata().ok()?;
                directory.revalidate().ok()?;
                format!("{}:{}", metadata.dev(), metadata.ino())
            }
            #[cfg(not(unix))]
            {
                directory.revalidate().ok()?;
                path.to_string_lossy().to_lowercase()
            }
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = guard.metadata().ok()?;
                format!("{}:{}", metadata.dev(), metadata.ino())
            }
            #[cfg(not(unix))]
            {
                path.to_string_lossy().to_lowercase()
            }
        };
        guard.revalidate().ok()?;
        if asset.category == AgentAssetCategory::Skill {
            return Some(format!("skill:{physical}"));
        }
        Some(format!(
            "{}:{physical}:{}:{}",
            asset.category.key(),
            asset.native_id,
            definition.declaration_key
        ))
    }

    pub(super) fn observe(
        &mut self,
        asset: &AgentAssetRecord,
        deadline: Instant,
        remaining_bytes: &mut usize,
    ) -> CurrentDefinition {
        let mut hook = None;
        let mut hook_bytes = 0;
        let (mut payload, mut fingerprint) = if *remaining_bytes == 0 {
            (
                Err("全局完整定义比较达到 64 MiB 预算，部分内容一致性未知".to_owned()),
                None,
            )
        } else if Instant::now() >= deadline {
            (
                Err("全局比较达到 10 秒预算，部分内容一致性未知".to_owned()),
                None,
            )
        } else if asset.category == AgentAssetCategory::Hook {
            match self.hook(asset) {
                Ok(rule) => {
                    let fingerprint = super::hook_definition::fingerprint(&rule.definition);
                    hook = super::hook_summary::rule(&rule.definition);
                    hook_bytes = serde_json::to_vec(&rule.definition)
                        .map_or(usize::MAX, |bytes| bytes.len());
                    (
                        adoptable_hook_payload(asset, &rule).map(Arc::new),
                        Some(fingerprint),
                    )
                }
                Err(reason) => (Err(reason), None),
            }
        } else {
            let payload = self.payload(asset);
            let fingerprint = payload
                .as_ref()
                .ok()
                .and_then(|payload| payload.binding_fingerprint(asset.agent_kind));
            (payload, fingerprint)
        };
        let bytes = payload
            .as_ref()
            .map_or(0, |payload| payload.comparison_bytes())
            .max(hook_bytes);
        if bytes > *remaining_bytes {
            *remaining_bytes = 0;
            payload = Err("全局完整定义比较达到 64 MiB 预算，部分内容一致性未知".to_owned());
            fingerprint = None;
            hook = None;
        } else {
            *remaining_bytes -= bytes;
        }
        let source = self.index.source(asset);
        CurrentDefinition {
            payload,
            fingerprint,
            physical_key: self.physical_key(asset),
            source_path: source.map(|source| source.path.clone()),
            hook,
        }
    }
}

pub(super) fn observe_payload(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
) -> Result<DefinitionPayload, String> {
    let payload = DefinitionReader::new(snapshot).payload(asset)?;
    Ok(Arc::unwrap_or_clone(payload))
}

fn adoptable_hook_payload(
    asset: &AgentAssetRecord,
    rule: &HookNativeRule,
) -> Result<DefinitionPayload, String> {
    let adapter: &NativeHookAdapter = definition(asset.agent_kind)
        .environment
        .hook_adapter()
        .ok_or("该 Agent 尚未注册原生 Hook 读取")?;
    if asset.relationships.provided_by.is_some() || asset.relationships.action_owner.is_some() {
        return Err("此 Hook 依赖父插件的执行目录和运行环境，无法独立复制为共享定义；可查看原生配置并使用可用的启停操作".to_owned());
    }
    (adapter.validate_adoption)(rule)?;
    (adapter.validate_definition)(&rule.definition)?;
    Ok(DefinitionPayload::Hook(BTreeMap::from([(
        asset.agent_kind,
        rule.definition.clone(),
    )])))
}
