pub(crate) const WINDOWS_POWERSHELL_SCRIPT_COMMAND: &str = "& $env:BALANCEHUB_TEMPORARY_CLI_SCRIPT";

#[cfg(target_os = "windows")]
use super::{
    probe_terminal_command, spawn_visible_command, terminal_probe_unavailable, TerminalLaunch,
};
#[cfg(target_os = "windows")]
use crate::{
    limits,
    models::{AppSettings, TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    platform::process::run_command_with_output_timeout,
    services::cli_runtime,
};
#[cfg(target_os = "windows")]
use std::{path::Path, process::Command, time::Duration};

#[cfg(target_os = "windows")]
pub fn probe_available_terminals() -> Vec<TemporaryTerminalProbeResult> {
    [
        TemporaryCliTerminalKind::WindowsTerminal,
        TemporaryCliTerminalKind::CommandPrompt,
        TemporaryCliTerminalKind::PowerShell,
    ]
    .into_iter()
    .map(probe_terminal)
    .filter(|result| result.available)
    .collect()
}

#[cfg(target_os = "windows")]
pub fn probe_terminal(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    match kind {
        TemporaryCliTerminalKind::WindowsTerminal => {
            probe_terminal_command(kind, "Windows Terminal", "wt", &["--version"])
        }
        TemporaryCliTerminalKind::CommandPrompt => {
            probe_terminal_command(kind, "命令提示符", "cmd", &["/C", "ver"])
        }
        TemporaryCliTerminalKind::PowerShell => probe_windows_powershell(kind),
        _ => terminal_probe_unavailable(kind, "临时终端", "当前系统不支持该终端"),
    }
}

#[cfg(target_os = "windows")]
fn windows_powershell_binary() -> Option<&'static str> {
    ["pwsh", "powershell"].into_iter().find(|binary| {
        let mut command = Command::new(binary);
        command.args([
            "-NoProfile",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ]);
        run_command_with_output_timeout(
            &mut command,
            Duration::from_secs(3),
            limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
        )
        .map(|output| !output.timed_out && output.status.is_some_and(|status| status.success()))
        .unwrap_or(false)
    })
}

#[cfg(target_os = "windows")]
fn probe_windows_powershell(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    let Some(binary) = windows_powershell_binary() else {
        return terminal_probe_unavailable(kind, "PowerShell", "未检测到 PowerShell");
    };
    probe_terminal_command(
        kind,
        "PowerShell",
        binary,
        &[
            "-NoProfile",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ],
    )
}

#[cfg(target_os = "windows")]
pub(crate) fn activate_terminal_target(
    _target: &cli_runtime::CliTerminalLocator,
) -> Result<(), String> {
    Err("当前系统暂不支持精确定位临时 CLI 窗口".to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::WindowsTerminal => open_windows_terminal(script, workdir)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WindowsTerminal)),
        TemporaryCliTerminalKind::CommandPrompt => open_windows_command_prompt(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::CommandPrompt)),
        TemporaryCliTerminalKind::PowerShell => open_windows_powershell(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::PowerShell)),
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
}

#[cfg(target_os = "windows")]
fn open_windows_terminal(script: &Path, workdir: &Path) -> Result<(), String> {
    spawn_visible_command(
        Command::new("wt")
            .arg("-d")
            .arg(workdir)
            .arg("cmd")
            .arg("/K")
            .arg(script),
        "无法调用 Windows Terminal",
    )
}

#[cfg(target_os = "windows")]
fn open_windows_command_prompt(script: &Path) -> Result<(), String> {
    spawn_visible_command(
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .arg(script),
        "无法调用命令提示符",
    )
}

#[cfg(target_os = "windows")]
fn open_windows_powershell(script: &Path) -> Result<(), String> {
    let binary = windows_powershell_binary().ok_or_else(|| "未检测到 PowerShell".to_string())?;
    spawn_visible_command(
        Command::new(binary)
            .env("BALANCEHUB_TEMPORARY_CLI_SCRIPT", script)
            .args(["-NoExit", "-ExecutionPolicy", "Bypass", "-Command"])
            .arg(WINDOWS_POWERSHELL_SCRIPT_COMMAND),
        "无法调用 PowerShell",
    )
}
