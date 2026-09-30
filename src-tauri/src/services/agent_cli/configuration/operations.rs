//! Background transaction lifetime, single-use plans, cancellation and results.
use super::{
    contracts::ConfigurationValidationRequest,
    errors::{conflict, internal},
    service::{at_after, digest},
    sources::ConfigurationSourceAuthority,
    writer::{CommitResult, StagedFile},
    ConfigurationService, EDIT_TTL,
};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        environment::mutation::{
            locking::{shared_locks, LockFailure},
            GuardedFile,
        },
    },
};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(super) struct PlannedFile {
    pub source: Arc<ConfigurationSourceAuthority>,
    pub text: String,
}
pub(super) struct PrivatePlan {
    pub edit_id: String,
    pub kind: AgentCliKind,
    pub files: Vec<PlannedFile>,
    pub dependencies: Vec<GuardedFile>,
    pub reload_hints: Vec<String>,
}
impl PrivatePlan {
    pub fn retained_bytes(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.text.len() + file.source.file.bytes().map_or(0, <[u8]>::len))
            .sum::<usize>()
            + self
                .dependencies
                .iter()
                .map(|file| file.bytes().map_or(0, <[u8]>::len))
                .sum::<usize>()
    }
    fn domains(&self) -> Vec<String> {
        let mut domains = BTreeSet::new();
        for file in &self.files {
            domains.extend(file.source.file.lock_domains());
            domains.insert(format!("config:{}", file.source.context.config_root));
        }
        for file in &self.dependencies {
            domains.extend(file.lock_domains());
        }
        domains.into_iter().collect()
    }
}
pub(super) struct OperationCell {
    pub actor: String,
    pub created: Instant,
    pub public: Mutex<AgentConfigurationOperation>,
    pub plan: Mutex<Option<PrivatePlan>>,
    pub canceled: AtomicBool,
    pub committed: AtomicBool,
    pub native_mutated: AtomicBool,
    running: AtomicBool,
    bytes: AtomicUsize,
    pub fingerprints: Vec<super::receipts::FileFingerprint>,
}
impl OperationCell {
    pub fn is_complete(&self) -> bool {
        self.public
            .lock()
            .map(|value| value.phase == AgentAssetOperationPhase::Completed)
            .unwrap_or(false)
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }
    fn snapshot(&self) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
        self.public
            .lock()
            .map(|value| value.clone())
            .map_err(|_| internal())
    }
    fn checkpoint(&self, deadline: Instant) -> Result<(), AgentConfigurationError> {
        if self.canceled.load(Ordering::Acquire) && !self.committed.load(Ordering::Acquire) {
            Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::Canceled,
            ))
        } else if Instant::now() >= deadline {
            Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::Timeout,
            ))
        } else {
            Ok(())
        }
    }
}
impl ConfigurationService {
    pub(crate) fn start(
        &self,
        actor: &str,
        request: AgentConfigurationApplyRequest,
    ) -> Result<(AgentConfigurationOperation, bool), AgentConfigurationError> {
        let mut state = self.state.lock().map_err(|_| internal())?;
        if let Some((owner, edit_id, id, _)) = state.consumed.get(&request.plan_token) {
            if owner != actor {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::ActorMismatch,
                ));
            }
            if edit_id != &request.edit_id {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::InvalidRequest,
                ));
            }
            return Ok((
                state
                    .operations
                    .get(id)
                    .ok_or_else(|| {
                        AgentConfigurationError::new(AgentConfigurationErrorKind::OperationNotFound)
                    })?
                    .snapshot()?,
                false,
            ));
        }
        let record = state.plans.get(&request.plan_token).ok_or_else(|| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::PlanExpired)
        })?;
        if record.actor != actor {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ActorMismatch,
            ));
        }
        if record.plan.edit_id != request.edit_id {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::InvalidRequest,
            ));
        }
        if record.expires <= Instant::now() {
            state.plans.remove(&request.plan_token);
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::PlanExpired,
            ));
        }
        if state
            .operations
            .values()
            .filter(|cell| !cell.is_complete())
            .count()
            >= 32
        {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::CapacityExceeded,
            ));
        }
        let record = state
            .plans
            .remove(&request.plan_token)
            .ok_or_else(internal)?;
        let plan = record.plan;
        let id = crate::services::agent_cli::environment::mutation::token::opaque_id()
            .map_err(|_| internal())?;
        let now = at_after(Duration::ZERO);
        let public = AgentConfigurationOperation {
            id: id.clone(),
            edit_id: request.edit_id.clone(),
            agent_kind: plan.kind,
            source_ids: plan
                .files
                .iter()
                .map(|file| file.source.source.source_id.clone())
                .collect(),
            revision: 1,
            phase: AgentAssetOperationPhase::Preparing,
            outcome: None,
            can_cancel: true,
            created_at: now.clone(),
            updated_at: now,
            message: Some("配置保存已提交后台".to_owned()),
            files: plan
                .files
                .iter()
                .map(|file| AgentConfigurationFileResult {
                    source_id: file.source.source.source_id.clone(),
                    state: AgentConfigurationFileState::Unchanged,
                    message: None,
                })
                .collect(),
            reload_hints: plan.reload_hints.clone(),
        };
        let fingerprints = plan
            .files
            .iter()
            .map(|file| super::receipts::FileFingerprint {
                source_id: file.source.source.source_id.clone(),
                before: file.source.file.bytes().map(digest),
                after: digest(file.text.as_bytes()),
            })
            .collect();
        let bytes = plan.retained_bytes();
        let cell = Arc::new(OperationCell {
            actor: actor.to_owned(),
            created: Instant::now(),
            public: Mutex::new(public.clone()),
            plan: Mutex::new(Some(plan)),
            canceled: AtomicBool::new(false),
            committed: AtomicBool::new(false),
            native_mutated: AtomicBool::new(false),
            running: AtomicBool::new(false),
            bytes: AtomicUsize::new(bytes),
            fingerprints,
        });
        state.edits.remove(&request.edit_id);
        state
            .plans
            .retain(|_, record| record.plan.edit_id != request.edit_id);
        state.consumed.insert(
            request.plan_token,
            (
                actor.to_owned(),
                request.edit_id,
                id.clone(),
                Instant::now() + EDIT_TTL,
            ),
        );
        state.operations.insert(id, Arc::clone(&cell));
        drop(state);
        Ok((public, true))
    }
    pub(crate) fn get_operation(
        &self,
        id: &str,
    ) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
        if let Some(cell) = self
            .state
            .lock()
            .map_err(|_| internal())?
            .operations
            .get(id)
            .cloned()
        {
            return cell.snapshot();
        }
        if let Some(operation) = self
            .state
            .lock()
            .map_err(|_| internal())?
            .recovered
            .get(id)
            .cloned()
        {
            return Ok(operation);
        }
        self.cache_recovered_receipts(&self.receipts.load())?;
        self.state
            .lock()
            .map_err(|_| internal())?
            .recovered
            .get(id)
            .cloned()
            .ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::OperationNotFound)
            })
    }
    pub(crate) fn list_operations(
        &self,
    ) -> Result<Vec<AgentConfigurationOperation>, AgentConfigurationError> {
        self.cache_recovered_receipts(&self.receipts.load())?;
        let state = self.state.lock().map_err(|_| internal())?;
        let mut output = state.recovered.clone();
        for (id, cell) in &state.operations {
            output.insert(id.clone(), cell.snapshot()?);
        }
        drop(state);
        let mut output = output.into_values().collect::<Vec<_>>();
        output.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        output.truncate(128);
        Ok(output)
    }
    pub(crate) fn cancel_operation(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<AgentConfigurationOperation, AgentConfigurationError> {
        let cell = self
            .state
            .lock()
            .map_err(|_| internal())?
            .operations
            .get(id)
            .cloned()
            .ok_or_else(|| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::OperationNotFound)
            })?;
        if cell.actor != actor {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::ActorMismatch,
            ));
        }
        let mut public = cell.public.lock().map_err(|_| internal())?;
        if public.can_cancel && !cell.committed.load(Ordering::Acquire) {
            cell.canceled.store(true, Ordering::Release);
            public.can_cancel = false;
            public.message = Some("正在取消配置保存".to_owned());
            public.revision += 1;
            public.updated_at = at_after(Duration::ZERO);
        }
        Ok(public.clone())
    }
    pub(crate) fn run_operation(&self, id: &str, emit: impl Fn(AgentConfigurationOperation)) {
        let cell = match self
            .state
            .lock()
            .ok()
            .and_then(|state| state.operations.get(id).cloned())
        {
            Some(cell) => cell,
            None => return,
        };
        if cell.is_complete() || cell.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut plan = match cell.plan.lock().ok().and_then(|mut plan| plan.take()) {
            Some(plan) => plan,
            None => {
                self.finish(
                    &cell,
                    AgentAssetOperationOutcome::UnchangedFailure,
                    "配置保存计划不可用，未再次执行",
                );
                return;
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute(&cell, &mut plan, &emit)
        }))
        .unwrap_or_else(|_| Err(internal()));
        match result {
            Ok(verified) => self.finish(
                &cell,
                if verified {
                    AgentAssetOperationOutcome::AppliedVerified
                } else {
                    AgentAssetOperationOutcome::AppliedUnverified
                },
                if verified {
                    "配置已写入并完成文件核验"
                } else {
                    "配置已写入，持久化或文件核验未全部完成"
                },
            ),
            Err(error) => {
                // A commit attempt is not evidence of mutation. Inspect only
                // original guarded targets; never retry or restore old bytes.
                if cell.committed.load(Ordering::Acquire) {
                    for (index, file) in plan.files.iter().enumerate() {
                        let unchanged = cell
                            .snapshot()
                            .ok()
                            .and_then(|operation| {
                                operation.files.get(index).map(|file| {
                                    file.state == AgentConfigurationFileState::Unchanged
                                })
                            })
                            .unwrap_or(false);
                        if unchanged && file.source.file.observe_write() != crate::services::agent_cli::environment::mutation::WriteObservation::Unchanged {
                            let _=self.file_result(&cell,index,AgentConfigurationFileState::Unknown,Some("文件结果待重新读取核对".to_owned()));
                        }
                    }
                }
                let public = cell.snapshot().ok();
                let applied = cell.native_mutated.load(Ordering::Acquire)
                    || public.as_ref().is_some_and(|operation| {
                        operation
                            .files
                            .iter()
                            .any(|file| file.state == AgentConfigurationFileState::Applied)
                    });
                let unknown = public.as_ref().is_some_and(|operation| {
                    operation
                        .files
                        .iter()
                        .any(|file| file.state == AgentConfigurationFileState::Unknown)
                });
                let outcome = if applied {
                    AgentAssetOperationOutcome::AppliedUnverified
                } else if cell.committed.load(Ordering::Acquire) && unknown {
                    AgentAssetOperationOutcome::OutcomeUnknown
                } else {
                    match error.kind {
                        AgentConfigurationErrorKind::Canceled => {
                            AgentAssetOperationOutcome::CanceledBeforeCommit
                        }
                        AgentConfigurationErrorKind::SourceChanged
                        | AgentConfigurationErrorKind::RootChanged => {
                            AgentAssetOperationOutcome::UnchangedConflict
                        }
                        _ => AgentAssetOperationOutcome::UnchangedFailure,
                    }
                };
                self.finish(&cell, outcome, &error.message);
            }
        }
        cell.bytes.store(0, Ordering::Release);
        if let Ok(snapshot) = cell.snapshot() {
            emit(snapshot);
        }
    }
    fn execute(
        &self,
        cell: &OperationCell,
        plan: &mut PrivatePlan,
        emit: &impl Fn(AgentConfigurationOperation),
    ) -> Result<bool, AgentConfigurationError> {
        cell.checkpoint(Instant::now() + Duration::from_secs(15))?;
        self.receipts.save(cell)?;
        let locks = shared_locks();
        self.phase(cell, AgentAssetOperationPhase::WaitingForLock, emit)?;
        let _locked = locks
            .acquire(
                &plan.domains(),
                &cell.canceled,
                Instant::now() + Duration::from_secs(15),
            )
            .map_err(|failure| {
                AgentConfigurationError::new(match failure {
                    LockFailure::Canceled => AgentConfigurationErrorKind::Canceled,
                    LockFailure::TimedOut => AgentConfigurationErrorKind::Timeout,
                    LockFailure::Internal => AgentConfigurationErrorKind::InternalFailure,
                })
            })?;
        self.phase(cell, AgentAssetOperationPhase::Revalidating, emit)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        for dependency in &plan.dependencies {
            cell.checkpoint(deadline)?;
            dependency.revalidate().map_err(|_| conflict())?;
        }
        for file in &plan.files {
            cell.checkpoint(deadline)?;
            file.source.file.revalidate().map_err(|_| conflict())?;
            self.revalidate_context(&file.source)?;
            let validation = (agent_cli::definition(plan.kind).configuration().validate)(
                ConfigurationValidationRequest {
                    source: &file.source.spec,
                    after: &file.text,
                },
            )?;
            if validation.diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == AgentConfigurationDiagnosticSeverity::Error
            }) {
                return Err(AgentConfigurationError::new(
                    AgentConfigurationErrorKind::UnsupportedScope,
                ));
            }
        }
        // Staging and fsync complete for all candidates before any target write.
        let mut staged = plan
            .files
            .iter()
            .map(|file| {
                StagedFile::prepare(&file.source.file, file.text.as_bytes(), || {
                    cell.checkpoint(deadline)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.phase(cell, AgentAssetOperationPhase::Applying, emit)?;
        for index in 0..staged.len() {
            staged[index].ensure_parent(
                || self.commit_boundary(cell, deadline, emit),
                || cell.native_mutated.store(true, Ordering::Release),
            )?;
            let parent = staged[index]
                .target_parent()
                .map(std::path::Path::to_path_buf)
                .ok_or_else(internal)?;
            for other in &mut staged {
                other.advance_after_creation(&parent)?;
            }
            for dependency in &mut plan.dependencies {
                dependency
                    .reanchor_after_parent_creation(&parent)
                    .map_err(|_| conflict())?;
            }
        }
        for dependency in &plan.dependencies {
            dependency.revalidate().map_err(|_| conflict())?;
        }
        let mut verified = true;
        for (index, stage) in staged.iter_mut().enumerate() {
            if !cell.committed.load(Ordering::Acquire) {
                cell.checkpoint(deadline)?;
            }
            match stage.commit(|| self.commit_boundary(cell, deadline, emit)) {
                Ok(result) => {
                    let state = match result {
                        CommitResult::Unchanged => AgentConfigurationFileState::Unchanged,
                        CommitResult::Applied => {
                            cell.native_mutated.store(true, Ordering::Release);
                            AgentConfigurationFileState::Applied
                        }
                        #[cfg(unix)]
                        CommitResult::AppliedNotSynced => {
                            cell.native_mutated.store(true, Ordering::Release);
                            verified = false;
                            AgentConfigurationFileState::Applied
                        }
                    };
                    self.file_result(cell, index, state, None)?;
                }
                Err(error) => {
                    let observed = plan.files[index].source.file.observe_write();
                    let state=match observed{crate::services::agent_cli::environment::mutation::WriteObservation::Unchanged=>AgentConfigurationFileState::Unchanged,crate::services::agent_cli::environment::mutation::WriteObservation::Changed|crate::services::agent_cli::environment::mutation::WriteObservation::Unknown=>AgentConfigurationFileState::Unknown};
                    self.file_result(cell, index, state, Some(error.message.clone()))?;
                    return Err(error);
                }
            }
            if let Ok(snapshot) = cell.snapshot() {
                emit(snapshot);
            }
        }
        self.phase(cell, AgentAssetOperationPhase::Verifying, emit)?;
        let verify_deadline = Instant::now() + Duration::from_secs(15);
        for (index, stage) in staged.iter().enumerate() {
            if Instant::now() >= verify_deadline || stage.verify().is_err() {
                verified = false;
                self.file_result(
                    cell,
                    index,
                    AgentConfigurationFileState::Unknown,
                    Some("文件核验未完成，请重新读取核对".to_owned()),
                )?;
            }
        }
        Ok(verified)
    }
    fn commit_boundary(
        &self,
        cell: &OperationCell,
        deadline: Instant,
        emit: &impl Fn(AgentConfigurationOperation),
    ) -> Result<(), AgentConfigurationError> {
        // The same mutex orders cancellation and the first irreversible step.
        let mut public = cell.public.lock().map_err(|_| internal())?;
        cell.checkpoint(deadline)?;
        let first = !cell.committed.swap(true, Ordering::AcqRel);
        if first {
            public.can_cancel = false;
            public.revision += 1;
            public.updated_at = at_after(Duration::ZERO);
            public.message = Some("正在写入配置文件".to_owned());
        }
        let snapshot = public.clone();
        drop(public);
        // Durable intent precedes the first native mutation. If it fails, no
        // native write is attempted and result classification remains unchanged.
        if first {
            self.receipts.save(cell)?;
            emit(snapshot);
        }
        Ok(())
    }
    fn phase(
        &self,
        cell: &OperationCell,
        phase: AgentAssetOperationPhase,
        emit: &impl Fn(AgentConfigurationOperation),
    ) -> Result<(), AgentConfigurationError> {
        {
            let mut public = cell.public.lock().map_err(|_| internal())?;
            public.phase = phase;
            public.revision += 1;
            public.updated_at = at_after(Duration::ZERO);
        }
        self.receipts.save(cell)?;
        emit(cell.snapshot()?);
        Ok(())
    }
    fn file_result(
        &self,
        cell: &OperationCell,
        index: usize,
        state: AgentConfigurationFileState,
        message: Option<String>,
    ) -> Result<(), AgentConfigurationError> {
        {
            let mut public = cell.public.lock().map_err(|_| internal())?;
            let file = public.files.get_mut(index).ok_or_else(internal)?;
            file.state = state;
            file.message = message;
            public.revision += 1;
            public.updated_at = at_after(Duration::ZERO);
        }
        self.receipts.save(cell)
    }
    fn finish(&self, cell: &OperationCell, outcome: AgentAssetOperationOutcome, message: &str) {
        if let Ok(mut public) = cell.public.lock() {
            public.phase = AgentAssetOperationPhase::Completed;
            public.can_cancel = false;
            public.outcome = Some(outcome);
            public.message = Some(message.to_owned());
            public.revision += 1;
            public.updated_at = at_after(Duration::ZERO);
        }
        if let Ok(mut plan) = cell.plan.lock() {
            plan.take();
        }
        cell.bytes.store(0, Ordering::Release);
        if self.receipts.save(cell).is_err() {
            if let Ok(mut public) = cell.public.lock() {
                if public.outcome == Some(AgentAssetOperationOutcome::AppliedVerified) {
                    public.outcome = Some(AgentAssetOperationOutcome::AppliedUnverified);
                }
                let unknown = public
                    .files
                    .iter()
                    .any(|file| file.state == AgentConfigurationFileState::Unknown);
                let applied = public
                    .files
                    .iter()
                    .any(|file| file.state == AgentConfigurationFileState::Applied);
                public.message =
                    Some(
                        if public.outcome == Some(AgentAssetOperationOutcome::OutcomeUnknown)
                            || unknown
                        {
                            "配置保存结果尚未确定，操作记录未完成持久化；请重新读取核对"
                        } else if public.outcome
                            == Some(AgentAssetOperationOutcome::AppliedUnverified)
                            || applied
                        {
                            "配置保存已产生变更，但操作记录未完成持久化；请重新读取核对"
                        } else {
                            "配置文件未写入，操作记录未完成持久化"
                        }
                        .to_owned(),
                    );
                public.revision += 1;
                public.updated_at = at_after(Duration::ZERO);
            }
        }
    }
}
