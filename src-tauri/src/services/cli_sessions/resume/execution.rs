use super::{emit, has_launch_evidence, AgentSessionResumeService, FinalizationState};
use crate::{
    models::{
        AgentSessionResumeOperation, AgentSessionResumeResult, AgentSessionResumeState,
        TemporaryCliInstance, TemporaryCliInstanceStatus,
    },
    services::{
        agent_runtime::runtime_id_for_instance,
        cli_runtime,
        cli_sessions::workbench::{resolve_session_ref, AgentSessionActor},
        temporary_cli::{launch_identity, ResumeCompletion, TemporaryCliLaunchService},
    },
    state::AppState,
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

impl AgentSessionResumeService {
    pub(crate) fn run(&self, app: &AppHandle, id: &str) {
        match catch_unwind(AssertUnwindSafe(|| self.execute(app, id))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.fail(app, id, error),
            Err(_) => self.fail(app, id, "继续任务异常中断，请核对已有终端状态".to_owned()),
        }
    }

    fn execute(&self, app: &AppHandle, id: &str) -> Result<(), String> {
        let entry = self.begin(id)?;
        emit(app, &entry.operation);
        let actor = AgentSessionActor {
            app,
            window_label: &entry.window_label,
        };
        let target = resolve_session_ref(
            actor,
            &entry.request.scope_revision,
            &entry.request.session_ref,
        )?;
        if !target.can_resume {
            return Err(target
                .resume_reason
                .unwrap_or_else(|| "该原生会话不支持独立继续".to_owned()));
        }
        let identity = launch_identity(&target);
        self.update(id, |entry| {
            if entry.operation.state != AgentSessionResumeState::Running {
                return Err("继续任务已取消或超时".to_owned());
            }
            entry.identity = Some(identity.clone());
            entry.operation.cli_kind = Some(target.cli_kind);
            Ok(())
        })?;
        let existing = cli_runtime::session_instances(target.cli_kind, &identity);
        if !existing.is_empty() {
            return self.reuse(app, id, existing);
        }

        let prepared = TemporaryCliLaunchService::new(app).prepare_resume(
            &entry.request,
            &target,
            &identity,
        )?;
        // Source and directory may change while the executable/terminal is
        // probed. Validate the original backend reference at the commit edge.
        let recheck = resolve_session_ref(
            actor,
            &entry.request.scope_revision,
            &entry.request.session_ref,
        );
        let refreshed = match recheck {
            Ok(target) => target,
            Err(error) => {
                prepared.cancel();
                return Err(error);
            }
        };
        if !refreshed.can_resume
            || refreshed.cli_kind != target.cli_kind
            || refreshed.workdir != target.workdir
            || !identity.matches_source_session(
                &crate::models::AgentRuntimeScope::Native,
                &refreshed.source_identity,
                &refreshed.native_session_id,
            )
        {
            prepared.cancel();
            return Err("原生会话来源或继续能力已变化，请刷新后重新选择".to_owned());
        }
        let existing = cli_runtime::session_instances(target.cli_kind, &identity)
            .into_iter()
            .filter(|instance| instance.id != prepared.instance().id)
            .collect::<Vec<_>>();
        if !existing.is_empty() {
            prepared.cancel();
            return self.reuse(app, id, existing);
        }
        if let Err(error) = self.mark_dispatch(id, prepared.instance().clone()) {
            prepared.cancel();
            return Err(error);
        }
        if let Some(entry) = self.entry(id) {
            emit(app, &entry.operation);
        }
        let (outcome, completion) = prepared.dispatch();
        let operation =
            self.finish_dispatch(id, outcome.instance, outcome.uncertainty, completion)?;
        emit(app, &operation);

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.reconcile(app, id);
            let Some(current) = self.entry(id) else {
                return Ok(());
            };
            if matches!(
                current.operation.state,
                AgentSessionResumeState::Succeeded
                    | AgentSessionResumeState::Failed
                    | AgentSessionResumeState::Cancelled
            ) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.fail(
                    app,
                    id,
                    "终端已接收继续请求，但尚未取得启动进程证据；请核对已有终端，不会自动重试"
                        .to_owned(),
                );
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub(super) fn finish_dispatch(
        &self,
        id: &str,
        instance: TemporaryCliInstance,
        uncertainty: Option<String>,
        completion: ResumeCompletion,
    ) -> Result<AgentSessionResumeOperation, String> {
        self.update(id, |entry| {
            entry.accept_dispatch(instance, uncertainty)?;
            entry.completion = Some(completion);
            entry.publish_progress();
            Ok(())
        })
    }

    fn reuse(
        &self,
        app: &AppHandle,
        id: &str,
        mut instances: Vec<TemporaryCliInstance>,
    ) -> Result<(), String> {
        instances.sort_by_key(|instance| !has_launch_evidence(instance));
        let instance = instances.first().ok_or("活动实例已变化")?.clone();
        let running = has_launch_evidence(&instance);
        let workspaces = app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .workspaces
            .clone();
        let operation = self.update(id, |entry| {
            if entry.operation.state != AgentSessionResumeState::Running {
                return Err("继续任务已取消或超时".to_owned());
            }
            entry.dispatched = true;
            entry.dispatch_finished = true;
            entry.launch_confirmed = running;
            entry.finalization = FinalizationState::Complete;
            entry.instance = Some(instance.clone());
            entry.operation.runtime_ids = instances
                .iter()
                .map(|instance| runtime_id_for_instance(&instance.id))
                .collect();
            entry.operation.can_cancel = false;
            entry.operation.state = if running {
                AgentSessionResumeState::Succeeded
            } else {
                AgentSessionResumeState::Uncertain
            };
            entry.operation.message = if running {
                "该原生会话已有活动实例，可返回已有终端".to_owned()
            } else {
                "该原生会话已有启动记录，结果尚未确定；未重复执行继续命令".to_owned()
            };
            entry.operation.result = Some(AgentSessionResumeResult {
                instance,
                workspaces,
                workspace_error: None,
                preference: None,
                reused: true,
            });
            Ok(())
        })?;
        emit(app, &operation);
        Ok(())
    }

    pub(super) fn reconcile(&self, app: &AppHandle, id: &str) {
        if let Some(operation) =
            self.reconcile_with(id, cli_runtime::instance, |completion, instance| {
                completion.record(app, instance)
            })
        {
            emit(app, &operation);
        }
    }

    /// The execution worker and IPC polling share this complete reconciliation
    /// transaction. Only instance reads and workspace persistence are external IO.
    pub(super) fn reconcile_with(
        &self,
        id: &str,
        read_instance: impl FnOnce(&str) -> Result<Option<TemporaryCliInstance>, String>,
        record: impl FnOnce(&ResumeCompletion, TemporaryCliInstance) -> AgentSessionResumeResult,
    ) -> Option<AgentSessionResumeOperation> {
        let entry = self.entry(id)?;
        if !entry.dispatched {
            return None;
        }
        let registered = entry.instance.as_ref()?;
        let instance = match read_instance(&registered.id) {
            Ok(Some(instance)) => instance,
            _ => {
                return self
                    .fail_record(
                        id,
                        "启动记录暂不可读，无法确定终端结果；未重复执行命令".to_owned(),
                    )
                    .ok();
            }
        };
        let mut operation = match self.observe_instance(id, instance) {
            Ok(operation) => operation,
            Err(error) => {
                return self.fail_record(id, error).ok();
            }
        };
        // Only the claiming observer records usage. Other get/list calls keep
        // the operation non-final until this IO and its result are complete.
        if let Some(claim) = self.claim_finalization(id) {
            let result = if claim.launched {
                match catch_unwind(AssertUnwindSafe(|| {
                    record(&claim.completion, claim.instance.clone())
                })) {
                    Ok(result) => result,
                    Err(_) => {
                        let mut result = claim.completion.pending(claim.instance);
                        result.workspace_error = Some("记录目录使用信息时任务异常中断".to_owned());
                        result
                    }
                }
            } else {
                claim.completion.pending(claim.instance)
            };
            if let Ok(recorded) = self.complete_finalization(id, result) {
                operation = recorded;
            }
        }
        Some(operation)
    }

    pub(super) fn observe_instance(
        &self,
        id: &str,
        mut instance: TemporaryCliInstance,
    ) -> Result<crate::models::AgentSessionResumeOperation, String> {
        self.update(id, |entry| {
            if !entry.matches_instance(&instance) {
                return Err("启动记录与原生会话来源不一致".to_owned());
            }
            let registered = entry.instance.as_ref().ok_or("启动记录不存在")?;
            if registered.status == TemporaryCliInstanceStatus::Exited {
                if instance.status != TemporaryCliInstanceStatus::Exited {
                    return Ok(());
                }
                instance.exit_code = instance.exit_code.or(registered.exit_code);
                instance.ended_at = instance.ended_at.or_else(|| registered.ended_at.clone());
            } else if registered.status == TemporaryCliInstanceStatus::Running
                && instance.status == TemporaryCliInstanceStatus::Starting
            {
                return Ok(());
            }
            if instance.terminal_locator.is_none() && registered.terminal_locator.is_some() {
                instance.terminal_kind = registered.terminal_kind;
                instance.terminal_name = registered.terminal_name.clone();
                instance.terminal_locator = registered.terminal_locator.clone();
                instance.can_activate = instance.status != TemporaryCliInstanceStatus::Exited;
            }
            entry.instance = Some(instance.clone());
            if let Some(result) = entry.operation.result.as_mut() {
                result.instance = instance.clone();
            }
            entry.launch_confirmed |= has_launch_evidence(&instance);
            entry.operation.can_cancel = false;
            entry.publish_progress();
            Ok(())
        })
    }
}
