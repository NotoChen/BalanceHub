use super::shell_runtime::script::{
    cleanup_launch_files, temporary_script_path, write_launch_script, LaunchScriptInput,
};
use super::terminal::open_script_in_terminal;
use super::{
    ensure_temporary_launch_supported, provider_account_label, resolve_resume_id,
    resolve_session_name, runtime_session_title, validate_full_api_key, validate_launch_options,
    LaunchOptions,
};
use crate::{
    models::{
        AgentCliKind, AgentSessionLaunchIdentity, AppSettings, OAuthCredential, Provider,
        ProviderAuth, ProviderProtocol, TemporaryCliInstance,
    },
    network,
    services::{
        agent_cli::{
            self,
            contracts::{EnvironmentPatch, TemporaryLaunchConfiguration, TemporaryLaunchRequest},
        },
        cli_runtime::{self, CliInstanceRegistration, RegisteredCliInstance},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(all(test, not(target_os = "windows")))]
mod tests;

pub(crate) enum CliLaunchTarget<'a> {
    Native,
    Provider(&'a Provider),
}

pub(crate) struct CliLaunchRequest<'a> {
    pub settings: &'a AppSettings,
    pub target: CliLaunchTarget<'a>,
    pub cli: &'a agent_cli::AgentCliExecutable,
    pub cli_kind: AgentCliKind,
    pub workdir: &'a Path,
    pub options: LaunchOptions<'a>,
    pub native_session: Option<&'a AgentSessionLaunchIdentity>,
    pub source_environment: Option<&'a EnvironmentPatch>,
}

/// Script preparation is reversible. The operation owns this value until its
/// final source/activity check and cancellation boundary have passed.
pub(crate) struct PreparedCliLaunch {
    settings: AppSettings,
    script: PathBuf,
    workdir: PathBuf,
    registered: RegisteredCliInstance,
    // Retained through dispatch; the persisted Starting record covers the handoff to the terminal.
    _execution_lease: crate::services::agent_cli::lifecycle::usage::ExecutionLease,
}

pub(crate) struct CliDispatchOutcome {
    pub instance: TemporaryCliInstance,
    pub uncertainty: Option<String>,
}

impl PreparedCliLaunch {
    pub(crate) fn instance(&self) -> &TemporaryCliInstance {
        &self.registered.instance
    }

    pub(crate) fn cancel(self) {
        cli_runtime::mark_instance_exited(&self.registered.status_path, None);
        cleanup_launch_files(&self.script);
    }

    pub(crate) fn dispatch(self) -> CliDispatchOutcome {
        match open_script_in_terminal(&self.settings, &self.script, &self.workdir) {
            Ok(terminal) => match cli_runtime::record_terminal_launch(
                &self.registered.instance.id,
                terminal.terminal_kind,
                terminal.locator,
            ) {
                Ok(instance) => CliDispatchOutcome {
                    instance,
                    uncertainty: None,
                },
                Err(error) => CliDispatchOutcome {
                    instance: self.registered.instance,
                    uncertainty: Some(format!("终端已接收继续命令，但启动记录未更新：{error}")),
                },
            },
            Err(error) => {
                // Automation may time out after the terminal has accepted the
                // command. Resume must retain its identity and status file.
                if self.registered.instance.native_session.is_none() {
                    cli_runtime::mark_instance_exited(&self.registered.status_path, None);
                    cleanup_launch_files(&self.script);
                }
                CliDispatchOutcome {
                    instance: self.registered.instance,
                    uncertainty: Some(error),
                }
            }
        }
    }
}

pub(crate) fn prepare_launch(request: CliLaunchRequest<'_>) -> Result<PreparedCliLaunch, String> {
    let CliLaunchRequest {
        settings,
        target,
        cli,
        cli_kind,
        workdir,
        options,
        native_session,
        source_environment,
    } = request;
    ensure_temporary_launch_supported(cli_kind)?;
    let execution_lease =
        crate::services::agent_cli::lifecycle::usage::acquire(Path::new(&cli.path))?;
    validate_launch_options(cli_kind, &options)?;
    if !workdir.is_dir() {
        return Err("工作目录不存在，无法继续会话".to_owned());
    }
    let resume_id = resolve_resume_id(options.session_mode, options.resume_id)?;
    let provider = match target {
        CliLaunchTarget::Native => None,
        CliLaunchTarget::Provider(provider) => Some(provider),
    };
    let oauth = provider
        .map(|provider| oauth_launch_credential(provider, cli_kind))
        .transpose()?
        .flatten();
    let api_key = if oauth.is_some() {
        ""
    } else {
        provider
            .map(|provider| {
                if options.api_key_override.trim().is_empty() {
                    provider.auth.api_key.trim()
                } else {
                    options.api_key_override.trim()
                }
            })
            .unwrap_or_default()
    };
    if let Some(provider) = provider {
        if oauth.is_none() {
            validate_full_api_key(api_key)?;
            if provider.identity.base_url.trim().is_empty() {
                return Err("缺少中转站地址，无法启动临时 CLI".to_owned());
            }
        }
    }
    let model = options.model_override.trim().to_string();
    let session_name = resolve_session_name(
        cli_kind,
        options.session_mode,
        options.session_name_override,
    )?;
    let base_url = provider
        .map(|provider| agent_cli::provider_base_url(cli_kind, provider))
        .unwrap_or_default();
    let proxy = provider
        .map(|provider| network::resolve_proxy(settings, provider))
        .unwrap_or_else(|| network::resolve_global_proxy(settings));
    let script = temporary_script_path(provider, cli_kind);
    let launch_directory = script.parent().ok_or("缺少启动目录")?;
    let definition = agent_cli::definition(cli_kind);
    let adapter = definition
        .temporary_launch()
        .ok_or_else(|| format!("{} 当前不支持临时启动", definition.label))?;
    let configuration = match provider {
        Some(provider) => match oauth {
            Some((auth, identity)) => TemporaryLaunchConfiguration::OAuth { auth, identity },
            None => TemporaryLaunchConfiguration::Provider {
                provider_name: &provider.identity.name,
                api_key,
                base_url: &base_url,
            },
        },
        None => TemporaryLaunchConfiguration::Native,
    };
    let mut plan = adapter.build_plan(TemporaryLaunchRequest {
        configuration,
        model: &model,
        session_name: &session_name,
        resume_id: &resume_id,
        session_mode: options.session_mode,
        launch_directory,
    })?;
    if let Some(environment) = source_environment {
        for name in environment.removed_names() {
            plan.environment.remove(name);
        }
        for (name, value) in environment.set_values() {
            plan.environment.set(name, value);
        }
    }
    let is_wsl = native_session.is_some_and(|identity| {
        matches!(
            identity.runtime_scope,
            crate::models::AgentRuntimeScope::Wsl { .. }
        )
    });
    if is_wsl
        && options
            .run_settings
            .is_some_and(|value| value != &Default::default())
    {
        return Err("WSL 历史会话当前使用原生启动配置，请恢复默认的本次设置".into());
    }
    let run = super::run_settings::apply(
        super::run_settings::RunSettingsRequest {
            app_settings: settings,
            kind: cli_kind,
            cli_path: &cli.path,
            workdir,
            mode: options.session_mode,
            model: options.model_override,
            inherited_model: options.inherited_model,
            directory: script.parent().ok_or("缺少启动目录")?,
            settings: options.run_settings,
        },
        &mut plan,
    )?;
    let wsl_path = if let Some(crate::models::AgentRuntimeScope::Wsl { distro_id }) =
        native_session.map(|identity| &identity.runtime_scope)
    {
        Some(crate::services::wsl::launch::wrap(
            distro_id,
            cli_kind,
            workdir,
            &cli.path,
            &mut plan,
            &proxy.environment(),
        )?)
    } else {
        None
    };
    let session_title = runtime_session_title(&options, &session_name);
    let account_label = provider
        .map(|provider| provider_account_label(provider, options.api_key_label))
        .unwrap_or_default();
    let registered = cli_runtime::register_instance(CliInstanceRegistration {
        provider,
        cli_kind,
        workdir,
        terminal_kind: settings.temporary_cli_terminal_kind,
        session_title: &session_title,
        account_label: &account_label,
        api_key_local_id: provider.and(options.api_key_local_id),
        native_session,
    })?;
    let prepared = PreparedCliLaunch {
        settings: settings.clone(),
        script,
        workdir: workdir.to_owned(),
        registered,
        _execution_lease: execution_lease,
    };
    let write = (|| {
        if let Some(parent) = prepared.script.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("创建临时 CLI 启动目录失败：{err}"))?;
            // The launch directory can hold a copy of an OAuth authorization.
            super::shell_runtime::script::restrict_directory_to_owner(parent)?;
        }
        run.materialize()?;
        cli_runtime::register_launch_directory(
            &prepared.registered.status_path,
            prepared.script.parent().ok_or("缺少启动目录")?,
        )?;
        if let Some(parent) = prepared.script.parent() {
            fs::write(
                parent.join("owner.json"),
                serde_json::to_vec(
                    &serde_json::json!({"statusPath": prepared.registered.status_path}),
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        write_launch_script(&LaunchScriptInput {
            script: &prepared.script,
            cli_path: wsl_path.as_deref().unwrap_or(&cli.path),
            cli_command_name: if wsl_path.is_some() {
                "wsl.exe"
            } else {
                definition.executable
            },
            workdir,
            plan: &plan,
            status_path: &prepared.registered.status_path,
            proxy_environment: &proxy.environment(),
            prefer_shell_cli: native_session.is_none()
                && options
                    .run_settings
                    .is_none_or(|value| value == &Default::default()),
        })
    })();
    if let Err(error) = write {
        prepared.cancel();
        return Err(error);
    }
    Ok(prepared)
}

/// Resolves the credential an OAuth card hands to its agent CLI, or `None` for
/// a relay card. The vendor is fixed by the card, so a Codex authorization can
/// never be offered to Gemini or written where another agent would read it.
pub(super) fn oauth_launch_credential(
    provider: &Provider,
    cli_kind: AgentCliKind,
) -> Result<Option<(&ProviderAuth, &OAuthCredential)>, String> {
    if provider.identity.protocol != ProviderProtocol::OAuth {
        return Ok(None);
    }
    agent_cli::config_support::oauth_credential(provider, cli_kind).map(Some)
}
