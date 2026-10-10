use super::definition::PackageFile;
use crate::{
    models::*,
    services::agent_cli::environment::{
        mutation::GuardedFile,
        verified_path::{inspect_verified_path, reopen_verified_path, VerifiedPathAnchor},
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    time::{Duration, Instant},
};

pub(super) const MAX_FILE_BYTES: usize = 512 * 1024;
pub(super) const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_PACKAGE_FILES: usize = 256;
const MAX_DEPTH: usize = 8;

pub(super) struct PackageSnapshot {
    pub files: BTreeMap<String, PackageFile>,
    pub anchors: Vec<VerifiedPathAnchor>,
}

impl PackageSnapshot {
    pub fn revalidate(&self) -> Result<(), String> {
        for anchor in &self.anchors {
            let guard = reopen_verified_path(anchor).map_err(|_| "Skill 包在读取期间发生变化")?;
            if anchor.source_kind() == AgentAssetSourceKind::File {
                let bytes = guard
                    .read_file_bounded(MAX_FILE_BYTES)
                    .map_err(|_| "Skill 资源不能安全读取")?;
                if !anchor.matches_bytes(&bytes) {
                    return Err("Skill 资源在读取期间发生变化".to_owned());
                }
            }
        }
        Ok(())
    }
}

pub(super) fn read_package(root: &Path, allowed_root: &Path) -> Result<PackageSnapshot, String> {
    read_package_contents(root, allowed_root, true, true)
}

/// A previously applied package may have lost only SKILL.md. Distribution can
/// restore that known file while still proving and preserving all other files.
pub(super) fn read_distribution_package(
    root: &Path,
    allowed_root: &Path,
) -> Result<PackageSnapshot, String> {
    read_package_contents(root, allowed_root, false, true)
}

/// Removal previews the complete file set even when the manifest is malformed.
pub(super) fn read_removal_package(
    root: &Path,
    allowed_root: &Path,
) -> Result<PackageSnapshot, String> {
    read_package_contents(root, allowed_root, true, false)
}

fn read_package_contents(
    root: &Path,
    allowed_root: &Path,
    require_manifest: bool,
    validate_manifest: bool,
) -> Result<PackageSnapshot, String> {
    let mut snapshot = PackageSnapshot {
        files: BTreeMap::new(),
        anchors: Vec::new(),
    };
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut bytes_read = 0usize;
    let deadline = Instant::now() + Duration::from_secs(3);
    while let Some((directory, depth)) = stack.pop() {
        if depth > MAX_DEPTH || Instant::now() >= deadline {
            return Err("Skill 包超过目录深度或读取预算，保留未知状态".to_owned());
        }
        let guard = inspect_verified_path(
            &[allowed_root],
            allowed_root,
            &directory,
            AgentAssetSourceKind::Directory,
        )
        .map_err(|_| "Skill 包路径不安全或包含软链接")?;
        let entries = guard.read_entries().map_err(|_| "无法读取 Skill 包目录")?;
        let mut count = 0usize;
        for entry in entries {
            count += 1;
            if count > MAX_PACKAGE_FILES
                || snapshot.files.len() + stack.len() >= MAX_PACKAGE_FILES
                || Instant::now() >= deadline
            {
                return Err("Skill 包超过 256 项读取限制".to_owned());
            }
            let entry = entry.map_err(|_| "无法读取 Skill 资源")?;
            if entry.is_symlink {
                return Err("Skill 包含软链接，无法证明完整可移植内容".to_owned());
            }
            let path = directory.join(&entry.name);
            if entry.source_kind == AgentAssetSourceKind::Directory {
                stack.push((path, depth + 1));
                continue;
            }
            let file = inspect_verified_path(
                &[allowed_root],
                allowed_root,
                &path,
                AgentAssetSourceKind::File,
            )
            .map_err(|_| "Skill 资源类型或路径不安全")?;
            let bytes = file
                .read_file_bounded(MAX_FILE_BYTES)
                .map_err(|_| "Skill 单个文件超过 512 KiB 或无法读取")?;
            bytes_read = bytes_read
                .checked_add(bytes.len())
                .ok_or("Skill 大小无效")?;
            if bytes_read > MAX_PACKAGE_BYTES {
                return Err("Skill 包超过 8 MiB".to_owned());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "Skill 资源越界")?
                .to_str()
                .ok_or("Skill 资源文件名必须是 UTF-8")?
                .replace('\\', "/");
            validate_relative(&relative)?;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                file.metadata()
                    .map_err(|_| "无法读取 Skill 权限")?
                    .permissions()
                    .mode()
                    & 0o111
                    != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            let revision = file_revision(&file, Some(&bytes));
            snapshot.anchors.push(file.anchor(&revision, Some(&bytes)));
            snapshot
                .files
                .insert(relative, PackageFile { bytes, executable });
        }
        let revision = file_revision(&guard, None);
        snapshot.anchors.push(guard.anchor(&revision, None));
    }
    if let Some(manifest) = snapshot.files.get("SKILL.md") {
        if validate_manifest {
            super::definition::validate_skill(&manifest.bytes)?;
        }
    } else if require_manifest {
        return Err("Skill 包缺少 SKILL.md".to_owned());
    }
    snapshot.revalidate()?;
    Ok(snapshot)
}

pub(super) fn validate_relative(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains(['\0', '\\'])
        || value.len() > 512
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Skill 相对路径无效".to_owned());
    }
    Ok(())
}

fn file_revision(
    guard: &crate::services::agent_cli::environment::verified_path::VerifiedPathGuard,
    bytes: Option<&[u8]>,
) -> AgentAssetRevision {
    let mut hash = Sha256::new();
    guard.update_revision(&mut hash);
    if let Some(bytes) = bytes {
        hash.update(bytes);
    }
    AgentAssetRevision {
        identity: format!("{:x}", hash.finalize()),
        observed_at: String::new(),
        size_bytes: bytes.map(|bytes| bytes.len() as u64),
        is_missing: false,
        is_directory: bytes.is_none(),
        is_symlink: false,
    }
}

/// Only backend-generated destinations call this. No raw path is accepted by IPC.
pub(super) fn capture_file(root: &Path, path: &Path, limit: usize) -> Result<GuardedFile, String> {
    GuardedFile::capture_path(root, path, limit)
        .map_err(|_| "配置路径不安全、超出限制或读取前置条件发生变化".to_owned())
}

pub(super) use crate::services::agent_cli::config_support::directories::{
    ensure_directory, ensure_directory_before,
};
