use super::{terminal_probe_available, terminal_probe_unavailable, TerminalLaunch};
use crate::{
    limits,
    models::{AppSettings, TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    platform::process::{run_command_with_output_timeout, CommandOutput},
    services::{
        cli_runtime,
        temporary_cli::script::{
            script_command, script_command_without_exec, set_executable, user_shell,
        },
    },
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SYSTEM_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const TERMINAL_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);

pub fn probe_available_terminals() -> Vec<TemporaryTerminalProbeResult> {
    [
        TemporaryCliTerminalKind::Warp,
        TemporaryCliTerminalKind::ITerm2,
        TemporaryCliTerminalKind::WezTerm,
        TemporaryCliTerminalKind::Kaku,
        TemporaryCliTerminalKind::Ghostty,
        TemporaryCliTerminalKind::Terminal,
        TemporaryCliTerminalKind::Kitty,
        TemporaryCliTerminalKind::Alacritty,
    ]
    .into_iter()
    .map(probe_terminal)
    .filter(|result| result.available)
    .collect()
}

pub fn probe_terminal(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    match kind {
        TemporaryCliTerminalKind::Terminal
        | TemporaryCliTerminalKind::ITerm2
        | TemporaryCliTerminalKind::Warp
        | TemporaryCliTerminalKind::WezTerm
        | TemporaryCliTerminalKind::Kaku
        | TemporaryCliTerminalKind::Ghostty
        | TemporaryCliTerminalKind::Kitty
        | TemporaryCliTerminalKind::Alacritty => probe_macos_terminal_app(kind),
        _ => terminal_probe_unavailable(kind, "临时终端", "仅支持自动扫描到的具体终端"),
    }
}

fn probe_macos_terminal_app(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    let (name, application, bundle_id) = match kind {
        TemporaryCliTerminalKind::Terminal => ("Terminal", "Terminal", "com.apple.Terminal"),
        TemporaryCliTerminalKind::ITerm2 => ("iTerm2", "iTerm", "com.googlecode.iterm2"),
        TemporaryCliTerminalKind::Warp => ("Warp", "Warp", "dev.warp.Warp-Stable"),
        TemporaryCliTerminalKind::WezTerm => ("WezTerm", "WezTerm", "org.wezfurlong.wezterm"),
        TemporaryCliTerminalKind::Kaku => ("Kaku", "Kaku", "com.kaku.Kaku"),
        TemporaryCliTerminalKind::Ghostty => ("Ghostty", "Ghostty", "com.mitchellh.ghostty"),
        TemporaryCliTerminalKind::Kitty => ("Kitty", "kitty", "net.kovidgoyal.kitty"),
        TemporaryCliTerminalKind::Alacritty => {
            ("Alacritty", "Alacritty", "org.alacritty.Alacritty")
        }
        _ => return terminal_probe_unavailable(kind, "临时终端", "当前系统不支持该终端"),
    };
    let Some(bundle) = locate_macos_application(application, bundle_id) else {
        return terminal_probe_unavailable(kind, name, format!("未检测到 {name}"));
    };
    terminal_probe_available(kind, name, macos_app_version(&bundle))
}

/// Locate a macOS application without sending it an Apple Event or activating it.
/// Known install locations are checked first; Spotlight is only a fallback for apps
/// installed outside the standard locations.
fn locate_macos_application(application: &str, bundle_id: &str) -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/Applications").join(format!("{application}.app")),
        PathBuf::from("/System/Applications").join(format!("{application}.app")),
        PathBuf::from("/System/Applications/Utilities").join(format!("{application}.app")),
    ];
    if let Some(home) = env::var_os("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join("Applications")
                .join(format!("{application}.app")),
        );
    }
    if let Some(bundle) = candidates
        .into_iter()
        .find(|path| path.join("Contents/Info.plist").is_file())
    {
        return Some(bundle);
    }

    let query = format!("kMDItemCFBundleIdentifier == '{bundle_id}'");
    let mut command = Command::new("/usr/bin/mdfind");
    command.arg(&query);
    let output = run_command_with_output_timeout(
        &mut command,
        SYSTEM_COMMAND_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return None;
    }
    output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .find(|path| path.join("Contents/Info.plist").is_file())
}

fn macos_app_version(bundle: &Path) -> String {
    let info_plist = bundle.join("Contents/Info.plist");
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        let mut command = Command::new("plutil");
        command
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(&info_plist);
        let Ok(output) = run_command_with_output_timeout(
            &mut command,
            SYSTEM_COMMAND_TIMEOUT,
            limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
        ) else {
            continue;
        };
        if !output.timed_out && output.status.is_some_and(|status| status.success()) {
            let version = output.stdout.trim().to_string();
            if !version.is_empty() {
                return version;
            }
        }
    }
    String::new()
}

pub(crate) fn activate_terminal_target(
    target: &cli_runtime::CliTerminalLocator,
) -> Result<(), String> {
    match target {
        cli_runtime::CliTerminalLocator::Ghostty { terminal_id } => {
            let script = build_macos_ghostty_activation_applescript(terminal_id);
            run_command(
                Command::new("osascript").arg("-e").arg(script),
                "无法打开对应的 Ghostty CLI 窗口，窗口可能已关闭",
            )
        }
    }
}

pub(crate) fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::Terminal => open_macos_terminal(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Terminal)),
        TemporaryCliTerminalKind::ITerm2 => open_macos_iterm2(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::ITerm2)),
        TemporaryCliTerminalKind::Warp => open_macos_warp(script, workdir)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Warp)),
        TemporaryCliTerminalKind::WezTerm => {
            open_macos_wezterm_compatible("WezTerm", script, workdir)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WezTerm))
        }
        TemporaryCliTerminalKind::Kaku => open_macos_wezterm_compatible("Kaku", script, workdir)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kaku)),
        TemporaryCliTerminalKind::Ghostty => open_macos_ghostty(script),
        TemporaryCliTerminalKind::Kitty => open_macos_shell_app("kitty", &["-e"], script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Kitty)),
        TemporaryCliTerminalKind::Alacritty => open_macos_shell_app("Alacritty", &["-e"], script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Alacritty)),
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
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

pub(crate) fn build_macos_terminal_applescript(script: &Path) -> String {
    let launcher = apple_script_exec_launcher_command(script);
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "Terminal" is running
tell application "Terminal"
    if was_running then
        activate
        do script launcher_script
    else
        launch
        do script launcher_script
        activate
    end if
end tell"#,
    )
}

pub(crate) fn build_macos_iterm2_applescript(script: &Path) -> String {
    let launcher = apple_script_exec_launcher_command(script);
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "iTerm" is running
tell application "iTerm"
    if was_running then
        activate
        if (count of windows) = 0 then
            create window with default profile
        else
            tell current window
                create tab with default profile
            end tell
        end if
    else
        activate
        set waited to 0
        repeat while (count of windows) = 0
            delay 0.1
            set waited to waited + 1
            if waited >= 30 then exit repeat
        end repeat
        if (count of windows) = 0 then
            create window with default profile
        end if
    end if
    tell current session of current window
        write text launcher_script
    end tell
end tell"#,
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

pub(crate) fn build_macos_ghostty_applescript(script: &Path) -> String {
    let launcher = apple_script_launcher_command(script);
    format!(
        r#"set launcher_command to {launcher}
tell application "Ghostty"
    set target_window to new window with configuration {{command:launcher_command}}
    set target_tab to selected tab of target_window
    set target_terminal to focused terminal of target_tab
    activate
    return id of target_terminal
end tell"#,
    )
}

pub(crate) fn build_macos_ghostty_activation_applescript(terminal_id: &str) -> String {
    let target_id = apple_script_quote(terminal_id);
    format!(
        r#"set target_id to {target_id}
tell application "Ghostty"
    set matching_terminals to every terminal whose id is target_id
    if (count of matching_terminals) is 0 then error "terminal not found"
    focus item 1 of matching_terminals
    activate
end tell"#,
    )
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

fn run_command(command: &mut Command, context: &str) -> Result<(), String> {
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

fn run_command_text(command: &mut Command, context: &str) -> Result<String, String> {
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

fn apple_script_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn apple_script_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command_without_exec(script))
}

fn apple_script_exec_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command(script))
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

fn command_error_message(output: &CommandOutput) -> String {
    let stderr = output.stderr.trim().to_string();
    if stderr.is_empty() {
        "系统终端没有成功启动临时 CLI".to_string()
    } else {
        stderr
    }
}
