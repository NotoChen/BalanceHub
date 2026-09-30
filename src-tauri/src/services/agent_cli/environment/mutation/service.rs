use super::super::{
    run::{MonotonicClock, SystemMonotonicClock},
    snapshot::now_string,
};
#[cfg(unix)]
use super::atomic::AtomicWriteResult;
use super::{
    atomic,
    execution::CliApplyEvidence,
    files::{conflict, WriteObservation},
    locking::{shared_locks, LockFailure, MutationLocks},
    prepared::{
        prepare_request, MutationExecution, MutationInspector, MutationVerification,
        PreparedMutation,
    },
    token::{opaque_id, PlanRegistry, DEFAULT_PLAN_TTL},
};
use crate::models::*;
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_ACTIVE_OPERATIONS: usize = 8;
const MAX_RETAINED_OPERATIONS: usize = 128;
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

struct CanonicalPlan {
    request: AgentAssetPlanRequest,
    prepared: PreparedMutation,
    inspector: Arc<dyn MutationInspector>,
    display: AgentAssetPlan,
}

struct OperationState {
    public: AgentAssetOperation,
    started: bool,
    committed: bool,
    verifying_again: bool,
}

struct OperationCell {
    actor: String,
    state: Mutex<OperationState>,
    canceled: AtomicBool,
    plan: Arc<CanonicalPlan>,
}

pub(crate) struct MutationService {
    plans: PlanRegistry<Arc<CanonicalPlan>>,
    operations: Mutex<BTreeMap<String, Arc<OperationCell>>>,
    locks: Arc<MutationLocks>,
    clock: Arc<dyn MonotonicClock>,
}

impl Default for MutationService {
    fn default() -> Self {
        Self {
            plans: PlanRegistry::default(),
            operations: Mutex::new(BTreeMap::new()),
            locks: shared_locks(),
            clock: Arc::new(SystemMonotonicClock),
        }
    }
}

impl MutationService {
    pub(crate) fn domain_locks(&self) -> Arc<MutationLocks> {
        Arc::clone(&self.locks)
    }

    #[cfg(test)]
    pub(crate) fn plan(
        &self,
        actor: &str,
        request: AgentAssetPlanRequest,
        inspector: Arc<dyn MutationInspector>,
    ) -> Result<AgentAssetPlan, AgentAssetMutationError> {
        self.plan_internal(actor, request, inspector, None, None)
    }

    pub(crate) fn plan_pinned(
        &self,
        actor: &str,
        request: AgentAssetPlanRequest,
        inspector: Arc<dyn MutationInspector>,
        expected_signature: &str,
    ) -> Result<AgentAssetPlan, AgentAssetMutationError> {
        self.plan_internal(actor, request, inspector, Some(expected_signature), None)
    }

    pub(crate) fn plan_read(
        &self,
        actor: &str,
        request: AgentAssetPlanRequest,
        inspector: Arc<dyn MutationInspector>,
        canceled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<AgentAssetPlan, AgentAssetMutationError> {
        self.plan_internal(actor, request, inspector, None, Some(canceled))
    }

    fn plan_internal(
        &self,
        actor: &str,
        request: AgentAssetPlanRequest,
        inspector: Arc<dyn MutationInspector>,
        expected_signature: Option<&str>,
        canceled: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<AgentAssetPlan, AgentAssetMutationError> {
        if actor.is_empty()
            || request.workspace.as_deref().map(std::path::Path::new) != inspector.workspace()
        {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::InvalidRequest,
            ));
        }
        let check = || {
            if canceled
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
            {
                Err(AgentAssetMutationError::new(
                    AgentAssetMutationErrorKind::PreparationFailed,
                ))
            } else {
                Ok(())
            }
        };
        check()?;
        let snapshot = match &canceled {
            Some(flag) => inspector.inspect_for_read(Arc::clone(flag))?,
            None => inspector.inspect()?,
        };
        check()?;
        let (prepared, mut display) = prepare_request(inspector.as_ref(), &snapshot, &request)?;
        if expected_signature.is_some_and(|expected| expected != prepared.signature) {
            return Err(conflict());
        }
        check()?;
        let now = self.clock.now();
        display.expires_at = (chrono::Local::now()
            + chrono::Duration::seconds(DEFAULT_PLAN_TTL.as_secs() as i64))
        .to_rfc3339();
        let value = Arc::new(CanonicalPlan {
            request: request.clone(),
            prepared,
            inspector,
            display: display.clone(),
        });
        display.token = self.plans.issue(
            actor,
            &request.asset_id,
            request.action,
            value,
            now,
            DEFAULT_PLAN_TTL,
        )?;
        Ok(display)
    }

    /// Returns immediately after atomically consuming the confirmed plan. The
    /// command layer schedules run_operation on a blocking worker.
    pub(crate) fn start(
        &self,
        actor: &str,
        request: &AgentAssetApplyRequest,
    ) -> Result<AgentAssetOperation, AgentAssetMutationError> {
        let mut operations = self.operations.lock().map_err(|_| internal_error())?;
        let active = operations
            .values()
            .filter(|cell| {
                cell.state.lock().map_or(true, |state| {
                    state.public.phase != AgentAssetOperationPhase::Completed
                })
            })
            .count();
        if active >= MAX_ACTIVE_OPERATIONS {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::CapacityExceeded,
            ));
        }
        let bound = self.plans.consume(actor, request, self.clock.now())?;
        let id = opaque_id()?;
        let now = now_string();
        let operation = AgentAssetOperation {
            id: id.clone(),
            asset_id: bound.asset_id,
            action: bound.action,
            phase: AgentAssetOperationPhase::Preparing,
            can_cancel: true,
            revision: 1,
            created_at: now.clone(),
            updated_at: now,
            outcome: None,
            message: None,
            affected_asset_ids: bound.value.prepared.affected_asset_ids.clone(),
            reload_effect: bound.value.display.reload_effect.clone(),
        };
        operations.insert(
            id,
            Arc::new(OperationCell {
                actor: bound.actor,
                canceled: AtomicBool::new(false),
                plan: bound.value,
                state: Mutex::new(OperationState {
                    public: operation.clone(),
                    started: false,
                    committed: false,
                    verifying_again: false,
                }),
            }),
        );
        prune_operations(&mut operations);
        Ok(operation)
    }

    pub(crate) fn run_operation(&self, operation_id: &str) {
        let Some(cell) = self
            .operations
            .lock()
            .ok()
            .and_then(|operations| operations.get(operation_id).cloned())
        else {
            return;
        };
        {
            let Ok(mut state) = cell.state.lock() else {
                return;
            };
            if state.started || state.public.phase == AgentAssetOperationPhase::Completed {
                return;
            }
            state.started = true;
        }
        if catch_unwind(AssertUnwindSafe(|| self.run(&cell))).is_err() {
            let committed = cell.state.lock().map_or(true, |state| state.committed);
            cell.complete(if committed {
                AgentAssetOperationOutcome::OutcomeUnknown
            } else {
                AgentAssetOperationOutcome::UnchangedFailure
            });
        }
    }

    fn run(&self, cell: &OperationCell) {
        use AgentAssetOperationOutcome as Outcome;
        cell.phase(AgentAssetOperationPhase::WaitingForLock);
        let _guard = match self.locks.acquire(
            &cell.plan.prepared.domains,
            &cell.canceled,
            Instant::now() + LOCK_TIMEOUT,
        ) {
            Ok(guard) => guard,
            Err(LockFailure::Canceled) => {
                cell.complete(Outcome::CanceledBeforeCommit);
                return;
            }
            Err(_) => {
                cell.complete(Outcome::UnchangedFailure);
                return;
            }
        };
        cell.phase(AgentAssetOperationPhase::Revalidating);
        if cell.canceled.load(Ordering::Acquire) {
            cell.complete(Outcome::CanceledBeforeCommit);
            return;
        }
        let fresh = match cell.plan.inspector.inspect() {
            Ok(snapshot) => snapshot,
            Err(_) => {
                cell.complete(Outcome::UnchangedFailure);
                return;
            }
        };
        let new_plan = prepare_request(cell.plan.inspector.as_ref(), &fresh, &cell.plan.request);
        let matches =
            new_plan.is_ok_and(|(prepared, _)| prepared.signature == cell.plan.prepared.signature);
        if !matches || cell.plan.prepared.revalidate().is_err() {
            cell.complete(Outcome::UnchangedConflict);
            return;
        }
        if cell.canceled.load(Ordering::Acquire) {
            cell.complete(Outcome::CanceledBeforeCommit);
            return;
        }

        let prepared = &cell.plan.prepared;
        let apply = match &prepared.execution {
            MutationExecution::AtomicFile {
                source_id,
                replacement,
            } => {
                let Some(source) = prepared
                    .files
                    .iter()
                    .find(|file| &file.source_id == source_id)
                else {
                    cell.complete(Outcome::UnchangedFailure);
                    return;
                };
                match atomic::replace(source, replacement, || {
                    prepared.revalidate()?;
                    cell.commit_boundary()
                }) {
                    #[cfg(unix)]
                    Ok(AtomicWriteResult::Unchanged) => ApplyEvidence::Noop,
                    #[cfg(unix)]
                    Ok(AtomicWriteResult::Replaced) => ApplyEvidence::FileReplaced,
                    #[cfg(unix)]
                    Ok(AtomicWriteResult::ReplacedNotSynced) => {
                        ApplyEvidence::FileReplacedNotSynced
                    }
                    Err(error) => {
                        if cell.canceled.load(Ordering::Acquire) && !cell.committed() {
                            cell.complete(Outcome::CanceledBeforeCommit);
                            return;
                        }
                        if !cell.committed()
                            && error.kind == AgentAssetMutationErrorKind::SourceConflict
                        {
                            cell.complete(Outcome::UnchangedConflict);
                            return;
                        }
                        ApplyEvidence::KnownFailure
                    }
                }
            }
            MutationExecution::ExactCli(command) => {
                let (evidence, _) = command.run(
                    cell.plan.inspector.settings(),
                    || prepared.revalidate(),
                    || cell.commit_boundary(),
                );
                if !cell.committed() {
                    cell.complete(if cell.canceled.load(Ordering::Acquire) {
                        Outcome::CanceledBeforeCommit
                    } else if evidence == CliApplyEvidence::RejectedBeforeSpawn {
                        Outcome::UnchangedConflict
                    } else {
                        Outcome::UnchangedFailure
                    });
                    return;
                }
                match evidence {
                    CliApplyEvidence::ExitedSuccessfully => ApplyEvidence::CliSucceeded,
                    CliApplyEvidence::TimedOut => ApplyEvidence::Uncertain,
                    _ => ApplyEvidence::KnownFailure,
                }
            }
        };
        if !cell.committed() && cell.canceled.load(Ordering::Acquire) {
            cell.complete(Outcome::CanceledBeforeCommit);
            return;
        }
        cell.phase(AgentAssetOperationPhase::Verifying);
        // Exactly one bounded native inspection after the commit boundary,
        // including failures/timeouts. It never retries or restores old bytes.
        let (verified, observation) = inspect_outcome(cell);
        cell.complete(classify_outcome(apply, verified, observation));
    }

    pub(crate) fn operation(
        &self,
        actor: &str,
        operation_id: &str,
    ) -> Result<AgentAssetOperation, AgentAssetMutationError> {
        let cell = self.cell(actor, operation_id)?;
        let result = cell
            .state
            .lock()
            .map_err(|_| internal_error())?
            .public
            .clone();
        Ok(result)
    }

    pub(crate) fn operations(&self, actor: &str) -> Vec<AgentAssetOperation> {
        let Ok(operations) = self.operations.lock() else {
            return Vec::new();
        };
        let mut result: Vec<_> = operations
            .values()
            .filter(|cell| cell.actor == actor)
            .filter_map(|cell| cell.state.lock().ok().map(|state| state.public.clone()))
            .collect();
        result.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        result
    }

    pub(crate) fn cancel(
        &self,
        actor: &str,
        operation_id: &str,
    ) -> Result<AgentAssetOperation, AgentAssetMutationError> {
        let cell = self.cell(actor, operation_id)?;
        let mut state = cell.state.lock().map_err(|_| internal_error())?;
        if state.public.can_cancel && !state.committed {
            cell.canceled.store(true, Ordering::Release);
            state.public.can_cancel = false;
            state.public.revision += 1;
            state.public.updated_at = now_string();
            if !state.started {
                state.public.phase = AgentAssetOperationPhase::Completed;
                state.public.outcome = Some(AgentAssetOperationOutcome::CanceledBeforeCommit);
                state.public.message =
                    Some(outcome_message(AgentAssetOperationOutcome::CanceledBeforeCommit).into());
            }
        }
        Ok(state.public.clone())
    }

    pub(crate) fn verify_operation(
        &self,
        actor: &str,
        operation_id: &str,
    ) -> Result<AgentAssetOperation, AgentAssetMutationError> {
        let cell = self.cell(actor, operation_id)?;
        {
            let mut state = cell.state.lock().map_err(|_| internal_error())?;
            if state.verifying_again
                || !matches!(
                    state.public.outcome,
                    Some(
                        AgentAssetOperationOutcome::AppliedUnverified
                            | AgentAssetOperationOutcome::OutcomeUnknown
                    )
                )
            {
                return Ok(state.public.clone());
            }
            state.verifying_again = true;
            state.public.phase = AgentAssetOperationPhase::Verifying;
            state.public.revision += 1;
            state.public.updated_at = now_string();
            state.public.can_cancel = false;
        }
        let canceled = AtomicBool::new(false);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = self
                .locks
                .acquire(
                    &cell.plan.prepared.domains,
                    &canceled,
                    Instant::now() + LOCK_TIMEOUT,
                )
                .ok()?;
            Some(inspect_outcome(&cell))
        }))
        .ok()
        .flatten();
        let outcome = match result {
            Some((true, _)) => AgentAssetOperationOutcome::AppliedVerified,
            Some((false, WriteObservation::Changed)) => {
                AgentAssetOperationOutcome::AppliedUnverified
            }
            _ => AgentAssetOperationOutcome::OutcomeUnknown,
        };
        cell.complete(outcome);
        self.operation(actor, operation_id)
    }

    pub(crate) fn remove_actor(&self, actor: &str) {
        self.plans.remove_actor(actor);
    }

    fn cell(&self, actor: &str, id: &str) -> Result<Arc<OperationCell>, AgentAssetMutationError> {
        let operations = self.operations.lock().map_err(|_| internal_error())?;
        let cell = operations.get(id).ok_or_else(|| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::OperationNotFound)
        })?;
        if cell.actor != actor {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::ActorMismatch,
            ));
        }
        Ok(cell.clone())
    }
}

impl OperationCell {
    fn phase(&self, phase: AgentAssetOperationPhase) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.public.phase = phase;
        state.public.can_cancel = !state.committed
            && !self.canceled.load(Ordering::Acquire)
            && !matches!(
                phase,
                AgentAssetOperationPhase::Verifying | AgentAssetOperationPhase::Completed
            );
        state.public.revision += 1;
        state.public.updated_at = now_string();
    }

    fn commit_boundary(&self) -> Result<(), AgentAssetMutationError> {
        let mut state = self.state.lock().map_err(|_| internal_error())?;
        if self.canceled.load(Ordering::Acquire) {
            return Err(conflict());
        }
        state.committed = true;
        state.public.phase = AgentAssetOperationPhase::Applying;
        state.public.can_cancel = false;
        state.public.revision += 1;
        state.public.updated_at = now_string();
        Ok(())
    }

    fn committed(&self) -> bool {
        self.state.lock().map_or(true, |state| state.committed)
    }

    fn complete(&self, outcome: AgentAssetOperationOutcome) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.public.phase = AgentAssetOperationPhase::Completed;
        state.public.outcome = Some(outcome);
        state.public.can_cancel = false;
        state.public.message = Some(outcome_message(outcome).into());
        state.public.revision += 1;
        state.public.updated_at = now_string();
        state.verifying_again = false;
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ApplyEvidence {
    #[cfg(unix)]
    Noop,
    #[cfg(unix)]
    FileReplaced,
    #[cfg(unix)]
    FileReplacedNotSynced,
    CliSucceeded,
    KnownFailure,
    Uncertain,
}

fn inspect_outcome(cell: &OperationCell) -> (bool, WriteObservation) {
    let prepared = &cell.plan.prepared;
    let verified = cell.plan.inspector.inspect().ok().is_some_and(|snapshot| {
        (prepared.verify)(MutationVerification {
            inventory: &snapshot.inventory,
            asset_id: &cell.plan.request.asset_id,
            action: cell.plan.request.action,
            affected_asset_ids: &prepared.affected_asset_ids,
        })
    });
    let observations: Vec<_> = prepared
        .files
        .iter()
        .filter(|file| prepared.write_source_ids.contains(&file.source_id))
        .map(|file| file.observe_write())
        .collect();
    let observation = if observations.contains(&WriteObservation::Unknown) {
        WriteObservation::Unknown
    } else if observations.contains(&WriteObservation::Changed) {
        WriteObservation::Changed
    } else {
        WriteObservation::Unchanged
    };
    (verified, observation)
}

pub(crate) fn classify_outcome(
    apply: ApplyEvidence,
    verified: bool,
    observation: WriteObservation,
) -> AgentAssetOperationOutcome {
    use AgentAssetOperationOutcome::*;
    if verified {
        #[cfg(unix)]
        if matches!(apply, ApplyEvidence::FileReplacedNotSynced) {
            return AppliedUnverified;
        }
        return AppliedVerified;
    }
    match (apply, observation) {
        #[cfg(unix)]
        (ApplyEvidence::FileReplaced | ApplyEvidence::FileReplacedNotSynced, _) => {
            AppliedUnverified
        }
        (_, WriteObservation::Changed) => AppliedUnverified,
        (ApplyEvidence::KnownFailure, WriteObservation::Unchanged) => UnchangedFailure,
        _ => OutcomeUnknown,
    }
}

fn outcome_message(outcome: AgentAssetOperationOutcome) -> &'static str {
    use AgentAssetOperationOutcome::*;
    match outcome {
        CanceledBeforeCommit => "已取消，未修改配置",
        UnchangedConflict => "配置已变化，未执行修改，请刷新后重新确认",
        UnchangedFailure => "操作未完成，未执行配置修改或已验证目标文件保持原状",
        AppliedVerified => "目标配置状态已验证；运行中的会话可能需要重新加载",
        AppliedUnverified => "配置已发生修改，但目标状态尚未完整验证，请重新检查",
        OutcomeUnknown => "无法确定操作结果，请重新检查配置；不会自动重试",
    }
}

fn internal_error() -> AgentAssetMutationError {
    AgentAssetMutationError::new(AgentAssetMutationErrorKind::InternalFailure)
}

fn prune_operations(operations: &mut BTreeMap<String, Arc<OperationCell>>) {
    while operations.len() > MAX_RETAINED_OPERATIONS {
        let oldest = operations
            .iter()
            .filter_map(|(id, cell)| {
                cell.state.lock().ok().and_then(|state| {
                    (state.public.phase == AgentAssetOperationPhase::Completed)
                        .then(|| (id.clone(), state.public.updated_at.clone()))
                })
            })
            .min_by(|a, b| a.1.cmp(&b.1))
            .map(|(id, _)| id);
        let Some(id) = oldest else {
            break;
        };
        operations.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::super::test_support::{TestExecution, TestInspector};
    use super::*;
    #[test]
    fn timeout_cannot_claim_unchanged_and_verified_state_overrules_exit_code() {
        use AgentAssetOperationOutcome::*;
        assert_eq!(
            classify_outcome(ApplyEvidence::Uncertain, false, WriteObservation::Unchanged),
            OutcomeUnknown
        );
        assert_eq!(
            classify_outcome(ApplyEvidence::Uncertain, false, WriteObservation::Changed),
            AppliedUnverified
        );
        assert_eq!(
            classify_outcome(ApplyEvidence::Uncertain, true, WriteObservation::Changed),
            AppliedVerified
        );
        assert_eq!(
            classify_outcome(
                ApplyEvidence::KnownFailure,
                false,
                WriteObservation::Unchanged
            ),
            UnchangedFailure
        );
        assert_eq!(
            classify_outcome(
                ApplyEvidence::KnownFailure,
                true,
                WriteObservation::Unchanged
            ),
            AppliedVerified
        );
        #[cfg(unix)]
        assert_eq!(
            classify_outcome(
                ApplyEvidence::FileReplaced,
                false,
                WriteObservation::Unknown
            ),
            AppliedUnverified
        );
    }

    #[cfg(unix)]
    fn start(
        service: &MutationService,
        inspector: Arc<TestInspector>,
        action: AgentAssetActionKind,
    ) -> (AgentAssetPlan, AgentAssetOperation) {
        let plan = service
            .plan("window-a", inspector.request(action), inspector)
            .unwrap();
        let request = AgentAssetApplyRequest {
            plan_token: plan.token.clone(),
            asset_id: plan.asset_id.clone(),
            action,
        };
        let operation = service.start("window-a", &request).unwrap();
        assert!(service.start("window-a", &request).is_err());
        (plan, operation)
    }

    #[cfg(unix)]
    #[test]
    fn confirmed_apply_returns_before_work_and_keeps_source_bytes_out_of_public_state() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        let service = MutationService::default();
        let before = std::fs::read(inspector.config()).unwrap();
        let (plan, operation) = start(&service, inspector.clone(), AgentAssetActionKind::Disable);
        assert_eq!(operation.phase, AgentAssetOperationPhase::Preparing);
        assert!(operation.can_cancel);
        assert_eq!(std::fs::read(inspector.config()).unwrap(), before);
        assert!(service.operation("window-b", &operation.id).is_err());
        service.run_operation(&operation.id);
        let completed = service.operation("window-a", &operation.id).unwrap();
        assert_eq!(
            completed.outcome,
            Some(AgentAssetOperationOutcome::AppliedVerified)
        );
        assert!(!completed.can_cancel);
        assert!(completed.revision > operation.revision);
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 3);
        for value in [
            serde_json::to_string(&plan).unwrap(),
            serde_json::to_string(&completed).unwrap(),
        ] {
            assert!(!value.contains("private-fixture-value"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn changed_source_consumes_plan_and_finishes_without_overwriting_external_edit() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        let service = MutationService::default();
        let (_, operation) = start(&service, inspector.clone(), AgentAssetActionKind::Disable);
        std::fs::write(inspector.config(), "external = true\n").unwrap();
        service.run_operation(&operation.id);
        assert_eq!(
            service
                .operation("window-a", &operation.id)
                .unwrap()
                .outcome,
            Some(AgentAssetOperationOutcome::UnchangedConflict)
        );
        assert_eq!(
            std::fs::read_to_string(inspector.config()).unwrap(),
            "external = true\n"
        );
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 2);
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_while_waiting_for_shared_source_lock_finishes_before_commit() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        let service = Arc::new(MutationService::default());
        let (_, operation) = start(&service, inspector.clone(), AgentAssetActionKind::Disable);
        let canceled = AtomicBool::new(false);
        let guard = service
            .locks
            .acquire(
                &[inspector.file().domain()],
                &canceled,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| service.run_operation(&operation.id));
            let deadline = Instant::now() + Duration::from_secs(2);
            while service.operation("window-a", &operation.id).unwrap().phase
                != AgentAssetOperationPhase::WaitingForLock
            {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(
                !service
                    .cancel("window-a", &operation.id)
                    .unwrap()
                    .can_cancel
            );
            drop(guard);
            worker.join().unwrap();
        });
        assert_eq!(
            service
                .operation("window-a", &operation.id)
                .unwrap()
                .outcome,
            Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
        );
        assert_eq!(
            std::fs::read_to_string(inspector.config()).unwrap(),
            super::super::test_support::CONFIG
        );
    }

    #[cfg(unix)]
    #[test]
    fn two_confirmed_plans_for_same_file_serialize_and_second_detects_conflict() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        let first = MutationService::default();
        let second = MutationService::default();
        assert!(Arc::ptr_eq(&first.locks, &second.locks));
        let (_, operation_a) = start(&first, inspector.clone(), AgentAssetActionKind::Disable);
        let (_, operation_b) = start(&second, inspector, AgentAssetActionKind::Disable);
        std::thread::scope(|scope| {
            scope.spawn(|| first.run_operation(&operation_a.id));
            scope.spawn(|| second.run_operation(&operation_b.id));
        });
        let outcomes = [
            first
                .operation("window-a", &operation_a.id)
                .unwrap()
                .outcome,
            second
                .operation("window-a", &operation_b.id)
                .unwrap()
                .outcome,
        ];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == Some(AgentAssetOperationOutcome::AppliedVerified))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == Some(AgentAssetOperationOutcome::UnchangedConflict))
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_postcommit_inspection_can_be_verified_again_without_reapplying() {
        use std::os::unix::fs::MetadataExt;
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        inspector.failure_at.store(3, Ordering::SeqCst);
        let service = MutationService::default();
        let (_, operation) = start(&service, inspector.clone(), AgentAssetActionKind::Disable);
        service.run_operation(&operation.id);
        assert_eq!(
            service
                .operation("window-a", &operation.id)
                .unwrap()
                .outcome,
            Some(AgentAssetOperationOutcome::AppliedUnverified)
        );
        let inode = std::fs::metadata(inspector.config()).unwrap().ino();
        let bytes = std::fs::read(inspector.config()).unwrap();
        assert_eq!(
            service
                .verify_operation("window-a", &operation.id)
                .unwrap()
                .outcome,
            Some(AgentAssetOperationOutcome::AppliedVerified)
        );
        assert_eq!(std::fs::metadata(inspector.config()).unwrap().ino(), inode);
        assert_eq!(std::fs::read(inspector.config()).unwrap(), bytes);
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 4);
    }

    #[cfg(unix)]
    #[test]
    fn native_already_enabled_exit_one_is_verified_and_raw_stderr_is_discarded() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        std::fs::write(
            &inspector.executable,
            "#!/bin/sh\nprintf 'private-fixture-value' >&2\nexit 1\n",
        )
        .unwrap();
        *inspector.execution.lock().unwrap() = TestExecution::Cli(vec!["already-enabled".into()]);
        let service = MutationService::default();
        let (_, operation) = start(&service, inspector, AgentAssetActionKind::Enable);
        service.run_operation(&operation.id);
        let completed = service.operation("window-a", &operation.id).unwrap();
        assert_eq!(
            completed.outcome,
            Some(AgentAssetOperationOutcome::AppliedVerified)
        );
        assert!(!serde_json::to_string(&completed)
            .unwrap()
            .contains("private-fixture-value"));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_after_native_spawn_cannot_claim_no_write() {
        let temporary = tempfile::tempdir().unwrap();
        let inspector = Arc::new(TestInspector::new(temporary.path()));
        std::fs::write(&inspector.executable, "#!/bin/sh\nbh_target=\"$1\"\nsleep 0.2\nprintf \"enabled = false\\nfixture_secret = 'private-fixture-value'\\n\" > \"$bh_target\"\n").unwrap();
        *inspector.execution.lock().unwrap() =
            TestExecution::Cli(vec![inspector.config().to_string_lossy().into_owned()]);
        let service = MutationService::default();
        let (_, operation) = start(&service, inspector, AgentAssetActionKind::Disable);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| service.run_operation(&operation.id));
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let current = service.operation("window-a", &operation.id).unwrap();
                if matches!(
                    current.phase,
                    AgentAssetOperationPhase::Applying
                        | AgentAssetOperationPhase::Verifying
                        | AgentAssetOperationPhase::Completed
                ) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(
                !service
                    .cancel("window-a", &operation.id)
                    .unwrap()
                    .can_cancel
            );
            worker.join().unwrap();
        });
        assert_eq!(
            service
                .operation("window-a", &operation.id)
                .unwrap()
                .outcome,
            Some(AgentAssetOperationOutcome::AppliedVerified)
        );
    }
}
