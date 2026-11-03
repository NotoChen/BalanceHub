//! Native asset mutations use the same staged writer as configuration editing.
use super::files::GuardedFile;
use crate::{
    models::{
        AgentAssetMutationError, AgentAssetMutationErrorKind, AgentConfigurationError,
        AgentConfigurationErrorKind,
    },
    services::agent_cli::config_support::file_writer::{CommitResult, StagedFile},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtomicWriteResult {
    Unchanged,
    Replaced,
    #[cfg(unix)]
    ReplacedNotSynced,
}

pub(crate) fn replace(
    source: &GuardedFile,
    replacement: &[u8],
    before_commit: impl FnOnce() -> Result<(), AgentAssetMutationError>,
) -> Result<AtomicWriteResult, AgentAssetMutationError> {
    let mut staged = StagedFile::prepare(source, replacement, || Ok(())).map_err(write_error)?;
    let mut boundary_error = None;
    let result = staged.commit(|| {
        before_commit().map_err(|error| {
            boundary_error = Some(error);
            AgentConfigurationError::new(AgentConfigurationErrorKind::Canceled)
        })
    });
    if let Some(error) = boundary_error {
        return Err(error);
    }
    match result.map_err(write_error)? {
        CommitResult::Unchanged => Ok(AtomicWriteResult::Unchanged),
        CommitResult::Applied => Ok(AtomicWriteResult::Replaced),
        #[cfg(unix)]
        CommitResult::AppliedNotSynced => Ok(AtomicWriteResult::ReplacedNotSynced),
    }
}

fn write_error(error: AgentConfigurationError) -> AgentAssetMutationError {
    let kind = match error.kind {
        AgentConfigurationErrorKind::SourceChanged | AgentConfigurationErrorKind::RootChanged => {
            AgentAssetMutationErrorKind::SourceConflict
        }
        AgentConfigurationErrorKind::ReadOnly => AgentAssetMutationErrorKind::ActionUnavailable,
        _ => AgentAssetMutationErrorKind::PreparationFailed,
    };
    let mut result = AgentAssetMutationError::new(kind);
    result.message = error.message;
    result
}
