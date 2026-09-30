use super::{validate_request, AgentSessionResumeService};
use crate::{
    models::{AgentSessionResumeOperation, AgentSessionResumeRequest},
    services::cli_sessions::workbench::{resolve_session_ref, AgentSessionActor},
};
use std::time::Duration;
use tauri::AppHandle;

const ADMISSION_TIMEOUT: Duration = Duration::from_secs(5);

impl AgentSessionResumeService {
    /// Every submission, including retries and shared reservations, must prove
    /// this window's current query grant before it can read or cancel an
    /// existing operation. Executable probing remains in the background worker.
    pub(crate) async fn start(
        &self,
        app: &AppHandle,
        actor: &str,
        window_label: &str,
        request: AgentSessionResumeRequest,
    ) -> Result<(AgentSessionResumeOperation, bool), String> {
        let app = app.clone();
        self.start_with_admission(
            actor,
            window_label,
            request,
            ADMISSION_TIMEOUT,
            move |window_label, scope_revision, session_ref| {
                resolve_session_ref(
                    AgentSessionActor {
                        app: &app,
                        window_label,
                    },
                    scope_revision,
                    session_ref,
                )
                .map(|_| ())
            },
        )
        .await
    }

    pub(super) async fn start_with_admission(
        &self,
        actor: &str,
        window_label: &str,
        request: AgentSessionResumeRequest,
        timeout: Duration,
        admit: impl FnOnce(&str, &str, &str) -> Result<(), String> + Send + 'static,
    ) -> Result<(AgentSessionResumeOperation, bool), String> {
        validate_request(&request)?;
        let admission_window = window_label.to_owned();
        let scope_revision = request.scope_revision.clone();
        let session_ref = request.session_ref.clone();
        // The blocking closure may outlive this timeout, so it may only resolve
        // the grant. Allocation, access grants and dispatch stay after await.
        let admission = tauri::async_runtime::spawn_blocking(move || {
            admit(&admission_window, &scope_revision, &session_ref)
        });
        tokio::time::timeout(timeout, admission)
            .await
            .map_err(|_| "验证会话引用超时，请刷新会话后重试".to_owned())?
            .map_err(|_| "验证会话引用失败，请刷新会话后重试".to_owned())??;
        self.reserve(actor, window_label, request)
    }
}
