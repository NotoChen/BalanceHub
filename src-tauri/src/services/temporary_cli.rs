use crate::{
    models::{
        AppSettings, LivenessCliKind, Provider, TemporaryCliInstance, TemporaryCliTerminalKind,
    },
    services::{
        cli_runtime,
        liveness::{anthropic_base_url, openai_base_url, LivenessRunner},
    },
    util::unix_millis as now_millis,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

struct TerminalLaunch {
    terminal_kind: TemporaryCliTerminalKind,
    locator: Option<cli_runtime::CliTerminalLocator>,
}

impl TerminalLaunch {
    fn untracked(terminal_kind: TemporaryCliTerminalKind) -> Self {
        Self {
            terminal_kind,
            locator: None,
        }
    }

    #[cfg(target_os = "macos")]
    fn tracked(
        terminal_kind: TemporaryCliTerminalKind,
        locator: cli_runtime::CliTerminalLocator,
    ) -> Self {
        Self {
            terminal_kind,
            locator: Some(locator),
        }
    }
}

pub fn launch(
    settings: &AppSettings,
    provider: &Provider,
    cli_kind: LivenessCliKind,
    workdir: &Path,
    api_key_override: &str,
    model_override: &str,
) -> Result<TemporaryCliInstance, String> {
    if !workdir.is_dir() {
        return Err("工作目录不存在".to_string());
    }
    let api_key = if api_key_override.trim().is_empty() {
        provider.auth.api_key.trim().to_string()
    } else {
        api_key_override.trim().to_string()
    };
    if api_key.is_empty() {
        return Err("缺少 API Key，无法启动临时 CLI".to_string());
    }
    if provider.identity.base_url.trim().is_empty() {
        return Err("缺少中转站地址，无法启动临时 CLI".to_string());
    }

    let cli = match cli_kind {
        LivenessCliKind::Codex => LivenessRunner::find_codex_cli(&settings.codex_cli_path)?,
        LivenessCliKind::ClaudeCode => LivenessRunner::find_claude_cli(&settings.claude_cli_path)?,
    };
    let model = if model_override.trim().is_empty() {
        effective_model(settings, provider)
    } else {
        model_override.trim().to_string()
    };
    let base_url = match cli_kind {
        LivenessCliKind::Codex => openai_base_url(provider),
        LivenessCliKind::ClaudeCode => anthropic_base_url(provider),
    };

    let registered = cli_runtime::register_instance(
        provider,
        cli_kind,
        workdir,
        settings.temporary_cli_terminal_kind,
    )?;
    let script = temporary_script_path(provider, cli_kind);
    if let Some(parent) = script.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            cli_runtime::mark_instance_exited(&registered.status_path, None);
            return Err(format!(
                "创建临时 CLI 启动目录失败({}): {err}",
                parent.display()
            ));
        }
    }

    let launch_script = LaunchScriptInput {
        script: &script,
        cli_kind,
        cli_path: &cli.path,
        workdir,
        provider_name: &provider.identity.name,
        api_key: &api_key,
        base_url: &base_url,
        model: &model,
        status_path: &registered.status_path,
    };
    if let Err(err) = write_launch_script(&launch_script) {
        cli_runtime::mark_instance_exited(&registered.status_path, None);
        cleanup_launch_files(&script, cli_kind);
        return Err(err);
    }

    let terminal_launch = match open_script_in_terminal(settings, &script, workdir) {
        Ok(terminal_launch) => terminal_launch,
        Err(err) => {
            cli_runtime::mark_instance_exited(&registered.status_path, None);
            cleanup_launch_files(&script, cli_kind);
            return Err(err);
        }
    };

    Ok(cli_runtime::record_terminal_launch(
        &registered.instance.id,
        terminal_launch.terminal_kind,
        terminal_launch.locator,
    )
    .unwrap_or(registered.instance))
}

pub fn activate(instance_id: &str) -> Result<(), String> {
    let target = cli_runtime::activation_target(instance_id)?;
    activate_terminal_target(&target)
}

#[cfg(target_os = "macos")]
fn activate_terminal_target(target: &cli_runtime::CliTerminalLocator) -> Result<(), String> {
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

#[cfg(not(target_os = "macos"))]
fn activate_terminal_target(_target: &cli_runtime::CliTerminalLocator) -> Result<(), String> {
    Err("当前系统暂不支持精确定位临时 CLI 窗口".to_string())
}

fn cleanup_launch_files(script: &Path, cli_kind: LivenessCliKind) {
    let _ = fs::remove_file(script);
    if let Some(settings_path) = temporary_claude_settings_path(script, cli_kind) {
        let _ = fs::remove_file(settings_path);
    }
    if let Some(parent) = script.parent() {
        let _ = fs::remove_dir(parent);
    }
}

fn effective_model(settings: &AppSettings, provider: &Provider) -> String {
    let provider_model = provider.liveness.model.trim();
    if provider_model.is_empty() {
        settings.liveness_model.trim().to_string()
    } else {
        provider_model.to_string()
    }
}

fn temporary_script_path(provider: &Provider, cli_kind: LivenessCliKind) -> PathBuf {
    let kind = match cli_kind {
        LivenessCliKind::Codex => "codex",
        LivenessCliKind::ClaudeCode => "claude",
    };
    env::temp_dir()
        .join(format!(
            "balancehub-temporary-cli-{}-{}-{}",
            sanitize_path_part(&provider.identity.id),
            std::process::id(),
            now_millis()
        ))
        .join(temporary_script_file_name(kind))
}

fn temporary_script_file_name(kind: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{kind}.cmd")
    } else if cfg!(target_os = "macos") {
        format!("{kind}.command")
    } else {
        format!("{kind}.sh")
    }
}

fn temporary_claude_settings_path(script: &Path, cli_kind: LivenessCliKind) -> Option<PathBuf> {
    matches!(cli_kind, LivenessCliKind::ClaudeCode).then(|| {
        script
            .parent()
            .map(|parent| parent.join("claude-settings.json"))
            .unwrap_or_else(|| env::temp_dir().join("claude-settings.json"))
    })
}

fn sanitize_path_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

struct LaunchScriptInput<'a> {
    script: &'a Path,
    cli_kind: LivenessCliKind,
    cli_path: &'a str,
    workdir: &'a Path,
    provider_name: &'a str,
    api_key: &'a str,
    base_url: &'a str,
    model: &'a str,
    status_path: &'a Path,
}

#[cfg(not(target_os = "windows"))]
fn write_launch_script(input: &LaunchScriptInput<'_>) -> Result<(), String> {
    let claude_settings_path = temporary_claude_settings_path(input.script, input.cli_kind);
    if let Some(path) = &claude_settings_path {
        write_claude_settings(path, input.api_key, input.base_url)?;
    }
    let args = cli_args(
        input.cli_kind,
        input.provider_name,
        input.base_url,
        input.model,
        claude_settings_path.as_deref(),
    );
    let path_export = LivenessRunner::runtime_path_for_cli(Path::new(input.cli_path))
        .map(|path| format!("export PATH={}\n", shell_quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let script_dir = input
        .script
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(env::temp_dir);

    let auth_block = match input.cli_kind {
        LivenessCliKind::Codex => format!(
            "unset CODEX_API_KEY CODEX_ACCESS_TOKEN\nexport OPENAI_API_KEY={}\n",
            shell_quote(input.api_key)
        ),
        LivenessCliKind::ClaudeCode => {
            "unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_BASE_URL\n".to_string()
        }
    };
    let cleanup_settings = claude_settings_path
        .as_ref()
        .map(|path| format!("rm -f {}\n", shell_quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let login_shell_bootstrap = login_shell_bootstrap(input.script);

    let text = format!(
        r#"#!/bin/sh
set -u
{login_shell_bootstrap}bh_status_file={status_path}
bh_write_status() {{
  bh_tmp="$bh_status_file.tmp.$$"
  printf '{{"status":"%s","pid":%s,"endedAt":%s,"exitCode":%s}}\n' "$1" "$2" "$3" "$4" > "$bh_tmp"
  mv -f "$bh_tmp" "$bh_status_file"
}}
bh_now_ms() {{
  echo $(( $(date +%s) * 1000 ))
}}
bh_write_status running "$$" null null
cd {workdir}
status=$?
if [ "$status" -ne 0 ]; then
  bh_write_status exited null "$(bh_now_ms)" "$status"
  exit "$status"
fi
{path_export}{color_block}{auth_block}{cli} {args}
status=$?
bh_write_status exited null "$(bh_now_ms)" "$status"
rm -f "$0"
{cleanup_settings}rmdir {script_dir} 2>/dev/null || true
exit "$status"
"#,
        status_path = shell_quote(&input.status_path.to_string_lossy()),
        login_shell_bootstrap = login_shell_bootstrap,
        workdir = shell_quote(&input.workdir.to_string_lossy()),
        color_block = unix_color_block(),
        cli = shell_quote(input.cli_path),
        args = args
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
        script_dir = shell_quote(&script_dir.to_string_lossy()),
    );

    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))?;
    set_executable(input.script)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_launch_script(input: &LaunchScriptInput<'_>) -> Result<(), String> {
    let claude_settings_path = temporary_claude_settings_path(input.script, input.cli_kind);
    if let Some(path) = &claude_settings_path {
        write_claude_settings(path, input.api_key, input.base_url)?;
    }
    let args = cli_args(
        input.cli_kind,
        input.provider_name,
        input.base_url,
        input.model,
        claude_settings_path.as_deref(),
    );
    let auth_block = match input.cli_kind {
        LivenessCliKind::Codex => format!(
            "set CODEX_API_KEY=\r\nset CODEX_ACCESS_TOKEN=\r\nset \"OPENAI_API_KEY={api_key}\"\r\n",
            api_key = escape_cmd_value(input.api_key)
        ),
        LivenessCliKind::ClaudeCode => {
            "set ANTHROPIC_API_KEY=\r\nset ANTHROPIC_AUTH_TOKEN=\r\nset ANTHROPIC_BASE_URL=\r\n"
                .to_string()
        }
    };
    let script_dir = input
        .script
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(env::temp_dir);
    let text = format!(
        "@echo off\r\nsetlocal\r\nset \"BH_STATUS_FILE={status_path}\"\r\nset \"BH_PID=null\"\r\nfor /f %%P in ('powershell -NoProfile -Command \"(Get-CimInstance Win32_Process ^| Where-Object ProcessId -eq $PID).ParentProcessId\"') do set \"BH_PID=%%P\"\r\ncall :BH_WRITE_STATUS running %BH_PID% null null\r\ncd /d \"{workdir}\"\r\nif errorlevel 1 goto BH_WORKDIR_ERROR\r\n{color_block}{auth_block}\"{cli}\" {args}\r\nset STATUS=%ERRORLEVEL%\r\ngoto BH_FINISH\r\n:BH_WORKDIR_ERROR\r\nset STATUS=%ERRORLEVEL%\r\n:BH_FINISH\r\nset \"BH_ENDED=null\"\r\nfor /f %%T in ('powershell -NoProfile -Command \"[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()\"') do set \"BH_ENDED=%%T\"\r\ncall :BH_WRITE_STATUS exited null %BH_ENDED% %STATUS%\r\n{cleanup_settings}del \"%~f0\"\r\nrmdir \"{script_dir}\" 2>nul\r\nexit /b %STATUS%\r\n:BH_WRITE_STATUS\r\nset \"BH_TMP=%BH_STATUS_FILE%.tmp.%RANDOM%\"\r\n> \"%BH_TMP%\" echo {{\"status\":\"%~1\",\"pid\":%~2,\"endedAt\":%~3,\"exitCode\":%~4}}\r\nmove /Y \"%BH_TMP%\" \"%BH_STATUS_FILE%\" >nul\r\nexit /b 0\r\n",
        status_path = escape_cmd_value(&input.status_path.display().to_string()),
        workdir = escape_cmd_value(&input.workdir.display().to_string()),
        color_block = windows_color_block(),
        cli = escape_cmd_value(input.cli_path),
        args = args
            .iter()
            .map(|arg| windows_quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
        script_dir = escape_cmd_value(&script_dir.display().to_string()),
        cleanup_settings = claude_settings_path
            .as_ref()
            .map(|path| format!("del \"{}\" 2>nul\r\n", escape_cmd_value(&path.display().to_string())))
            .unwrap_or_default(),
    );

    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))
}

#[cfg(not(target_os = "windows"))]
fn unix_color_block() -> &'static str {
    "unset NO_COLOR\nexport CLICOLOR=1\nif [ \"${TERM:-dumb}\" = \"dumb\" ]; then export TERM=xterm-256color; fi\n"
}

#[cfg(target_os = "windows")]
fn windows_color_block() -> &'static str {
    "set NO_COLOR=\r\nset CLICOLOR=1\r\nif not defined TERM set \"TERM=xterm-256color\"\r\n"
}

fn write_claude_settings(path: &Path, api_key: &str, base_url: &str) -> Result<(), String> {
    let config = serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": api_key.trim(),
            "ANTHROPIC_BASE_URL": base_url.trim(),
        }
    });
    let text = serde_json::to_string_pretty(&config)
        .map_err(|err| format!("生成 Claude 配置失败: {err}"))?;
    fs::write(path, text).map_err(|err| format!("写入 Claude 临时配置失败: {err}"))?;
    restrict_to_owner(path)
}

/// 临时配置里有明文 API Key，权限收紧到仅属主可读写（脚本本身已是 0700）。
#[cfg(not(target_os = "windows"))]
fn restrict_to_owner(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("读取 Claude 临时配置权限失败: {err}"))?
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|err| format!("设置 Claude 临时配置权限失败: {err}"))
}

#[cfg(target_os = "windows")]
fn restrict_to_owner(_path: &Path) -> Result<(), String> {
    // %TEMP% 位于用户目录下，默认 ACL 已限制为本用户可见。
    Ok(())
}

/// 清理历史残留的临时文件：启动脚本目录（终端从未执行脚本时不会自清）、
/// 测活的隔离 HOME 与输出文件（超时/崩溃路径可能泄漏）。这些目录里可能包含
/// 明文凭据，启动时兜底清扫一次；只清超过 24 小时的，避免碰到正在使用的会话。
pub fn cleanup_stale() {
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    const PREFIXES: [&str; 4] = [
        "balancehub-temporary-cli-",
        "balancehub-codex-home-",
        "balancehub-claude-home-",
        "balancehub-codex-",
    ];

    let Ok(entries) = fs::read_dir(env::temp_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_AFTER);
        if !stale {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}

fn cli_args(
    cli_kind: LivenessCliKind,
    provider_name: &str,
    base_url: &str,
    model: &str,
    claude_settings_path: Option<&Path>,
) -> Vec<String> {
    match cli_kind {
        LivenessCliKind::Codex => {
            let mut args = Vec::new();
            let display_name = provider_name.trim();
            let display_name = if display_name.is_empty() {
                "Custom"
            } else {
                display_name
            };
            if !model.trim().is_empty() {
                args.extend(["-m".to_string(), model.trim().to_string()]);
            }
            args.extend([
                "-c".to_string(),
                "model_provider=\"custom\"".to_string(),
                "-c".to_string(),
                format!(
                    "model_providers.custom.name=\"{}\"",
                    escape_toml_string(display_name)
                ),
                "-c".to_string(),
                format!(
                    "model_providers.custom.base_url=\"{}\"",
                    escape_toml_string(base_url)
                ),
                "-c".to_string(),
                "model_providers.custom.wire_api=\"responses\"".to_string(),
                "-c".to_string(),
                "model_providers.custom.env_key=\"OPENAI_API_KEY\"".to_string(),
                "-c".to_string(),
                "model_providers.custom.requires_openai_auth=true".to_string(),
            ]);
            args
        }
        LivenessCliKind::ClaudeCode => {
            let mut args = Vec::new();
            if let Some(path) = claude_settings_path {
                args.extend(["--settings".to_string(), path.to_string_lossy().to_string()]);
            }
            if !model.trim().is_empty() {
                args.extend(["--model".to_string(), model.trim().to_string()]);
            }
            args
        }
    }
}

#[cfg(target_os = "macos")]
fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::Auto => open_macos_auto(script, workdir),
        TemporaryCliTerminalKind::SystemDefault => open_macos_system_default(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::SystemDefault)),
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
        TemporaryCliTerminalKind::Custom => {
            open_unix_custom_terminal(&settings.temporary_cli_terminal_command, script, workdir)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Custom))
        }
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
}

#[cfg(target_os = "macos")]
fn open_macos_auto(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    let mut errors = Vec::new();

    if app_exists_macos("Warp") {
        match open_macos_warp(script, workdir) {
            Ok(()) => return Ok(TerminalLaunch::untracked(TemporaryCliTerminalKind::Warp)),
            Err(err) => errors.push(err),
        }
    }
    if app_exists_macos("iTerm") {
        match open_macos_iterm2(script) {
            Ok(()) => return Ok(TerminalLaunch::untracked(TemporaryCliTerminalKind::ITerm2)),
            Err(err) => errors.push(err),
        }
    }
    if app_exists_macos("WezTerm") {
        match open_macos_wezterm_compatible("WezTerm", script, workdir) {
            Ok(()) => return Ok(TerminalLaunch::untracked(TemporaryCliTerminalKind::WezTerm)),
            Err(err) => errors.push(err),
        }
    }
    if app_exists_macos("Kaku") {
        match open_macos_wezterm_compatible("Kaku", script, workdir) {
            Ok(()) => return Ok(TerminalLaunch::untracked(TemporaryCliTerminalKind::Kaku)),
            Err(err) => errors.push(err),
        }
    }
    if app_exists_macos("Ghostty") {
        match open_macos_ghostty(script) {
            Ok(terminal_launch) => return Ok(terminal_launch),
            Err(err) => errors.push(err),
        }
    }
    match open_macos_terminal(script) {
        Ok(()) => {
            return Ok(TerminalLaunch::untracked(
                TemporaryCliTerminalKind::Terminal,
            ))
        }
        Err(err) => errors.push(err),
    }

    Err(format!("无法自动启动临时 CLI 终端: {}", errors.join("；")))
}

#[cfg(target_os = "macos")]
fn open_macos_system_default(script: &Path) -> Result<(), String> {
    run_command(Command::new("open").arg(script), "无法调用系统默认终端")
}

#[cfg(target_os = "macos")]
fn open_macos_terminal(script: &Path) -> Result<(), String> {
    run_command(
        Command::new("osascript")
            .arg("-e")
            .arg(build_macos_terminal_applescript(script)),
        "无法调用 Terminal",
    )
}

#[cfg(target_os = "macos")]
fn open_macos_iterm2(script: &Path) -> Result<(), String> {
    run_command(
        Command::new("osascript")
            .arg("-e")
            .arg(build_macos_iterm2_applescript(script)),
        "无法调用 iTerm2",
    )
}

#[cfg(target_os = "macos")]
fn build_macos_terminal_applescript(script: &Path) -> String {
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

#[cfg(target_os = "macos")]
fn build_macos_iterm2_applescript(script: &Path) -> String {
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn build_macos_ghostty_applescript(script: &Path) -> String {
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

#[cfg(target_os = "macos")]
fn build_macos_ghostty_activation_applescript(terminal_id: &str) -> String {
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn warp_launcher_script_path(script: &Path) -> PathBuf {
    script
        .parent()
        .map(|parent| parent.join("warp-launcher"))
        .unwrap_or_else(|| env::temp_dir().join("warp-launcher"))
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn app_exists_macos(app: &str) -> bool {
    Command::new("osascript")
        .arg("-e")
        .arg(format!("id of application {}", apple_script_quote(app)))
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::Auto => open_windows_auto(script, workdir),
        TemporaryCliTerminalKind::SystemDefault | TemporaryCliTerminalKind::WindowsTerminal => {
            open_windows_terminal(script, workdir)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WindowsTerminal))
        }
        TemporaryCliTerminalKind::CommandPrompt | TemporaryCliTerminalKind::Terminal => {
            open_windows_command_prompt(script)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::CommandPrompt))
        }
        TemporaryCliTerminalKind::PowerShell => open_windows_powershell(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::PowerShell)),
        TemporaryCliTerminalKind::Custom => {
            open_windows_custom_terminal(&settings.temporary_cli_terminal_command, script, workdir)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Custom))
        }
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
}

#[cfg(target_os = "windows")]
fn open_windows_auto(script: &Path, workdir: &Path) -> Result<TerminalLaunch, String> {
    open_windows_terminal(script, workdir)
        .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::WindowsTerminal))
        .or_else(|_| {
            open_windows_command_prompt(script)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::CommandPrompt))
        })
}

#[cfg(target_os = "windows")]
fn open_windows_terminal(script: &Path, workdir: &Path) -> Result<(), String> {
    run_command(
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
    run_command(
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .arg(script),
        "无法调用命令提示符",
    )
}

#[cfg(target_os = "windows")]
fn open_windows_powershell(script: &Path) -> Result<(), String> {
    run_command(
        Command::new("powershell")
            .args(["-NoExit", "-ExecutionPolicy", "Bypass", "-Command"])
            .arg(format!(
                "cmd /c {}",
                windows_quote(&script.to_string_lossy())
            )),
        "无法调用 PowerShell",
    )
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_script_in_terminal(
    settings: &AppSettings,
    script: &Path,
    workdir: &Path,
) -> Result<TerminalLaunch, String> {
    match settings.temporary_cli_terminal_kind {
        TemporaryCliTerminalKind::Auto => open_linux_default(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Auto)),
        TemporaryCliTerminalKind::SystemDefault => open_linux_default(script)
            .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::SystemDefault)),
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
        TemporaryCliTerminalKind::Custom => {
            open_unix_custom_terminal(&settings.temporary_cli_terminal_command, script, workdir)
                .map(|()| TerminalLaunch::untracked(TemporaryCliTerminalKind::Custom))
        }
        _ => Err("当前系统不支持所选临时 CLI 终端".to_string()),
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_linux_default(script: &Path) -> Result<(), String> {
    let mut errors = Vec::new();

    if let Ok(output) = Command::new("xdg-terminal-exec").arg(script).output() {
        if output.status.success() {
            return Ok(());
        }
        errors.push(command_error_message(output));
    }

    if let Some(terminal) = env::var_os("TERMINAL").filter(|value| !value.is_empty()) {
        match Command::new(&terminal).arg("-e").arg(script).output() {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => errors.push(command_error_message(output)),
            Err(err) => errors.push(format!("{}: {err}", terminal.to_string_lossy())),
        }
    }

    let terminals = [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ];
    for terminal in terminals {
        let output = Command::new(terminal).arg("-e").arg(script).output();
        match output {
            Ok(output) if output.status.success() => return Ok(()),
            Ok(output) => errors.push(command_error_message(output)),
            Err(err) => errors.push(err.to_string()),
        }
    }
    Err(format!("无法调用系统终端: {}", errors.join("；")))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_linux_command(binary: &str, args: &[&str], script: &Path) -> Result<(), String> {
    let mut command = Command::new(binary);
    command.args(args).arg(script);
    run_command(&mut command, &format!("无法调用 {binary}"))
}

#[cfg(not(target_os = "windows"))]
fn open_unix_custom_terminal(template: &str, script: &Path, workdir: &Path) -> Result<(), String> {
    let command = custom_terminal_command(template, script, workdir, shell_quote)?;
    run_command(
        Command::new("/bin/sh").arg("-lc").arg(command),
        "无法调用自定义终端命令",
    )
}

#[cfg(target_os = "windows")]
fn open_windows_custom_terminal(
    template: &str,
    script: &Path,
    workdir: &Path,
) -> Result<(), String> {
    let command = custom_terminal_command(template, script, workdir, windows_quote)?;
    run_command(
        Command::new("cmd").arg("/C").arg(command),
        "无法调用自定义终端命令",
    )
}

fn custom_terminal_command(
    template: &str,
    script: &Path,
    workdir: &Path,
    quote: fn(&str) -> String,
) -> Result<String, String> {
    let trimmed = template.trim();
    if trimmed.is_empty() {
        return Err("自定义终端命令为空".to_string());
    }

    let script_value = quote(&script.to_string_lossy());
    let workdir_value = quote(&workdir.to_string_lossy());
    let mut command = trimmed
        .replace("{script}", &script_value)
        .replace("{workdir}", &workdir_value);
    if !trimmed.contains("{script}") {
        command.push(' ');
        command.push_str(&script_value);
    }
    Ok(command)
}

fn run_command(command: &mut Command, context: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|err| format!("{context}: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{context}: {}", command_error_message(output)))
    }
}

#[cfg(target_os = "macos")]
fn run_command_text(command: &mut Command, context: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|err| format!("{context}: {err}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("{context}: {}", command_error_message(output)))
    }
}

#[cfg(not(target_os = "windows"))]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("读取临时脚本权限失败: {err}"))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).map_err(|err| format!("设置临时脚本权限失败: {err}"))
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(target_os = "windows"))]
fn login_shell_bootstrap(script: &Path) -> String {
    let command = format!("exec /bin/sh {}", shell_quote(&script.to_string_lossy()));
    format!(
        "if [ \"${{BALANCEHUB_LOGIN_ENV_READY:-}}\" != \"1\" ]; then\n  export BALANCEHUB_LOGIN_ENV_READY=1\n  exec {} -lic {}\nfi\nunset BALANCEHUB_LOGIN_ENV_READY\n",
        shell_quote(&user_shell()),
        shell_quote(&command),
    )
}

#[cfg(target_os = "macos")]
fn script_command(script: &Path) -> String {
    format!("exec {}", script_command_without_exec(script))
}

#[cfg(target_os = "macos")]
fn script_command_without_exec(script: &Path) -> String {
    format!("/bin/sh {}", shell_quote(&script.to_string_lossy()))
}

#[cfg(not(target_os = "windows"))]
fn user_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_string()
        } else {
            "/bin/sh".to_string()
        }
    })
}

#[cfg(any(target_os = "windows", test))]
fn escape_cmd_value(value: &str) -> String {
    // cmd 批处理里 % 触发变量展开（%% 才是字面 %）；引号会截断 set "VAR=…" 的
    // 引号上下文，换行能直接注入新命令行，一律剔除。
    value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\r' | '\n'))
        .collect::<String>()
        .replace('%', "%%")
}

#[cfg(target_os = "windows")]
fn windows_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn apple_script_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn apple_script_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command_without_exec(script))
}

#[cfg(target_os = "macos")]
fn apple_script_exec_launcher_command(script: &Path) -> String {
    apple_script_quote(&script_command(script))
}

#[cfg(target_os = "macos")]
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

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn command_error_message(output: std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        "系统终端没有成功启动临时 CLI".to_string()
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthMode, ProviderInput};

    fn provider_with_liveness_model(model: &str) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: crate::models::ProviderIdentityInput {
                    name: "Relay".to_string(),
                    base_url: "https://relay.example.com".to_string(),
                    ..crate::models::ProviderIdentityInput::default()
                },
                auth: crate::models::ProviderAuth {
                    mode: AuthMode::ApiKey,
                    api_key: "sk-test".to_string(),
                    ..ProviderInput::default().auth
                },
                liveness: crate::models::ProviderLivenessInput {
                    model: model.to_string(),
                    ..ProviderInput::default().liveness
                },
                ..ProviderInput::default()
            },
            "provider/test".to_string(),
        )
    }

    #[test]
    fn effective_model_prefers_provider_model() {
        let settings = AppSettings {
            liveness_model: "gpt-5.5".to_string(),
            ..AppSettings::default()
        };
        let provider = provider_with_liveness_model("claude-opus-4-6");

        assert_eq!(effective_model(&settings, &provider), "claude-opus-4-6");
    }

    #[test]
    fn effective_model_falls_back_to_global_model() {
        let settings = AppSettings {
            liveness_model: "gpt-5.5".to_string(),
            ..AppSettings::default()
        };
        let provider = provider_with_liveness_model("");

        assert_eq!(effective_model(&settings, &provider), "gpt-5.5");
    }

    #[test]
    fn codex_args_override_provider_without_ignoring_user_config() {
        let args = cli_args(
            LivenessCliKind::Codex,
            "Relay Site",
            "https://relay.example.com/v1",
            "gpt-5.5",
            None,
        );

        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5.5"]));
        assert!(args.contains(&"model_provider=\"custom\"".to_string()));
        assert!(args.contains(&"model_providers.custom.name=\"Relay Site\"".to_string()));
        assert!(args.contains(
            &"model_providers.custom.base_url=\"https://relay.example.com/v1\"".to_string()
        ));
        assert!(args.contains(&"model_providers.custom.env_key=\"OPENAI_API_KEY\"".to_string()));
        assert!(args.contains(&"model_providers.custom.wire_api=\"responses\"".to_string()));
        assert!(args.contains(&"model_providers.custom.requires_openai_auth=true".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("balancehub")));
        assert!(!args.iter().any(|arg| arg.contains("identity.base_url")));
        assert!(!args.contains(&"--ignore-user-config".to_string()));
    }

    #[test]
    fn codex_args_escape_toml_values() {
        let args = cli_args(
            LivenessCliKind::Codex,
            "Relay \"Site\"",
            "https://relay.example.com/openai/\"tenant\"",
            "",
            None,
        );

        assert!(!args.contains(&"-m".to_string()));
        assert!(args.contains(&"model_providers.custom.name=\"Relay \\\"Site\\\"\"".to_string()));
        assert!(args.contains(
            &"model_providers.custom.base_url=\"https://relay.example.com/openai/\\\"tenant\\\"\""
                .to_string()
        ));
    }

    #[test]
    fn claude_args_include_settings_and_model_when_configured() {
        let settings_path = Path::new("/tmp/claude settings.json");

        assert_eq!(
            cli_args(
                LivenessCliKind::ClaudeCode,
                "Relay Site",
                "https://relay.example.com",
                "claude-sonnet-4-5",
                Some(settings_path)
            ),
            vec![
                "--settings".to_string(),
                "/tmp/claude settings.json".to_string(),
                "--model".to_string(),
                "claude-sonnet-4-5".to_string(),
            ]
        );
        assert_eq!(
            cli_args(
                LivenessCliKind::ClaudeCode,
                "Relay Site",
                "https://relay.example.com",
                "",
                Some(settings_path)
            ),
            vec![
                "--settings".to_string(),
                "/tmp/claude settings.json".to_string(),
            ]
        );
    }

    #[test]
    fn temporary_script_path_sanitizes_provider_id() {
        let provider = provider_with_liveness_model("");
        let path = temporary_script_path(&provider, LivenessCliKind::Codex);
        let text = path.to_string_lossy();

        assert!(text.contains("balancehub-temporary-cli-provider_test-"));
        assert!(
            text.ends_with("codex.command")
                || text.ends_with("codex.sh")
                || text.ends_with("codex.cmd")
        );
    }

    #[test]
    fn terminal_kind_serialization_matches_frontend_values() {
        assert_eq!(
            serde_json::to_string(&TemporaryCliTerminalKind::ITerm2).unwrap(),
            "\"iTerm2\""
        );
        assert_eq!(
            serde_json::to_string(&TemporaryCliTerminalKind::WezTerm).unwrap(),
            "\"wezTerm\""
        );
        assert_eq!(
            serde_json::to_string(&TemporaryCliTerminalKind::Kaku).unwrap(),
            "\"kaku\""
        );
        assert_eq!(
            serde_json::to_string(&TemporaryCliTerminalKind::WindowsTerminal).unwrap(),
            "\"windowsTerminal\""
        );
        assert_eq!(
            serde_json::to_string(&TemporaryCliTerminalKind::PowerShell).unwrap(),
            "\"powerShell\""
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn launch_runs_temporary_codex_cli_without_ui_when_custom_terminal_executes_script() {
        let root = env::temp_dir().join(format!(
            "balancehub-temporary-cli-launch-test-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let workdir = root.join("work dir");
        let bindir = root.join("fake bin");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&bindir).unwrap();

        let capture = root.join("capture.txt");
        let fake_codex = bindir.join("codex");
        fs::write(
            &fake_codex,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex 0.0.0"
  exit 0
fi
{{
  printf 'PWD=%s\n' "$(pwd)"
  printf 'OPENAI_API_KEY=%s\n' "$OPENAI_API_KEY"
  printf 'NO_COLOR=%s\n' "$NO_COLOR"
  printf 'CLICOLOR=%s\n' "$CLICOLOR"
  printf 'ARGS=%s\n' "$*"
}} > {}
"#,
                shell_quote(&capture.to_string_lossy())
            ),
        )
        .unwrap();
        set_executable(&fake_codex).unwrap();

        let settings = AppSettings {
            codex_cli_path: fake_codex.to_string_lossy().to_string(),
            temporary_cli_terminal_kind: TemporaryCliTerminalKind::Custom,
            temporary_cli_terminal_command: "BALANCEHUB_LOGIN_ENV_READY=1 NO_COLOR=1 {script}"
                .to_string(),
            liveness_model: "gpt-5.5".to_string(),
            ..AppSettings::default()
        };
        let mut provider = provider_with_liveness_model("");
        provider.identity.name = "Relay Site".to_string();

        let instance = launch(
            &settings,
            &provider,
            LivenessCliKind::Codex,
            &workdir,
            "",
            "",
        )
        .unwrap();
        assert_eq!(instance.cli_kind, LivenessCliKind::Codex);
        assert_eq!(instance.provider_id, provider.identity.id);

        let captured = fs::read_to_string(&capture).unwrap();
        let args_line = captured
            .lines()
            .find(|line| line.starts_with("ARGS="))
            .unwrap_or_default();
        assert!(captured.contains(&format!("PWD={}", workdir.to_string_lossy())));
        assert!(captured.contains("OPENAI_API_KEY=sk-test"));
        assert!(captured.lines().any(|line| line == "NO_COLOR="));
        assert!(captured.lines().any(|line| line == "CLICOLOR=1"));
        assert!(args_line.contains("-m gpt-5.5"));
        assert!(args_line.contains("model_provider=\"custom\""));
        assert!(args_line.contains("model_providers.custom.name=\"Relay Site\""));
        assert!(
            args_line.contains("model_providers.custom.base_url=\"https://relay.example.com/v1\"")
        );
        assert!(!args_line.contains("balancehub"));

        let stored = cli_runtime::instance_by_id(&instance.id).unwrap();
        assert_eq!(
            stored.status,
            crate::models::TemporaryCliInstanceStatus::Exited
        );
        let _ = fs::remove_dir_all(
            env::temp_dir()
                .join("balancehub-cli-runtime-v1")
                .join("instances")
                .join(instance.id),
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn launch_runs_temporary_claude_cli_with_settings_without_env_api_key() {
        let root = env::temp_dir().join(format!(
            "balancehub-temporary-claude-launch-test-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let workdir = root.join("work dir");
        let bindir = root.join("fake bin");
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&bindir).unwrap();

        let capture = root.join("capture.txt");
        let fake_claude = bindir.join("claude");
        fs::write(
            &fake_claude,
            format!(
                r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "claude 0.0.0"
  exit 0
fi
settings_path=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--settings" ]; then
    settings_path="$arg"
  fi
  previous="$arg"
done
{{
  printf 'PWD=%s\n' "$(pwd)"
  printf 'ANTHROPIC_API_KEY=%s\n' "$ANTHROPIC_API_KEY"
  printf 'ANTHROPIC_AUTH_TOKEN=%s\n' "$ANTHROPIC_AUTH_TOKEN"
  printf 'ANTHROPIC_BASE_URL=%s\n' "$ANTHROPIC_BASE_URL"
  printf 'NO_COLOR=%s\n' "$NO_COLOR"
  printf 'CLICOLOR=%s\n' "$CLICOLOR"
  printf 'ARGS=%s\n' "$*"
  printf 'SETTINGS_BEGIN\n'
  if [ -n "$settings_path" ]; then
    cat "$settings_path"
  fi
  printf '\nSETTINGS_END\n'
}} > {}
"#,
                shell_quote(&capture.to_string_lossy())
            ),
        )
        .unwrap();
        set_executable(&fake_claude).unwrap();

        let settings = AppSettings {
            claude_cli_path: fake_claude.to_string_lossy().to_string(),
            temporary_cli_terminal_kind: TemporaryCliTerminalKind::Custom,
            temporary_cli_terminal_command: "BALANCEHUB_LOGIN_ENV_READY=1 NO_COLOR=1 {script}"
                .to_string(),
            liveness_model: "claude-sonnet-4-5".to_string(),
            ..AppSettings::default()
        };
        let provider = provider_with_liveness_model("");

        let instance = launch(
            &settings,
            &provider,
            LivenessCliKind::ClaudeCode,
            &workdir,
            "",
            "",
        )
        .unwrap();
        assert_eq!(instance.cli_kind, LivenessCliKind::ClaudeCode);
        assert_eq!(instance.provider_id, provider.identity.id);

        let captured = fs::read_to_string(&capture).unwrap();
        let args_line = captured
            .lines()
            .find(|line| line.starts_with("ARGS="))
            .unwrap_or_default();
        assert!(captured.contains(&format!("PWD={}", workdir.to_string_lossy())));
        assert!(captured.contains("ANTHROPIC_API_KEY="));
        assert!(captured.contains("ANTHROPIC_AUTH_TOKEN="));
        assert!(captured.contains("ANTHROPIC_BASE_URL="));
        assert!(captured.lines().any(|line| line == "NO_COLOR="));
        assert!(captured.lines().any(|line| line == "CLICOLOR=1"));
        assert!(args_line.contains("--settings"));
        assert!(args_line.contains("--model claude-sonnet-4-5"));
        assert!(captured.contains("\"ANTHROPIC_AUTH_TOKEN\": \"sk-test\""));
        assert!(captured.contains("\"ANTHROPIC_BASE_URL\": \"https://relay.example.com\""));
        assert!(!captured.contains("\"ANTHROPIC_API_KEY\""));

        let stored = cli_runtime::instance_by_id(&instance.id).unwrap();
        assert_eq!(
            stored.status,
            crate::models::TemporaryCliInstanceStatus::Exited
        );
        let _ = fs::remove_dir_all(
            env::temp_dir()
                .join("balancehub-cli-runtime-v1")
                .join("instances")
                .join(instance.id),
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn custom_terminal_command_replaces_placeholders() {
        let command = custom_terminal_command(
            "open -a Warp --args {script} --cwd {workdir}",
            Path::new("/tmp/a b/run.command"),
            Path::new("/Users/me/work repo"),
            shell_quote,
        )
        .unwrap();

        assert_eq!(
            command,
            "open -a Warp --args '/tmp/a b/run.command' --cwd '/Users/me/work repo'"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn custom_terminal_command_appends_script_when_placeholder_missing() {
        let command = custom_terminal_command(
            "open -a Warp",
            Path::new("/tmp/run.command"),
            Path::new("/Users/me/work"),
            shell_quote,
        )
        .unwrap();

        assert_eq!(command, "open -a Warp '/tmp/run.command'");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn temporary_script_enters_the_interactive_login_shell_once() {
        let bootstrap = login_shell_bootstrap(Path::new("/tmp/launch test.command"));

        assert!(bootstrap.contains("BALANCEHUB_LOGIN_ENV_READY"));
        assert!(bootstrap.contains(" -lic "));
        assert!(bootstrap.contains("exec /bin/sh"));
        assert!(bootstrap.contains("/tmp/launch test.command"));
    }

    #[test]
    fn escape_cmd_value_neutralizes_batch_metacharacters() {
        assert_eq!(escape_cmd_value("sk-abc%TEMP%def"), "sk-abc%%TEMP%%def");
        assert_eq!(escape_cmd_value("sk-a\"b\r\ndel C:\\*"), "sk-abdel C:\\*",);
        assert_eq!(escape_cmd_value("sk-normal-key"), "sk-normal-key");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_launch_returns_the_created_terminal_id() {
        let script = build_macos_ghostty_applescript(Path::new("/tmp/launch test.command"));

        assert!(script.contains("set target_window to new window with configuration"));
        assert!(script.contains("set target_terminal to focused terminal of target_tab"));
        assert!(script.contains("return id of target_terminal"));
        assert!(script.contains("/bin/sh '/tmp/launch test.command'"));
        assert!(!script.contains("/bin/zsh"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_activation_targets_the_stored_terminal_id() {
        let script = build_macos_ghostty_activation_applescript("terminal-123");

        assert!(script.contains(r#"set target_id to "terminal-123""#));
        assert!(script.contains("every terminal whose id is target_id"));
        assert!(script.contains("focus item 1 of matching_terminals"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_launch_avoids_cold_start_default_window() {
        let script = build_macos_terminal_applescript(Path::new("/tmp/launch test.command"));

        assert!(script.contains(r#"set was_running to application "Terminal" is running"#));
        assert!(script.contains("launch\n        do script launcher_script"));
        assert!(script.contains("exec /bin/sh '/tmp/launch test.command'"));
        assert!(!script.contains(r#"tell application "Terminal" to do script"#));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn iterm_launch_reuses_cold_start_default_window() {
        let script = build_macos_iterm2_applescript(Path::new("/tmp/launch test.command"));

        assert!(script.contains(r#"set was_running to application "iTerm" is running"#));
        assert!(script.contains("create tab with default profile"));
        assert!(script.contains("repeat while (count of windows) = 0"));
        assert!(script.contains("write text launcher_script"));
        assert!(script.contains("exec /bin/sh '/tmp/launch test.command'"));
        assert!(!script.contains("activate\ncreate window with default profile"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn warp_launcher_uses_extensionless_sibling() {
        let launcher = warp_launcher_script_path(Path::new("/tmp/run dir/claude.command"));

        assert_eq!(launcher, PathBuf::from("/tmp/run dir/warp-launcher"));
    }
}
