use super::journal::{Journal, Record};
use super::planning::{LifecycleContext, LifecyclePlanner, NativePlanner, PreparedLifecycle};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        execution::CliApplyEvidence,
        locking::{LockFailure, MutationLocks},
        token::{opaque_id, PlanRegistry, DEFAULT_PLAN_TTL},
    },
};
use chrono::Local;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_ACTIVE_OPERATIONS: usize = 8;
const MAX_OPERATION_HISTORY: usize = 128;
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct LifecycleService {
    locks: Arc<MutationLocks>,
    planner: Arc<dyn LifecyclePlanner>,
    plans: PlanRegistry<Arc<PreparedLifecycle>, (AgentCliKind, AgentLifecycleActionKind)>,
    operations: Mutex<BTreeMap<String, Arc<OperationCell>>>,
    journal: Arc<Journal>,
    history: Mutex<Result<Vec<Record>, String>>,
}

struct OperationCell {
    journal: Arc<Journal>,
    actor: String,
    plan: Arc<PreparedLifecycle>,
    canceled: AtomicBool,
    state: Mutex<OperationState>,
}

struct OperationState {
    public: AgentLifecycleOperation,
    started: bool,
    committed: bool,
}

impl LifecycleService {
    pub(crate) fn new(locks: Arc<MutationLocks>) -> Self {
        Self::with_planner(locks, Arc::new(NativePlanner))
    }

    pub(super) fn with_planner(
        locks: Arc<MutationLocks>,
        planner: Arc<dyn LifecyclePlanner>,
    ) -> Self {
        let journal = Arc::new(Journal::new());
        let history = Mutex::new(journal.load().map(|mut records| {
            for record in &mut records {
                record.operation.recovered = true;
            }
            records
        }));
        Self {
            locks,
            planner,
            plans: PlanRegistry::default(),
            operations: Mutex::default(),
            journal,
            history,
        }
    }

    pub(crate) async fn catalog(
        &self,
        settings: AppSettings,
        home: PathBuf,
        request: AgentLifecycleCatalogRequest,
    ) -> Result<AgentLifecycleCatalog, AgentLifecycleError> {
        self.planner
            .catalog(self.context(settings, home), request)
            .await
    }

    pub(crate) async fn plan(
        &self,
        actor: &str,
        settings: AppSettings,
        home: PathBuf,
        request: AgentLifecyclePlanRequest,
    ) -> Result<AgentLifecyclePlan, AgentLifecycleError> {
        if actor.is_empty()
            || request.target_id.is_empty()
            || request.target_id.len() > 256
            || request.expected_evidence_revision.is_empty()
            || request.expected_evidence_revision.len() > 256
        {
            return Err(AgentLifecycleError::new(
                AgentLifecycleErrorKind::InvalidRequest,
            ));
        }
        let prepared = self
            .planner
            .plan(self.context(settings, home), request.clone())
            .await?;
        let mut public = prepared.public.clone();
        let now = Instant::now();
        public.expires_at = (Local::now()
            + chrono::Duration::seconds(DEFAULT_PLAN_TTL.as_secs() as i64))
        .to_rfc3339();
        public.plan_token = self
            .plans
            .issue(
                actor,
                &request.target_id,
                (request.agent_kind, request.action),
                Arc::new(prepared),
                now,
                DEFAULT_PLAN_TTL,
            )
            .map_err(map_mutation_error)?;
        Ok(public)
    }

    /// Allocates and journals the task on the IPC worker before it may execute.
    pub(crate) fn start(
        &self,
        actor: &str,
        request: &AgentLifecycleApplyRequest,
    ) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
        if self.history.lock().map_err(|_| internal())?.is_err() {
            return Err(internal());
        }
        let mut operations = self.operations.lock().map_err(|_| internal())?;
        let active = operations
            .values()
            .filter(|cell| cell.snapshot().outcome.is_none())
            .count();
        if active >= MAX_ACTIVE_OPERATIONS {
            return Err(AgentLifecycleError::new(
                AgentLifecycleErrorKind::CapacityExceeded,
            ));
        }
        let bound = self
            .plans
            .consume_bound(
                actor,
                &request.plan_token,
                &request.target_id,
                &(request.agent_kind, request.action),
                Instant::now(),
            )
            .map_err(map_mutation_error)?;
        let plan = bound.value;
        let public = &plan.public;
        let timestamp = now_string();
        let operation = AgentLifecycleOperation {
            id: opaque_id().map_err(map_mutation_error)?,
            agent_kind: public.agent_kind,
            target_id: public.target_id.clone(),
            installation_id: public.installation_id.clone(),
            action: public.action,
            channel: public.channel,
            channel_label: public.channel_label.clone(),
            directory: public.directory.clone(),
            from_version: public.from_version.clone(),
            to_version: public.to_version.clone(),
            observed_version: None,
            recovered: false,
            next_launch: None,
            verified_executable_path: None,
            phase: AgentAssetOperationPhase::Preparing,
            can_cancel: true,
            revision: 1,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            outcome: None,
            message: Some("升级任务已加入后台".to_owned()),
            timed_out: false,
            output_truncated: false,
            command_preview: public.command_preview.clone(),
            diagnostics: None,
        };
        self.journal
            .save(&Record {
                operation: operation.clone(),
                installer_started: false,
            })
            .map_err(|_| internal())?;
        operations.insert(
            operation.id.clone(),
            Arc::new(OperationCell {
                journal: Arc::clone(&self.journal),
                actor: actor.to_owned(),
                plan,
                canceled: AtomicBool::new(false),
                state: Mutex::new(OperationState {
                    public: operation.clone(),
                    started: false,
                    committed: false,
                }),
            }),
        );
        while operations.len() > MAX_OPERATION_HISTORY {
            let oldest = operations
                .iter()
                .filter_map(|(id, cell)| {
                    let public = cell.snapshot();
                    public.outcome.map(|_| (id.clone(), public.updated_at))
                })
                .min_by(|left, right| left.1.cmp(&right.1))
                .map(|(id, _)| id);
            let Some(oldest) = oldest else { break };
            if let Err(error) = self.journal.remove(&oldest) {
                eprintln!("Unable to prune upgrade history: {error}");
            }
            operations.remove(&oldest);
        }
        Ok(operation)
    }

    pub(crate) fn run_operation(&self, operation_id: &str) {
        let Some(cell) = self
            .operations
            .lock()
            .ok()
            .and_then(|cells| cells.get(operation_id).cloned())
        else {
            return;
        };
        {
            let mut state = cell.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.started || state.public.outcome.is_some() {
                return;
            }
            state.started = true;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run(&cell)));
        if result.is_err() {
            let committed = cell
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .committed;
            cell.finish(
                if committed {
                    AgentAssetOperationOutcome::OutcomeUnknown
                } else {
                    AgentAssetOperationOutcome::UnchangedFailure
                },
                "升级任务异常，请刷新核对实际安装和版本",
            );
        }
    }

    fn run(&self, cell: &OperationCell) {
        cell.phase(
            AgentAssetOperationPhase::WaitingForLock,
            "等待所选安装的其他操作完成",
        );
        let _guard = match self.locks.acquire(
            &cell.plan.lock_keys,
            &cell.canceled,
            Instant::now() + LOCK_TIMEOUT,
        ) {
            Ok(guard) => guard,
            Err(LockFailure::Canceled) => {
                cell.finish(
                    AgentAssetOperationOutcome::CanceledBeforeCommit,
                    "已在安装程序启动前取消",
                );
                return;
            }
            Err(_) => {
                cell.finish(
                    AgentAssetOperationOutcome::UnchangedFailure,
                    "等待安装锁超时，未开始升级",
                );
                return;
            }
        };
        if cell.canceled.load(Ordering::Acquire) {
            cell.finish(
                AgentAssetOperationOutcome::CanceledBeforeCommit,
                "已在安装程序启动前取消",
            );
            return;
        }
        cell.phase(
            AgentAssetOperationPhase::Revalidating,
            "重新核对安装、目录、渠道与执行程序",
        );
        if let Err(error) = cell.plan.execution.revalidate() {
            cell.finish(
                AgentAssetOperationOutcome::UnchangedConflict,
                &format!("升级前校验失败，未开始写入：{}", error.message),
            );
            return;
        }
        let evidence = cell.plan.execution.run(&mut || cell.commit());
        {
            let mut state = cell.state.lock().unwrap_or_else(|error| error.into_inner());
            state.public.diagnostics = cell.plan.execution.diagnostics();
            state.public.timed_out = evidence.kind == CliApplyEvidence::TimedOut;
            state.public.output_truncated = evidence.output_truncated;
        }

        let committed = cell
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .committed;
        if !committed {
            if cell.canceled.load(Ordering::Acquire) {
                cell.finish(
                    AgentAssetOperationOutcome::CanceledBeforeCommit,
                    "已在安装程序启动前取消",
                );
            } else {
                cell.finish(
                    AgentAssetOperationOutcome::UnchangedFailure,
                    "执行前检查未通过，未开始升级",
                );
            }
            return;
        }
        cell.phase(
            AgentAssetOperationPhase::Verifying,
            "核对升级后的实际可执行文件与版本",
        );
        let verified = cell.plan.execution.verify();
        {
            let mut state = cell.state.lock().unwrap_or_else(|error| error.into_inner());
            state.public.observed_version = verified.version.clone();
            state.public.verified_executable_path = verified.executable_path.clone();
            state.public.timed_out = evidence.kind == CliApplyEvidence::TimedOut;
            state.public.output_truncated = evidence.output_truncated;
        }
        let version_state = crate::services::agent_cli::environment::version_state(
            cell.plan.public.from_version.as_deref(),
            verified.version.as_deref(),
        );
        let acceptable_version = matches!(
            version_state,
            AgentLifecycleVersionState::UpdateAvailable | AgentLifecycleVersionState::UpToDate
        );
        if evidence.kind == CliApplyEvidence::ExitedSuccessfully
            && acceptable_version
            && verified.executable_path.is_some()
        {
            cell.finish(
                AgentAssetOperationOutcome::AppliedVerified,
                if version_state == AgentLifecycleVersionState::UpToDate {
                    "更新命令已完成，当前渠道／版本策略未更新版本；实际版本已核对"
                } else {
                    "升级完成，实际版本已核对"
                },
            );
        } else if verified.changed {
            cell.finish(AgentAssetOperationOutcome::AppliedUnverified, "安装内容已变化，但命令退出结果、版本或启动入口未全部通过核验；请查看日志并刷新检查，不会自动重试");
        } else if verified.unchanged
            && matches!(
                evidence.kind,
                CliApplyEvidence::ExitedUnsuccessfully
                    | CliApplyEvidence::SpawnFailed
                    | CliApplyEvidence::RejectedBeforeSpawn
            )
        {
            cell.finish(
                AgentAssetOperationOutcome::UnchangedFailure,
                "升级失败，已核对原安装未变化",
            );
        } else {
            cell.finish(
                AgentAssetOperationOutcome::OutcomeUnknown,
                "无法确认升级结果，请刷新核对实际版本；不会自动重试或回滚",
            );
        }
    }

    pub(crate) fn operation(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
        if let Ok(cell) = self.cell(actor, id) {
            return Ok(cell.snapshot());
        }
        self.history
            .lock()
            .map_err(|_| internal())?
            .as_ref()
            .map_err(|_| internal())?
            .iter()
            .find(|record| record.operation.id == id)
            .map(|record| record.operation.clone())
            .ok_or_else(|| AgentLifecycleError::new(AgentLifecycleErrorKind::OperationNotFound))
    }

    pub(crate) fn operations(
        &self,
        actor: &str,
    ) -> Result<Vec<AgentLifecycleOperation>, AgentLifecycleError> {
        let mut result = self
            .operations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|cell| cell.actor == actor)
            .map(|cell| cell.snapshot())
            .collect::<Vec<_>>();
        result.extend(
            self.history
                .lock()
                .map_err(|_| internal())?
                .as_ref()
                .map_err(|_| internal())?
                .iter()
                .map(|record| record.operation.clone()),
        );
        result.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(result)
    }

    /// Publish launch resolution changes as revisions so the frontend can accept them.
    pub(crate) fn publish_launch(
        &self,
        actor: &str,
        operation: AgentLifecycleOperation,
    ) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
        if let Ok(cell) = self.cell(actor, &operation.id) {
            let mut state = cell.state.lock().map_err(|_| internal())?;
            if state.public.revision == operation.revision
                && state.public.next_launch != operation.next_launch
            {
                state.public.next_launch = operation.next_launch;
                bump(&mut state.public);
            }
            return Ok(state.public.clone());
        }
        let mut history = self.history.lock().map_err(|_| internal())?;
        let record = history
            .as_mut()
            .map_err(|_| internal())?
            .iter_mut()
            .find(|record| record.operation.id == operation.id)
            .ok_or_else(|| AgentLifecycleError::new(AgentLifecycleErrorKind::OperationNotFound))?;
        if record.operation.revision == operation.revision
            && record.operation.next_launch != operation.next_launch
        {
            record.operation.next_launch = operation.next_launch;
            bump(&mut record.operation);
        }
        Ok(record.operation.clone())
    }

    pub(crate) fn reconcile_history(
        &self,
        settings: AppSettings,
        home: PathBuf,
    ) -> Result<(), AgentLifecycleError> {
        let mut history = self.history.lock().map_err(|_| internal())?;
        let records = history.as_mut().map_err(|_| internal())?;
        if !records.iter().any(needs_reconciliation) {
            return Ok(());
        }
        let targets = super::planning::installation_facts(&LifecycleContext { settings, home })?;
        for record in records
            .iter_mut()
            .filter(|record| needs_reconciliation(record))
        {
            let operation = &mut record.operation;
            operation.recovered = true;
            operation.can_cancel = false;
            operation.phase = AgentAssetOperationPhase::Completed;
            operation.outcome = Some(if record.installer_started {
                AgentAssetOperationOutcome::OutcomeUnknown
            } else {
                AgentAssetOperationOutcome::CanceledBeforeCommit
            });
            operation.message = Some(
                if record.installer_started {
                    "App 曾中断，原安装程序可能仍在运行；已重新检测安装，不会自动重试升级"
                } else {
                    "App 在安装程序启动前退出，任务未执行"
                }
                .to_owned(),
            );
            let matches = targets
                .iter()
                .filter(|target| {
                    target.agent_kind == operation.agent_kind
                        && target.channel == operation.channel
                        && target.directory.as_deref() == Some(operation.directory.as_str())
                })
                .collect::<Vec<_>>();
            if let [target] = matches.as_slice() {
                operation.observed_version = target.installation.installed_version.clone();
                operation.verified_executable_path = target.installation.executable_path.clone();
                if record.installer_started
                    && crate::services::agent_cli::environment::version_state(
                        operation.from_version.as_deref(),
                        operation.observed_version.as_deref(),
                    ) == AgentLifecycleVersionState::UpdateAvailable
                {
                    operation.outcome = Some(AgentAssetOperationOutcome::AppliedUnverified);
                    operation.message = Some(
                        "重启后已检测到比原安装更新的版本；原任务的退出结果无法恢复，未重试升级"
                            .to_owned(),
                    );
                }
            }
            bump(operation);
            self.journal.save(record).map_err(|_| internal())?;
        }
        Ok(())
    }

    pub(crate) fn cancel(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<AgentLifecycleOperation, AgentLifecycleError> {
        let cell = self.cell(actor, id)?;
        let mut state = cell.state.lock().map_err(|_| internal())?;
        if state.public.outcome.is_none() && !state.committed {
            cell.canceled.store(true, Ordering::Release);
            state.public.phase = AgentAssetOperationPhase::Completed;
            state.public.outcome = Some(AgentAssetOperationOutcome::CanceledBeforeCommit);
            state.public.can_cancel = false;
            state.public.message = Some("已在安装程序启动前取消".to_owned());
            bump(&mut state.public);
            persist_state(&self.journal, &mut state);
        }
        Ok(state.public.clone())
    }

    pub(crate) fn remove_actor(&self, actor: &str) {
        self.plans.remove_actor(actor);
        // Submitted tasks retain their private actor and complete verification.
        // Restarted windows may read persisted history, never executable plans.
    }

    fn cell(&self, actor: &str, id: &str) -> Result<Arc<OperationCell>, AgentLifecycleError> {
        self.operations
            .lock()
            .map_err(|_| internal())?
            .get(id)
            .filter(|cell| cell.actor == actor)
            .cloned()
            .ok_or_else(|| AgentLifecycleError::new(AgentLifecycleErrorKind::OperationNotFound))
    }

    fn context(&self, settings: AppSettings, home: PathBuf) -> LifecycleContext {
        LifecycleContext { home, settings }
    }
}

impl OperationCell {
    fn snapshot(&self) -> AgentLifecycleOperation {
        let diagnostics = self.plan.execution.diagnostics();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if diagnostics.is_some() && state.public.diagnostics != diagnostics {
            state.public.diagnostics = diagnostics;
            bump(&mut state.public);
        }
        state.public.clone()
    }

    fn phase(&self, phase: AgentAssetOperationPhase, message: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.public.outcome.is_none() {
            state.public.phase = phase;
            state.public.message = Some(message.to_owned());
            bump(&mut state.public);
            persist_state(&self.journal, &mut state);
        }
    }

    fn commit(&self) -> Result<(), AgentAssetMutationError> {
        let mut state = self.state.lock().map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::InternalFailure)
        })?;
        if self.canceled.load(Ordering::Acquire) || state.public.outcome.is_some() {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::PreparationFailed,
            ));
        }
        let mut committing = state.public.clone();
        committing.can_cancel = false;
        committing.phase = AgentAssetOperationPhase::Applying;
        self.journal
            .save(&Record {
                operation: committing,
                installer_started: true,
            })
            .map_err(|_| {
                AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
            })?;
        state.committed = true;
        state.public.can_cancel = false;
        state.public.phase = AgentAssetOperationPhase::Applying;
        state.public.message = Some("安装程序已启动，完成后将继续核对版本".to_owned());
        bump(&mut state.public);
        persist_state(&self.journal, &mut state);
        Ok(())
    }

    fn finish(&self, outcome: AgentAssetOperationOutcome, message: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.public.outcome.is_none() {
            state.public.phase = AgentAssetOperationPhase::Completed;
            state.public.can_cancel = false;
            state.public.outcome = Some(outcome);
            state.public.message = Some(message.to_owned());
            bump(&mut state.public);
            persist_state(&self.journal, &mut state);
        }
    }
}

fn needs_reconciliation(record: &Record) -> bool {
    record.operation.outcome.is_none()
        || (record.operation.recovered
            && record.operation.outcome == Some(AgentAssetOperationOutcome::OutcomeUnknown))
}

fn persist_state(journal: &Journal, state: &mut OperationState) {
    if let Err(error) = journal.save(&Record {
        operation: state.public.clone(),
        installer_started: state.committed,
    }) {
        eprintln!("Unable to persist upgrade task: {error}");
        let message = state.public.message.get_or_insert_with(String::new);
        message.push_str("；任务记录保存失败，重启后需重新核对");
    }
}

fn bump(operation: &mut AgentLifecycleOperation) {
    operation.revision = operation.revision.saturating_add(1);
    operation.updated_at = now_string();
}

pub(super) fn now_string() -> String {
    Local::now().to_rfc3339()
}

pub(super) fn internal() -> AgentLifecycleError {
    AgentLifecycleError::new(AgentLifecycleErrorKind::InternalFailure)
}

pub(super) fn map_mutation_error(error: AgentAssetMutationError) -> AgentLifecycleError {
    use AgentAssetMutationErrorKind as Source;
    use AgentLifecycleErrorKind as Target;
    let kind = match error.kind {
        Source::PlanExpired => Target::PlanExpired,
        Source::PlanConsumed => Target::PlanConsumed,
        Source::ActorMismatch => Target::ActorMismatch,
        Source::TargetMismatch => Target::TargetMismatch,
        Source::ActionMismatch => Target::ActionMismatch,
        Source::OperationNotFound => Target::OperationNotFound,
        Source::SourceConflict => Target::TargetChanged,
        Source::ActionUnavailable => Target::ActionUnavailable,
        Source::CapacityExceeded => Target::CapacityExceeded,
        Source::InvalidRequest => Target::InvalidRequest,
        _ => Target::InternalFailure,
    };
    AgentLifecycleError::new(kind)
}
