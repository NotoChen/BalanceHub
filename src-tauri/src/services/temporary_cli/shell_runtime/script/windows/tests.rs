use super::*;
use crate::{
    models::{AppSettings, ProxyMode},
    network,
    services::agent_cli::contracts::EnvironmentPatch,
};

fn render_fixture(workdir: &Path, script: &Path, status: &Path, auxiliary: &Path) -> String {
    let plan = TemporaryLaunchPlan {
        args: Vec::new(),
        environment: EnvironmentPatch::default(),
        auxiliary_file_content: None,
    };
    let proxy = network::resolve_global_proxy(&AppSettings {
        proxy_mode: ProxyMode::NoProxy,
        ..AppSettings::default()
    });
    render_launch_script(
        &LaunchScriptInput {
            script,
            cli_path: "fixture",
            cli_command_name: "fixture",
            workdir,
            plan: &plan,
            auxiliary_file_path: Some(auxiliary),
            status_path: status,
            proxy_environment: &proxy.environment(),
            prefer_shell_cli: false,
        },
        &script.with_file_name("launch.json"),
    )
    .unwrap()
}

#[test]
fn batch_paths_are_literal_and_local_directories_remain_selected() {
    let script = render_fixture(
        Path::new(r"C:\项目 (demo) & 100% !"),
        Path::new("launch/fixture.cmd"),
        Path::new("status.json"),
        Path::new("launch/auxiliary.json"),
    );
    assert!(script.starts_with(
        "@echo off\r\nsetlocal EnableExtensions DisableDelayedExpansion\r\nchcp 65001 >nul 2>nul\r\n"
    ));
    assert!(script
        .contains("cd /d \"C:\\项目 (demo) & 100%% !\"\r\nif errorlevel 1 goto BH_WORKDIR_ERROR"));
    assert!(!script.contains("set \"BH_PUSHED=1\""));
    assert!(!script.replace("\r\n", "").contains('\n'));
}

#[test]
fn shared_directories_use_pushd_and_release_the_temporary_drive() {
    let script = render_fixture(
        Path::new(r"\\server\share\project"),
        Path::new("launch/fixture.cmd"),
        Path::new("status.json"),
        Path::new("launch/auxiliary.json"),
    );
    assert!(script.contains("pushd \"\\\\server\\share\\project\"\r\nif errorlevel 1 goto BH_WORKDIR_ERROR\r\nset \"BH_PUSHED=1\""));
    assert!(script.contains("if defined BH_PUSHED popd\r\n"));
}

#[test]
fn cleanup_releases_the_batch_context_after_status_is_written() {
    let script = render_fixture(
        Path::new("workdir"),
        Path::new("launch/fixture.cmd"),
        Path::new("status.json"),
        Path::new("launch/auxiliary.json"),
    );
    let finish = script.split(":BH_FINISH\r\n").nth(1).unwrap();
    let cleanup = finish.split("\r\n(goto) 2>nul & ").nth(1).unwrap();
    assert!(
        finish.find("call :BH_WRITE_STATUS exited").unwrap() < finish.find("\r\n(goto)").unwrap()
    );
    assert!(cleanup.starts_with(
        "del \"%~f0\" 2>nul & rmdir /s /q \"launch\" 2>nul & cmd /D /C exit %BH_EXIT_CODE%\r\n"
    ));
}

#[cfg(windows)]
#[test]
fn canonical_workdir_is_not_embedded_as_a_verbatim_path() {
    let script = render_fixture(
        Path::new(r"\\?\C:\Users\demo"),
        Path::new(r"\\?\C:\Temp\launch\fixture.cmd"),
        Path::new(r"\\?\C:\Temp\status.json"),
        Path::new(r"\\?\C:\Temp\launch\auxiliary.json"),
    );
    assert!(!script.contains(r"\\?\"));
    assert!(script.contains(r#"cd /d "C:\Users\demo""#));
}

#[cfg(windows)]
#[test]
fn cmd_launches_in_the_selected_directory_and_removes_its_wrapper() {
    run_windows_fixture(0, true);
}

#[cfg(windows)]
#[test]
fn cmd_preserves_cli_failure_and_still_completes_cleanup() {
    run_windows_fixture(7, true);
}

#[cfg(windows)]
#[test]
fn missing_workdir_never_runs_the_cli_and_still_completes_cleanup() {
    run_windows_fixture(0, false);
}

#[cfg(windows)]
fn run_windows_fixture(cli_exit_code: i32, create_workdir: bool) {
    use crate::platform::process::run_command_with_output_timeout;
    use std::{os::windows::process::CommandExt, process::Command, time::Duration};

    let root = tempfile::Builder::new()
        .prefix("balancehub-windows-script-")
        .tempdir()
        .unwrap();
    let workdir = root.path().join("项目 (demo) & 100%PATH% !");
    if create_workdir {
        fs::create_dir(&workdir).unwrap();
    }
    let launch_dir = root.path().join("启动脚本 (demo) & 100%PATH% !");
    fs::create_dir(&launch_dir).unwrap();
    let script = launch_dir.join("fixture.cmd");
    let auxiliary = launch_dir.join("auxiliary.json");
    let status = root.path().join("status.json");
    let capture = root.path().join("cwd.txt");
    let executable = root.path().join("fixture 中文 cli.cmd");
    let live_pid = root.path().join("live-pid.txt");
    fs::write(
        &executable,
        format!("@echo off\r\n> \"%BALANCEHUB_TEST_CAPTURE%\" cd\r\npowershell.exe -NoProfile -Command \"$BH_State = Get-Content -Raw -Encoding UTF8 -LiteralPath $env:BALANCEHUB_TEST_STATUS | ConvertFrom-Json; [System.IO.File]::WriteAllText($env:BALANCEHUB_TEST_LIVE_PID, [string][System.Diagnostics.Process]::GetProcessById($BH_State.pid).Id)\"\r\nexit /b {cli_exit_code}\r\n"),
    )
    .unwrap();
    fs::write(&auxiliary, "{}").unwrap();

    let canonical_workdir = root
        .path()
        .canonicalize()
        .unwrap()
        .join(workdir.file_name().unwrap());
    let plan = TemporaryLaunchPlan {
        args: Vec::new(),
        environment: EnvironmentPatch::default(),
        auxiliary_file_content: None,
    };
    let proxy = network::resolve_global_proxy(&AppSettings {
        proxy_mode: ProxyMode::NoProxy,
        ..AppSettings::default()
    });
    let payload = windows_launch_payload(WindowsLaunchPayloadInput {
        cli_path: executable.to_str().unwrap(),
        cli_command_name: "balancehub-script-fixture",
        plan: &plan,
        proxy_environment: &proxy.environment(),
        shell_snapshot: &ShellEnvironmentSnapshot::default(),
        prefer_shell_cli: false,
    });
    fs::write(
        script.with_file_name("launch.json"),
        serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
    fs::write(
        &script,
        render_fixture(&canonical_workdir, &script, &status, &auxiliary),
    )
    .unwrap();

    let mut command = Command::new("cmd.exe");
    command
        .args(["/D", "/E:ON", "/V:OFF", "/S", "/C"])
        .raw_arg(r#"""%BALANCEHUB_TEST_SCRIPT%"""#)
        .env("BALANCEHUB_TEST_SCRIPT", &script)
        .env("BALANCEHUB_TEST_CAPTURE", &capture)
        .env("BALANCEHUB_TEST_STATUS", &status)
        .env("BALANCEHUB_TEST_LIVE_PID", &live_pid)
        .current_dir(root.path());
    let output =
        run_command_with_output_timeout(&mut command, Duration::from_secs(30), 32 * 1024).unwrap();
    assert!(!output.timed_out, "{}", output.stderr);
    let result: serde_json::Value = serde_json::from_slice(&fs::read(&status).unwrap()).unwrap();
    assert_eq!(result["status"], "exited");
    assert!(result["endedAt"].is_number());
    if create_workdir {
        assert_eq!(
            output.status.and_then(|status| status.code()),
            Some(cli_exit_code),
            "stdout={}\nstderr={}\nstatus={}",
            output.stdout,
            output.stderr,
            result
        );
        assert_eq!(result["exitCode"], cli_exit_code);
        assert!(
            fs::read_to_string(&live_pid)
                .unwrap()
                .parse::<u32>()
                .unwrap()
                > 0
        );
        assert_eq!(
            Path::new(fs::read_to_string(&capture).unwrap().trim())
                .canonicalize()
                .unwrap(),
            workdir.canonicalize().unwrap()
        );
        assert!(output.stderr.trim().is_empty(), "{}", output.stderr);
    } else {
        assert!(!output.status.unwrap().success());
        assert_ne!(result["exitCode"], 0);
        assert!(!capture.exists());
    }
    assert!(!script.exists());
    assert!(!auxiliary.exists());
    assert!(!launch_dir.exists());
    assert!(!output.stderr.contains("找不到批处理文件"));
    assert!(!output
        .stderr
        .to_lowercase()
        .contains("batch file cannot be found"));
}
