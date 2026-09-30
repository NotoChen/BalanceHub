use crate::{
    models::{
        AgentAssetRevision, AgentAssetSourceKind, AgentLifecycleError, AgentLifecycleErrorKind,
    },
    services::agent_cli::environment::verified_path::{
        inspect_verified_path, reopen_verified_path, VerifiedPathAnchor,
    },
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Clone)]
pub(super) struct FileStamp {
    anchor: VerifiedPathAnchor,
}

impl FileStamp {
    pub(super) fn read(path: &Path, limit: usize) -> Result<(Self, Vec<u8>), AgentLifecycleError> {
        let root = path.parent().ok_or_else(changed)?;
        let guard = inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File)
            .map_err(|_| changed())?;
        let bytes = guard.read_file_bounded(limit).map_err(|_| changed())?;
        let mut digest = Sha256::new();
        guard.update_revision(&mut digest);
        digest.update(&bytes);
        let revision = AgentAssetRevision {
            identity: format!("{:x}", digest.finalize()),
            observed_at: String::new(),
            size_bytes: Some(bytes.len() as u64),
            is_missing: false,
            is_directory: false,
            is_symlink: false,
        };
        Ok((
            Self {
                anchor: guard.anchor(&revision, Some(&bytes)),
            },
            bytes,
        ))
    }

    pub(super) fn signature(&self) -> &str {
        &self.anchor.revision().identity
    }

    pub(super) fn revalidate(&self) -> Result<(), AgentLifecycleError> {
        let guard = reopen_verified_path(&self.anchor).map_err(|_| changed())?;
        let limit = self.anchor.revision().size_bytes.ok_or_else(changed)? as usize;
        let bytes = guard.read_file_bounded(limit).map_err(|_| changed())?;
        if !self.anchor.matches_bytes(&bytes) {
            return Err(changed());
        }
        guard.revalidate().map_err(|_| changed())
    }
}

pub(super) fn owned_writable_directory(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.permissions().readonly()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o300 == 0o300
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(super) fn changed() -> AgentLifecycleError {
    AgentLifecycleError::new(AgentLifecycleErrorKind::TargetChanged)
}
