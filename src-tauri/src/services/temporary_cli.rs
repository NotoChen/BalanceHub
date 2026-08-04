mod script;
mod terminal;
#[cfg(test)]
mod tests;

use crate::{
    models::{AppSettings, LivenessCliKind, Provider, TemporaryCliInstance},
    network,
    services::{
        cli_runtime,
        liveness::{anthropic_base_url, openai_base_url, LivenessRunner},
    },
};
use script::{
    cleanup_launch_files, effective_model, temporary_script_path, write_launch_script,
    LaunchScriptInput,
};
use std::{fs, path::Path};
use terminal::{activate_terminal_target, open_script_in_terminal};

pub use script::cleanup_stale;
pub use terminal::{probe_available_terminals, probe_terminal};

pub fn launch(
    settings: &AppSettings,
    provider: &Provider,
    cli_kind: LivenessCliKind,
    workdir: &Path,
    api_key_override: &str,
    model_override: &str,
    resume_id: Option<&str>,
) -> Result<TemporaryCliInstance, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在".to_string());
    }
    let api_key = if api_key_override.trim().is_empty() {
        provider.auth.api_key.trim().to_string()
    } else {
        api_key_override.trim().to_string()
    };
    if api_key.is_empty() {
        return Err("缺少 API Key，无法启动临时 CLI".to_string());
    }
    if provider.identity.base_url.trim().is_empty() {
        return Err("缺少中转站地址，无法启动临时 CLI".to_string());
    }

    let cli = match cli_kind {
        LivenessCliKind::Codex => LivenessRunner::find_codex_cli(&settings.codex_cli_path)?,
        LivenessCliKind::ClaudeCode => LivenessRunner::find_claude_cli(&settings.claude_cli_path)?,
    };
    let model = resolve_launch_model(settings, provider, model_override, resume_id);
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
        resume_id,
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

fn resolve_launch_model(
    settings: &AppSettings,
    provider: &Provider,
    model_override: &str,
    resume_id: Option<&str>,
) -> String {
    if !model_override.trim().is_empty() {
        return model_override.trim().to_string();
    }
    // 恢复已有会话时，CLI 自己会从会话元数据恢复模型；注入测活默认模型
    // 可能改变原会话的行为。新会话仍沿用现有的全局/中转站回退规则。
    if resume_id.is_some() {
        String::new()
    } else {
        effective_model(settings, provider)
    }
}

pub fn activate(instance_id: &str) -> Result<(), String> {
    let target = cli_runtime::activation_target(instance_id)?;
    activate_terminal_target(&target)
}
