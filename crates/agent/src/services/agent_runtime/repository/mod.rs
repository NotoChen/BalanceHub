//! Durable repository for the unified Agent runtime projection.
//!
//! The repository is deliberately the only owner of the durable runtime event
//! history.  A refresh reads a bounded Hook batch and the launch repository,
//! rebuilds the projection with the shared reducer, and commits the new
//! history before acknowledging any Hook files.

mod storage;

use super::{
    enrichment::{RuntimeEnrichmentPriority, RuntimeEnrichmentRequest},
    hook::{HookSpoolRepository, SpoolDiagnostic, DEFAULT_SPOOL_BATCH_SIZE},
    launcher::event_from_temporary_cli,
    reducer::{resolve_runtime_id, AgentRuntimeEvent, AgentRuntimeEventKind, RuntimeEnrichment},
};
use crate::models::{
    AgentRuntimeOrigin, AgentRuntimeSession, AgentRuntimeState, TemporaryCliInstance,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use storage::{
    append_unique_event, ensure_directory_chain, reduce_events, reject_symlink_or_non_directory,
    snapshot_from_stored, trim_event_history,
};

pub const RUNTIME_REPOSITORY_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_RUNTIME_EVENT_HISTORY: usize = 8_192;
pub const MAX_RUNTIME_PROJECTION_BYTES: usize = 8 * 1024 * 1024;
pub const EXTERNAL_RUNTIME_STALE_AFTER_MILLIS: i64 = 30 * 60 * 1000;
const RUNTIME_REPOSITORY_DIR: &str = "agent-runtime-v1";
const RUNTIME_PROJECTION_FILE: &str = "projection.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeRepositoryError {
    InvalidRoot,
    UnsafePath,
    Unavailable,
    Io,
    CorruptProjection,
    UnsupportedSchema(u16),
    Serialization,
    Spool(SpoolDiagnostic),
}

impl std::fmt::Display for AgentRuntimeRepositoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot => formatter.write_str("运行时 repository 根目录无效"),
            Self::UnsafePath => formatter.write_str("运行时 repository 路径不安全"),
            Self::Unavailable => formatter.write_str("运行时 repository 不可用"),
            Self::Io => formatter.write_str("运行时 repository 文件操作失败"),
            Self::CorruptProjection => formatter.write_str("运行时 projection 格式无效"),
            Self::UnsupportedSchema(version) => {
                write!(
                    formatter,
                    "运行时 projection schemaVersion {version} 不受支持"
                )
            }
            Self::Serialization => formatter.write_str("运行时 projection 序列化失败"),
            Self::Spool(diagnostic) => write!(formatter, "Hook spool 操作失败: {diagnostic:?}"),
        }
    }
}

impl std::error::Error for AgentRuntimeRepositoryError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeSnapshot {
    pub schema_version: u16,
    pub revision: u64,
    pub updated_at: i64,
    pub sessions: Vec<AgentRuntimeSession>,
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeRefreshOutcome {
    pub snapshot: AgentRuntimeSnapshot,
    pub enrichment_requests: Vec<RuntimeEnrichmentRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRuntimeProjection {
    schema_version: u16,
    revision: u64,
    updated_at: i64,
    events: Vec<AgentRuntimeEvent>,
}

impl Default for StoredRuntimeProjection {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_REPOSITORY_SCHEMA_VERSION,
            revision: 0,
            updated_at: 0,
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeRepository {
    projection_path: PathBuf,
    spool: HookSpoolRepository,
    lock: Arc<Mutex<()>>,
    max_event_history: usize,
}

impl AgentRuntimeRepository {
    /// Creates a repository below an application-owned, injectable root.
    ///
    /// The root is canonicalized once and all repository paths are created
    /// below it.  This makes test repositories independent from the installed
    /// app and prevents a caller from accidentally pointing at a symlink.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, AgentRuntimeRepositoryError> {
        Self::with_history_limit(root, DEFAULT_RUNTIME_EVENT_HISTORY)
    }

    pub fn with_history_limit(
        root: impl AsRef<Path>,
        max_event_history: usize,
    ) -> Result<Self, AgentRuntimeRepositoryError> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(AgentRuntimeRepositoryError::InvalidRoot);
        }
        fs::create_dir_all(root).map_err(|_| AgentRuntimeRepositoryError::Unavailable)?;
        reject_symlink_or_non_directory(root)?;
        let root = fs::canonicalize(root).map_err(|_| AgentRuntimeRepositoryError::InvalidRoot)?;
        let repository_dir = root.join(RUNTIME_REPOSITORY_DIR);
        fs::create_dir_all(&repository_dir)
            .map_err(|_| AgentRuntimeRepositoryError::Unavailable)?;
        ensure_directory_chain(&repository_dir)?;
        let spool = HookSpoolRepository::new(&root).map_err(AgentRuntimeRepositoryError::Spool)?;
        Ok(Self {
            projection_path: repository_dir.join(RUNTIME_PROJECTION_FILE),
            spool,
            lock: Arc::new(Mutex::new(())),
            max_event_history: max_event_history.max(1),
        })
    }

    /// Reads the last committed projection without consuming incoming Hook
    /// files.  Missing state is a valid empty projection.
    pub fn snapshot(&self) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let stored = self.read_stored_projection()?;
        Ok(snapshot_from_stored(&stored))
    }

    /// Consumes one bounded Hook batch and reconciles launch snapshots found
    /// below the injected root.
    /// Same as [`refresh`], but accepts launch records from the existing
    /// launcher service.  This keeps the repository testable and lets callers
    /// use their existing launcher source while the old public domain remains.
    #[cfg(test)]
    pub fn refresh_with_launch_snapshots(
        &self,
        launch_snapshots: &[TemporaryCliInstance],
    ) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError> {
        Ok(self
            .refresh_with_launch_snapshots_outcome_at(
                launch_snapshots,
                chrono::Utc::now().timestamp_millis(),
            )?
            .snapshot)
    }

    #[cfg(test)]
    pub fn refresh_with_launch_snapshots_at(
        &self,
        launch_snapshots: &[TemporaryCliInstance],
        observed_at: i64,
    ) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError> {
        Ok(self
            .refresh_with_launch_snapshots_outcome_at(launch_snapshots, observed_at)?
            .snapshot)
    }

    pub fn refresh_with_launch_snapshots_outcome_at(
        &self,
        launch_snapshots: &[TemporaryCliInstance],
        observed_at: i64,
    ) -> Result<AgentRuntimeRefreshOutcome, AgentRuntimeRepositoryError> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let stored = self.read_stored_projection()?;
        let known_event_ids = stored
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<BTreeSet<_>>();
        let batch = self
            .spool
            .read_batch(DEFAULT_SPOOL_BATCH_SIZE, &known_event_ids)
            .map_err(AgentRuntimeRepositoryError::Spool)?;

        let mut events = stored.events.clone();
        let mut event_ids = known_event_ids;
        let mut changed = false;
        for instance in launch_snapshots {
            let event = event_from_temporary_cli(instance, observed_at);
            changed |= append_unique_event(&mut events, &mut event_ids, event);
        }
        let mut enrichment_requests = Vec::new();
        for record in &batch.events {
            if let Some(request) = enrichment_request_for(&record.event) {
                enrichment_requests.push(request);
            }
            for event in record.event.clone().into_runtime_events() {
                changed |= append_unique_event(&mut events, &mut event_ids, event);
            }
        }
        changed |= append_external_timeout_events(&mut events, &mut event_ids, observed_at);
        if !changed {
            self.spool
                .acknowledge(&batch)
                .map_err(AgentRuntimeRepositoryError::Spool)?;
            return Ok(AgentRuntimeRefreshOutcome {
                snapshot: snapshot_from_stored(&StoredRuntimeProjection {
                    schema_version: RUNTIME_REPOSITORY_SCHEMA_VERSION,
                    revision: stored.revision,
                    updated_at: stored.updated_at,
                    events,
                }),
                enrichment_requests,
            });
        }
        trim_event_history(&mut events, self.max_event_history);

        let candidate = StoredRuntimeProjection {
            schema_version: RUNTIME_REPOSITORY_SCHEMA_VERSION,
            revision: stored.revision.saturating_add(1),
            updated_at: observed_at,
            events,
        };
        // Ack happens strictly after this atomic commit.  Any write or
        // serialization error therefore leaves incoming Hook files intact.
        self.write_stored_projection(&candidate)?;
        self.spool
            .acknowledge(&batch)
            .map_err(AgentRuntimeRepositoryError::Spool)?;
        Ok(AgentRuntimeRefreshOutcome {
            snapshot: snapshot_from_stored(&candidate),
            enrichment_requests,
        })
    }

    /// Conditionally persists one successful adapter result. The target is
    /// revalidated under the repository lock so a resumed/replaced runtime
    /// cannot receive a stale worker result.
    pub fn append_enrichment_if_current(
        &self,
        request: &RuntimeEnrichmentRequest,
        enrichment: RuntimeEnrichment,
    ) -> Result<AgentRuntimeSnapshot, AgentRuntimeRepositoryError> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let stored = self.read_stored_projection()?;
        let current = reduce_events(&stored.events);
        let Some(session) = current.sessions().find(|session| {
            session.runtime_id == request.target_runtime_id
                && session.runtime_scope == request.runtime_scope
                && session.agent_kind == request.agent_kind
                && session.agent_session_id.as_deref() == Some(request.agent_session_id.as_str())
        }) else {
            return Ok(snapshot_from_stored(&stored));
        };
        let latest_hook_evidence =
            session.evidence.iter().rev().find(|evidence| {
                evidence.source == crate::models::AgentRuntimeEvidenceSource::Hook
            });
        let is_recovery = request.trigger_event_id.starts_with("recovery:");
        let stale_request = if is_recovery {
            latest_hook_evidence
                .is_some_and(|evidence| evidence.observed_at > request.trigger_observed_at)
        } else {
            latest_hook_evidence.is_some_and(|evidence| {
                evidence.event_id != request.trigger_event_id
                    && evidence.event_id != format!("{}:enrichment", request.trigger_event_id)
            })
        };
        if stale_request {
            return Ok(snapshot_from_stored(&stored));
        }
        let changes = enrichment
            .title
            .as_deref()
            .is_some_and(|value| session.title.as_deref() != Some(value))
            || enrichment
                .model
                .as_deref()
                .is_some_and(|value| session.model.as_deref() != Some(value))
            || enrichment
                .workdir
                .as_deref()
                .is_some_and(|value| session.workdir.as_deref() != Some(value))
            || enrichment.source_last_activity_at.is_some_and(|value| {
                session
                    .last_activity_at
                    .is_none_or(|current| value > current)
            });
        if !changes {
            return Ok(snapshot_from_stored(&stored));
        }
        let event_id = enrichment
            .source_revision
            .as_deref()
            .map(|revision| format!("enrichment:{}:{revision}", request.target_runtime_id))
            .unwrap_or_else(|| {
                format!(
                    "enrichment:{}:{}",
                    request.target_runtime_id, request.trigger_event_id
                )
            });
        let event = AgentRuntimeEvent {
            event_id,
            runtime_id: Some(request.target_runtime_id.clone()),
            balancehub_instance_id: session.balancehub_instance_id.clone(),
            agent_session_id: Some(request.agent_session_id.clone()),
            runtime_scope: request.runtime_scope.clone(),
            origin: session.origin,
            agent_kind: request.agent_kind,
            observed_at: request.trigger_observed_at,
            kind: AgentRuntimeEventKind::Enrichment(enrichment),
        };
        let mut events = stored.events.clone();
        let mut ids = events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<BTreeSet<_>>();
        if !append_unique_event(&mut events, &mut ids, event) {
            return Ok(snapshot_from_stored(&stored));
        }
        trim_event_history(&mut events, self.max_event_history);
        let candidate = StoredRuntimeProjection {
            schema_version: RUNTIME_REPOSITORY_SCHEMA_VERSION,
            revision: stored.revision.saturating_add(1),
            updated_at: stored.updated_at,
            events,
        };
        self.write_stored_projection(&candidate)?;
        Ok(snapshot_from_stored(&candidate))
    }
}

fn enrichment_request_for(
    event: &super::hook::event::NormalizedHookEvent,
) -> Option<RuntimeEnrichmentRequest> {
    let session_id = event
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())?
        .to_string();
    let target_runtime_id = resolve_runtime_id(&event.clone().into_runtime_event());
    Some(RuntimeEnrichmentRequest {
        trigger_event_id: event.event_id.clone(),
        target_runtime_id,
        runtime_scope: event.runtime_scope.clone(),
        agent_kind: event.agent_kind,
        agent_session_id: session_id,
        workdir: event.cwd.as_deref().map(PathBuf::from),
        transcript_path_hint: event.transcript_path.as_deref().map(PathBuf::from),
        trigger_observed_at: event.observed_at,
        priority: match event.lifecycle {
            super::hook::event::NormalizedHookLifecycle::Ended
            | super::hook::event::NormalizedHookLifecycle::Idle => RuntimeEnrichmentPriority::Final,
            _ => RuntimeEnrichmentPriority::Normal,
        },
    })
}

fn append_external_timeout_events(
    events: &mut Vec<AgentRuntimeEvent>,
    event_ids: &mut BTreeSet<String>,
    observed_at: i64,
) -> bool {
    let current = reduce_events(events);
    let stale_before = observed_at.saturating_sub(EXTERNAL_RUNTIME_STALE_AFTER_MILLIS);
    let mut changed = false;
    let stale_sessions = current
        .sessions()
        .filter(|session| {
            session.origin == AgentRuntimeOrigin::ExternalHook
                && !matches!(
                    session.state,
                    AgentRuntimeState::Ended | AgentRuntimeState::Unknown
                )
                && session
                    .state_observed_at
                    .is_some_and(|last_activity| last_activity <= stale_before)
        })
        .cloned()
        .collect::<Vec<_>>();
    for session in stale_sessions {
        let last_activity = session.state_observed_at.unwrap_or_default();
        changed |= append_unique_event(
            events,
            event_ids,
            AgentRuntimeEvent {
                event_id: format!("external-timeout:{}:{last_activity}", session.runtime_id),
                runtime_id: Some(session.runtime_id),
                balancehub_instance_id: session.balancehub_instance_id,
                agent_session_id: session.agent_session_id,
                runtime_scope: session.runtime_scope,
                origin: AgentRuntimeOrigin::ExternalHook,
                agent_kind: session.agent_kind,
                observed_at,
                kind: AgentRuntimeEventKind::ExternalTimeout,
            },
        );
    }
    changed
}

#[cfg(test)]
mod tests;
