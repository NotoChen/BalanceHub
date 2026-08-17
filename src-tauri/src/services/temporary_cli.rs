mod environment;
mod script;
mod terminal;
#[cfg(test)]
mod tests;

use crate::{
    models::{
        AgentCliKind, AppSettings, Provider, TemporaryCliInstance, TemporaryCliLaunchPreview,
        TemporaryCliSessionMode,
    },
    network,
    services::{
        agent_cli::{self, contracts::TemporaryLaunchRequest},
        cli_runtime,
    },
};
use script::{
    cleanup_launch_files, effective_model, format_cli_command, preview_cli_auxiliary_path,
    temporary_cli_auxiliary_path, temporary_script_path, write_launch_script, LaunchScriptInput,
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
    cli_kind: AgentCliKind,
    workdir: &Path,
    options: LaunchOptions<'_>,
) -> Result<TemporaryCliLaunchPreview, String> {
    ensure_temporary_launch_supported(cli_kind)?;
    validate_launch_options(cli_kind, &options)?;
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
    let cli = agent_cli::find(settings, cli_kind, true)?;
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
    let definition = agent_cli::definition(cli_kind);
    let launch_adapter = definition
        .temporary_launch()
        .ok_or_else(|| format!("{} 当前不支持临时启动", definition.label))?;
    let base_url = agent_cli::provider_base_url(cli_kind, provider);
    let proxy_environment = network::resolve_proxy(settings, provider).environment();
    let auxiliary_file_path = preview_cli_auxiliary_path(launch_adapter.auxiliary_file_name());
    let plan = launch_adapter.build_plan(TemporaryLaunchRequest {
        provider_name: &provider.identity.name,
        api_key: "***",
        base_url: &base_url,
        model: &model,
        session_name: &session_name,
        resume_id: &resume_id,
        session_mode: options.session_mode,
        auxiliary_file_path: auxiliary_file_path.as_deref(),
    })?;
    let mut environment = environment::capture_shell_environment();
    for (name, value) in proxy_environment.variables() {
        environment::insert_environment(&mut environment, name, value);
    }
    let environment_entries = environment
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    Ok(TemporaryCliLaunchPreview {
        provider_name: provider.identity.name.clone(),
        cli_kind,
        cli_path: cli.path.clone(),
        command: format_cli_command(
            &cli.path,
            &plan.args,
            &plan.environment,
            &environment_entries,
        ),
        args: plan.args,
        terminal_kind: settings.temporary_cli_terminal_kind,
        terminal_name: terminal.name,
        workdir: workdir.to_string_lossy().to_string(),
        base_url,
        api_key: "***".to_string(),
        model,
        session_mode: options.session_mode,
        session_name,
        resume_id,
        environment,
        settings_path: auxiliary_file_path.map(|path| path.to_string_lossy().to_string()),
        settings_content: plan.auxiliary_file_content,
    })
}

pub fn launch(
    settings: &AppSettings,
    provider: &Provider,
    cli_kind: AgentCliKind,
    workdir: &Path,
    options: LaunchOptions<'_>,
) -> Result<TemporaryCliInstance, String> {
    ensure_temporary_launch_supported(cli_kind)?;
    validate_launch_options(cli_kind, &options)?;
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

    let cli = agent_cli::find(settings, cli_kind, true)?;
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
    let base_url = agent_cli::provider_base_url(cli_kind, provider);
    let proxy = network::resolve_proxy(settings, provider);
    let proxy_environment = proxy.environment();

    let script = temporary_script_path(provider, cli_kind);
    let definition = agent_cli::definition(cli_kind);
    let launch_adapter = definition
        .temporary_launch()
        .ok_or_else(|| format!("{} 当前不支持临时启动", definition.label))?;
    let auxiliary_file_path =
        temporary_cli_auxiliary_path(&script, launch_adapter.auxiliary_file_name());
    let plan = launch_adapter.build_plan(TemporaryLaunchRequest {
        provider_name: &provider.identity.name,
        api_key: &api_key,
        base_url: &base_url,
        model: &model,
        session_name: &session_name,
        resume_id: &resume_id,
        session_mode: options.session_mode,
        auxiliary_file_path: auxiliary_file_path.as_deref(),
    })?;
    let registered = cli_runtime::register_instance(
        provider,
        cli_kind,
        workdir,
        settings.temporary_cli_terminal_kind,
    )?;
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
        cli_path: &cli.path,
        cli_command_name: definition.executable,
        workdir,
        plan: &plan,
        auxiliary_file_path: auxiliary_file_path.as_deref(),
        status_path: &registered.status_path,
        proxy_environment: &proxy_environment,
    };
    if let Err(err) = write_launch_script(&launch_script) {
        cli_runtime::mark_instance_exited(&registered.status_path, None);
        cleanup_launch_files(&script, launch_adapter.auxiliary_file_name());
        return Err(err);
    }

    let terminal_launch = match open_script_in_terminal(settings, &script, workdir) {
        Ok(terminal_launch) => terminal_launch,
        Err(err) => {
            cli_runtime::mark_instance_exited(&registered.status_path, None);
            cleanup_launch_files(&script, launch_adapter.auxiliary_file_name());
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
    cli_kind: AgentCliKind,
    session_mode: TemporaryCliSessionMode,
    session_name_override: &str,
) -> Result<String, String> {
    // Session naming is capability-gated. Agent-specific argument construction remains in
    // the launch adapter, so unsupported CLIs never receive an invented startup flag.
    if !agent_cli::definition(cli_kind).capabilities().session_name
        || !matches!(session_mode, TemporaryCliSessionMode::New)
    {
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

fn ensure_temporary_launch_supported(cli_kind: AgentCliKind) -> Result<(), String> {
    let definition = agent_cli::definition(cli_kind);
    if definition.temporary_launch().is_some() {
        Ok(())
    } else {
        Err(format!("{} 当前不支持临时启动", definition.label))
    }
}

fn validate_launch_options(
    cli_kind: AgentCliKind,
    options: &LaunchOptions<'_>,
) -> Result<(), String> {
    let definition = agent_cli::definition(cli_kind);
    let capabilities = definition.capabilities();
    if !options.model_override.trim().is_empty() && !capabilities.model_selection {
        return Err(format!("{} 当前不支持启动时指定模型", definition.label));
    }
    if matches!(options.session_mode, TemporaryCliSessionMode::History)
        && (!capabilities.session_history || !capabilities.session_resume)
    {
        return Err(format!("{} 当前不支持恢复历史会话", definition.label));
    }
    Ok(())
}

pub fn activate(instance_id: &str) -> Result<(), String> {
    let target = cli_runtime::activation_target(instance_id)?;
    activate_terminal_target(&target)
}
