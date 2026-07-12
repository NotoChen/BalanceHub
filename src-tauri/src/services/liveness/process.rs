use super::cli::runtime_path_for;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) struct ChildOutcome {
    pub status: Option<std::process::ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// 读线程 join 的常规等待：进程（组）结束后管道立即 EOF，正常远小于此值。
const READER_JOIN_GRACE: Duration = Duration::from_secs(5);
/// 常规等待超时后补杀进程组，再给读线程的最后等待。
const READER_JOIN_FINAL_GRACE: Duration = Duration::from_secs(2);

/// 让子进程运行在独立进程组。
///
/// codex/claude 都是会再拉起 helper/MCP 子进程的 Node CLI：超时只 kill 直接子进程会
/// 留下继续消耗真实额度的孤儿，孤儿若还握着 stdout 写端，读线程永远等不到 EOF。
/// Unix 下 spawn 时 setpgid(0)，超时可整组杀灭；Windows 走 taskkill /T 杀进程树，
/// 无需 spawn 期配置。
pub(super) fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// 尽力杀灭以 `pid` 为根的整棵进程树（组）。对未经 [`configure_process_group`]
/// 启动的子进程调用也安全：组 id 不存在时内核返回 ESRCH，无副作用。
fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        // spawn 时已 setpgid(0)，进程组 id 即子进程 pid；负数表示整组。
        // pid 在这几秒内被系统复用的概率可忽略。
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
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
    let pid = child.id();
    let stdout_reader = child.stdout.take().map(spawn_pipe_reader);
    let stderr_reader = child.stderr.take().map(spawn_pipe_reader);

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                kill_process_tree(pid);
                // 兜底直杀：未走进程组启动的调用方（如版本探测）也能被终止。
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let stdout = join_reader_with_deadline(stdout_reader, pid);
    let stderr = join_reader_with_deadline(stderr_reader, pid);

    ChildOutcome {
        status,
        stdout,
        stderr,
        timed_out,
    }
}

fn spawn_pipe_reader(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = String::new();
        let _ = pipe.read_to_string(&mut buffer);
        let _ = sender.send(buffer);
    });
    receiver
}

/// 等待读线程完成，带两级保底：子进程（组）已结束后管道应立即 EOF；若仍有未知
/// 进程继承着写端导致读不到 EOF，先补杀整组再短等一次，最终宁可丢弃输出也绝不
/// 让调用方（spawn_blocking 工作线程）无限期挂死。超时后读线程被遗弃自灭。
fn join_reader_with_deadline(reader: Option<mpsc::Receiver<String>>, pid: u32) -> String {
    let Some(receiver) = reader else {
        return String::new();
    };
    match receiver.recv_timeout(READER_JOIN_GRACE) {
        Ok(output) => output,
        Err(_) => {
            kill_process_tree(pid);
            receiver
                .recv_timeout(READER_JOIN_FINAL_GRACE)
                .unwrap_or_default()
        }
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
    configure_process_group(&mut command);
    let child = command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    let outcome = wait_with_output_timeout(child, Duration::from_secs(3));
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// 超时路径必须连孙进程一起杀：起一个 shell，让它再起一个长眠孙进程并把
    /// stdout 写端传给孙进程 —— 旧实现只杀 shell，读线程会被孙进程握着的管道
    /// 卡死；组杀后整个函数应在超时 + 保底等待内返回。
    #[test]
    fn timeout_kills_grandchildren_and_returns() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 30 & echo started; sleep 30")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);
        let child = command.spawn().expect("test child should spawn");
        let pid = child.id();

        let started = Instant::now();
        let outcome = wait_with_output_timeout(child, Duration::from_millis(300));
        assert!(outcome.timed_out);
        // 300ms 超时 + 读线程保底，远小于孙进程的 30s 睡眠即返回。
        assert!(started.elapsed() < Duration::from_secs(10));

        // 整组（含后台 sleep 孙进程）都应已被杀灭。被杀进程在被 reap 前是僵尸、
        // kill(0) 仍视为存在，故轮询等待而非单次断言。
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut alive = true;
        while Instant::now() < deadline {
            alive = unsafe { libc::kill(-(pid as i32), 0) } == 0;
            if !alive {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(!alive, "process group should be fully killed");
    }

    #[test]
    fn normal_exit_collects_output() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("echo hello")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);
        let child = command.spawn().expect("test child should spawn");

        let outcome = wait_with_output_timeout(child, Duration::from_secs(5));
        assert!(!outcome.timed_out);
        assert_eq!(outcome.stdout.trim(), "hello");
        assert!(outcome.status.is_some_and(|status| status.success()));
    }
}
