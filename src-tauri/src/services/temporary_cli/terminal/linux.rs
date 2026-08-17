use super::{
    probe_terminal_command, spawn_visible_command, terminal_probe_available,
    terminal_probe_unavailable, TerminalLaunch,
};
use crate::{
    models::{AppSettings, TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    services::{agent_cli, cli_runtime},
};
use std::{env, ffi::OsString, path::Path, process::Command};

pub fn probe_available_terminals() -> Vec<TemporaryTerminalProbeResult> {
    [
        TemporaryCliTerminalKind::Terminal,
        TemporaryCliTerminalKind::Warp,
        TemporaryCliTerminalKind::WezTerm,
        TemporaryCliTerminalKind::Ghostty,
        TemporaryCliTerminalKind::Kitty,
        TemporaryCliTerminalKind::Alacritty,
    ]
    .into_iter()
    .map(probe_terminal)
    .filter(|result| result.available)
    .collect()
}

pub fn probe_terminal(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    if matches!(kind, TemporaryCliTerminalKind::Terminal) {
        return probe_linux_default_terminal();
    }
    let requested = match kind {
        TemporaryCliTerminalKind::Warp => (kind, "Warp", "warp-terminal"),
        TemporaryCliTerminalKind::WezTerm => (kind, "WezTerm", "wezterm"),
        TemporaryCliTerminalKind::Ghostty => (kind, "Ghostty", "ghostty"),
        TemporaryCliTerminalKind::Kitty => (kind, "Kitty", "kitty"),
        TemporaryCliTerminalKind::Alacritty => (kind, "Alacritty", "alacritty"),
        _ => return terminal_probe_unavailable(kind, "临时终端", "当前系统不支持该终端"),
    };

    probe_terminal_command(requested.0, requested.1, requested.2, &["--version"])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinuxTerminalInvocation {
    Direct,
    Execute,
}

#[derive(Clone)]
struct LinuxTerminalCandidate {
    binary: OsString,
    invocation: LinuxTerminalInvocation,
}

fn linux_default_terminal_candidates() -> Vec<LinuxTerminalCandidate> {
    let mut candidates = vec![LinuxTerminalCandidate {
        binary: "xdg-terminal-exec".into(),
        invocation: LinuxTerminalInvocation::Direct,
    }];
    if let Some(terminal) = env::var_os("TERMINAL").filter(|value| !value.is_empty()) {
        candidates.push(LinuxTerminalCandidate {
            binary: terminal,
            invocation: LinuxTerminalInvocation::Execute,
        });
    }
    candidates.extend(
        [
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "xterm",
        ]
        .into_iter()
        .map(|binary| LinuxTerminalCandidate {
            binary: binary.into(),
            invocation: LinuxTerminalInvocation::Execute,
        }),
    );

    let mut unique = Vec::new();
    candidates
        .into_iter()
        .filter(|candidate| {
            if unique
                .iter()
                .any(|known: &OsString| known == &candidate.binary)
            {
                false
            } else {
                unique.push(candidate.binary.clone());
                true
            }
        })
        .collect()
}

fn linux_command_available(binary: &std::ffi::OsStr) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let executable = |path: &Path| {
        path.metadata()
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    };
    let binary_path = Path::new(binary);
    if binary_path.components().count() > 1 {
        return executable(binary_path);
    }
    agent_cli::runtime_path_for(binary_path)
        .map(|path| env::split_paths(&path).any(|dir| executable(&dir.join(binary))))
        .unwrap_or(false)
}

fn linux_terminal_command(candidate: &LinuxTerminalCandidate, script: &Path) -> Command {
    let mut command = Command::new(&candidate.binary);
    if matches!(candidate.invocation, LinuxTerminalInvocation::Execute) {
        command.arg("-e");
    }
    command.arg(script);
    if let Some(path) = agent_cli::runtime_path_for(Path::new(&candidate.binary)) {
        command.env("PATH", path);
    }
    command
}

fn probe_linux_default_terminal() -> TemporaryTerminalProbeResult {
    let Some(candidate) = linux_default_terminal_candidates()
        .into_iter()
        .find(|candidate| linux_command_available(&candidate.binary))
    else {
        return terminal_probe_unavailable(
            TemporaryCliTerminalKind::Terminal,
            "系统终端",
            "未检测到可启动的系统终端",
        );
    };
    terminal_probe_available(
        TemporaryCliTerminalKind::Terminal,
        "系统终端",
        candidate.binary.to_string_lossy(),
    )
}

pub(crate) fn activate_terminal_target(
    _target: &cli_runtime::CliTerminalLocator,
) -> Result<(), String> {
    Err("当前系统暂不支持精确定位临时 CLI 窗口".to_string())
}

pub(crate) fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::Terminal => open_linux_default(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Terminal)),
        TemporaryCliTerminalKind::Warp => open_linux_command("warp-terminal", &[], script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Warp)),
        TemporaryCliTerminalKind::WezTerm => open_linux_command(
            "wezterm",
            &["start", "--cwd", &workdir.to_string_lossy()],
            script,
        )
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WezTerm)),
        TemporaryCliTerminalKind::Ghostty => open_linux_command(
            "ghostty",
            &[
                "--working-directory",
                &workdir.to_string_lossy(),
                "-e",
                "/bin/sh",
            ],
            script,
        )
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Ghostty)),
        TemporaryCliTerminalKind::Kitty => open_linux_command(
            "kitty",
            &["--directory", &workdir.to_string_lossy(), "/bin/sh"],
            script,
        )
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kitty)),
        TemporaryCliTerminalKind::Alacritty => open_linux_command(
            "alacritty",
            &[
                "--working-directory",
                &workdir.to_string_lossy(),
                "-e",
                "/bin/sh",
            ],
            script,
        )
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Alacritty)),
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
}

fn open_linux_default(script: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for candidate in linux_default_terminal_candidates() {
        match linux_terminal_command(&candidate, script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(err) => errors.push(format!("{}: {err}", candidate.binary.to_string_lossy())),
        }
    }
    Err(format!("无法调用系统终端: {}", errors.join("；")))
}

fn open_linux_command(binary: &str, args: &[&str], script: &Path) -> Result<(), String> {
    let mut command = Command::new(binary);
    command.args(args).arg(script);
    if let Some(path) = agent_cli::runtime_path_for(Path::new(binary)) {
        command.env("PATH", path);
    }
    spawn_visible_command(&mut command, &format!("无法调用 {binary}"))
}
