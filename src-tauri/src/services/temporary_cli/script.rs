#[cfg(target_os = "windows")]
use super::environment::capture_shell_snapshot;
#[cfg(any(target_os = "windows", test))]
use super::environment::ShellEnvironmentSnapshot;
#[cfg(not(target_os = "windows"))]
use crate::services::liveness::LivenessRunner;
use crate::{
    models::{AppSettings, LivenessCliKind, Provider, TemporaryCliSessionMode},
    network::ProxyEnvironment,
    util::unix_millis as now_millis,
};
#[cfg(any(target_os = "windows", test))]
use std::collections::{BTreeMap, BTreeSet};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub(super) fn cleanup_launch_files(script: &Path, cli_kind: LivenessCliKind) {
    let _ = fs::remove_file(script);
    let _ = fs::remove_file(temporary_windows_launch_payload_path(script));
    if let Some(settings_path) = temporary_claude_settings_path(script, cli_kind) {
        let _ = fs::remove_file(settings_path);
    }
    if let Some(parent) = script.parent() {
        let _ = fs::remove_dir(parent);
    }
}

pub(super) fn effective_model(settings: &AppSettings, provider: &Provider) -> String {
    let provider_model = provider.liveness.model.trim();
    if provider_model.is_empty() {
        settings.liveness_model.trim().to_string()
    } else {
        provider_model.to_string()
    }
}

pub(super) fn temporary_script_path(provider: &Provider, cli_kind: LivenessCliKind) -> PathBuf {
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

pub(super) fn preview_claude_settings_path(cli_kind: LivenessCliKind) -> Option<PathBuf> {
    matches!(cli_kind, LivenessCliKind::ClaudeCode)
        .then(|| PathBuf::from("<temporary-claude-settings.json>"))
}

fn temporary_windows_launch_payload_path(script: &Path) -> PathBuf {
    script
        .parent()
        .map(|parent| parent.join("launch.json"))
        .unwrap_or_else(|| env::temp_dir().join("launch.json"))
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

pub(super) struct LaunchScriptInput<'a> {
    pub(super) script: &'a Path,
    pub(super) cli_kind: LivenessCliKind,
    pub(super) cli_path: &'a str,
    pub(super) workdir: &'a Path,
    pub(super) provider_name: &'a str,
    pub(super) api_key: &'a str,
    pub(super) base_url: &'a str,
    pub(super) model: &'a str,
    pub(super) session_name: &'a str,
    pub(super) resume_id: &'a str,
    pub(super) session_mode: TemporaryCliSessionMode,
    pub(super) status_path: &'a Path,
    pub(super) proxy_environment: &'a ProxyEnvironment,
}

#[cfg(not(target_os = "windows"))]
pub(super) fn write_launch_script(input: &LaunchScriptInput<'_>) -> Result<(), String> {
    let claude_settings_path = temporary_claude_settings_path(input.script, input.cli_kind);
    if let Some(path) = &claude_settings_path {
        write_claude_settings(path, input.api_key, input.base_url)?;
    }
    let args = cli_args(
        input.cli_kind,
        input.provider_name,
        input.base_url,
        input.model,
        input.session_name,
        input.session_mode,
        claude_settings_path.as_deref(),
    );
    let mut args = args;
    insert_resume_id(&mut args, input.cli_kind, input.resume_id);
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
    let script_path = shell_quote(&input.script.to_string_lossy());
    let login_shell_bootstrap = login_shell_bootstrap(input.script);
    let cli_invocation = unix_cli_invocation(input.cli_kind, input.cli_path, &args);
    let proxy_block = unix_proxy_block(input.proxy_environment);

    let text = format!(
        r#"#!/bin/sh
set -u
bh_script_path={script_path}
{login_shell_bootstrap}bh_status_file={status_path}
bh_write_status() {{
  bh_tmp="$bh_status_file.tmp.$$"
  printf '{{"status":"%s","pid":%s,"endedAt":%s,"exitCode":%s}}\n' "$1" "$2" "$3" "$4" > "$bh_tmp"
  mv -f "$bh_tmp" "$bh_status_file"
}}
bh_now_ms() {{
  echo $(( $(date +%s) * 1000 ))
}}
cd {workdir}
status=$?
if [ "$status" -ne 0 ]; then
  bh_write_status exited null "$(bh_now_ms)" "$status"
  exit "$status"
fi
{path_export}{color_block}{proxy_block}{auth_block}bh_write_status running "$$" null null
{cli_invocation}
status=$?
bh_write_status exited null "$(bh_now_ms)" "$status"
rm -f "$bh_script_path"
{cleanup_settings}rmdir {script_dir} 2>/dev/null || true
exit "$status"
"#,
        status_path = shell_quote(&input.status_path.to_string_lossy()),
        login_shell_bootstrap = login_shell_bootstrap,
        script_path = script_path,
        workdir = shell_quote(&input.workdir.to_string_lossy()),
        color_block = unix_color_block(),
        proxy_block = proxy_block,
        cli_invocation = cli_invocation,
        script_dir = shell_quote(&script_dir.to_string_lossy()),
    );

    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))?;
    set_executable(input.script)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn write_launch_script(input: &LaunchScriptInput<'_>) -> Result<(), String> {
    let claude_settings_path = temporary_claude_settings_path(input.script, input.cli_kind);
    if let Some(path) = &claude_settings_path {
        write_claude_settings(path, input.api_key, input.base_url)?;
    }
    let args = cli_args(
        input.cli_kind,
        input.provider_name,
        input.base_url,
        input.model,
        input.session_name,
        input.session_mode,
        claude_settings_path.as_deref(),
    );
    let mut args = args;
    insert_resume_id(&mut args, input.cli_kind, input.resume_id);
    let launch_payload_path = temporary_windows_launch_payload_path(input.script);
    let shell_snapshot = capture_shell_snapshot();
    let launch_payload = windows_launch_payload(
        input.cli_kind,
        input.cli_path,
        &args,
        input.api_key,
        input.proxy_environment,
        &shell_snapshot,
    );
    let launch_payload_text = serde_json::to_string_pretty(&launch_payload)
        .map_err(|err| format!("生成 Windows 临时 CLI 启动参数失败: {err}"))?;
    fs::write(&launch_payload_path, launch_payload_text)
        .map_err(|err| format!("写入 Windows 临时 CLI 启动参数失败: {err}"))?;
    restrict_to_owner(&launch_payload_path)?;
    let script_dir = input
        .script
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(env::temp_dir);
    let text = format!(
        "@echo off\r\nsetlocal\r\nset \"BH_STATUS_FILE={status_path}\"\r\nset \"BH_LAUNCH_FILE={launch_payload_path}\"\r\nset \"BH_POWERSHELL=\"\r\nwhere pwsh.exe >nul 2>nul && set \"BH_POWERSHELL=pwsh.exe\"\r\nif not defined BH_POWERSHELL where powershell.exe >nul 2>nul && set \"BH_POWERSHELL=powershell.exe\"\r\nset \"BH_PID=null\"\r\nif defined BH_POWERSHELL for /f %%P in ('%BH_POWERSHELL% -NoProfile -Command \"(Get-CimInstance Win32_Process ^| Where-Object ProcessId -eq $PID).ParentProcessId\"') do set \"BH_PID=%%P\"\r\ncd /d \"{workdir}\"\r\nif errorlevel 1 goto BH_WORKDIR_ERROR\r\nif not defined BH_POWERSHELL goto BH_POWERSHELL_ERROR\r\n{color_block}call :BH_WRITE_STATUS running %BH_PID% null null\r\n%BH_POWERSHELL% -NoProfile -ExecutionPolicy Bypass -Command \"{powershell_launch_command}\"\r\nset STATUS=%ERRORLEVEL%\r\ngoto BH_FINISH\r\n:BH_WORKDIR_ERROR\r\nset STATUS=%ERRORLEVEL%\r\ngoto BH_FINISH\r\n:BH_POWERSHELL_ERROR\r\nset STATUS=9009\r\n:BH_FINISH\r\nset \"BH_ENDED=null\"\r\nif defined BH_POWERSHELL for /f %%T in ('%BH_POWERSHELL% -NoProfile -Command \"[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()\"') do set \"BH_ENDED=%%T\"\r\ncall :BH_WRITE_STATUS exited null %BH_ENDED% %STATUS%\r\ndel \"{launch_payload_path}\" 2>nul\r\n{cleanup_settings}del \"%~f0\"\r\nrmdir \"{script_dir}\" 2>nul\r\nexit /b %STATUS%\r\n:BH_WRITE_STATUS\r\nset \"BH_TMP=%BH_STATUS_FILE%.tmp.%RANDOM%\"\r\n> \"%BH_TMP%\" echo {{\"status\":\"%~1\",\"pid\":%~2,\"endedAt\":%~3,\"exitCode\":%~4}}\r\nmove /Y \"%BH_TMP%\" \"%BH_STATUS_FILE%\" >nul\r\nexit /b 0\r\n",
        status_path = escape_cmd_value(&input.status_path.display().to_string()),
        launch_payload_path = escape_cmd_value(&launch_payload_path.display().to_string()),
        workdir = escape_cmd_value(&input.workdir.display().to_string()),
        color_block = windows_color_block(),
        powershell_launch_command = WINDOWS_LAUNCH_PAYLOAD_COMMAND,
        script_dir = escape_cmd_value(&script_dir.display().to_string()),
        cleanup_settings = claude_settings_path
            .as_ref()
            .map(|path| format!("del \"{}\" 2>nul\r\n", escape_cmd_value(&path.display().to_string())))
            .unwrap_or_default(),
    );

    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))
}

#[cfg(any(target_os = "windows", test))]
pub(super) const WINDOWS_LAUNCH_PAYLOAD_COMMAND: &str = concat!(
    "$ErrorActionPreference = 'Stop'; ",
    "$launch = Get-Content -Raw -LiteralPath $env:BH_LAUNCH_FILE | ConvertFrom-Json; ",
    "foreach ($name in @($launch.removeEnv)) { ",
    "Remove-Item -LiteralPath ('Env:' + [string]$name) -ErrorAction SilentlyContinue }; ",
    "foreach ($entry in @($launch.setEnv.PSObject.Properties)) { ",
    "Set-Item -LiteralPath ('Env:' + $entry.Name) -Value ([string]$entry.Value) }; ",
    // The payload is executed with -NoProfile; restore only the two CLI names
    // so a user's profile wrapper does not silently disappear on Windows.
    "if ($null -ne $launch.functions) { foreach ($entry in @($launch.functions.PSObject.Properties)) { ",
    "Set-Item -LiteralPath ('Function:\\' + [string]$entry.Name) -Value ([scriptblock]::Create([string]$entry.Value)) -Force } }; ",
    "if ($null -ne $launch.aliases) { foreach ($entry in @($launch.aliases.PSObject.Properties)) { ",
    "Set-Alias -Name ([string]$entry.Name) -Value ([string]$entry.Value) -Scope Local -Force } }; ",
    "$commandInfo = Get-Command -Name ([string]$launch.cliCommandName) -ErrorAction SilentlyContinue | Select-Object -First 1; ",
    "$arguments = @($launch.args | ForEach-Object { [string]$_ }); ",
    "if ($null -ne $commandInfo -and @('Alias', 'Function', 'Filter') -contains [string]$commandInfo.CommandType) { & ([string]$launch.cliCommandName) @arguments } else { & ([string]$launch.cliPath) @arguments }; ",
    "if ($null -eq $LASTEXITCODE) { exit 0 } else { exit $LASTEXITCODE }"
);

#[cfg(any(target_os = "windows", test))]
pub(super) fn windows_launch_payload(
    cli_kind: LivenessCliKind,
    cli_path: &str,
    args: &[String],
    api_key: &str,
    proxy_environment: &ProxyEnvironment,
    shell_snapshot: &ShellEnvironmentSnapshot,
) -> serde_json::Value {
    let mut remove_env = proxy_environment
        .removed_names()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut set_env = shell_snapshot.variables.clone();
    let proxy_variables = proxy_environment
        .variables()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect::<Vec<_>>();
    for (name, _) in &proxy_variables {
        remove_environment_name(&mut set_env, name);
    }
    for (name, value) in proxy_variables {
        set_env.insert(name, value);
    }

    match cli_kind {
        LivenessCliKind::Codex => {
            remove_environment_name(&mut set_env, "OPENAI_API_KEY");
            set_env.insert("OPENAI_API_KEY".to_string(), api_key.to_string());
            remove_env.insert("CODEX_API_KEY".to_string());
            remove_env.insert("CODEX_ACCESS_TOKEN".to_string());
        }
        LivenessCliKind::ClaudeCode => {
            remove_env.insert("ANTHROPIC_API_KEY".to_string());
            remove_env.insert("ANTHROPIC_AUTH_TOKEN".to_string());
            remove_env.insert("ANTHROPIC_BASE_URL".to_string());
        }
    }

    serde_json::json!({
        "cliPath": cli_path,
        "cliCommandName": match cli_kind {
            LivenessCliKind::Codex => "codex",
            LivenessCliKind::ClaudeCode => "claude",
        },
        "args": args,
        "removeEnv": remove_env,
        "setEnv": set_env,
        "aliases": shell_snapshot.aliases,
        "functions": shell_snapshot.functions,
    })
}

#[cfg(any(target_os = "windows", test))]
fn remove_environment_name(target: &mut BTreeMap<String, String>, name: &str) {
    let matching = target
        .keys()
        .filter(|existing| existing.eq_ignore_ascii_case(name))
        .cloned()
        .collect::<Vec<_>>();
    for existing in matching {
        target.remove(&existing);
    }
}

#[cfg(not(target_os = "windows"))]
fn unix_color_block() -> &'static str {
    "unset NO_COLOR\nexport CLICOLOR=1\nif [ \"${TERM:-dumb}\" = \"dumb\" ]; then export TERM=xterm-256color; fi\n"
}

#[cfg(target_os = "windows")]
fn windows_color_block() -> &'static str {
    "set NO_COLOR=\r\nset CLICOLOR=1\r\nif not defined TERM set \"TERM=xterm-256color\"\r\n"
}

#[cfg(not(target_os = "windows"))]
fn unix_proxy_block(environment: &ProxyEnvironment) -> String {
    if environment.inherits() {
        return String::new();
    }

    let removed = environment.removed_names().collect::<Vec<_>>();
    let mut block = String::new();
    if !removed.is_empty() {
        block.push_str("unset ");
        block.push_str(&removed.join(" "));
        block.push('\n');
    }
    for (name, value) in environment.variables() {
        block.push_str("export ");
        block.push_str(name);
        block.push('=');
        block.push_str(&shell_quote(value));
        block.push('\n');
    }
    block
}

pub(super) fn claude_settings_content(api_key: &str, base_url: &str) -> Result<String, String> {
    let config = serde_json::json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": api_key.trim(),
            "ANTHROPIC_BASE_URL": base_url.trim(),
        }
    });
    serde_json::to_string_pretty(&config).map_err(|err| format!("生成 Claude 配置失败: {err}"))
}

fn write_claude_settings(path: &Path, api_key: &str, base_url: &str) -> Result<(), String> {
    let text = claude_settings_content(api_key, base_url)?;
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

pub(super) fn cli_args(
    cli_kind: LivenessCliKind,
    provider_name: &str,
    base_url: &str,
    model: &str,
    session_name: &str,
    session_mode: TemporaryCliSessionMode,
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
            match session_mode {
                TemporaryCliSessionMode::New => {}
                TemporaryCliSessionMode::History => args.push("resume".to_string()),
            }
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
            if matches!(session_mode, TemporaryCliSessionMode::New)
                && !session_name.trim().is_empty()
            {
                args.extend(["--name".to_string(), session_name.trim().to_string()]);
            }
            match session_mode {
                TemporaryCliSessionMode::New => {}
                TemporaryCliSessionMode::History => args.push("--resume".to_string()),
            }
            args
        }
    }
}

pub(super) fn insert_resume_id(args: &mut Vec<String>, cli_kind: LivenessCliKind, resume_id: &str) {
    if resume_id.trim().is_empty() {
        return;
    }
    let subcommand = match cli_kind {
        LivenessCliKind::Codex => "resume",
        LivenessCliKind::ClaudeCode => "--resume",
    };
    if let Some(index) = args.iter().position(|arg| arg == subcommand) {
        args.insert(index + 1, resume_id.trim().to_string());
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("读取临时脚本权限失败: {err}"))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).map_err(|err| format!("设置临时脚本权限失败: {err}"))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(target_os = "windows"))]
fn preview_quote(value: &str) -> String {
    shell_quote(value)
}

#[cfg(target_os = "windows")]
fn preview_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 生成确认弹窗中的 CLI 调用。临时 shell/cmd 包装脚本仍由启动链路负责，
/// 这里只展示用户真正关心的可执行文件、参数和显式环境变量。
pub(super) fn format_cli_command(
    cli_kind: LivenessCliKind,
    cli_path: &str,
    args: &[String],
    api_key: &str,
    environment: &[(String, String)],
) -> String {
    #[cfg(not(target_os = "windows"))]
    {
        let mut parts = Vec::new();
        if matches!(cli_kind, LivenessCliKind::Codex) {
            parts.push(format!("OPENAI_API_KEY={}", preview_quote(api_key)));
        }
        parts.extend(
            environment
                .iter()
                .map(|(name, value)| format!("{name}={}", preview_quote(value))),
        );
        parts.push(preview_quote(cli_path));
        parts.extend(args.iter().map(|arg| preview_quote(arg)));
        parts.join(" ")
    }

    #[cfg(target_os = "windows")]
    {
        let mut assignments = Vec::new();
        if matches!(cli_kind, LivenessCliKind::Codex) {
            assignments.push(format!("$env:OPENAI_API_KEY={}", preview_quote(api_key)));
        }
        assignments.extend(
            environment
                .iter()
                .map(|(name, value)| format!("$env:{name}={}", preview_quote(value))),
        );
        let command = std::iter::once(preview_quote(cli_path))
            .chain(args.iter().map(|arg| preview_quote(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        if assignments.is_empty() {
            format!("& {command}")
        } else {
            format!("{}; & {command}", assignments.join("; "))
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn login_shell_bootstrap(script: &Path) -> String {
    let script = shell_quote(&script.to_string_lossy());
    // POSIX-compatible shells can source the launcher in the same process, so
    // aliases/functions from the interactive startup files remain available.
    // fish and unknown shells still export their environment through a child
    // /bin/sh because the launcher itself is POSIX syntax.
    let command = if shell_supports_posix_source(&user_shell()) {
        format!(". {script}")
    } else {
        format!("exec /bin/sh {script}")
    };
    format!(
        "if [ \"${{BALANCEHUB_LOGIN_ENV_READY:-}}\" != \"1\" ]; then\n  export BALANCEHUB_LOGIN_ENV_READY=1\n  exec {} -lic {}\nfi\nunset BALANCEHUB_LOGIN_ENV_READY\n",
        shell_quote(&user_shell()),
        shell_quote(&command),
    )
}

#[cfg(not(target_os = "windows"))]
pub(super) fn shell_supports_posix_source(shell: &str) -> bool {
    let name = Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "ash" | "bash" | "busybox" | "dash" | "ksh" | "mksh" | "sh" | "zsh"
    )
}

#[cfg(not(target_os = "windows"))]
pub(super) fn unix_cli_invocation(
    cli_kind: LivenessCliKind,
    cli_path: &str,
    args: &[String],
) -> String {
    let command_name = match cli_kind {
        LivenessCliKind::Codex => "codex",
        LivenessCliKind::ClaudeCode => "claude",
    };
    let args = args
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "bh_use_shell_cli=0\nif alias {command_name} >/dev/null 2>&1; then\n  bh_use_shell_cli=1\nelif command -v typeset >/dev/null 2>&1 && typeset -f {command_name} >/dev/null 2>&1; then\n  bh_use_shell_cli=1\nfi\nif [ \"$bh_use_shell_cli\" -eq 1 ]; then\n  {command_name} {args}\nelse\n  {cli_path} {args}\nfi",
        command_name = command_name,
        cli_path = shell_quote(cli_path),
        args = args,
    )
}

#[cfg(target_os = "macos")]
pub(super) fn script_command(script: &Path) -> String {
    format!("exec {}", script_command_without_exec(script))
}

#[cfg(target_os = "macos")]
pub(super) fn script_command_without_exec(script: &Path) -> String {
    format!("/bin/sh {}", shell_quote(&script.to_string_lossy()))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn user_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_string()
        } else {
            "/bin/sh".to_string()
        }
    })
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn escape_cmd_value(value: &str) -> String {
    // cmd 批处理里 % 触发变量展开（%% 才是字面 %）；引号会截断 set "VAR=…" 的
    // 引号上下文，换行能直接注入新命令行，一律剔除。
    value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\r' | '\n'))
        .collect::<String>()
        .replace('%', "%%")
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
