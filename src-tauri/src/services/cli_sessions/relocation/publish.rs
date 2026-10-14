use crate::{
    models::AgentAssetSourceKind,
    services::agent_cli::environment::verified_path::{inspect_verified_path, VerifiedPathGuard},
};
use std::{io::Write, path::Path};

pub(super) fn create_backup(root: &Path, backup: &Path) -> Result<VerifiedPathGuard, String> {
    let parent = backup.parent().ok_or("迁移备份目录无效")?;
    crate::services::agent_cli::config_support::directories::ensure_directory(root, parent)?;
    let guard = inspect_verified_path(&[root], root, parent, AgentAssetSourceKind::Directory)
        .map_err(|_| "迁移备份父目录不可访问")?;
    // A task owns only a newly created directory. Never clean a pre-existing
    // directory even if a caller happens to reuse an operation identifier.
    std::fs::create_dir(backup).map_err(|_| "无法创建独立迁移备份目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(backup, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法保护迁移备份目录")?;
    }
    guard
        .revalidate_identity()
        .map_err(|_| "迁移备份父目录发生变化")?;
    inspect_verified_path(&[root], root, backup, AgentAssetSourceKind::Directory)
        .map_err(|_| "迁移备份目录不可访问".to_owned())
}

pub(super) fn clean_unpublished(
    backup: &VerifiedPathGuard,
    staged: &Path,
    original: &Path,
) -> Result<(), String> {
    backup
        .revalidate_identity()
        .map_err(|_| "迁移目录已变化，保留暂存内容")?;
    match std::fs::symlink_metadata(original) {
        Ok(_) => return Ok(()), // Keep both sides when publication needs recovery.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("无法核对原始备份，保留暂存内容".to_owned()),
    }
    match std::fs::symlink_metadata(staged) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {
            // Rust's directory removal does not follow symlinks. This private
            // staging tree was created exclusively for this operation.
            std::fs::remove_dir_all(staged)
                .map_err(|_| "部分暂存文件未能清理，详见迁移记录目录".to_owned())
        }
        _ => Err("暂存目录状态已变化，未自动清理".to_owned()),
    }
}

pub(super) fn require_space(root: &Path, bytes: u64) -> Result<(), String> {
    #[cfg(unix)]
    let available = {
        let info = rustix::fs::statvfs(root).map_err(|_| "无法读取会话磁盘剩余空间")?;
        info.f_bavail.saturating_mul(info.f_frsize)
    };
    #[cfg(windows)]
    let available = {
        use std::os::windows::ffi::OsStrExt;
        let path = root
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut available = 0u64;
        // SAFETY: live terminated path and output storage; the API retains neither.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err("无法读取会话磁盘剩余空间".to_owned());
        }
        available
    };
    if available < bytes.saturating_add(256 * 1024 * 1024) {
        return Err(format!(
            "迁移需要约 {:.1} MiB 暂存空间，当前磁盘空间不足",
            bytes as f64 / 1024.0 / 1024.0
        ));
    }
    Ok(())
}
pub(super) fn manifest(
    root: &Path,
    directory: &Path,
    value: &serde_json::Value,
) -> Result<(), String> {
    let guard = inspect_verified_path(&[root], root, directory, AgentAssetSourceKind::Directory)
        .map_err(|_| "迁移记录目录不可访问")?;
    let path = directory.join("manifest.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "无法保存迁移恢复记录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "无法保护迁移记录")?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "迁移恢复记录编码失败")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "迁移恢复记录同步失败")?;
    guard
        .revalidate_identity()
        .map_err(|_| "迁移记录目录发生变化".to_owned())
}

/// Never replace an existing file or directory, including an empty directory.
/// Return synchronization separately: after rename the caller must know that
/// publication happened even if syncing a directory did not succeed.
pub(super) fn move_directory(root: &Path, from: &Path, to: &Path) -> Result<bool, String> {
    let source = inspect_verified_path(&[root], root, from, AgentAssetSourceKind::Directory)
        .map_err(|_| "待迁移目录不可安全访问")?;
    let from_parent = from.parent().ok_or("源目录无父级")?;
    let to_parent = to.parent().ok_or("目标目录无父级")?;
    let source_parent =
        inspect_verified_path(&[root], root, from_parent, AgentAssetSourceKind::Directory)
            .map_err(|_| "源目录父级不可访问")?;
    let destination_parent =
        inspect_verified_path(&[root], root, to_parent, AgentAssetSourceKind::Directory)
            .map_err(|_| "目标目录父级不可访问")?;
    source.revalidate().map_err(|_| "待迁移目录发生变化")?;
    drop(source);
    #[cfg(unix)]
    {
        rustix::fs::renameat_with(
            source_parent.source_handle(),
            from.file_name().ok_or("源目录名称无效")?,
            destination_parent.source_handle(),
            to.file_name().ok_or("目标目录名称无效")?,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(|error| format!("目录移动失败，目标可能已存在或不在同一磁盘：{error}"))?;
        let synced = source_parent.source_handle().sync_all().is_ok()
            && destination_parent.source_handle().sync_all().is_ok();
        Ok(synced)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let from = from
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let to = to
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: both buffers are terminated and retained through the call.
        // No REPLACE_EXISTING or COPY_ALLOWED: same filesystem, no overwrite.
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(format!(
                "目录移动失败，目标可能已存在或文件被占用：{}",
                std::io::Error::last_os_error()
            ));
        }
        let _ = (source_parent, destination_parent);
        Ok(true)
    }
}
