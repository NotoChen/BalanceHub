//! Native resume intents and operation ownership. Query/locator validation stays
//! in the session query service; terminal and credential handling stays in the
//! existing temporary CLI launcher.
mod activity;
mod admission;
mod execution;
mod finalization;
#[cfg(test)]
mod tests;

pub(crate) use activity::native_session_activity;
use finalization::{has_launch_evidence, FinalizationState};

use super::session_sort_key;
use crate::{
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT},
    models::{
        AgentSessionLaunchIdentity, AgentSessionResumeOperation, AgentSessionResumeRequest,
        AgentSessionResumeState, TemporaryCliInstance, TemporaryCliInstanceStatus,
    },
    services::temporary_cli::ResumeCompletion,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use tauri::{AppHandle, Emitter};

const MAX_OPERATIONS: usize = 256;
pub(crate) const OPERATION_TIMEOUT_SECONDS: u64 = 60;

#[derive(Default)]
pub(crate) struct AgentSessionResumeService {
    registry: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    operations: BTreeMap<String, Entry>,
    requests: BTreeMap<(String, String), RequestBinding>,
    reservations: BTreeMap<String, String>,
}

struct RequestBinding {
    operation_id: String,
    request: AgentSessionResumeRequest,
}

#[derive(Clone)]
struct Entry {
    actors: BTreeSet<String>,
    window_label: String,
    request: AgentSessionResumeRequest,
    operation: AgentSessionResumeOperation,
    dispatched: bool,
    dispatch_finished: bool,
    launch_confirmed: bool,
    identity: Option<AgentSessionLaunchIdentity>,
    instance: Option<TemporaryCliInstance>,
    completion: Option<ResumeCompletion>,
    finalization: FinalizationState,
}

impl Entry {
    fn matches_instance(&self, instance: &TemporaryCliInstance) -> bool {
        self.instance
            .as_ref()
            .is_some_and(|registered| registered.id == instance.id)
            && self.operation.cli_kind == Some(instance.cli_kind)
            && self.identity.as_ref().is_some_and(|expected| {
                instance.native_session.as_ref().is_some_and(|actual| {
                    actual.matches_source_session(
                        &expected.runtime_scope,
                        &expected.source_identity,
                        &expected.native_session_id,
                    )
                })
            })
    }

    fn accept_dispatch(
        &mut self,
        instance: TemporaryCliInstance,
        uncertainty: Option<String>,
    ) -> Result<(), String> {
        if !self.matches_instance(&instance) {
            return Err("启动记录与原生会话来源不一致".to_owned());
        }
        let registered = self.instance.as_mut().ok_or("启动记录不存在")?;
        if registered.status == TemporaryCliInstanceStatus::Starting {
            *registered = instance;
        } else if instance.terminal_locator.is_some() {
            // A get/list reconciliation may finish before terminal automation
            // returns. Retain its stronger process evidence and add only the
            // terminal locator that the automation just proved.
            registered.terminal_kind = instance.terminal_kind;
            registered.terminal_name = instance.terminal_name;
            registered.terminal_locator = instance.terminal_locator;
            registered.can_activate = registered.status != TemporaryCliInstanceStatus::Exited;
        }
        if let Some(result) = self.operation.result.as_mut() {
            result.instance = registered.clone();
        }
        self.dispatch_finished = true;
        self.launch_confirmed |= has_launch_evidence(registered);
        if matches!(
            self.operation.state,
            AgentSessionResumeState::Running | AgentSessionResumeState::Uncertain
        ) {
            if let Some(error) = uncertainty {
                self.operation.state = AgentSessionResumeState::Uncertain;
                self.operation.message = format!("终端启动结果尚未确定：{error}；不会自动重复执行");
            }
        }
        Ok(())
    }
}

impl AgentSessionResumeService {
    /// Admission must finish before this private registry transaction grants
    /// access or shares a reservation with another window. No terminal or CLI
    /// work takes place while holding the registry lock.
    fn reserve(
        &self,
        actor: &str,
        window_label: &str,
        request: AgentSessionResumeRequest,
    ) -> Result<(AgentSessionResumeOperation, bool), String> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let request_key = (actor.to_owned(), request.request_id.clone());
        if let Some(binding) = registry.requests.get(&request_key) {
            if binding.request != request {
                return Err("相同请求标识不能用于另一个继续请求".to_owned());
            }
            let entry = registry
                .operations
                .get(&binding.operation_id)
                .ok_or("继续请求记录已失效")?;
            return Ok((entry.operation.clone(), false));
        }
        if let Some(id) = registry.reservations.get(&request.session_ref).cloned() {
            let entry = registry
                .operations
                .get_mut(&id)
                .ok_or("继续任务记录已失效")?;
            entry.actors.insert(actor.to_owned());
            let operation = entry.operation.clone();
            registry.requests.insert(
                request_key,
                RequestBinding {
                    operation_id: id,
                    request,
                },
            );
            return Ok((operation, false));
        }
        prune_finished(&mut registry);
        if registry.operations.len() >= MAX_OPERATIONS {
            return Err("继续任务记录已满，请先核对尚未确定的启动结果".to_owned());
        }
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| "无法分配继续任务标识".to_owned())?;
        let id = format!("resume-{:032x}", u128::from_ne_bytes(nonce));
        let now = chrono::Local::now().to_rfc3339();
        let operation = AgentSessionResumeOperation {
            id: id.clone(),
            revision: 1,
            request_id: request.request_id.clone(),
            session_ref: request.session_ref.clone(),
            cli_kind: None,
            state: AgentSessionResumeState::Queued,
            message: "等待继续原生会话".to_owned(),
            created_at: now.clone(),
            updated_at: now,
            can_cancel: true,
            runtime_ids: Vec::new(),
            result: None,
        };
        registry
            .reservations
            .insert(request.session_ref.clone(), id.clone());
        registry.requests.insert(
            request_key,
            RequestBinding {
                operation_id: id.clone(),
                request: request.clone(),
            },
        );
        registry.operations.insert(
            id,
            Entry {
                actors: BTreeSet::from([actor.to_owned()]),
                window_label: window_label.to_owned(),
                request,
                operation: operation.clone(),
                dispatched: false,
                dispatch_finished: false,
                launch_confirmed: false,
                identity: None,
                instance: None,
                completion: None,
                finalization: FinalizationState::Pending,
            },
        );
        Ok((operation, true))
    }

    pub(crate) fn operation(
        &self,
        app: &AppHandle,
        actor: &str,
        id: &str,
    ) -> Result<AgentSessionResumeOperation, String> {
        self.authorize(actor, id)?;
        self.reconcile(app, id);
        self.entry(id)
            .map(|entry| entry.operation)
            .ok_or_else(|| "继续任务不存在".to_owned())
    }

    pub(crate) fn operations(
        &self,
        app: &AppHandle,
        actor: &str,
    ) -> Vec<AgentSessionResumeOperation> {
        let ids = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .operations
            .values()
            .filter(|entry| entry.actors.contains(actor))
            .map(|entry| entry.operation.id.clone())
            .collect::<Vec<_>>();
        let mut operations = ids
            .iter()
            .filter_map(|id| self.operation(app, actor, id).ok())
            .collect::<Vec<_>>();
        sort_operations(&mut operations);
        operations
    }

    pub(crate) fn cancel(
        &self,
        app: &AppHandle,
        actor: &str,
        id: &str,
    ) -> Result<AgentSessionResumeOperation, String> {
        let operation = self.cancel_record(actor, id)?;
        emit(app, &operation);
        Ok(operation)
    }

    fn cancel_record(&self, actor: &str, id: &str) -> Result<AgentSessionResumeOperation, String> {
        self.authorize(actor, id)?;
        self.update(id, |entry| {
            if !entry.operation.can_cancel {
                return Err("继续命令已交给终端，不能撤销；请核对已有终端状态".to_owned());
            }
            entry.operation.state = AgentSessionResumeState::Cancelled;
            entry.operation.can_cancel = false;
            entry.operation.message = "已取消继续，未再次执行命令".to_owned();
            Ok(())
        })
    }

    fn authorize(&self, actor: &str, id: &str) -> Result<(), String> {
        if self
            .entry(id)
            .is_some_and(|entry| entry.actors.contains(actor))
        {
            Ok(())
        } else {
            Err("继续任务不存在或不属于当前窗口".to_owned())
        }
    }

    fn entry(&self, id: &str) -> Option<Entry> {
        self.registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .operations
            .get(id)
            .cloned()
    }

    fn update(
        &self,
        id: &str,
        apply: impl FnOnce(&mut Entry) -> Result<(), String>,
    ) -> Result<AgentSessionResumeOperation, String> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = registry.operations.get_mut(id).ok_or("继续任务不存在")?;
        apply(entry)?;
        entry.operation.revision = entry.operation.revision.saturating_add(1);
        entry.operation.updated_at = chrono::Local::now().to_rfc3339();
        let operation = entry.operation.clone();
        if matches!(
            operation.state,
            AgentSessionResumeState::Succeeded
                | AgentSessionResumeState::Failed
                | AgentSessionResumeState::Cancelled
        ) {
            registry.reservations.retain(|_, owner| owner != id);
        }
        Ok(operation)
    }

    fn begin(&self, id: &str) -> Result<Entry, String> {
        self.update(id, |entry| {
            if entry.operation.state != AgentSessionResumeState::Queued {
                return Err("继续任务已处理".to_owned());
            }
            entry.operation.state = AgentSessionResumeState::Running;
            entry.operation.message = "正在核对原生来源与启动条件".to_owned();
            Ok(())
        })?;
        self.entry(id).ok_or_else(|| "继续任务不存在".to_owned())
    }

    fn mark_dispatch(&self, id: &str, instance: TemporaryCliInstance) -> Result<(), String> {
        self.update(id, |entry| {
            if entry.operation.state != AgentSessionResumeState::Running
                || !entry.operation.can_cancel
            {
                return Err("继续任务已取消或超时".to_owned());
            }
            entry.dispatched = true;
            entry.instance = Some(instance.clone());
            entry.operation.runtime_ids =
                vec![crate::services::agent_runtime::runtime_id_for_instance(
                    &instance.id,
                )];
            entry.operation.can_cancel = false;
            entry.operation.message = "已提交终端启动，正在等待进程证据".to_owned();
            Ok(())
        })
        .map(|_| ())
    }

    fn fail(&self, app: &AppHandle, id: &str, error: String) {
        if let Ok(operation) = self.fail_record(id, error) {
            emit(app, &operation);
        }
    }

    fn fail_record(&self, id: &str, error: String) -> Result<AgentSessionResumeOperation, String> {
        self.update(id, |entry| {
            if matches!(
                entry.operation.state,
                AgentSessionResumeState::Cancelled
                    | AgentSessionResumeState::Failed
                    | AgentSessionResumeState::Succeeded
            ) {
                return Ok(());
            }
            entry.operation.state = if entry.dispatched {
                AgentSessionResumeState::Uncertain
            } else {
                AgentSessionResumeState::Failed
            };
            entry.operation.message = error;
            entry.operation.can_cancel = false;
            Ok(())
        })
    }

    pub(crate) fn timeout(&self, app: &AppHandle, id: &str) {
        let Some(entry) = self.entry(id) else {
            return;
        };
        if matches!(
            entry.operation.state,
            AgentSessionResumeState::Queued | AgentSessionResumeState::Running
        ) {
            self.fail(
                app,
                id,
                if entry.dispatched {
                    "启动结果尚未确定，请核对已有终端；不会自动再次执行继续命令".to_owned()
                } else {
                    "核对启动条件超时，已停止本次继续".to_owned()
                },
            );
        }
    }
}

fn prune_finished(registry: &mut Registry) {
    if registry.operations.len() < MAX_OPERATIONS {
        return;
    }
    let reserved = registry
        .reservations
        .values()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut finished = registry
        .operations
        .values()
        .filter(|entry| !reserved.contains(&entry.operation.id))
        .map(|entry| {
            (
                session_sort_key(Some(&entry.operation.updated_at)),
                entry.operation.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    finished.sort();
    for (_, id) in finished.into_iter().take(MAX_OPERATIONS / 4) {
        registry.operations.remove(&id);
        registry
            .requests
            .retain(|_, binding| binding.operation_id != id);
    }
}

fn validate_request(request: &AgentSessionResumeRequest) -> Result<(), String> {
    for (value, limit) in [
        (&request.request_id, 160),
        (&request.session_ref, 512),
        (&request.scope_revision, 512),
        (&request.cli_path, 4096),
    ] {
        if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
            return Err("继续请求参数无效，请重新选择会话和 CLI".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn emit(app: &AppHandle, operation: &AgentSessionResumeOperation) {
    let _ = app.emit(BACKGROUND_TASK_EVENT, background_event(operation));
}

fn sort_operations(operations: &mut [AgentSessionResumeOperation]) {
    operations.sort_by(|left, right| {
        session_sort_key(Some(&right.created_at))
            .cmp(&session_sort_key(Some(&left.created_at)))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn background_event(operation: &AgentSessionResumeOperation) -> BackgroundTaskEvent {
    let (status, finished) = match operation.state {
        AgentSessionResumeState::Queued => ("running", false),
        AgentSessionResumeState::Running => ("running", false),
        AgentSessionResumeState::Succeeded => ("success", true),
        AgentSessionResumeState::Failed => ("failed", true),
        AgentSessionResumeState::Cancelled => ("failed", true),
        AgentSessionResumeState::Uncertain => ("failed", true),
    };
    BackgroundTaskEvent {
        task_id: format!("agent-session-resume-{}", operation.id),
        kind: "cliLaunch".to_owned(),
        status: status.to_owned(),
        title: "继续原生会话".to_owned(),
        detail: operation.message.clone(),
        progress: None,
        started_at: u64::try_from(session_sort_key(Some(&operation.created_at)))
            .unwrap_or_default(),
        finished_at: finished.then(|| {
            u64::try_from(session_sort_key(Some(&operation.updated_at))).unwrap_or_default()
        }),
        error: matches!(
            operation.state,
            AgentSessionResumeState::Failed | AgentSessionResumeState::Uncertain
        )
        .then(|| operation.message.clone()),
        can_cancel: None,
        can_show_window: None,
        login_account_id: None,
        provider_id: None,
    }
}

#[cfg(test)]
mod background_event_tests {
    use super::*;
    use crate::models::AgentCliKind;

    #[test]
    fn resume_events_preserve_operation_state_without_enabling_login_controls() {
        let mut operation = AgentSessionResumeOperation {
            id: "resume-fixture".to_owned(),
            revision: 1,
            request_id: "request-fixture".to_owned(),
            session_ref: "opaque-fixture".to_owned(),
            cli_kind: Some(AgentCliKind::Codex),
            state: AgentSessionResumeState::Queued,
            message: "继续会话状态".to_owned(),
            created_at: "2026-09-18T08:00:00Z".to_owned(),
            updated_at: "2026-09-18T08:00:01Z".to_owned(),
            can_cancel: true,
            runtime_ids: Vec::new(),
            result: None,
        };

        for (state, status, finished, failed) in [
            (AgentSessionResumeState::Queued, "running", false, false),
            (AgentSessionResumeState::Running, "running", false, false),
            (AgentSessionResumeState::Succeeded, "success", true, false),
            (AgentSessionResumeState::Failed, "failed", true, true),
            (AgentSessionResumeState::Cancelled, "failed", true, false),
            (AgentSessionResumeState::Uncertain, "failed", true, true),
        ] {
            operation.state = state;
            let value = serde_json::to_value(background_event(&operation)).unwrap();

            assert_eq!(value["taskId"], "agent-session-resume-resume-fixture");
            assert_eq!(value["kind"], "cliLaunch");
            assert_eq!(value["title"], "继续原生会话");
            assert_eq!(value["detail"], "继续会话状态");
            assert_eq!(value["status"], status);
            assert_eq!(value["finishedAt"].is_number(), finished);
            assert_eq!(value["error"].is_string(), failed);
            for field in ["canCancel", "canShowWindow", "loginAccountId", "providerId"] {
                assert!(value.get(field).is_none(), "{state:?} exposed {field}");
            }
        }
    }
}
