use super::{
    super::{
        digest, native::NativeCatalogAdapter, package, projection, removal, repository::Receipt,
        source_index::definition_anchor,
    },
    remove_skill::SkillRemoval,
};
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::mutation::{atomic, GuardedFile, MutationInspector, MutationInventory},
    },
};
use std::path::PathBuf;

pub(super) struct PreparedRemoval {
    pub signature: String,
    pub domains: Vec<String>,
    pub changes: Vec<AgentAssetPlanChange>,
    pub affected_asset_ids: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    name: String,
    context: AgentConfigurationContext,
    scope: AgentAssetScope,
    kind: RemovalKind,
    receipt_targets: Vec<(String, AgentAssetScope, String, String)>,
}

enum RemovalKind {
    Mcp {
        file: GuardedFile,
        after: Vec<u8>,
        adapter: &'static NativeCatalogAdapter,
    },
    Skill(SkillRemoval),
}

impl PreparedRemoval {
    pub fn prepare(
        snapshot: &MutationInventory,
        inspector: &dyn MutationInspector,
        item: &AgentCatalogAsset,
        id: &str,
    ) -> Result<Self, String> {
        let binding = item
            .bindings
            .iter()
            .find(|binding| binding.id == id)
            .ok_or("所选原生配置已不存在，请刷新后核对")?;
        let native = &binding.native;
        let source = removal::source(snapshot, native)?;
        let context = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| context.id == native.context_id)
            .ok_or("Agent 配置上下文已变化")?
            .clone();
        let allowed = std::path::Path::new(&source.allowed_root);
        let root = if allowed.starts_with(inspector.home()) {
            inspector.home()
        } else if inspector
            .workspace()
            .is_some_and(|workspace| allowed.starts_with(workspace))
        {
            inspector.workspace().ok_or("工作区已变化")?
        } else {
            return Err("配置不在当前用户或项目范围内".to_owned());
        }
        .to_path_buf();
        let anchor = snapshot
            .source_anchors
            .get(&source.id)
            .ok_or("来源缺少当前文件证据")?;
        if anchor.revision().identity != source.revision.identity {
            return Err("来源文件已变化".to_owned());
        }
        let (path, kind, changes, mut domains, evidence) = if native.category
            == AgentAssetCategory::Mcp
        {
            let adapter = definition(native.agent_kind)
                .environment
                .catalog_adapter()
                .ok_or("该 Agent 未提供 MCP 文档适配器")?;
            let declaration = definition_anchor(snapshot, native).ok_or("MCP 声明不明确")?;
            let file = GuardedFile::capture(source, &snapshot.source_anchors)
                .map_err(|error| error.message)?;
            let before = file.bytes().ok_or("MCP 文件已缺失")?;
            let document = adapter.parse_document(before).ok_or("MCP 文件无效")?;
            adapter.declaration_value(&document, declaration, &context)?;
            let after = adapter.remove_mcp_at(before, &native.native_id, &context, native.scope)?;
            let changes = vec![AgentAssetPlanChange {
                label: format!("移除 MCP · {}", native.native_id),
                path: Some(source.path.clone()),
                before: Some(
                    std::str::from_utf8(before)
                        .map_err(|_| "MCP 文件不是 UTF-8")?
                        .to_owned(),
                ),
                after: Some(
                    std::str::from_utf8(&after)
                        .map_err(|_| "MCP 文件不是 UTF-8")?
                        .to_owned(),
                ),
            }];
            let domains = file.lock_domains();
            let evidence = vec![file.signature()];
            (
                PathBuf::from(&source.path),
                RemovalKind::Mcp {
                    file,
                    after,
                    adapter,
                },
                changes,
                domains,
                evidence,
            )
        } else {
            let skill = SkillRemoval::prepare(&root, anchor)?;
            let path = std::path::Path::new(&source.path)
                .parent()
                .ok_or("Skill 目录无效")?
                .to_path_buf();
            let changes = skill.changes();
            let evidence = skill.signatures();
            let mut domains = vec![
                format!("catalog-directory:{}", path.display()),
                format!("catalog-directory:{}", skill.physical_path().display()),
            ];
            // Coordinate with existing file writers for every previewed member.
            if !anchor.is_readonly_reference() {
                for change in &changes {
                    if let Some(path) = &change.path {
                        domains.extend(
                            package::capture_file(
                                &root,
                                std::path::Path::new(path),
                                package::MAX_FILE_BYTES,
                            )?
                            .lock_domains(),
                        );
                    }
                }
            }
            (path, RemovalKind::Skill(skill), changes, domains, evidence)
        };
        domains.push(format!("config:{}", context.config_root));
        domains.extend(
            context
                .compatible_installation_ids
                .iter()
                .map(|id| format!("installation:{id}")),
        );
        let affected_asset_ids = snapshot
            .inventory
            .assets
            .iter()
            .filter(|other| {
                if other.category != native.category {
                    return false;
                }
                let Some(other_source) = projection::definition_source(snapshot, other) else {
                    return false;
                };
                if anchor.is_readonly_reference() {
                    return other_source.path == source.path;
                }
                let same_file =
                    projection::source_path(snapshot, other_source) == anchor.physical_path();
                same_file
                    && (native.category == AgentAssetCategory::Skill
                        || (other.native_id == native.native_id
                            && other.scope == native.scope
                            && (native.scope != AgentAssetScope::Local
                                || snapshot
                                    .inventory
                                    .contexts
                                    .iter()
                                    .find(|context| context.id == other.context_id)
                                    .is_some_and(|other| {
                                        other.workspace_id == context.workspace_id
                                    }))))
            })
            .map(|asset| asset.stable_id.clone())
            .collect::<Vec<_>>();
        let receipt_targets = snapshot
            .inventory
            .assets
            .iter()
            .filter(|asset| affected_asset_ids.contains(&asset.stable_id))
            .filter_map(|asset| {
                let source = projection::definition_source(snapshot, asset)?;
                let path = std::path::Path::new(&source.path);
                let path = if asset.category == AgentAssetCategory::Skill {
                    path.parent()?
                } else {
                    path
                };
                Some((
                    asset.context_id.clone(),
                    asset.scope,
                    asset.native_id.clone(),
                    path.to_string_lossy().into_owned(),
                ))
            })
            .collect::<Vec<_>>();
        let signature = digest(
            &serde_json::to_vec(&(
                &source.path,
                &native.native_id,
                &context,
                native.scope,
                &evidence,
                &changes,
                &affected_asset_ids,
                &receipt_targets,
            ))
            .map_err(|_| "移除计划无法编码")?,
        );
        Ok(Self {
            signature,
            domains,
            changes,
            affected_asset_ids,
            root,
            path,
            name: native.native_id.clone(),
            context,
            scope: native.scope,
            kind,
            receipt_targets,
        })
    }

    pub fn group_key(&self) -> String {
        match &self.kind {
            RemovalKind::Mcp { file, .. } => format!("mcp:{}", file.domain()),
            RemovalKind::Skill(skill) => format!("skill:{}", skill.physical_path().display()),
        }
    }

    pub fn retained_private_bytes(&self) -> usize {
        super::cache::encoded_bytes(&self.changes).saturating_add(match &self.kind {
            RemovalKind::Mcp { file, after, .. } => file
                .bytes()
                .map_or(0, <[u8]>::len)
                .saturating_add(after.len()),
            RemovalKind::Skill(skill) => skill.bytes(),
        })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        match &self.kind {
            RemovalKind::Mcp { file, .. } => file.revalidate().map_err(|error| error.message),
            RemovalKind::Skill(skill) => skill.revalidate(),
        }
    }

    pub fn validate_group(targets: &[(usize, Self)]) -> Result<(), String> {
        let (_, first) = targets.first().ok_or("移除目标为空")?;
        if let RemovalKind::Mcp { file, adapter, .. } = &first.kind {
            if targets
                .iter()
                .any(|(_, target)| target.path != first.path || target.root != first.root)
            {
                return Err("MCP 移除目标存在物理别名，请分别操作".to_owned());
            }
            for (_, target) in targets {
                match &target.kind {
                    RemovalKind::Mcp {
                        file: other,
                        adapter: other_adapter,
                        ..
                    } if other.bytes() == file.bytes() && other_adapter.key == adapter.key => {
                        Ok(())
                    }
                    _ => Err("同一文档存在不一致的移除前置条件".to_owned()),
                }?;
            }
            Self::compose(targets, file, adapter)?;
        } else {
            let mut paths = std::collections::BTreeMap::new();
            for (_, target) in targets {
                if target.group_key() != first.group_key() {
                    return Err("Skill 物理来源不一致".to_owned());
                }
                if let Some(changes) = paths.insert(&target.path, &target.changes) {
                    if changes != &target.changes {
                        return Err("同一 Skill 入口的移除内容不一致".to_owned());
                    }
                }
            }
        }
        Ok(())
    }

    fn compose(
        targets: &[(usize, Self)],
        file: &GuardedFile,
        adapter: &NativeCatalogAdapter,
    ) -> Result<Vec<u8>, String> {
        let mut after = file.bytes().ok_or("MCP 文件缺失")?.to_vec();
        // Apply exact removals sequentially through the same native codec. This
        // preserves JSONC comments and TOML formatting, including shared aliases.
        for (_, target) in targets {
            let document = adapter.parse_document(&after).ok_or("MCP 文件无效")?;
            if adapter
                .mcp_node_checked(&document, &target.context, target.scope, &target.name)?
                .is_some()
            {
                after =
                    adapter.remove_mcp_at(&after, &target.name, &target.context, target.scope)?;
            }
        }
        Ok(after)
    }

    pub fn apply_group(
        targets: &[(usize, Self)],
        before_commit: impl Fn() -> Result<(), AgentAssetMutationError>,
    ) -> Result<(), String> {
        Self::validate_group(targets)?;
        for (_, target) in targets {
            target.revalidate()?;
        }
        let (_, first) = targets.first().ok_or("移除目标为空")?;
        match &first.kind {
            RemovalKind::Skill(_) => {
                let mut members = targets
                    .iter()
                    .filter_map(|(_, target)| match &target.kind {
                        RemovalKind::Skill(skill) => Some((&target.path, skill)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                // Unlink selected aliases before deleting a shared source they
                // refer to. Multiple Agent consumers of one path commit once.
                members.sort_by_key(|(_, skill)| !skill.is_reference());
                let mut removed = std::collections::BTreeSet::new();
                for (path, skill) in members {
                    if removed.insert(path) {
                        skill.apply(|| before_commit().map_err(|error| error.message))?;
                    }
                }
                Ok(())
            }
            RemovalKind::Mcp { file, adapter, .. } => {
                let after = Self::compose(targets, file, adapter)?;
                let result =
                    atomic::replace(file, &after, before_commit).map_err(|error| error.message)?;
                #[cfg(unix)]
                if matches!(result, atomic::AtomicWriteResult::ReplacedNotSynced) {
                    return Err("MCP 配置已移除，但目录同步失败，请刷新核实".to_owned());
                }
                #[cfg(not(unix))]
                let _ = result;
                let current =
                    package::capture_file(&first.root, &first.path, package::MAX_FILE_BYTES)?;
                if current.bytes() != Some(after.as_slice()) {
                    return Err("MCP 移除后文档内容未能确认".to_owned());
                }
                Ok(())
            }
        }
    }

    pub fn verify(&self) -> Result<bool, String> {
        match &self.kind {
            RemovalKind::Skill(skill) => skill.absent(),
            RemovalKind::Mcp { adapter, .. } => {
                let current =
                    package::capture_file(&self.root, &self.path, package::MAX_FILE_BYTES)?;
                let document = adapter
                    .parse_document(current.bytes().ok_or("MCP 文件缺失")?)
                    .ok_or("MCP 文件无效")?;
                Ok(adapter
                    .mcp_node_checked(&document, &self.context, self.scope, &self.name)?
                    .is_none())
            }
        }
    }

    pub fn matches_receipt(&self, receipt: &Receipt) -> bool {
        self.receipt_targets
            .iter()
            .any(|(context, scope, name, path)| {
                receipt.context_id == *context
                    && receipt.scope == *scope
                    && (matches!(self.kind, RemovalKind::Skill(_)) || receipt.name == *name)
                    && receipt.path == *path
            })
    }
}
