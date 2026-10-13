use super::{paths, WslAgent, WslDistribution};
use crate::{models::AgentCliKind, platform::process};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) fn executable() -> Result<PathBuf, String> {
    let system = std::env::var_os("SystemRoot").ok_or("无法定位 Windows 系统目录")?;
    let path = Path::new(&system).join("System32/wsl.exe");
    if !path.is_absolute() || !path.is_file() {
        return Err("未找到 Windows WSL 启动程序".into());
    }
    Ok(path)
}

pub(super) fn names(running: bool) -> Result<Vec<String>, String> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let mut command = Command::new(executable()?);
    command.args(["--list", "--quiet"]);
    if running {
        command.arg("--running");
    }
    process::configure_background_command(&mut command);
    #[cfg(windows)]
    let output = process::run_command_with_output_timeout_encoded(
        &mut command,
        Duration::from_secs(5),
        64 * 1024,
        process::OutputEncoding::Utf16Le,
    );
    #[cfg(not(windows))]
    let output =
        process::run_command_with_output_timeout(&mut command, Duration::from_secs(5), 64 * 1024);
    let output = output.map_err(|_| "无法调用 wsl.exe，请确认已安装 WSL".to_owned())?;
    if output.timed_out {
        return Err("读取 WSL 发行版超时".into());
    }
    if output.stdout_truncated || !output.status.is_some_and(|status| status.success()) {
        return Err("无法读取 WSL 发行版，请在终端运行 wsl --list 检查状态".into());
    }
    let mut names = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| paths::valid_distro(line))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

// The only printed values are paths and versions. Credentials are never read.
const PROBE: &str = r#"
printf 'BH_HOME\t%s\n' "$HOME"
bh_mode=$(wslinfo --networking-mode 2>/dev/null || true)
bh_gateway=$(ip -4 route show default 2>/dev/null | awk 'NR == 1 {print $3}')
printf 'BH_NETWORK\t%s\t%s\n' "$bh_mode" "$bh_gateway"
for bh_kind in codex claude gemini grok; do
  case "$bh_kind" in
    codex) bh_root="${CODEX_HOME:-$HOME/.codex}" ;;
    claude) bh_root="${CLAUDE_CONFIG_DIR:-$HOME/.claude}" ;;
    gemini) bh_root="${GEMINI_CLI_HOME:-$HOME}/.gemini" ;;
    grok) bh_root="${GROK_HOME:-$HOME/.grok}" ;;
  esac
  bh_executable=$(command -v "$bh_kind" 2>/dev/null || true)
  bh_version=''
  if [ -n "$bh_executable" ] && command -v timeout >/dev/null 2>&1; then
    bh_version=$(timeout 2s "$bh_executable" --version 2>/dev/null | head -n 1 | tr '\t\r\n' '   ')
  fi
  printf 'BH_AGENT\t%s\t%s\t%s\t%s\n' "$bh_kind" "$bh_root" "$bh_executable" "$bh_version"
done
"#;

pub(super) struct GuestInfo {
    pub home: String,
    pub agents: Vec<WslAgent>,
    pub network_mode: Option<String>,
    pub host_gateway: Option<String>,
}

pub(super) fn inspect(name: &str, timeout: Duration) -> Result<GuestInfo, String> {
    if !cfg!(windows) || !paths::valid_distro(name) {
        return Err("当前系统不能读取此 WSL 发行版".into());
    }
    let mut command = Command::new(executable()?);
    command.args(["--distribution", name, "--exec", "/bin/sh", "-lc", PROBE]);
    process::configure_background_command(&mut command);
    let output = process::run_command_with_output_timeout(&mut command, timeout, 32 * 1024)
        .map_err(|_| "无法读取 WSL 用户环境".to_owned())?;
    if output.timed_out {
        return Err("WSL 环境检测超时，请重试".into());
    }
    if output.stdout_truncated || !output.status.is_some_and(|status| status.success()) {
        return Err("WSL 环境检测失败，请检查发行版是否正常运行".into());
    }
    let mut home = None;
    let mut agents = Vec::new();
    let mut network_mode = None;
    let mut host_gateway = None;
    for line in output.stdout.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if let ["BH_HOME", path] = parts.as_slice() {
            home = Some(paths::guest_path(path)?.to_owned());
        }
        if let ["BH_NETWORK", mode, gateway] = parts.as_slice() {
            network_mode = ["mirrored", "nat", "virtioproxy", "none"]
                .contains(mode)
                .then(|| (*mode).to_owned());
            host_gateway = gateway
                .parse::<std::net::Ipv4Addr>()
                .ok()
                .map(|value| value.to_string());
        }
        if let ["BH_AGENT", kind, root, executable, version] = parts.as_slice() {
            let agent_kind = match *kind {
                "codex" => AgentCliKind::Codex,
                "claude" => AgentCliKind::ClaudeCode,
                "gemini" => AgentCliKind::Gemini,
                "grok" => AgentCliKind::Grok,
                _ => continue,
            };
            let config_root = paths::guest_path(root)?.to_owned();
            let executable = (!executable.is_empty())
                .then(|| paths::guest_path(executable).map(str::to_owned))
                .transpose()?;
            agents.push(WslAgent {
                agent_kind,
                config_root,
                executable,
                version: (!version.trim().is_empty())
                    .then(|| version.trim().chars().take(128).collect()),
            });
        }
    }
    if agents.len() != 4 {
        return Err("WSL 环境检测返回不完整，请重试".into());
    }
    Ok(GuestInfo {
        home: home.ok_or_else(|| "WSL 没有返回用户目录".to_owned())?,
        agents,
        network_mode,
        host_gateway,
    })
}

pub(super) fn distribution(
    name: String,
    previous: Option<&WslDistribution>,
    running: bool,
) -> WslDistribution {
    WslDistribution {
        id: name.to_lowercase(),
        name,
        running,
        home: previous.and_then(|item| item.home.clone()),
        agents: previous.map(|item| item.agents.clone()).unwrap_or_default(),
        network_mode: previous.and_then(|item| item.network_mode.clone()),
        host_gateway: previous.and_then(|item| item.host_gateway.clone()),
        error: None,
    }
}
