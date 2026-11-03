use super::{
    files::{self, FileWrite},
    MaintenanceService,
};
use crate::services::agent_cli::environment::verified_path::remove_reference::DirectoryReference;
use crate::{
    models::*,
    services::agent_cli::{
        config_support::{directories, native_history},
        environment::mutation::GuardedFile,
    },
};
use std::path::PathBuf;

pub(crate) fn plan(
    service: &MaintenanceService,
    actor: &str,
    id: &str,
    selected_paths: Option<Vec<String>>,
) -> Result<AgentMaintenancePlan, String> {
    let store = native_history::store()?;
    let snapshot = store.snapshot(id)?;
    if let Some(paths) = &selected_paths {
        if paths.is_empty()
            || paths
                .iter()
                .any(|path| !snapshot.entry.paths.contains(path))
        {
            return Err("请选择此记录中的文件".to_owned());
        }
    }
    let mut writes = Vec::new();
    let mut directories = Vec::new();
    let mut links = Vec::new();
    let mut removed_links = Vec::new();
    let mut changes = Vec::new();
    let mut items = Vec::new();
    let mut bytes = 0usize;
    for file in snapshot.files {
        if !crate::services::wsl::can_access_path(&file.path) {
            return Err("请先启动配置所属的 WSL 发行版".to_owned());
        }
        let key = native_history::hash(file.path.as_os_str().as_encoded_bytes());
        let label = crate::platform::paths::compatible_path(&file.path)
            .to_string_lossy()
            .into_owned();
        if selected_paths
            .as_ref()
            .is_some_and(|paths| !paths.contains(&label))
        {
            continue;
        }
        match file.kind.as_str() {
            "file" => {
                let source = GuardedFile::capture_path(&file.root, &file.path, 64 * 1024 * 1024)
                    .map_err(|_| format!("无法读取当前文件：{label}"))?;
                let after = file
                    .before
                    .as_deref()
                    .map(|id| store.blob(id))
                    .transpose()?;
                #[cfg(unix)]
                let permission_change = if after.is_some() && source.bytes().is_some() {
                    use std::os::unix::fs::PermissionsExt;
                    let current_mode = source
                        .reopen_present()
                        .map_err(|_| "文件已变化")?
                        .metadata()
                        .map_err(|_| "文件权限无法读取")?
                        .permissions()
                        .mode()
                        & 0o777;
                    file.mode.filter(|mode| *mode != current_mode).map(|mode| {
                        AgentAssetPlanChange {
                            label: format!("{label} · 权限"),
                            path: Some(label.clone()),
                            before: Some(format!("mode {current_mode:03o}\n")),
                            after: Some(format!("mode {mode:03o}\n")),
                        }
                    })
                } else {
                    None
                };
                #[cfg(not(unix))]
                let permission_change: Option<AgentAssetPlanChange> = None;
                if source.bytes() == after.as_deref() && permission_change.is_none() {
                    continue;
                }
                if let Some(change) = permission_change {
                    changes.push(change);
                }
                let write = FileWrite {
                    id: key.clone(),
                    source,
                    after,
                    executable: None,
                    mode: file.mode,
                };
                bytes = bytes.checked_add(write.bytes()).ok_or("恢复文件大小无效")?;
                if bytes > native_history::MAX_SNAPSHOT_BYTES {
                    return Err("本次恢复内容超过 256 MiB，请按文件分批恢复".to_owned());
                }
                changes.push(write.change());
                writes.push(write);
            }
            "directory" | "link" => {
                if let Ok(meta) = std::fs::symlink_metadata(&file.path) {
                    if file.kind == "directory" && meta.is_dir() && !meta.is_symlink() {
                        crate::services::agent_cli::environment::verified_path::inspect_verified_path(&[&file.root], &file.root, &file.path, AgentAssetSourceKind::Directory).map_err(|_| "历史目录位置已变成链接或不可访问")?;
                        continue;
                    }
                    if file.kind == "link"
                        && file.link_target.as_ref().is_some_and(|target| {
                            DirectoryReference::capture(
                                &file.root,
                                &file.path,
                                target,
                                file.link_junction,
                            )
                            .is_ok()
                        })
                    {
                        continue;
                    }
                    return Err(format!("目标已存在：{label}；请先检查当前来源"));
                }
                let source = GuardedFile::capture_path(&file.root, &file.path, 1)
                    .map_err(|_| "原目录已变化")?;
                if file.kind == "directory" {
                    directories.push((key.clone(), source, file.root, file.path, file.mode));
                } else {
                    let target = file.link_target.ok_or("历史链接缺少目标")?;
                    let resolved = if target.is_absolute() {
                        target.clone()
                    } else {
                        file.path.parent().ok_or("链接路径无效")?.join(&target)
                    };
                    let canonical = resolved
                        .canonicalize()
                        .map_err(|_| "历史链接指向的共享目录已不存在")?;
                    let guard = crate::services::agent_cli::environment::verified_path::inspect_verified_path(&[&canonical], &canonical, &canonical, AgentAssetSourceKind::Directory).map_err(|_| "历史链接目标不可访问")?;
                    links.push((key.clone(), source, target, guard, file.link_junction));
                }
                changes.push(AgentAssetPlanChange {
                    label: label.clone(),
                    path: Some(label.clone()),
                    before: None,
                    after: Some(
                        if file.kind == "directory" {
                            "恢复目录"
                        } else {
                            "恢复共享目录链接"
                        }
                        .to_owned(),
                    ),
                });
            }
            "createdLink" => {
                match std::fs::symlink_metadata(&file.path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return Err(format!("无法核对目录链接：{label}")),
                    Ok(_) => {}
                }
                let target = file.link_target.ok_or("历史链接缺少目标")?;
                let reference = DirectoryReference::capture(
                    &file.root,
                    &file.path,
                    &target,
                    file.link_junction,
                )?;
                removed_links.push((key.clone(), reference));
                changes.push(AgentAssetPlanChange {
                    label: label.clone(),
                    path: Some(label.clone()),
                    before: Some(format!("目录链接 → {}", target.display())),
                    after: None,
                });
            }
            _ => return Err("历史记录包含未知文件类型".to_owned()),
        }
        items.push(super::item(key, label));
    }
    let mut display = super::preview(
        AgentMaintenanceKind::Restore,
        format!("恢复 · {}", snapshot.entry.title),
        changes,
        items,
    );
    display.notes.push(
        "恢复到此次操作之前的内容；下方展示当前内容与将恢复的内容。恢复前会再次保存当前版本。"
            .to_owned(),
    );
    service.plan(actor, display, bytes, move |context| {
        let domains = writes
            .iter()
            .flat_map(|write| write.source.lock_domains())
            .chain(
                directories
                    .iter()
                    .flat_map(|(_, source, _, _, _)| source.lock_domains()),
            )
            .chain(
                links
                    .iter()
                    .flat_map(|(_, source, _, _, _)| source.lock_domains()),
            )
            .chain(
                removed_links
                    .iter()
                    .map(|(_, reference)| reference.domain()),
            )
            .collect::<Vec<_>>();
        let locks = crate::services::agent_cli::environment::mutation::locking::shared_locks();
        let _locked = locks
            .acquire(&domains, context.canceled(), context.deadline())
            .map_err(|_| "等待恢复目标可写超时或已取消")?;
        // Validate the complete baseline before creating anything.
        for (_, source, _, _, _) in &directories {
            source.revalidate().map_err(|_| "目录在预览后已变化")?;
        }
        for (_, source, _, guard, _) in &links {
            source.revalidate().map_err(|_| "链接位置在预览后已变化")?;
            guard.revalidate_identity().map_err(|_| "共享目录已变化")?;
        }
        for (_, reference) in &removed_links {
            reference.revalidate()?;
        }
        for write in &writes {
            write
                .source
                .revalidate()
                .map_err(|_| "文件在预览后已变化")?;
        }
        let mut ensured: Vec<PathBuf> = writes
            .iter()
            .filter_map(|write| write.source.path().parent().map(PathBuf::from))
            .collect();
        files::apply_locked(&mut writes, context)?;
        for (key, reference) in removed_links {
            reference.remove(|| {
                context.check()?;
                context.changed();
                Ok(())
            })?;
            context.item(&key, AgentMaintenanceItemState::Completed, None);
        }
        directories.sort_by_key(|(_, _, _, path, _)| path.components().count());
        let mut directory_modes = Vec::with_capacity(directories.len());
        for (key, mut source, root, path, mode) in directories {
            context.check()?;
            if ensured.iter().any(|parent| parent.starts_with(&path)) {
                directory_modes.push((key, root, path, mode));
                continue;
            }
            for parent in &ensured {
                source
                    .reanchor_after_parent_creation(parent)
                    .map_err(|error| error.message)?;
            }
            source.revalidate().map_err(|_| "目录目标发生变化")?;
            directories::ensure_directory_before(&root, &path, || context.check())?;
            context.changed();
            ensured.push(path.clone());
            directory_modes.push((key, root, path, mode));
        }
        for (key, mut source, target, guard, junction) in links {
            context.check()?;
            for parent in &ensured {
                source
                    .reanchor_after_parent_creation(parent)
                    .map_err(|error| error.message)?;
            }
            source.revalidate().map_err(|_| "链接位置发生变化")?;
            guard.revalidate_identity().map_err(|_| "共享目录已变化")?;
            let parent = source
                .path()
                .parent()
                .ok_or("链接父目录无效")?
                .to_path_buf();
            directories::ensure_directory_before(source.allowed_root(), &parent, || {
                context.check()
            })?;
            source
                .reanchor_after_parent_creation(&parent)
                .map_err(|error| error.message)?;
            source.revalidate().map_err(|_| "链接位置发生变化")?;
            let parent_guard =
                crate::services::agent_cli::environment::verified_path::inspect_verified_path(
                    &[source.allowed_root()],
                    source.allowed_root(),
                    &parent,
                    AgentAssetSourceKind::Directory,
                )
                .map_err(|_| "链接父目录已变化")?;
            native_history::record_created_link(
                source.allowed_root(),
                source.path(),
                &target,
                junction,
            )?;
            #[cfg(unix)]
            {
                if junction {
                    return Err("此平台不能恢复 Windows junction".to_owned());
                }
                rustix::fs::symlinkat(
                    &target,
                    parent_guard.source_handle(),
                    source.path().file_name().ok_or("链接文件名无效")?,
                )
                .map_err(|_| "无法恢复共享目录链接")?;
            }
            #[cfg(windows)]
            {
                if junction {
                    junction::create(&target, source.path())
                        .map_err(|_| "无法恢复 Windows junction")?;
                } else {
                    std::os::windows::fs::symlink_dir(&target, source.path()).map_err(|_| {
                        "无法恢复目录链接，请启用 Windows 开发者模式或使用管理员权限"
                    })?;
                }
            }
            parent_guard
                .revalidate_identity()
                .map_err(|_| "恢复期间父目录发生变化")?;
            context.changed();
            DirectoryReference::capture(source.allowed_root(), source.path(), &target, junction)?
                .revalidate()?;
            context.item(&key, AgentMaintenanceItemState::Completed, None);
        }
        // Restore restrictive permissions only after all children exist, deepest first.
        for (key, root, path, mode) in directory_modes.into_iter().rev() {
            context.check()?;
            restore_directory_mode(&root, &path, mode)?;
            context.item(&key, AgentMaintenanceItemState::Completed, None);
        }
        Ok(())
    })
}

fn restore_directory_mode(
    root: &std::path::Path,
    path: &std::path::Path,
    mode: Option<u32>,
) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let guard = crate::services::agent_cli::environment::verified_path::inspect_verified_path(
            &[root],
            root,
            path,
            AgentAssetSourceKind::Directory,
        )
        .map_err(|_| "恢复目录不可访问")?;
        guard
            .source_handle()
            .set_permissions(std::fs::Permissions::from_mode(mode & 0o777))
            .map_err(|_| "目录已恢复，但权限设置失败")?;
    }
    #[cfg(not(unix))]
    let _ = (root, path, mode);
    Ok(())
}
