//! Delete only a verified file or an empty directory, never a newly discovered subtree.
use super::{reopen_verified_directory_identity, reopen_verified_path, VerifiedPathAnchor};
use crate::models::AgentAssetSourceKind;

pub(crate) fn remove(
    anchor: &VerifiedPathAnchor,
    expected: Option<&[u8]>,
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if anchor.is_readonly_reference() {
        return Err("链接必须通过链接移除入口处理".to_owned());
    }
    let directory = anchor.source_kind() == AgentAssetSourceKind::Directory;
    let guard = if directory {
        reopen_verified_directory_identity(anchor)
    } else {
        reopen_verified_path(anchor)
    }
    .map_err(|_| "移除目标已变化，请重新预览")?;
    if !directory {
        let expected = expected.ok_or("移除文件缺少内容校验")?;
        let bytes = guard
            .read_file_bounded(expected.len().max(1))
            .map_err(|_| "无法校验待移除文件")?;
        if bytes != expected {
            return Err("文件内容已变化，请重新预览".to_owned());
        }
    }
    crate::services::agent_cli::config_support::native_history::record_removed(anchor, expected)?;
    #[cfg(unix)]
    {
        use rustix::fs::{unlinkat, AtFlags};
        let parent = guard.parent_handle().ok_or("移除目标父目录无效")?;
        before_commit()?;
        if directory {
            guard.revalidate_identity()
        } else {
            guard.revalidate()
        }
        .map_err(|_| "移除目标在提交前已变化")?;
        unlinkat(
            parent,
            anchor.display_path().file_name().ok_or("文件名无效")?,
            if directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )
        .map_err(|_| "移除失败，目录中可能有新增文件；已完成的操作可在变更记录中查看")?;
        parent
            .sync_all()
            .map_err(|_| "已移除，但目录同步失败，请刷新核实")?;
        Ok(())
    }
    #[cfg(windows)]
    {
        guard
            .platform
            .remove(anchor.physical_path(), expected, before_commit)
    }
}
