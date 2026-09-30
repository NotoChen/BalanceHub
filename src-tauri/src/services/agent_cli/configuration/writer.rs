//! Configuration transaction staging. Every target is prepared before the first
//! file is replaced; Windows keeps verified ancestors through ReplaceFileW.
use super::errors::{conflict, internal};
use crate::{
    models::*,
    services::agent_cli::{
        config_support::directories,
        environment::{
            mutation::GuardedFile,
            verified_path::{inspect_verified_path, VerifiedPathGuard},
        },
    },
};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct StagedFile {
    source: GuardedFile,
    parent: VerifiedPathGuard,
    temporary: PathBuf,
    replacement: Vec<u8>,
    unchanged: bool,
    removed: bool,
}
pub(super) enum CommitResult {
    Unchanged,
    Applied,
    AppliedNotSynced,
}
impl StagedFile {
    pub(super) fn prepare(
        source: &GuardedFile,
        replacement: &[u8],
        checkpoint: impl Fn() -> Result<(), AgentConfigurationError>,
    ) -> Result<Self, AgentConfigurationError> {
        checkpoint()?;
        source.revalidate().map_err(|_| conflict())?;
        let parent_path = source
            .creation_ancestor()
            .unwrap_or_else(|| source.path().parent().expect("native file parent"));
        let parent = inspect_verified_path(
            &[parent_path],
            parent_path,
            parent_path,
            AgentAssetSourceKind::Directory,
        )
        .map_err(|_| conflict())?;
        let name = format!(
            ".balancehub-config-{}.tmp",
            crate::services::agent_cli::environment::mutation::token::opaque_id()
                .map_err(|_| internal())?
        );
        let temporary = parent_path.join(&name);
        let unchanged = source.bytes() == Some(replacement);
        if !unchanged {
            let mut file = create_staging(&parent, &temporary, &name)?;
            let result = (|| {
                if let Some(anchor) = source.verified_anchor() {
                    let guard=crate::services::agent_cli::environment::verified_path::reopen_verified_path(anchor).map_err(|_|conflict())?;
                    file.set_permissions(guard.metadata().map_err(|_| conflict())?.permissions())
                        .map_err(|_| failed())?;
                }
                for bytes in replacement.chunks(64 * 1024) {
                    checkpoint()?;
                    file.write_all(bytes).map_err(|_| failed())?;
                }
                file.sync_all().map_err(|_| failed())
            })();
            if let Err(error) = result {
                drop(file);
                remove_staging(&parent, &temporary);
                return Err(error);
            }
        }
        Ok(Self {
            source: source.clone(),
            parent,
            temporary,
            replacement: replacement.to_vec(),
            unchanged,
            removed: unchanged,
        })
    }
    pub(super) fn ensure_parent(
        &mut self,
        before_commit: impl Fn() -> Result<(), AgentConfigurationError>,
        after_create: impl Fn(),
    ) -> Result<(), AgentConfigurationError> {
        let Some(ancestor) = self.source.creation_ancestor().map(Path::to_path_buf) else {
            return Ok(());
        };
        let target_parent = self
            .source
            .path()
            .parent()
            .ok_or_else(internal)?
            .to_path_buf();
        if ancestor == target_parent {
            return Ok(());
        }
        self.source.revalidate().map_err(|_| conflict())?;
        let boundary_error = std::cell::RefCell::new(None);
        directories::ensure_directory_observed(
            &ancestor,
            &target_parent,
            || {
                before_commit().map_err(|error| {
                    let message = error.message.clone();
                    *boundary_error.borrow_mut() = Some(error);
                    message
                })
            },
            after_create,
        )
        .map_err(|_| boundary_error.into_inner().unwrap_or_else(failed))?;
        self.source
            .reanchor_after_parent_creation(&target_parent)
            .map_err(|_| conflict())?;
        self.relocate_staging()?;
        Ok(())
    }
    pub(super) fn advance_after_creation(
        &mut self,
        parent: &Path,
    ) -> Result<(), AgentConfigurationError> {
        self.source
            .reanchor_after_parent_creation(parent)
            .map_err(|_| conflict())?;
        self.relocate_staging()
    }
    pub(super) fn target_parent(&self) -> Option<&Path> {
        self.source.path().parent()
    }
    fn relocate_staging(&mut self) -> Result<(), AgentConfigurationError> {
        let target_parent = self.source.path().parent().ok_or_else(internal)?;
        if self.unchanged
            || self.parent.display_path() == target_parent
            || self.source.creation_ancestor() != Some(target_parent)
        {
            return Ok(());
        }
        let parent = self
            .source
            .reopen_creation_parent()
            .map_err(|_| conflict())?;
        self.parent.revalidate_identity().map_err(|_| conflict())?;
        let name = self.temporary.file_name().ok_or_else(internal)?;
        let new_path = target_parent.join(name);
        relocate(&self.parent, &self.temporary, &parent, &new_path)?;
        self.temporary = new_path;
        self.parent = parent;
        Ok(())
    }
    pub(super) fn commit(
        &mut self,
        before_commit: impl FnOnce() -> Result<(), AgentConfigurationError>,
    ) -> Result<CommitResult, AgentConfigurationError> {
        if self.unchanged {
            return Ok(CommitResult::Unchanged);
        }
        self.source.revalidate().map_err(|_| conflict())?;
        self.parent.revalidate_identity().map_err(|_| conflict())?;
        let result = commit_staging(self, before_commit)?;
        self.removed = true;
        Ok(result)
    }
    pub(super) fn verify(&self) -> Result<(), AgentConfigurationError> {
        let parent = self.source.path().parent().ok_or_else(internal)?;
        let guard = inspect_verified_path(
            &[parent],
            parent,
            self.source.path(),
            AgentAssetSourceKind::File,
        )
        .map_err(|_| conflict())?;
        let bytes = guard
            .read_file_bounded(super::MAX_DOCUMENT_BYTES)
            .map_err(|_| failed())?;
        if bytes != self.replacement {
            return Err(conflict());
        }
        guard.revalidate().map_err(|_| conflict())
    }
}
impl Drop for StagedFile {
    fn drop(&mut self) {
        if !self.removed {
            remove_staging(&self.parent, &self.temporary);
        }
    }
}

#[cfg(unix)]
fn create_staging(
    parent: &VerifiedPathGuard,
    _path: &Path,
    name: &str,
) -> Result<File, AgentConfigurationError> {
    crate::services::agent_cli::environment::mutation::atomic::create_staging(
        parent.source_handle(),
        std::ffi::OsStr::new(name),
    )
    .map_err(Into::into)
}
#[cfg(windows)]
fn create_staging(
    _parent: &VerifiedPathGuard,
    path: &Path,
    _name: &str,
) -> Result<File, AgentConfigurationError> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| failed())
}
#[cfg(unix)]
fn remove_staging(parent: &VerifiedPathGuard, path: &Path) {
    if let Some(name) = path.file_name() {
        let _ = rustix::fs::unlinkat(parent.source_handle(), name, rustix::fs::AtFlags::empty());
    }
}
#[cfg(windows)]
fn remove_staging(_parent: &VerifiedPathGuard, path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
fn commit_staging(
    stage: &StagedFile,
    before_commit: impl FnOnce() -> Result<(), AgentConfigurationError>,
) -> Result<CommitResult, AgentConfigurationError> {
    use crate::services::agent_cli::environment::mutation::atomic::{self, AtomicWriteResult};
    let present = stage.source.bytes().is_some();
    let target = if present {
        stage.source.reopen_present()
    } else {
        stage.source.reopen_creation_parent()
    }
    .map_err(|_| conflict())?;
    let target_parent = if present {
        target.parent_handle().ok_or_else(conflict)?
    } else {
        target.source_handle()
    };
    let name = stage.source.path().file_name().ok_or_else(internal)?;
    let temporary = stage.temporary.file_name().ok_or_else(internal)?;
    before_commit()?;
    match atomic::commit_staging(
        stage.parent.source_handle(),
        temporary,
        target_parent,
        name,
        present,
    )? {
        AtomicWriteResult::Unchanged => Ok(CommitResult::Unchanged),
        AtomicWriteResult::Replaced => Ok(CommitResult::Applied),
        AtomicWriteResult::ReplacedNotSynced => Ok(CommitResult::AppliedNotSynced),
    }
}

#[cfg(unix)]
fn relocate(
    from: &VerifiedPathGuard,
    source: &Path,
    to: &VerifiedPathGuard,
    target: &Path,
) -> Result<(), AgentConfigurationError> {
    use rustix::fs::{linkat, unlinkat, AtFlags};
    let source = source.file_name().ok_or_else(internal)?;
    let target = target.file_name().ok_or_else(internal)?;
    linkat(
        from.source_handle(),
        source,
        to.source_handle(),
        target,
        AtFlags::empty(),
    )
    .map_err(|_| failed())?;
    if to.source_handle().sync_all().is_err() {
        let _ = unlinkat(to.source_handle(), target, AtFlags::empty());
        return Err(failed());
    }
    if unlinkat(from.source_handle(), source, AtFlags::empty()).is_err() {
        let _ = unlinkat(to.source_handle(), target, AtFlags::empty());
        return Err(failed());
    }
    Ok(())
}

#[cfg(windows)]
fn relocate(
    from: &VerifiedPathGuard,
    source: &Path,
    to: &VerifiedPathGuard,
    target: &Path,
) -> Result<(), AgentConfigurationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    from.revalidate_identity().map_err(|_| conflict())?;
    to.revalidate_identity().map_err(|_| conflict())?;
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(failed());
    }
    Ok(())
}

#[cfg(windows)]
fn commit_staging(
    stage: &StagedFile,
    before_commit: impl FnOnce() -> Result<(), AgentConfigurationError>,
) -> Result<CommitResult, AgentConfigurationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH,
    };
    let target_parent = stage.source.path().parent().ok_or_else(internal)?;
    // This handle chain forbids deletion/renaming of every ancestor. The file
    // guard itself must be released before Windows replacement (no delete share).
    let parent = inspect_verified_path(
        &[target_parent],
        target_parent,
        target_parent,
        AgentAssetSourceKind::Directory,
    )
    .map_err(|_| conflict())?;
    stage.source.revalidate().map_err(|_| conflict())?;
    let source = stage
        .temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = stage
        .source
        .path()
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    before_commit()?;
    // No backup+rename fallback: an unsupported/reparse/racing target fails.
    // Missing MoveFileExW deliberately omits REPLACE_EXISTING (no clobber).
    let applied = unsafe {
        if stage.source.bytes().is_some() {
            ReplaceFileW(
                target.as_ptr(),
                source.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH)
        }
    };
    if applied == 0 {
        return Err(failed());
    }
    parent.revalidate_identity().map_err(|_| conflict())?;
    Ok(CommitResult::Applied)
}
fn failed() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::WriteFailed)
}
