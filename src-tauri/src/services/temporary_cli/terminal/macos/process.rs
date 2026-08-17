use crate::{
    limits,
    platform::process::{run_command_with_output_timeout, CommandOutput},
};
use std::{process::Command, time::Duration};

const TERMINAL_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn run_command(command: &mut Command, context: &str) -> Result<(), String> {
    let output = run_command_with_output_timeout(
        command,
        TERMINAL_LAUNCH_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map_err(|err| format!("{context}: {err}"))?;

    if output.timed_out {
        Err(format!("{context}: 命令执行超时"))
    } else if output.status.is_some_and(|status| status.success()) {
        Ok(())
    } else {
        Err(format!("{context}: {}", command_error_message(&output)))
    }
}

pub(super) fn run_command_text(command: &mut Command, context: &str) -> Result<String, String> {
    let output = run_command_with_output_timeout(
        command,
        TERMINAL_LAUNCH_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .map_err(|err| format!("{context}: {err}"))?;

    if output.timed_out {
        Err(format!("{context}: 命令执行超时"))
    } else if output.status.is_some_and(|status| status.success()) {
        Ok(output.stdout.trim().to_string())
    } else {
        Err(format!("{context}: {}", command_error_message(&output)))
    }
}

fn command_error_message(output: &CommandOutput) -> String {
    let stderr = output.stderr.trim().to_string();
    if stderr.is_empty() {
        "系统终端没有成功启动临时 CLI".to_string()
    } else {
        stderr
    }
}
