//! All check-in entry points enqueue here. Active human waits keep their browser;
//! suspended automatic tasks release it and remain resumable.
mod window;
use crate::{
    adapters::browser::BrowserWindowControl,
    models::{
        provider_domain, CheckInBatch, CheckInError, CheckInPhase, CheckInSource, CheckInTask,
        NotificationEvent, Provider,
    },
    services::{
        notifications,
        provider_service::{ProviderRequestContext, ProviderService},
    },
    util::unix_millis,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::sync::{watch, Semaphore};

const EVENT: &str = "balancehub://check-in-task";
static REVISION: AtomicU64 = AtomicU64::new(0);
static RUNS: OnceLock<Mutex<HashMap<String, Run>>> = OnceLock::new();
/// 结果未确认的提交必须跨重启保留：任务表只在内存里，用户看到“已停止自动
/// 重试”后退出再打开，调度器不能把可能已经发出的登录/签到再提交一次。
const UNCERTAIN_FILE: &str = "check-in-uncertain.json";
const UNCERTAIN_MAX_BYTES: usize = 512 * 1024;
const UNCERTAIN_MAX_ENTRIES: usize = 128;
static UNCERTAIN: OnceLock<Mutex<Vec<UncertainSubmission>>> = OnceLock::new();
pub(super) static HTTP_SLOTS: Semaphore = Semaphore::const_new(6);
pub use window::show as show_window;

struct Run {
    task: CheckInTask,
    account_key: String,
    date: String,
    cancel: watch::Sender<bool>,
    executing: bool,
    context: ProviderRequestContext,
    automatic_attempt: u32,
    window: Option<BrowserWindowControl>,
}

/// 一次可能已提交、但结果未确认的签到。按站点 ID 和登录账号双重匹配，
/// 与进程内的 `blocks_automatic` 保持同一判断口径。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UncertainSubmission {
    provider_id: String,
    account_key: String,
    /// 本地日期（YYYY-MM-DD）。跨天后不再阻止新一天的自动签到。
    date: String,
}

#[derive(Clone)]
pub struct CheckInContext {
    pub run_id: String,
    pub interactive: bool,
}

impl CheckInContext {
    pub fn queued_for_browser(&self) {
        publish(
            &self.run_id,
            CheckInPhase::Queued,
            "正在等待可用签到窗口，同一登录账号的任务依次处理".to_string(),
            true,
        );
    }

    pub fn phase(&self, phase: CheckInPhase) {
        let message = if phase == CheckInPhase::WaitingHuman && self.interactive {
            "请在浏览器中完成验证，完成后自动继续"
        } else if phase == CheckInPhase::WaitingLogin && self.interactive {
            "正在使用绑定账号登录；如需补充登录信息，请在浏览器中完成"
        } else {
            phase.message()
        };
        publish(&self.run_id, phase, message.to_string(), true);
    }

    pub fn authenticated(&self, provider: &Provider) {
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

fn uncertain_path() -> Option<std::path::PathBuf> {
    crate::host::data_dir_optional().map(|directory| directory.join(UNCERTAIN_FILE))
}

fn load_uncertain(path: Option<&Path>, date: &str) -> Vec<UncertainSubmission> {
    let Some(path) = path else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    match crate::util::read_text_file_limited(path, UNCERTAIN_MAX_BYTES, "读取未确认的签到提交")
        .and_then(|text| {
            serde_json::from_str::<Vec<UncertainSubmission>>(&text)
                .map_err(|error| format!("解析未确认的签到提交失败：{error}"))
        }) {
        Ok(mut entries) => {
            entries.retain(|entry| entry.date == date);
            entries.truncate(UNCERTAIN_MAX_ENTRIES);
            entries
        }
        Err(error) => {
            log::warn!("{error}；本次启动按无未确认提交处理");
            Vec::new()
        }
    }
}

fn save_uncertain(path: Option<&Path>, entries: &[UncertainSubmission]) {
    let Some(path) = path else {
        return;
    };
    // cache::write 只接收 Sized 值；当天最多几条未确认记录，复制一次
    // 不构成负担。
    if let Err(error) = crate::util::cache::write(path, &entries.to_vec()) {
        log::warn!("保存未确认的签到提交失败：{error}");
    }
}

fn uncertain_store() -> &'static Mutex<Vec<UncertainSubmission>> {
    UNCERTAIN.get_or_init(|| Mutex::new(load_uncertain(uncertain_path().as_deref(), &today())))
}

fn remember_uncertain_submission(provider_id: &str, account_key: &str, date: &str) {
    let Ok(mut entries) = uncertain_store().lock() else {
        return;
    };
    entries.retain(|entry| entry.date == date);
    let recorded = entries
        .iter()
        .any(|entry| entry.provider_id == provider_id && entry.account_key == account_key);
    if !recorded {
        if entries.len() >= UNCERTAIN_MAX_ENTRIES {
            entries.remove(0);
        }
        entries.push(UncertainSubmission {
            provider_id: provider_id.to_string(),
            account_key: account_key.to_string(),
            date: date.to_string(),
        });
    }
    save_uncertain(uncertain_path().as_deref(), &entries);
}

fn uncertain_blocks(
    entries: &[UncertainSubmission],
    provider_id: &str,
    account_key: &str,
    date: &str,
) -> bool {
    entries.iter().any(|entry| {
        entry.date == date && (entry.provider_id == provider_id || entry.account_key == account_key)
    })
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

pub fn automatic_pending(provider: &Provider) -> bool {
    let date = today();
    let blocked_in_process = runs()
        .lock()
        .map(|runs| {
            runs.values()
                .any(|run| blocks_automatic(run, provider, &date))
        })
        .unwrap_or(true);
    if blocked_in_process {
        return true;
    }
    // 进程重启后任务表为空，但“可能已提交”的事实仍在：持久记录继续挡住
    // 当天的自动重试。用户手动签到不受影响。
    uncertain_store()
        .lock()
        .map(|entries| {
            uncertain_blocks(
                &entries,
                &provider.identity.id,
                &account_key(provider),
                &date,
            )
        })
        .unwrap_or(true)
}

pub fn list() -> Vec<CheckInTask> {
    expire_waiting();
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

pub fn enqueue(
    id: String,
    source: CheckInSource,
    batch_id: Option<String>,
    automatic_attempt: u32,
) -> Result<CheckInTask, String> {
    expire_waiting();
    let provider = crate::host::core_state()
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
    provider_domain::check_in::validate_credentials(&provider)?;
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
            let existing = run.task.clone();
            drop(runs);
            // A user-triggered batch also continues work previously suspended
            // by the scheduler, without requiring a second click per station.
            return if source.allows_interaction() && existing.can_resume {
                resume(&existing.run_id)
            } else {
                Ok(existing)
            };
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
            can_show_window: false,
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
                window: None,
            },
        );
        task
    };
    crate::host::emit(EVENT, &task);
    spawn(task.clone(), receiver, source.allows_interaction());
    Ok(task)
}

pub fn enqueue_all() -> Result<CheckInBatch, String> {
    let providers = crate::host::core_state()
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
        // 一个站点的配置不完整不应该中断整批签到；它只是没被排进队列，
        // 其余站点照常处理，计数里统一显示为「跳过」。
        let provider_id = provider.identity.id.clone();
        match enqueue(
            provider.identity.id,
            CheckInSource::Batch,
            Some(batch_id.clone()),
            0,
        ) {
            Ok(task) => tasks.push(task),
            Err(error) => {
                log::debug!("批量签到跳过（{provider_id}）：{error}");
                skipped += 1;
            }
        }
    }
    Ok(CheckInBatch {
        batch_id,
        tasks,
        skipped,
    })
}

pub fn resume(run_id: &str) -> Result<CheckInTask, String> {
    expire_waiting();
    let (task, receiver) = {
        let mut runs = runs().lock().map_err(|_| "签到任务状态不可用")?;
        let run = runs.get_mut(run_id).ok_or("签到任务不存在")?;
        if run.executing || !run.task.can_resume {
            return Err("当前任务无法继续".to_string());
        }
        let state = crate::host::core_state();
        let data = state.data.read().map_err(|_| "读取中转站失败")?;
        if !data
            .providers
            .iter()
            .any(|provider| provider.runtime.enabled && run.context.matches_check_in(provider))
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
    crate::host::emit(EVENT, &task);
    spawn(task.clone(), receiver, true);
    Ok(task)
}

pub fn cancel(run_id: &str) -> Result<(), String> {
    cancel_with_message(run_id, "签到已取消")
}

fn cancel_with_message(run_id: &str, message: &str) -> Result<(), String> {
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
        publish(run_id, CheckInPhase::Cancelled, message.to_string(), false);
    }
    Ok(())
}

/// 站点被删除、停用或配置被替换后，挂起的浏览器任务不能继续等一个已不存在的配置。
pub fn cancel_outdated() {
    let providers = match crate::host::core_state().data.read() {
        Ok(data) => data.providers.clone(),
        Err(_) => return,
    };
    let ids = outdated_ids(
        &runs().lock().unwrap_or_else(|err| err.into_inner()),
        &providers,
    );
    for id in ids {
        let _ = cancel_with_message(&id, "中转站已删除、停用或账号配置已变更，签到已取消");
    }
}

fn outdated_ids(runs: &HashMap<String, Run>, providers: &[Provider]) -> Vec<String> {
    runs.values()
        .filter(|run| {
            !run.task.finished
                && !providers.iter().any(|provider| {
                    provider.runtime.enabled && run.context.matches_check_in(provider)
                })
        })
        .map(|run| run.task.run_id.clone())
        .collect()
}

fn set_phase(task: &mut CheckInTask, phase: CheckInPhase, message: String, executing: bool) {
    task.phase = phase;
    task.message = message;
    task.finished = phase.finished();
    task.can_resume = phase.waiting() && !executing;
    task.can_cancel = !task.finished && phase != CheckInPhase::Saving;
    if task.finished || !executing || matches!(phase, CheckInPhase::Queued | CheckInPhase::Saving) {
        task.can_show_window = false;
    }
    task.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
    task.finished_at = task.finished.then(|| unix_millis() as u64);
}

fn publish(id: &str, phase: CheckInPhase, message: String, executing: bool) {
    let (task, uncertain) = {
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
        run.task.can_show_window = window::can_show(run);
        if !executing {
            run.window = None;
        }
        // 文件写入放在任务表锁之外，避免一次磁盘操作拖住其他任务的状态。
        let uncertain = (phase == CheckInPhase::Unconfirmed).then(|| {
            (
                run.task.provider_id.clone(),
                run.account_key.clone(),
                run.date.clone(),
            )
        });
        (run.task.clone(), uncertain)
    };
    if let Some((provider_id, account_key, date)) = uncertain {
        remember_uncertain_submission(&provider_id, &account_key, &date);
    }
    crate::host::emit(EVENT, task);
}

/// 等待人工验证的浏览器不会自己收尾；跨天后由调度节拍主动结束，
/// 避免任务中心整晚挂着一个已经无法继续的签到任务。
pub(crate) fn expire_waiting() {
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
            &id,
            CheckInPhase::Cancelled,
            "等待任务已跨天，请重新签到".to_string(),
            false,
        );
    }
}

fn spawn(task: CheckInTask, mut cancelled: watch::Receiver<bool>, interactive: bool) {
    crate::runtime::spawn(async move {
        let context = CheckInContext {
            run_id: task.run_id.clone(),
            interactive,
        };
        let service = ProviderService::new();
        let outcome = tokio::select! {
            // 适配器解析站点响应时可能 panic；一旦任务 future 异常退出，
            // 这里永远不会再发布终态，任务会一直停在“进行中”并拦住退出。
            // 把 panic 收敛成终态，请求可能已提交时按“结果待确认”处理。
            result = async {
                use futures_util::FutureExt;
                std::panic::AssertUnwindSafe(
                    service.check_in_attempt(task.provider_id.clone(), &context),
                )
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    log::error!("签到任务异常退出（{}）", task.provider_id);
                    let phase = runs()
                        .lock()
                        .ok()
                        .and_then(|runs| runs.get(&task.run_id).map(|run| run.task.phase));
                    if phase.is_some_and(CheckInPhase::may_have_submitted) {
                        Err(CheckInError::Unconfirmed(
                            "签到过程异常退出；请求可能已提交，请查看站点记录".to_string(),
                        ))
                    } else {
                        Err(CheckInError::Failed(
                            "签到过程异常退出，请重新签到".to_string(),
                        ))
                    }
                })
            } => result,
            _ = async { if !*cancelled.borrow() { let _ = cancelled.changed().await; } } => {
                let phase = runs().lock().ok().and_then(|runs| runs.get(&task.run_id).map(|run| run.task.phase));
                if phase.is_some_and(CheckInPhase::may_have_submitted) {
                    Err(CheckInError::Unconfirmed("请求可能已经发出；已停止后续提交，请查看站点记录".to_string()))
                } else {
                    publish(&task.run_id, CheckInPhase::Cancelled, "签到已取消".to_string(), false);
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
            Err(CheckInError::WaitingLogin) => (
                CheckInPhase::WaitingLogin,
                CheckInPhase::WaitingLogin.message().to_string(),
            ),
            Err(CheckInError::WaitingBrowser(message)) => (CheckInPhase::WaitingBrowser, message),
            Err(CheckInError::LoginTimeout(message)) => (CheckInPhase::Failed, message),
            Err(CheckInError::Cancelled(message)) => (CheckInPhase::Cancelled, message),
            Err(CheckInError::Unconfirmed(message)) => (CheckInPhase::Unconfirmed, message),
            Err(CheckInError::Failed(message)) => (CheckInPhase::Failed, message),
        };
        publish(&task.run_id, phase, message.clone(), false);
        // 一次签到只影响这一个站点：带上 id 让前端只补这张卡片，批量签到
        // 时不必每完成一个站点就重读整份配置。
        crate::app_events::emit_providers_changed(std::slice::from_ref(&task.provider_id));
        crate::host::refresh_tray();
        // 定时签到停在等待阶段时，界面上的任务中心可能在托盘或后台，
        // 用户不会主动发现。只在无窗口的自动尝试上提醒一次；用户继续
        // 登录后再停住属于当场可见的交互，不再重复通知。
        let waiting_for_human = !phase.finished()
            && phase.waiting()
            && !interactive
            && task.source == CheckInSource::Automatic;
        if phase == CheckInPhase::Cancelled {
            return;
        }
        if !phase.finished() && !waiting_for_human {
            return;
        }
        let attempt = runs()
            .lock()
            .ok()
            .and_then(|runs| runs.get(&task.run_id).map(|run| run.automatic_attempt))
            .unwrap_or(0);
        if !waiting_for_human
            && task.source == CheckInSource::Automatic
            && phase != CheckInPhase::Completed
            && attempt > 1
        {
            return;
        }
        let data = {
            let state = crate::host::core_state();
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
        } else if waiting_for_human {
            "BalanceHub 签到需要继续"
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
                &data.settings,
                provider,
                NotificationEvent::CheckIn,
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
    fn outdated_runs_follow_removed_disabled_or_rewritten_providers() {
        let waiting_run =
            |run_id: &str, provider: &Provider, context: ProviderRequestContext, finished: bool| {
                let task = CheckInTask {
                    run_id: run_id.to_string(),
                    provider_id: provider.identity.id.clone(),
                    provider_name: provider.identity.name.clone(),
                    batch_id: None,
                    source: CheckInSource::Manual,
                    phase: if finished {
                        CheckInPhase::Completed
                    } else {
                        CheckInPhase::WaitingHuman
                    },
                    message: String::new(),
                    revision: 0,
                    finished,
                    can_resume: !finished,
                    can_cancel: !finished,
                    can_show_window: false,
                    started_at: 0,
                    finished_at: None,
                };
                Run {
                    task,
                    account_key: account_key(provider),
                    date: today(),
                    cancel: watch::channel(false).0,
                    executing: false,
                    context,
                    automatic_attempt: 0,
                    window: None,
                }
            };
        let live = Provider::from_input(ProviderInput::default(), "live".to_string());
        let mut disabled = Provider::from_input(ProviderInput::default(), "disabled".to_string());
        disabled.runtime.enabled = false;
        let mut rewritten = Provider::from_input(ProviderInput::default(), "rewritten".to_string());
        let rewritten_context = ProviderRequestContext::capture(&rewritten);
        rewritten.auth.login_password = "new-password".to_string();
        let removed = Provider::from_input(ProviderInput::default(), "removed".to_string());

        let mut runs = HashMap::new();
        for (run_id, provider, context, finished) in [
            (
                "live",
                live.clone(),
                ProviderRequestContext::capture(&live),
                false,
            ),
            (
                "disabled",
                disabled.clone(),
                ProviderRequestContext::capture(&disabled),
                false,
            ),
            ("rewritten", rewritten.clone(), rewritten_context, false),
            (
                "finished",
                live.clone(),
                ProviderRequestContext::capture(&live),
                true,
            ),
            (
                "removed",
                removed.clone(),
                ProviderRequestContext::capture(&removed),
                false,
            ),
        ] {
            runs.insert(
                run_id.to_string(),
                waiting_run(run_id, &provider, context, finished),
            );
        }

        let mut outdated = outdated_ids(&runs, &[live, disabled, rewritten]);
        outdated.sort();
        // 已结束的任务不能再被取消；停用、凭据改写或整站删除的等待任务必须结束，
        // 否则任务中心会一直挂着一个永远等不到结果的签到。
        assert_eq!(outdated, vec!["disabled", "removed", "rewritten"]);
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
            can_show_window: false,
            started_at: 0,
            finished_at: None,
        };
        set_phase(&mut task, CheckInPhase::WaitingHuman, String::new(), true);
        assert!(!task.can_resume);
        task.can_show_window = true;
        set_phase(&mut task, CheckInPhase::WaitingHuman, String::new(), false);
        assert!(task.can_resume && task.can_cancel && !task.finished);
        assert!(!task.can_show_window);
        set_phase(&mut task, CheckInPhase::WaitingLogin, String::new(), true);
        assert!(!task.can_resume && task.can_cancel);
        set_phase(&mut task, CheckInPhase::WaitingLogin, String::new(), false);
        assert!(task.can_resume && task.can_cancel && !task.finished);
        set_phase(&mut task, CheckInPhase::LoggingIn, String::new(), true);
        assert!(task.phase.may_have_submitted());
        assert!(!task.can_resume && task.can_cancel);
        task.can_show_window = true;
        set_phase(&mut task, CheckInPhase::Saving, String::new(), true);
        assert!(!task.can_cancel && !task.finished);
        assert!(!task.can_show_window);
        set_phase(&mut task, CheckInPhase::Completed, String::new(), false);
        assert!(task.finished && task.finished_at.is_some());
        assert!(!task.can_resume && !task.can_cancel);
    }

    #[test]
    fn user_started_batches_keep_human_verification_interactive() {
        assert!(CheckInSource::Manual.allows_interaction());
        assert!(CheckInSource::Batch.allows_interaction());
        assert!(!CheckInSource::Automatic.allows_interaction());
    }

    #[test]
    fn an_uncertain_submission_blocks_the_same_site_and_account_after_a_restart() {
        let mut first = Provider::from_input(ProviderInput::default(), "first".to_string());
        first.auth.api_user = "42".to_string();
        let mut duplicate = first.clone();
        duplicate.identity.id = "duplicate".to_string();
        let date = "2026-07-10";
        let entries = vec![UncertainSubmission {
            provider_id: first.identity.id.clone(),
            account_key: account_key(&first),
            date: date.to_string(),
        }];
        // 重启后内存任务已清空，持久记录仍按站点 ID 或同一登录账号挡住自动重试。
        assert!(uncertain_blocks(
            &entries,
            &first.identity.id,
            "rewritten-key",
            date
        ));
        assert!(uncertain_blocks(
            &entries,
            &duplicate.identity.id,
            &account_key(&duplicate),
            date
        ));
        assert!(!uncertain_blocks(
            &entries,
            &duplicate.identity.id,
            &account_key(&duplicate),
            "2026-07-11"
        ));
        let mut other = duplicate.clone();
        other.identity.id = "other".to_string();
        other.auth.api_user = "43".to_string();
        assert!(!uncertain_blocks(
            &entries,
            &other.identity.id,
            &account_key(&other),
            date
        ));
    }

    #[test]
    fn uncertain_submissions_round_trip_and_expire_with_the_local_date() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-uncertain-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        let path = directory.join(UNCERTAIN_FILE);
        let entry = UncertainSubmission {
            provider_id: "fixture".to_string(),
            account_key: "fixture-key".to_string(),
            date: "2026-07-10".to_string(),
        };
        save_uncertain(Some(&path), std::slice::from_ref(&entry));
        let restored = load_uncertain(Some(&path), "2026-07-10");
        assert_eq!(restored, vec![entry.clone()]);
        assert!(uncertain_blocks(
            &restored,
            &entry.provider_id,
            &entry.account_key,
            "2026-07-10"
        ));
        // 新的一天不能继承昨天的抑制；加载时顺手丢掉过期记录。
        assert!(load_uncertain(Some(&path), "2026-07-11").is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
