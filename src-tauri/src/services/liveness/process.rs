use super::cli::{runtime_path_for, runtime_path_for_without_shell};
use crate::{
    limits,
    platform::process::{
        configure_process_group as configure_shared_process_group, run_command_with_output_timeout,
        wait_with_output_timeout as wait_with_shared_output_timeout, CommandOutput,
    },
};
use std::{path::Path, process::Command, time::Duration};

pub(super) fn configure_process_group(command: &mut Command) {
    configure_shared_process_group(command);
}

pub(super) fn wait_with_output_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> CommandOutput {
    wait_with_shared_output_timeout(child, timeout, limits::MAX_COMMAND_OUTPUT_BYTES)
}

pub(super) fn cli_version(
    path: &Path,
    require_substring: Option<&str>,
    include_shell: bool,
) -> Result<String, String> {
    let mut command = Command::new(path);
    let runtime_path = if include_shell {
        runtime_path_for(path)
    } else {
        runtime_path_for_without_shell(path)
    };
    if let Some(path_env) = runtime_path {
        command.env("PATH", path_env);
    }
    command.arg("--version");
    let outcome = run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(3),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map_err(|err| err.to_string())?;
    if outcome.timed_out {
        return Err("CLI 版本探测超时".to_string());
    }
    if !outcome.status.is_some_and(|status| status.success()) {
        let detail = outcome
            .stderr
            .lines()
            .chain(outcome.stdout.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(180).collect::<String>());
        return Err(detail
            .map(|detail| format!("CLI 不可用：{detail}"))
            .unwrap_or_else(|| "CLI 不可用".to_string()));
    }
    let version = outcome
        .stdout
        .lines()
        .chain(outcome.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if version.is_empty() {
        return Err("CLI 未返回版本信息".to_string());
    }
    if let Some(substring) = require_substring {
        if !version.to_ascii_lowercase().contains(substring) {
            return Err("CLI 版本信息不匹配".to_string());
        }
    }
    Ok(version)
}
