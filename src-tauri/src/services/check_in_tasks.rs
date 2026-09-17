//! All check-in entry points enqueue here. Waiting tasks own no browser or HTTP slot.
use crate::{
    app_events::PROVIDERS_CHANGED_EVENT,
    models::{
        provider_domain, CheckInBatch, CheckInError, CheckInPhase, CheckInSource, CheckInTask,
        Provider,
    },
    services::{
        notifications,
        provider_service::{ProviderRequestContext, ProviderService},
    },
    state::AppState,
    tray,
    util::unix_millis,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{watch, Semaphore};

const EVENT: &str = "balancehub://check-in-task";
static REVISION: AtomicU64 = AtomicU64::new(0);
static RUNS: OnceLock<Mutex<HashMap<String, Run>>> = OnceLock::new();
pub(super) static HTTP_SLOTS: Semaphore = Semaphore::const_new(6);

struct Run {
    task: CheckInTask,
    account_key: String,
    date: String,
    cancel: watch::Sender<bool>,
    executing: bool,
    context: ProviderRequestContext,
    automatic_attempt: u32,
}

#[derive(Clone)]
pub(crate) struct CheckInContext {
    pub run_id: String,
    pub interactive: bool,
}

impl CheckInContext {
    pub(crate) fn queued_for_browser(&self, app: &AppHandle) {
        publish(
            app,
            &self.run_id,
            CheckInPhase::Queued,
            "正在排队等待验证窗口，前面的中转站处理后自动继续".to_string(),
            true,
        );
    }

    pub(crate) fn phase(&self, app: &AppHandle, phase: CheckInPhase) {
        let message = if phase == CheckInPhase::WaitingHuman && self.interactive {
            "请在浏览器中完成验证，完成后自动继续"
        } else {
            phase.message()
        };
        publish(app, &self.run_id, phase, message.to_string(), true);
    }

    pub(crate) fn authenticated(&self, provider: &Provider) {
        if let Ok(mut runs) = runs().lock() {
            if let Some(run) = runs.get_mut(&self.run_id) {
                run.context = ProviderRequestContext::capture(provider);
                run.account_key = account_key(provider);
            }
        }
    }
}

fn runs() -> &'static Mutex<HashMap<String, Run>> {
    RUNS.get_or_init(Mutex::default)
}
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn account_key(provider: &Provider) -> String {
    let account = if !provider.auth.api_user.trim().is_empty() {
        provider.auth.api_user.trim()
    } else if !provider.auth.login_username.trim().is_empty() {
        provider.auth.login_username.trim()
    } else {
        &provider.identity.id
    };
    format!(
        "{:?}|{}|{}",
        provider.identity.protocol,
        provider.identity.base_url.trim().trim_end_matches('/'),
        account
    )
}

fn blocks_automatic(run: &Run, provider: &Provider, date: &str) -> bool {
    run.date == date
        && (run.task.provider_id == provider.identity.id
            || run.account_key == account_key(provider))
        && (!run.task.finished
            || matches!(
                run.task.phase,
                CheckInPhase::Unconfirmed | CheckInPhase::Cancelled
            ))
}

pub(crate) fn automatic_pending(provider: &Provider) -> bool {
    runs()
        .lock()
        .map(|runs| {
            runs.values()
                .any(|run| blocks_automatic(run, provider, &today()))
        })
        .unwrap_or(true)
}

pub(crate) fn list(app: &AppHandle) -> Vec<CheckInTask> {
    expire_waiting(app);
    let mut tasks = runs()
        .lock()
        .map(|runs| {
            runs.values()
                .map(|run| run.task.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tasks.sort_by_key(|task| task.started_at);
    tasks
}

pub(crate) fn enqueue(
    app: &AppHandle,
    id: String,
    source: CheckInSource,
    batch_id: Option<String>,
    automatic_attempt: u32,
) -> Result<CheckInTask, String> {
    expire_waiting(app);
    let provider = app
        .state::<AppState>()
        .data
        .read()
        .map_err(|_| "读取中转站失败")?
        .providers
        .iter()
        .find(|provider| provider.identity.id == id)
        .cloned()
        .ok_or("中转站已删除")?;
    if !provider.runtime.enabled {
        return Err("中转站已停用".to_string());
    }
    if !provider_domain::capabilities::supports_check_in(&provider) {
        return Err("当前站点或认证方式不支持签到，请检查账号配置".to_string());
    }
    let key = account_key(&provider);
    let (sender, receiver) = watch::channel(false);
    let task = {
        let mut runs = runs().lock().map_err(|_| "签到任务状态不可用")?;
        if let Some(run) = runs.values().find(|run| {
            !run.task.finished && (run.task.provider_id == id || run.account_key == key)
        }) {
            return Ok(run.task.clone());
        }
        if runs.len() >= 128 {
            let cutoff = unix_millis() as u64 - 15 * 60 * 1000;
            runs.retain(|_, run| {
                !run.task.finished
                    || run.task.finished_at.is_some_and(|time| time >= cutoff)
                    || matches!(
                        run.task.phase,
                        CheckInPhase::Unconfirmed | CheckInPhase::Cancelled
                    ) && run.date == today()
            });
        }
        let revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        let task = CheckInTask {
            run_id: format!("checkin-{}-{revision}", unix_millis()),
            provider_id: id,
            provider_name: provider.display_label().to_string(),
            batch_id,
            source,
            phase: CheckInPhase::Queued,
            message: CheckInPhase::Queued.message().to_string(),
            revision,
            finished: false,
            can_resume: false,
            can_cancel: true,
            started_at: unix_millis() as u64,
            finished_at: None,
        };
        runs.insert(
            task.run_id.clone(),
            Run {
                task: task.clone(),
                account_key: key,
                date: today(),
                cancel: sender,
                executing: true,
                context: ProviderRequestContext::capture(&provider),
                automatic_attempt,
            },
        );
        task
    };
    let _ = app.emit(EVENT, &task);
    spawn(
        app.clone(),
        task.clone(),
        receiver,
        source == CheckInSource::Manual,
    );
    Ok(task)
}

pub(crate) fn enqueue_all(app: &AppHandle) -> Result<CheckInBatch, String> {
    let providers = app
        .state::<AppState>()
        .data
        .read()
        .map_err(|_| "读取中转站失败")?
        .providers
        .clone();
    let batch_id = format!("checkin-batch-{}", REVISION.fetch_add(1, Ordering::Relaxed));
    let mut tasks = Vec::new();
    let mut skipped = 0;
    for provider in providers {
        if !provider.runtime.enabled
            || !provider_domain::capabilities::supports_check_in(&provider)
            || provider_domain::capabilities::checked_in_today(&provider)
        {
            skipped += 1;
            continue;
        }
        tasks.push(enqueue(
            app,
            provider.identity.id,
            CheckInSource::Batch,
            Some(batch_id.clone()),
            0,
        )?);
    }
    Ok(CheckInBatch {
        batch_id,
        tasks,
        skipped,
    })
}

pub(crate) fn resume(app: &AppHandle, run_id: &str) -> Result<CheckInTask, String> {
    expire_waiting(app);
    let (task, receiver) = {
        let mut runs = runs().lock().map_err(|_| "签到任务状态不可用")?;
        let run = runs.get_mut(run_id).ok_or("签到任务不存在")?;
        if run.executing || !run.task.can_resume {
            return Err("当前任务无法继续".to_string());
        }
        let state = app.state::<AppState>();
        let data = state.data.read().map_err(|_| "读取中转站失败")?;
        if !data
            .providers
            .iter()
            .any(|provider| provider.runtime.enabled && run.context.matches(provider))
        {
            return Err("账号配置已变更，请取消此任务后重新签到".to_string());
        }
        let (sender, receiver) = watch::channel(false);
        run.cancel = sender;
        run.executing = true;
        set_phase(
            &mut run.task,
            CheckInPhase::Queued,
            "已恢复签到，正在重新检查今日状态".to_string(),
            true,
        );
        (run.task.clone(), receiver)
    };
    let _ = app.emit(EVENT, &task);
    spawn(app.clone(), task.clone(), receiver, true);
    Ok(task)
}

pub(crate) fn cancel(app: &AppHandle, run_id: &str) -> Result<(), String> {
    let waiting = {
        let runs = runs().lock().map_err(|_| "签到任务状态不可用")?;
        let Some(run) = runs.get(run_id) else {
            return Ok(());
        };
        if run.task.finished {
            return Ok(());
        }
        if !run.task.can_cancel {
            return Err("正在保存已确认的结果，请稍候".to_string());
        }
        let _ = run.cancel.send(true);
        !run.executing
    };
    if waiting {
        publish(
            app,
            run_id,
            CheckInPhase::Cancelled,
            "签到已取消".to_string(),
            false,
        );
    }
    Ok(())
}

/// Editing a policy or credential must also stop a currently waiting browser.
pub(crate) fn cancel_outdated(app: &AppHandle) {
    let providers = match app.state::<AppState>().data.read() {
        Ok(data) => data.providers.clone(),
        Err(_) => return,
    };
    let ids = runs()
        .lock()
        .map(|runs| {
            runs.values()
                .filter(|run| {
                    !run.task.finished
                        && !providers.iter().any(|provider| {
                            provider.runtime.enabled && run.context.matches(provider)
                        })
                })
                .map(|run| run.task.run_id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for id in ids {
        let _ = cancel(app, &id);
    }
}

fn set_phase(task: &mut CheckInTask, phase: CheckInPhase, message: String, executing: bool) {
    task.phase = phase;
    task.message = message;
    task.finished = phase.finished();
    task.can_resume = phase.waiting() && !executing;
    task.can_cancel = !task.finished && phase != CheckInPhase::Saving;
    task.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
    task.finished_at = task.finished.then(|| unix_millis() as u64);
}

fn publish(app: &AppHandle, id: &str, phase: CheckInPhase, message: String, executing: bool) {
    let task = {
        let Ok(mut runs) = runs().lock() else {
            return;
        };
        let Some(run) = runs.get_mut(id) else {
            return;
        };
        if run.task.finished {
            return;
        }
        run.executing = executing;
        set_phase(&mut run.task, phase, message, executing);
        run.task.clone()
    };
    let _ = app.emit(EVENT, task);
}

fn expire_waiting(app: &AppHandle) {
    let date = today();
    let ids = runs()
        .lock()
        .map(|runs| {
            runs.values()
                .filter(|run| !run.executing && !run.task.finished && run.date != date)
                .map(|run| run.task.run_id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for id in ids {
        publish(
            app,
            &id,
            CheckInPhase::Cancelled,
            "等待任务已跨天，请重新签到".to_string(),
            false,
        );
    }
}

fn spawn(
    app: AppHandle,
    task: CheckInTask,
    mut cancelled: watch::Receiver<bool>,
    interactive: bool,
) {
    tauri::async_runtime::spawn(async move {
        let context = CheckInContext {
            run_id: task.run_id.clone(),
            interactive,
        };
        let service = ProviderService::new(&app);
        let outcome = tokio::select! {
            result = service.check_in_attempt(task.provider_id.clone(), &context) => result,
            _ = async { if !*cancelled.borrow() { let _ = cancelled.changed().await; } } => {
                let phase = runs().lock().ok().and_then(|runs| runs.get(&task.run_id).map(|run| run.task.phase));
                if phase.is_some_and(CheckInPhase::may_have_submitted) {
                    Err(CheckInError::Unconfirmed("请求可能已经发出；已停止后续提交，请查看站点记录".to_string()))
                } else {
                    publish(&app, &task.run_id, CheckInPhase::Cancelled, "签到已取消".to_string(), false);
                    return;
                }
            }
        };
        let (phase, message) = match outcome {
            Ok(result) if result.ok => (CheckInPhase::Completed, result.message),
            Ok(result) => (CheckInPhase::Failed, result.message),
            Err(CheckInError::WaitingHuman) => (
                CheckInPhase::WaitingHuman,
                CheckInPhase::WaitingHuman.message().to_string(),
            ),
            Err(CheckInError::WaitingBrowser(message)) => (CheckInPhase::WaitingBrowser, message),
            Err(CheckInError::Unconfirmed(message)) => (CheckInPhase::Unconfirmed, message),
            Err(CheckInError::Failed(message)) => (CheckInPhase::Failed, message),
        };
        publish(&app, &task.run_id, phase, message.clone(), false);
        let _ = app.emit(PROVIDERS_CHANGED_EVENT, ());
        tray::refresh_from_state(&app);
        if !phase.finished() || phase == CheckInPhase::Cancelled {
            return;
        }
        let attempt = runs()
            .lock()
            .ok()
            .and_then(|runs| runs.get(&task.run_id).map(|run| run.automatic_attempt))
            .unwrap_or(0);
        if task.source == CheckInSource::Automatic
            && phase != CheckInPhase::Completed
            && attempt > 1
        {
            return;
        }
        let data = {
            let state = app.state::<AppState>();
            let Ok(data) = state.data.read() else {
                return;
            };
            data.clone()
        };
        let Some(provider) = data
            .providers
            .iter()
            .find(|provider| provider.identity.id == task.provider_id)
        else {
            return;
        };
        let title = if phase == CheckInPhase::Completed {
            "BalanceHub 签到成功"
        } else {
            "BalanceHub 签到未完成"
        };
        let markdown = format!(
            "**中转站**：{}\n\n**结果**：{message}",
            provider.display_label()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(20),
            notifications::send_provider_notification(
                &app,
                &data.settings,
                provider,
                title,
                markdown,
                false,
            ),
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    #[test]
    fn account_key_shares_a_lease_across_duplicate_provider_cards() {
        let mut first = Provider::from_input(ProviderInput::default(), "first".to_string());
        first.auth.api_user = "123".to_string();
        let mut second = first.clone();
        second.identity.id = "second".to_string();
        assert_eq!(account_key(&first), account_key(&second));
        second.auth.api_user = "456".to_string();
        assert_ne!(account_key(&first), account_key(&second));
    }

    #[test]
    fn human_wait_is_resumable_only_after_browser_resources_are_released() {
        let mut task = CheckInTask {
            run_id: "test".into(),
            provider_id: "p".into(),
            provider_name: "test".into(),
            batch_id: None,
            source: CheckInSource::Batch,
            phase: CheckInPhase::Queued,
            message: String::new(),
            revision: 0,
            finished: false,
            can_resume: false,
            can_cancel: true,
            started_at: 0,
            finished_at: None,
        };
        set_phase(&mut task, CheckInPhase::WaitingHuman, String::new(), true);
        assert!(!task.can_resume);
        set_phase(&mut task, CheckInPhase::WaitingHuman, String::new(), false);
        assert!(task.can_resume && task.can_cancel && !task.finished);
        set_phase(&mut task, CheckInPhase::Saving, String::new(), true);
        assert!(!task.can_cancel && !task.finished);
        set_phase(&mut task, CheckInPhase::Completed, String::new(), false);
        assert!(task.finished && task.finished_at.is_some());
        assert!(!task.can_resume && !task.can_cancel);
    }
}
