use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT, PROVIDERS_CHANGED_EVENT},
    models::{provider_domain, AppSettings, NotificationEvent, Provider, ProviderStatus},
    services::{notifications, provider_service::ProviderService},
    util::{unix_millis, unix_secs},
};

struct SchedulerTaskDescriptor {
    id: &'static str,
    kind: &'static str,
    title: &'static str,
}

const AUTO_REFRESH_TASK: SchedulerTaskDescriptor = SchedulerTaskDescriptor {
    id: "scheduler-refresh",
    kind: "autoRefresh",
    title: "自动刷新中转站",
};
const AUTO_LIVENESS_TASK: SchedulerTaskDescriptor = SchedulerTaskDescriptor {
    id: "scheduler-liveness",
    kind: "autoLiveness",
    title: "自动测活中转站",
};

fn emit_background_task(
    task: &SchedulerTaskDescriptor,
    status: &str,
    detail: impl Into<String>,
    progress: Option<f32>,
    started_at: u64,
    error: Option<String>,
) {
    crate::host::emit(
        BACKGROUND_TASK_EVENT,
        BackgroundTaskEvent {
            can_show_window: None,
            login_account_id: None,
            provider_id: None,
            can_cancel: None,
            task_id: task.id.to_string(),
            kind: task.kind.to_string(),
            status: status.to_string(),
            title: task.title.to_string(),
            detail: detail.into(),
            progress,
            started_at,
            finished_at: if status == "running" {
                None
            } else {
                Some(unix_millis() as u64)
            },
            error,
        },
    );
}

/// 调度节拍。签到按「本地时间 ≥ 设定点且当天未签到」判定，刷新/测活按到期时间判定，
/// 30s 粒度足够精确，开销极低。每个 tick 跑完再 sleep（而非固定频率），天然避免自身重叠。
const TICK_SECS: u64 = 30;
/// 自动测活并发上限，沿用原前端实现，避免一批到期时被单个最长超时拖垮。
const LIVENESS_CONCURRENCY: usize = 3;
/// 首轮执行前的等待：给前端 webview 注册 `providers-changed` 监听留出时间，
/// 确保启动后的首次刷新/签到结果能通过事件回流到界面。
const INITIAL_DELAY_SECS: u64 = 5;
/// 自动签到当日尝试上限：签到接口按天发奖，失败大多是凭据/站点问题，重试太多没有意义。
const CHECK_IN_MAX_ATTEMPTS_PER_DAY: u32 = 3;
/// 自动签到两次尝试之间的退避间隔。
const CHECK_IN_RETRY_BACKOFF_SECS: u64 = 30 * 60;
/// 刷新抖动上限：刷新间隔的十分之一，且不超过 5 分钟。
const REFRESH_JITTER_MAX_SECS: u64 = 300;
/// 单个中转站当日自动签到的尝试记录。
///
/// 「失败后何时重试、何时放弃」的状态落在本机数据目录里，与展示用的
/// `runtime.error_message` 彻底解耦 —— 旧实现把重试抑制寄生在错误文案前缀上，
/// 刷新成功清空文案就会意外解除抑制，形成「重签→失败→再通知」的全天循环。
/// 持久化只影响重启后的重试节奏：同一天重启不会重新获得三次尝试机会。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckInAttemptState {
    /// 本地日期（YYYY-MM-DD），跨天后视为全新一天。
    date: String,
    attempts: u32,
    last_attempt_secs: u64,
}

/// 自动签到重试状态。属于运行读数而非用户配置：损坏或丢失时回退到
/// 「重启后允许重新尝试」，和旧版内存行为一致，不影响签到结果本身。
const CHECK_IN_ATTEMPT_FILE: &str = "check-in-attempts.json";
const CHECK_IN_ATTEMPT_MAX_BYTES: usize = 512 * 1024;

fn check_in_attempt_path() -> Option<std::path::PathBuf> {
    crate::host::data_dir_optional().map(|directory| directory.join(CHECK_IN_ATTEMPT_FILE))
}

fn load_check_in_attempts() -> HashMap<String, CheckInAttemptState> {
    let Some(path) = check_in_attempt_path() else {
        return HashMap::new();
    };
    if !path.exists() {
        return HashMap::new();
    }
    let text = match crate::util::read_text_file_limited(
        &path,
        CHECK_IN_ATTEMPT_MAX_BYTES,
        "读取自动签到重试状态",
    ) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("{error}；本次启动按无重试记录处理");
            return HashMap::new();
        }
    };
    match serde_json::from_str(&text) {
        Ok(attempts) => attempts,
        Err(error) => {
            log::warn!("自动签到重试状态格式无效，已忽略：{error}");
            HashMap::new()
        }
    }
}

fn save_check_in_attempts(attempts: &HashMap<String, CheckInAttemptState>) {
    let Some(path) = check_in_attempt_path() else {
        return;
    };
    if let Err(error) = crate::util::cache::write(&path, attempts) {
        log::warn!("保存自动签到重试状态失败：{error}");
    }
}

/// 调度器跨 tick 的状态。刷新尝试只影响进程内的下一次 tick；自动签到尝试
/// 从数据目录恢复，避免同一天反复重启把失败站点的三次上限重新清零。
///
/// 失败后的冷却不在这里：它写在 `runtime.cooldown_until` 上，随配置持久化，
/// 卡片和调度器读同一份，重启后也不会立刻重试一个已经进入长冷却的站点。
#[derive(Default)]
struct SchedulerState {
    /// 每个中转站「上次发起刷新」的时刻（秒），避免刷新失败时每 tick 重试。
    refresh_attempts: HashMap<String, u64>,
    check_in_attempts: HashMap<String, CheckInAttemptState>,
}

impl SchedulerState {
    fn load() -> Self {
        Self {
            check_in_attempts: load_check_in_attempts(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct AutomationBatchResult {
    changed: bool,
    failed: usize,
}

/// 启动后台调度任务。
///
/// 调度逻辑原先放在前端 webview 的 `window.setInterval` 里，关窗到托盘 / 系统休眠 /
/// webview 后台定时器节流 / 任意 JS 异常都会让自动签到、测活、刷新静默停摆。
/// 现下沉到 Rust 后台任务：只要进程存活（含隐藏到托盘）就持续运行；配合开机自启，
/// 实际等价于「常驻」。完全退出 App 仍不会运行——那需要 OS 级调度（launchd/任务计划），
/// 属于后续可选增强。
pub fn start() {
    crate::runtime::spawn(async move {
        let mut state = SchedulerState::load();
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            // 单个站点的适配器 panic 已在各自链路内收敛；这里再兜一层，
            // 避免任何漏网异常让自动刷新/签到/测活整条调度循环静默停摆。
            use futures_util::FutureExt;
            if std::panic::AssertUnwindSafe(run_tick(&mut state))
                .catch_unwind()
                .await
                .is_err()
            {
                log::error!("自动调度本轮异常退出，等待下一轮重试");
            }
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
        }
    });
}

async fn run_tick(state: &mut SchedulerState) {
    let service = ProviderService::background();
    // 等待人工验证的签到任务不会自己收尾；每个节拍检查一次是否已跨天，
    // 让任务中心不用等到窗口重新获得焦点才清理过期条目。
    super::check_in_tasks::expire_waiting();
    // 配置加载失败（storage 保护态）时暂停所有自动化，避免基于残缺状态误操作。
    let Ok(data) = service.load_app_data_async().await else {
        return;
    };
    let settings = &data.settings;
    let now_secs = unix_secs();
    let now_millis = unix_millis();
    let today = local_date_today();
    if prune_state(state, &data.providers, &today) {
        save_check_in_attempts(&state.check_in_attempts);
    }
    let mut changed = false;

    // ---- 自动刷新余额 ----
    if settings.auto_refresh_enabled {
        let due: Vec<String> = data
            .providers
            .iter()
            .filter(|provider| provider.runtime.enabled)
            .filter(|provider| {
                provider_domain::failure::refresh_due(
                    provider,
                    settings,
                    now_secs,
                    &state.refresh_attempts,
                )
            })
            .map(|provider| provider.identity.id.clone())
            .collect();
        if !due.is_empty() {
            let task_started_at = unix_millis() as u64;
            emit_background_task(
                &AUTO_REFRESH_TASK,
                "running",
                format!("正在同步 {} 个到期中转站", due.len()),
                None,
                task_started_at,
                None,
            );
            // 闸门被手动刷新占用时跳过本轮且不记尝试，下个 tick 重新评估到期。
            match service.try_refresh_by_ids(due.clone()).await {
                None => {
                    emit_background_task(
                        &AUTO_REFRESH_TASK,
                        "success",
                        "手动刷新正在进行，本轮自动刷新已顺延",
                        Some(1.0),
                        task_started_at,
                        None,
                    );
                }
                Some(Ok(result)) => {
                    let failed_ids = result
                        .updated_providers
                        .iter()
                        .filter(|provider| matches!(provider.runtime.status, ProviderStatus::Error))
                        .map(|provider| provider.identity.id.clone())
                        .collect::<Vec<_>>();
                    record_refresh_outcome(state, settings, &data.providers, &due, now_secs);
                    changed = true;
                    let failed = failed_ids.len();
                    emit_background_task(
                        &AUTO_REFRESH_TASK,
                        if failed > 0 { "failed" } else { "success" },
                        if failed > 0 {
                            format!("已完成 {} 个站点，{} 个站点失败", due.len(), failed)
                        } else {
                            format!("已完成 {} 个站点的余额和模型同步", due.len())
                        },
                        Some(1.0),
                        task_started_at,
                        (failed > 0).then(|| format!("{} 个站点刷新失败", failed)),
                    );
                    notify_refresh_failures(
                        settings,
                        &data.providers,
                        &due,
                        &result.updated_providers,
                        task_started_at / 1000,
                    )
                    .await;
                }
                Some(Err(error)) => {
                    // 整批请求失败（存储/快照问题）：没有可写的站点状态，
                    // 只记本轮尝试，下个正常间隔再评估。
                    record_refresh_outcome(state, settings, &data.providers, &due, now_secs);
                    emit_background_task(
                        &AUTO_REFRESH_TASK,
                        "failed",
                        "自动刷新请求失败",
                        Some(1.0),
                        task_started_at,
                        Some(error),
                    );
                }
            }
        }
    }

    // ---- 自动签到 ----
    if settings.auto_check_in_enabled {
        let due: Vec<Provider> = data
            .providers
            .iter()
            .filter(|provider| {
                provider.runtime.enabled
                    && provider_domain::capabilities::supports_check_in(provider)
                    && !provider_domain::capabilities::checked_in_today(provider)
                    && provider_domain::automation::check_in_due_now(provider, settings)
                    && check_in_attempt_allowed(
                        state.check_in_attempts.get(&provider.identity.id),
                        &today,
                        now_secs,
                    )
            })
            .cloned()
            .collect();
        let mut recorded = false;
        for provider in due {
            if super::check_in_tasks::automatic_pending(&provider) {
                continue;
            }
            let attempt =
                next_check_in_attempt(state.check_in_attempts.get(&provider.identity.id), &today);
            // 只有真正排进签到队列才消耗当天的自动尝试次数。站点在快照之后
            // 被停用或凭据被清空时 enqueue 会拒绝，不能让它空耗一次机会和
            // 随后的冷却时间。
            match super::check_in_tasks::enqueue(
                provider.identity.id.clone(),
                crate::models::CheckInSource::Automatic,
                None,
                attempt,
            ) {
                Ok(_) => {
                    record_check_in_attempt(
                        &mut state.check_in_attempts,
                        &provider,
                        &today,
                        now_secs,
                    );
                    recorded = true;
                }
                Err(error) => log::debug!("自动签到未能排队（{}）：{error}", provider.identity.id),
            }
        }
        if recorded {
            save_check_in_attempts(&state.check_in_attempts);
        }
    }

    // ---- 自动测活 ----
    // 测活要启动本机 Agent CLI；没有安装执行器的宿主（服务端）直接跳过，
    // 否则每个到期站点都会以「当前宿主不支持本地测活」计入失败。
    let liveness_available = super::liveness::executor().is_some();
    let due_liveness: Vec<Provider> = data
        .providers
        .iter()
        .filter(|provider| {
            liveness_available
                && provider_domain::liveness::automatic_enabled(provider, settings)
                && provider_domain::liveness::is_due(
                    provider,
                    now_millis,
                    u128::from(super::liveness::max_interval(settings, provider)) * 1000,
                )
                && !super::liveness::waits_for_agent_update(settings, provider)
        })
        .cloned()
        .collect();
    if !due_liveness.is_empty() {
        let task_started_at = unix_millis() as u64;
        emit_background_task(
            &AUTO_LIVENESS_TASK,
            "running",
            format!("正在检测 {} 个到期站点", due_liveness.len()),
            None,
            task_started_at,
            None,
        );
        let liveness_summary = run_due_liveness(settings, due_liveness).await;
        if liveness_summary.changed {
            changed = true;
        }
        emit_background_task(
            &AUTO_LIVENESS_TASK,
            if liveness_summary.failed > 0 {
                "failed"
            } else {
                "success"
            },
            if liveness_summary.failed > 0 {
                format!("自动测活已完成，{} 个站点失败", liveness_summary.failed)
            } else {
                "自动测活任务已完成".to_string()
            },
            Some(1.0),
            task_started_at,
            (liveness_summary.failed > 0)
                .then(|| format!("{} 个站点测活失败", liveness_summary.failed)),
        );
    }

    if changed {
        crate::host::refresh_tray();
        crate::host::emit(PROVIDERS_CHANGED_EVENT, ());
    }
}

/// 清理调度器内存状态：删掉已不存在的中转站与过期日期的记录，避免无界增长。
/// 返回是否真的删掉了签到记录；只有变化时才落盘，避免每 30 秒重写一次。
fn prune_state(state: &mut SchedulerState, providers: &[Provider], today: &str) -> bool {
    let exists = |id: &str| providers.iter().any(|provider| provider.identity.id == id);
    state.refresh_attempts.retain(|id, _| exists(id));
    let before = state.check_in_attempts.len();
    state
        .check_in_attempts
        .retain(|id, attempt| exists(id) && attempt.date == today);
    state.check_in_attempts.len() != before
}

/// 记录本轮刷新的发起时刻。失败后的下一次尝试由刷新流程写在
/// `runtime.cooldown_until` 上（见 `provider_domain::failure`），这里不再维护
/// 第二份重试时刻；成功且开启抖动的站点把尝试时刻整体后移，避免整批刷新
/// 之后仍然同一秒集中打满网络。
fn record_refresh_outcome(
    state: &mut SchedulerState,
    settings: &AppSettings,
    providers: &[Provider],
    attempted: &[String],
    now_secs: u64,
) {
    for id in attempted {
        state.refresh_attempts.insert(id.clone(), now_secs);
        if !settings.refresh_jitter {
            continue;
        }
        let interval = providers
            .iter()
            .find(|provider| provider.identity.id == *id)
            .map(|provider| provider_domain::automation::refresh_interval(provider, settings))
            .unwrap_or(settings.refresh_interval);
        let span = (interval / 10).min(REFRESH_JITTER_MAX_SECS);
        if span > 0 {
            state
                .refresh_attempts
                .insert(id.clone(), now_secs + jitter_offset(id, now_secs, span));
        }
    }
}

/// 稳定的伪随机偏移：对「中转站 ID + 当前轮次」做 FNV-1a 哈希。
/// 不引入随机数依赖，同一轮里各站点也会拿到不同偏移。
fn jitter_offset(id: &str, seed: u64, span: u64) -> u64 {
    if span == 0 {
        return 0;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= seed;
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    hash % span
}

/// 是否允许发起（又一次）自动签到尝试：当日未达上限，且距上次尝试已过退避间隔。
fn check_in_attempt_allowed(
    attempt: Option<&CheckInAttemptState>,
    today: &str,
    now_secs: u64,
) -> bool {
    match attempt {
        None => true,
        Some(state) if state.date != today => true,
        Some(state) => {
            // 系统时钟回拨后旧尝试时间落在未来；按无退避处理，否则当天
            // 剩余尝试会被拖到时钟追上。每日次数上限仍然生效。
            let clock_rolled_back = state.last_attempt_secs
                > now_secs.saturating_add(provider_domain::CLOCK_ROLLBACK_TOLERANCE_SECS);
            state.attempts < CHECK_IN_MAX_ATTEMPTS_PER_DAY
                && (clock_rolled_back
                    || now_secs
                        >= state
                            .last_attempt_secs
                            .saturating_add(CHECK_IN_RETRY_BACKOFF_SECS))
        }
    }
}

/// 在发起请求前登记本次尝试（失败或挂起都算一次），返回这是今天的第几次。
fn next_check_in_attempt(current: Option<&CheckInAttemptState>, today: &str) -> u32 {
    match current {
        Some(state) if state.date == today => state.attempts.saturating_add(1),
        _ => 1,
    }
}

fn record_check_in_attempt(
    attempts: &mut HashMap<String, CheckInAttemptState>,
    provider: &Provider,
    today: &str,
    now_secs: u64,
) -> u32 {
    let entry = attempts
        .entry(provider.identity.id.clone())
        .and_modify(|state| {
            if state.date == today {
                state.attempts += 1;
            } else {
                state.date = today.to_string();
                state.attempts = 1;
            }
            state.last_attempt_secs = now_secs;
        })
        .or_insert_with(|| CheckInAttemptState {
            date: today.to_string(),
            attempts: 1,
            last_attempt_secs: now_secs,
        });
    entry.attempts
}

/// 自动刷新的边沿触发通知：只对「本轮刷新中从非 Error 翻转为 Error」的中转站各发一条。
/// 持续失败的站点在下一次翻转（恢复后再失败）前不会重复通知。
async fn notify_refresh_failures(
    settings: &AppSettings,
    before: &[Provider],
    refreshed_ids: &[String],
    after: &[Provider],
    batch_started_secs: u64,
) {
    for id in refreshed_ids {
        let was_error = before
            .iter()
            .find(|provider| provider.identity.id == *id)
            .is_some_and(|provider| matches!(provider.runtime.status, ProviderStatus::Error));
        let Some(current) = after.iter().find(|provider| provider.identity.id == *id) else {
            continue;
        };
        let is_error = matches!(current.runtime.status, ProviderStatus::Error);
        if is_error && !was_error {
            // 凭据被拒时刷新链路已经接手自动重新认证：成功会静默修复卡片，
            // 失败也会由重新认证自己通知。这里再报一次“刷新失败”只会让
            // 用户在修复已经开始后收到一条已经过时的告警。
            if crate::services::provider_browser_login::attempted_since(id, batch_started_secs) {
                log::debug!("自动重新认证已接手，跳过刷新失败通知（{id}）");
                continue;
            }
            let message = current
                .runtime
                .error_message
                .as_deref()
                .unwrap_or("刷新失败");
            notify_provider_event(
                settings,
                current,
                NotificationEvent::RefreshFailure,
                "BalanceHub 刷新失败",
                message,
            )
            .await;
        }
    }
}

/// 滚动并发跑到期测活（最多 3 个在飞，一个完成下一个立刻补位），
/// 返回是否有状态变更及失败数量。
/// 测活从「上次成功」翻转为「本次失败」时发一条边沿触发通知。
async fn run_due_liveness(settings: &AppSettings, due: Vec<Provider>) -> AutomationBatchResult {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(LIVENESS_CONCURRENCY));
    let mut handles = Vec::with_capacity(due.len());
    for provider in &due {
        // 在 async 侧先拿许可再 spawn_blocking，许可随任务结束释放，形成滚动窗口。
        let Ok(permit) = Arc::clone(&semaphore).acquire_owned().await else {
            break;
        };
        let id = provider.identity.id.clone();
        // 测活是阻塞型（spawn 子进程并等待），必须放到 spawn_blocking，避免占用 async 线程。
        handles.push(crate::runtime::spawn_blocking(move || {
            let _permit = permit;
            ProviderService::new().run_liveness(id, None, true)
        }));
    }

    let mut summary = AutomationBatchResult::default();
    for (provider, handle) in due.iter().zip(handles) {
        match handle.await {
            Ok(Ok(result)) => {
                summary.changed = true;
                if !result.ok {
                    summary.failed += 1;
                }
                if !result.ok {
                    // 单次失败可能只是一次抖动：只在恰好跨过连续失败阈值的那一次通知，
                    // 阈值设成 1 就退化为「首次失败即通知」。
                    let trailing_failures = provider
                        .liveness
                        .records
                        .iter()
                        .rev()
                        .take_while(|record| !record.ok)
                        .count() as u64;
                    if trailing_failures + 1 == settings.liveness_failure_threshold {
                        notify_provider_event(
                            settings,
                            provider,
                            NotificationEvent::LivenessFailure,
                            "BalanceHub 测活失败",
                            non_empty(&result.message, "测活失败"),
                        )
                        .await;
                    }
                }
            }
            Ok(Err(_)) | Err(_) => summary.failed += 1,
        }
    }
    summary
}

async fn notify_provider_event(
    settings: &AppSettings,
    provider: &Provider,
    event: NotificationEvent,
    title: &str,
    message: &str,
) {
    let markdown = format!(
        "**中转站**：{}\n\n**结果**：{message}",
        provider.display_label()
    );
    let result = notifications::send_provider_notification(
        settings, provider, event, title, markdown, false,
    )
    .await;
    for delivery in result.results.iter().filter(|delivery| !delivery.ok) {
        log::warn!(
            "后台通知发送失败: provider={}, channel={}({:?}), error={}",
            provider.identity.id,
            delivery.channel_name,
            delivery.channel_kind,
            delivery.message
        );
    }
}

fn local_date_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderInput;

    fn provider(id: &str) -> Provider {
        Provider::from_input(ProviderInput::default(), id.to_string())
    }

    fn attempt(date: &str, attempts: u32, last_attempt_secs: u64) -> CheckInAttemptState {
        CheckInAttemptState {
            date: date.to_string(),
            attempts,
            last_attempt_secs,
        }
    }

    #[test]
    fn allows_first_attempt_and_new_day() {
        assert!(check_in_attempt_allowed(None, "2026-07-10", 1_000));
        assert!(check_in_attempt_allowed(
            Some(&attempt("2026-07-09", 3, 900)),
            "2026-07-10",
            1_000
        ));
    }

    #[test]
    fn next_attempt_matches_what_would_be_recorded() {
        let mut attempts = std::collections::HashMap::new();
        assert_eq!(next_check_in_attempt(None, "2026-07-10"), 1);
        let target = provider("scheduler-test");
        assert_eq!(
            record_check_in_attempt(&mut attempts, &target, "2026-07-10", 1_000),
            1
        );
        assert_eq!(
            next_check_in_attempt(attempts.get(&target.identity.id), "2026-07-10"),
            2
        );
        assert_eq!(
            record_check_in_attempt(&mut attempts, &target, "2026-07-10", 1_100),
            2
        );
        assert_eq!(
            next_check_in_attempt(attempts.get(&target.identity.id), "2026-07-11"),
            1
        );
    }

    #[test]
    fn blocks_attempt_within_backoff_window() {
        let state = attempt("2026-07-10", 1, 1_000);
        assert!(!check_in_attempt_allowed(
            Some(&state),
            "2026-07-10",
            1_000 + 60
        ));
        assert!(check_in_attempt_allowed(
            Some(&state),
            "2026-07-10",
            1_000 + CHECK_IN_RETRY_BACKOFF_SECS
        ));
    }

    #[test]
    fn a_backward_clock_does_not_extend_the_retry_backoff() {
        // 时钟回拨一小时，上次尝试落在未来 55 分钟，不应因此拖到时钟追上。
        let state = attempt("2026-07-10", 1, 10_000);
        assert!(check_in_attempt_allowed(
            Some(&state),
            "2026-07-10",
            10_000 - 3_600
        ));
        // 每日上限不受时钟回拨影响。
        let capped = attempt("2026-07-10", CHECK_IN_MAX_ATTEMPTS_PER_DAY, 10_000);
        assert!(!check_in_attempt_allowed(
            Some(&capped),
            "2026-07-10",
            10_000 - 3_600
        ));
    }

    #[test]
    fn blocks_attempt_after_daily_limit() {
        let state = attempt("2026-07-10", CHECK_IN_MAX_ATTEMPTS_PER_DAY, 1_000);
        assert!(!check_in_attempt_allowed(
            Some(&state),
            "2026-07-10",
            1_000 + CHECK_IN_RETRY_BACKOFF_SECS * 10
        ));
    }

    #[test]
    fn record_attempt_counts_per_day_and_resets_across_days() {
        let mut attempts = HashMap::new();
        let provider = provider("p1");
        assert_eq!(
            record_check_in_attempt(&mut attempts, &provider, "2026-07-10", 1_000),
            1
        );
        assert_eq!(
            record_check_in_attempt(&mut attempts, &provider, "2026-07-10", 2_000),
            2
        );
        assert_eq!(
            record_check_in_attempt(&mut attempts, &provider, "2026-07-11", 3_000),
            1
        );
    }

    #[test]
    fn prune_drops_removed_providers_and_stale_dates() {
        let mut state = SchedulerState::default();
        state.refresh_attempts.insert("kept".to_string(), 1);
        state.refresh_attempts.insert("removed".to_string(), 1);
        state
            .check_in_attempts
            .insert("kept".to_string(), attempt("2026-07-10", 1, 1));
        state
            .check_in_attempts
            .insert("stale-date".to_string(), attempt("2026-07-09", 1, 1));

        let providers = vec![provider("kept"), provider("stale-date")];
        assert!(prune_state(&mut state, &providers, "2026-07-10"));
        assert!(!prune_state(&mut state, &providers, "2026-07-10"));

        assert!(state.refresh_attempts.contains_key("kept"));
        assert!(!state.refresh_attempts.contains_key("removed"));
        assert!(state.check_in_attempts.contains_key("kept"));
        assert!(!state.check_in_attempts.contains_key("stale-date"));
    }

    #[test]
    fn check_in_attempts_survive_a_restart_round_trip() {
        let mut attempts = HashMap::new();
        record_check_in_attempt(&mut attempts, &provider("p1"), "2026-07-10", 1_700_000_000);
        let text = serde_json::to_string(&attempts).unwrap();
        let restored: HashMap<String, CheckInAttemptState> = serde_json::from_str(&text).unwrap();

        assert_eq!(restored, attempts);
        assert!(!check_in_attempt_allowed(
            restored.get("p1"),
            "2026-07-10",
            1_700_000_000 + 60
        ));
    }
}
