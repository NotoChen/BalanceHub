#[cfg(unix)]
use super::files::conflict;
use super::files::GuardedFile;
#[cfg(not(unix))]
use crate::models::AgentAssetActionUnavailableReason;
use crate::models::AgentAssetMutationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicWriteResult {
    #[cfg(unix)]
    Unchanged,
    #[cfg(unix)]
    Replaced,
    #[cfg(unix)]
    ReplacedNotSynced,
}

#[cfg(unix)]
pub(crate) fn replace(
    source: &GuardedFile,
    replacement: &[u8],
    before_commit: impl FnOnce() -> Result<(), AgentAssetMutationError>,
) -> Result<AtomicWriteResult, AgentAssetMutationError> {
    use rustix::fs::{unlinkat, AtFlags};
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let present = source.bytes().is_some();
    let guard = if present {
        source.reopen_present()?
    } else {
        source.reopen_creation_parent()?
    };
    if source.bytes() == Some(replacement) {
        return Ok(AtomicWriteResult::Unchanged);
    }
    let parent = if present {
        guard.parent_handle().ok_or_else(conflict)?
    } else {
        guard.source_handle()
    };
    let name = source.path().file_name().ok_or_else(conflict)?;
    let temporary = format!(".balancehub-{}.tmp", super::token::opaque_id()?);
    let mut file = create_staging(parent, std::ffi::OsStr::new(&temporary))?;
    let pending = (|| {
        let mode = if present {
            guard
                .metadata()
                .map_err(|_| conflict())?
                .permissions()
                .mode()
        } else {
            0o600
        };
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|_| write_failed())?;
        file.write_all(replacement).map_err(|_| write_failed())?;
        file.sync_all().map_err(|_| write_failed())?;
        source.revalidate()?;
        if present {
            guard.revalidate().map_err(|_| conflict())?;
        } else {
            guard.revalidate_identity().map_err(|_| conflict())?;
        }
        before_commit()?;
        commit_staging(
            parent,
            std::ffi::OsStr::new(&temporary),
            parent,
            name,
            present,
        )
    })();
    if pending.is_err() {
        let _ = unlinkat(parent, temporary.as_str(), AtFlags::empty());
    }
    pending
}

/// Native configuration and resource writers share the same no-follow staging
/// and no-clobber commit primitives; neither accepts a client pathname.
#[cfg(unix)]
pub(crate) fn create_staging(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
) -> Result<std::fs::File, AgentAssetMutationError> {
    use rustix::fs::{openat, Mode, OFlags};
    openat(
        parent,
        name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map(std::fs::File::from)
    .map_err(|_| write_failed())
}

#[cfg(unix)]
pub(crate) fn commit_staging(
    from_parent: &std::fs::File,
    temporary: &std::ffi::OsStr,
    target_parent: &std::fs::File,
    name: &std::ffi::OsStr,
    present: bool,
) -> Result<AtomicWriteResult, AgentAssetMutationError> {
    use rustix::fs::{linkat, renameat, unlinkat, AtFlags};
    if present {
        renameat(from_parent, temporary, target_parent, name).map_err(|_| write_failed())?;
    } else {
        linkat(
            from_parent,
            temporary,
            target_parent,
            name,
            AtFlags::empty(),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                conflict()
            } else {
                write_failed()
            }
        })?;
        let _ = unlinkat(from_parent, temporary, AtFlags::empty());
    }
    Ok(if target_parent.sync_all().is_ok() {
        AtomicWriteResult::Replaced
    } else {
        AtomicWriteResult::ReplacedNotSynced
    })
}

#[cfg(not(unix))]
pub(crate) fn replace(
    _source: &GuardedFile,
    _replacement: &[u8],
    _before_commit: impl FnOnce() -> Result<(), AgentAssetMutationError>,
) -> Result<AtomicWriteResult, AgentAssetMutationError> {
    Err(AgentAssetMutationError::unavailable(
        AgentAssetActionUnavailableReason::UnsupportedPlatform,
    ))
}

#[cfg(unix)]
fn write_failed() -> AgentAssetMutationError {
    AgentAssetMutationError::new(crate::models::AgentAssetMutationErrorKind::PreparationFailed)
}
