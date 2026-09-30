//! One handle-bound path boundary for inventory, preview and native mutation.

use super::identity::lexical_absolute;
use crate::models::{AgentAssetRevision, AgentAssetSourceKind};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::{DirectoryReader, PlatformAnchor, PlatformGuard};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{DirectoryReader, PlatformAnchor, PlatformGuard};

#[cfg(not(any(unix, windows)))]
compile_error!("Agent asset paths support Unix and Windows desktop targets only");

pub(crate) mod remove_reference;

const MAX_VERIFIED_COMPONENTS: usize = 256;

#[derive(Debug)]
pub(crate) enum VerifiedPathError {
    OutsideAllowedRoot,
    SymlinkRejected,
    TypeMismatch { actual: AgentAssetSourceKind },
    RootChanged,
    SourceChanged,
    TooLarge,
    Io(io::Error),
}

impl From<io::Error> for VerifiedPathError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl VerifiedPathError {
    pub(crate) fn into_io_error(self) -> io::Error {
        match self {
            Self::Io(error) => error,
            Self::OutsideAllowedRoot => io::Error::from(io::ErrorKind::InvalidInput),
            _ => io::Error::from(io::ErrorKind::InvalidData),
        }
    }
}

/// Owned evidence only. Holding an inventory generation never holds OS handles.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedPathAnchor {
    trusted_root: PathBuf,
    allowed_root: PathBuf,
    path: PathBuf,
    kind: AgentAssetSourceKind,
    platform: PlatformAnchor,
    revision: AgentAssetRevision,
    content_digest: Option<[u8; 32]>,
    file_times: (Option<SystemTime>, Option<SystemTime>),
    readonly_skill_link: Option<Box<ReadonlySkillLinkAnchor>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectoryLinkEvidence {
    identity: [u8; 32],
    target_text: PathBuf,
}

#[derive(Debug, Clone)]
struct ReadonlySkillLinkAnchor {
    manifest: Box<VerifiedPathAnchor>,
    entry_name: String,
    declared_path: PathBuf,
    evidence: DirectoryLinkEvidence,
}

struct ReadonlySkillLinkGuard {
    manifest: Box<VerifiedPathGuard>,
    anchor: ReadonlySkillLinkAnchor,
}

impl VerifiedPathAnchor {
    pub(crate) fn file_times(&self) -> (Option<SystemTime>, Option<SystemTime>) {
        self.file_times
    }

    pub(crate) fn revision(&self) -> &AgentAssetRevision {
        &self.revision
    }

    pub(crate) fn display_path(&self) -> &Path {
        self.readonly_skill_link
            .as_ref()
            .map_or(&self.path, |link| &link.declared_path)
    }

    /// Identity captured with the snapshot bytes; this is not new path authority.
    pub(crate) fn physical_path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn allowed_root(&self) -> &Path {
        self.readonly_skill_link
            .as_ref()
            .map_or(&self.allowed_root, |link| link.manifest.allowed_root())
    }

    pub(crate) fn is_readonly_reference(&self) -> bool {
        self.readonly_skill_link.is_some()
    }

    pub(crate) fn source_kind(&self) -> AgentAssetSourceKind {
        self.kind
    }

    pub(crate) fn matches_bytes(&self, bytes: &[u8]) -> bool {
        self.content_digest
            .is_some_and(|expected| expected == <[u8; 32]>::from(Sha256::digest(bytes)))
    }
}

pub(crate) struct VerifiedPathGuard {
    trusted_root: PathBuf,
    allowed_root: PathBuf,
    path: PathBuf,
    kind: AgentAssetSourceKind,
    platform: PlatformGuard,
    readonly_skill_link: Option<Box<ReadonlySkillLinkGuard>>,
}

/// Resolve the adapter's boundary without canonicalizing any untrusted path.
/// Every filesystem lookup happens in the platform's retained handle chain.
pub(crate) fn inspect_verified_path(
    trusted_roots: &[&Path],
    allowed_root: &Path,
    path: &Path,
    kind: AgentAssetSourceKind,
) -> Result<VerifiedPathGuard, VerifiedPathError> {
    let allowed_root =
        lexical_absolute(allowed_root).ok_or(VerifiedPathError::OutsideAllowedRoot)?;
    let path = lexical_absolute(path).ok_or(VerifiedPathError::OutsideAllowedRoot)?;
    let trusted_root = trusted_roots
        .iter()
        .filter_map(|root| lexical_absolute(root))
        .filter(|root| allowed_root.starts_with(root))
        .max_by_key(|root| root.components().count())
        .ok_or(VerifiedPathError::OutsideAllowedRoot)?;
    if !path.starts_with(&allowed_root) || path.components().count() > MAX_VERIFIED_COMPONENTS {
        return Err(VerifiedPathError::OutsideAllowedRoot);
    }
    let platform = PlatformGuard::open(&path, &allowed_root, kind)?;
    platform.revalidate()?;
    Ok(VerifiedPathGuard {
        trusted_root,
        allowed_root,
        path,
        kind,
        platform,
        readonly_skill_link: None,
    })
}

/// Follow only one manifest-selected directory link. Its textual target is
/// normalized before any lookup and must be one direct child of the explicit
/// native shared Skill root; all remaining lookups still use NoFollow.
pub(crate) fn inspect_readonly_skill_link(
    trusted_roots: &[&Path],
    manifest_anchor: &VerifiedPathAnchor,
    entry_name: &str,
    shared_root: &Path,
) -> Result<VerifiedPathGuard, VerifiedPathError> {
    let mut entry_components = Path::new(entry_name).components();
    if manifest_anchor.kind != AgentAssetSourceKind::Directory
        || manifest_anchor.is_readonly_reference()
        || !matches!(
            (entry_components.next(), entry_components.next()),
            (Some(Component::Normal(_)), None)
        )
    {
        return Err(VerifiedPathError::SymlinkRejected);
    }
    let manifest = reopen_verified_path(manifest_anchor)?;
    let evidence = manifest.platform.read_directory_link(entry_name.as_ref())?;
    let shared_root = lexical_absolute(shared_root).ok_or(VerifiedPathError::OutsideAllowedRoot)?;
    let target = normalize_skill_link_target(manifest.display_path(), &evidence.target_text)?;
    if target.parent() != Some(shared_root.as_path()) {
        return Err(VerifiedPathError::OutsideAllowedRoot);
    }
    let mut guard = inspect_verified_path(
        trusted_roots,
        &shared_root,
        &target.join("SKILL.md"),
        AgentAssetSourceKind::File,
    )?;
    let declared_path = manifest.display_path().join(entry_name).join("SKILL.md");
    guard.readonly_skill_link = Some(Box::new(ReadonlySkillLinkGuard {
        manifest: Box::new(manifest),
        anchor: ReadonlySkillLinkAnchor {
            manifest: Box::new(manifest_anchor.clone()),
            entry_name: entry_name.to_owned(),
            declared_path,
            evidence,
        },
    }));
    guard.revalidate()?;
    Ok(guard)
}

fn normalize_skill_link_target(
    parent: &Path,
    target_text: &Path,
) -> Result<PathBuf, VerifiedPathError> {
    let absolute = target_text.is_absolute();
    let component_count = target_text.components().count()
        + if absolute {
            0
        } else {
            parent.components().count()
        };
    if component_count > MAX_VERIFIED_COMPONENTS {
        return Err(VerifiedPathError::OutsideAllowedRoot);
    }
    let mut normalized = if absolute {
        PathBuf::new()
    } else {
        parent.to_path_buf()
    };
    // Only a leading relative prefix can climb ancestors already retained by
    // the manifest guard. Collapsing `name/..` could skip a missing directory
    // or another symlink and disagree with the OS's declared-path resolution.
    let mut may_ascend_manifest = !absolute;
    for component in target_text.components() {
        match component {
            Component::ParentDir if may_ascend_manifest => {
                if !normalized.pop() {
                    return Err(VerifiedPathError::OutsideAllowedRoot);
                }
            }
            Component::ParentDir => return Err(VerifiedPathError::OutsideAllowedRoot),
            Component::CurDir => {}
            _ => {
                may_ascend_manifest = false;
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

pub(crate) fn reopen_verified_path(
    anchor: &VerifiedPathAnchor,
) -> Result<VerifiedPathGuard, VerifiedPathError> {
    let readonly_skill_link = if let Some(link) = &anchor.readonly_skill_link {
        let manifest = reopen_verified_path(&link.manifest)?;
        if manifest
            .platform
            .read_directory_link(link.entry_name.as_ref())?
            != link.evidence
        {
            return Err(VerifiedPathError::SourceChanged);
        }
        Some(Box::new(ReadonlySkillLinkGuard {
            manifest: Box::new(manifest),
            anchor: (**link).clone(),
        }))
    } else {
        None
    };
    let mut guard = inspect_verified_path(
        &[anchor.trusted_root.as_path()],
        &anchor.allowed_root,
        &anchor.path,
        anchor.kind,
    )?;
    guard.platform.compare_anchor(&anchor.platform)?;
    guard.readonly_skill_link = readonly_skill_link;
    guard.revalidate()?;
    Ok(guard)
}

/// A mutation of a missing sibling changes the containing directory's times.
/// That operation needs its parent identity, not an unchanged directory manifest.
pub(crate) fn reopen_verified_directory_identity(
    anchor: &VerifiedPathAnchor,
) -> Result<VerifiedPathGuard, VerifiedPathError> {
    if anchor.is_readonly_reference() {
        return Err(VerifiedPathError::SymlinkRejected);
    }
    if anchor.kind != AgentAssetSourceKind::Directory {
        return Err(VerifiedPathError::TypeMismatch {
            actual: anchor.kind,
        });
    }
    let guard = inspect_verified_path(
        &[anchor.trusted_root.as_path()],
        &anchor.allowed_root,
        &anchor.path,
        anchor.kind,
    )?;
    guard.platform.compare_anchor_identity(&anchor.platform)?;
    guard.revalidate_identity()?;
    Ok(guard)
}

impl VerifiedPathGuard {
    pub(crate) fn metadata(&self) -> Result<Metadata, VerifiedPathError> {
        self.source_handle().metadata().map_err(Into::into)
    }

    pub(crate) fn revalidate(&self) -> Result<(), VerifiedPathError> {
        self.platform.revalidate()?;
        if let Some(link) = &self.readonly_skill_link {
            link.manifest.revalidate()?;
            if link
                .manifest
                .platform
                .read_directory_link(link.anchor.entry_name.as_ref())?
                != link.anchor.evidence
            {
                return Err(VerifiedPathError::SourceChanged);
            }
        }
        Ok(())
    }

    pub(crate) fn revalidate_identity(&self) -> Result<(), VerifiedPathError> {
        if self.readonly_skill_link.is_some() {
            return Err(VerifiedPathError::SymlinkRejected);
        }
        self.platform.revalidate_identity()
    }

    pub(crate) fn display_path(&self) -> &Path {
        self.readonly_skill_link
            .as_ref()
            .map_or(&self.path, |link| &link.anchor.declared_path)
    }

    /// A bounded reader may use the proven physical location while retaining
    /// this guard and revalidating afterwards. This is never write authority.
    pub(crate) fn verified_read_path(&self) -> (&Path, &Path) {
        (&self.path, &self.allowed_root)
    }

    pub(crate) fn read_file_bounded(&self, limit: usize) -> Result<Vec<u8>, VerifiedPathError> {
        self.read_file_bounded_then(limit, || {})
    }

    fn read_file_bounded_then<F>(
        &self,
        limit: usize,
        after_read: F,
    ) -> Result<Vec<u8>, VerifiedPathError>
    where
        F: FnOnce(),
    {
        if self.kind != AgentAssetSourceKind::File {
            return Err(VerifiedPathError::TypeMismatch { actual: self.kind });
        }
        self.revalidate()?;
        let size = self.metadata()?.len();
        if size > limit as u64 {
            return Err(VerifiedPathError::TooLarge);
        }
        let mut file = self.source_handle().try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity((size as usize).min(limit));
        file.take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        after_read();
        if bytes.len() > limit {
            return Err(VerifiedPathError::TooLarge);
        }
        self.revalidate()?;
        Ok(bytes)
    }

    #[cfg(unix)]
    pub(crate) fn read_entries(&self) -> io::Result<DirectoryReader> {
        self.platform.read_entries()
    }

    #[cfg(windows)]
    pub(crate) fn read_entries(&self) -> io::Result<DirectoryReader<'_>> {
        self.platform.read_entries()
    }

    pub(crate) fn anchor(
        &self,
        revision: &AgentAssetRevision,
        bytes: Option<&[u8]>,
    ) -> VerifiedPathAnchor {
        VerifiedPathAnchor {
            trusted_root: self.trusted_root.clone(),
            allowed_root: self.allowed_root.clone(),
            path: self.path.clone(),
            kind: self.kind,
            platform: self.platform.anchor(),
            revision: revision.clone(),
            content_digest: bytes.map(|bytes| Sha256::digest(bytes).into()),
            file_times: self
                .metadata()
                .ok()
                .map(|metadata| (metadata.created().ok(), metadata.modified().ok()))
                .unwrap_or_default(),
            readonly_skill_link: self
                .readonly_skill_link
                .as_ref()
                .map(|link| Box::new(link.anchor.clone())),
        }
    }

    pub(crate) fn update_revision(&self, hasher: &mut Sha256) {
        for path in [&self.trusted_root, &self.allowed_root, &self.path] {
            let value = path.as_os_str().as_encoded_bytes();
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
        }
        self.platform.update_revision(hasher);
        if let Some(link) = &self.readonly_skill_link {
            hasher.update(b"readonly-native-skill-link-v1");
            hasher.update(link.anchor.manifest.revision.identity.as_bytes());
            hasher.update(link.anchor.evidence.identity);
            for path in [
                &link.anchor.declared_path,
                &link.anchor.evidence.target_text,
            ] {
                let bytes = path.as_os_str().as_encoded_bytes();
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
        }
    }

    /// Native same-directory writes use these retained handles, never a fresh
    /// pathname walk. Their lifetime cannot exceed this guard.
    pub(crate) fn source_handle(&self) -> &File {
        self.platform.source_handle()
    }

    #[cfg(unix)]
    pub(crate) fn parent_handle(&self) -> Option<&File> {
        if self.readonly_skill_link.is_some() {
            None
        } else {
            self.platform.parent_handle()
        }
    }
}

pub(crate) struct DirectoryEntryCandidate {
    pub(crate) name: OsString,
    pub(crate) source_kind: AgentAssetSourceKind,
    pub(crate) is_symlink: bool,
}

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod readonly_skill_links;
