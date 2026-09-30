use std::{
    collections::VecDeque,
    io::Read,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TryRecvError},
        Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak,
    },
    thread,
    time::{Duration, Instant},
};

const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(50);
const REAPER_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_MANAGED_PROCESS_LIFECYCLES: usize = 8;

static PROCESS_LIFECYCLE_MANAGER: OnceLock<Arc<ProcessLifecycleManager>> = OnceLock::new();

pub(crate) struct CommandOutput {
    pub status: Option<ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl CommandOutput {
    fn timed_out_without_starting() -> Self {
        Self {
            status: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }
}

#[derive(Default)]
struct CapturedText {
    text: String,
    truncated: bool,
}

/// Opt-in bounded live output for user-requested installer diagnostics.
#[derive(Clone, Default)]
pub(crate) struct ProcessOutputSnapshot {
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Default)]
pub(crate) struct ProcessOutputCapture(Mutex<ProcessOutputSnapshot>);
impl ProcessOutputCapture {
    pub fn snapshot(&self) -> ProcessOutputSnapshot {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    pub fn finish(&self, exit_code: Option<i32>, error: Option<String>) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.exit_code = exit_code;
        state.error = error;
    }
    fn update(&self, stderr: bool, buffer: &VecDeque<u8>, truncated: bool) {
        let text =
            String::from_utf8_lossy(&buffer.iter().copied().collect::<Vec<_>>()).into_owned();
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if stderr {
            state.stderr = text;
        } else {
            state.stdout = text;
        }
        state.truncated |= truncated;
    }
}

struct OutputCaptureOptions {
    max_bytes: usize,
    live: Option<Arc<ProcessOutputCapture>>,
}

struct ProcessLifecycleManager {
    capacity: usize,
    state: Mutex<ProcessLifecycleState>,
    slot_available: Condvar,
    reaper_alive: AtomicBool,
    #[cfg(test)]
    reaper_threads_started: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
struct ProcessLifecycleState {
    active: usize,
    pending: Vec<PendingProcessReap>,
}

struct ProcessLifecyclePermit {
    manager: Option<Arc<ProcessLifecycleManager>>,
}

struct PendingProcessReap {
    child: Option<Child>,
    stdout_reader: Option<mpsc::Receiver<CapturedText>>,
    stderr_reader: Option<mpsc::Receiver<CapturedText>>,
    cleanup_child: Option<Child>,
    _permit: ProcessLifecyclePermit,
}

struct ReaperWorkerGuard {
    manager: Weak<ProcessLifecycleManager>,
}

impl ProcessLifecycleManager {
    fn global() -> Arc<Self> {
        PROCESS_LIFECYCLE_MANAGER
            .get_or_init(|| Self::start(MAX_MANAGED_PROCESS_LIFECYCLES))
            .clone()
    }

    fn start(capacity: usize) -> Arc<Self> {
        let manager = Arc::new(Self {
            capacity: capacity.max(1),
            state: Mutex::new(ProcessLifecycleState::default()),
            slot_available: Condvar::new(),
            reaper_alive: AtomicBool::new(true),
            #[cfg(test)]
            reaper_threads_started: std::sync::atomic::AtomicUsize::new(0),
        });
        let worker_manager = Arc::downgrade(&manager);
        match thread::Builder::new()
            .name("balancehub-process-reaper".to_string())
            .spawn(move || reap_pending_processes(worker_manager))
        {
            Ok(_) => {
                #[cfg(test)]
                manager
                    .reaper_threads_started
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                // Fail closed: without the singleton reaper, no managed child is
                // allowed to start and become an unbounded unreaped resource.
                manager.reaper_alive.store(false, Ordering::Release);
            }
        }
        manager
    }

    fn acquire_until(self: &Arc<Self>, deadline: Instant) -> Option<ProcessLifecyclePermit> {
        let mut state = self.lock_state();
        loop {
            if !self.reaper_alive.load(Ordering::Acquire) || Instant::now() >= deadline {
                return None;
            }
            if state.active < self.capacity {
                state.active += 1;
                return Some(ProcessLifecyclePermit {
                    manager: Some(Arc::clone(self)),
                });
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next_state, _) = self
                .slot_available
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next_state;
        }
    }

    fn enqueue(&self, pending: PendingProcessReap) {
        let mut state = self.lock_state();
        debug_assert!(state.pending.len() < self.capacity);
        state.pending.push(pending);
    }

    fn poll_pending(&self) -> bool {
        let mut completed = Vec::new();
        let mut state = self.lock_state();
        let had_pending = !state.pending.is_empty();
        let mut index = 0;
        while index < state.pending.len() {
            if state.pending[index].poll_complete() {
                completed.push(state.pending.swap_remove(index));
            } else {
                index += 1;
            }
        }
        drop(state);

        // Permit destruction takes the same state lock, so completed entries
        // must be dropped only after the pending collection lock is released.
        drop(completed);
        had_pending
    }

    fn release(&self) {
        let mut state = self.lock_state();
        debug_assert!(state.active > 0);
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.slot_available.notify_all();
    }

    fn lock_state(&self) -> MutexGuard<'_, ProcessLifecycleState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.lock_state().active
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.lock_state().pending.len()
    }

    #[cfg(test)]
    fn reaper_thread_count(&self) -> usize {
        self.reaper_threads_started.load(Ordering::Relaxed)
    }
}

impl Drop for ProcessLifecyclePermit {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.take() {
            manager.release();
        }
    }
}

impl PendingProcessReap {
    fn poll_complete(&mut self) -> bool {
        poll_child(&mut self.child);
        poll_reader(&mut self.stdout_reader);
        poll_reader(&mut self.stderr_reader);
        poll_child(&mut self.cleanup_child);
        self.child.is_none()
            && self.stdout_reader.is_none()
            && self.stderr_reader.is_none()
            && self.cleanup_child.is_none()
    }
}

impl Drop for ReaperWorkerGuard {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.upgrade() {
            manager.reaper_alive.store(false, Ordering::Release);
            manager.slot_available.notify_all();
        }
    }
}

fn reap_pending_processes(manager: Weak<ProcessLifecycleManager>) {
    let _worker_guard = ReaperWorkerGuard {
        manager: manager.clone(),
    };
    loop {
        let Some(manager) = manager.upgrade() else {
            return;
        };
        let had_pending = manager.poll_pending();
        drop(manager);
        thread::sleep(if had_pending {
            CHILD_POLL_INTERVAL
        } else {
            REAPER_IDLE_POLL_INTERVAL
        });
    }
}

fn poll_child(child: &mut Option<Child>) {
    let completed = match child.as_mut() {
        None => true,
        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
    };
    if completed {
        *child = None;
    }
}

fn poll_reader(reader: &mut Option<mpsc::Receiver<CapturedText>>) {
    let completed = match reader.as_ref() {
        None => true,
        Some(reader) => matches!(reader.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)),
    };
    if completed {
        *reader = None;
    }
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
    let manager = ProcessLifecycleManager::global();
    run_command_with_manager(&manager, command, timeout, max_output_bytes)
}

/// Revalidate after acquiring a process slot, then recheck the deadline before
/// closing the cancellation boundary. `before_spawn` must be a constant-time
/// commit marker; all filesystem/process validation belongs in `preflight`.
pub(crate) fn run_command_with_output_timeout_before_spawn(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
    preflight: impl FnOnce() -> std::io::Result<()>,
    before_spawn: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<CommandOutput> {
    run_command_with_output_observed(
        command,
        timeout,
        max_output_bytes,
        preflight,
        before_spawn,
        None,
    )
}

pub(crate) fn run_command_with_output_observed(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
    preflight: impl FnOnce() -> std::io::Result<()>,
    before_spawn: impl FnOnce() -> std::io::Result<()>,
    live: Option<Arc<ProcessOutputCapture>>,
) -> std::io::Result<CommandOutput> {
    run_observed_with_manager(
        &ProcessLifecycleManager::global(),
        command,
        timeout,
        OutputCaptureOptions {
            max_bytes: max_output_bytes,
            live,
        },
        preflight,
        before_spawn,
    )
}

fn run_command_with_manager(
    manager: &Arc<ProcessLifecycleManager>,
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> std::io::Result<CommandOutput> {
    run_command_with_manager_before_spawn(
        manager,
        command,
        timeout,
        max_output_bytes,
        || Ok(()),
        || Ok(()),
    )
}

fn run_command_with_manager_before_spawn(
    manager: &Arc<ProcessLifecycleManager>,
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: usize,
    preflight: impl FnOnce() -> std::io::Result<()>,
    before_spawn: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<CommandOutput> {
    run_observed_with_manager(
        manager,
        command,
        timeout,
        OutputCaptureOptions {
            max_bytes: max_output_bytes,
            live: None,
        },
        preflight,
        before_spawn,
    )
}

fn run_observed_with_manager(
    manager: &Arc<ProcessLifecycleManager>,
    command: &mut Command,
    timeout: Duration,
    output: OutputCaptureOptions,
    preflight: impl FnOnce() -> std::io::Result<()>,
    before_spawn: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<CommandOutput> {
    let deadline = Instant::now() + timeout;
    let Some(permit) = manager.acquire_until(deadline) else {
        return Ok(CommandOutput::timed_out_without_starting());
    };
    if Instant::now() >= deadline {
        drop(permit);
        return Ok(CommandOutput::timed_out_without_starting());
    }
    let output = OutputCaptureOptions {
        max_bytes: output.max_bytes.max(1),
        ..output
    };
    configure_process_group(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    preflight()?;
    if Instant::now() >= deadline {
        return Ok(CommandOutput::timed_out_without_starting());
    }
    before_spawn()?;
    let child = command.spawn()?;
    Ok(wait_with_output_until(
        manager, child, deadline, output, permit,
    ))
}

fn wait_with_output_until(
    manager: &Arc<ProcessLifecycleManager>,
    mut child: Child,
    deadline: Instant,
    output: OutputCaptureOptions,
    permit: ProcessLifecyclePermit,
) -> CommandOutput {
    let pid = child.id();
    let stdout_reader = child
        .stdout
        .take()
        .map(|pipe| spawn_observed_pipe_reader(pipe, output.max_bytes, output.live.clone(), false));
    let stderr_reader = child
        .stderr
        .take()
        .map(|pipe| spawn_observed_pipe_reader(pipe, output.max_bytes, output.live.clone(), true));

    let mut timed_out = false;
    let mut pending_child = None;
    let mut cleanup_child = None;
    let mut kill_requested = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                let termination = terminate_child(child, pid);
                pending_child = termination.child;
                cleanup_child = termination.cleanup_child;
                kill_requested = true;
                break termination.status;
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                thread::sleep(CHILD_POLL_INTERVAL.min(remaining));
            }
            Err(_) => {
                let termination = terminate_child(child, pid);
                pending_child = termination.child;
                cleanup_child = termination.cleanup_child;
                kill_requested = true;
                break termination.status;
            }
        }
    };

    let stdout = join_reader_until(stdout_reader, deadline);
    if stdout.pending.is_some() {
        timed_out = true;
        if !kill_requested {
            cleanup_child = request_process_tree_kill(pid);
            kill_requested = true;
        }
    }
    let stderr = join_reader_until(stderr_reader, deadline);
    if stderr.pending.is_some() {
        timed_out = true;
        if !kill_requested {
            cleanup_child = request_process_tree_kill(pid);
        }
    }

    let output = CommandOutput {
        status,
        stdout: stdout.output.text,
        stderr: stderr.output.text,
        timed_out,
        stdout_truncated: stdout.output.truncated,
        stderr_truncated: stderr.output.truncated,
    };
    let pending = PendingProcessReap {
        child: pending_child,
        stdout_reader: stdout.pending,
        stderr_reader: stderr.pending,
        cleanup_child,
        _permit: permit,
    };
    if pending.child.is_some()
        || pending.stdout_reader.is_some()
        || pending.stderr_reader.is_some()
        || pending.cleanup_child.is_some()
    {
        manager.enqueue(pending);
    }
    output
}

struct TerminationResult {
    status: Option<ExitStatus>,
    child: Option<Child>,
    cleanup_child: Option<Child>,
}

fn terminate_child(mut child: Child, pid: u32) -> TerminationResult {
    let cleanup_child = request_process_tree_kill(pid);
    let _ = child.kill();
    match child.try_wait() {
        Ok(Some(status)) => TerminationResult {
            status: Some(status),
            child: None,
            cleanup_child,
        },
        Ok(None) | Err(_) => TerminationResult {
            status: None,
            child: Some(child),
            cleanup_child,
        },
    }
}

#[cfg(test)]
fn spawn_pipe_reader(
    pipe: impl Read + Send + 'static,
    max_output_bytes: usize,
) -> mpsc::Receiver<CapturedText> {
    spawn_observed_pipe_reader(pipe, max_output_bytes, None, false)
}

fn spawn_observed_pipe_reader(
    mut pipe: impl Read + Send + 'static,
    max_output_bytes: usize,
    live: Option<Arc<ProcessOutputCapture>>,
    stderr: bool,
) -> mpsc::Receiver<CapturedText> {
    let (sender, receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("balancehub-process-output".to_string())
        .spawn(move || {
            let mut buffer = VecDeque::with_capacity(max_output_bytes.min(64 * 1024));
            let mut chunk = [0_u8; 8 * 1024];
            let mut truncated = false;
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                if read > max_output_bytes {
                    buffer.clear();
                    buffer.extend(&chunk[read - max_output_bytes..read]);
                    truncated = true;
                    if let Some(live) = &live {
                        live.update(stderr, &buffer, truncated);
                    }
                    continue;
                }
                while buffer.len().saturating_add(read) > max_output_bytes {
                    buffer.pop_front();
                    truncated = true;
                }
                buffer.extend(&chunk[..read]);
                if let Some(live) = &live {
                    live.update(stderr, &buffer, truncated);
                }
            }

            let mut text =
                String::from_utf8_lossy(&buffer.into_iter().collect::<Vec<_>>()).into_owned();
            if truncated {
                text.insert_str(0, "[输出过长，仅保留末尾内容]\n");
            }
            let _ = sender.send(CapturedText { text, truncated });
        });
    receiver
}

struct ReaderJoinResult {
    output: CapturedText,
    pending: Option<mpsc::Receiver<CapturedText>>,
}

fn join_reader_until(
    reader: Option<mpsc::Receiver<CapturedText>>,
    deadline: Instant,
) -> ReaderJoinResult {
    let Some(receiver) = reader else {
        return ReaderJoinResult {
            output: CapturedText::default(),
            pending: None,
        };
    };

    match receiver.try_recv() {
        Ok(output) => {
            return ReaderJoinResult {
                output,
                pending: None,
            };
        }
        Err(TryRecvError::Disconnected) => {
            return ReaderJoinResult {
                output: CapturedText::default(),
                pending: None,
            };
        }
        Err(TryRecvError::Empty) => {}
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return ReaderJoinResult {
            output: receiver.try_recv().unwrap_or_default(),
            pending: Some(receiver),
        };
    }

    match receiver.recv_timeout(remaining) {
        Ok(output) => ReaderJoinResult {
            output,
            pending: None,
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => ReaderJoinResult {
            output: CapturedText::default(),
            pending: None,
        },
        Err(mpsc::RecvTimeoutError::Timeout) => ReaderJoinResult {
            output: receiver.try_recv().unwrap_or_default(),
            pending: Some(receiver),
        },
    }
}

/// Request best-effort cleanup without waiting indefinitely for the process tree
/// to exit. Windows cleanup uses the same bounded lifecycle manager as probes.
pub(crate) fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("taskkill");
        command.args(["/T", "/F", "/PID", &pid.to_string()]);
        let _ = run_command_with_output_timeout(&mut command, Duration::from_secs(2), 8 * 1024);
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Only Windows cleanup spawns a child; the other targets return None.
        let _ = request_process_tree_kill(pid);
    }
}

fn request_process_tree_kill(pid: u32) -> Option<Child> {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("taskkill");
        configure_background_command(&mut command);
        command
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn external_tree_kill_terminates_a_synthetic_process_group() {
        use std::os::unix::process::{CommandExt, ExitStatusExt};

        fn wait_until_exited(
            child: &mut Child,
            deadline: Instant,
        ) -> std::io::Result<Option<ExitStatus>> {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok(Some(status));
                }
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        struct FixtureChildren(Vec<Child>);

        impl Drop for FixtureChildren {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    let _ = child.kill();
                }
                let deadline = Instant::now() + Duration::from_secs(2);
                for child in &mut self.0 {
                    let _ = wait_until_exited(child, deadline);
                }
            }
        }

        let sleeper = || {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "exec sleep 10"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command
        };
        let mut leader = sleeper();
        configure_process_group(&mut leader);
        let leader = leader.spawn().unwrap();
        let pid = leader.id();
        // Both group members remain direct children, so this fixture can reap
        // each one without depending on orphan adoption or a second reaper.
        let mut children = FixtureChildren(vec![leader]);
        let mut member = sleeper();
        member.process_group(i32::try_from(pid).unwrap());
        children.0.push(member.spawn().unwrap());

        let started = Instant::now();
        kill_process_tree(pid);
        assert!(started.elapsed() < Duration::from_secs(2));

        let deadline = Instant::now() + Duration::from_secs(2);
        for child in &mut children.0 {
            let status = wait_until_exited(child, deadline)
                .unwrap()
                .expect("group member should exit within the cleanup deadline");
            assert_eq!(status.signal(), Some(libc::SIGKILL));
        }
    }

    #[test]
    fn waiting_for_process_capacity_never_crosses_commit_boundary() {
        let manager = ProcessLifecycleManager::start(1);
        let permit = manager
            .acquire_until(Instant::now() + Duration::from_secs(1))
            .unwrap();
        let crossed = AtomicBool::new(false);
        let mut command = Command::new("bh-fixture-must-not-run");
        let result = run_command_with_manager_before_spawn(
            &manager,
            &mut command,
            Duration::from_millis(10),
            32,
            || Ok(()),
            || {
                crossed.store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap();
        assert!(result.timed_out);
        assert!(!crossed.load(Ordering::Acquire));
        assert_eq!(manager.active_count(), 1);
        drop(permit);
    }

    #[test]
    fn rejected_preflight_releases_process_slot_without_spawn() {
        let manager = ProcessLifecycleManager::start(1);
        let mut command = Command::new("bh-fixture-must-not-run");
        let error = run_command_with_manager_before_spawn(
            &manager,
            &mut command,
            Duration::from_secs(1),
            32,
            || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            || panic!("rejected preflight must never commit"),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn expired_preflight_never_commits_or_spawns() {
        let manager = ProcessLifecycleManager::start(1);
        let mut command = Command::new("bh-fixture-must-not-run");
        let crossed = AtomicBool::new(false);
        let output = run_command_with_manager_before_spawn(
            &manager,
            &mut command,
            Duration::from_millis(5),
            32,
            || {
                thread::sleep(Duration::from_millis(15));
                Ok(())
            },
            || {
                crossed.store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap();
        assert!(output.timed_out);
        assert!(output.status.is_none());
        assert!(!crossed.load(Ordering::Acquire));
        assert_eq!(manager.active_count(), 0);
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn exited_child_cannot_extend_timeout_by_leaving_output_pipe_open() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 30 & printf 'parent exited\\n'");
        let started = Instant::now();
        let output =
            run_command_with_output_timeout(&mut command, Duration::from_millis(200), 64 * 1024)
                .unwrap();

        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn output_is_bounded_and_keeps_tail() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("printf '1234567890tail'");
        let output =
            run_command_with_output_timeout(&mut command, Duration::from_secs(2), 8).unwrap();
        assert!(output.stdout.starts_with("[输出过长，仅保留末尾内容]"));
        assert!(output.stdout.ends_with("90tail"));
        assert!(output.stdout_truncated);
        assert!(!output.stderr_truncated);
    }

    #[test]
    fn reader_marks_stdout_exact_cap_as_complete() {
        let receiver = spawn_pipe_reader(std::io::Cursor::new(b"12345678".to_vec()), 8);
        let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(output.text, "12345678");
        assert!(!output.truncated);
    }

    #[test]
    fn reader_marks_stdout_one_over_cap_as_truncated() {
        let receiver = spawn_pipe_reader(std::io::Cursor::new(b"123456789".to_vec()), 8);
        let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(output.text.starts_with("[输出过长，仅保留末尾内容]"));
        assert!(output.text.ends_with("23456789"));
        assert!(output.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn stderr_reports_exact_and_one_over_caps() {
        let mut exact = Command::new("/bin/sh");
        exact.arg("-c").arg("printf '12345678' >&2");
        let exact = run_command_with_output_timeout(&mut exact, Duration::from_secs(2), 8).unwrap();
        assert_eq!(exact.stderr, "12345678");
        assert!(!exact.stderr_truncated);

        let mut overflow = Command::new("/bin/sh");
        overflow.arg("-c").arg("printf '123456789' >&2");
        let overflow =
            run_command_with_output_timeout(&mut overflow, Duration::from_secs(2), 8).unwrap();
        assert!(overflow.stderr.ends_with("23456789"));
        assert!(overflow.stderr_truncated);
    }

    #[test]
    fn lifecycle_gate_bounds_unreapable_items_and_polls_past_the_first() {
        let manager = ProcessLifecycleManager::start(2);
        assert_eq!(manager.reaper_thread_count(), 1);

        let (first_sender, first_reader) = mpsc::channel();
        let first_permit = manager
            .acquire_until(Instant::now() + Duration::from_secs(1))
            .unwrap();
        manager.enqueue(PendingProcessReap {
            child: None,
            stdout_reader: Some(first_reader),
            stderr_reader: None,
            cleanup_child: None,
            _permit: first_permit,
        });

        let (second_sender, second_reader) = mpsc::channel();
        let second_permit = manager
            .acquire_until(Instant::now() + Duration::from_secs(1))
            .unwrap();
        manager.enqueue(PendingProcessReap {
            child: None,
            stdout_reader: Some(second_reader),
            stderr_reader: None,
            cleanup_child: None,
            _permit: second_permit,
        });

        for _ in 0..3 {
            let mut command = Command::new("__balancehub_lifecycle_gate_must_not_spawn__");
            let output =
                run_command_with_manager(&manager, &mut command, Duration::from_millis(25), 8)
                    .unwrap();
            assert!(output.timed_out);
            assert_eq!(manager.active_count(), 2);
            assert!(manager.pending_count() <= 2);
            assert_eq!(manager.reaper_thread_count(), 1);
        }

        drop(second_sender);
        wait_for_active_count(&manager, 1);
        assert_eq!(manager.pending_count(), 1);

        drop(first_sender);
        wait_for_active_count(&manager, 0);
        assert_eq!(manager.pending_count(), 0);
        assert_eq!(manager.reaper_thread_count(), 1);
    }

    fn wait_for_active_count(manager: &ProcessLifecycleManager, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while manager.active_count() != expected && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(manager.active_count(), expected);
    }
}
