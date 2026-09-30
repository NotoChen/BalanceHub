//! Codex system requirements use the native OS location, independently of
//! CODEX_HOME and process environment variables. File IO remains in snapshots.

use super::{source, AgentAssetCategory, AgentAssetScope, AgentAssetSourceKind, SourceInput};
use crate::services::agent_cli::contracts::AgentAssetSourceSpec;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub(in crate::services::agent_cli::codex) fn system_root() -> PathBuf {
    // /etc is a macOS-owned alias. Use its physical OS location so strict
    // no-follow reads still reject user-controlled symlinks below this root.
    PathBuf::from("/private/etc/codex")
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(in crate::services::agent_cli::codex) fn system_root() -> PathBuf {
    PathBuf::from("/etc/codex")
}

#[cfg(windows)]
pub(in crate::services::agent_cli::codex) fn system_root() -> PathBuf {
    codex_dir_from_program_data(program_data_known_folder(), Path::new(r"C:\ProgramData"))
}

#[cfg(any(windows, test))]
fn codex_dir_from_program_data(known_folder: Option<PathBuf>, fallback: &Path) -> PathBuf {
    known_folder
        .unwrap_or_else(|| fallback.to_path_buf())
        .join("OpenAI")
        .join("Codex")
}

#[cfg(windows)]
fn program_data_known_folder() -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };

    struct FolderBuffer(*mut u16);
    impl Drop for FolderBuffer {
        fn drop(&mut self) {
            // SAFETY: SHGetKnownFolderPath transfers a CoTaskMem allocation;
            // it must also be freed on a failed HRESULT if one was returned.
            unsafe {
                CoTaskMemFree(self.0.cast());
            }
        }
    }

    let mut raw = ptr::null_mut();
    // SAFETY: all arguments follow the Known Folder API contract; `raw` is a
    // writable out pointer and the returned buffer is owned by FolderBuffer.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_ProgramData,
            KF_FLAG_DEFAULT as u32,
            ptr::null_mut(),
            &mut raw,
        )
    };
    let buffer = FolderBuffer(raw);
    if result < 0 || buffer.0.is_null() {
        return None;
    }
    // SAFETY: a successful non-null result is a NUL-terminated UTF-16 string
    // that remains live until the guard is dropped after conversion.
    let path = unsafe {
        let mut length = 0;
        while *buffer.0.add(length) != 0 {
            length += 1;
        }
        OsString::from_wide(std::slice::from_raw_parts(buffer.0, length))
    };
    Some(PathBuf::from(path))
}

pub(super) fn requirements_source(root: &Path) -> AgentAssetSourceSpec {
    source(SourceInput {
        origin: crate::models::AgentAssetInstallationOrigin::ConfigEntry,
        native_source_key: "system-requirements",
        label: "Codex 托管要求",
        path: root.join("requirements.toml"),
        allowed_root: root,
        scope: AgentAssetScope::Managed,
        precedence: 100,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[AgentAssetCategory::Mcp],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_folder_and_fallback_keep_the_same_exact_source_contract() {
        let base = std::env::temp_dir().join("balancehub-programdata-fixture");
        let fallback = base.join("fallback");
        let known = base.join("known");
        for (input, expected) in [(Some(known.clone()), &known), (None, &fallback)] {
            let root = codex_dir_from_program_data(input, &fallback);
            assert_eq!(root, expected.join("OpenAI").join("Codex"));
            let spec = requirements_source(&root);
            assert_eq!(spec.path, root.join("requirements.toml"));
            assert_eq!(spec.path.parent(), Some(spec.allowed_root.as_path()));
            assert_eq!(spec.native_source_key, "system-requirements");
            assert_eq!(spec.scope, AgentAssetScope::Managed);
            assert_eq!(spec.precedence, 100);
            assert_eq!(spec.categories, [AgentAssetCategory::Mcp]);
            assert_eq!(spec.source_kind, AgentAssetSourceKind::File);
            assert!(spec.sensitive);
            assert!(!spec.writable);
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn unix_requirements_location_is_unchanged() {
        assert_eq!(
            requirements_source(&system_root()).path,
            Path::new("/etc/codex/requirements.toml")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_system_sources_use_the_physical_os_directory() {
        use crate::services::agent_cli::environment::verified_path::inspect_verified_path;
        let root = system_root();
        assert_eq!(root, Path::new("/private/etc/codex"));
        assert_eq!(
            requirements_source(&root).path,
            root.join("requirements.toml")
        );
        // Exercise the real strict path walker on the default system parent;
        // reading metadata does not inspect or modify any installed settings.
        let parent = root.parent().unwrap();
        let guard =
            inspect_verified_path(&[parent], parent, parent, AgentAssetSourceKind::Directory);
        assert!(
            guard.is_ok(),
            "the native system descriptor must not traverse /etc's symlink"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_fallback_uses_the_native_absolute_location() {
        assert_eq!(
            codex_dir_from_program_data(None, Path::new(r"C:\ProgramData")),
            Path::new(r"C:\ProgramData\OpenAI\Codex")
        );
    }
}
