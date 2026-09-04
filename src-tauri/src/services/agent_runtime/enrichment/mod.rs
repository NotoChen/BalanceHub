//! Asynchronous, coalescing metadata enrichment for active Agent sessions.
//!
//! This module owns only scheduling. It never scans an Agent home and never
//! calls `SessionAdapter::list`; each job is an exact `(scope, agent, id)` lookup.

use crate::{
    models::{AgentCliKind, AgentRuntimeScope},
    services::agent_cli::contracts::{
        SessionMetadataCursor, SessionMetadataLookupBudget, SessionMetadataLookupError,
        SessionMetadataLookupResult,
    },
    services::{
        agent_cli,
        agent_runtime::{
            reducer::RuntimeEnrichment,
            repository::{AgentRuntimeRepository, AgentRuntimeSnapshot},
        },
    },
};
use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) const MAX_LOOKUP_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const LOOKUP_DEADLINE: Duration = Duration::from_millis(150);
pub(crate) const LOOKUP_OUTER_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const MAX_REGISTRY_ENTRIES: usize = 256;
const WORKER_COUNT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RuntimeEnrichmentLookupKey {
    pub scope: AgentRuntimeScope,
    pub agent_kind: AgentCliKind,
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeEnrichmentRequest {
    pub trigger_event_id: String,
    pub target_runtime_id: String,
    pub runtime_scope: AgentRuntimeScope,
    pub agent_kind: AgentCliKind,
    pub agent_session_id: String,
    pub workdir: Option<PathBuf>,
    pub transcript_path_hint: Option<PathBuf>,
    pub trigger_observed_at: i64,
    pub priority: RuntimeEnrichmentPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RuntimeEnrichmentPriority {
    Normal,
    Final,
}

#[derive(Debug, Clone)]
struct PendingJob {
    request: RuntimeEnrichmentRequest,
    generation: u64,
    next_attempt_at: Instant,
    failure_count: u8,
}

pub(crate) fn default_budget(cancelled: Arc<AtomicBool>) -> SessionMetadataLookupBudget {
    SessionMetadataLookupBudget {
        max_bytes: MAX_LOOKUP_BYTES,
        deadline: Instant::now() + LOOKUP_DEADLINE,
        cancelled,
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeEnrichmentProducer {
    state: Arc<ProducerInner>,
}

struct ProducerInner {
    queue: Mutex<ProducerQueue>,
    wake: Condvar,
    repository: Arc<AgentRuntimeRepository>,
    emit: Arc<dyn Fn(AgentRuntimeSnapshot) + Send + Sync>,
}

struct ProducerQueue {
    jobs: HashMap<(String, RuntimeEnrichmentLookupKey), PendingJob>,
    latest_generation: HashMap<(String, RuntimeEnrichmentLookupKey), u64>,
    active_agents: BTreeSet<AgentCliKind>,
    active_cancellations: HashMap<(String, RuntimeEnrichmentLookupKey), Arc<AtomicBool>>,
    failure_counts: HashMap<(String, RuntimeEnrichmentLookupKey), u8>,
    cursors: HashMap<RuntimeEnrichmentLookupKey, SessionMetadataCursor>,
    sequence: u64,
}

impl RuntimeEnrichmentProducer {
    pub(crate) fn new(
        repository: Arc<AgentRuntimeRepository>,
        emit: Arc<dyn Fn(AgentRuntimeSnapshot) + Send + Sync>,
    ) -> Self {
        let state = Arc::new(ProducerInner {
            queue: Mutex::new(ProducerQueue {
                jobs: HashMap::new(),
                latest_generation: HashMap::new(),
                active_agents: BTreeSet::new(),
                active_cancellations: HashMap::new(),
                failure_counts: HashMap::new(),
                cursors: HashMap::new(),
                sequence: 0,
            }),
            wake: Condvar::new(),
            repository,
            emit,
        });
        for _ in 0..WORKER_COUNT {
            let worker = state.clone();
            std::thread::Builder::new()
                .name("balancehub-agent-metadata".to_string())
                .spawn(move || worker_loop(worker))
                .ok();
        }
        Self { state }
    }

    pub(crate) fn enqueue(&self, request: RuntimeEnrichmentRequest) {
        let mut queue = self
            .state
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.sequence = queue.sequence.wrapping_add(1).max(1);
        let generation = queue.sequence;
        let key = request_key(&request);
        if queue.latest_generation.len() >= MAX_REGISTRY_ENTRIES
            && !queue.latest_generation.contains_key(&key)
        {
            let evict = queue.jobs.keys().next().cloned();
            if let Some(key) = evict {
                queue.jobs.remove(&key);
                queue.latest_generation.remove(&key);
                queue.failure_counts.remove(&key);
            }
        }
        if let Some(cancelled) = queue.active_cancellations.get(&key) {
            cancelled.store(true, Ordering::Release);
        }
        let previous_job = queue.jobs.remove(&key);
        let failure_count = previous_job
            .as_ref()
            .map(|job| job.failure_count)
            .or_else(|| queue.failure_counts.get(&key).copied())
            .unwrap_or_default();
        let next_attempt_at = if request.priority == RuntimeEnrichmentPriority::Final {
            Instant::now() + Duration::from_millis(200)
        } else {
            previous_job
                .map(|job| job.next_attempt_at)
                .unwrap_or_else(|| Instant::now() + Duration::from_millis(200))
        };
        queue.latest_generation.insert(key.clone(), generation);
        queue.jobs.insert(
            key,
            PendingJob {
                request,
                generation,
                next_attempt_at,
                failure_count,
            },
        );
        self.state.wake.notify_all();
    }
}

fn request_key(request: &RuntimeEnrichmentRequest) -> (String, RuntimeEnrichmentLookupKey) {
    (
        request.target_runtime_id.clone(),
        RuntimeEnrichmentLookupKey {
            scope: request.runtime_scope.clone(),
            agent_kind: request.agent_kind,
            session_id: request.agent_session_id.clone(),
        },
    )
}

fn worker_loop(state: Arc<ProducerInner>) {
    loop {
        let (key, job, cancelled) = {
            let mut queue = state
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            loop {
                let now = Instant::now();
                let selected = select_ready_job(&queue, now);
                if let Some((key, job)) = selected {
                    queue.jobs.remove(&key);
                    queue.active_agents.insert(job.request.agent_kind);
                    let cancelled = Arc::new(AtomicBool::new(false));
                    queue
                        .active_cancellations
                        .insert(key.clone(), cancelled.clone());
                    break (key, job, cancelled);
                }
                let wait = queue
                    .jobs
                    .values()
                    .map(|job| job.next_attempt_at.saturating_duration_since(now))
                    .min()
                    .unwrap_or(Duration::from_secs(60));
                let (guard, _) = state
                    .wake
                    .wait_timeout(queue, wait.min(Duration::from_secs(2)))
                    .unwrap_or_else(|error| error.into_inner());
                queue = guard;
            }
        };
        let previous = {
            let queue = state
                .queue
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            queue.cursors.get(&key.1).cloned()
        };
        let result = lookup_with_outer_timeout(
            job.request.clone(),
            cancelled.clone(),
            previous,
            LOOKUP_OUTER_TIMEOUT,
        );
        let mut queue = state
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.active_agents.remove(&job.request.agent_kind);
        if queue
            .active_cancellations
            .get(&key)
            .is_some_and(|active| Arc::ptr_eq(active, &cancelled))
        {
            queue.active_cancellations.remove(&key);
        }
        let current_generation = queue.latest_generation.get(&key).copied();
        if current_generation != Some(job.generation) {
            state.wake.notify_all();
            continue;
        }
        match result {
            Ok(SessionMetadataLookupResult::Ready { snapshot, cursor }) => {
                if let Some(cursor) = cursor {
                    queue.cursors.insert(key.1.clone(), cursor);
                    if queue.cursors.len() > MAX_REGISTRY_ENTRIES {
                        if let Some(oldest) = queue.cursors.keys().next().cloned() {
                            queue.cursors.remove(&oldest);
                        }
                    }
                }
                let enrichment = RuntimeEnrichment {
                    source: super::reducer::RuntimeEnrichmentSource::SessionAdapter,
                    title: snapshot.title,
                    model: snapshot.model,
                    workdir: snapshot.workdir,
                    source_last_activity_at: snapshot.last_activity_at,
                    source_revision: Some(snapshot.source_revision),
                };
                let commit = state
                    .repository
                    .append_enrichment_if_current(&job.request, enrichment);
                match commit {
                    Ok(updated) => {
                        queue.latest_generation.remove(&key);
                        queue.failure_counts.remove(&key);
                        drop(queue);
                        (state.emit)(updated);
                    }
                    Err(_) => {
                        schedule_retry(&mut queue, key, job);
                        state.wake.notify_all();
                    }
                }
            }
            Ok(SessionMetadataLookupResult::Pending { cursor, .. }) => {
                queue.cursors.insert(key.1.clone(), cursor);
                trim_cursors(&mut queue);
                schedule_retry(&mut queue, key, job);
                state.wake.notify_all();
            }
            Ok(SessionMetadataLookupResult::NotReady) | Err(_) => {
                schedule_retry(&mut queue, key, job);
                state.wake.notify_all();
            }
        }
    }
}

fn select_ready_job(
    queue: &ProducerQueue,
    now: Instant,
) -> Option<((String, RuntimeEnrichmentLookupKey), PendingJob)> {
    queue
        .jobs
        .iter()
        .filter(|(_, job)| {
            job.next_attempt_at <= now && !queue.active_agents.contains(&job.request.agent_kind)
        })
        .max_by_key(|(_, job)| (job.request.priority, Reverse(job.next_attempt_at)))
        .map(|(key, job)| (key.clone(), job.clone()))
}

fn trim_cursors(queue: &mut ProducerQueue) {
    if queue.cursors.len() > MAX_REGISTRY_ENTRIES {
        if let Some(oldest) = queue.cursors.keys().next().cloned() {
            queue.cursors.remove(&oldest);
        }
    }
}

fn schedule_retry(
    queue: &mut ProducerQueue,
    key: (String, RuntimeEnrichmentLookupKey),
    mut job: PendingJob,
) {
    job.failure_count = job.failure_count.saturating_add(1);
    let backoff = match job.failure_count {
        0 | 1 => 1,
        2 => 5,
        3 => 30,
        _ => 120,
    };
    job.next_attempt_at = Instant::now() + Duration::from_secs(backoff);
    queue.failure_counts.insert(key.clone(), job.failure_count);
    queue.jobs.insert(key, job);
}

fn lookup_with_outer_timeout(
    request: RuntimeEnrichmentRequest,
    cancelled: Arc<AtomicBool>,
    previous: Option<SessionMetadataCursor>,
    timeout: Duration,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    let worker_cancelled = cancelled.clone();
    run_lookup_task(cancelled, timeout, move || {
        lookup_one(&request, worker_cancelled, previous.as_ref())
    })
}

fn run_lookup_task<F>(
    cancelled: Arc<AtomicBool>,
    timeout: Duration,
    lookup: F,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError>
where
    F: FnOnce() -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("balancehub-agent-metadata-lookup".to_string())
        .spawn(move || {
            let result = lookup();
            let _ = sender.send(result);
        });
    if spawned.is_err() {
        return Err(SessionMetadataLookupError::Io(
            "无法启动 Agent metadata lookup".to_string(),
        ));
    }
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::Release);
            Err(SessionMetadataLookupError::TimedOut)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(SessionMetadataLookupError::Parse(
            "Agent metadata lookup 异常退出".to_string(),
        )),
    }
}

fn lookup_one(
    request: &RuntimeEnrichmentRequest,
    cancelled: Arc<AtomicBool>,
    previous: Option<&SessionMetadataCursor>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
    let adapter = agent_cli::definition(request.agent_kind)
        .sessions()
        .ok_or_else(|| {
            SessionMetadataLookupError::Unsupported("当前 Agent CLI 无会话适配器".into())
        })?;
    if !adapter.supports_metadata() {
        return Err(SessionMetadataLookupError::Unsupported(
            "当前 Agent CLI 不支持定向会话元数据读取".into(),
        ));
    }
    adapter.lookup_metadata(
        crate::services::agent_cli::contracts::SessionMetadataLookupRequest {
            cli_kind: request.agent_kind,
            session_id: &request.agent_session_id,
            workdir: request.workdir.as_deref(),
            transcript_path_hint: request.transcript_path_hint.as_deref(),
            previous,
            budget: default_budget(cancelled),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(runtime_id: &str, agent_kind: AgentCliKind) -> RuntimeEnrichmentRequest {
        RuntimeEnrichmentRequest {
            trigger_event_id: format!("event-{runtime_id}"),
            target_runtime_id: runtime_id.to_string(),
            runtime_scope: AgentRuntimeScope::Native,
            agent_kind,
            agent_session_id: format!("session-{runtime_id}"),
            workdir: Some(PathBuf::from("/workspace")),
            transcript_path_hint: None,
            trigger_observed_at: 1,
            priority: RuntimeEnrichmentPriority::Normal,
        }
    }

    fn queue_with_jobs(jobs: Vec<PendingJob>) -> ProducerQueue {
        let mut queue = ProducerQueue {
            jobs: HashMap::new(),
            latest_generation: HashMap::new(),
            active_agents: BTreeSet::new(),
            active_cancellations: HashMap::new(),
            failure_counts: HashMap::new(),
            cursors: HashMap::new(),
            sequence: 0,
        };
        for job in jobs {
            let key = request_key(&job.request);
            queue.latest_generation.insert(key.clone(), job.generation);
            queue.jobs.insert(key, job);
        }
        queue
    }

    #[test]
    fn scheduler_prefers_final_then_oldest_and_serializes_each_agent() {
        let now = Instant::now();
        let oldest = PendingJob {
            request: request("oldest", AgentCliKind::Codex),
            generation: 1,
            next_attempt_at: now - Duration::from_secs(2),
            failure_count: 0,
        };
        let newer = PendingJob {
            request: request("newer", AgentCliKind::Gemini),
            generation: 2,
            next_attempt_at: now - Duration::from_secs(1),
            failure_count: 0,
        };
        let mut queue = queue_with_jobs(vec![oldest, newer]);
        assert_eq!(
            select_ready_job(&queue, now)
                .unwrap()
                .1
                .request
                .target_runtime_id,
            "oldest"
        );

        queue
            .jobs
            .get_mut(&request_key(&request("newer", AgentCliKind::Gemini)))
            .unwrap()
            .request
            .priority = RuntimeEnrichmentPriority::Final;
        assert_eq!(
            select_ready_job(&queue, now)
                .unwrap()
                .1
                .request
                .target_runtime_id,
            "newer"
        );

        queue.active_agents.insert(AgentCliKind::Gemini);
        assert_eq!(
            select_ready_job(&queue, now)
                .unwrap()
                .1
                .request
                .target_runtime_id,
            "oldest"
        );
        assert_eq!(WORKER_COUNT, 2);
    }

    #[test]
    fn outer_timeout_sets_cooperative_cancellation_and_drops_late_result() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let result = run_lookup_task(cancelled.clone(), Duration::from_millis(20), move || {
            while !worker_cancelled.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            Ok(SessionMetadataLookupResult::NotReady)
        });
        assert_eq!(result, Err(SessionMetadataLookupError::TimedOut));
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn cursor_registry_is_bounded() {
        let mut queue = queue_with_jobs(Vec::new());
        for index in 0..=MAX_REGISTRY_ENTRIES {
            queue.cursors.insert(
                RuntimeEnrichmentLookupKey {
                    scope: AgentRuntimeScope::Native,
                    agent_kind: AgentCliKind::Codex,
                    session_id: format!("session-{index}"),
                },
                SessionMetadataCursor {
                    source_identity: format!("source-{index}"),
                    source_len: 0,
                    next_offset: 0,
                    parser_version: 1,
                    opaque_state: Vec::new(),
                },
            );
            trim_cursors(&mut queue);
        }
        assert_eq!(queue.cursors.len(), MAX_REGISTRY_ENTRIES);
    }
}
