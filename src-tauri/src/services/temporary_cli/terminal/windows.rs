pub(crate) const WINDOWS_POWERSHELL_SCRIPT_COMMAND: &str = "& $env:BALANCEHUB_TEMPORARY_CLI_SCRIPT";

#[cfg(any(target_os = "windows", test))]
use super::{
    probe_terminal_command, spawn_visible_command, terminal_probe_unavailable, TerminalDefinition,
    TerminalLaunch,
};
#[cfg(any(target_os = "windows", test))]
use crate::{
    limits,
    models::{TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    platform::process::run_command_with_output_timeout,
};
#[cfg(any(target_os = "windows", test))]
use std::{path::Path, process::Command, time::Duration};

#[cfg(any(target_os = "windows", test))]
const DEFINITIONS: &[TerminalDefinition] = &[
    TerminalDefinition::new(
        TemporaryCliTerminalKind::WindowsTerminal,
        probe_windows_terminal,
        launch_windows_terminal,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::CommandPrompt,
        probe_command_prompt,
        launch_command_prompt,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::PowerShell,
        probe_powershell,
        launch_powershell,
        None,
    ),
];

#[cfg(any(target_os = "windows", test))]
pub(super) const fn definitions() -> &'static [TerminalDefinition] {
    DEFINITIONS
}

#[cfg(any(target_os = "windows", test))]
fn probe_windows_terminal() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::WindowsTerminal,
        "Windows Terminal",
        "wt",
        &["--version"],
    )
}

#[cfg(any(target_os = "windows", test))]
fn probe_command_prompt() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::CommandPrompt,
        "命令提示符",
        "cmd",
        &["/C", "ver"],
    )
}

#[cfg(any(target_os = "windows", test))]
fn probe_powershell() -> TemporaryTerminalProbeResult {
    probe_windows_powershell(TemporaryCliTerminalKind::PowerShell)
}

#[cfg(any(target_os = "windows", test))]
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

#[cfg(any(target_os = "windows", test))]
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

#[cfg(any(target_os = "windows", test))]
fn launch_windows_terminal(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_windows_terminal(script, workdir)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WindowsTerminal))
}

#[cfg(any(target_os = "windows", test))]
fn launch_command_prompt(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_windows_command_prompt(script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::CommandPrompt))
}

#[cfg(any(target_os = "windows", test))]
fn launch_powershell(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_windows_powershell(script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::PowerShell))
}

#[cfg(any(target_os = "windows", test))]
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

#[cfg(any(target_os = "windows", test))]
fn open_windows_command_prompt(script: &Path) -> Result<(), String> {
    spawn_visible_command(
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .arg(script),
        "无法调用命令提示符",
    )
}

#[cfg(any(target_os = "windows", test))]
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
