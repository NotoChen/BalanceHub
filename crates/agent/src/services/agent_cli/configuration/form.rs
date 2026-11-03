//! Read/transform a visual draft under the same edit authority as the raw editor.
use super::{
    errors::{conflict, diagnostic, internal},
    selection::DocumentSelection,
    ConfigurationService, MAX_DOCUMENT_BYTES,
};
use crate::{models::*, services::agent_cli::settings};
use std::time::Instant;

impl ConfigurationService {
    pub(crate) fn configuration_form(
        &self,
        actor: &str,
        request: AgentConfigurationFormRequest,
        app_settings: &AppSettings,
    ) -> Result<AgentConfigurationForm, AgentConfigurationError> {
        if request.text.len() > MAX_DOCUMENT_BYTES || request.changes.len() > 64 {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::CapacityExceeded,
            ));
        }
        let edit = self
            .state
            .lock()
            .map_err(|_| internal())?
            .edits
            .get(&request.edit_id)
            .cloned()
            .ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::EditExpired)
            })?;
        if edit.actor != actor {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ActorMismatch,
            ));
        }
        if edit.expires <= Instant::now() {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::EditExpired,
            ));
        }
        if edit.public.revision != request.expected_revision {
            return Err(conflict());
        }
        let document = edit
            .documents
            .iter()
            .find(|document| document.authority.source.source_id == request.source_id)
            .ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidRequest)
            })?;
        if !matches!(document.selection, DocumentSelection::Whole)
            || !document.authority.source.visual_settings
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::UnsupportedFormat,
            ));
        }
        if !request.changes.is_empty() && document.read_only_reason.is_some() {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ReadOnly,
            ));
        }
        let _read = crate::services::agent_cli::cache::ReadScope::enter();
        // Resolve the current runtime before touching guest paths. WSL forms
        // must use the guest home and never inherit host policy or environment.
        let runtime = self.revalidate_context(&document.authority)?;
        document
            .authority
            .file
            .revalidate()
            .map_err(|_| conflict())?;
        settings::render_form(
            &runtime.home,
            &document.authority,
            app_settings.agent_cli_path(edit.public.agent_kind),
            &request.text,
            &request.changes,
        )
        .map_err(|message| {
            // Form errors contain field labels and fixed validation messages,
            // never parser excerpts or configuration values.
            let mut detail = diagnostic(
                "nativeSettingValidation",
                &message,
                AgentConfigurationDiagnosticSeverity::Error,
            );
            detail.source_id = Some(request.source_id);
            let mut error =
                AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidRequest);
            error.diagnostics.push(detail);
            error
        })
    }
}
