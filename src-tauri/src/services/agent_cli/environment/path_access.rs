//! Opaque access orchestration. This module never authorizes a caller pathname.

use super::{
    access_registry::{
        AgentAssetAccessAnchor, AgentAssetAccessRegistry, AgentAssetAccessRequest,
        AgentAssetAccessTargetKind,
    },
    preview::{render_preview, AgentPreviewMetadataReason, AgentPreviewOutput},
    snapshot::system_time_millis,
    verified_path::{
        reopen_verified_path, VerifiedPathAnchor, VerifiedPathError, VerifiedPathGuard,
    },
};
use crate::models::{
    AgentAssetAccessError, AgentAssetAccessErrorKind, AgentAssetAccessRisk, AgentAssetActionKind,
    AgentAssetOpenTarget, AgentAssetReadDiagnostic, AgentAssetReadResult, AgentAssetSourceKind,
};
use std::{path::Path, sync::Arc};

pub(crate) fn read_asset(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    require_target_kind(request, AgentAssetAccessTargetKind::Asset)?;
    read_target(registry, request)
}

pub(crate) fn read_source(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    require_target_kind(request, AgentAssetAccessTargetKind::Source)?;
    read_target(registry, request)
}

pub(crate) fn open_asset<F>(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
    target: AgentAssetOpenTarget,
    accepted_risks: &[AgentAssetAccessRisk],
    opener: F,
) -> Result<(), AgentAssetAccessError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    require_target_kind(request, AgentAssetAccessTargetKind::Asset)?;
    open_target(registry, request, target, accepted_risks, opener)
}

pub(crate) fn open_source<F>(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
    target: AgentAssetOpenTarget,
    accepted_risks: &[AgentAssetAccessRisk],
    opener: F,
) -> Result<(), AgentAssetAccessError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    require_target_kind(request, AgentAssetAccessTargetKind::Source)?;
    open_target(registry, request, target, accepted_risks, opener)
}

fn require_target_kind(
    request: AgentAssetAccessRequest<'_>,
    expected: AgentAssetAccessTargetKind,
) -> Result<(), AgentAssetAccessError> {
    if request.target_kind != expected {
        return Err(AgentAssetAccessError::new(
            AgentAssetAccessErrorKind::TargetMismatch,
        ));
    }
    Ok(())
}

fn read_target(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
) -> Result<AgentAssetReadResult, AgentAssetAccessError> {
    // Cloning the immutable anchor keeps this operation alive if a new
    // generation is published after command admission.
    let anchor = registry.resolve(request)?;
    anchor.require_action(AgentAssetActionKind::Preview)?;
    let guard = reopen_verified_path(&anchor.verified).map_err(access_error)?;
    let metadata = guard.metadata().map_err(access_error)?;
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(system_time_millis)
        .map(|value| value.to_string());
    let preview = if anchor.verified.source_kind() == AgentAssetSourceKind::Directory {
        guard.revalidate().map_err(access_error)?;
        None
    } else {
        Some(match read_authorized_bytes(&anchor, &guard) {
            Ok(_) if anchor.invalid_document => {
                AgentPreviewOutput::metadata(AgentPreviewMetadataReason::InvalidDocument)
            }
            Ok(bytes) => render_preview(anchor.policy, &bytes),
            Err(error) if error.kind == AgentAssetAccessErrorKind::AccessUnavailable => {
                AgentPreviewOutput::metadata(AgentPreviewMetadataReason::ReadLimit)
            }
            Err(error) => return Err(error),
        })
    };
    let (content, truncated, diagnostics) = match preview {
        None => (
            None,
            false,
            vec![AgentAssetReadDiagnostic::DirectoryMetadataOnly],
        ),
        Some(preview) => {
            let diagnostic = match preview.metadata_reason {
                Some(AgentPreviewMetadataReason::UnsupportedSchema) if anchor.sensitive => {
                    Some(AgentAssetReadDiagnostic::SensitiveFileMetadataOnly)
                }
                Some(AgentPreviewMetadataReason::UnsupportedSchema) => {
                    Some(AgentAssetReadDiagnostic::UnsupportedSchemaMetadataOnly)
                }
                Some(AgentPreviewMetadataReason::InvalidDocument) => {
                    Some(AgentAssetReadDiagnostic::InvalidDocumentMetadataOnly)
                }
                Some(AgentPreviewMetadataReason::ReadLimit) => {
                    Some(AgentAssetReadDiagnostic::ReadLimitMetadataOnly)
                }
                None if preview.redacted => Some(AgentAssetReadDiagnostic::SensitiveValuesRedacted),
                None => None,
            };
            (
                preview.content,
                preview.truncated,
                diagnostic.into_iter().collect(),
            )
        }
    };
    // Redaction operates on owned, verified bytes; no raw fragment can escape
    // a read/revalidation failure. Directory metadata remains handle-bound too.
    guard.revalidate().map_err(access_error)?;
    Ok(AgentAssetReadResult {
        stable_id: request.target_id.to_string(),
        access_id: anchor.access_id.clone(),
        source_revision: anchor.verified.revision().clone(),
        path: guard.display_path().to_string_lossy().into_owned(),
        metadata_only: content.is_none(),
        content,
        size_bytes: metadata.len(),
        modified_at,
        truncated,
        diagnostics,
    })
}

fn open_target<F>(
    registry: &AgentAssetAccessRegistry,
    request: AgentAssetAccessRequest<'_>,
    target: AgentAssetOpenTarget,
    accepted_risks: &[AgentAssetAccessRisk],
    opener: F,
) -> Result<(), AgentAssetAccessError>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let anchor = registry.resolve(request)?;
    let action_kind = match target {
        AgentAssetOpenTarget::Asset => AgentAssetActionKind::Open,
        AgentAssetOpenTarget::Reveal => AgentAssetActionKind::Reveal,
    };
    let action = anchor.require_action(action_kind)?;
    // Exact risk-set acknowledgement is required on every click. Duplicates,
    // missing risks, or extra stale risks do not count as confirmation.
    if !action.confirmation_required {
        return Err(AgentAssetAccessError::new(
            AgentAssetAccessErrorKind::ConfirmationRequired,
        ));
    }
    acknowledge_risks(&action.risks, accepted_risks)?;
    with_verified_external_path(&anchor, |_, path| opener(path))
}

pub(crate) fn acknowledge_risks(
    risks: &[AgentAssetAccessRisk],
    accepted_risks: &[AgentAssetAccessRisk],
) -> Result<(), AgentAssetAccessError> {
    if accepted_risks.len() != risks.len()
        || accepted_risks
            .iter()
            .enumerate()
            .any(|(index, risk)| !risks.contains(risk) || accepted_risks[..index].contains(risk))
    {
        return Err(AgentAssetAccessError::new(
            AgentAssetAccessErrorKind::ConfirmationRequired,
        ));
    }
    Ok(())
}

fn with_verified_external_path<F>(
    anchor: &Arc<AgentAssetAccessAnchor>,
    opener: F,
) -> Result<(), AgentAssetAccessError>
where
    F: FnOnce(&VerifiedPathGuard, &Path) -> Result<(), String>,
{
    open_verified_source(&anchor.verified, anchor.max_read_bytes, opener)
}

pub(crate) fn open_verified_source<F>(
    anchor: &VerifiedPathAnchor,
    max_read_bytes: usize,
    opener: F,
) -> Result<(), AgentAssetAccessError>
where
    F: FnOnce(&VerifiedPathGuard, &Path) -> Result<(), String>,
{
    let guard = reopen_verified_path(anchor).map_err(access_error)?;
    if anchor.source_kind() == AgentAssetSourceKind::File && max_read_bytes != 0 {
        let bytes = guard
            .read_file_bounded(max_read_bytes)
            .map_err(access_error)?;
        if !anchor.matches_bytes(&bytes) {
            return Err(AgentAssetAccessError::new(
                AgentAssetAccessErrorKind::SourceChanged,
            ));
        }
    }
    guard.revalidate().map_err(access_error)?;
    // The external OS API still consumes a pathname asynchronously. These
    // handles stay live through its return, but do not eliminate that risk.
    // The command chooses real open/reveal APIs; it never receives a bare path.
    opener(&guard, guard.display_path())
        .map_err(|_| AgentAssetAccessError::new(AgentAssetAccessErrorKind::ExternalOpenFailed))
}

fn read_authorized_bytes(
    anchor: &AgentAssetAccessAnchor,
    guard: &VerifiedPathGuard,
) -> Result<Vec<u8>, AgentAssetAccessError> {
    let bytes = guard
        .read_file_bounded(anchor.max_read_bytes)
        .map_err(access_error)?;
    if !anchor.verified.matches_bytes(&bytes) {
        return Err(AgentAssetAccessError::new(
            AgentAssetAccessErrorKind::SourceChanged,
        ));
    }
    guard.revalidate().map_err(access_error)?;
    Ok(bytes)
}

fn access_error(error: VerifiedPathError) -> AgentAssetAccessError {
    let kind = match error {
        VerifiedPathError::OutsideAllowedRoot => AgentAssetAccessErrorKind::OutsideAllowedRoot,
        VerifiedPathError::SymlinkRejected => AgentAssetAccessErrorKind::SymlinkRejected,
        VerifiedPathError::RootChanged => AgentAssetAccessErrorKind::RootChanged,
        VerifiedPathError::SourceChanged | VerifiedPathError::TypeMismatch { .. } => {
            AgentAssetAccessErrorKind::SourceChanged
        }
        VerifiedPathError::TooLarge => AgentAssetAccessErrorKind::AccessUnavailable,
        VerifiedPathError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            AgentAssetAccessErrorKind::SourceChanged
        }
        VerifiedPathError::Io(_) => AgentAssetAccessErrorKind::ReadFailed,
    };
    AgentAssetAccessError::new(kind)
}

#[cfg(test)]
mod tests;
