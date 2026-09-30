use crate::services::agent_cli::environment::mutation::GuardedFile;

#[cfg(unix)]
pub(super) fn save(file: &GuardedFile, bytes: &[u8]) -> Result<(), String> {
    use crate::services::agent_cli::environment::mutation::atomic;
    match atomic::replace(file, bytes, || Ok(())) {
        Ok(atomic::AtomicWriteResult::Unchanged | atomic::AtomicWriteResult::Replaced) => Ok(()),
        Ok(atomic::AtomicWriteResult::ReplacedNotSynced) => {
            Err("共享库已替换，但目录同步失败；请重新读取当前版本，勿重复提交".to_owned())
        }
        Err(_) => Err("共享库原子保存失败，已保留当前文件".to_owned()),
    }
}

/// App-private persistence is available independently of native mutation
/// certification. This does not enable unverified Windows Agent distribution.
#[cfg(windows)]
pub(super) fn save(file: &GuardedFile, bytes: &[u8]) -> Result<(), String> {
    use crate::{
        models::AgentAssetSourceKind,
        services::agent_cli::environment::verified_path::inspect_verified_path,
    };
    use std::{
        fs::OpenOptions,
        io::Write,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, FILE_FLAG_OPEN_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING,
        MOVEFILE_WRITE_THROUGH,
    };

    let root = file.path().parent().ok_or("共享库目录无效")?;
    let parent = inspect_verified_path(&[root], root, root, AgentAssetSourceKind::Directory)
        .map_err(|_| "共享库目录不可安全访问")?;
    if file.bytes() == Some(bytes) {
        return Ok(());
    }
    let temporary = root.join(format!(".balancehub-library-{}.tmp", super::opaque_id()?));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&temporary)
        .map_err(|_| "共享库临时文件无法创建")?;
    let result = (|| {
        output.write_all(bytes).map_err(|_| "共享库写入失败")?;
        output.sync_all().map_err(|_| "共享库同步失败")?;
        file.revalidate().map_err(|_| "共享库在保存前已变化")?;
        parent
            .revalidate_identity()
            .map_err(|_| "共享库目录已变化")?;
        // Close our non-shared temporary handle immediately before the atomic
        // move. Retained parent handles still prevent ancestry replacement.
        drop(output);
        let from = temporary
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let to = file
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let flags = MOVEFILE_WRITE_THROUGH
            | if file.bytes().is_some() {
                MOVEFILE_REPLACE_EXISTING
            } else {
                0
            };
        // SAFETY: both paths are live, NUL-terminated buffers; no pointers are
        // retained by MoveFileExW. A missing target deliberately uses no-clobber.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) } == 0 {
            return Err("共享库原子替换失败，文件可能被占用；请重新读取".to_owned());
        }
        Ok(())
    })();
    // This is only the random temporary file created by this call, beneath
    // the retained private-directory handle. Never remove the destination.
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(not(any(unix, windows)))]
pub(super) fn save(_file: &GuardedFile, _bytes: &[u8]) -> Result<(), String> {
    Err("此平台不支持共享库持久化".to_owned())
}
