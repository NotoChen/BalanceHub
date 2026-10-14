use super::super::package;
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
        let package = self.package.as_ref().ok_or("Skill 包未准备")?;
        for anchor in package
            .anchors
            .iter()
            .filter(|anchor| anchor.source_kind() == AgentAssetSourceKind::File)
        {
            let relative = anchor
                .physical_path()
                .strip_prefix(&self.path)
                .map_err(|_| "Skill 文件越界")?;
            let key = relative.to_string_lossy().replace('\\', "/");
            let file = package.files.get(&key).ok_or("Skill 文件不在预览中")?;
            crate::services::agent_cli::environment::verified_path::removal::remove(
                anchor,
                Some(&file.bytes),
                &before_commit,
            )?;
        }
        let mut directories = package
            .anchors
            .iter()
            .filter(|anchor| anchor.source_kind() == AgentAssetSourceKind::Directory)
            .collect::<Vec<_>>();
        directories
            .sort_by_key(|anchor| std::cmp::Reverse(anchor.display_path().components().count()));
        for anchor in directories {
            crate::services::agent_cli::environment::verified_path::removal::remove(
                anchor,
                None,
                &before_commit,
            )?;
        }
        Ok(())
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
