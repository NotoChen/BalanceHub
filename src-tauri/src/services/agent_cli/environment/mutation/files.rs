use super::super::verified_path::{
    inspect_verified_path, reopen_verified_path, VerifiedPathAnchor,
};
use super::super::verified_path::{reopen_verified_directory_identity, VerifiedPathGuard};
use crate::models::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Backend-only source evidence. Contents and path anchors never enter a plan DTO.
#[derive(Clone)]
pub(crate) struct GuardedFile {
    pub(crate) source_id: String,
    path: PathBuf,
    allowed_root: PathBuf,
    evidence: FileEvidence,
    bytes: Option<Arc<[u8]>>,
    lock_identity: String,
    byte_limit: usize,
}

#[derive(Clone)]
enum FileEvidence {
    Present(VerifiedPathAnchor),
    /// Explicitly metadata-only sources never carry credential-cache bytes.
    Metadata(VerifiedPathAnchor),
    Missing {
        parent: VerifiedPathAnchor,
    },
}

pub(crate) enum GuardedDirectory {
    Present(VerifiedPathAnchor),
    Missing {
        path: PathBuf,
        parent: VerifiedPathAnchor,
    },
}

impl GuardedDirectory {
    pub(crate) fn capture_path(path: &Path) -> Result<Self, AgentAssetMutationError> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing {
                path: path.to_path_buf(),
                parent: capture_missing_parent(path)?,
            }),
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let guard =
                    inspect_verified_path(&[path], path, path, AgentAssetSourceKind::Directory)
                        .map_err(|_| unavailable())?;
                let mut digest = Sha256::new();
                guard.update_revision(&mut digest);
                let revision = AgentAssetRevision {
                    identity: format!("{:x}", digest.finalize()),
                    observed_at: String::new(),
                    size_bytes: None,
                    is_missing: false,
                    is_directory: true,
                    is_symlink: false,
                };
                Ok(Self::Present(guard.anchor(&revision, None)))
            }
            _ => Err(unavailable()),
        }
    }

    pub(super) fn capture(
        source: &AgentAssetSource,
        anchors: &BTreeMap<String, VerifiedPathAnchor>,
    ) -> Result<Self, AgentAssetMutationError> {
        if anchors
            .get(&source.id)
            .is_some_and(VerifiedPathAnchor::is_readonly_reference)
        {
            return Err(unavailable());
        }
        if source.source_kind != AgentAssetSourceKind::Directory || source.revision.is_symlink {
            return Err(unavailable());
        }
        if source.revision.is_missing {
            let path = PathBuf::from(&source.path);
            let parent = capture_missing_parent(&path)?;
            return Ok(Self::Missing { path, parent });
        }
        let anchor = anchors.get(&source.id).ok_or_else(unavailable)?.clone();
        if anchor.revision().identity != source.revision.identity {
            return Err(conflict());
        }
        reopen_verified_path(&anchor).map_err(|_| conflict())?;
        Ok(Self::Present(anchor))
    }

    pub(crate) fn signature(&self) -> String {
        let mut hash = Sha256::new();
        match self {
            Self::Present(anchor) => {
                hash.update([1]);
                hash.update(anchor.display_path().as_os_str().as_encoded_bytes());
                hash.update(anchor.revision().identity.as_bytes());
            }
            Self::Missing { path, parent } => {
                hash.update([0]);
                hash.update(path.as_os_str().as_encoded_bytes());
                hash.update(parent.display_path().as_os_str().as_encoded_bytes());
                hash.update(parent.revision().identity.as_bytes());
            }
        }
        format!("{:x}", hash.finalize())
    }

    pub(crate) fn revalidate(&self) -> Result<(), AgentAssetMutationError> {
        match self {
            Self::Present(anchor) => reopen_verified_path(anchor)
                .map(|_| ())
                .map_err(|_| conflict()),
            Self::Missing { path, parent } => revalidate_missing(parent, path),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteObservation {
    Unchanged,
    Changed,
    Unknown,
}

impl GuardedFile {
    pub(crate) fn capture_metadata_path(
        root: &Path,
        path: &Path,
    ) -> Result<Self, AgentAssetMutationError> {
        if !root.is_absolute()
            || !path.starts_with(root)
            || path
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(unavailable());
        }
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // The leaf may appear after this observation. Missing evidence
                // must fail closed instead of entering the text reader then.
                return Self::capture_missing_path(root, path, "", 0);
            }
            Err(_) => return Err(unavailable()),
            Ok(_) => {}
        }
        let guard = inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File)
            .map_err(|_| unavailable())?;
        let metadata = guard.metadata().map_err(|_| unavailable())?;
        let mut hash = Sha256::new();
        guard.update_revision(&mut hash);
        let revision = AgentAssetRevision {
            identity: format!("{:x}", hash.finalize()),
            observed_at: String::new(),
            size_bytes: Some(metadata.len()),
            is_missing: false,
            is_directory: false,
            is_symlink: false,
        };
        guard.revalidate().map_err(|_| conflict())?;
        Ok(Self {
            source_id: String::new(),
            path: path.to_owned(),
            allowed_root: root.to_owned(),
            evidence: FileEvidence::Metadata(guard.anchor(&revision, None)),
            bytes: None,
            lock_identity: physical_identity(&metadata, path),
            byte_limit: 0,
        })
    }

    fn capture_missing_path(
        root: &Path,
        path: &Path,
        source_id: &str,
        byte_limit: usize,
    ) -> Result<Self, AgentAssetMutationError> {
        let parent = capture_missing_parent(path)?;
        let lock_identity = format!(
            "missing:{}:{}",
            parent.revision().identity,
            path.strip_prefix(parent.display_path())
                .map_err(|_| unavailable())?
                .to_string_lossy()
                .to_lowercase()
        );
        Ok(Self {
            source_id: source_id.to_owned(),
            path: path.to_owned(),
            allowed_root: root.to_owned(),
            evidence: FileEvidence::Missing { parent },
            bytes: None,
            lock_identity,
            byte_limit,
        })
    }

    /// Capture an exact destination selected by a trusted backend. IPC paths
    /// must first be resolved through the owning adapter's target registry.
    pub(crate) fn capture_path(
        root: &Path,
        path: &Path,
        byte_limit: usize,
    ) -> Result<Self, AgentAssetMutationError> {
        if !root.is_absolute()
            || !path.starts_with(root)
            || path
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(unavailable());
        }
        let id = crate::services::agent_cli::environment::stable_id(
            "catalog-source",
            &[&path.to_string_lossy()],
        );
        if byte_limit > 64 * 1024 * 1024 {
            return Err(unavailable());
        }
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Self::capture_missing_path(root, path, &id, byte_limit);
            }
            Err(_) => return Err(unavailable()),
            Ok(_) => {}
        }
        let guard = inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File)
            .map_err(|_| unavailable())?;
        let bytes = guard
            .read_file_bounded(byte_limit)
            .map_err(|_| unavailable())?;
        let mut hash = Sha256::new();
        guard.update_revision(&mut hash);
        hash.update(&bytes);
        let revision = AgentAssetRevision {
            identity: format!("{:x}", hash.finalize()),
            observed_at: String::new(),
            size_bytes: Some(bytes.len() as u64),
            is_missing: false,
            is_directory: false,
            is_symlink: false,
        };
        let anchor = guard.anchor(&revision, Some(&bytes));
        let lock_identity = physical_identity(&guard.metadata().map_err(|_| conflict())?, path);
        guard.revalidate().map_err(|_| conflict())?;
        // The retained handle already proved these exact bytes. Do not create
        // a synthetic inventory record and reopen/read the same file again.
        Ok(Self {
            source_id: id,
            path: path.to_owned(),
            allowed_root: root.to_owned(),
            evidence: FileEvidence::Present(anchor),
            bytes: Some(bytes.into()),
            lock_identity,
            byte_limit,
        })
    }

    pub(crate) fn capture(
        source: &AgentAssetSource,
        anchors: &BTreeMap<String, VerifiedPathAnchor>,
    ) -> Result<Self, AgentAssetMutationError> {
        Self::capture_with_limit(source, anchors, AgentAssetLimits::DEFAULT.bytes_per_source)
    }

    pub(crate) fn capture_with_limit(
        source: &AgentAssetSource,
        anchors: &BTreeMap<String, VerifiedPathAnchor>,
        byte_limit: usize,
    ) -> Result<Self, AgentAssetMutationError> {
        Self::capture_snapshot(source, anchors, byte_limit, false)
    }

    /// Retain verified bytes for a resource reader. Read-only link evidence is
    /// still refused by all mutation capture paths and never grants a write.
    pub(crate) fn capture_for_read(
        source: &AgentAssetSource,
        anchors: &BTreeMap<String, VerifiedPathAnchor>,
        byte_limit: usize,
    ) -> Result<Self, AgentAssetMutationError> {
        Self::capture_snapshot(source, anchors, byte_limit, true)
    }

    fn capture_snapshot(
        source: &AgentAssetSource,
        anchors: &BTreeMap<String, VerifiedPathAnchor>,
        byte_limit: usize,
        readonly: bool,
    ) -> Result<Self, AgentAssetMutationError> {
        // Display paths or even a rewritten canonical path cannot turn a
        // read-only native reference anchor into mutation evidence.
        if !readonly
            && anchors
                .get(&source.id)
                .is_some_and(VerifiedPathAnchor::is_readonly_reference)
        {
            return Err(unavailable());
        }
        if byte_limit > 64 * 1024 * 1024 {
            return Err(unavailable());
        }
        if source.source_kind != AgentAssetSourceKind::File || source.revision.is_symlink {
            return Err(unavailable());
        }
        let path = PathBuf::from(&source.path);
        let allowed_root = PathBuf::from(&source.allowed_root);
        if source.revision.is_missing {
            return Self::capture_missing_path(&allowed_root, &path, &source.id, byte_limit);
        }
        let anchor = anchors.get(&source.id).ok_or_else(unavailable)?.clone();
        if anchor.revision().identity != source.revision.identity {
            return Err(conflict());
        }
        let guard = reopen_verified_path(&anchor).map_err(|_| conflict())?;
        let bytes = guard
            .read_file_bounded(byte_limit)
            .map_err(|_| unavailable())?;
        if !anchor.matches_bytes(&bytes) {
            return Err(conflict());
        }
        guard.revalidate().map_err(|_| conflict())?;
        let lock_identity = physical_identity(&guard.metadata().map_err(|_| conflict())?, &path);
        Ok(Self {
            source_id: source.id.clone(),
            path,
            allowed_root,
            evidence: FileEvidence::Present(anchor),
            bytes: Some(bytes.into()),
            lock_identity,
            byte_limit,
        })
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn revision(&self) -> AgentAssetRevision {
        match &self.evidence {
            FileEvidence::Present(anchor) | FileEvidence::Metadata(anchor) => {
                anchor.revision().clone()
            }
            FileEvidence::Missing { .. } => AgentAssetRevision {
                identity: self.signature(),
                is_missing: true,
                ..AgentAssetRevision::default()
            },
        }
    }

    pub(crate) fn verified_anchor(&self) -> Option<&VerifiedPathAnchor> {
        match &self.evidence {
            FileEvidence::Present(anchor) | FileEvidence::Metadata(anchor) => Some(anchor),
            FileEvidence::Missing { .. } => None,
        }
    }

    pub(crate) fn creation_ancestor(&self) -> Option<&Path> {
        match &self.evidence {
            FileEvidence::Missing { parent } => Some(parent.display_path()),
            _ => None,
        }
    }

    /// Fixed pathname identity complements inode domains through atomic replace.
    /// A new generation must still wait for an older writer of the same path.
    pub(crate) fn pathname_domain(&self) -> String {
        #[cfg(not(windows))]
        let value = self.path.as_os_str().as_encoded_bytes();
        #[cfg(windows)]
        let value = self.path.to_string_lossy().to_lowercase().into_bytes();
        format!("pathname:{:x}", Sha256::digest(value))
    }

    pub(crate) fn lock_domains(&self) -> Vec<String> {
        vec![self.domain(), self.pathname_domain()]
    }

    pub(crate) fn domain(&self) -> String {
        // File identity serializes hard links and case aliases across contexts.
        // Missing siblings use the retained parent identity plus relative name.
        format!("source:{}", self.lock_identity)
    }

    pub(crate) fn signature(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.source_id.as_bytes());
        hash.update([0]);
        hash.update(self.path.as_os_str().as_encoded_bytes());
        match &self.evidence {
            FileEvidence::Present(anchor) | FileEvidence::Metadata(anchor) => {
                hash.update([1]);
                hash.update(anchor.revision().identity.as_bytes());
                if let Some(bytes) = &self.bytes {
                    hash.update(bytes);
                }
            }
            FileEvidence::Missing { parent } => {
                hash.update([0]);
                hash.update(parent.display_path().as_os_str().as_encoded_bytes());
                hash.update(parent.revision().identity.as_bytes());
            }
        }
        format!("{:x}", hash.finalize())
    }

    pub(crate) fn revalidate(&self) -> Result<(), AgentAssetMutationError> {
        match &self.evidence {
            FileEvidence::Metadata(anchor) => reopen_verified_path(anchor)
                .map(|_| ())
                .map_err(|_| conflict()),
            FileEvidence::Present(anchor) => {
                let guard = reopen_verified_path(anchor).map_err(|_| conflict())?;
                let bytes = guard
                    .read_file_bounded(self.byte_limit)
                    .map_err(|_| conflict())?;
                if !anchor.matches_bytes(&bytes) {
                    return Err(conflict());
                }
                guard.revalidate().map_err(|_| conflict())
            }
            FileEvidence::Missing { parent } => revalidate_missing(parent, &self.path),
        }
    }

    /// Advance missing-file evidence only along a parent chain that this
    /// transaction has just ensured. Callers must validate the old read set
    /// before creating directories. The original source and lock domain remain
    /// stable so aliases still join the same in-flight write.
    pub(crate) fn reanchor_after_parent_creation(
        &mut self,
        ensured_parent: &Path,
    ) -> Result<(), AgentAssetMutationError> {
        let FileEvidence::Missing { parent } = &self.evidence else {
            return Ok(());
        };
        if !ensured_parent.is_absolute()
            || ensured_parent
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(unavailable());
        }
        let previous_parent = parent.display_path();
        let Ok(ensured_relative) = ensured_parent.strip_prefix(previous_parent) else {
            return Ok(());
        };
        let target_parent = self.path.parent().ok_or_else(conflict)?;
        let target_relative = target_parent
            .strip_prefix(previous_parent)
            .map_err(|_| conflict())?;
        let mut advanced_parent = previous_parent.to_path_buf();
        for (target, ensured) in target_relative
            .components()
            .zip(ensured_relative.components())
        {
            if target != ensured {
                break;
            }
            let Component::Normal(name) = target else {
                return Err(conflict());
            };
            advanced_parent.push(name);
        }
        if advanced_parent == previous_parent {
            return Ok(());
        }
        let previous = reopen_verified_directory_identity(parent).map_err(|_| conflict())?;
        require_missing(&self.path)?;
        let advanced = inspect_verified_path(
            &[previous_parent],
            previous_parent,
            &advanced_parent,
            AgentAssetSourceKind::Directory,
        )
        .map_err(|_| conflict())?;
        let revision = AgentAssetRevision {
            identity: physical_identity(
                &advanced.metadata().map_err(|_| conflict())?,
                &advanced_parent,
            ),
            is_directory: true,
            ..AgentAssetRevision::default()
        };
        let anchor = advanced.anchor(&revision, None);
        // A sibling outside the ensured chain may still have missing parents.
        // Its first uncreated component must remain absent, not merely its leaf.
        require_same_missing_component(&anchor, &self.path)?;
        advanced.revalidate_identity().map_err(|_| conflict())?;
        previous.revalidate_identity().map_err(|_| conflict())?;
        self.evidence = FileEvidence::Missing { parent: anchor };
        Ok(())
    }

    pub(crate) fn reopen_creation_parent(
        &self,
    ) -> Result<VerifiedPathGuard, AgentAssetMutationError> {
        self.revalidate()?;
        match &self.evidence {
            FileEvidence::Missing { parent }
                if self.path.parent() == Some(parent.display_path()) =>
            {
                reopen_verified_directory_identity(parent).map_err(|_| conflict())
            }
            _ => Err(unavailable()),
        }
    }

    #[cfg(unix)]
    pub(crate) fn reopen_present(&self) -> Result<VerifiedPathGuard, AgentAssetMutationError> {
        self.revalidate()?;
        match &self.evidence {
            FileEvidence::Present(anchor) => reopen_verified_path(anchor).map_err(|_| conflict()),
            FileEvidence::Missing { .. } | FileEvidence::Metadata(_) => Err(unavailable()),
        }
    }

    pub(crate) fn reopen_metadata(&self) -> Result<VerifiedPathGuard, AgentAssetMutationError> {
        match &self.evidence {
            FileEvidence::Metadata(anchor) => reopen_verified_path(anchor).map_err(|_| conflict()),
            _ => Err(unavailable()),
        }
    }

    pub(crate) fn observe_write(&self) -> WriteObservation {
        if matches!(self.evidence, FileEvidence::Metadata(_)) {
            return WriteObservation::Unknown;
        }
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if self.bytes.is_none() {
                    WriteObservation::Unchanged
                } else {
                    WriteObservation::Changed
                }
            }
            Err(_) => WriteObservation::Unknown,
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                WriteObservation::Unknown
            }
            Ok(_) => {
                let root = self.allowed_root.as_path();
                let bytes =
                    inspect_verified_path(&[root], root, &self.path, AgentAssetSourceKind::File)
                        .and_then(|guard| guard.read_file_bounded(self.byte_limit));
                match bytes {
                    Ok(bytes) if self.bytes.as_deref() == Some(bytes.as_slice()) => {
                        WriteObservation::Unchanged
                    }
                    Ok(_) => WriteObservation::Changed,
                    Err(_) => WriteObservation::Unknown,
                }
            }
        }
    }
}

fn capture_missing_parent(path: &Path) -> Result<VerifiedPathAnchor, AgentAssetMutationError> {
    require_missing(path)?;
    let mut parent = path.parent().ok_or_else(unavailable)?;
    loop {
        match fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parent = parent.parent().ok_or_else(unavailable)?;
            }
            _ => return Err(unavailable()),
        }
    }
    let guard = inspect_verified_path(&[parent], parent, parent, AgentAssetSourceKind::Directory)
        .map_err(|_| unavailable())?;
    require_missing(path)?;
    guard.revalidate().map_err(|_| conflict())?;
    // This anchor binds only the pre-existing ancestor. Missing paths never
    // acquire read authority or a synthetic file handle.
    let revision = AgentAssetRevision {
        identity: physical_identity(&guard.metadata().map_err(|_| conflict())?, parent),
        observed_at: String::new(),
        size_bytes: None,
        is_missing: false,
        is_directory: true,
        is_symlink: false,
    };
    Ok(guard.anchor(&revision, None))
}

fn physical_identity(metadata: &fs::Metadata, path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        format!("{}:{}", metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        path.to_string_lossy().to_lowercase()
    }
}

fn require_missing(path: &Path) -> Result<(), AgentAssetMutationError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(conflict()),
    }
}

fn require_same_missing_component(
    parent: &VerifiedPathAnchor,
    path: &Path,
) -> Result<(), AgentAssetMutationError> {
    let component = path
        .strip_prefix(parent.display_path())
        .map_err(|_| conflict())?
        .components()
        .next()
        .ok_or_else(conflict)?;
    // The retained parent was the nearest existing ancestor. Its first absent
    // child must remain absent; checking only the leaf could follow a new link.
    require_missing(&parent.display_path().join(component.as_os_str()))
}

fn revalidate_missing(
    parent: &VerifiedPathAnchor,
    path: &Path,
) -> Result<(), AgentAssetMutationError> {
    let guard = reopen_verified_directory_identity(parent).map_err(|_| conflict())?;
    require_same_missing_component(parent, path)?;
    guard.revalidate_identity().map_err(|_| conflict())
}

pub(super) fn conflict() -> AgentAssetMutationError {
    AgentAssetMutationError::new(AgentAssetMutationErrorKind::SourceConflict)
}

fn unavailable() -> AgentAssetMutationError {
    AgentAssetMutationError::unavailable(AgentAssetActionUnavailableReason::SourceUnavailable)
}
