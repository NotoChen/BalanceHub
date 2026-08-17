#[cfg(any(
    all(not(target_os = "macos"), not(target_os = "windows")),
    all(target_os = "macos", test)
))]
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
use crate::services::agent_cli;
#[cfg(any(not(target_os = "macos"), test))]
use crate::{limits, platform::process::run_command_with_output_timeout};
use crate::{
    models::{AppSettings, TemporaryCliTerminalKind, TemporaryTerminalProbeResult},
    services::cli_runtime,
};
use std::path::Path;
#[cfg(any(not(target_os = "macos"), test))]
use std::process::Command;
#[cfg(any(not(target_os = "macos"), test))]
use std::time::Duration;

#[cfg(all(target_os = "macos", test))]
pub(super) use macos::{
    build_macos_ghostty_activation_applescript, build_macos_ghostty_applescript,
    build_macos_iterm2_applescript, build_macos_terminal_applescript, warp_launcher_script_path,
};
#[cfg(test)]
pub(super) use windows::WINDOWS_POWERSHELL_SCRIPT_COMMAND;

type TerminalProbe = fn() -> TemporaryTerminalProbeResult;
type TerminalLauncher = fn(&Path, &Path) -> Result<TerminalLaunch, String>;
type TerminalActivator = fn(&cli_runtime::CliTerminalLocator) -> Result<(), String>;

#[derive(Clone, Copy)]
pub(super) struct TerminalDefinition {
    pub(super) kind: TemporaryCliTerminalKind,
    probe: TerminalProbe,
    launch: TerminalLauncher,
    activate: Option<TerminalActivator>,
}

impl TerminalDefinition {
    pub(super) const fn new(
        kind: TemporaryCliTerminalKind,
        probe: TerminalProbe,
        launch: TerminalLauncher,
        activate: Option<TerminalActivator>,
    ) -> Self {
        Self {
            kind,
            probe,
            launch,
            activate,
        }
    }

    fn probe(&self) -> TemporaryTerminalProbeResult {
        (self.probe)()
    }

    fn launch(&self, script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
        (self.launch)(script, workdir)
    }

    fn activate(&self, locator: &cli_runtime::CliTerminalLocator) -> Result<(), String> {
        self.activate
            .ok_or_else(|| "当前终端不支持精确定位临时 CLI 窗口".to_string())?(
            locator,
        )
    }
}

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

fn definition(kind: TemporaryCliTerminalKind) -> Option<&'static TerminalDefinition> {
    platform::definitions()
        .iter()
        .find(|definition| definition.kind == kind)
}

pub fn probe_available_terminals() -> Vec<TemporaryTerminalProbeResult> {
    TemporaryCliTerminalKind::ALL
        .iter()
        .filter_map(|kind| definition(*kind))
        .map(TerminalDefinition::probe)
        .filter(|result| result.available)
        .collect()
}

pub fn probe_terminal(kind: TemporaryCliTerminalKind) -> TemporaryTerminalProbeResult {
    definition(kind).map_or_else(
        || terminal_probe_unavailable(kind, kind.label(), "当前系统不支持该终端"),
        TerminalDefinition::probe,
    )
}

pub(super) fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    definition(settings.temporary_cli_terminal_kind)
        .ok_or_else(|| "当前系统不支持所选临时 CLI 终端".to_string())?
        .launch(script, workdir)
}

pub(super) fn activate_terminal_target(
    target: &cli_runtime::CliTerminalActivationTarget,
) -> Result<(), String> {
    definition(target.terminal_kind)
        .ok_or_else(|| "当前系统不支持所选临时 CLI 终端".to_string())?
        .activate(&target.locator)
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

#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn probe_terminal_command(
    kind: TemporaryCliTerminalKind,
    name: &str,
    binary: &str,
    args: &[&str],
) -> TemporaryTerminalProbeResult {
    let mut command = Command::new(binary);
    command.args(args);
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    if let Some(path) = agent_cli::runtime_path_for(Path::new(binary)) {
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

#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn spawn_visible_command(command: &mut Command, context: &str) -> Result<(), String> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("{context}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn assert_registry_is_valid(
        platform_name: &str,
        definitions: &[TerminalDefinition],
        expected_default: TemporaryCliTerminalKind,
    ) {
        let kinds = definitions
            .iter()
            .map(|definition| definition.kind)
            .collect::<HashSet<_>>();
        assert_eq!(
            kinds.len(),
            definitions.len(),
            "{platform_name} 终端注册表存在重复 kind"
        );
        assert!(
            kinds.contains(&expected_default),
            "{platform_name} 终端注册表缺少默认终端 {expected_default:?}"
        );
        assert!(
            kinds
                .iter()
                .all(|kind| TemporaryCliTerminalKind::ALL.contains(kind)),
            "{platform_name} 终端注册表包含未登记的 kind"
        );
    }

    #[test]
    fn terminal_identity_catalog_has_unique_keys() {
        let keys = TemporaryCliTerminalKind::ALL
            .iter()
            .map(|kind| serde_json::to_string(kind).expect("serialize terminal kind"))
            .collect::<HashSet<_>>();
        assert_eq!(keys.len(), TemporaryCliTerminalKind::ALL.len());
        assert!(TemporaryCliTerminalKind::ALL
            .iter()
            .all(|kind| !kind.label().is_empty()));
    }

    #[test]
    fn native_terminal_registry_has_unique_supported_kinds() {
        assert_registry_is_valid(
            "当前平台",
            platform::definitions(),
            TemporaryCliTerminalKind::default(),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_terminal_registry_is_valid() {
        assert_registry_is_valid(
            "macOS",
            macos::definitions(),
            TemporaryCliTerminalKind::Terminal,
        );
    }

    #[cfg(any(
        all(not(target_os = "macos"), not(target_os = "windows")),
        target_os = "macos"
    ))]
    #[test]
    fn linux_terminal_registry_is_valid() {
        assert_registry_is_valid(
            "Linux",
            linux::definitions(),
            TemporaryCliTerminalKind::Terminal,
        );
    }

    #[test]
    fn windows_terminal_registry_is_valid() {
        assert_registry_is_valid(
            "Windows",
            windows::definitions(),
            TemporaryCliTerminalKind::WindowsTerminal,
        );
    }
}
