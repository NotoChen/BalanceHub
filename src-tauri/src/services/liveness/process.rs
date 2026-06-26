use super::cli::runtime_path_for;
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) struct ChildOutcome {
    pub status: Option<std::process::ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// 带超时地等待子进程结束并收集其输出。
///
/// 关键点：spawn 后立刻用独立线程并发读取 stdout/stderr。若等进程退出后再读，
/// 子进程一旦向管道写满 OS 缓冲（约 64KB，codex `--json` 事件流可能触及）就会阻塞在写、
/// 永不退出，只能被超时 kill，从而把一次正常调用误报为超时。
pub(super) fn wait_with_output_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> ChildOutcome {
    let stdout_reader = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buffer = String::new();
            let _ = pipe.read_to_string(&mut buffer);
            buffer
        })
    });
    let stderr_reader = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buffer = String::new();
            let _ = pipe.read_to_string(&mut buffer);
            buffer
        })
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let stdout = stdout_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    ChildOutcome {
        status,
        stdout,
        stderr,
        timed_out,
    }
}

pub(super) fn cli_version(
    path: &std::path::Path,
    require_substring: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new(path);
    if let Some(path_env) = runtime_path_for(path) {
        command.env("PATH", path_env);
    }
    let child = command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    let outcome = wait_with_output_timeout(child, Duration::from_secs(10));
    if outcome.timed_out {
        return Err("CLI 版本探测超时".to_string());
    }
    if !outcome.status.is_some_and(|status| status.success()) {
        return Err("CLI 不可用".to_string());
    }
    let version = outcome.stdout.trim().to_string();
    if let Some(substring) = require_substring {
        if !version.to_ascii_lowercase().contains(substring) {
            return Err("CLI 版本信息不匹配".to_string());
        }
    }
    Ok(version)
}
