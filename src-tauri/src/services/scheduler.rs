use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::{
    models::{provider_domain, AppSettings, Provider, ProviderStatus},
    providers::newapi_http::provider_is_anyrouter,
    services::{notifications, provider_service::ProviderService},
    tray,
    util::{unix_millis, unix_secs},
};

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
/// 前端监听此事件后重新拉取内存状态刷新视图。
pub const PROVIDERS_CHANGED_EVENT: &str = "providers-changed";

/// 单个中转站当日自动签到的尝试记录。
///
/// 「失败后何时重试、何时放弃」的状态只活在调度器内存里，与展示用的
/// `runtime.error_message` 彻底解耦 —— 旧实现把重试抑制寄生在错误文案前缀上，
/// 刷新成功清空文案就会意外解除抑制，形成「重签→失败→再通知」的全天循环。
#[derive(Debug, Clone, PartialEq)]
struct CheckInAttemptState {
    /// 本地日期（YYYY-MM-DD），跨天后视为全新一天。
    date: String,
    attempts: u32,
    last_attempt_secs: u64,
}

/// 调度器跨 tick 的内存状态。进程重启后清零：代价只是重启后允许重新尝试一轮，可接受。
#[derive(Default)]
struct SchedulerState {
    /// 每个中转站「上次发起刷新」的时刻（秒），避免刷新失败时每 tick 重试。
    refresh_attempts: HashMap<String, u64>,
    check_in_attempts: HashMap<String, CheckInAttemptState>,
}

/// 启动后台调度任务。
///
/// 调度逻辑原先放在前端 webview 的 `window.setInterval` 里，关窗到托盘 / 系统休眠 /
/// webview 后台定时器节流 / 任意 JS 异常都会让自动签到、测活、刷新静默停摆。
/// 现下沉到 Rust 后台任务：只要进程存活（含隐藏到托盘）就持续运行；配合开机自启，
/// 实际等价于「常驻」。完全退出 App 仍不会运行——那需要 OS 级调度（launchd/任务计划），
/// 属于后续可选增强。
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut state = SchedulerState::default();
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            run_tick(&app, &mut state).await;
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
        }
    });
}

async fn run_tick(app: &AppHandle, state: &mut SchedulerState) {
    let service = ProviderService::new(app);
    // 配置加载失败（storage 保护态）时暂停所有自动化，避免基于残缺状态误操作。
    let Ok(data) = service.load_app_data() else {
        return;
    };
    let settings = &data.settings;
    let now_secs = unix_secs();
    let now_millis = unix_millis();
    let today = local_date_today();
    prune_state(state, &data.providers, &today);
    let mut changed = false;

    // ---- 自动刷新余额 ----
    if settings.auto_refresh_enabled {
        let due: Vec<String> = data
            .providers
            .iter()
            .filter(|provider| provider.runtime.enabled)
            .filter(|provider| {
                provider_domain::automation::refresh_due(
                    provider,
                    settings,
                    now_secs,
                    &state.refresh_attempts,
                )
            })
            .map(|provider| provider.identity.id.clone())
            .collect();
        if !due.is_empty() {
            // 闸门被手动刷新占用时跳过本轮且不记尝试，下个 tick 重新评估到期。
            match service.try_refresh_by_ids(due.clone()).await {
                None => {}
                Some(outcome) => {
                    for id in &due {
                        state.refresh_attempts.insert(id.clone(), now_secs);
                    }
                    if let Ok(result) = outcome {
                        changed = true;
                        notify_refresh_failures(
                            app,
                            settings,
                            &data.providers,
                            &due,
                            &result.providers,
                        )
                        .await;
                    }
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
                let is_anyrouter = provider_is_anyrouter(provider);
                provider.runtime.enabled
                    && provider_domain::capabilities::supports_check_in(provider, is_anyrouter)
                    && !provider_domain::capabilities::checked_in_today(provider, is_anyrouter)
                    && provider_domain::automation::check_in_due_now(provider, settings)
                    && check_in_attempt_allowed(
                        state.check_in_attempts.get(&provider.identity.id),
                        &today,
                        now_secs,
                    )
            })
            .cloned()
            .collect();
        for provider in due {
            let attempt =
                record_check_in_attempt(&mut state.check_in_attempts, &provider, &today, now_secs);
            if run_auto_check_in(app, &service, &provider, settings, attempt).await {
                changed = true;
            }
        }
    }

    // ---- 自动测活 ----
    let due_liveness: Vec<Provider> = data
        .providers
        .iter()
        .filter(|provider| {
            provider_domain::liveness::automatic_enabled(provider, settings)
                && provider_domain::liveness::is_due(provider, now_millis)
        })
        .cloned()
        .collect();
    if run_due_liveness(app, settings, due_liveness).await {
        changed = true;
    }

    if changed {
        tray::refresh_from_state(app);
        let _ = app.emit(PROVIDERS_CHANGED_EVENT, ());
    }
}

/// 清理调度器内存状态：删掉已不存在的中转站与过期日期的记录，避免无界增长。
fn prune_state(state: &mut SchedulerState, providers: &[Provider], today: &str) {
    let exists = |id: &str| providers.iter().any(|provider| provider.identity.id == id);
    state.refresh_attempts.retain(|id, _| exists(id));
    state
        .check_in_attempts
        .retain(|id, attempt| exists(id) && attempt.date == today);
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
            state.attempts < CHECK_IN_MAX_ATTEMPTS_PER_DAY
                && now_secs
                    >= state
                        .last_attempt_secs
                        .saturating_add(CHECK_IN_RETRY_BACKOFF_SECS)
        }
    }
}

/// 在发起请求前登记本次尝试（失败或挂起都算一次），返回这是今天的第几次。
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

/// 自动签到，返回是否产生了需要刷新视图的状态变更。
///
/// 失败仍写入 `error_message` 供界面展示，但重试与否只由调度器的尝试记录决定；
/// 通知只在当日首次失败时发送一条，后续静默重试，避免轰炸。
async fn run_auto_check_in(
    app: &AppHandle,
    service: &ProviderService<'_>,
    provider: &Provider,
    settings: &AppSettings,
    attempt: u32,
) -> bool {
    let retry_hint = if attempt < CHECK_IN_MAX_ATTEMPTS_PER_DAY {
        format!(
            "，约 {} 分钟后自动重试（今日最多 {} 次）",
            CHECK_IN_RETRY_BACKOFF_SECS / 60,
            CHECK_IN_MAX_ATTEMPTS_PER_DAY
        )
    } else {
        "，今日已达重试上限，明天再试".to_string()
    };
    match service.check_in(provider.identity.id.clone()).await {
        Ok(result) if result.ok => {
            notify_provider_event(
                app,
                settings,
                provider,
                "BalanceHub 签到成功",
                non_empty(&result.message, "签到成功"),
            )
            .await;
            true
        }
        Ok(result) => {
            let display = format!("自动签到失败：{}", non_empty(&result.message, "签到失败"));
            let _ =
                service.mark_auto_check_in_failure(provider.identity.id.clone(), display.clone());
            if attempt == 1 {
                notify_provider_event(
                    app,
                    settings,
                    provider,
                    "BalanceHub 签到失败",
                    &format!("{display}{retry_hint}"),
                )
                .await;
            }
            true
        }
        Err(message) => {
            let display = format!("自动签到异常：{}", non_empty(&message, "签到异常"));
            let _ =
                service.mark_auto_check_in_failure(provider.identity.id.clone(), display.clone());
            if attempt == 1 {
                notify_provider_event(
                    app,
                    settings,
                    provider,
                    "BalanceHub 签到异常",
                    &format!("{display}{retry_hint}"),
                )
                .await;
            }
            true
        }
    }
}

/// 自动刷新的边沿触发通知：只对「本轮刷新中从非 Error 翻转为 Error」的中转站各发一条。
/// 持续失败的站点在下一次翻转（恢复后再失败）前不会重复通知。
async fn notify_refresh_failures(
    app: &AppHandle,
    settings: &AppSettings,
    before: &[Provider],
    refreshed_ids: &[String],
    after: &[Provider],
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
            let message = current
                .runtime
                .error_message
                .as_deref()
                .unwrap_or("刷新失败");
            notify_provider_event(app, settings, current, "BalanceHub 刷新失败", message).await;
        }
    }
}

/// 滚动并发跑到期测活（最多 3 个在飞，一个完成下一个立刻补位），返回是否有状态变更。
/// 测活从「上次成功」翻转为「本次失败」时发一条边沿触发通知。
async fn run_due_liveness(app: &AppHandle, settings: &AppSettings, due: Vec<Provider>) -> bool {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(LIVENESS_CONCURRENCY));
    let mut handles = Vec::with_capacity(due.len());
    for provider in &due {
        // 在 async 侧先拿许可再 spawn_blocking，许可随任务结束释放，形成滚动窗口。
        let Ok(permit) = Arc::clone(&semaphore).acquire_owned().await else {
            break;
        };
        let app = app.clone();
        let id = provider.identity.id.clone();
        // 测活是阻塞型（spawn 子进程并等待），必须放到 spawn_blocking，避免占用 async 线程。
        handles.push(tauri::async_runtime::spawn_blocking(move || {
            let _permit = permit;
            ProviderService::new(&app).run_liveness(id, None, true)
        }));
    }

    let mut changed = false;
    for (provider, handle) in due.iter().zip(handles) {
        if let Ok(Ok(result)) = handle.await {
            changed = true;
            let previously_ok = provider
                .liveness
                .records
                .last()
                .map(|record| record.ok)
                .unwrap_or(true);
            if previously_ok && !result.ok {
                notify_provider_event(
                    app,
                    settings,
                    provider,
                    "BalanceHub 测活失败",
                    non_empty(&result.message, "测活失败"),
                )
                .await;
            }
        }
    }
    changed
}

async fn notify_provider_event(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    title: &str,
    message: &str,
) {
    let markdown = format!(
        "**中转站**：{}\n\n**结果**：{message}",
        provider.identity.name
    );
    let _ =
        notifications::send_provider_notification(app, settings, provider, title, markdown, false)
            .await;
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
        prune_state(&mut state, &providers, "2026-07-10");

        assert!(state.refresh_attempts.contains_key("kept"));
        assert!(!state.refresh_attempts.contains_key("removed"));
        assert!(state.check_in_attempts.contains_key("kept"));
        assert!(!state.check_in_attempts.contains_key("stale-date"));
    }
}
