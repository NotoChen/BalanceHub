#[cfg(not(target_os = "windows"))]
mod unix;
#[cfg(any(target_os = "windows", test))]
mod windows;

#[cfg(test)]
pub(in crate::services::temporary_cli) use unix::{
    login_shell_bootstrap, shell_quote, shell_supports_posix_source, unix_cli_invocation,
};
#[cfg(target_os = "macos")]
pub(in crate::services::temporary_cli) use unix::{script_command, script_command_without_exec};
#[cfg(not(target_os = "windows"))]
pub(in crate::services::temporary_cli) use unix::{
    set_executable, user_shell, write_launch_script,
};
#[cfg(target_os = "windows")]
pub(in crate::services::temporary_cli) use windows::write_launch_script;
#[cfg(any(target_os = "windows", test))]
pub(in crate::services::temporary_cli) use windows::{
    escape_cmd_value, windows_launch_payload, WindowsLaunchPayloadInput,
    WINDOWS_LAUNCH_PAYLOAD_COMMAND,
};

use crate::{
    models::{AgentCliKind, AppSettings, Provider},
    network::ProxyEnvironment,
    services::agent_cli::{
        self,
        contracts::{EnvironmentPatch, TemporaryLaunchPlan},
    },
    util::unix_millis as now_millis,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(in crate::services::temporary_cli) fn cleanup_launch_files(
    script: &Path,
    auxiliary_file_name: Option<&str>,
) {
    let _ = fs::remove_file(script);
    let _ = fs::remove_file(temporary_windows_launch_payload_path(script));
    if let Some(settings_path) = temporary_cli_auxiliary_path(script, auxiliary_file_name) {
        let _ = fs::remove_file(settings_path);
    }
    if let Some(parent) = script.parent() {
        let _ = fs::remove_dir(parent);
    }
}

pub(in crate::services::temporary_cli) fn effective_model(
    settings: &AppSettings,
    provider: &Provider,
) -> String {
    let provider_model = provider.liveness.model.trim();
    if provider_model.is_empty() {
        settings.liveness_model.trim().to_string()
    } else {
        provider_model.to_string()
    }
}

pub(in crate::services::temporary_cli) fn temporary_script_path(
    provider: &Provider,
    cli_kind: AgentCliKind,
) -> PathBuf {
    let kind = agent_cli::definition(cli_kind).executable;
    env::temp_dir()
        .join(format!(
            "balancehub-temporary-cli-{}-{}-{}",
            sanitize_path_part(&provider.identity.id),
            std::process::id(),
            now_millis()
        ))
        .join(temporary_script_file_name(kind))
}

fn temporary_script_file_name(kind: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{kind}.cmd")
    } else if cfg!(target_os = "macos") {
        format!("{kind}.command")
    } else {
        format!("{kind}.sh")
    }
}

pub(in crate::services::temporary_cli) fn temporary_cli_auxiliary_path(
    script: &Path,
    auxiliary_file_name: Option<&str>,
) -> Option<PathBuf> {
    let file_name = auxiliary_file_name?;
    Some(
        script
            .parent()
            .map(|parent| parent.join(file_name))
            .unwrap_or_else(|| env::temp_dir().join(file_name)),
    )
}

pub(in crate::services::temporary_cli) fn preview_cli_auxiliary_path(
    auxiliary_file_name: Option<&str>,
) -> Option<PathBuf> {
    auxiliary_file_name.map(|name| PathBuf::from(format!("<temporary-{name}>")))
}

pub(super) fn temporary_windows_launch_payload_path(script: &Path) -> PathBuf {
    script
        .parent()
        .map(|parent| parent.join("launch.json"))
        .unwrap_or_else(|| env::temp_dir().join("launch.json"))
}

fn sanitize_path_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(in crate::services::temporary_cli) struct LaunchScriptInput<'a> {
    pub(in crate::services::temporary_cli) script: &'a Path,
    pub(in crate::services::temporary_cli) cli_path: &'a str,
    pub(in crate::services::temporary_cli) cli_command_name: &'a str,
    pub(in crate::services::temporary_cli) workdir: &'a Path,
    pub(in crate::services::temporary_cli) plan: &'a TemporaryLaunchPlan,
    pub(in crate::services::temporary_cli) auxiliary_file_path: Option<&'a Path>,
    pub(in crate::services::temporary_cli) status_path: &'a Path,
    pub(in crate::services::temporary_cli) proxy_environment: &'a ProxyEnvironment,
}

pub(super) fn write_auxiliary_file(
    path: Option<&Path>,
    content: Option<&str>,
) -> Result<(), String> {
    match (path, content) {
        (None, None) => Ok(()),
        (Some(path), Some(content)) => {
            fs::write(path, content).map_err(|err| format!("写入临时 CLI 配置失败: {err}"))?;
            restrict_to_owner(path)
        }
        _ => Err("Agent CLI 临时配置计划不完整".to_string()),
    }
}

#[cfg(not(target_os = "windows"))]
fn restrict_to_owner(path: &Path) -> Result<(), String> {
    unix::restrict_to_owner(path)
}

#[cfg(target_os = "windows")]
fn restrict_to_owner(path: &Path) -> Result<(), String> {
    windows::restrict_to_owner(path)
}

/// 清理历史残留的临时文件：启动脚本目录（终端从未执行脚本时不会自清）、
/// 测活的隔离 HOME 与输出文件（超时/崩溃路径可能泄漏）。这些目录里可能包含
/// 明文凭据，启动时兜底清扫一次；只清超过 24 小时的，避免碰到正在使用的会话。
pub fn cleanup_stale() {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    const PREFIXES: [&str; 2] = ["balancehub-temporary-cli-", "balancehub-agent-liveness-"];

    let Ok(entries) = fs::read_dir(env::temp_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_AFTER);
        if !stale {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 生成确认弹窗中的 CLI 调用。临时 shell/cmd 包装脚本仍由启动链路负责，
/// 这里只展示用户真正关心的可执行文件、参数和显式环境变量。
pub(in crate::services::temporary_cli) fn format_cli_command(
    cli_path: &str,
    args: &[String],
    agent_environment: &EnvironmentPatch,
    environment: &[(String, String)],
) -> String {
    #[cfg(not(target_os = "windows"))]
    {
        let mut parts = Vec::new();
        parts.extend(
            agent_environment
                .set_values()
                .map(|(name, value)| format!("{name}={}", unix::preview_quote(value))),
        );
        parts.extend(
            environment
                .iter()
                .map(|(name, value)| format!("{name}={}", unix::preview_quote(value))),
        );
        parts.push(unix::preview_quote(cli_path));
        parts.extend(args.iter().map(|arg| unix::preview_quote(arg)));
        parts.join(" ")
    }

    #[cfg(target_os = "windows")]
    {
        let mut assignments = Vec::new();
        assignments.extend(
            agent_environment
                .set_values()
                .map(|(name, value)| format!("$env:{name}={}", windows::preview_quote(value))),
        );
        assignments.extend(
            environment
                .iter()
                .map(|(name, value)| format!("$env:{name}={}", windows::preview_quote(value))),
        );
        let command = std::iter::once(windows::preview_quote(cli_path))
            .chain(args.iter().map(|arg| windows::preview_quote(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        if assignments.is_empty() {
            format!("& {command}")
        } else {
            format!("{}; & {command}", assignments.join("; "))
        }
    }
}
