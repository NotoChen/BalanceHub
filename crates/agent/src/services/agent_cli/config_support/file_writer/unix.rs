use super::{conflict, failed, CommitResult};
use crate::models::AgentConfigurationError;
use rustix::fs::{linkat, openat, renameat, unlinkat, AtFlags, Mode, OFlags};
use std::{ffi::OsStr, fs::File};

pub(super) fn create_staging(parent: &File, name: &OsStr) -> Result<File, AgentConfigurationError> {
    openat(
        parent,
        name,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map(File::from)
    .map_err(|_| failed())
}

pub(super) fn commit_staging(
    from_parent: &File,
    temporary: &OsStr,
    target_parent: &File,
    name: &OsStr,
    present: bool,
) -> Result<CommitResult, AgentConfigurationError> {
    if present {
        renameat(from_parent, temporary, target_parent, name).map_err(|_| failed())?;
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
                failed()
            }
        })?;
        let _ = unlinkat(from_parent, temporary, AtFlags::empty());
    }
    Ok(if target_parent.sync_all().is_ok() {
        CommitResult::Applied
    } else {
        CommitResult::AppliedNotSynced
    })
}
