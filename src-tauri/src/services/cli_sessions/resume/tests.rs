use super::*;
use crate::models::{
    AgentCliKind, AgentRuntimeScope, AgentSessionResumeIntent, TemporaryCliInstanceStatus,
    TemporaryCliTerminalKind,
};
use crate::services::cli_sessions::workbench::ResumeAdmissionFixture;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};

fn request(id: &str, reference: &str) -> AgentSessionResumeRequest {
    AgentSessionResumeRequest {
        request_id: id.to_owned(),
        session_ref: reference.to_owned(),
        scope_revision: "scope-1".to_owned(),
        cli_path: "/synthetic/bin/codex".to_owned(),
        terminal_kind: TemporaryCliTerminalKind::Terminal,
        intent: AgentSessionResumeIntent::Native {},
    }
}

fn workbench_admission(
    fixture: Arc<ResumeAdmissionFixture>,
) -> impl FnOnce(&str, &str, &str) -> Result<(), String> + Send + 'static {
    move |window, scope, reference| fixture.resolve(window, scope, reference)
}

fn instance(reference: &str, source: &str) -> TemporaryCliInstance {
    TemporaryCliInstance {
        id: "instance-1".to_owned(),
        provider_id: None,
        provider_name: None,
        native_session: Some(AgentSessionLaunchIdentity {
            session_ref: reference.to_owned(),
            source_identity: source.to_owned(),
            native_session_id: "same-native-id".to_owned(),
            runtime_scope: AgentRuntimeScope::Native,
        }),
        session_title: String::new(),
        account_label: String::new(),
        api_key_local_id: None,
        cli_kind: AgentCliKind::Codex,
        workdir: "/synthetic/workspace".to_owned(),
        terminal_kind: TemporaryCliTerminalKind::Terminal,
        terminal_name: "Terminal".to_owned(),
        terminal_locator: None,
        started_at: "1000".to_owned(),
        ended_at: None,
        pid: None,
        status: TemporaryCliInstanceStatus::Starting,
        exit_code: None,
        can_activate: false,
    }
}

fn dispatched(service: &AgentSessionResumeService) -> (String, TemporaryCliInstance) {
    let (operation, _) = service
        .reserve("window-1", "main", request("request-1", "stable-ref"))
        .unwrap();
    service.begin(&operation.id).unwrap();
    let instance = instance("stable-ref", "source-1");
    service
        .update(&operation.id, |entry| {
            entry.identity = instance.native_session.clone();
            entry.operation.cli_kind = Some(instance.cli_kind);
            Ok(())
        })
        .unwrap();
    service
        .mark_dispatch(&operation.id, instance.clone())
        .unwrap();
    (operation.id, instance)
}

fn dispatch_returned(
    service: &AgentSessionResumeService,
    id: &str,
    instance: TemporaryCliInstance,
    uncertainty: Option<String>,
) {
    service
        .finish_dispatch(
            id,
            instance,
            uncertainty,
            ResumeCompletion::new(
                AgentCliKind::Codex,
                "/synthetic/bin/codex".to_owned(),
                "/synthetic/workspace".into(),
                Vec::new(),
                None,
            ),
        )
        .unwrap();
}

fn finish_registration(
    service: &AgentSessionResumeService,
    id: &str,
) -> AgentSessionResumeOperation {
    let instance = service.entry(id).unwrap().instance.unwrap();
    service
        .reconcile_with(id, |_| Ok(Some(instance)), ResumeCompletion::pending)
        .unwrap()
}

#[test]
fn concurrent_windows_and_scope_revisions_share_one_pending_operation() {
    let service = Arc::new(AgentSessionResumeService::default());
    let barrier = Arc::new(Barrier::new(16));
    let workers = (0..16)
        .map(|index| {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut request = request(&format!("request-{index}"), "stable-ref");
                request.scope_revision = format!("scope-{index}");
                barrier.wait();
                service
                    .reserve(
                        &format!("actor-{index}"),
                        &format!("window-{index}"),
                        request,
                    )
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|(_, created)| *created).count(), 1);
    assert!(results
        .iter()
        .all(|(operation, _)| operation.id == results[0].0.id));
    assert!(results
        .iter()
        .all(|(operation, _)| operation.state == AgentSessionResumeState::Queued));
    assert!(results.iter().all(|(operation, _)| operation.can_cancel));
    assert!(results
        .iter()
        .all(|(operation, _)| operation.result.is_none()));
}

#[test]
fn a_cancelled_preparation_cannot_dispatch_or_block_an_explicit_new_request() {
    let service = AgentSessionResumeService::default();
    let (first, _) = service
        .reserve("actor", "main", request("first", "stable-ref"))
        .unwrap();
    service.begin(&first.id).unwrap();
    let cancelled = service.cancel_record("actor", &first.id).unwrap();
    assert_eq!(cancelled.state, AgentSessionResumeState::Cancelled);
    assert!(!cancelled.can_cancel);
    assert!(service
        .mark_dispatch(&first.id, instance("stable-ref", "source-1"))
        .is_err());
    let (second, created) = service
        .reserve("actor", "main", request("second", "stable-ref"))
        .unwrap();
    assert!(created);
    assert_ne!(first.id, second.id);
}

#[test]
fn unknown_dispatch_keeps_the_native_reservation_without_a_hook_id() {
    let service = AgentSessionResumeService::default();
    let (id, instance) = dispatched(&service);
    let uncertain = service
        .fail_record(&id, "terminal timeout".to_owned())
        .unwrap();
    assert_eq!(uncertain.state, AgentSessionResumeState::Uncertain);
    assert!(!uncertain.can_cancel);
    assert!(service.cancel_record("window-1", &id).is_err());
    let mut another = request("request-2", "stable-ref");
    another.scope_revision = "changed-workspace-set".to_owned();
    another.intent = AgentSessionResumeIntent::Provider {
        provider_id: "provider-1".to_owned(),
        api_key_local_id: None,
    };
    let (duplicate, created) = service.reserve("window-2", "other", another).unwrap();
    assert!(!created);
    assert_eq!(duplicate.id, id);
    assert_eq!(duplicate.state, AgentSessionResumeState::Uncertain);
    assert_eq!(
        service.entry(&id).unwrap().instance.unwrap().native_session,
        instance.native_session
    );
}

#[test]
fn an_explicit_exit_resolves_uncertainty_and_releases_the_reservation() {
    let service = AgentSessionResumeService::default();
    let (id, mut instance) = dispatched(&service);
    service
        .fail_record(&id, "terminal timeout".to_owned())
        .unwrap();
    instance.status = TemporaryCliInstanceStatus::Exited;
    instance.exit_code = Some(17);
    instance.ended_at = Some("2000".to_owned());
    let pending = service.observe_instance(&id, instance.clone()).unwrap();
    assert_eq!(pending.state, AgentSessionResumeState::Uncertain);
    assert!(pending.result.is_none());
    dispatch_returned(&service, &id, instance, None);
    let failed = finish_registration(&service, &id);
    assert_eq!(failed.state, AgentSessionResumeState::Failed);
    assert!(failed.message.contains("17"));
    let (next, created) = service
        .reserve("window-1", "main", request("request-2", "stable-ref"))
        .unwrap();
    assert!(created);
    assert_ne!(id, next.id);
}

#[test]
fn late_running_evidence_resolves_uncertainty_without_replaying_dispatch() {
    let service = AgentSessionResumeService::default();
    let (id, mut instance) = dispatched(&service);
    service
        .fail_record(&id, "terminal timeout".to_owned())
        .unwrap();
    instance.status = TemporaryCliInstanceStatus::Running;
    instance.pid = Some(42);
    let observed = service.observe_instance(&id, instance.clone()).unwrap();
    assert_eq!(observed.state, AgentSessionResumeState::Uncertain);
    assert!(observed.result.is_none());
    dispatch_returned(&service, &id, instance, None);
    let running = finish_registration(&service, &id);
    assert_eq!(running.state, AgentSessionResumeState::Succeeded);
    assert_eq!(running.id, id);
    assert_eq!(running.runtime_ids, ["balancehub:instance-1"]);
}

#[test]
fn the_same_native_id_from_another_source_cannot_resolve_an_operation() {
    let service = AgentSessionResumeService::default();
    let (id, mut instance) = dispatched(&service);
    service
        .fail_record(&id, "terminal timeout".to_owned())
        .unwrap();
    instance.native_session.as_mut().unwrap().source_identity = "source-2".to_owned();
    instance.status = TemporaryCliInstanceStatus::Exited;
    instance.exit_code = Some(0);
    assert!(service.observe_instance(&id, instance).is_err());
    assert_eq!(
        service.entry(&id).unwrap().operation.state,
        AgentSessionResumeState::Uncertain
    );
    let (_, other_source_created) = service
        .reserve("window-1", "main", request("request-2", "other-source-ref"))
        .unwrap();
    assert!(other_source_created);
}

#[test]
fn idempotency_rejects_rebinding_and_window_reads_stay_scoped() {
    let service = AgentSessionResumeService::default();
    let (operation, _) = service
        .reserve("actor", "main", request("first", "stable-ref"))
        .unwrap();
    let (retry, created) = service
        .reserve("actor", "main", request("first", "stable-ref"))
        .unwrap();
    assert!(!created);
    assert_eq!(retry.id, operation.id);
    assert!(service
        .reserve("actor", "main", request("first", "different-ref"))
        .is_err());
    assert!(service.authorize("another-actor", &operation.id).is_err());
}

#[test]
fn preparation_failure_releases_the_inflight_key() {
    let service = AgentSessionResumeService::default();
    let (first, _) = service
        .reserve("actor", "main", request("first", "stable-ref"))
        .unwrap();
    service.begin(&first.id).unwrap();
    let failed = service
        .fail_record(&first.id, "CLI unavailable".to_owned())
        .unwrap();
    assert_eq!(failed.state, AgentSessionResumeState::Failed);
    assert!(!failed.can_cancel);
    assert!(
        service
            .reserve("actor", "main", request("second", "stable-ref"))
            .unwrap()
            .1
    );
}

#[test]
fn a_coalesced_request_keeps_its_own_idempotency_binding() {
    let service = AgentSessionResumeService::default();
    let (first, _) = service
        .reserve("actor-1", "main", request("first", "stable-ref"))
        .unwrap();
    let mut second_request = request("second", "stable-ref");
    second_request.scope_revision = "another-window-scope".to_owned();
    let (second, created) = service
        .reserve("actor-2", "other", second_request.clone())
        .unwrap();
    assert!(!created);
    assert_eq!(first.id, second.id);
    let (retry, created) = service
        .reserve("actor-2", "other", second_request.clone())
        .unwrap();
    assert!(!created);
    assert_eq!(retry.id, first.id);
    assert!(service.authorize("actor-2", &first.id).is_ok());
    second_request.cli_path = "/synthetic/bin/other-codex".to_owned();
    assert!(service.reserve("actor-2", "other", second_request).is_err());
}

#[test]
fn a_late_terminal_error_cannot_replace_confirmed_running_evidence() {
    let service = AgentSessionResumeService::default();
    let (id, pending) = dispatched(&service);
    let mut running = pending.clone();
    running.status = TemporaryCliInstanceStatus::Running;
    running.pid = Some(42);
    let early = service.observe_instance(&id, running).unwrap();
    assert_eq!(early.state, AgentSessionResumeState::Running);
    assert!(early.result.is_none());
    let mut late_dispatch = pending;
    late_dispatch.terminal_locator = Some("verified-terminal".to_owned());
    dispatch_returned(
        &service,
        &id,
        late_dispatch,
        Some("late automation timeout".to_owned()),
    );
    let confirmed = finish_registration(&service, &id);
    assert_eq!(confirmed.state, AgentSessionResumeState::Succeeded);
    let instance = service.entry(&id).unwrap().instance.unwrap();
    assert_eq!(instance.status, TemporaryCliInstanceStatus::Running);
    assert_eq!(instance.pid, Some(42));
    assert_eq!(
        instance.terminal_locator.as_deref(),
        Some("verified-terminal")
    );
    assert!(instance.can_activate);
}

#[test]
fn stale_running_or_dispatch_results_do_not_reopen_an_exited_attempt() {
    let service = AgentSessionResumeService::default();
    let (id, pending) = dispatched(&service);
    let mut exited = pending.clone();
    exited.status = TemporaryCliInstanceStatus::Exited;
    exited.exit_code = Some(17);
    exited.ended_at = Some("2000".to_owned());
    let early = service.observe_instance(&id, exited.clone()).unwrap();
    assert_eq!(early.state, AgentSessionResumeState::Running);
    dispatch_returned(&service, &id, exited, None);
    let finished = finish_registration(&service, &id);
    assert_eq!(finished.state, AgentSessionResumeState::Failed);
    let mut stale = pending.clone();
    stale.status = TemporaryCliInstanceStatus::Running;
    stale.pid = Some(42);
    let final_state = service.observe_instance(&id, stale).unwrap();
    assert_eq!(final_state.state, AgentSessionResumeState::Failed);
    let after_dispatch = service
        .update(&id, |entry| {
            entry.accept_dispatch(pending, Some("late automation timeout".to_owned()))
        })
        .unwrap();
    assert_eq!(after_dispatch.state, AgentSessionResumeState::Failed);
    let instance = service.entry(&id).unwrap().instance.unwrap();
    assert_eq!(instance.status, TemporaryCliInstanceStatus::Exited);
    assert_eq!(instance.exit_code, Some(17));
}

#[test]
fn missing_process_or_a_different_launch_cannot_confirm_the_operation() {
    let service = AgentSessionResumeService::default();
    let (id, mut instance) = dispatched(&service);
    service.fail_record(&id, "timeout".to_owned()).unwrap();
    instance.status = TemporaryCliInstanceStatus::Running;
    let unresolved = service.observe_instance(&id, instance.clone()).unwrap();
    assert_eq!(unresolved.state, AgentSessionResumeState::Uncertain);
    instance.id = "other-launch-same-native-session".to_owned();
    instance.pid = Some(42);
    assert!(service.observe_instance(&id, instance).is_err());
    let (same, created) = service
        .reserve("actor-2", "other", request("another", "stable-ref"))
        .unwrap();
    assert!(!created);
    assert_eq!(same.id, id);
}

#[test]
fn operation_wire_times_and_background_events_share_nonzero_instants() {
    let service = AgentSessionResumeService::default();
    let (queued, _) = service
        .reserve("actor", "main", request("first", "stable-ref"))
        .unwrap();
    let queued_wire = serde_json::to_value(&queued).unwrap();
    let created =
        chrono::DateTime::parse_from_rfc3339(queued_wire["createdAt"].as_str().unwrap()).unwrap();
    assert!(created.timestamp_millis() > 0);
    let queued_event = background_event(&queued);
    assert_eq!(queued_event.started_at, created.timestamp_millis() as u64);
    assert_eq!(queued_event.finished_at, None);

    service.begin(&queued.id).unwrap();
    let failed = service
        .fail_record(&queued.id, "synthetic preparation failure".to_owned())
        .unwrap();
    let failed_wire = serde_json::to_value(&failed).unwrap();
    let updated =
        chrono::DateTime::parse_from_rfc3339(failed_wire["updatedAt"].as_str().unwrap()).unwrap();
    assert!(updated >= created);
    assert_eq!(failed_wire["createdAt"], queued_wire["createdAt"]);
    let failed_event = background_event(&failed);
    assert_eq!(failed_event.started_at, created.timestamp_millis() as u64);
    assert_eq!(
        failed_event.finished_at,
        Some(updated.timestamp_millis() as u64)
    );
    assert!(failed_event.finished_at.unwrap() >= failed_event.started_at);
    assert_eq!(failed_event.status, "failed");
}

#[test]
fn operation_sort_uses_rfc3339_instants_instead_of_offset_text() {
    let service = AgentSessionResumeService::default();
    let (mut earlier, _) = service
        .reserve("actor", "main", request("first", "ref-1"))
        .unwrap();
    let (mut later, _) = service
        .reserve("actor", "main", request("second", "ref-2"))
        .unwrap();
    earlier.created_at = "2026-09-17T08:00:00+08:00".to_owned();
    later.created_at = "2026-09-17T01:00:00+00:00".to_owned();
    let later_id = later.id.clone();
    let mut operations = vec![earlier, later];
    sort_operations(&mut operations);
    assert_eq!(operations[0].id, later_id);
}

#[tokio::test]
async fn workbench_grants_reject_missing_or_stale_windows_before_operation_access_is_shared() {
    let service = AgentSessionResumeService::default();
    let grants = Arc::new(ResumeAdmissionFixture::new("reject"));
    grants.grant("main", "scope-1");
    grants.grant_scope("without-reference", "scope-1");
    grants.grant("stale-grant", "outdated-scope");
    grants.advance_scope("stale-grant", "current-scope");
    let owner = grants.actor_key("main");
    let (first, _) = service
        .start_with_admission(
            &owner,
            "main",
            request("first", grants.session_ref()),
            std::time::Duration::from_secs(5),
            workbench_admission(Arc::clone(&grants)),
        )
        .await
        .unwrap();
    for (window, scope, error) in [
        ("without-scope", "scope-1", "会话范围已失效"),
        ("without-reference", "scope-1", "会话引用已失效"),
        (
            "stale-grant",
            "outdated-scope",
            "目录集合或原生会话来源已变化",
        ),
    ] {
        let actor = grants.actor_key(window);
        let mut submission = request("other-request", grants.session_ref());
        submission.scope_revision = scope.to_owned();
        let denied = service
            .start_with_admission(
                &actor,
                window,
                submission,
                std::time::Duration::from_secs(5),
                workbench_admission(Arc::clone(&grants)),
            )
            .await
            .unwrap_err();
        assert!(denied.contains(error));
        assert!(service.authorize(&actor, &first.id).is_err());
        assert!(service.cancel_record(&actor, &first.id).is_err());
    }
    assert!(service.entry(&first.id).unwrap().operation.can_cancel);
    assert_eq!(service.registry.lock().unwrap().operations.len(), 1);
    assert_eq!(service.registry.lock().unwrap().requests.len(), 1);

    grants.advance_scope("main", "new-scope");
    let retry = service
        .start_with_admission(
            &owner,
            "main",
            request("first", grants.session_ref()),
            std::time::Duration::from_secs(5),
            workbench_admission(Arc::clone(&grants)),
        )
        .await
        .unwrap_err();
    assert!(retry.contains("目录集合或原生会话来源已变化"));
}

#[tokio::test]
async fn workbench_granted_windows_share_one_operation_before_dispatch() {
    let service = AgentSessionResumeService::default();
    let grants = Arc::new(ResumeAdmissionFixture::new("share"));
    grants.grant("main", "scope-1");
    grants.grant("other", "scope-2");
    let first_actor = grants.actor_key("main");
    let second_actor = grants.actor_key("other");
    let mut second = request("second", grants.session_ref());
    second.scope_revision = "scope-2".to_owned();
    let (first, second) = tokio::join!(
        service.start_with_admission(
            &first_actor,
            "main",
            request("first", grants.session_ref()),
            std::time::Duration::from_secs(5),
            workbench_admission(Arc::clone(&grants)),
        ),
        service.start_with_admission(
            &second_actor,
            "other",
            second,
            std::time::Duration::from_secs(5),
            workbench_admission(Arc::clone(&grants)),
        ),
    );
    let (first, first_created) = first.unwrap();
    let (second, second_created) = second.unwrap();
    assert_eq!(usize::from(first_created) + usize::from(second_created), 1);
    assert_eq!(first.id, second.id);
    assert_eq!(service.registry.lock().unwrap().operations.len(), 1);
    assert!(service.authorize(&first_actor, &first.id).is_ok());
    assert!(service.authorize(&second_actor, &first.id).is_ok());
    assert!(!service.entry(&first.id).unwrap().dispatched);
}

#[tokio::test]
async fn admission_timeout_cannot_create_a_late_orphan_operation() {
    let service = AgentSessionResumeService::default();
    let grants = Arc::new(ResumeAdmissionFixture::new("timeout"));
    grants.grant("main", "scope-1");
    let resolver = Arc::clone(&grants);
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let (finished, completion) = tokio::sync::oneshot::channel();
    let result = service
        .start_with_admission(
            &grants.actor_key("main"),
            "main",
            request("first", grants.session_ref()),
            std::time::Duration::from_millis(20),
            move |window, scope, reference| {
                blocked
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .map_err(|error| error.to_string())?;
                let admitted = resolver.resolve(window, scope, reference);
                let _ = finished.send(());
                admitted
            },
        )
        .await;
    assert!(result.unwrap_err().contains("超时"));
    assert!(service.registry.lock().unwrap().operations.is_empty());
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), completion)
        .await
        .unwrap()
        .unwrap();
    assert!(service.registry.lock().unwrap().operations.is_empty());
    assert!(service.registry.lock().unwrap().reservations.is_empty());
    assert!(service.registry.lock().unwrap().requests.is_empty());
}

#[test]
fn dispatch_and_reconcile_keep_early_evidence_nonfinal_until_workspace_io_finishes() {
    let service = Arc::new(AgentSessionResumeService::default());
    let (id, pending) = dispatched(&service);
    let mut running = pending.clone();
    running.status = TemporaryCliInstanceStatus::Running;
    running.pid = Some(42);
    let early = service
        .reconcile_with(
            &id,
            |instance_id| {
                assert_eq!(instance_id, running.id);
                Ok(Some(running.clone()))
            },
            |_, _| panic!("workspace IO must wait for terminal dispatch to return"),
        )
        .unwrap();
    assert_eq!(early.state, AgentSessionResumeState::Running);
    assert!(early.result.is_none());
    assert_eq!(service.registry.lock().unwrap().reservations.len(), 1);

    dispatch_returned(&service, &id, pending, None);
    let writes = Arc::new(AtomicUsize::new(0));
    let (recording, observer) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let worker_service = Arc::clone(&service);
    let worker_id = id.clone();
    let worker_instance = running.clone();
    let worker_writes = Arc::clone(&writes);
    let worker = std::thread::spawn(move || {
        worker_service
            .reconcile_with(
                &worker_id,
                |_| Ok(Some(worker_instance)),
                |completion, instance| {
                    worker_writes.fetch_add(1, Ordering::SeqCst);
                    recording.send(()).unwrap();
                    blocked
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .unwrap();
                    let mut result = completion.pending(instance);
                    result.workspaces = vec![crate::models::Workspace {
                        path: "/synthetic/recorded-workspace".to_owned(),
                        use_count: 9,
                    }];
                    result
                },
            )
            .unwrap()
    });
    observer
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();

    let during_write = service
        .reconcile_with(
            &id,
            |_| Ok(Some(running.clone())),
            |_, _| {
                writes.fetch_add(1, Ordering::SeqCst);
                panic!("a concurrent poll must not repeat workspace IO")
            },
        )
        .unwrap();
    assert_eq!(during_write.state, AgentSessionResumeState::Running);
    assert!(during_write.result.is_none());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(service.registry.lock().unwrap().reservations.len(), 1);

    running.status = TemporaryCliInstanceStatus::Exited;
    running.pid = None;
    running.exit_code = Some(17);
    running.ended_at = Some("2000".to_owned());
    let ended_during_write = service
        .reconcile_with(
            &id,
            |_| Ok(Some(running.clone())),
            |_, _| {
                writes.fetch_add(1, Ordering::SeqCst);
                panic!("exit evidence must not start a second workspace write")
            },
        )
        .unwrap();
    assert_eq!(ended_during_write.state, AgentSessionResumeState::Running);
    assert!(ended_during_write.result.is_none());
    release.send(()).unwrap();
    let completed = worker.join().unwrap();
    assert_eq!(completed.state, AgentSessionResumeState::Succeeded);
    let result = completed.result.unwrap();
    assert_eq!(result.workspaces[0].path, "/synthetic/recorded-workspace");
    assert_eq!(result.workspaces[0].use_count, 9);
    assert_eq!(result.instance.status, TemporaryCliInstanceStatus::Exited);
    assert_eq!(result.instance.exit_code, Some(17));
    let repeated = service
        .reconcile_with(
            &id,
            |_| Ok(Some(running)),
            |_, _| {
                writes.fetch_add(1, Ordering::SeqCst);
                panic!("a completed launch must not repeat workspace IO")
            },
        )
        .unwrap();
    assert_eq!(repeated.state, AgentSessionResumeState::Succeeded);
    assert_eq!(repeated.result.unwrap().workspaces, result.workspaces);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(service.registry.lock().unwrap().reservations.is_empty());
}

#[test]
fn dispatch_and_reconcile_publish_workspace_errors_only_with_the_final_result() {
    let service = AgentSessionResumeService::default();
    let (id, mut instance) = dispatched(&service);
    instance.status = TemporaryCliInstanceStatus::Running;
    instance.pid = Some(42);
    service.observe_instance(&id, instance.clone()).unwrap();
    dispatch_returned(&service, &id, instance.clone(), None);
    let completed = service
        .reconcile_with(
            &id,
            |_| Ok(Some(instance)),
            |completion, instance| {
                let mut result = completion.pending(instance);
                result.workspace_error = Some("synthetic workspace persistence error".to_owned());
                result
            },
        )
        .unwrap();
    assert_eq!(completed.state, AgentSessionResumeState::Succeeded);
    assert_eq!(
        completed.result.unwrap().workspace_error.as_deref(),
        Some("synthetic workspace persistence error")
    );
}
