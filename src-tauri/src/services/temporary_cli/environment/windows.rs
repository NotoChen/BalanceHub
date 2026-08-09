use super::ShellEnvironmentSnapshot;
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{collections::BTreeMap, env, process::Command, time::Duration};

const ENV_START: &str = "__BALANCEHUB_ENV_START__";
const ENV_END: &str = "__BALANCEHUB_ENV_END__";
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn capture() -> ShellEnvironmentSnapshot {
    let mut snapshot = ShellEnvironmentSnapshot {
        variables: current_process_environment(),
        ..ShellEnvironmentSnapshot::default()
    };
    if let Some(cmd_environment) = capture_cmd() {
        merge_map(&mut snapshot.variables, cmd_environment);
    }
    if let Some(power_shell_snapshot) = capture_powershell() {
        merge_map(&mut snapshot.variables, power_shell_snapshot.variables);
        snapshot.aliases = power_shell_snapshot.aliases;
        snapshot.functions = power_shell_snapshot.functions;
    }
    snapshot
}

fn current_process_environment() -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();
    for (name, value) in env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with('=') {
            continue;
        }
        super::insert_environment(&mut environment, &name, &value.to_string_lossy());
    }
    environment
}

fn capture_powershell() -> Option<ShellEnvironmentSnapshot> {
    let binary = powershell_binary()?;
    let command = format!(
        "$profiles = @($PROFILE.CurrentUserAllHosts, $PROFILE.CurrentUserCurrentHost, $PROFILE.AllUsersAllHosts, $PROFILE.AllUsersCurrentHost) | Where-Object {{ $_ -and (Test-Path -LiteralPath $_) }} | Select-Object -Unique; foreach ($profilePath in $profiles) {{ try {{ . $profilePath }} catch {{ }} }}; $values = [ordered]@{{}}; Get-ChildItem Env: | ForEach-Object {{ $values[[string]$_.Name] = [string]$_.Value }}; $aliases = [ordered]@{{}}; $functions = [ordered]@{{}}; foreach ($name in @('codex', 'claude')) {{ $commandInfo = Get-Command -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1; if ($null -ne $commandInfo -and $commandInfo.CommandType -eq 'Alias') {{ $aliases[$name] = [string]$commandInfo.Definition }} elseif ($null -ne $commandInfo -and @('Function', 'Filter') -contains [string]$commandInfo.CommandType) {{ $functions[$name] = [string]$commandInfo.Definition }} }}; $snapshot = [ordered]@{{ environment = $values; aliases = $aliases; functions = $functions }}; [Console]::Out.WriteLine('{ENV_START}'); [Console]::Out.WriteLine(($snapshot | ConvertTo-Json -Compress -Depth 8)); [Console]::Out.WriteLine('{ENV_END}')"
    );
    let mut process = Command::new(binary);
    process.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &command,
    ]);
    let output = run_command_with_output_timeout(
        &mut process,
        CAPTURE_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return None;
    }
    let json = marked_block(&output.stdout)?;
    let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let object = value.as_object()?;
    Some(ShellEnvironmentSnapshot {
        variables: string_map(object.get("environment")?),
        aliases: string_map(object.get("aliases").unwrap_or(&serde_json::Value::Null)),
        functions: string_map(object.get("functions").unwrap_or(&serde_json::Value::Null)),
    })
}

fn string_map(value: &serde_json::Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.clone(), value.to_string()))
        })
        .collect()
}

fn capture_cmd() -> Option<BTreeMap<String, String>> {
    let command_line = format!("echo {ENV_START} & set & echo {ENV_END}");
    let mut process = Command::new("cmd");
    process.args(["/C", &command_line]);
    let output = run_command_with_output_timeout(
        &mut process,
        CAPTURE_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return None;
    }
    let block = marked_block(&output.stdout)?;
    let mut environment = BTreeMap::new();
    for line in block.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        // cmd exposes drive-current-directory entries such as `=C:` through
        // `set`; they are not real environment variables and are invalid in
        // PowerShell's Env: provider.
        if !name.is_empty() && !name.starts_with('=') {
            super::insert_environment(&mut environment, name, value);
        }
    }
    Some(environment)
}

fn powershell_binary() -> Option<&'static str> {
    ["pwsh", "powershell"].into_iter().find(|binary| {
        let mut process = Command::new(binary);
        process.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ]);
        run_command_with_output_timeout(&mut process, CAPTURE_TIMEOUT, 16 * 1024)
            .map(|output| !output.timed_out && output.status.is_some_and(|status| status.success()))
            .unwrap_or(false)
    })
}

fn marked_block(output: &str) -> Option<&str> {
    let start = output.find(ENV_START)? + ENV_START.len();
    let end = start + output[start..].find(ENV_END)?;
    Some(output[start..end].trim())
}

fn merge_map(target: &mut BTreeMap<String, String>, source: BTreeMap<String, String>) {
    for (name, value) in source {
        super::insert_environment(target, &name, &value);
    }
}

#[cfg(test)]
mod tests {
    use super::marked_block;
    use crate::services::temporary_cli::environment::insert_environment;
    use std::collections::BTreeMap;

    #[test]
    fn marked_block_ignores_profile_noise() {
        let output = "profile output\n__BALANCEHUB_ENV_START__\n{\"Path\":\"C:\\\\bin\"}\n__BALANCEHUB_ENV_END__\n";
        assert_eq!(marked_block(output), Some("{\"Path\":\"C:\\\\bin\"}"));
    }

    #[test]
    fn environment_names_are_merged_case_insensitively() {
        let mut values = BTreeMap::new();
        insert_environment(&mut values, "Path", "one");
        insert_environment(&mut values, "PATH", "two");
        assert_eq!(values.len(), 1);
        assert_eq!(values.get("PATH"), Some(&"two".to_string()));
    }
}
