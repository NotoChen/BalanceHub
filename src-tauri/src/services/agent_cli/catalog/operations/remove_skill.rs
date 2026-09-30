use super::super::package;
#[cfg(unix)]
use crate::services::agent_cli::environment::verified_path::reopen_verified_directory_identity;
use crate::{
    models::*,
    services::agent_cli::environment::verified_path::{
        inspect_verified_path, remove_reference::remove_skill_reference, reopen_verified_path,
        VerifiedPathAnchor,
    },
};
use std::path::{Path, PathBuf};

pub(super) struct SkillRemoval {
    path: PathBuf,
    root: PathBuf,
    reference: Option<VerifiedPathAnchor>,
    package: Option<package::PackageSnapshot>,
    physical_path: PathBuf,
}

impl SkillRemoval {
    pub fn prepare(root: &Path, source: &VerifiedPathAnchor) -> Result<Self, String> {
        let path = source
            .display_path()
            .parent()
            .ok_or("Skill 目录无效")?
            .to_path_buf();
        let reference = source.is_readonly_reference().then(|| source.clone());
        let package = if reference.is_some() {
            None
        } else {
            Some(package::read_removal_package(&path, root)?)
        };
        Ok(Self {
            path,
            root: root.to_path_buf(),
            reference,
            package,
            physical_path: source
                .physical_path()
                .parent()
                .ok_or("Skill 物理目录无效")?
                .to_path_buf(),
        })
    }

    pub fn physical_path(&self) -> &Path {
        &self.physical_path
    }
    pub fn is_reference(&self) -> bool {
        self.reference.is_some()
    }

    pub fn signatures(&self) -> Vec<String> {
        self.reference
            .iter()
            .chain(
                self.package
                    .iter()
                    .flat_map(|package| package.anchors.iter()),
            )
            .map(|anchor| anchor.revision().identity.clone())
            .collect()
    }

    pub fn bytes(&self) -> usize {
        self.package.as_ref().map_or(0, |package| {
            package.files.values().map(|file| file.bytes.len()).sum()
        })
    }

    pub fn changes(&self) -> Vec<AgentAssetPlanChange> {
        if let Some(reference) = &self.reference {
            return vec![AgentAssetPlanChange {
                label: "移除 Agent 的 Skill 链接，保留共享源文件".to_owned(),
                path: Some(self.path.to_string_lossy().into_owned()),
                before: Some(format!(
                    "链接 → {}",
                    reference
                        .physical_path()
                        .parent()
                        .unwrap_or(reference.physical_path())
                        .display()
                )),
                after: None,
            }];
        }
        self.package
            .iter()
            .flat_map(|package| package.files.iter())
            .map(|(name, file)| AgentAssetPlanChange {
                label: format!("删除 Skill 文件 · {name}"),
                path: Some(self.path.join(name).to_string_lossy().into_owned()),
                before: Some(
                    std::str::from_utf8(&file.bytes)
                        .map(str::to_owned)
                        .unwrap_or_else(|_| format!("二进制文件 · {} 字节", file.bytes.len())),
                ),
                after: None,
            })
            .collect()
    }

    pub fn revalidate(&self) -> Result<(), String> {
        if let Some(reference) = &self.reference {
            reopen_verified_path(reference).map_err(|_| "Skill 链接或共享源已变化")?;
        }
        if let Some(package) = &self.package {
            package.revalidate()?;
        }
        Ok(())
    }

    pub fn apply(&self, before_commit: impl Fn() -> Result<(), String>) -> Result<(), String> {
        if let Some(reference) = &self.reference {
            return remove_skill_reference(reference, before_commit);
        }
        self.revalidate()?;
        #[cfg(unix)]
        {
            use rustix::fs::{unlinkat, AtFlags};
            let package = self.package.as_ref().ok_or("Skill 包未准备")?;
            // Delete only previewed members. Never recurse over a new directory
            // listing; unexpected files make the final empty-directory removal fail.
            for anchor in package
                .anchors
                .iter()
                .filter(|anchor| anchor.source_kind() == AgentAssetSourceKind::File)
            {
                let file = reopen_verified_path(anchor).map_err(|_| "Skill 文件已变化")?;
                let bytes = file
                    .read_file_bounded(package::MAX_FILE_BYTES)
                    .map_err(|_| "Skill 文件不可读取")?;
                if !anchor.matches_bytes(&bytes) {
                    return Err("Skill 内容已变化，请重新预览".to_owned());
                }
                file.revalidate().map_err(|_| "Skill 文件已变化")?;
                let parent = file.parent_handle().ok_or("Skill 文件父目录不可用")?;
                before_commit()?;
                file.revalidate().map_err(|_| "Skill 文件在提交前已变化")?;
                unlinkat(
                    parent,
                    anchor
                        .display_path()
                        .file_name()
                        .ok_or("Skill 文件名无效")?,
                    AtFlags::empty(),
                )
                .map_err(|_| "Skill 文件移除失败；已完成的移除不会回滚")?;
                parent
                    .sync_all()
                    .map_err(|_| "Skill 文件已移除，但目录同步失败")?;
            }
            let mut directories = package
                .anchors
                .iter()
                .filter(|anchor| anchor.source_kind() == AgentAssetSourceKind::Directory)
                .collect::<Vec<_>>();
            directories.sort_by_key(|anchor| {
                std::cmp::Reverse(anchor.display_path().components().count())
            });
            for anchor in directories {
                let directory = reopen_verified_directory_identity(anchor)
                    .map_err(|_| "Skill 目录对象已变化")?;
                let parent = directory.parent_handle().ok_or("Skill 父目录不可用")?;
                before_commit()?;
                directory
                    .revalidate_identity()
                    .map_err(|_| "Skill 目录在提交前已变化")?;
                unlinkat(
                    parent,
                    anchor
                        .display_path()
                        .file_name()
                        .ok_or("Skill 目录名无效")?,
                    AtFlags::REMOVEDIR,
                )
                .map_err(|_| "Skill 目录无法移除，可能存在新文件；未删除未预览的内容")?;
                parent
                    .sync_all()
                    .map_err(|_| "Skill 目录已移除，但同步失败")?;
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = before_commit;
            Err("当前平台尚未提供目录移除执行器".to_owned())
        }
    }

    pub fn absent(&self) -> Result<bool, String> {
        let parent = self.path.parent().ok_or("Skill 父目录无效")?;
        let guard = inspect_verified_path(
            &[&self.root],
            &self.root,
            parent,
            AgentAssetSourceKind::Directory,
        )
        .map_err(|_| "移除后 Skill 父目录无法核对")?;
        let name = self.path.file_name().ok_or("Skill 目录名无效")?;
        for entry in guard.read_entries().map_err(|_| "无法核对 Skill 目录")? {
            if entry.map_err(|_| "无法核对 Skill 目录")?.name == name {
                return Ok(false);
            }
        }
        guard.revalidate().map_err(|_| "核对期间 Skill 目录变化")?;
        Ok(true)
    }
}
