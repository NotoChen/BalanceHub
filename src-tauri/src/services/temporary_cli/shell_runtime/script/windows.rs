use super::LaunchScriptInput;
#[cfg(target_os = "windows")]
use super::{
    runtime_instance_id_from_status_path, temporary_windows_launch_payload_path,
    write_auxiliary_file,
};
#[cfg(target_os = "windows")]
use crate::services::temporary_cli::shell_runtime::environment::capture_shell_snapshot;
use crate::{
    network::ProxyEnvironment,
    platform::paths::{compatible_path, windows_shell_path},
    services::{
        agent_cli::contracts::TemporaryLaunchPlan,
        temporary_cli::shell_runtime::environment::ShellEnvironmentSnapshot,
    },
};
#[cfg(target_os = "windows")]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::Path,
};

#[cfg(target_os = "windows")]
pub(in crate::services::temporary_cli) fn write_launch_script(
    input: &LaunchScriptInput<'_>,
) -> Result<(), String> {
    let launch_payload_path = temporary_windows_launch_payload_path(input.script);
    let text = render_launch_script(input, &launch_payload_path)?;
    write_auxiliary_file(
        input.auxiliary_file_path,
        input.plan.auxiliary_file_content.as_deref(),
    )?;
    let shell_snapshot = capture_shell_snapshot();
    let launch_payload = windows_launch_payload(WindowsLaunchPayloadInput {
        cli_path: input.cli_path,
        cli_command_name: input.cli_command_name,
        plan: input.plan,
        proxy_environment: input.proxy_environment,
        shell_snapshot: &shell_snapshot,
        prefer_shell_cli: input.prefer_shell_cli,
    });
    let mut launch_payload = launch_payload;
    if let Some(instance_id) = runtime_instance_id_from_status_path(input.status_path) {
        if let Some(set_env) = launch_payload
            .get_mut("setEnv")
            .and_then(serde_json::Value::as_object_mut)
        {
            set_env.insert(
                "BALANCEHUB_CLI_INSTANCE_ID".to_string(),
                serde_json::Value::String(instance_id),
            );
        }
    }
    let launch_payload_text = serde_json::to_string_pretty(&launch_payload)
        .map_err(|err| format!("生成 Windows 临时 CLI 启动参数失败: {err}"))?;
    fs::write(&launch_payload_path, launch_payload_text)
        .map_err(|err| format!("写入 Windows 临时 CLI 启动参数失败: {err}"))?;
    restrict_to_owner(&launch_payload_path)?;
    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))
}

fn render_launch_script(
    input: &LaunchScriptInput<'_>,
    launch_payload_path: &Path,
) -> Result<String, String> {
    windows_shell_path(input.script)?;
    let script_dir = input
        .script
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(env::temp_dir);
    let cleanup_settings = input
        .auxiliary_file_path
        .map(|path| escape_cmd_path(path).map(|path| format!("del \"{path}\" 2>nul\n")))
        .transpose()?
        .unwrap_or_default();
    let workdir = escape_cmd_path(input.workdir)?;
    let network_directory = workdir.starts_with(r"\\");
    // `exit /b` still consults the batch file. Leave the batch context before
    // deleting it; expand cleanup arguments first and return the original code
    // through a child CMD so an interactive parent shell stays open.
    let text = format!(
        r#"@echo off
setlocal EnableExtensions DisableDelayedExpansion
chcp 65001 >nul 2>nul
set "BH_STATUS_FILE={status_path}"
set "BH_LAUNCH_FILE={launch_payload_path}"
set "BH_POWERSHELL="
where pwsh.exe >nul 2>nul && set "BH_POWERSHELL=pwsh.exe"
if not defined BH_POWERSHELL (
where powershell.exe >nul 2>nul && set "BH_POWERSHELL=powershell.exe"
)
set "BH_PUSHED="
{change_directory} "{workdir}"
if errorlevel 1 goto BH_WORKDIR_ERROR
{remember_pushd}if not defined BH_POWERSHELL goto BH_POWERSHELL_ERROR
{color_block}%BH_POWERSHELL% -NoProfile -ExecutionPolicy Bypass -Command "{powershell_launch_command}"
set "BH_EXIT_CODE=%ERRORLEVEL%"
goto BH_FINISH
:BH_WORKDIR_ERROR
set "BH_EXIT_CODE=%ERRORLEVEL%"
goto BH_FINISH
:BH_POWERSHELL_ERROR
set "BH_EXIT_CODE=9009"
:BH_FINISH
set "BH_ENDED=null"
if defined BH_POWERSHELL for /f %%T in ('%BH_POWERSHELL% -NoProfile -Command "[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()"') do set "BH_ENDED=%%T"
call :BH_WRITE_STATUS exited null %BH_ENDED% %BH_EXIT_CODE%
if defined BH_PUSHED popd
del "{launch_payload_path}" 2>nul
{cleanup_settings}(goto) 2>nul & del "%~f0" 2>nul & rmdir "{script_dir}" 2>nul & cmd /D /C exit %BH_EXIT_CODE%
:BH_WRITE_STATUS
set "BH_TMP=%BH_STATUS_FILE%.tmp.%RANDOM%"
> "%BH_TMP%" echo {{"status":"%~1","pid":%~2,"endedAt":%~3,"exitCode":%~4}}
move /Y "%BH_TMP%" "%BH_STATUS_FILE%" >nul
exit /b 0
"#,
        status_path = escape_cmd_path(input.status_path)?,
        launch_payload_path = escape_cmd_path(launch_payload_path)?,
        change_directory = if network_directory { "pushd" } else { "cd /d" },
        remember_pushd = if network_directory {
            "set \"BH_PUSHED=1\"\n"
        } else {
            ""
        },
        color_block = windows_color_block(),
        powershell_launch_command = WINDOWS_LAUNCH_PAYLOAD_COMMAND,
        script_dir = escape_cmd_path(&script_dir)?,
    );
    Ok(text.replace('\n', "\r\n"))
}

fn escape_cmd_path(path: &Path) -> Result<String, String> {
    Ok(escape_cmd_value(
        &windows_shell_path(path)?.to_string_lossy(),
    ))
}

pub(in crate::services::temporary_cli) const WINDOWS_LAUNCH_PAYLOAD_COMMAND: &str = concat!(
    "$ErrorActionPreference = 'Stop'; ",
    "$BH_Launch = Get-Content -Raw -Encoding UTF8 -LiteralPath $env:BH_LAUNCH_FILE | ConvertFrom-Json; ",
    // This PowerShell stays alive for the CLI invocation. A `for /f` probe
    // instead observes a short-lived intermediate CMD and records a stale PID.
    "if ($env:BH_STATUS_FILE) { ",
    "$BH_StatusTemp = $env:BH_STATUS_FILE + '.tmp.' + [Guid]::NewGuid().ToString('N'); ",
    "$BH_StatusJson = @{ status = 'running'; pid = $PID; endedAt = $null; exitCode = $null } | ConvertTo-Json -Compress; ",
    "[System.IO.File]::WriteAllText($BH_StatusTemp, $BH_StatusJson, [System.Text.UTF8Encoding]::new($false)); ",
    "Move-Item -LiteralPath $BH_StatusTemp -Destination $env:BH_STATUS_FILE -Force }; ",
    "foreach ($BH_Name in @($BH_Launch.removeEnv)) { ",
    "Remove-Item -LiteralPath ('Env:' + [string]$BH_Name) -ErrorAction SilentlyContinue }; ",
    "foreach ($BH_Entry in @($BH_Launch.setEnv.PSObject.Properties)) { ",
    "Set-Item -LiteralPath ('Env:' + $BH_Entry.Name) -Value ([string]$BH_Entry.Value) }; ",
    // The payload is executed with -NoProfile; restore registered Agent CLI names
    // so a user's profile wrapper does not silently disappear on Windows.
    "if ($null -ne $BH_Launch.functions) { foreach ($BH_Entry in @($BH_Launch.functions.PSObject.Properties)) { ",
    "Set-Item -LiteralPath ('Function:\\' + [string]$BH_Entry.Name) -Value ([scriptblock]::Create([string]$BH_Entry.Value)) -Force } }; ",
    "if ($null -ne $BH_Launch.aliases) { foreach ($BH_Entry in @($BH_Launch.aliases.PSObject.Properties)) { ",
    "Set-Alias -Name ([string]$BH_Entry.Name) -Value ([string]$BH_Entry.Value) -Scope Local -Force } }; ",
    "$BH_CommandInfo = Get-Command -Name ([string]$BH_Launch.cliCommandName) -ErrorAction SilentlyContinue | Select-Object -First 1; ",
    "$BH_Arguments = @($BH_Launch.args | ForEach-Object { [string]$_ }); ",
    "if ($BH_Launch.preferShellCli -and $null -ne $BH_CommandInfo -and @('Alias', 'Function', 'Filter') -contains [string]$BH_CommandInfo.CommandType) { & ([string]$BH_Launch.cliCommandName) @BH_Arguments } else { & ([string]$BH_Launch.cliPath) @BH_Arguments }; ",
    "if ($null -eq $LASTEXITCODE) { exit 0 } else { exit $LASTEXITCODE }"
);

pub(in crate::services::temporary_cli) struct WindowsLaunchPayloadInput<'a> {
    pub(in crate::services::temporary_cli) cli_path: &'a str,
    pub(in crate::services::temporary_cli) cli_command_name: &'a str,
    pub(in crate::services::temporary_cli) plan: &'a TemporaryLaunchPlan,
    pub(in crate::services::temporary_cli) proxy_environment: &'a ProxyEnvironment,
    pub(in crate::services::temporary_cli) shell_snapshot: &'a ShellEnvironmentSnapshot,
    pub(in crate::services::temporary_cli) prefer_shell_cli: bool,
}

pub(in crate::services::temporary_cli) fn windows_launch_payload(
    input: WindowsLaunchPayloadInput<'_>,
) -> serde_json::Value {
    let WindowsLaunchPayloadInput {
        cli_path,
        cli_command_name,
        plan,
        proxy_environment,
        shell_snapshot,
        prefer_shell_cli,
    } = input;
    let mut remove_env = proxy_environment
        .removed_names()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut set_env = BTreeMap::new();
    for (name, value) in &shell_snapshot.variables {
        remove_environment_name(&mut set_env, name);
        set_env.insert(name.clone(), value.clone());
    }
    for name in proxy_environment.removed_names() {
        remove_environment_name(&mut set_env, name);
    }
    for (name, value) in proxy_environment.variables() {
        // Windows environment names and PowerShell JSON object properties are
        // case-insensitive. Keep one spelling when upper/lower proxy pairs
        // from the common resolver are projected into the launch payload.
        let name = name.to_ascii_uppercase();
        remove_environment_name(&mut set_env, &name);
        set_env.insert(name, value.to_string());
    }

    for name in plan.environment.removed_names() {
        remove_environment_name(&mut set_env, name);
        remove_env.insert(name.to_string());
    }
    for (name, value) in plan.environment.set_values() {
        remove_environment_name(&mut set_env, name);
        remove_env.remove(name);
        set_env.insert(name.to_string(), value.to_string());
    }

    serde_json::json!({
        "cliPath": compatible_path(Path::new(cli_path)).to_string_lossy(),
        "cliCommandName": cli_command_name,
        "preferShellCli": prefer_shell_cli,
        "args": plan.args,
        "removeEnv": remove_env,
        "setEnv": set_env,
        "aliases": shell_snapshot.aliases,
        "functions": shell_snapshot.functions,
    })
}

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

fn windows_color_block() -> &'static str {
    "set NO_COLOR=\nset CLICOLOR=1\nif not defined TERM set \"TERM=xterm-256color\"\n"
}

#[cfg(target_os = "windows")]
pub(super) fn restrict_to_owner(_path: &Path) -> Result<(), String> {
    // %TEMP% 位于用户目录下，默认 ACL 已限制为本用户可见。
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn preview_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(in crate::services::temporary_cli) fn escape_cmd_value(value: &str) -> String {
    // cmd 批处理里 % 触发变量展开（%% 才是字面 %）；引号会截断 set "VAR=…" 的
    // 引号上下文，换行能直接注入新命令行，一律剔除。
    value
        .chars()
        .filter(|ch| !matches!(ch, '"' | '\r' | '\n'))
        .collect::<String>()
        .replace('%', "%%")
}

#[cfg(test)]
mod tests;
