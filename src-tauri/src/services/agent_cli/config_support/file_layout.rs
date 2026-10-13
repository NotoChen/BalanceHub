//! Metadata needed by every staged replacement of a native configuration.
use std::{fs::File, io};

pub(crate) fn require_single_link(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    let links = {
        use std::os::unix::fs::MetadataExt;
        file.metadata()?.nlink()
    };
    #[cfg(windows)]
    let links = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the borrowed file handle and exact output structure are live.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(io::Error::last_os_error());
        }
        u64::from(information.nNumberOfLinks)
    };
    if links > 1 {
        return Err(io::Error::other(
            "此文件存在硬链接，替换会拆开共享关系；请使用原生编辑器修改",
        ));
    }
    Ok(())
}

pub(crate) fn copy_permissions(original: &File, replacement: &File) -> io::Result<()> {
    require_single_link(original)?;
    let metadata = original.metadata()?;
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::fs::MetadataExt};
        let current = replacement.metadata()?;
        if current.uid() != metadata.uid() || current.gid() != metadata.gid() {
            // Ownership is set before mode; chown can clear permission bits.
            // SAFETY: only the newly created, owned staging file is changed.
            if unsafe { libc::fchown(replacement.as_raw_fd(), metadata.uid(), metadata.gid()) } != 0
            {
                return Err(io::Error::other("无法保持原配置的用户和用户组，已取消保存"));
            }
        }
    }
    replacement.set_permissions(metadata.permissions())
}
