//! Free-space probing for optional component installs.
use std::path::Path;

/// Bytes available to the current user on the filesystem that holds `path`.
/// `None` means the platform call failed; callers treat that as unknown and do
/// not block the operation.
pub fn available_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs reads the NUL-terminated path and writes one struct.
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: a zero return initialized the buffer.
        let stats = unsafe { stats.assume_init() };
        let block = if stats.f_frsize == 0 {
            stats.f_bsize
        } else {
            stats.f_frsize
        };
        // 先按 128 位计算再收窄，32/64 位 Unix 都不需要平台相关转换。
        let bytes = u128::from(stats.f_bavail).saturating_mul(u128::from(block));
        Some(u64::try_from(bytes).unwrap_or(u64::MAX))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let path = path
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut available = 0u64;
        // SAFETY: the path is NUL-terminated and the out-pointer is valid;
        // the two total-size outputs are optional and stay null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(available)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_directory_reports_a_positive_amount() {
        let directory = std::env::temp_dir();
        assert!(available_space(&directory).is_some_and(|space| space > 0));
    }

    #[test]
    fn a_missing_path_is_unknown_rather_than_zero() {
        let missing = std::env::temp_dir().join("balancehub-missing-disk-probe");
        assert_eq!(available_space(&missing), None);
    }
}
