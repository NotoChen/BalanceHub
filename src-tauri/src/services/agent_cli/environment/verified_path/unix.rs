use super::{DirectoryEntryCandidate, DirectoryLinkEvidence, VerifiedPathError};
use crate::models::AgentAssetSourceKind;
use rustix::fs::{open, openat, readlinkat_raw, statat, AtFlags, Dir, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, Metadata},
    io,
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectIdentity {
    device: u128,
    inode: u128,
    kind: AgentAssetSourceKind,
}

impl ObjectIdentity {
    fn from_metadata(metadata: &Metadata) -> Result<Self, VerifiedPathError> {
        let kind = metadata_kind(metadata)?;
        Ok(Self {
            device: metadata.dev() as u128,
            inode: metadata.ino() as u128,
            kind,
        })
    }

    fn from_stat(stat: &rustix::fs::Stat) -> Result<Self, VerifiedPathError> {
        let kind = match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => AgentAssetSourceKind::Directory,
            FileType::RegularFile => AgentAssetSourceKind::File,
            FileType::Symlink => return Err(VerifiedPathError::SymlinkRejected),
            _ => return Err(VerifiedPathError::SourceChanged),
        };
        Ok(Self {
            device: stat.st_dev as u128,
            inode: stat.st_ino as u128,
            kind,
        })
    }

    fn update_revision(self, hasher: &mut Sha256) {
        hasher.update(self.device.to_le_bytes());
        hasher.update(self.inode.to_le_bytes());
        hasher.update([self.kind as u8]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectStamp {
    identity: ObjectIdentity,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

impl ObjectStamp {
    fn from_metadata(metadata: &Metadata) -> Result<Self, VerifiedPathError> {
        Ok(Self {
            identity: ObjectIdentity::from_metadata(metadata)?,
            size: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }

    fn update_revision(self, hasher: &mut Sha256) {
        self.identity.update_revision(hasher);
        hasher.update(self.size.to_le_bytes());
        hasher.update(self.modified_seconds.to_le_bytes());
        hasher.update(self.modified_nanoseconds.to_le_bytes());
        hasher.update(self.changed_seconds.to_le_bytes());
        hasher.update(self.changed_nanoseconds.to_le_bytes());
    }
}

struct PathNode {
    handle: File,
    component: Option<OsString>,
    identity: ObjectIdentity,
}

#[derive(Debug, Clone)]
pub(super) struct PlatformAnchor {
    identities: Vec<ObjectIdentity>,
    source_stamp: ObjectStamp,
    allowed_root_index: usize,
}

pub(super) struct PlatformGuard {
    nodes: Vec<PathNode>,
    source_stamp: ObjectStamp,
    allowed_root_index: usize,
}

impl PlatformGuard {
    pub(super) fn open(
        path: &Path,
        allowed_root: &Path,
        kind: AgentAssetSourceKind,
    ) -> Result<Self, VerifiedPathError> {
        let components = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect::<Vec<_>>();
        let allowed_root_index = allowed_root
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .count();
        let directory_flags =
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let root = File::from(open("/", directory_flags, Mode::empty()).map_err(io::Error::from)?);
        let root_identity = ObjectIdentity::from_metadata(&root.metadata()?)?;
        let mut nodes = vec![PathNode {
            handle: root,
            component: None,
            identity: root_identity,
        }];
        for (index, component) in components.iter().enumerate() {
            let final_node = index + 1 == components.len();
            let expected_kind = if final_node {
                kind
            } else {
                AgentAssetSourceKind::Directory
            };
            let flags = if expected_kind == AgentAssetSourceKind::Directory {
                directory_flags
            } else {
                // Nonblocking prevents an unexpected FIFO/device from hanging
                // before fstat can reject its type.
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK
            };
            let parent = &nodes.last().expect("filesystem root handle").handle;
            let child = match openat(parent, *component, flags, Mode::empty()) {
                Ok(handle) => File::from(handle),
                Err(error) => {
                    if let Ok(stat) = statat(parent, *component, AtFlags::SYMLINK_NOFOLLOW) {
                        match FileType::from_raw_mode(stat.st_mode) {
                            FileType::Symlink => return Err(VerifiedPathError::SymlinkRejected),
                            FileType::RegularFile
                                if expected_kind == AgentAssetSourceKind::Directory =>
                            {
                                return Err(VerifiedPathError::TypeMismatch {
                                    actual: AgentAssetSourceKind::File,
                                });
                            }
                            FileType::Directory if expected_kind == AgentAssetSourceKind::File => {
                                return Err(VerifiedPathError::TypeMismatch {
                                    actual: AgentAssetSourceKind::Directory,
                                });
                            }
                            _ => {}
                        }
                    }
                    return Err(VerifiedPathError::Io(io::Error::from(error)));
                }
            };
            let identity = ObjectIdentity::from_metadata(&child.metadata()?)?;
            if identity.kind != expected_kind {
                return Err(VerifiedPathError::TypeMismatch {
                    actual: identity.kind,
                });
            }
            validate_binding(parent, component, identity)?;
            nodes.push(PathNode {
                handle: child,
                component: Some(component.to_os_string()),
                identity,
            });
        }
        let source_stamp = ObjectStamp::from_metadata(
            &nodes
                .last()
                .expect("filesystem root handle")
                .handle
                .metadata()?,
        )?;
        if source_stamp.identity.kind != kind {
            return Err(VerifiedPathError::TypeMismatch {
                actual: source_stamp.identity.kind,
            });
        }
        Ok(Self {
            nodes,
            source_stamp,
            allowed_root_index,
        })
    }

    pub(super) fn source_handle(&self) -> &File {
        &self.nodes.last().expect("verified source handle").handle
    }

    pub(super) fn parent_handle(&self) -> Option<&File> {
        self.nodes
            .len()
            .checked_sub(2)
            .map(|index| &self.nodes[index].handle)
    }

    pub(super) fn read_entries(&self) -> io::Result<DirectoryReader> {
        let inner = Dir::read_from(self.source_handle()).map_err(io::Error::from)?;
        Ok(DirectoryReader { inner })
    }

    pub(super) fn read_directory_link(
        &self,
        entry_name: &OsStr,
    ) -> Result<DirectoryLinkEvidence, VerifiedPathError> {
        self.revalidate()?;
        let before = statat(self.source_handle(), entry_name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)?;
        let identity = link_identity(&before)?;
        let mut buffer = [0u8; 4096];
        let count = readlinkat_raw(self.source_handle(), entry_name, &mut buffer[..])
            .map_err(io::Error::from)?;
        if count == 0 || count >= buffer.len() {
            return Err(VerifiedPathError::TooLarge);
        }
        let after = statat(self.source_handle(), entry_name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)?;
        if link_identity(&after)? != identity {
            return Err(VerifiedPathError::SourceChanged);
        }
        self.revalidate()?;
        Ok(DirectoryLinkEvidence {
            identity,
            target_text: PathBuf::from(OsString::from_vec(buffer[..count].to_vec())),
        })
    }

    pub(super) fn revalidate(&self) -> Result<(), VerifiedPathError> {
        self.revalidate_identity()?;
        if ObjectStamp::from_metadata(&self.source_handle().metadata()?)? != self.source_stamp {
            return Err(VerifiedPathError::SourceChanged);
        }
        Ok(())
    }

    pub(super) fn revalidate_identity(&self) -> Result<(), VerifiedPathError> {
        for (index, node) in self.nodes.iter().enumerate() {
            let current = ObjectIdentity::from_metadata(&node.handle.metadata()?)?;
            if current != node.identity {
                return Err(self.changed(index));
            }
            if let Some(component) = node.component.as_deref() {
                validate_binding(&self.nodes[index - 1].handle, component, node.identity).map_err(
                    |error| match error {
                        VerifiedPathError::SymlinkRejected => error,
                        _ => self.changed(index),
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn anchor(&self) -> PlatformAnchor {
        PlatformAnchor {
            identities: self.nodes.iter().map(|node| node.identity).collect(),
            source_stamp: self.source_stamp,
            allowed_root_index: self.allowed_root_index,
        }
    }

    pub(super) fn compare_anchor(
        &self,
        expected: &PlatformAnchor,
    ) -> Result<(), VerifiedPathError> {
        self.compare_anchor_identity(expected)?;
        if self.source_stamp != expected.source_stamp {
            return Err(VerifiedPathError::SourceChanged);
        }
        Ok(())
    }

    pub(super) fn compare_anchor_identity(
        &self,
        expected: &PlatformAnchor,
    ) -> Result<(), VerifiedPathError> {
        if self.nodes.len() != expected.identities.len()
            || self.allowed_root_index != expected.allowed_root_index
        {
            return Err(VerifiedPathError::RootChanged);
        }
        for (index, (node, expected)) in self.nodes.iter().zip(&expected.identities).enumerate() {
            if node.identity != *expected {
                return Err(self.changed(index));
            }
        }
        Ok(())
    }

    pub(super) fn update_revision(&self, hasher: &mut Sha256) {
        hasher.update(b"unix-verified-path-v1");
        hasher.update((self.allowed_root_index as u64).to_le_bytes());
        for node in &self.nodes {
            node.identity.update_revision(hasher);
        }
        self.source_stamp.update_revision(hasher);
    }

    fn changed(&self, index: usize) -> VerifiedPathError {
        if index <= self.allowed_root_index {
            VerifiedPathError::RootChanged
        } else {
            VerifiedPathError::SourceChanged
        }
    }
}

fn link_identity(stat: &rustix::fs::Stat) -> Result<[u8; 32], VerifiedPathError> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Symlink {
        return Err(VerifiedPathError::SymlinkRejected);
    }
    let mut hasher = Sha256::new();
    for value in [
        stat.st_dev as i128,
        stat.st_ino as i128,
        stat.st_mode as i128,
        stat.st_size as i128,
        stat.st_mtime as i128,
        stat.st_mtime_nsec as i128,
        stat.st_ctime as i128,
        stat.st_ctime_nsec as i128,
    ] {
        hasher.update(value.to_le_bytes());
    }
    Ok(hasher.finalize().into())
}

fn metadata_kind(metadata: &Metadata) -> Result<AgentAssetSourceKind, VerifiedPathError> {
    if metadata.file_type().is_symlink() {
        Err(VerifiedPathError::SymlinkRejected)
    } else if metadata.is_dir() {
        Ok(AgentAssetSourceKind::Directory)
    } else if metadata.is_file() {
        Ok(AgentAssetSourceKind::File)
    } else {
        Err(VerifiedPathError::SourceChanged)
    }
}

fn validate_binding(
    parent: &File,
    component: &OsStr,
    expected: ObjectIdentity,
) -> Result<(), VerifiedPathError> {
    let stat = statat(parent, component, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
    if ObjectIdentity::from_stat(&stat)? != expected {
        return Err(VerifiedPathError::SourceChanged);
    }
    Ok(())
}

pub(crate) struct DirectoryReader {
    inner: Dir,
}

impl Iterator for DirectoryReader {
    type Item = io::Result<DirectoryEntryCandidate>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.inner.next()? {
                Ok(entry) => entry,
                Err(error) => return Some(Err(io::Error::from(error))),
            };
            let name_bytes = entry.file_name().to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            let entry_type = match entry.file_type() {
                FileType::Directory => (AgentAssetSourceKind::Directory, false),
                FileType::Symlink => (AgentAssetSourceKind::File, true),
                FileType::Unknown => match entry_type_from_stat(&self.inner, entry.file_name()) {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                },
                _ => (AgentAssetSourceKind::File, false),
            };
            return Some(Ok(DirectoryEntryCandidate {
                name: OsString::from_vec(name_bytes.to_vec()),
                source_kind: entry_type.0,
                is_symlink: entry_type.1,
            }));
        }
    }
}

fn entry_type_from_stat(
    dir: &Dir,
    name: &rustix::ffi::CStr,
) -> io::Result<(AgentAssetSourceKind, bool)> {
    let stat = statat(
        dir.fd().map_err(io::Error::from)?,
        name,
        AtFlags::SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)?;
    Ok(match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => (AgentAssetSourceKind::Directory, false),
        FileType::Symlink => (AgentAssetSourceKind::File, true),
        _ => (AgentAssetSourceKind::File, false),
    })
}
