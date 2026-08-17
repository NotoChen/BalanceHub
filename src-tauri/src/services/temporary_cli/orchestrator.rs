use super::{launch, preview, probe_terminal, LaunchOptions};
use crate::{
    models::{
        AgentCliKind, AppData, AppSettings, Provider, TemporaryCliLaunchInput,
        TemporaryCliLaunchPreview, TemporaryCliLaunchResult, TemporaryCliPreference,
        TemporaryCliSessionMode,
    },
    services::{agent_cli, provider_service::ProviderService, workspaces},
    state::AppState,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub(crate) struct TemporaryCliLaunchService<'a> {
    app: &'a AppHandle,
}

struct PreparedTemporaryCliLaunch {
    data: AppData,
    provider: Provider,
    settings: AppSettings,
    input: TemporaryCliLaunchInput,
    cli_path: String,
    cli_kind: AgentCliKind,
    workdir: PathBuf,
    api_key: String,
    model: String,
    preference_model: String,
}

impl<'a> TemporaryCliLaunchService<'a> {
    pub(crate) fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    pub(crate) fn launch(
        &self,
        input: TemporaryCliLaunchInput,
    ) -> Result<TemporaryCliLaunchResult, String> {
        let prepared = self.prepare(input)?;
        let instance = launch(
            &prepared.settings,
            &prepared.provider,
            prepared.cli_kind,
            &prepared.workdir,
            launch_options(&prepared),
        )?;
        let fallback_preference = TemporaryCliPreference {
            provider_id: prepared.provider.identity.id.clone(),
            cli_kind: prepared.cli_kind,
            api_key_token_id: prepared.input.api_key_token_id.trim().to_string(),
            model: prepared.preference_model.clone(),
            workspace_path: prepared.workdir.to_string_lossy().to_string(),
        };
        let (workspaces, workspace_error, preference) = match ProviderService::new(self.app)
            .record_temporary_cli_launch(
                &prepared.provider.identity.id,
                prepared.cli_kind,
                &prepared.cli_path,
                &prepared.workdir,
                &prepared.input.api_key_token_id,
                &prepared.preference_model,
            ) {
            Ok((workspaces, preference)) => (workspaces, None, preference),
            Err(error) => (
                prepared.data.workspaces.clone(),
                Some(error),
                fallback_preference,
            ),
        };
        Ok(TemporaryCliLaunchResult {
            instance,
            workspaces,
            workspace_error,
            preference,
        })
    }

    pub(crate) fn preview(
        &self,
        input: TemporaryCliLaunchInput,
    ) -> Result<TemporaryCliLaunchPreview, String> {
        let prepared = self.prepare(input)?;
        preview(
            &prepared.settings,
            &prepared.provider,
            prepared.cli_kind,
            &prepared.workdir,
            launch_options(&prepared),
        )
    }

    fn prepare(
        &self,
        input: TemporaryCliLaunchInput,
    ) -> Result<PreparedTemporaryCliLaunch, String> {
        let data = self
            .app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let provider = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == input.provider_id)
            .cloned()
            .ok_or_else(|| "中转站不存在".to_string())?;
        let cli_kind = input.cli_kind;
        let session_mode = input.session_mode;
        let api_key = if input.api_key.trim().is_empty() {
            provider.auth.api_key.trim().to_string()
        } else {
            input.api_key.trim().to_string()
        };
        if api_key.is_empty() {
            return Err("缺少 API Key，无法启动临时 CLI".to_string());
        }
        let saved_preference = data
            .temporary_cli_preferences
            .iter()
            .find(|preference| preference.provider_id == provider.identity.id);
        let saved_model = saved_preference
            .map(|preference| preference.model.trim().to_string())
            .unwrap_or_default();
        let model = match session_mode {
            TemporaryCliSessionMode::New => [
                input.model.trim(),
                saved_preference
                    .map(|preference| preference.model.trim())
                    .unwrap_or_default(),
                provider.cli.preferred_model.trim(),
                provider.liveness.model.trim(),
                data.settings.liveness_model.trim(),
            ]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string(),
            TemporaryCliSessionMode::History => input.model.trim().to_string(),
        };
        let preference_model = match session_mode {
            TemporaryCliSessionMode::New if !model.is_empty() => model.clone(),
            _ => saved_model,
        };

        let cli = agent_cli::find(&data.settings, cli_kind, true)?;
        let terminal = probe_terminal(input.terminal_kind);
        if !terminal.available {
            let detail = terminal.message.trim();
            return Err(if detail.is_empty() {
                "所选终端当前不可用，请重新扫描终端".to_string()
            } else {
                format!("所选终端当前不可用，请重新扫描终端：{detail}")
            });
        }
        let mut settings = data.settings.clone();
        settings.temporary_cli_terminal_kind = input.terminal_kind;
        settings.set_agent_cli_path(cli_kind, cli.path.clone());
        let workdir = workspaces::normalize_directory(std::path::Path::new(&input.workdir))?;
        Ok(PreparedTemporaryCliLaunch {
            data,
            provider,
            settings,
            input,
            cli_path: cli.path,
            cli_kind,
            workdir,
            api_key,
            model,
            preference_model,
        })
    }
}

fn launch_options(prepared: &PreparedTemporaryCliLaunch) -> LaunchOptions<'_> {
    LaunchOptions {
        api_key_override: &prepared.api_key,
        model_override: &prepared.model,
        session_name_override: &prepared.input.session_name,
        resume_id: &prepared.input.resume_id,
        session_mode: prepared.input.session_mode,
    }
}
