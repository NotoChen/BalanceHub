use super::{
    probe_terminal_command, spawn_visible_command, terminal_probe_available,
    terminal_probe_unavailable, TerminalDefinition, TerminalLaunch,
};
use crate::{
    models::{TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    services::agent_cli,
};
use std::{env, ffi::OsString, path::Path, process::Command};

const DEFINITIONS: &[TerminalDefinition] = &[
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Terminal,
        probe_linux_default_terminal,
        launch_linux_default,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Warp,
        probe_warp,
        launch_warp,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::WezTerm,
        probe_wezterm,
        launch_wezterm,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Ghostty,
        probe_ghostty,
        launch_ghostty,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Kitty,
        probe_kitty,
        launch_kitty,
        None,
    ),
    TerminalDefinition::new(
        TemporaryCliTerminalKind::Alacritty,
        probe_alacritty,
        launch_alacritty,
        None,
    ),
];

pub(super) const fn definitions() -> &'static [TerminalDefinition] {
    DEFINITIONS
}

fn probe_warp() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::Warp,
        "Warp",
        "warp-terminal",
        &["--version"],
    )
}

fn probe_wezterm() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::WezTerm,
        "WezTerm",
        "wezterm",
        &["--version"],
    )
}

fn probe_ghostty() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::Ghostty,
        "Ghostty",
        "ghostty",
        &["--version"],
    )
}

fn probe_kitty() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::Kitty,
        "Kitty",
        "kitty",
        &["--version"],
    )
}

fn probe_alacritty() -> TemporaryTerminalProbeResult {
    probe_terminal_command(
        TemporaryCliTerminalKind::Alacritty,
        "Alacritty",
        "alacritty",
        &["--version"],
    )
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

fn launch_linux_default(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_default(script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Terminal))
}

fn launch_warp(script: &Path, _workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_command("warp-terminal", &[], script)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Warp))
}

fn launch_wezterm(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_command(
        "wezterm",
        &["start", "--cwd", &workdir.to_string_lossy()],
        script,
    )
    .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WezTerm))
}

fn launch_ghostty(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_command(
        "ghostty",
        &[
            "--working-directory",
            &workdir.to_string_lossy(),
            "-e",
            "/bin/sh",
        ],
        script,
    )
    .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Ghostty))
}

fn launch_kitty(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_command(
        "kitty",
        &["--directory", &workdir.to_string_lossy(), "/bin/sh"],
        script,
    )
    .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kitty))
}

fn launch_alacritty(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_linux_command(
        "alacritty",
        &[
            "--working-directory",
            &workdir.to_string_lossy(),
            "-e",
            "/bin/sh",
        ],
        script,
    )
    .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Alacritty))
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
