#[cfg(target_os = "windows")]
use super::{temporary_windows_launch_payload_path, write_auxiliary_file, LaunchScriptInput};
#[cfg(target_os = "windows")]
use crate::services::temporary_cli::shell_runtime::environment::capture_shell_snapshot;
use crate::{
    network::ProxyEnvironment,
    services::{
        agent_cli::contracts::TemporaryLaunchPlan,
        temporary_cli::shell_runtime::environment::ShellEnvironmentSnapshot,
    },
};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "windows")]
use std::{env, fs, path::Path};

#[cfg(target_os = "windows")]
pub(in crate::services::temporary_cli) fn write_launch_script(
    input: &LaunchScriptInput<'_>,
) -> Result<(), String> {
    write_auxiliary_file(
        input.auxiliary_file_path,
        input.plan.auxiliary_file_content.as_deref(),
    )?;
    let launch_payload_path = temporary_windows_launch_payload_path(input.script);
    let shell_snapshot = capture_shell_snapshot();
    let launch_payload = windows_launch_payload(WindowsLaunchPayloadInput {
        cli_path: input.cli_path,
        cli_command_name: input.cli_command_name,
        plan: input.plan,
        proxy_environment: input.proxy_environment,
        shell_snapshot: &shell_snapshot,
    });
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
        "@echo off\r\nsetlocal\r\nset \"BH_STATUS_FILE={status_path}\"\r\nset \"BH_LAUNCH_FILE={launch_payload_path}\"\r\nset \"BH_POWERSHELL=\"\r\nwhere pwsh.exe >nul 2>nul && set \"BH_POWERSHELL=pwsh.exe\"\r\nif not defined BH_POWERSHELL where powershell.exe >nul 2>nul && set \"BH_POWERSHELL=powershell.exe\"\r\nset \"BH_PID=null\"\r\nif defined BH_POWERSHELL for /f %%P in ('%BH_POWERSHELL% -NoProfile -Command \"(Get-CimInstance Win32_Process ^| Where-Object ProcessId -eq $PID).ParentProcessId\"') do set \"BH_PID=%%P\"\r\ncd /d \"{workdir}\"\r\nif errorlevel 1 goto BH_WORKDIR_ERROR\r\nif not defined BH_POWERSHELL goto BH_POWERSHELL_ERROR\r\n{color_block}call :BH_WRITE_STATUS running %BH_PID% null null\r\n%BH_POWERSHELL% -NoProfile -ExecutionPolicy Bypass -Command \"{powershell_launch_command}\"\r\nset \"BH_EXIT_CODE=%ERRORLEVEL%\"\r\ngoto BH_FINISH\r\n:BH_WORKDIR_ERROR\r\nset \"BH_EXIT_CODE=%ERRORLEVEL%\"\r\ngoto BH_FINISH\r\n:BH_POWERSHELL_ERROR\r\nset \"BH_EXIT_CODE=9009\"\r\n:BH_FINISH\r\nset \"BH_ENDED=null\"\r\nif defined BH_POWERSHELL for /f %%T in ('%BH_POWERSHELL% -NoProfile -Command \"[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()\"') do set \"BH_ENDED=%%T\"\r\ncall :BH_WRITE_STATUS exited null %BH_ENDED% %BH_EXIT_CODE%\r\ndel \"{launch_payload_path}\" 2>nul\r\n{cleanup_settings}del \"%~f0\"\r\nrmdir \"{script_dir}\" 2>nul\r\nexit /b %BH_EXIT_CODE%\r\n:BH_WRITE_STATUS\r\nset \"BH_TMP=%BH_STATUS_FILE%.tmp.%RANDOM%\"\r\n> \"%BH_TMP%\" echo {{\"status\":\"%~1\",\"pid\":%~2,\"endedAt\":%~3,\"exitCode\":%~4}}\r\nmove /Y \"%BH_TMP%\" \"%BH_STATUS_FILE%\" >nul\r\nexit /b 0\r\n",
        status_path = escape_cmd_value(&input.status_path.display().to_string()),
        launch_payload_path = escape_cmd_value(&launch_payload_path.display().to_string()),
        workdir = escape_cmd_value(&input.workdir.display().to_string()),
        color_block = windows_color_block(),
        powershell_launch_command = WINDOWS_LAUNCH_PAYLOAD_COMMAND,
        script_dir = escape_cmd_value(&script_dir.display().to_string()),
        cleanup_settings = input
            .auxiliary_file_path
            .map(|path| format!(
                "del \"{}\" 2>nul\r\n",
                escape_cmd_value(&path.display().to_string())
            ))
            .unwrap_or_default(),
    );

    fs::write(input.script, text).map_err(|err| format!("写入临时 CLI 启动脚本失败: {err}"))
}

pub(in crate::services::temporary_cli) const WINDOWS_LAUNCH_PAYLOAD_COMMAND: &str = concat!(
    "$ErrorActionPreference = 'Stop'; ",
    "$launch = Get-Content -Raw -LiteralPath $env:BH_LAUNCH_FILE | ConvertFrom-Json; ",
    "foreach ($name in @($launch.removeEnv)) { ",
    "Remove-Item -LiteralPath ('Env:' + [string]$name) -ErrorAction SilentlyContinue }; ",
    "foreach ($entry in @($launch.setEnv.PSObject.Properties)) { ",
    "Set-Item -LiteralPath ('Env:' + $entry.Name) -Value ([string]$entry.Value) }; ",
    // The payload is executed with -NoProfile; restore registered Agent CLI names
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

pub(in crate::services::temporary_cli) struct WindowsLaunchPayloadInput<'a> {
    pub(in crate::services::temporary_cli) cli_path: &'a str,
    pub(in crate::services::temporary_cli) cli_command_name: &'a str,
    pub(in crate::services::temporary_cli) plan: &'a TemporaryLaunchPlan,
    pub(in crate::services::temporary_cli) proxy_environment: &'a ProxyEnvironment,
    pub(in crate::services::temporary_cli) shell_snapshot: &'a ShellEnvironmentSnapshot,
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
    } = input;
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
        "cliPath": cli_path,
        "cliCommandName": cli_command_name,
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

#[cfg(target_os = "windows")]
fn windows_color_block() -> &'static str {
    "set NO_COLOR=\r\nset CLICOLOR=1\r\nif not defined TERM set \"TERM=xterm-256color\"\r\n"
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
