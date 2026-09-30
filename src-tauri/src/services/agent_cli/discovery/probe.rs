use super::paths::{canonicalize_for_owner, lexical_comparison_key, RuntimePathSnapshot};
use super::{AgentCliDefinition, AgentCliExecutable};
use crate::{
    models::{AgentAssetLimits, AgentExecutableProbeErrorKind},
    platform::process::run_command_with_output_timeout,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, Metadata},
    path::{Path, PathBuf},
    process::Command,
    sync::{Condvar, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ProbedCli {
    pub executable: AgentCliExecutable,
    pub owner_key: String,
    pub canonical_path: String,
    pub executable_revision: String,
    pub output_truncated: bool,
}

#[derive(Debug)]
pub(super) struct ProbeFailure {
    pub kind: AgentExecutableProbeErrorKind,
    pub message: String,
    pub output_truncated: bool,
    pub budget_exceeded: bool,
}

impl ProbeFailure {
    fn new(kind: AgentExecutableProbeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            output_truncated: false,
            budget_exceeded: false,
        }
    }

    pub(super) fn budget_exceeded() -> Self {
        Self {
            kind: AgentExecutableProbeErrorKind::TimedOut,
            message: "CLI 版本探测等待超时".to_string(),
            output_truncated: false,
            budget_exceeded: true,
        }
    }
}

pub(super) struct CliProcessGate {
    capacity: usize,
    active: Mutex<usize>,
    wake: Condvar,
}

impl CliProcessGate {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            active: Mutex::new(0),
            wake: Condvar::new(),
        }
    }

    pub(super) fn acquire_until(&self, deadline: Instant) -> Option<CliProcessPermit<'_>> {
        if self.capacity == 0 {
            return None;
        }
        let mut active = self.lock_active();
        loop {
            if *active < self.capacity {
                *active += 1;
                return Some(CliProcessPermit { gate: self });
            }

            let remaining = deadline.checked_duration_since(Instant::now())?;
            let (next, wait) = self
                .wake
                .wait_timeout(active, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            active = next;
            if wait.timed_out() && Instant::now() >= deadline {
                return None;
            }
        }
    }

    fn lock_active(&self) -> MutexGuard<'_, usize> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    pub(super) fn active(&self) -> usize {
        *self.lock_active()
    }
}

pub(super) struct CliProcessPermit<'a> {
    gate: &'a CliProcessGate,
}

impl Drop for CliProcessPermit<'_> {
    fn drop(&mut self) {
        let mut active = self.gate.lock_active();
        *active = active.saturating_sub(1);
        self.gate.wake.notify_one();
    }
}

pub(super) fn global_cli_process_gate() -> &'static CliProcessGate {
    static GATE: OnceLock<CliProcessGate> = OnceLock::new();
    GATE.get_or_init(|| CliProcessGate::new(AgentAssetLimits::HARD_CAP.cli_concurrency))
}

#[derive(Debug, PartialEq, Eq)]
struct ExecutableSnapshot {
    canonical_path: PathBuf,
    canonical_text: String,
    owner_key: String,
    binding_revision: String,
}

impl ExecutableSnapshot {
    fn capture(candidate: &Path, spec: &AgentCliDefinition) -> Result<Self, ProbeFailure> {
        let launcher_metadata = fs::symlink_metadata(candidate).map_err(metadata_failure)?;
        let target_metadata = fs::metadata(candidate).map_err(metadata_failure)?;
        if !target_metadata.is_file() {
            return Err(ProbeFailure::new(
                AgentExecutableProbeErrorKind::NotFound,
                "文件不存在",
            ));
        }

        let canonical_path = canonicalize_for_owner(candidate).map_err(metadata_failure)?;
        if let Some(validate) = spec.invalid_path_reason {
            if let Some(message) = validate(&canonical_path) {
                return Err(ProbeFailure::new(
                    AgentExecutableProbeErrorKind::Failed,
                    message,
                ));
            }
        }
        let canonical_metadata = fs::metadata(&canonical_path).map_err(metadata_failure)?;
        let canonical_text = canonical_path.to_string_lossy().into_owned();
        let owner_key = lexical_comparison_key(&canonical_path);
        let binding_revision = executable_binding_revision(
            candidate,
            &launcher_metadata,
            &canonical_path,
            &canonical_metadata,
        );

        Ok(Self {
            canonical_path,
            canonical_text,
            owner_key,
            binding_revision,
        })
    }

    fn matches_binding(&self, other: &Self) -> bool {
        self.owner_key == other.owner_key
            && self.canonical_path == other.canonical_path
            && self.binding_revision == other.binding_revision
    }
}

fn metadata_failure(error: std::io::Error) -> ProbeFailure {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => AgentExecutableProbeErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => AgentExecutableProbeErrorKind::PermissionDenied,
        _ => AgentExecutableProbeErrorKind::Failed,
    };
    let message = match kind {
        AgentExecutableProbeErrorKind::NotFound => "文件不存在".to_string(),
        AgentExecutableProbeErrorKind::PermissionDenied => "权限不足".to_string(),
        _ => error.to_string(),
    };
    ProbeFailure::new(kind, message)
}

pub(super) fn probe_cli_candidate(
    candidate: &Path,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    deadline: Instant,
    timeout: Duration,
    output_limit: usize,
) -> Result<ProbedCli, ProbeFailure> {
    super::version_cache::cached(candidate, spec, runtime_path, deadline, || {
        probe_cli_candidate_with_gate(
            candidate,
            spec,
            runtime_path,
            deadline,
            timeout,
            output_limit,
            global_cli_process_gate(),
        )
    })
}

pub(super) fn probe_cli_candidate_with_gate(
    candidate: &Path,
    spec: &AgentCliDefinition,
    runtime_path: &RuntimePathSnapshot,
    deadline: Instant,
    timeout: Duration,
    output_limit: usize,
    gate: &CliProcessGate,
) -> Result<ProbedCli, ProbeFailure> {
    let preflight = ExecutableSnapshot::capture(candidate, spec)?;
    let permit = gate
        .acquire_until(deadline)
        .ok_or_else(ProbeFailure::budget_exceeded)?;

    let (version, output_truncated) = cli_version_bounded(
        candidate,
        spec.require_version_substring,
        runtime_path,
        deadline,
        timeout,
        output_limit,
    )?;
    if Instant::now() >= deadline {
        let mut failure = ProbeFailure::budget_exceeded();
        failure.output_truncated = output_truncated;
        return Err(failure);
    }
    let postflight =
        ExecutableSnapshot::capture(candidate, spec).map_err(|failure| ProbeFailure {
            kind: AgentExecutableProbeErrorKind::ChangedDuringProbe,
            message: format!("CLI 在版本探测期间发生变化：{}", failure.message),
            output_truncated: output_truncated || failure.output_truncated,
            budget_exceeded: false,
        })?;
    if Instant::now() >= deadline {
        let mut failure = ProbeFailure::budget_exceeded();
        failure.output_truncated = output_truncated;
        return Err(failure);
    }
    drop(permit);

    if !preflight.matches_binding(&postflight) {
        return Err(ProbeFailure {
            kind: AgentExecutableProbeErrorKind::ChangedDuringProbe,
            message: "CLI 在版本探测期间发生变化".to_string(),
            output_truncated,
            budget_exceeded: false,
        });
    }

    let executable_revision = executable_revision(&postflight, candidate, &version);
    Ok(ProbedCli {
        executable: AgentCliExecutable {
            path: candidate.to_string_lossy().into_owned(),
            version,
        },
        owner_key: postflight.owner_key,
        canonical_path: postflight.canonical_text,
        executable_revision,
        output_truncated,
    })
}

fn cli_version_bounded(
    path: &Path,
    require_substring: Option<&str>,
    runtime_path: &RuntimePathSnapshot,
    deadline: Instant,
    timeout: Duration,
    output_limit: usize,
) -> Result<(String, bool), ProbeFailure> {
    let mut command = Command::new(path);
    if let Some(path_env) = runtime_path.path_for(path) {
        command.env("PATH", path_env);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ProbeFailure::budget_exceeded());
    }
    command.arg("--version");
    let outcome =
        run_command_with_output_timeout(&mut command, remaining.min(timeout), output_limit)
            .map_err(metadata_failure)?;
    let output_truncated = outcome.stdout_truncated || outcome.stderr_truncated;
    if outcome.timed_out {
        return Err(ProbeFailure {
            kind: AgentExecutableProbeErrorKind::TimedOut,
            message: "CLI 版本探测超时".to_string(),
            output_truncated,
            budget_exceeded: false,
        });
    }
    if !outcome.status.is_some_and(|status| status.success()) {
        let detail = (!outcome.stderr_truncated)
            .then_some(outcome.stderr.as_str())
            .into_iter()
            .chain((!outcome.stdout_truncated).then_some(outcome.stdout.as_str()))
            .flat_map(str::lines)
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(180).collect::<String>());
        return Err(ProbeFailure {
            kind: AgentExecutableProbeErrorKind::Failed,
            message: detail
                .map(|detail| format!("CLI 不可用：{detail}"))
                .unwrap_or_else(|| "CLI 不可用".to_string()),
            output_truncated,
            budget_exceeded: false,
        });
    }

    let version = (!outcome.stdout_truncated)
        .then_some(outcome.stdout.as_str())
        .into_iter()
        .chain((!outcome.stderr_truncated).then_some(outcome.stderr.as_str()))
        .flat_map(str::lines)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if version.is_empty() {
        return Err(ProbeFailure {
            kind: AgentExecutableProbeErrorKind::InvalidVersion,
            message: if output_truncated {
                "CLI 版本输出超过上限，无法确认版本".to_string()
            } else {
                "CLI 未返回版本信息".to_string()
            },
            output_truncated,
            budget_exceeded: false,
        });
    }
    if let Some(substring) = require_substring {
        if !version.to_ascii_lowercase().contains(substring) {
            return Err(ProbeFailure {
                kind: AgentExecutableProbeErrorKind::InvalidVersion,
                message: "CLI 版本信息不匹配".to_string(),
                output_truncated,
                budget_exceeded: false,
            });
        }
    }
    Ok((version, output_truncated))
}

fn executable_binding_revision(
    candidate: &Path,
    launcher_metadata: &Metadata,
    canonical_path: &Path,
    canonical_metadata: &Metadata,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"executable-binding:v1\0");
    update_path(&mut hasher, candidate);
    update_metadata(&mut hasher, launcher_metadata);
    if let Ok(target) = fs::read_link(candidate) {
        hasher.update(b"link\0");
        update_path(&mut hasher, &target);
    }
    update_path(&mut hasher, canonical_path);
    update_metadata(&mut hasher, canonical_metadata);
    format!("binding:{:x}", hasher.finalize())
}

fn executable_revision(snapshot: &ExecutableSnapshot, candidate: &Path, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"executable-revision:v1\0");
    hasher.update(lexical_comparison_key(candidate).as_bytes());
    hasher.update([0]);
    hasher.update(snapshot.owner_key.as_bytes());
    hasher.update([0]);
    hasher.update(snapshot.canonical_text.as_bytes());
    hasher.update([0]);
    hasher.update(snapshot.binding_revision.as_bytes());
    hasher.update([0]);
    hasher.update(version.as_bytes());
    format!("executable-revision:v1:{:x}", hasher.finalize())
}

fn update_path(hasher: &mut Sha256, path: &Path) {
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update([0]);
}

fn update_metadata(hasher: &mut Sha256, metadata: &Metadata) {
    let file_type = metadata.file_type();
    hasher.update([
        u8::from(file_type.is_file()),
        u8::from(file_type.is_dir()),
        u8::from(file_type.is_symlink()),
    ]);
    hasher.update(metadata.len().to_le_bytes());
    update_system_time(hasher, metadata.modified().ok());
    update_system_time(hasher, metadata.created().ok());

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hasher.update(metadata.dev().to_le_bytes());
        hasher.update(metadata.ino().to_le_bytes());
        hasher.update(metadata.mode().to_le_bytes());
        hasher.update(metadata.ctime().to_le_bytes());
        hasher.update(metadata.ctime_nsec().to_le_bytes());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        hasher.update(metadata.file_attributes().to_le_bytes());
        hasher.update(metadata.creation_time().to_le_bytes());
        hasher.update(metadata.last_write_time().to_le_bytes());
        hasher.update(metadata.file_size().to_le_bytes());
    }
}

fn update_system_time(hasher: &mut Sha256, time: Option<std::time::SystemTime>) {
    let nanos = time
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    hasher.update(nanos.to_le_bytes());
}
