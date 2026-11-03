//! Confirmed maintenance work, shared by history, bulk assets and migration.
pub(crate) mod files;
pub(crate) mod history;

use super::environment::mutation::token::{opaque_id, PlanRegistry, DEFAULT_PLAN_TTL};
use crate::models::*;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

type Work = Box<dyn FnOnce(&TaskContext) -> Result<(), String> + Send>;
struct Prepared {
    display: AgentMaintenancePlan,
    work: Work,
    _budget: Budget,
}
struct Cell {
    actor: String,
    public: Mutex<AgentMaintenanceOperation>,
    work: Mutex<Option<(Work, Budget)>>,
    canceled: AtomicBool,
    changed: AtomicBool,
}

#[derive(Default)]
pub(crate) struct MaintenanceService {
    plans: PlanRegistry<Prepared, AgentMaintenanceKind>,
    operations: Mutex<BTreeMap<String, Arc<Cell>>>,
    bytes: Arc<std::sync::atomic::AtomicUsize>,
}

struct Budget {
    used: Arc<std::sync::atomic::AtomicUsize>,
    bytes: usize,
}
impl Drop for Budget {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

pub(crate) struct TaskContext {
    cell: Arc<Cell>,
    deadline: Instant,
}
impl TaskContext {
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.cell.canceled.load(Ordering::Acquire) {
            return Err("已取消尚未完成的操作".to_owned());
        }
        if Instant::now() >= self.deadline {
            return Err("操作超时；已完成的变更保留，可在变更记录中恢复".to_owned());
        }
        Ok(())
    }
    pub(crate) fn canceled(&self) -> &AtomicBool {
        &self.cell.canceled
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn id(&self) -> String {
        self.cell
            .public
            .lock()
            .map(|public| public.id.clone())
            .unwrap_or_default()
    }
    pub(crate) fn changed(&self) {
        self.cell.changed.store(true, Ordering::Release);
    }
    pub(crate) fn begin_commit(&self, message: &str) -> Result<(), String> {
        let mut public = self.cell.public.lock().map_err(|_| "后台任务不可用")?;
        self.check()?;
        public.can_cancel = false;
        public.message = message.to_owned();
        public.revision += 1;
        public.updated_at = now();
        Ok(())
    }
    pub(crate) fn item(&self, id: &str, state: AgentMaintenanceItemState, message: Option<String>) {
        self.update(|public| {
            if let Some(item) = public.items.iter_mut().find(|item| item.id == id) {
                item.state = state;
                item.message = message;
            }
            public.completed = public
                .items
                .iter()
                .filter(|item| {
                    matches!(
                        item.state,
                        AgentMaintenanceItemState::Completed
                            | AgentMaintenanceItemState::Skipped
                            | AgentMaintenanceItemState::Failed
                            | AgentMaintenanceItemState::Canceled
                    )
                })
                .count();
        });
    }
    pub(crate) fn progress(&self, message: impl Into<String>) {
        self.update(|public| public.message = message.into());
    }
    fn update(&self, change: impl FnOnce(&mut AgentMaintenanceOperation)) {
        if let Ok(mut public) = self.cell.public.lock() {
            change(&mut public);
            public.revision += 1;
            public.updated_at = now();
        }
    }
}

pub(crate) fn item(id: impl Into<String>, label: impl Into<String>) -> AgentMaintenanceItem {
    AgentMaintenanceItem {
        id: id.into(),
        label: label.into(),
        state: AgentMaintenanceItemState::Pending,
        message: None,
    }
}
fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

impl MaintenanceService {
    pub(crate) fn plan(
        &self,
        actor: &str,
        mut display: AgentMaintenancePlan,
        bytes: usize,
        work: impl FnOnce(&TaskContext) -> Result<(), String> + Send + 'static,
    ) -> Result<AgentMaintenancePlan, String> {
        self.plans.prune_expired(Instant::now());
        if display.items.is_empty() {
            return Err("没有需要更改的项目".to_owned());
        }
        let bytes = display.changes.iter().fold(bytes, |total, change| {
            total
                .saturating_add(change.before.as_ref().map_or(0, String::len))
                .saturating_add(change.after.as_ref().map_or(0, String::len))
        });
        self.bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= 512 * 1024 * 1024)
            })
            .map_err(|_| "已有大型预览占用内存，请关闭预览或稍后重试")?;
        let budget = Budget {
            used: Arc::clone(&self.bytes),
            bytes,
        };
        display.expires_at = (chrono::Local::now()
            + chrono::Duration::seconds(DEFAULT_PLAN_TTL.as_secs() as i64))
        .to_rfc3339();
        let mut summary = AgentMaintenancePlan {
            token: String::new(),
            title: display.title.clone(),
            kind: display.kind,
            expires_at: display.expires_at.clone(),
            items: display.items.clone(),
            notes: display.notes.clone(),
            refresh: display.refresh.clone(),
            changes: display
                .changes
                .iter()
                .map(|change| AgentAssetPlanChange {
                    label: change.label.clone(),
                    path: change.path.clone(),
                    before: None,
                    after: None,
                })
                .collect(),
        };
        let token = self
            .plans
            .issue(
                actor,
                "maintenance",
                display.kind,
                Prepared {
                    display,
                    work: Box::new(work),
                    _budget: budget,
                },
                Instant::now(),
                DEFAULT_PLAN_TTL,
            )
            .map_err(|error| error.message)?;
        summary.token = token;
        Ok(summary)
    }
    pub(crate) fn change(
        &self,
        actor: &str,
        token: &str,
        index: usize,
    ) -> Result<AgentAssetPlanChange, String> {
        self.plans
            .inspect(actor, token, |plan| {
                plan.display.changes.get(index).cloned()
            })
            .map_err(|error| error.message)?
            .ok_or_else(|| "变更文件不存在".to_owned())
    }
    pub(crate) fn discard(&self, actor: &str, token: &str) -> Result<(), String> {
        self.plans
            .discard(actor, token)
            .map_err(|error| error.message)
    }
    pub(crate) fn start(
        &self,
        actor: &str,
        request: AgentMaintenanceApplyRequest,
    ) -> Result<AgentMaintenanceOperation, String> {
        let mut operations = self.operations.lock().map_err(|_| "后台任务不可用")?;
        for cell in operations.values().filter(|cell| cell.actor == actor) {
            let public = cell.public.lock().map_err(|_| "后台任务不可用")?;
            if public.plan_token == request.plan_token && public.kind == request.kind {
                return Ok(public.clone());
            }
        }
        if operations
            .values()
            .filter(|cell| {
                cell.public
                    .lock()
                    .is_ok_and(|public| public.phase != AgentAssetOperationPhase::Completed)
            })
            .count()
            >= 4
        {
            return Err("已有多个维护任务正在执行，请等待或取消后重试".to_owned());
        }
        let plan = self
            .plans
            .consume_bound(
                actor,
                &request.plan_token,
                "maintenance",
                &request.kind,
                Instant::now(),
            )
            .map_err(|error| error.message)?
            .value;
        let id = opaque_id().map_err(|error| error.message)?;
        let public = AgentMaintenanceOperation {
            id: id.clone(),
            plan_token: request.plan_token,
            kind: request.kind,
            title: plan.display.title,
            revision: 1,
            phase: AgentAssetOperationPhase::Preparing,
            outcome: None,
            can_cancel: true,
            created_at: now(),
            updated_at: now(),
            message: "任务已提交，正在准备".to_owned(),
            completed: 0,
            total: plan.display.items.len(),
            items: plan.display.items,
            refresh: plan.display.refresh,
        };
        operations.insert(
            id,
            Arc::new(Cell {
                actor: actor.to_owned(),
                public: Mutex::new(public.clone()),
                work: Mutex::new(Some((plan.work, plan._budget))),
                canceled: AtomicBool::new(false),
                changed: AtomicBool::new(false),
            }),
        );
        while operations.len() > 128 {
            let oldest = operations
                .iter()
                .filter_map(|(id, cell)| {
                    cell.public
                        .lock()
                        .ok()
                        .filter(|public| public.phase == AgentAssetOperationPhase::Completed)
                        .map(|public| (public.created_at.clone(), id.clone()))
                })
                .min();
            if let Some((_, id)) = oldest {
                operations.remove(&id);
            } else {
                break;
            }
        }
        Ok(public)
    }
    pub(crate) fn run(&self, id: &str) {
        let Some(cell) = self
            .operations
            .lock()
            .ok()
            .and_then(|items| items.get(id).cloned())
        else {
            return;
        };
        let Some((work, _budget)) = cell.work.lock().ok().and_then(|mut work| work.take()) else {
            return;
        };
        let context = TaskContext {
            cell: Arc::clone(&cell),
            deadline: Instant::now() + Duration::from_secs(10 * 60),
        };
        context.update(|public| {
            public.phase = AgentAssetOperationPhase::Applying;
            public.message = "正在处理所选项目".to_owned();
        });
        let title = cell
            .public
            .lock()
            .map(|public| public.title.clone())
            .unwrap_or_else(|_| "资产维护".to_owned());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _history = super::config_support::native_history::scope(id, &title);
            context.check()?;
            work(&context)
        }))
        .unwrap_or_else(|_| Err("任务异常停止，请刷新检查已完成的项目".to_owned()));
        context.update(|public| {
            let canceled = cell.canceled.load(Ordering::Acquire);
            for item in &mut public.items {
                if matches!(
                    item.state,
                    AgentMaintenanceItemState::Pending | AgentMaintenanceItemState::Running
                ) {
                    item.state = if canceled {
                        AgentMaintenanceItemState::Canceled
                    } else if result.is_err() {
                        AgentMaintenanceItemState::Failed
                    } else {
                        AgentMaintenanceItemState::Completed
                    };
                    item.message.clone_from(&result.as_ref().err().cloned());
                }
            }
            let changed = cell.changed.load(Ordering::Acquire);
            if !changed {
                public.refresh.catalog = false;
                public.refresh.agents.clear();
            }
            let failed = public
                .items
                .iter()
                .any(|item| item.state == AgentMaintenanceItemState::Failed);
            public.outcome = Some(if canceled {
                if changed {
                    AgentMaintenanceOutcome::Partial
                } else {
                    AgentMaintenanceOutcome::Canceled
                }
            } else if failed || result.is_err() {
                if changed {
                    AgentMaintenanceOutcome::Partial
                } else {
                    AgentMaintenanceOutcome::Failed
                }
            } else {
                AgentMaintenanceOutcome::Succeeded
            });
            public.message = result.err().unwrap_or_else(|| {
                if failed {
                    "部分项目未完成，请查看各项结果".to_owned()
                } else {
                    "已完成".to_owned()
                }
            });
            public.phase = AgentAssetOperationPhase::Completed;
            public.can_cancel = false;
            public.completed = public.total;
        });
    }
    pub(crate) fn get(&self, actor: &str, id: &str) -> Result<AgentMaintenanceOperation, String> {
        let operations = self.operations.lock().map_err(|_| "后台任务不可用")?;
        let cell = operations
            .get(id)
            .filter(|cell| cell.actor == actor)
            .ok_or("任务不存在或属于其他窗口")?;
        let public = cell.public.lock().map_err(|_| "后台任务不可用")?.clone();
        Ok(public)
    }
    pub(crate) fn list(&self, actor: &str) -> Vec<AgentMaintenanceOperation> {
        self.operations
            .lock()
            .map(|items| {
                items
                    .values()
                    .filter(|cell| cell.actor == actor)
                    .filter_map(|cell| cell.public.lock().ok().map(|public| public.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn cancel(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<AgentMaintenanceOperation, String> {
        let operations = self.operations.lock().map_err(|_| "后台任务不可用")?;
        let cell = operations
            .get(id)
            .filter(|cell| cell.actor == actor)
            .ok_or("任务不存在")?;
        let mut public = cell.public.lock().map_err(|_| "后台任务不可用")?;
        if public.can_cancel {
            cell.canceled.store(true, Ordering::Release);
            public.can_cancel = false;
            public.message = "正在取消剩余操作；已完成的变更保留".to_owned();
            public.revision += 1;
            public.updated_at = now();
        }
        Ok(public.clone())
    }
    pub(crate) fn remove_actor(&self, actor: &str) {
        self.plans.remove_actor(actor);
    }
}

pub(crate) fn preview(
    kind: AgentMaintenanceKind,
    title: impl Into<String>,
    changes: Vec<AgentAssetPlanChange>,
    items: Vec<AgentMaintenanceItem>,
) -> AgentMaintenancePlan {
    AgentMaintenancePlan {
        token: String::new(),
        kind,
        title: title.into(),
        expires_at: String::new(),
        changes,
        items,
        notes: Vec::new(),
        refresh: AgentMaintenanceRefresh {
            catalog: kind != AgentMaintenanceKind::SessionRelocation,
            agents: if kind == AgentMaintenanceKind::Restore {
                super::definitions()
                    .iter()
                    .map(|agent| agent.kind)
                    .collect()
            } else {
                Vec::new()
            },
        },
    }
}
