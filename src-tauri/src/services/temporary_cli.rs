mod environment;
mod script;
mod terminal;
#[cfg(test)]
mod tests;

use crate::{
    models::{
        AppSettings, LivenessCliKind, Provider, TemporaryCliInstance, TemporaryCliLaunchPreview,
        TemporaryCliSessionMode,
    },
    network,
    services::{
        cli_runtime,
        liveness::{anthropic_base_url, openai_base_url, LivenessRunner},
    },
};
use script::{
    claude_settings_content, cleanup_launch_files, cli_args, effective_model, format_cli_command,
    insert_resume_id, preview_claude_settings_path, temporary_script_path, write_launch_script,
    LaunchScriptInput,
};
use std::{fs, path::Path};
use terminal::{activate_terminal_target, open_script_in_terminal};

pub use script::cleanup_stale;
pub use terminal::{probe_available_terminals, probe_terminal};

/// 本次临时 CLI 启动的调用方覆盖项。配置解析和实例注册仍由本模块统一负责，
/// 这里只承载 IPC 层已经解析出的单次启动选择。
pub(crate) struct LaunchOptions<'a> {
    pub(crate) api_key_override: &'a str,
    pub(crate) model_override: &'a str,
    pub(crate) session_name_override: &'a str,
    pub(crate) resume_id: &'a str,
    pub(crate) session_mode: TemporaryCliSessionMode,
}

pub fn preview(
    settings: &AppSettings,
    provider: &Provider,
    cli_kind: LivenessCliKind,
    workdir: &Path,
    options: LaunchOptions<'_>,
) -> Result<TemporaryCliLaunchPreview, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在".to_string());
    }
    let api_key = if options.api_key_override.trim().is_empty() {
        provider.auth.api_key.trim().to_string()
    } else {
        options.api_key_override.trim().to_string()
    };
    if api_key.is_empty() {
        return Err("缺少 API Key，无法启动临时 CLI".to_string());
    }
    if provider.identity.base_url.trim().is_empty() {
        return Err("缺少中转站地址，无法启动临时 CLI".to_string());
    }
    let resume_id = resolve_resume_id(options.session_mode, options.resume_id)?;
    let cli = match cli_kind {
        LivenessCliKind::Codex => LivenessRunner::find_codex_cli(&settings.codex_cli_path)?,
        LivenessCliKind::ClaudeCode => LivenessRunner::find_claude_cli(&settings.claude_cli_path)?,
    };
    let terminal = probe_terminal(settings.temporary_cli_terminal_kind);
    if !terminal.available {
        let detail = terminal.message.trim();
        return Err(if detail.is_empty() {
            "所选终端当前不可用，请重新扫描终端".to_string()
        } else {
            format!("所选终端当前不可用，请重新扫描终端：{detail}")
        });
    }
    let model = resolve_launch_model(
        settings,
        provider,
        options.model_override,
        options.session_mode,
    );
    let session_name = resolve_session_name(
        cli_kind,
        options.session_mode,
        options.session_name_override,
    )?;
    let base_url = match cli_kind {
        LivenessCliKind::Codex => openai_base_url(provider),
        LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
    };
    let proxy_environment = network::resolve_proxy(settings, provider).environment();
    let settings_path = preview_claude_settings_path(cli_kind);
    let mut args = cli_args(
        cli_kind,
        &provider.identity.name,
        &base_url,
        &model,
        &session_name,
        options.session_mode,
        settings_path.as_deref(),
    );
    insert_resume_id(&mut args, cli_kind, &resume_id);
    let mut environment = environment::capture_shell_environment();
    for (name, value) in proxy_environment.variables() {
        environment::insert_environment(&mut environment, name, value);
    }
    let environment_entries = environment
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    let settings_content = matches!(cli_kind, LivenessCliKind::ClaudeCode)
        .then(|| claude_settings_content(&api_key, &base_url))
        .transpose()?;

    Ok(TemporaryCliLaunchPreview {
        provider_name: provider.identity.name.clone(),
        cli_kind,
        cli_path: cli.path.clone(),
        command: format_cli_command(cli_kind, &cli.path, &args, &api_key, &environment_entries),
        args,
        terminal_kind: settings.temporary_cli_terminal_kind,
        terminal_name: terminal.name,
        workdir: workdir.to_string_lossy().to_string(),
        base_url,
        api_key,
        model,
        session_mode: options.session_mode,
        session_name,
        resume_id,
        environment,
        settings_path: settings_path.map(|path| path.to_string_lossy().to_string()),
        settings_content,
    })
}

pub fn launch(
    settings: &AppSettings,
    provider: &Provider,
    cli_kind: LivenessCliKind,
    workdir: &Path,
    options: LaunchOptions<'_>,
) -> Result<TemporaryCliInstance, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在".to_string());
    }
    let api_key = if options.api_key_override.trim().is_empty() {
        provider.auth.api_key.trim().to_string()
    } else {
        options.api_key_override.trim().to_string()
    };
    if api_key.is_empty() {
        return Err("缺少 API Key，无法启动临时 CLI".to_string());
    }
    if provider.identity.base_url.trim().is_empty() {
        return Err("缺少中转站地址，无法启动临时 CLI".to_string());
    }
    let resume_id = resolve_resume_id(options.session_mode, options.resume_id)?;

    let cli = match cli_kind {
        LivenessCliKind::Codex => LivenessRunner::find_codex_cli(&settings.codex_cli_path)?,
        LivenessCliKind::ClaudeCode => LivenessRunner::find_claude_cli(&settings.claude_cli_path)?,
    };
    let model = resolve_launch_model(
        settings,
        provider,
        options.model_override,
        options.session_mode,
    );
    let session_name = resolve_session_name(
        cli_kind,
        options.session_mode,
        options.session_name_override,
    )?;
    let base_url = match cli_kind {
        LivenessCliKind::Codex => openai_base_url(provider),
        LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
    };
    let proxy = network::resolve_proxy(settings, provider);
    let proxy_environment = proxy.environment();

    let registered = cli_runtime::register_instance(
        provider,
        cli_kind,
        workdir,
        settings.temporary_cli_terminal_kind,
    )?;
    let script = temporary_script_path(provider, cli_kind);
    if let Some(parent) = script.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            cli_runtime::mark_instance_exited(&registered.status_path, None);
            return Err(format!(
                "创建临时 CLI 启动目录失败({}): {err}",
                parent.display()
            ));
        }
    }

    let launch_script = LaunchScriptInput {
        script: &script,
        cli_kind,
        cli_path: &cli.path,
        workdir,
        provider_name: &provider.identity.name,
        api_key: &api_key,
        base_url: &base_url,
        model: &model,
        session_name: &session_name,
        resume_id: &resume_id,
        session_mode: options.session_mode,
        status_path: &registered.status_path,
        proxy_environment: &proxy_environment,
    };
    if let Err(err) = write_launch_script(&launch_script) {
        cli_runtime::mark_instance_exited(&registered.status_path, None);
        cleanup_launch_files(&script, cli_kind);
        return Err(err);
    }

    let terminal_launch = match open_script_in_terminal(settings, &script, workdir) {
        Ok(terminal_launch) => terminal_launch,
        Err(err) => {
            cli_runtime::mark_instance_exited(&registered.status_path, None);
            cleanup_launch_files(&script, cli_kind);
            return Err(err);
        }
    };

    Ok(cli_runtime::record_terminal_launch(
        &registered.instance.id,
        terminal_launch.terminal_kind,
        terminal_launch.locator,
    )
    .unwrap_or(registered.instance))
}

fn resolve_resume_id(
    session_mode: TemporaryCliSessionMode,
    resume_id: &str,
) -> Result<String, String> {
    if matches!(session_mode, TemporaryCliSessionMode::New) {
        return Ok(String::new());
    }
    let id = resume_id.trim();
    if id.is_empty() {
        return Err("请选择一个历史会话后再启动".to_string());
    }
    if id.chars().any(char::is_control) {
        return Err("历史会话 ID 不能包含换行或控制字符".to_string());
    }
    Ok(id.to_string())
}

fn resolve_launch_model(
    settings: &AppSettings,
    provider: &Provider,
    model_override: &str,
    session_mode: TemporaryCliSessionMode,
) -> String {
    if !model_override.trim().is_empty() {
        return model_override.trim().to_string();
    }
    // 恢复已有会话时，CLI 自己会从会话元数据恢复模型；注入测活默认模型
    // 可能改变原会话的行为。新会话仍沿用现有的全局/中转站回退规则。
    if !matches!(session_mode, TemporaryCliSessionMode::New) {
        String::new()
    } else {
        effective_model(settings, provider)
    }
}

fn resolve_session_name(
    cli_kind: LivenessCliKind,
    session_mode: TemporaryCliSessionMode,
    session_name_override: &str,
) -> Result<String, String> {
    // Codex only exposes /new and /rename inside its TUI; never turn this into
    // an undocumented startup argument.
    if !cli_kind.supports_session_name() || !matches!(session_mode, TemporaryCliSessionMode::New) {
        return Ok(String::new());
    }

    let name = session_name_override.trim();
    if name.is_empty() {
        return Ok(String::new());
    }
    if name.chars().any(char::is_control) {
        return Err("会话名称不能包含换行或控制字符".to_string());
    }
    Ok(name.to_string())
}

pub fn activate(instance_id: &str) -> Result<(), String> {
    let target = cli_runtime::activation_target(instance_id)?;
    activate_terminal_target(&target)
}
