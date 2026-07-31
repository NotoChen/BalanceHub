use std::{
    collections::VecDeque,
    io::Read,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const READER_JOIN_GRACE: Duration = Duration::from_secs(5);
const READER_JOIN_FINAL_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct CommandOutput {
    pub status: Option<ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Default)]
struct CapturedText {
    text: String,
}

/// 隐藏仅用于后台探测、测活或清理的 Windows 控制台窗口。
///
/// 用户主动打开的终端不应调用该函数，否则会把本应可见的 CLI 窗口一并隐藏。
pub(crate) fn configure_background_command(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

/// 让后台子进程运行在独立进程组，以便超时时杀掉 helper/MCP 等后代进程。
pub(crate) fn configure_process_group(command: &mut Command) -> &mut Command {
    configure_background_command(command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

pub(crate) fn run_command_with_output_timeout(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> std::io::Result<CommandOutput> {
    let max_output_bytes = max_output_bytes.max(1);
    configure_process_group(command);
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(wait_with_output_timeout(child, timeout, max_output_bytes))
}

pub(crate) fn wait_with_output_timeout(
    mut child: std::process::Child,
    timeout: Duration,
    max_output_bytes: usize,
) -> CommandOutput {
    let pid = child.id();
    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| spawn_pipe_reader(pipe, max_output_bytes));
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| spawn_pipe_reader(pipe, max_output_bytes));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                kill_process_tree(pid);
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let stdout = join_reader_with_deadline(stdout_reader, pid);
    let stderr = join_reader_with_deadline(stderr_reader, pid);
    CommandOutput {
        status,
        stdout: stdout.text,
        stderr: stderr.text,
        timed_out,
    }
}

fn spawn_pipe_reader(
    mut pipe: impl Read + Send + 'static,
    max_output_bytes: usize,
) -> mpsc::Receiver<CapturedText> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = VecDeque::with_capacity(max_output_bytes.min(64 * 1024));
        let mut chunk = [0_u8; 8 * 1024];
        let mut truncated = false;
        while let Ok(read) = pipe.read(&mut chunk) {
            if read == 0 {
                break;
            }
            if read >= max_output_bytes {
                buffer.clear();
                buffer.extend(&chunk[read - max_output_bytes..read]);
                truncated = true;
                continue;
            }
            while buffer.len().saturating_add(read) > max_output_bytes {
                buffer.pop_front();
                truncated = true;
            }
            buffer.extend(&chunk[..read]);
        }

        let mut text =
            String::from_utf8_lossy(&buffer.into_iter().collect::<Vec<_>>()).into_owned();
        if truncated {
            text.insert_str(0, "[输出过长，仅保留末尾内容]\n");
        }
        let _ = sender.send(CapturedText { text });
    });
    receiver
}

fn join_reader_with_deadline(
    reader: Option<mpsc::Receiver<CapturedText>>,
    pid: u32,
) -> CapturedText {
    let Some(receiver) = reader else {
        return CapturedText::default();
    };
    match receiver.recv_timeout(READER_JOIN_GRACE) {
        Ok(output) => output,
        Err(_) => {
            kill_process_tree(pid);
            receiver
                .recv_timeout(READER_JOIN_FINAL_GRACE)
                .unwrap_or(CapturedText {
                    text: String::new(),
                })
        }
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("taskkill");
        configure_background_command(&mut command);
        let _ = command
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn timeout_kills_grandchildren_and_returns() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 30 & echo started; sleep 30");
        let started = Instant::now();
        let output =
            run_command_with_output_timeout(&mut command, Duration::from_millis(300), 64 * 1024)
                .unwrap();
        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn output_is_bounded_and_keeps_tail() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("printf '1234567890tail'");
        let output =
            run_command_with_output_timeout(&mut command, Duration::from_secs(2), 8).unwrap();
        assert!(output.stdout.starts_with("[输出过长，仅保留末尾内容]"));
        assert!(output.stdout.ends_with("90tail"));
    }
}
