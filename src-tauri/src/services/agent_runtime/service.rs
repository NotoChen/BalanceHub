//! Tauri-facing orchestration for the unified Agent runtime repository.
//!
//! The repository owns projection and spool persistence. This service owns the
//! application lifecycle: it initializes the repository below App data,
//! provides blocking-safe IPC access, and emits only real revision changes.

use super::{
    enrichment::RuntimeEnrichmentProducer,
    repository::{AgentRuntimeRefreshOutcome, AgentRuntimeRepository, AgentRuntimeSnapshot},
};
use crate::services::cli_runtime;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const AGENT_RUNTIME_UPDATED_EVENT: &str = "agent-runtime-updated";
pub const AGENT_RUNTIME_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const AGENT_RUNTIME_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
type RuntimeActivator = dyn Fn(&str) -> Result<(), String> + Send + Sync;
type RuntimeSnapshotEmitter = dyn Fn(AgentRuntimeSnapshot) -> Result<(), String> + Send + Sync;

struct RefreshInFlightGuard {
    state: Arc<AtomicU64>,
    token: u64,
}

impl Drop for RefreshInFlightGuard {
    fn drop(&mut self) {
        let _ = self
            .state
            .compare_exchange(self.token, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// A small, serializable status surface lets the UI explain initialization or
/// refresh failures without turning them into a startup failure or a stream of
/// notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeServiceStatus {
    pub initialized: bool,
    pub revision: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct AgentRuntimeService {
    repository: Option<Arc<AgentRuntimeRepository>>,
    initialization_error: Option<String>,
    last_error: Arc<Mutex<Option<String>>>,
    last_successful_revision: Arc<AtomicU64>,
    last_emitted_revision: Arc<AtomicU64>,
    refresh_in_flight: Arc<AtomicU64>,
    refresh_sequence: Arc<AtomicU64>,
    started: Arc<AtomicBool>,
    activator: Arc<RuntimeActivator>,
    enrichment: Option<RuntimeEnrichmentProducer>,
}

impl AgentRuntimeService {
    /// Initializes from the Tauri-owned App data directory. Failure is kept as
    /// data so the desktop application can continue starting in fail-open mode.
    pub fn from_app(app: &AppHandle) -> Self {
        let result = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("获取 Agent runtime App data 目录失败: {error}"))
            .and_then(|path| {
                AgentRuntimeRepository::new(path)
                    .map_err(|error| format!("初始化 Agent runtime repository 失败: {error}"))
            });
        let app = app.clone();
        Self::build(
            result,
            Arc::new(crate::services::temporary_cli::activate),
            Some(Arc::new(move |snapshot| {
                app.emit(AGENT_RUNTIME_UPDATED_EVENT, snapshot)
                    .map_err(|error| format!("发送 Agent runtime 更新事件失败: {error}"))
            })),
        )
    }

    /// Injection point for repository initialization failure tests.
    #[cfg(test)]
    pub fn from_repository_result(result: Result<AgentRuntimeRepository, String>) -> Self {
        Self::with_activator(result, Arc::new(crate::services::temporary_cli::activate))
    }

    #[cfg(test)]
    pub(crate) fn with_activator(
        result: Result<AgentRuntimeRepository, String>,
        activator: Arc<RuntimeActivator>,
    ) -> Self {
        Self::build(result, activator, None)
    }

    fn build(
        result: Result<AgentRuntimeRepository, String>,
        activator: Arc<RuntimeActivator>,
        snapshot_emitter: Option<Arc<RuntimeSnapshotEmitter>>,
    ) -> Self {
        let (repository, initialization_error, revision, snapshot_error) = match result {
            Ok(repository) => {
                let (revision, snapshot_error) = match repository.snapshot() {
                    Ok(snapshot) => (snapshot.revision, None),
                    Err(error) => (
                        0,
                        Some(format!("读取 Agent runtime projection 失败: {error}")),
                    ),
                };
                (Some(Arc::new(repository)), None, revision, snapshot_error)
            }
            Err(error) => (None, Some(error), 0, None),
        };
        let last_error = Arc::new(Mutex::new(snapshot_error));
        let last_successful_revision = Arc::new(AtomicU64::new(revision));
        let last_emitted_revision = Arc::new(AtomicU64::new(revision));
        let enrichment = repository.as_ref().map(|repository| {
            let successful_revision = last_successful_revision.clone();
            let emitted_revision = last_emitted_revision.clone();
            let last_error = last_error.clone();
            let emitter = snapshot_emitter
                .clone()
                .unwrap_or_else(|| Arc::new(|_| Ok(())));
            RuntimeEnrichmentProducer::new(
                repository.clone(),
                Arc::new(move |snapshot| {
                    successful_revision.fetch_max(snapshot.revision, Ordering::AcqRel);
                    if claim_runtime_revision(&emitted_revision, snapshot.revision) {
                        if let Err(error) = emitter(snapshot) {
                            let mut last_error =
                                last_error.lock().unwrap_or_else(|error| error.into_inner());
                            *last_error = Some(error);
                        }
                    }
                }),
            )
        });
        Self {
            repository,
            initialization_error,
            last_error,
            last_successful_revision,
            last_emitted_revision,
            refresh_in_flight: Arc::new(AtomicU64::new(0)),
            refresh_sequence: Arc::new(AtomicU64::new(0)),
            started: Arc::new(AtomicBool::new(false)),
            activator,
            enrichment,
        }
    }

    pub fn status(&self) -> AgentRuntimeServiceStatus {
        let error = self.initialization_error.clone().or_else(|| {
            self.last_error
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        });
        AgentRuntimeServiceStatus {
            initialized: self.repository.is_some(),
            revision: self
                .repository
                .as_ref()
                .map(|_| self.last_successful_revision.load(Ordering::Acquire)),
            error,
        }
    }

    pub fn snapshot(&self) -> Result<AgentRuntimeSnapshot, String> {
        let repository = self.repository.as_ref().ok_or_else(|| {
            self.initialization_error
                .clone()
                .unwrap_or_else(|| "Agent runtime repository 不可用".to_string())
        })?;
        let result = repository
            .snapshot()
            .map_err(|error| format!("读取 Agent runtime snapshot 失败: {error}"));
        match &result {
            Ok(snapshot) => {
                self.last_successful_revision
                    .store(snapshot.revision, Ordering::Release);
                self.last_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
            Err(error) => self.record_error(error.clone()),
        }
        result
    }

    /// Refreshes from the bounded Hook spool and the existing launcher source.
    /// This method is synchronous by design; callers that can be on an async
    /// path must use [`refresh_and_emit`] so all filesystem work is offloaded.
    fn refresh_now_outcome(&self) -> Result<AgentRuntimeRefreshOutcome, String> {
        let repository = self.repository.as_ref().ok_or_else(|| {
            self.initialization_error
                .clone()
                .unwrap_or_else(|| "Agent runtime repository 不可用".to_string())
        })?;
        let launch_snapshots = cli_runtime::runtime_instances();
        let result = repository
            .refresh_with_launch_snapshots_outcome_at(
                &launch_snapshots,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| format!("刷新 Agent runtime projection 失败: {error}"));
        match &result {
            Ok(outcome) => {
                self.last_successful_revision
                    .store(outcome.snapshot.revision, Ordering::Release);
                self.last_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
            }
            Err(error) => {
                let mut last_error = self
                    .last_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if last_error.as_deref() != Some(error.as_str()) {
                    *last_error = Some(error.clone());
                }
            }
        }
        result
    }

    /// Starts one backend task. It never performs filesystem work on the async
    /// executor and never terminates the app when a refresh fails.
    pub fn start(&self, app: &AppHandle) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let service = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            service.enqueue_recovery_requests().await;
            let mut interval = tokio::time::interval(AGENT_RUNTIME_REFRESH_INTERVAL);
            loop {
                interval.tick().await;
                service.refresh_and_emit(&app).await;
            }
        });
    }

    async fn enqueue_recovery_requests(&self) {
        let service = self.clone();
        let result = tauri::async_runtime::spawn_blocking(move || service.snapshot()).await;
        let Ok(Ok(snapshot)) = result else { return };
        let Some(producer) = &self.enrichment else {
            return;
        };
        for session in snapshot.sessions {
            if !needs_enrichment_recovery(&session) {
                continue;
            }
            let Some(session_id) = session.agent_session_id else {
                continue;
            };
            producer.enqueue(super::enrichment::RuntimeEnrichmentRequest {
                trigger_event_id: format!(
                    "recovery:{}:{}",
                    session.runtime_id,
                    session.last_activity_at.unwrap_or_default()
                ),
                target_runtime_id: session.runtime_id,
                runtime_scope: session.runtime_scope,
                agent_kind: session.agent_kind,
                agent_session_id: session_id,
                workdir: session.workdir.map(std::path::PathBuf::from),
                transcript_path_hint: None,
                trigger_observed_at: session.last_activity_at.unwrap_or_default(),
                priority: super::enrichment::RuntimeEnrichmentPriority::Normal,
            });
        }
    }

    pub async fn refresh_and_emit(&self, app: &AppHandle) {
        let token = self
            .refresh_sequence
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        if self
            .refresh_in_flight
            .compare_exchange(0, token, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let service = self.clone();
        let refresh_in_flight = self.refresh_in_flight.clone();
        let task = tauri::async_runtime::spawn_blocking(move || {
            let _guard = RefreshInFlightGuard {
                state: refresh_in_flight,
                token,
            };
            service.refresh_now_outcome()
        });
        let snapshot = match tokio::time::timeout(AGENT_RUNTIME_REFRESH_TIMEOUT, task).await {
            Err(_) => {
                // `spawn_blocking` cannot be cancelled safely. Release the
                // gate here so a stalled filesystem probe does not disable
                // every later refresh; the guard still releases it when the
                // detached worker eventually exits.
                let _ = self.refresh_in_flight.compare_exchange(
                    token,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                self.record_error("Agent runtime 刷新超时".to_string());
                return;
            }
            Ok(Err(error)) => {
                self.record_error(format!("Agent runtime 后台刷新任务异常: {error}"));
                return;
            }
            Ok(Ok(Err(error))) => {
                self.record_error(error);
                return;
            }
            Ok(Ok(Ok(outcome))) => {
                if let Some(producer) = &self.enrichment {
                    for request in &outcome.enrichment_requests {
                        producer.enqueue(request.clone());
                    }
                }
                outcome.snapshot
            }
        };
        if !claim_runtime_revision(&self.last_emitted_revision, snapshot.revision) {
            return;
        }
        if let Err(error) = app.emit(AGENT_RUNTIME_UPDATED_EVENT, snapshot) {
            self.record_error(format!("发送 Agent runtime 更新事件失败: {error}"));
        }
    }

    pub fn activate_runtime(&self, runtime_id: &str) -> Result<(), String> {
        if runtime_id.trim().is_empty() {
            return Err("runtime_id 不能为空".to_string());
        }
        let snapshot = self.snapshot()?;
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.runtime_id == runtime_id)
            .ok_or_else(|| "未找到可激活的 Agent runtime".to_string())?;
        let instance_id = runtime_activation_instance_id(session)?;
        (self.activator)(instance_id)
    }

    fn record_error(&self, error: String) {
        let mut last_error = self
            .last_error
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if last_error.as_deref() != Some(error.as_str()) {
            *last_error = Some(error);
        }
    }
}

fn runtime_activation_instance_id(
    session: &crate::models::AgentRuntimeSession,
) -> Result<&str, String> {
    if !session.actions.can_activate_terminal
        || session
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.locator.as_deref())
            .is_none()
    {
        return Err("当前 Agent runtime 没有精确终端定位能力".to_string());
    }
    if session.origin != crate::models::AgentRuntimeOrigin::BalancehubLaunch {
        return Err("外部 Agent runtime 不允许由 BalanceHub 激活终端".to_string());
    }
    session
        .balancehub_instance_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "Agent runtime 缺少精确 BalanceHub 实例标识".to_string())
}

fn needs_enrichment_recovery(session: &crate::models::AgentRuntimeSession) -> bool {
    let latest_hook = session
        .evidence
        .iter()
        .filter(|evidence| evidence.source == crate::models::AgentRuntimeEvidenceSource::Hook)
        .map(|evidence| evidence.observed_at)
        .max();
    let latest_adapter = session
        .evidence
        .iter()
        .filter(|evidence| {
            evidence.source == crate::models::AgentRuntimeEvidenceSource::SessionAdapter
        })
        .map(|evidence| evidence.observed_at)
        .max();
    session.title.is_none()
        || session.model.is_none()
        || latest_hook.is_some_and(|hook| latest_adapter.is_none_or(|adapter| adapter < hook))
}

pub(crate) fn claim_runtime_revision(last_emitted: &AtomicU64, current_revision: u64) -> bool {
    let mut previous = last_emitted.load(Ordering::Acquire);
    while current_revision > previous {
        match last_emitted.compare_exchange_weak(
            previous,
            current_revision,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(actual) => previous = actual,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AgentCliKind, AgentRuntimeActions, AgentRuntimeOrigin, AgentRuntimeScope,
        AgentRuntimeState, AgentRuntimeTerminalEvidence, TemporaryCliInstance,
        TemporaryCliInstanceStatus, TemporaryCliTerminalKind,
    };

    fn session(
        origin: AgentRuntimeOrigin,
        locator: Option<&str>,
    ) -> crate::models::AgentRuntimeSession {
        crate::models::AgentRuntimeSession {
            runtime_id: "balancehub:instance-1".to_string(),
            runtime_scope: AgentRuntimeScope::Native,
            origin,
            agent_kind: AgentCliKind::Codex,
            agent_session_id: None,
            balancehub_instance_id: Some("instance-1".to_string()),
            provider: None,
            workdir: None,
            title: None,
            model: None,
            process: None,
            terminal: locator.map(|locator| AgentRuntimeTerminalEvidence {
                kind: TemporaryCliTerminalKind::Ghostty,
                locator: Some(locator.to_string()),
                observed_at: 1,
            }),
            state: AgentRuntimeState::Idle,
            evidence: Vec::new(),
            started_at: Some(1),
            last_activity_at: Some(1),
            ended_at: None,
            exit_code: None,
            actions: AgentRuntimeActions {
                can_activate_terminal: true,
                ..AgentRuntimeActions::default()
            },
            state_observed_at: Some(1),
        }
    }

    #[test]
    fn initialization_failure_is_fail_open_and_readable() {
        let service = AgentRuntimeService::from_repository_result(Err("permission denied".into()));
        assert!(!service.status().initialized);
        assert_eq!(service.status().error.as_deref(), Some("permission denied"));
        assert!(service.snapshot().is_err());
    }

    #[test]
    fn activation_requires_locator_and_balancehub_origin() {
        assert!(runtime_activation_instance_id(&session(
            AgentRuntimeOrigin::BalancehubLaunch,
            None
        ))
        .is_err());
        assert!(runtime_activation_instance_id(&session(
            AgentRuntimeOrigin::ExternalHook,
            Some("terminal-1")
        ))
        .is_err());
    }

    #[test]
    fn valid_activation_passes_exact_instance_id_to_injected_activator() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-runtime-service-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repository = AgentRuntimeRepository::new(&root).unwrap();
        repository
            .refresh_with_launch_snapshots(&[TemporaryCliInstance {
                id: "instance-1".to_string(),
                provider_id: "provider-1".to_string(),
                provider_name: "Provider".to_string(),
                session_title: "Session".to_string(),
                account_label: "Account".to_string(),
                api_key_local_id: None,
                cli_kind: AgentCliKind::Codex,
                workdir: "/workspace".to_string(),
                terminal_kind: TemporaryCliTerminalKind::Ghostty,
                terminal_name: "Ghostty".to_string(),
                terminal_locator: Some(
                    "{\"kind\":\"ghostty\",\"terminalId\":\"terminal-1\"}".to_string(),
                ),
                started_at: "1".to_string(),
                ended_at: None,
                pid: Some(42),
                status: TemporaryCliInstanceStatus::Running,
                exit_code: None,
                can_activate: true,
            }])
            .unwrap();
        let called = Arc::new(Mutex::new(Vec::<String>::new()));
        let called_by_activator = called.clone();
        let service = AgentRuntimeService::with_activator(
            Ok(repository),
            Arc::new(move |instance_id: &str| {
                called_by_activator
                    .lock()
                    .unwrap()
                    .push(instance_id.to_string());
                Ok(())
            }),
        );
        service.activate_runtime("balancehub:instance-1").unwrap();
        assert_eq!(called.lock().unwrap().as_slice(), &["instance-1"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn revision_claim_is_monotonic() {
        let revision = AtomicU64::new(7);
        assert!(!claim_runtime_revision(&revision, 7));
        assert!(claim_runtime_revision(&revision, 9));
        assert!(!claim_runtime_revision(&revision, 8));
        assert_eq!(revision.load(Ordering::Acquire), 9);
    }

    #[test]
    fn recovery_requires_missing_metadata_or_newer_hook_evidence() {
        let mut candidate = session(AgentRuntimeOrigin::ExternalHook, None);
        candidate.agent_session_id = Some("session-1".to_string());
        candidate.title = Some("Title".to_string());
        candidate.model = Some("Model".to_string());
        candidate.evidence = vec![
            crate::models::AgentRuntimeEvidence {
                source: crate::models::AgentRuntimeEvidenceSource::Hook,
                confidence: crate::models::AgentRuntimeConfidence::Observed,
                observed_at: 10,
                event_id: "hook-1".to_string(),
            },
            crate::models::AgentRuntimeEvidence {
                source: crate::models::AgentRuntimeEvidenceSource::SessionAdapter,
                confidence: crate::models::AgentRuntimeConfidence::Observed,
                observed_at: 11,
                event_id: "adapter-1".to_string(),
            },
        ];
        assert!(!needs_enrichment_recovery(&candidate));

        candidate
            .evidence
            .push(crate::models::AgentRuntimeEvidence {
                source: crate::models::AgentRuntimeEvidenceSource::Hook,
                confidence: crate::models::AgentRuntimeConfidence::Observed,
                observed_at: 12,
                event_id: "hook-2".to_string(),
            });
        assert!(needs_enrichment_recovery(&candidate));

        candidate.evidence[1].observed_at = 13;
        candidate.model = None;
        assert!(needs_enrichment_recovery(&candidate));
    }

    #[test]
    fn stale_refresh_guard_cannot_clear_a_new_owner() {
        let state = Arc::new(AtomicU64::new(2));
        let stale = RefreshInFlightGuard {
            state: state.clone(),
            token: 1,
        };
        drop(stale);
        assert_eq!(state.load(Ordering::Acquire), 2);

        let current = RefreshInFlightGuard {
            state: state.clone(),
            token: 2,
        };
        drop(current);
        assert_eq!(state.load(Ordering::Acquire), 0);
    }
}
