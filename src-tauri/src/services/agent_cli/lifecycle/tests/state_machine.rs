use super::super::{
    planning::{
        LifecycleContext, LifecycleExecution, LifecyclePlanner, PreparedLifecycle, RunEvidence,
        Verification,
    },
    service::LifecycleService,
};
use super::{
    apply_request, installation, plan_request, public_plan, target, write_package, Fixture,
};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        execution::CliApplyEvidence, locking::MutationLocks,
    },
};
use futures_util::{future::BoxFuture, FutureExt};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const ACTOR: &str = "lifecycle-fixture-window";

struct FixturePlanner {
    target: AgentLifecycleTarget,
    execution: Arc<ControlledExecution>,
    keys: Vec<String>,
}

impl LifecyclePlanner for FixturePlanner {
    fn catalog(
        &self,
        _context: LifecycleContext,
        _request: AgentLifecycleCatalogRequest,
    ) -> BoxFuture<'static, Result<AgentLifecycleCatalog, AgentLifecycleError>> {
        let target = self.target.clone();
        async move {
            Ok(AgentLifecycleCatalog {
                targets: vec![target],
                refreshed_at: String::new(),
                next_check_at: None,
            })
        }
        .boxed()
    }

    fn plan(
        &self,
        _context: LifecycleContext,
        request: AgentLifecyclePlanRequest,
    ) -> BoxFuture<'static, Result<PreparedLifecycle, AgentLifecycleError>> {
        let target = self.target.clone();
        let execution = self.execution.clone();
        let keys = self.keys.clone();
        async move {
            if request.expected_evidence_revision != target.evidence_revision {
                return Err(AgentLifecycleError::new(
                    AgentLifecycleErrorKind::TargetChanged,
                ));
            }
            Ok(PreparedLifecycle {
                public: public_plan(&target, "2.0.0"),
                lock_keys: keys,
                execution,
            })
        }
        .boxed()
    }
}

#[derive(Clone, Copy, Default)]
enum PanicAt {
    #[default]
    Never,
    BeforeCommit,
    AfterCommit,
    Verification,
}

struct ControlledExecution {
    revalidation_fails: bool,
    evidence: CliApplyEvidence,
    verification: Verification,
    panic_at: PanicAt,
    runs: AtomicUsize,
    commits: AtomicUsize,
    verifications: AtomicUsize,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
}

impl Default for ControlledExecution {
    fn default() -> Self {
        Self {
            revalidation_fails: false,
            evidence: CliApplyEvidence::ExitedSuccessfully,
            verification: Verification {
                version: Some("2.0.0".to_owned()),
                executable_path: Some("fixture-verified-executable".to_owned()),
                changed: true,
                unchanged: false,
            },
            panic_at: PanicAt::Never,
            runs: AtomicUsize::new(0),
            commits: AtomicUsize::new(0),
            verifications: AtomicUsize::new(0),
            entered: Mutex::new(None),
            release: Mutex::new(None),
        }
    }
}

impl LifecycleExecution for ControlledExecution {
    fn revalidate(&self) -> Result<(), AgentLifecycleError> {
        if self.revalidation_fails {
            Err(AgentLifecycleError::new(
                AgentLifecycleErrorKind::TargetChanged,
            ))
        } else {
            Ok(())
        }
    }

    fn run(&self, commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>) -> RunEvidence {
        self.runs.fetch_add(1, Ordering::AcqRel);
        assert!(
            !matches!(self.panic_at, PanicAt::BeforeCommit),
            "fixture panic before commit"
        );
        if commit().is_err() {
            return RunEvidence {
                kind: CliApplyEvidence::RejectedBeforeSpawn,
                output_truncated: false,
            };
        }
        self.commits.fetch_add(1, Ordering::AcqRel);
        assert!(
            !matches!(self.panic_at, PanicAt::AfterCommit),
            "fixture panic after commit"
        );
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        if let Some(release) = self.release.lock().unwrap().take() {
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        RunEvidence {
            kind: self.evidence,
            output_truncated: self.evidence == CliApplyEvidence::TimedOut,
        }
    }

    fn verify(&self) -> Verification {
        self.verifications.fetch_add(1, Ordering::AcqRel);
        assert!(
            !matches!(self.panic_at, PanicAt::Verification),
            "fixture verification panic"
        );
        self.verification.clone()
    }
}

struct ServiceFixture {
    files: Fixture,
    target: AgentLifecycleTarget,
    execution: Arc<ControlledExecution>,
    locks: Arc<MutationLocks>,
    service: Arc<LifecycleService>,
}

impl ServiceFixture {
    fn new(execution: ControlledExecution) -> Self {
        let files = Fixture::new();
        let kind = AgentCliKind::Codex;
        let prefix = files.prefix(kind);
        let executable = write_package(&prefix, kind, "1.0.0");
        let target = target(kind, &prefix, installation(kind, &executable, "1.0.0"));
        let locks = Arc::new(MutationLocks::default());
        let execution = Arc::new(execution);
        let planner = FixturePlanner {
            target: target.clone(),
            execution: execution.clone(),
            keys: vec![
                "installation:fixture-codex".to_owned(),
                "lifecycle-directory:fixture-prefix".to_owned(),
            ],
        };
        let service = Arc::new(LifecycleService::with_planner(
            locks.clone(),
            Arc::new(planner),
        ));
        Self {
            files,
            target,
            execution,
            locks,
            service,
        }
    }

    async fn plan(&self) -> AgentLifecyclePlan {
        self.service
            .plan(
                ACTOR,
                AppSettings::default(),
                self.files.home.clone(),
                plan_request(&self.target),
            )
            .await
            .unwrap()
    }

    async fn start(&self) -> AgentLifecycleOperation {
        self.service
            .start(ACTOR, &apply_request(&self.plan().await))
            .unwrap()
    }
}

fn wait_for_phase(service: &LifecycleService, id: &str, phase: AgentAssetOperationPhase) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while service.operation(ACTOR, id).unwrap().phase != phase {
        assert!(
            Instant::now() < deadline,
            "operation did not reach {phase:?}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[tokio::test]
async fn plan_and_start_do_not_run_or_lock_the_background_executor() {
    let fixture = ServiceFixture::new(ControlledExecution::default());
    let plan = fixture.plan().await;
    assert_eq!(plan.plan_token.len(), 64);
    assert!(!plan.expires_at.is_empty());
    let operation = fixture.service.start(ACTOR, &apply_request(&plan)).unwrap();
    assert_eq!(operation.phase, AgentAssetOperationPhase::Preparing);
    assert_eq!(operation.revision, 1);
    assert!(operation.can_cancel && operation.outcome.is_none());
    assert_eq!(fixture.execution.runs.load(Ordering::Acquire), 0);
    assert_eq!(fixture.service.operations(ACTOR).unwrap().len(), 1);
    assert!(fixture
        .service
        .operations("other-window")
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .service
            .operation("other-window", &operation.id)
            .unwrap_err()
            .kind,
        AgentLifecycleErrorKind::OperationNotFound
    );
}

#[tokio::test]
async fn actor_target_agent_and_action_mismatches_do_not_burn_the_plan() {
    let fixture = ServiceFixture::new(ControlledExecution::default());
    let request = apply_request(&fixture.plan().await);
    assert_eq!(
        fixture
            .service
            .start("wrong-actor", &request)
            .unwrap_err()
            .kind,
        AgentLifecycleErrorKind::ActorMismatch
    );
    let mut changed = request.clone();
    changed.target_id.push_str("-wrong");
    assert_eq!(
        fixture.service.start(ACTOR, &changed).unwrap_err().kind,
        AgentLifecycleErrorKind::TargetMismatch
    );
    changed = request.clone();
    changed.agent_kind = AgentCliKind::Grok;
    assert_eq!(
        fixture.service.start(ACTOR, &changed).unwrap_err().kind,
        AgentLifecycleErrorKind::ActionMismatch
    );
    assert!(fixture.service.start(ACTOR, &request).is_ok());
    assert_eq!(
        fixture.service.start(ACTOR, &request).unwrap_err().kind,
        AgentLifecycleErrorKind::PlanConsumed
    );
}

#[tokio::test]
async fn actor_cleanup_invalidates_pending_tokens_but_submitted_tasks_finish() {
    let fixture = ServiceFixture::new(ControlledExecution::default());
    let operation = fixture.start().await;
    let pending = apply_request(&fixture.plan().await);
    fixture.service.remove_actor(ACTOR);
    assert!(fixture.service.start(ACTOR, &pending).is_err());
    fixture.service.run_operation(&operation.id);
    let completed = fixture.service.operation(ACTOR, &operation.id).unwrap();
    assert_eq!(
        completed.outcome,
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );
    assert_eq!(
        fixture
            .service
            .operation("replacement-window", &operation.id)
            .unwrap_err()
            .kind,
        AgentLifecycleErrorKind::OperationNotFound
    );
}

#[tokio::test]
async fn cancellation_before_dispatch_never_runs_the_executor() {
    let fixture = ServiceFixture::new(ControlledExecution::default());
    let operation = fixture.start().await;
    let canceled = fixture.service.cancel(ACTOR, &operation.id).unwrap();
    assert_eq!(canceled.phase, AgentAssetOperationPhase::Completed);
    assert_eq!(
        canceled.outcome,
        Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
    );
    assert!(!canceled.can_cancel);
    assert!(canceled.revision > operation.revision);
    fixture.service.run_operation(&operation.id);
    assert_eq!(fixture.execution.runs.load(Ordering::Acquire), 0);
    assert_eq!(
        fixture
            .service
            .operation(ACTOR, &operation.id)
            .unwrap()
            .revision,
        canceled.revision
    );
}

#[tokio::test]
async fn shared_installation_and_prefix_locks_can_be_canceled_without_a_spawn() {
    for domain in [
        "installation:fixture-codex",
        "lifecycle-directory:fixture-prefix",
    ] {
        let fixture = ServiceFixture::new(ControlledExecution::default());
        let canceled = AtomicBool::new(false);
        let guard = fixture
            .locks
            .acquire(
                &[domain.to_owned()],
                &canceled,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let operation = fixture.start().await;
        let service = fixture.service.clone();
        let id = operation.id.clone();
        let worker = thread::spawn(move || service.run_operation(&id));
        wait_for_phase(
            &fixture.service,
            &operation.id,
            AgentAssetOperationPhase::WaitingForLock,
        );
        let outcome = fixture.service.cancel(ACTOR, &operation.id).unwrap();
        assert_eq!(
            outcome.outcome,
            Some(AgentAssetOperationOutcome::CanceledBeforeCommit)
        );
        worker.join().unwrap();
        assert_eq!(fixture.execution.runs.load(Ordering::Acquire), 0);
        drop(guard);
    }
}

#[tokio::test]
async fn a_committed_task_remains_independent_and_cannot_be_canceled() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let execution = ControlledExecution {
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(Some(release_rx)),
        ..ControlledExecution::default()
    };
    let fixture = ServiceFixture::new(execution);
    let operation = fixture.start().await;
    let service = fixture.service.clone();
    let id = operation.id.clone();
    let worker = thread::spawn(move || service.run_operation(&id));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let applying = fixture.service.cancel(ACTOR, &operation.id).unwrap();
    assert_eq!(applying.phase, AgentAssetOperationPhase::Applying);
    assert!(!applying.can_cancel && applying.outcome.is_none());
    // Listing, another plan and another start stay available while this promise
    // is pending. They never wait for the external process or held domain lock.
    assert_eq!(fixture.service.operations(ACTOR).unwrap().len(), 1);
    let another = fixture.start().await;
    fixture.service.cancel(ACTOR, &another.id).unwrap();
    fixture.service.remove_actor(ACTOR);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let completed = fixture.service.operation(ACTOR, &operation.id).unwrap();
    assert!(completed.revision > applying.revision);
    assert_eq!(
        completed.outcome,
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );
    fixture.service.run_operation(&operation.id);
    assert_eq!(fixture.execution.runs.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn source_drift_is_an_unchanged_conflict_before_execution() {
    let fixture = ServiceFixture::new(ControlledExecution {
        revalidation_fails: true,
        ..ControlledExecution::default()
    });
    let operation = fixture.start().await;
    fixture.service.run_operation(&operation.id);
    let completed = fixture.service.operation(ACTOR, &operation.id).unwrap();
    assert_eq!(
        completed.outcome,
        Some(AgentAssetOperationOutcome::UnchangedConflict)
    );
    assert_eq!(fixture.execution.runs.load(Ordering::Acquire), 0);
    assert_eq!(fixture.execution.verifications.load(Ordering::Acquire), 0);
    assert!(!completed.can_cancel);
}

#[tokio::test]
async fn actual_version_evidence_controls_timeout_failure_and_partial_results() {
    let installed = |version: &str, changed, unchanged, launcher: bool| Verification {
        version: Some(version.to_owned()),
        executable_path: launcher.then(|| "verified-fixture-path".to_owned()),
        changed,
        unchanged,
    };
    for (process, verification, expected) in [
        (
            CliApplyEvidence::TimedOut,
            installed("2.0.0", true, false, true),
            AgentAssetOperationOutcome::AppliedUnverified,
        ),
        (
            CliApplyEvidence::ExitedUnsuccessfully,
            installed("2.0.0", true, false, true),
            AgentAssetOperationOutcome::AppliedUnverified,
        ),
        (
            CliApplyEvidence::TimedOut,
            installed("1.0.0", false, true, true),
            AgentAssetOperationOutcome::OutcomeUnknown,
        ),
        (
            CliApplyEvidence::ExitedUnsuccessfully,
            installed("1.0.0", false, true, true),
            AgentAssetOperationOutcome::UnchangedFailure,
        ),
        (
            CliApplyEvidence::ExitedSuccessfully,
            installed("1.5.0", true, false, true),
            AgentAssetOperationOutcome::AppliedVerified,
        ),
        (
            CliApplyEvidence::ExitedSuccessfully,
            installed("2.0.0", true, false, false),
            AgentAssetOperationOutcome::AppliedUnverified,
        ),
        (
            CliApplyEvidence::SpawnFailed,
            Verification::default(),
            AgentAssetOperationOutcome::OutcomeUnknown,
        ),
    ] {
        let fixture = ServiceFixture::new(ControlledExecution {
            evidence: process,
            verification,
            ..ControlledExecution::default()
        });
        let operation = fixture.start().await;
        fixture.service.run_operation(&operation.id);
        let completed = fixture.service.operation(ACTOR, &operation.id).unwrap();
        assert_eq!(completed.outcome, Some(expected), "{process:?}");
        assert_eq!(completed.phase, AgentAssetOperationPhase::Completed);
        assert!(!completed.can_cancel);
        assert_eq!(completed.timed_out, process == CliApplyEvidence::TimedOut);
        assert_eq!(
            completed.output_truncated,
            process == CliApplyEvidence::TimedOut
        );
        assert_eq!(fixture.execution.verifications.load(Ordering::Acquire), 1);
    }
}

#[tokio::test]
async fn panic_paths_complete_and_release_the_shared_domains() {
    for (panic_at, expected) in [
        (
            PanicAt::BeforeCommit,
            AgentAssetOperationOutcome::UnchangedFailure,
        ),
        (
            PanicAt::AfterCommit,
            AgentAssetOperationOutcome::OutcomeUnknown,
        ),
        (
            PanicAt::Verification,
            AgentAssetOperationOutcome::OutcomeUnknown,
        ),
    ] {
        let fixture = ServiceFixture::new(ControlledExecution {
            panic_at,
            ..ControlledExecution::default()
        });
        let operation = fixture.start().await;
        fixture.service.run_operation(&operation.id);
        let completed = fixture.service.operation(ACTOR, &operation.id).unwrap();
        assert_eq!(completed.outcome, Some(expected));
        assert_eq!(completed.phase, AgentAssetOperationPhase::Completed);
        assert!(!completed.can_cancel);
        let canceled = AtomicBool::new(false);
        assert!(fixture
            .locks
            .acquire(
                &["installation:fixture-codex".to_owned()],
                &canceled,
                Instant::now() + Duration::from_secs(1)
            )
            .is_ok());
    }
}

#[tokio::test]
async fn capacity_rejection_preserves_the_plan_for_after_a_task_finishes() {
    let fixture = ServiceFixture::new(ControlledExecution::default());
    let mut operations = Vec::new();
    for _ in 0..8 {
        operations.push(fixture.start().await);
    }
    let request = apply_request(&fixture.plan().await);
    assert_eq!(
        fixture.service.start(ACTOR, &request).unwrap_err().kind,
        AgentLifecycleErrorKind::CapacityExceeded
    );
    fixture.service.cancel(ACTOR, &operations[0].id).unwrap();
    assert!(fixture.service.start(ACTOR, &request).is_ok());
}
