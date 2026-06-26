use std::collections::HashMap;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::{
    models::{provider_domain, AppSettings, Provider},
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
/// 前端监听此事件后重新拉取内存状态刷新视图。
pub const PROVIDERS_CHANGED_EVENT: &str = "providers-changed";

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
        // 记录每个中转站「上次发起刷新」的时刻（秒），跨 tick 复用，避免失败时每 tick 重试。
        let mut refresh_attempts: HashMap<String, u64> = HashMap::new();
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            run_tick(&app, &mut refresh_attempts).await;
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
        }
    });
}

async fn run_tick(app: &AppHandle, refresh_attempts: &mut HashMap<String, u64>) {
    let service = ProviderService::new(app);
    // 配置加载失败（storage 保护态）时暂停所有自动化，避免基于残缺状态误操作。
    let Ok(data) = service.load_app_data() else {
        return;
    };
    let settings = &data.settings;
    let now_secs = unix_secs();
    let now_millis = unix_millis();
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
                    refresh_attempts,
                )
            })
            .map(|provider| provider.identity.id.clone())
            .collect();
        if !due.is_empty() {
            for id in &due {
                refresh_attempts.insert(id.clone(), now_secs);
            }
            if service.refresh_by_ids(due).await.is_ok() {
                changed = true;
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
                    && !has_auto_check_in_error(provider)
                    && !provider_domain::capabilities::checked_in_today(provider, is_anyrouter)
                    && provider_domain::automation::check_in_due_now(provider, settings)
            })
            .cloned()
            .collect();
        for provider in due {
            if run_auto_check_in(app, &service, &provider, settings).await {
                changed = true;
            }
        }
    }

    // ---- 自动测活 ----
    let due_liveness: Vec<String> = data
        .providers
        .iter()
        .filter(|provider| {
            provider_domain::liveness::automatic_enabled(provider, settings)
                && provider_domain::liveness::is_due(provider, now_millis)
        })
        .map(|provider| provider.identity.id.clone())
        .collect();
    if run_due_liveness(app, due_liveness).await {
        changed = true;
    }

    if changed {
        tray::refresh_from_state(app);
        let _ = app.emit(PROVIDERS_CHANGED_EVENT, ());
    }
}

/// 自动签到，返回是否产生了需要刷新视图的状态变更。
async fn run_auto_check_in(
    app: &AppHandle,
    service: &ProviderService<'_>,
    provider: &Provider,
    settings: &AppSettings,
) -> bool {
    match service.check_in(provider.identity.id.clone()).await {
        Ok(result) if result.ok => {
            notify_check_in(
                app,
                settings,
                provider,
                "BalanceHub 签到成功",
                &provider.identity.name,
                non_empty(&result.message, "签到成功"),
            )
            .await;
            true
        }
        Ok(result) => {
            let message = format!("自动签到失败：{}", non_empty(&result.message, "签到失败"));
            let _ =
                service.mark_auto_check_in_failure(provider.identity.id.clone(), message.clone());
            notify_check_in(
                app,
                settings,
                provider,
                "BalanceHub 签到失败",
                &provider.identity.name,
                &message,
            )
            .await;
            true
        }
        Err(message) => {
            let message = format!("自动签到异常：{}", non_empty(&message, "签到异常"));
            let _ =
                service.mark_auto_check_in_failure(provider.identity.id.clone(), message.clone());
            notify_check_in(
                app,
                settings,
                provider,
                "BalanceHub 签到异常",
                &provider.identity.name,
                &message,
            )
            .await;
            true
        }
    }
}

/// 按并发上限批量跑到期测活，返回是否有成功（需要刷新视图）。
async fn run_due_liveness(app: &AppHandle, due: Vec<String>) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < due.len() {
        let end = (index + LIVENESS_CONCURRENCY).min(due.len());
        let mut handles = Vec::with_capacity(end - index);
        for id in &due[index..end] {
            let app = app.clone();
            let id = id.clone();
            // 测活是阻塞型（spawn 子进程并等待），必须放到 spawn_blocking，避免占用 async 线程。
            handles.push(tauri::async_runtime::spawn_blocking(move || {
                ProviderService::new(&app).test_liveness(id, None, true)
            }));
        }
        for handle in handles {
            if matches!(handle.await, Ok(Ok(_))) {
                changed = true;
            }
        }
        index = end;
    }
    changed
}

async fn notify_check_in(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    title: &str,
    provider_name: &str,
    message: &str,
) {
    let markdown = format!("**中转站**：{provider_name}\n\n**结果**：{message}");
    let _ =
        notifications::send_provider_notification(app, settings, provider, title, markdown, false)
            .await;
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn has_auto_check_in_error(provider: &Provider) -> bool {
    provider
        .runtime
        .error_message
        .as_deref()
        .is_some_and(is_auto_check_in_error)
}

fn is_auto_check_in_error(message: &str) -> bool {
    message.starts_with("自动签到失败：") || message.starts_with("自动签到异常：")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_auto_check_in_error_markers() {
        assert!(is_auto_check_in_error("自动签到失败：cookie 已过期"));
        assert!(is_auto_check_in_error("自动签到异常：网络错误"));
        assert!(!is_auto_check_in_error("刷新额度失败：网络错误"));
        assert!(!is_auto_check_in_error("签到失败：cookie 已过期"));
    }
}
