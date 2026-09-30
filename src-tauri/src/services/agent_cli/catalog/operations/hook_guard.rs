use super::super::{digest, package};
use crate::{
    models::*,
    services::agent_cli::environment::{
        mutation::{GuardedDirectory, GuardedFile, MutationInventory},
        verified_path::{reopen_verified_path, VerifiedPathAnchor},
    },
};

#[derive(Clone)]
pub(super) enum HookReadGuard {
    File {
        file: GuardedFile,
        #[cfg(unix)]
        root: std::path::PathBuf,
    },
    Directory(VerifiedPathAnchor),
    MissingDirectory(std::sync::Arc<GuardedDirectory>),
    Reference {
        anchor: VerifiedPathAnchor,
        digest: String,
    },
}

impl HookReadGuard {
    pub(super) fn retained_private_bytes(&self) -> usize {
        match self {
            Self::File { file, .. } => file.bytes().map_or(0, <[u8]>::len),
            Self::Directory(_) | Self::MissingDirectory(_) | Self::Reference { .. } => 0,
        }
    }

    pub fn capture(
        snapshot: &MutationInventory,
        source: &AgentAssetSource,
    ) -> Result<Self, String> {
        if source.source_kind == AgentAssetSourceKind::Directory {
            if source.revision.is_missing {
                let guard = GuardedDirectory::capture_path(std::path::Path::new(&source.path))
                    .map_err(|_| "Hook 依赖目录缺失证据已变化")?;
                if !matches!(guard, GuardedDirectory::Missing { .. }) {
                    return Err("Hook 依赖目录已出现".to_owned());
                }
                return Ok(Self::MissingDirectory(std::sync::Arc::new(guard)));
            }
            let anchor = snapshot
                .source_anchors
                .get(&source.id)
                .ok_or("Hook 依赖目录无法确认")?;
            if source.revision.identity != anchor.revision().identity {
                return Err("Hook 依赖目录代际不一致".to_owned());
            }
            reopen_verified_path(anchor)
                .and_then(|guard| guard.revalidate())
                .map_err(|_| "Hook 依赖目录已变化")?;
            return Ok(Self::Directory(anchor.clone()));
        }
        if let Some(anchor) = snapshot
            .source_anchors
            .get(&source.id)
            .filter(|anchor| anchor.is_readonly_reference())
        {
            if source.revision.identity != anchor.revision().identity {
                return Err("Hook 依赖来源代际不一致".to_owned());
            }
            let guard = reopen_verified_path(anchor).map_err(|_| "Hook 只读依赖无法安全打开")?;
            let bytes = guard
                .read_file_bounded(package::MAX_FILE_BYTES)
                .map_err(|_| "Hook 只读依赖无法完整读取")?;
            if !anchor.matches_bytes(&bytes) {
                return Err("Hook 只读依赖已变化".to_owned());
            }
            guard.revalidate().map_err(|_| "Hook 只读依赖已变化")?;
            Ok(Self::Reference {
                anchor: anchor.clone(),
                digest: digest(&bytes),
            })
        } else {
            GuardedFile::capture(source, &snapshot.source_anchors)
                .map(|file| Self::File {
                    file,
                    #[cfg(unix)]
                    root: std::path::PathBuf::from(&source.allowed_root),
                })
                .map_err(|_| "Hook 原生依赖无法安全准备".to_owned())
        }
    }
    pub fn signature(&self) -> String {
        match self {
            Self::File { file, .. } => file.signature(),
            Self::Directory(anchor) => format!("directory:{}", anchor.revision().identity),
            Self::MissingDirectory(guard) => guard.signature(),
            Self::Reference { anchor, digest } => {
                format!("{}:{digest}", anchor.revision().identity)
            }
        }
    }
    pub fn lock_domains(&self) -> Vec<String> {
        match self {
            Self::File { file, .. } => file.lock_domains(),
            _ => Vec::new(),
        }
    }

    #[cfg(unix)]
    pub fn domain(&self) -> Option<String> {
        match self {
            Self::File { file, .. } => Some(file.domain()),
            Self::Directory(_) | Self::MissingDirectory(_) | Self::Reference { .. } => None,
        }
    }
    pub fn revalidate(&self) -> Result<(), String> {
        match self {
            Self::File { file, .. } => file
                .revalidate()
                .map_err(|_| "Hook 原生依赖已变化".to_owned()),
            Self::Directory(anchor) => reopen_verified_path(anchor)
                .and_then(|guard| guard.revalidate())
                .map_err(|_| "Hook 依赖目录成员已变化".to_owned()),
            Self::MissingDirectory(guard) => guard
                .revalidate()
                .map_err(|_| "Hook 依赖目录已出现".to_owned()),
            Self::Reference {
                anchor,
                digest: expected,
            } => {
                let guard = reopen_verified_path(anchor).map_err(|_| "Hook 只读依赖已变化")?;
                let bytes = guard
                    .read_file_bounded(package::MAX_FILE_BYTES)
                    .map_err(|_| "Hook 只读依赖无法完整读取")?;
                if digest(&bytes) != *expected {
                    return Err("Hook 只读依赖内容已变化".to_owned());
                }
                guard
                    .revalidate()
                    .map_err(|_| "Hook 只读依赖已变化".to_owned())
            }
        }
    }

    /// Advance only missing file evidence along the parent's verified creation
    /// chain. Directory and readonly-reference dependencies keep their original
    /// evidence, and the file's source and lock domain remain unchanged.
    #[cfg(unix)]
    pub fn reanchor_after_parent_creation(
        &mut self,
        ensured_parent: &std::path::Path,
    ) -> Result<(), String> {
        if let Self::File { file, .. } = self {
            file.reanchor_after_parent_creation(ensured_parent)
                .map_err(|_| "Hook 缺失文件的原路径证据在目录创建期间变化".to_owned())?;
        }
        Ok(())
    }

    /// The same physical file may have several observed source IDs. Once this
    /// transaction replaces it, every alias checks the committed bytes rather
    /// than comparing an obsolete inode/content snapshot or skipping the read.
    #[cfg(unix)]
    pub fn revalidate_after_write(&self, expected: &[u8]) -> Result<(), String> {
        let Self::File { file, root } = self else {
            return self.revalidate();
        };
        let current = package::capture_file(root, file.path(), package::MAX_FILE_BYTES)?;
        if current.bytes() != Some(expected) {
            return Err("已提交的 Hook 文件再次变化".to_owned());
        }
        current
            .revalidate()
            .map_err(|_| "已提交的 Hook 文件路径再次变化".to_owned())
    }
}
