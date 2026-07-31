#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod windows;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
use crate::services::liveness::LivenessRunner;
#[cfg(not(target_os = "macos"))]
use crate::{limits, platform::process::run_command_with_output_timeout};
use crate::{
    models::{TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    services::cli_runtime,
};
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
use std::path::Path;
#[cfg(not(target_os = "macos"))]
use std::process::Command;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

#[cfg(all(target_os = "macos", test))]
pub(super) use macos::{
    build_macos_ghostty_activation_applescript, build_macos_ghostty_applescript,
    build_macos_iterm2_applescript, build_macos_terminal_applescript, warp_launcher_script_path,
};
pub(super) use platform::{activate_terminal_target, open_script_in_terminal};
pub use platform::{probe_available_terminals, probe_terminal};
#[cfg(test)]
pub(super) use windows::WINDOWS_POWERSHELL_SCRIPT_COMMAND;

pub(super) struct TerminalLaunch {
    pub(super) terminal_kind: TemporaryCliTerminalKind,
    pub(super) locator: Option<cli_runtime::CliTerminalLocator>,
}

impl TerminalLaunch {
    pub(super) fn untracked(terminal_kind: TemporaryCliTerminalKind) -> Self {
        Self {
            terminal_kind,
            locator: None,
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn tracked(
        terminal_kind: TemporaryCliTerminalKind,
        locator: cli_runtime::CliTerminalLocator,
    ) -> Self {
        Self {
            terminal_kind,
            locator: Some(locator),
        }
    }
}

pub(super) fn terminal_probe_available(
    kind: TemporaryCliTerminalKind,
    name: impl Into<String>,
    version: impl Into<String>,
) -> TemporaryTerminalProbeResult {
    TemporaryTerminalProbeResult {
        available: true,
        kind,
        name: name.into(),
        version: version.into(),
        message: String::new(),
    }
}

pub(super) fn terminal_probe_unavailable(
    kind: TemporaryCliTerminalKind,
    name: impl Into<String>,
    message: impl Into<String>,
) -> TemporaryTerminalProbeResult {
    TemporaryTerminalProbeResult {
        available: false,
        kind,
        name: name.into(),
        version: String::new(),
        message: message.into(),
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn probe_terminal_command(
    kind: TemporaryCliTerminalKind,
    name: &str,
    binary: &str,
    args: &[&str],
) -> TemporaryTerminalProbeResult {
    let mut command = Command::new(binary);
    command.args(args);
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    if let Some(path) = LivenessRunner::runtime_path_for_cli(Path::new(binary)) {
        command.env("PATH", path);
    }
    let Ok(output) = run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(3),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    ) else {
        return terminal_probe_unavailable(kind, name, format!("未检测到 {name}"));
    };
    let stdout = output.stdout;
    let stderr = output.stderr;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        let detail = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("命令执行失败");
        return terminal_probe_unavailable(kind, name, format!("{name}: {detail}"));
    }
    let version = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    terminal_probe_available(kind, name, version)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn spawn_visible_command(command: &mut Command, context: &str) -> Result<(), String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("{context}: {err}"))
}
