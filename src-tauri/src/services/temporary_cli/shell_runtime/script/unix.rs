use super::{write_auxiliary_file, LaunchScriptInput};
use crate::{
    network::ProxyEnvironment,
    services::agent_cli::{self, contracts::EnvironmentPatch},
};
use std::{env, fs, path::Path};

pub(in crate::services::temporary_cli) fn write_launch_script(
    input: &LaunchScriptInput<'_>,
) -> Result<(), String> {
    write_auxiliary_file(
        input.auxiliary_file_path,
        input.plan.auxiliary_file_content.as_deref(),
    )?;
    let path_export = agent_cli::runtime_path_for(Path::new(input.cli_path))
        .map(|path| format!("export PATH={}\n", shell_quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let script_dir = input
        .script
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(env::temp_dir);

    let agent_environment_block = unix_environment_block(&input.plan.environment);
    let cleanup_settings = input
        .auxiliary_file_path
        .as_ref()
        .map(|path| format!("rm -f {}\n", shell_quote(&path.to_string_lossy())))
        .unwrap_or_default();
    let script_path = shell_quote(&input.script.to_string_lossy());
    let login_shell_bootstrap = login_shell_bootstrap(input.script);
    let cli_invocation =
        unix_cli_invocation(input.cli_command_name, input.cli_path, &input.plan.args);
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
bh_exit_code=$?
if [ "$bh_exit_code" -ne 0 ]; then
  bh_write_status exited null "$(bh_now_ms)" "$bh_exit_code"
  exit "$bh_exit_code"
fi
    {path_export}{color_block}{proxy_block}{agent_environment_block}bh_write_status running "$$" null null
{cli_invocation}
bh_exit_code=$?
bh_write_status exited null "$(bh_now_ms)" "$bh_exit_code"
rm -f "$bh_script_path"
{cleanup_settings}rmdir {script_dir} 2>/dev/null || true
exit "$bh_exit_code"
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

fn unix_color_block() -> &'static str {
    "unset NO_COLOR\nexport CLICOLOR=1\nif [ \"${TERM:-dumb}\" = \"dumb\" ]; then export TERM=xterm-256color; fi\n"
}

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

fn unix_environment_block(environment: &EnvironmentPatch) -> String {
    let removed = environment.removed_names().collect::<Vec<_>>();
    let mut block = String::new();
    if !removed.is_empty() {
        block.push_str("unset ");
        block.push_str(&removed.join(" "));
        block.push('\n');
    }
    for (name, value) in environment.set_values() {
        block.push_str("export ");
        block.push_str(name);
        block.push('=');
        block.push_str(&shell_quote(value));
        block.push('\n');
    }
    block
}

pub(super) fn restrict_to_owner(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("读取临时 CLI 配置权限失败: {err}"))?
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|err| format!("设置临时 CLI 配置权限失败: {err}"))
}

pub(in crate::services::temporary_cli) fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("读取临时脚本权限失败: {err}"))?
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).map_err(|err| format!("设置临时脚本权限失败: {err}"))
}

pub(in crate::services::temporary_cli) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn preview_quote(value: &str) -> String {
    shell_quote(value)
}

pub(in crate::services::temporary_cli) fn login_shell_bootstrap(script: &Path) -> String {
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

pub(in crate::services::temporary_cli) fn shell_supports_posix_source(shell: &str) -> bool {
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

pub(in crate::services::temporary_cli) fn unix_cli_invocation(
    command_name: &str,
    cli_path: &str,
    args: &[String],
) -> String {
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
pub(in crate::services::temporary_cli) fn script_command(script: &Path) -> String {
    format!("exec {}", script_command_without_exec(script))
}

#[cfg(target_os = "macos")]
pub(in crate::services::temporary_cli) fn script_command_without_exec(script: &Path) -> String {
    format!("/bin/sh {}", shell_quote(&script.to_string_lossy()))
}

pub(in crate::services::temporary_cli) fn user_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_string()
        } else {
            "/bin/sh".to_string()
        }
    })
}
