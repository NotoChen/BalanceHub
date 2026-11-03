use super::{
    applescript::{
        build_macos_ghostty_applescript, build_macos_iterm2_applescript,
        build_macos_terminal_applescript,
    },
    process::{run_command, run_command_text},
};
use crate::{
    models::TemporaryCliTerminalKind,
    services::{
        cli_runtime,
        temporary_cli::shell_runtime::script::{
            script_command, script_command_without_exec, set_executable, user_shell,
        },
    },
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::super::TerminalLaunch;

pub(super) fn launch_terminal(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_terminal(script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Terminal))
}

pub(super) fn launch_iterm2(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_iterm2(script).map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::ITerm2))
}

pub(super) fn launch_warp(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_warp(script, workdir)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Warp))
}

pub(super) fn launch_wezterm(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_wezterm_compatible("WezTerm", script, workdir)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WezTerm))
}

pub(super) fn launch_kaku(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_wezterm_compatible("Kaku", script, workdir)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kaku))
}

pub(super) fn launch_ghostty(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_ghostty(script)
}

pub(super) fn launch_kitty(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_shell_app("kitty", &["-e"], script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kitty))
}

pub(super) fn launch_alacritty(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_macos_shell_app("Alacritty", &["-e"], script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Alacritty))
}

fn open_macos_terminal(script: &Path) -> Result<(), String> {
    run_command(
        Command::new("osascript")
            .arg("-e")
            .arg(build_macos_terminal_applescript(script)),
        "无法调用 Terminal",
    )
}

fn open_macos_iterm2(script: &Path) -> Result<(), String> {
    run_command(
        Command::new("osascript")
            .arg("-e")
            .arg(build_macos_iterm2_applescript(script)),
        "无法调用 iTerm2",
    )
}

fn open_macos_ghostty(script: &Path) -> Result<TerminalLaunch, String> {
    let script_text = build_macos_ghostty_applescript(script);
    match run_command_text(
        Command::new("osascript").arg("-e").arg(script_text),
        "无法调用 Ghostty",
    ) {
        Ok(terminal_id) if !terminal_id.trim().is_empty() => Ok(TerminalLaunch::tracked(
            TemporaryCliTerminalKind::Ghostty,
            cli_runtime::CliTerminalLocator::Ghostty {
                terminal_id: terminal_id.trim().to_string(),
            },
        )),
        Ok(_) => Ok(TerminalLaunch::untracked(TemporaryCliTerminalKind::Ghostty)),
        Err(primary_error) => open_macos_ghostty_initial_command(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Ghostty))
            .map_err(|fallback_error| format!("{primary_error}；{fallback_error}")),
    }
}

fn open_macos_ghostty_initial_command(script: &Path) -> Result<(), String> {
    let launcher = format!("--initial-command={}", script_command_without_exec(script));
    run_command(
        Command::new("open")
            .arg("-na")
            .arg("Ghostty")
            .arg("--args")
            .arg("--quit-after-last-window-closed=true")
            .arg(launcher),
        "无法调用 Ghostty",
    )
}

fn open_macos_warp(script: &Path, workdir: &Path) -> Result<(), String> {
    let launcher = warp_launcher_script_path(script);
    let launcher_text = format!(
        "#!/bin/sh\nrm -f \"$0\"\nexec {}\n",
        script_command_without_exec(script)
    );
    fs::write(&launcher, launcher_text)
        .map_err(|err| format!("写入 Warp 临时启动脚本失败: {err}"))?;
    if let Err(err) = set_executable(&launcher) {
        let _ = fs::remove_file(&launcher);
        return Err(err);
    }

    let url = format!(
        "warp://action/new_tab?path={}",
        percent_encode(&launcher.to_string_lossy())
    );
    let _ = workdir;
    run_command(Command::new("open").arg(url), "无法调用 Warp").inspect_err(|_| {
        let _ = fs::remove_file(&launcher);
    })
}

pub(crate) fn warp_launcher_script_path(script: &Path) -> PathBuf {
    script
        .parent()
        .map(|parent| parent.join("warp-launcher"))
        .unwrap_or_else(|| env::temp_dir().join("warp-launcher"))
}

fn open_macos_wezterm_compatible(app: &str, script: &Path, workdir: &Path) -> Result<(), String> {
    let mut command = Command::new("open");
    command
        .arg("-na")
        .arg(app)
        .arg("--args")
        .arg("start")
        .arg("--cwd")
        .arg(workdir)
        .arg("--")
        .arg(user_shell())
        .arg("-c")
        .arg(script_command(script));
    run_command(&mut command, &format!("无法调用 {app}"))
}

fn open_macos_shell_app(app: &str, prefix_args: &[&str], script: &Path) -> Result<(), String> {
    let mut command = Command::new("open");
    command
        .arg("-na")
        .arg(app)
        .arg("--args")
        .args(prefix_args)
        .arg(user_shell())
        .arg("-l")
        .arg("-c")
        .arg(script_command(script));
    run_command(&mut command, &format!("无法调用 {app}"))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
