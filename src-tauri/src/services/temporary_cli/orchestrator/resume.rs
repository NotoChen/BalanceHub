use super::{launch_options, TemporaryCliLaunchService};
use crate::{
    models::{
        AgentCliKind, AgentRuntimeScope, AgentSessionLaunchIdentity, AgentSessionResumeIntent,
        AgentSessionResumeRequest, AgentSessionResumeResult, TemporaryCliInstance,
        TemporaryCliLaunchInput, TemporaryCliPreference, TemporaryCliSessionMode, Workspace,
    },
    services::{
        agent_cli,
        cli_sessions::workbench::ResolvedSessionTarget,
        provider_service::ProviderService,
        temporary_cli::{
            prepare_launch,
            prepared::{CliDispatchOutcome, PreparedCliLaunch},
            probe_terminal, CliLaunchRequest, CliLaunchTarget, LaunchOptions,
        },
        workspaces,
    },
    state::AppState,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub(crate) struct PreparedSessionResume {
    launch: PreparedCliLaunch,
    completion: ResumeCompletion,
}

impl PreparedSessionResume {
    pub(crate) fn instance(&self) -> &TemporaryCliInstance {
        self.launch.instance()
    }
    pub(crate) fn cancel(self) {
        self.launch.cancel();
    }
    pub(crate) fn dispatch(self) -> (CliDispatchOutcome, ResumeCompletion) {
        (self.launch.dispatch(), self.completion)
    }
}

/// Safe local references retained while an uncertain terminal launch is being
/// reconciled. API keys and native configuration content are never retained.
#[derive(Clone)]
pub(crate) struct ResumeCompletion {
    cli_kind: AgentCliKind,
    cli_path: String,
    workdir: PathBuf,
    fallback_workspaces: Vec<Workspace>,
    preference: Option<TemporaryCliPreference>,
}

impl ResumeCompletion {
    pub(crate) fn new(
        cli_kind: AgentCliKind,
        cli_path: String,
        workdir: PathBuf,
        fallback_workspaces: Vec<Workspace>,
        preference: Option<TemporaryCliPreference>,
    ) -> Self {
        Self {
            cli_kind,
            cli_path,
            workdir,
            fallback_workspaces,
            preference,
        }
    }

    pub(crate) fn pending(&self, instance: TemporaryCliInstance) -> AgentSessionResumeResult {
        AgentSessionResumeResult {
            instance,
            workspaces: self.fallback_workspaces.clone(),
            workspace_error: None,
            preference: None,
            reused: false,
        }
    }

    pub(crate) fn record(
        &self,
        app: &AppHandle,
        instance: TemporaryCliInstance,
    ) -> AgentSessionResumeResult {
        let service = ProviderService::new(app);
        let recorded = match &self.preference {
            Some(preference) => service
                .record_temporary_cli_launch(
                    &preference.provider_id,
                    self.cli_kind,
                    &self.cli_path,
                    &self.workdir,
                    &preference.api_key_local_id,
                    &preference.model,
                )
                .map(|(workspaces, preference)| (workspaces, Some(preference))),
            None => service
                .record_native_cli_launch(self.cli_kind, &self.cli_path, &self.workdir)
                .map(|workspaces| (workspaces, None)),
        };
        let (workspaces, preference, workspace_error) = match recorded {
            Ok((workspaces, preference)) => (workspaces, preference, None),
            Err(error) => (
                self.fallback_workspaces.clone(),
                self.preference.clone(),
                Some(error),
            ),
        };
        AgentSessionResumeResult {
            instance,
            workspaces,
            workspace_error,
            preference,
            reused: false,
        }
    }
}

pub(crate) fn launch_identity(target: &ResolvedSessionTarget) -> AgentSessionLaunchIdentity {
    AgentSessionLaunchIdentity {
        session_ref: target.session_ref.clone(),
        source_identity: target.source_identity.clone(),
        native_session_id: target.native_session_id.clone(),
        runtime_scope: AgentRuntimeScope::Native,
    }
}

impl TemporaryCliLaunchService<'_> {
    pub(crate) fn prepare_resume(
        &self,
        request: &AgentSessionResumeRequest,
        target: &ResolvedSessionTarget,
        identity: &AgentSessionLaunchIdentity,
    ) -> Result<PreparedSessionResume, String> {
        if !target.can_resume {
            return Err(target
                .resume_reason
                .clone()
                .unwrap_or_else(|| "该原生会话不支持独立继续".to_owned()));
        }
        let terminal = probe_terminal(request.terminal_kind);
        if !terminal.available {
            return Err(format!("所选终端当前不可用：{}", terminal.message));
        }
        match &request.intent {
            AgentSessionResumeIntent::Provider {
                provider_id,
                api_key_local_id,
            } => {
                let prepared = self.prepare(TemporaryCliLaunchInput {
                    provider_id: provider_id.clone(),
                    cli_kind: target.cli_kind,
                    cli_path: request.cli_path.clone(),
                    workdir: target.workdir.to_string_lossy().into_owned(),
                    api_key: String::new(),
                    api_key_local_id: api_key_local_id.clone().unwrap_or_default(),
                    model: String::new(),
                    session_mode: TemporaryCliSessionMode::History,
                    session_name: String::new(),
                    resume_id: target.native_session_id.clone(),
                    session_title: String::new(),
                    terminal_kind: request.terminal_kind,
                })?;
                let launch = prepare_launch(CliLaunchRequest {
                    settings: &prepared.settings,
                    target: CliLaunchTarget::Provider(&prepared.provider),
                    cli: &prepared.cli,
                    cli_kind: target.cli_kind,
                    workdir: &prepared.workdir,
                    options: launch_options(&prepared),
                    native_session: Some(identity),
                    source_environment: Some(&target.launch_environment),
                })?;
                Ok(PreparedSessionResume {
                    launch,
                    completion: ResumeCompletion::new(
                        target.cli_kind,
                        prepared.cli.path,
                        prepared.workdir.clone(),
                        prepared.data.workspaces,
                        Some(TemporaryCliPreference {
                            provider_id: prepared.provider.identity.id,
                            cli_kind: target.cli_kind,
                            api_key_local_id: prepared.input.api_key_local_id,
                            model: prepared.preference_model,
                            workspace_path: prepared.workdir.to_string_lossy().into_owned(),
                        }),
                    ),
                })
            }
            AgentSessionResumeIntent::Native {} => {
                let data = self
                    .app
                    .state::<AppState>()
                    .data
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                let cli = agent_cli::find_at_path(target.cli_kind, request.cli_path.trim())?;
                let workdir = workspaces::normalize_directory(&target.workdir)?;
                let mut settings = data.settings;
                settings.temporary_cli_terminal_kind = request.terminal_kind;
                settings.set_agent_cli_path(target.cli_kind, cli.path.clone());
                let launch = prepare_launch(CliLaunchRequest {
                    settings: &settings,
                    target: CliLaunchTarget::Native,
                    cli: &cli,
                    cli_kind: target.cli_kind,
                    workdir: &workdir,
                    options: LaunchOptions {
                        api_key_override: "",
                        model_override: "",
                        session_name_override: "",
                        session_title: "",
                        resume_id: &target.native_session_id,
                        session_mode: TemporaryCliSessionMode::History,
                        api_key_label: "",
                        api_key_local_id: None,
                    },
                    native_session: Some(identity),
                    source_environment: Some(&target.launch_environment),
                })?;
                Ok(PreparedSessionResume {
                    launch,
                    completion: ResumeCompletion::new(
                        target.cli_kind,
                        cli.path,
                        workdir,
                        data.workspaces,
                        None,
                    ),
                })
            }
        }
    }
}
