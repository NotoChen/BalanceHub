pub(crate) const WINDOWS_POWERSHELL_SCRIPT_COMMAND: &str = r#"$BH_Start = New-Object System.Diagnostics.ProcessStartInfo; $BH_Start.FileName = 'cmd.exe'; $BH_Start.Arguments = '/D /E:ON /V:OFF /S /C ""%BALANCEHUB_TEMPORARY_CLI_SCRIPT%""'; $BH_Start.UseShellExecute = $false; $BH_Child = [System.Diagnostics.Process]::Start($BH_Start); $BH_Child.WaitForExit(); $BH_Child.Dispose()"#;

#[cfg(any(target_os = "windows", test))]
use super::{
    probe_terminal_command, spawn_visible_command, terminal_probe_unavailable, TerminalDefinition,
    TerminalLaunch,
};
#[cfg(any(target_os = "windows", test))]
use crate::{
    limits,
    models::{TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    platform::{
        paths::windows_shell_path,
        process::run_command_with_output_timeout,
        windows_console::{ConsoleCommand, CMD_SCRIPT_ARGUMENTS},
    },
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
        &mut windows_terminal_command(script, workdir)?,
        "无法调用 Windows Terminal",
    )
}

#[cfg(any(target_os = "windows", test))]
fn open_windows_command_prompt(script: &Path) -> Result<(), String> {
    command_prompt_command(script)?.spawn()
}

#[cfg(any(target_os = "windows", test))]
fn open_windows_powershell(script: &Path) -> Result<(), String> {
    let binary = windows_powershell_binary().ok_or_else(|| "未检测到 PowerShell".to_string())?;
    powershell_command(binary, script)?.spawn()
}

#[cfg(any(target_os = "windows", test))]
fn windows_terminal_command(script: &Path, workdir: &Path) -> Result<Command, String> {
    let script = windows_shell_path(script)?;
    let workdir = windows_shell_path(workdir)?;
    // CMD rejects an inherited UNC working directory before reading the batch.
    // Start from the launcher directory; the script maps the share with pushd.
    let startup_dir = if workdir.to_string_lossy().starts_with(r"\\") {
        script.parent().unwrap_or(&script)
    } else {
        workdir.as_ref()
    };
    let mut command = Command::new("wt");
    command
        .arg("-d")
        .arg(startup_dir)
        .args(["cmd", "/D", "/E:ON", "/V:OFF", "/K"])
        .arg(script.as_ref());
    Ok(command)
}

#[cfg(any(target_os = "windows", test))]
fn command_prompt_command(script: &Path) -> Result<ConsoleCommand, String> {
    ConsoleCommand::new("cmd.exe", CMD_SCRIPT_ARGUMENTS, script)
}

#[cfg(any(target_os = "windows", test))]
fn powershell_command(binary: &str, script: &Path) -> Result<ConsoleCommand, String> {
    let encoded =
        crate::platform::windows_shell::encoded_powershell(WINDOWS_POWERSHELL_SCRIPT_COMMAND);
    ConsoleCommand::new(
        binary,
        &format!("-NoExit -ExecutionPolicy Bypass -EncodedCommand {encoded}"),
        script,
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn all_windows_terminals_receive_ordinary_launch_paths() {
        let script = Path::new(r"\\?\C:\Temp\launch with spaces\codex.cmd");
        let workdir = Path::new(r"\\?\C:\Users\demo");
        let expected_script = OsStr::new(r"C:\Temp\launch with spaces\codex.cmd");
        let terminal = windows_terminal_command(script, workdir).unwrap();
        assert_eq!(
            terminal.get_args().nth(1),
            Some(OsStr::new(r"C:\Users\demo"))
        );
        assert_eq!(terminal.get_args().last(), Some(expected_script));
        assert_eq!(
            command_prompt_command(script).unwrap().script.as_os_str(),
            expected_script
        );
        let powershell = powershell_command("powershell", script).unwrap();
        assert_eq!(powershell.script.as_os_str(), expected_script);
    }

    #[test]
    fn network_workdir_is_entered_by_the_script_after_cmd_has_started() {
        let command = windows_terminal_command(
            Path::new(r"C:\Temp\launch\codex.cmd"),
            Path::new(r"\\?\UNC\server\share\project"),
        )
        .unwrap();
        assert_eq!(
            command.get_args().nth(1),
            Some(OsStr::new(r"C:\Temp\launch"))
        );
    }
}
