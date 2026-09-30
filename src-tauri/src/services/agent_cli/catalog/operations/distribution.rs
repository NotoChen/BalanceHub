use super::super::{
    definition::{DefinitionPayload, StoredDefinition},
    package, projection,
    repository::Receipt,
};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        atomic, execution::ExecutableStamp, GuardedDirectory, GuardedFile, MutationInspector,
        MutationInventory, WriteObservation,
    },
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) struct PreparedDistribution {
    pub destination: PathBuf,
    pub signature: String,
    pub write_signature: String,
    pub domains: Vec<String>,
    pub changes: Vec<AgentAssetPlanChange>,
    pub affected_asset_ids: Vec<String>,
    pub context_id: String,
    scope: AgentAssetScope,
    agent_kind: AgentCliKind,
    pub native_name: String,
    pub version: u64,
    pub fingerprint: String,
    root: PathBuf,
    parent: GuardedDirectory,
    package: Option<package::PackageSnapshot>,
    writes: Vec<DistributionWrite>,
    desired: DefinitionPayload,
    adapter: &'static super::super::native::NativeCatalogAdapter,
    source_path: PathBuf,
    installations: Vec<ExecutableStamp>,
    context: AgentConfigurationContext,
}

struct DistributionWrite {
    file: GuardedFile,
    bytes: Vec<u8>,
    executable: Option<bool>,
}

struct ComposedDistributionWrite {
    root: PathBuf,
    write: DistributionWrite,
}

impl PreparedDistribution {
    pub(super) fn retained_private_bytes(&self) -> usize {
        let package = self.package.as_ref().map_or(0, |package| {
            package.files.iter().fold(0_usize, |total, (path, file)| {
                total
                    .saturating_add(path.len())
                    .saturating_add(file.bytes.len())
            })
        });
        let writes = self.writes.iter().fold(0_usize, |total, write| {
            total
                .saturating_add(write.file.bytes().map_or(0, <[u8]>::len))
                .saturating_add(write.bytes.len())
        });
        self.desired
            .comparison_bytes()
            .saturating_add(package)
            .saturating_add(writes)
            .saturating_add(super::cache::encoded_bytes(&(&self.changes, &self.context)))
    }

    pub fn prepare(
        snapshot: &MutationInventory,
        inspector: &dyn MutationInspector,
        item: &AgentCatalogAsset,
        target: &AgentCatalogTarget,
        name: &str,
        definition: &StoredDefinition,
        receipt: Option<&Receipt>,
    ) -> Result<Self, String> {
        if item.version != Some(definition.version) || item.application.source_binding_id.is_some()
        {
            return Err("请先收录当前原生定义，再选择已保存版本进行应用".to_owned());
        }
        if !item.application.available {
            return Err(item.application.reason.clone().unwrap_or_else(|| {
                "共享定义当前有归属冲突或不可应用，请先检查资产状态".to_owned()
            }));
        }
        if !target.available {
            return Err(target
                .reason
                .clone()
                .unwrap_or_else(|| "目标不可用".to_owned()));
        }
        if !target.categories.contains(&item.category) {
            return Err("目标不支持该类别的定义".to_owned());
        }
        let (context, source, adapter) = projection::resolve_target(snapshot, &target.id)?;
        if !source.writable
            || matches!(
                source.origin,
                AgentAssetInstallationOrigin::NativePackage
                    | AgentAssetInstallationOrigin::Bundled
                    | AgentAssetInstallationOrigin::Linked
            )
            || !adapter
                .target_sources(&snapshot.inventory, context)
                .iter()
                .any(|candidate| candidate.id == source.id)
            || !adapter
                .target_scopes(source, context)
                .contains(&target.scope)
        {
            return Err("目标不是当前作用域已确认的原生可写配置".to_owned());
        }
        if receipt.is_some_and(|receipt| {
            receipt.context_id != context.id
                || receipt.scope != target.scope
                || receipt.agent_kind != context.agent_kind
        }) {
            return Err("已保存的应用记录与当前目标来源不一致，请重新检查目标".to_owned());
        }
        let bindings =
            projection::bindings_at_target(snapshot, item, &context.id, target.scope, source);
        if bindings.len() > 1 {
            return Err("该上下文有多个变体，请先明确归并或分别管理原生配置".to_owned());
        }
        let native_name = bindings
            .first()
            .map(|binding| binding.native.native_id.clone())
            .or_else(|| receipt.map(|receipt| receipt.name.clone()))
            .unwrap_or_else(|| name.to_owned());
        if snapshot.inventory.assets.iter().any(|asset| {
            asset.context_id == context.id
                && asset.scope == target.scope
                && asset.category == item.category
                && asset.native_id == native_name
                && projection::definition_source(snapshot, asset).is_some_and(|origin| {
                    if item.category == AgentAssetCategory::Skill {
                        Path::new(&origin.path).starts_with(&source.path)
                    } else {
                        origin.id == source.id
                    }
                })
                && !item
                    .bindings
                    .iter()
                    .any(|binding| binding.id == asset.stable_id)
        }) {
            return Err("目标已有同名但未关联资产，请先明确关联，避免覆盖另一份定义".to_owned());
        }
        if bindings
            .first()
            .is_some_and(|binding| binding.native.relationships.provided_by.is_some())
        {
            return Err("插件提供的资源不能通过独立分发改写".to_owned());
        }
        let allowed = Path::new(&source.allowed_root);
        let root = if allowed.starts_with(inspector.home()) {
            inspector.home()
        } else if inspector
            .workspace()
            .is_some_and(|workspace| allowed.starts_with(workspace))
        {
            inspector.workspace().ok_or("工作区已失效")?
        } else {
            return Err("目标不在用户或所选项目的原生可写范围".to_owned());
        }
        .to_path_buf();
        let source_path = PathBuf::from(&source.path);
        let mut package_snapshot = None;
        let mut writes = Vec::new();
        let (destination, desired) = match &definition.payload {
            DefinitionPayload::Mcp(definition) => {
                if receipt.is_some_and(|receipt| Path::new(&receipt.path) != source_path) {
                    return Err("MCP 应用记录与当前目标路径不一致".to_owned());
                }
                let file = GuardedFile::capture(source, &snapshot.source_anchors)
                    .map_err(|_| "MCP 目标文件无法安全准备")?;
                let bytes = adapter.patch_mcp_at(
                    file.bytes(),
                    &native_name,
                    definition,
                    context,
                    target.scope,
                )?;
                let value = adapter.parse_document(&bytes).ok_or("MCP 原生转换无效")?;
                let resulting = adapter.decode(
                    adapter
                        .mcp_node(&value, context, target.scope, &native_name)
                        .ok_or("MCP 原生目标缺失")?,
                )?;
                writes.push(DistributionWrite {
                    file,
                    bytes,
                    executable: None,
                });
                (source_path.clone(), DefinitionPayload::Mcp(resulting))
            }
            DefinitionPayload::Skill(files) => {
                super::super::definition::validate_name(name)?;
                let binding_destination = bindings
                    .first()
                    .and_then(|binding| projection::definition_source(snapshot, &binding.native))
                    .map(|source| {
                        let path = PathBuf::from(&source.path);
                        if path.file_name().is_some_and(|name| name == "SKILL.md") {
                            path.parent().unwrap_or(&path).to_path_buf()
                        } else {
                            path
                        }
                    });
                let destination = binding_destination
                    .clone()
                    .or_else(|| receipt.map(|receipt| PathBuf::from(&receipt.path)))
                    .unwrap_or_else(|| source_path.join(name));
                if !destination.starts_with(&source_path) {
                    return Err("已有 Skill 不在所选原生包目录，无法分发到该目标".to_owned());
                }
                match std::fs::symlink_metadata(&destination) {
                    Ok(_) => {
                        let owned = binding_destination.as_ref() == Some(&destination)
                            || receipt
                                .is_some_and(|receipt| Path::new(&receipt.path) == destination);
                        if !owned {
                            return Err("目标已有未收录的 Skill 目录，不能接管或覆盖其中资源；请先收录该目录".to_owned());
                        }
                        package_snapshot =
                            Some(package::read_distribution_package(&destination, &root)?);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("Skill 目标状态无法确定".to_owned()),
                }
                if package_snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.files.keys().any(|path| !files.contains_key(path))
                }) {
                    return Err(
                        "目标含共享包之外的资源，已保留它们；请先收录目标版本或选择空目标"
                            .to_owned(),
                    );
                }
                for (relative, file) in files {
                    package::validate_relative(relative)?;
                    let target = destination.join(relative);
                    writes.push(DistributionWrite {
                        file: package::capture_file(&root, &target, package::MAX_FILE_BYTES)?,
                        bytes: file.bytes.clone(),
                        executable: Some(file.executable),
                    });
                }
                (destination, DefinitionPayload::Skill(files.clone()))
            }
            DefinitionPayload::Hook(_) => {
                return Err("Hook 定义必须使用原生 Hook 操作管线".to_owned())
            }
        };
        let directory = if matches!(&definition.payload, DefinitionPayload::Mcp(_)) {
            destination.parent().ok_or("目标父目录无效")?
        } else {
            destination.as_path()
        };
        let parent =
            GuardedDirectory::capture_path(directory).map_err(|_| "无法固定目录创建前置条件")?;
        let mut domains = writes
            .iter()
            .flat_map(|write| write.file.lock_domains())
            .collect::<Vec<_>>();
        domains.push(format!("config:{}", context.config_root));
        domains.push(format!("catalog-directory:{}", destination.display()));
        domains.extend(
            context
                .compatible_installation_ids
                .iter()
                .map(|id| format!("installation:{id}")),
        );
        let changes = distribution_changes(
            &writes,
            &desired,
            adapter,
            &native_name,
            definition.version,
            context,
            target.scope,
        );
        let installations = snapshot
            .inventory
            .installations
            .iter()
            .filter(|installation| {
                context
                    .compatible_installation_ids
                    .contains(&installation.id)
            })
            .collect::<Vec<_>>();
        let installation_stamps = installations
            .iter()
            .filter(|installation| {
                installation.availability == AgentInstallationAvailability::Available
            })
            .map(|installation| {
                ExecutableStamp::capture(installation)
                    .map_err(|_| "无法固定目标 Agent 可执行文件".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let writes_identity = writes
            .iter()
            .map(|write| {
                (
                    write.file.path(),
                    super::super::digest(&write.bytes),
                    write.executable,
                )
            })
            .collect::<Vec<_>>();
        let write_signature = super::super::digest(
            &serde_json::to_vec(&writes_identity).map_err(|_| "分发计划无效")?,
        );
        let fingerprint = desired.fingerprint();
        let affected_asset_ids = snapshot
            .inventory
            .assets
            .iter()
            .filter(|asset| asset.category == item.category)
            .filter(|asset| {
                projection::definition_source(snapshot, asset).is_some_and(|source| {
                    let path = snapshot
                        .source_anchors
                        .get(&source.id)
                        .map_or_else(|| Path::new(&source.path), |anchor| anchor.physical_path());
                    if item.category == AgentAssetCategory::Skill {
                        path.starts_with(&destination)
                    } else {
                        path == destination
                    }
                })
            })
            .map(|asset| asset.stable_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let signature = super::super::digest(&serde_json::to_vec(&serde_json::json!({
            "writes": write_signature,
            "files": writes.iter().map(|write| write.file.signature()).collect::<Vec<_>>(),
            "parent": parent.signature(), "context": context, "target": target.id,
            "receipt": receipt,
            "source": { "id": source.id, "path": source.path, "root": source.allowed_root,
                "scope": source.scope, "writable": source.writable, "revision": source.revision.identity },
            "installations": installations.iter().map(|installation| serde_json::json!({
                "id": installation.id, "identity": installation.executable_identity,
                "revision": installation.executable_revision, "version": installation.installed_version,
                "distribution": installation.distribution, "availability": installation.availability,
            })).collect::<Vec<_>>(),
            "adapter": adapter.key, "definition": fingerprint, "version": definition.version,
            "affectedAssetIds": affected_asset_ids,
        })).map_err(|_| "分发计划无效")?);
        Ok(Self {
            destination,
            signature,
            write_signature,
            domains,
            changes,
            affected_asset_ids,
            context_id: context.id.clone(),
            scope: target.scope,
            agent_kind: context.agent_kind,
            native_name,
            version: definition.version,
            fingerprint,
            root,
            parent,
            package: package_snapshot,
            writes,
            desired,
            adapter,
            source_path,
            installations: installation_stamps,
            context: context.clone(),
        })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        for installation in &self.installations {
            installation
                .revalidate()
                .map_err(|_| "目标 Agent 已替换或升级")?;
        }
        self.parent.revalidate().map_err(|_| "目标目录已变化")?;
        if let Some(package) = &self.package {
            package.revalidate()?;
        }
        for write in &self.writes {
            write
                .file
                .revalidate()
                .map_err(|_| "目标文件已被外部修改")?;
        }
        Ok(())
    }

    pub fn group_key(&self) -> String {
        if matches!(self.desired, DefinitionPayload::Mcp(_)) {
            format!(
                "mcp:{}",
                self.writes
                    .first()
                    .map(|write| write.file.domain())
                    .unwrap_or_default()
            )
        } else {
            format!("{}:{}", self.destination.display(), self.write_signature)
        }
    }

    pub fn validate_group(targets: &[(usize, Self)]) -> Result<(), String> {
        Self::compose(targets).map(|_| ())
    }

    fn compose(targets: &[(usize, Self)]) -> Result<Vec<ComposedDistributionWrite>, String> {
        let mut groups = BTreeMap::<String, Vec<(&Self, &DistributionWrite)>>::new();
        for (_, target) in targets {
            for write in &target.writes {
                groups
                    .entry(write.file.domain())
                    .or_default()
                    .push((target, write));
            }
        }
        groups
            .into_values()
            .map(|members| {
                let (first, write) = members.first().copied().ok_or("分发目标为空")?;
                if members.iter().any(|(target, other)| {
                    target.root != first.root
                        || other.file.path() != write.file.path()
                        || other.file.bytes() != write.file.bytes()
                        || other.executable != write.executable
                        || (matches!(target.desired, DefinitionPayload::Mcp(_))
                            && target.adapter.format != first.adapter.format)
                }) {
                    return Err("同一物理分发目标存在不同来源或权限前置条件".to_owned());
                }
                let bytes = if members.iter().all(|(_, other)| other.bytes == write.bytes) {
                    write.bytes.clone()
                } else if members
                    .iter()
                    .all(|(target, _)| matches!(target.desired, DefinitionPayload::Mcp(_)))
                {
                    super::merge::compose(
                        write.file.bytes().unwrap_or_default(),
                        &members
                            .iter()
                            .map(|(_, write)| write.bytes.clone())
                            .collect::<Vec<_>>(),
                        first.adapter.format,
                    )?
                } else {
                    return Err("多个 Skill 目标要求写入不同包内容，无法合并".to_owned());
                };
                Ok(ComposedDistributionWrite {
                    root: first.root.clone(),
                    write: DistributionWrite {
                        file: write.file.clone(),
                        bytes,
                        executable: write.executable,
                    },
                })
            })
            .collect()
    }

    pub fn apply_group(
        targets: &[(usize, Self)],
        before_commit: impl Fn() -> Result<(), AgentAssetMutationError>,
    ) -> Result<(), String> {
        for (_, target) in targets {
            target.revalidate()?;
        }
        for composed in Self::compose(targets)? {
            let root = &composed.root;
            let write = &composed.write;
            let parent = write.file.path().parent().ok_or("目标父目录无效")?;
            package::ensure_directory_before(root, parent, || {
                before_commit().map_err(|error| error.message)
            })?;
            if write.file.bytes().is_some() {
                write
                    .file
                    .revalidate()
                    .map_err(|_| "原生文件在提交前被替换")?;
            }
            let current = package::capture_file(root, write.file.path(), package::MAX_FILE_BYTES)?;
            if current.bytes() != write.file.bytes() {
                return Err("提交前目标被外部修改；已完成目标不会回滚".to_owned());
            }
            let outcome = atomic::replace(&current, &write.bytes, &before_commit)
                .map_err(|_| "原生文件写入失败；已完成目标不会回滚")?;
            #[cfg(unix)]
            if outcome == atomic::AtomicWriteResult::ReplacedNotSynced {
                return Err("原生文件已替换但目录同步失败，请重新检查；不会回滚".to_owned());
            }
            #[cfg(not(unix))]
            let _ = outcome;
            if let Some(executable) = write.executable {
                set_executable(root, write.file.path(), executable, &before_commit)?;
            }
        }
        Ok(())
    }

    pub fn observe_write(&self) -> WriteObservation {
        let observations = self
            .writes
            .iter()
            .map(|write| write.file.observe_write())
            .collect::<Vec<_>>();
        if observations.contains(&WriteObservation::Changed) {
            WriteObservation::Changed
        } else if observations.contains(&WriteObservation::Unknown) {
            WriteObservation::Unknown
        } else {
            WriteObservation::Unchanged
        }
    }

    pub fn verify(&self) -> Result<bool, String> {
        let observed = match &self.desired {
            DefinitionPayload::Mcp(_) => {
                let file =
                    package::capture_file(&self.root, &self.source_path, package::MAX_FILE_BYTES)?;
                let value = self
                    .adapter
                    .parse_document(file.bytes().ok_or("应用后的原生配置缺失")?)
                    .ok_or("应用后的原生配置无效")?;
                DefinitionPayload::Mcp(
                    self.adapter.decode(
                        self.adapter
                            .mcp_node(&value, &self.context, self.scope, &self.native_name)
                            .ok_or("应用后 MCP 定义缺失")?,
                    )?,
                )
            }
            DefinitionPayload::Skill(_) => DefinitionPayload::Skill(
                package::read_package(&self.destination, &self.root)?.files,
            ),
            DefinitionPayload::Hook(_) => {
                return Err("Hook 分发不能使用 MCP/Skill 验证器".to_owned())
            }
        };
        Ok(observed.fingerprint() == self.fingerprint)
    }

    pub fn receipt(&self) -> Receipt {
        Receipt {
            context_id: self.context_id.clone(),
            scope: self.scope,
            agent_kind: self.agent_kind,
            path: if matches!(&self.desired, DefinitionPayload::Skill(_)) {
                self.destination.to_string_lossy().into_owned()
            } else {
                self.source_path.to_string_lossy().into_owned()
            },
            name: self.native_name.clone(),
            version: self.version,
            fingerprint: self.fingerprint.clone(),
            hook: None,
        }
    }
}

#[cfg(unix)]
fn set_executable(
    root: &Path,
    path: &Path,
    executable: bool,
    before_commit: impl Fn() -> Result<(), AgentAssetMutationError>,
) -> Result<(), String> {
    use crate::services::agent_cli::environment::verified_path::inspect_verified_path;
    use std::os::unix::fs::PermissionsExt;
    let guard = inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File)
        .map_err(|_| "资源权限目标已变化")?;
    let mode = guard
        .metadata()
        .map_err(|_| "无法读取资源权限")?
        .permissions()
        .mode();
    let desired = if executable {
        mode | 0o100
    } else {
        mode & !0o111
    };
    if desired != mode {
        guard.revalidate().map_err(|_| "资源权限目标已变化")?;
        before_commit().map_err(|error| error.message)?;
        guard
            .source_handle()
            .set_permissions(std::fs::Permissions::from_mode(desired))
            .map_err(|_| "无法保留资源执行权限")?;
        guard
            .source_handle()
            .sync_all()
            .map_err(|_| "资源权限已写入但同步失败")?;
    }
    Ok(())
}
#[cfg(not(unix))]
fn set_executable(
    _root: &Path,
    _path: &Path,
    _executable: bool,
    _before_commit: impl Fn() -> Result<(), AgentAssetMutationError>,
) -> Result<(), String> {
    Err("此平台尚未验证资源权限写入".to_owned())
}

fn distribution_changes(
    writes: &[DistributionWrite],
    desired: &DefinitionPayload,
    adapter: &super::super::native::NativeCatalogAdapter,
    name: &str,
    version: u64,
    context: &AgentConfigurationContext,
    scope: AgentAssetScope,
) -> Vec<AgentAssetPlanChange> {
    let bounded = |text: String| -> String {
        if text.chars().count() > 8192 {
            format!(
                "{}\n…预览已截断，完整定义仍按版本应用",
                text.chars().take(8192).collect::<String>()
            )
        } else {
            text
        }
    };
    writes
        .iter()
        .map(|write| {
            let path = Some(write.file.path().to_string_lossy().into_owned());
            let (before, after) = match desired {
                DefinitionPayload::Mcp(value) => {
                    let before = write
                        .file
                        .bytes()
                        .and_then(|bytes| adapter.parse_document(bytes))
                        .and_then(|root| adapter.mcp_node(&root, context, scope, name).cloned())
                        .and_then(|value| adapter.decode(&value).ok())
                        .and_then(|value| serde_json::to_string_pretty(&value.as_input()).ok())
                        .map(&bounded);
                    let after = serde_json::to_string_pretty(&value.as_input())
                        .ok()
                        .map(&bounded);
                    (before, after)
                }
                DefinitionPayload::Skill(_)
                    if write
                        .file
                        .path()
                        .file_name()
                        .is_some_and(|name| name == "SKILL.md") =>
                {
                    let render = |bytes: &[u8]| {
                        std::str::from_utf8(bytes)
                            .ok()
                            .map(|text| bounded(text.to_owned()))
                    };
                    (write.file.bytes().and_then(render), render(&write.bytes))
                }
                DefinitionPayload::Skill(_) => (
                    write
                        .file
                        .bytes()
                        .map(|bytes| format!("资源：{} 字节", bytes.len())),
                    Some(format!(
                        "资源：{} 字节；{}",
                        write.bytes.len(),
                        if write.file.bytes() == Some(write.bytes.as_slice()) {
                            "保留原内容"
                        } else {
                            "应用共享包内容"
                        }
                    )),
                ),
                DefinitionPayload::Hook(_) => {
                    unreachable!("Hook uses the native Hook catalog pipeline")
                }
            };
            let changed = write.file.bytes() != Some(write.bytes.as_slice());
            AgentAssetPlanChange {
                label: format!(
                    "{} · 共享版本 {version}",
                    if changed {
                        "应用定义"
                    } else {
                        "保留定义与资源"
                    }
                ),
                path,
                before,
                after,
            }
        })
        .collect()
}
