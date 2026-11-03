use super::super::{terminal_probe_available, terminal_probe_unavailable};
use crate::{
    limits,
    models::{TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    platform::process::run_command_with_output_timeout,
};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SYSTEM_COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn probe_terminal() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Terminal,
        "Terminal",
        "Terminal",
        "com.apple.Terminal",
    )
}

pub(super) fn probe_iterm2() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::ITerm2,
        "iTerm2",
        "iTerm",
        "com.googlecode.iterm2",
    )
}

pub(super) fn probe_warp() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Warp,
        "Warp",
        "Warp",
        "dev.warp.Warp-Stable",
    )
}

pub(super) fn probe_wezterm() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::WezTerm,
        "WezTerm",
        "WezTerm",
        "org.wezfurlong.wezterm",
    )
}

pub(super) fn probe_kaku() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Kaku,
        "Kaku",
        "Kaku",
        "com.kaku.Kaku",
    )
}

pub(super) fn probe_ghostty() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Ghostty,
        "Ghostty",
        "Ghostty",
        "com.mitchellh.ghostty",
    )
}

pub(super) fn probe_kitty() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Kitty,
        "Kitty",
        "kitty",
        "net.kovidgoyal.kitty",
    )
}

pub(super) fn probe_alacritty() -> TemporaryTerminalProbeResult {
    probe_macos_terminal_app(
        TemporaryCliTerminalKind::Alacritty,
        "Alacritty",
        "Alacritty",
        "org.alacritty.Alacritty",
    )
}

fn probe_macos_terminal_app(
    kind: TemporaryCliTerminalKind,
    name: &str,
    application: &str,
    bundle_id: &str,
) -> TemporaryTerminalProbeResult {
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
