//! Refresh-triggered reauthentication for browser-bound relay accounts.
//!
//! A rejected session is not something the user can fix by pressing refresh
//! again. When the card already has a bound platform login, the stored browser
//! profile can usually renew it without a window. This path only starts after
//! the site rejected the credential; it never opens a visible window and never
//! touches accounts that cannot log in through a bound browser profile.
use super::*;
use crate::{
    models::{provider_domain, CheckInError, NotificationEvent},
    services::notifications,
};
use std::time::Duration;

/// One automatic attempt per provider per ten minutes. A successful re-login
/// immediately refreshes the card; the marker keeps that refresh from starting
/// a second attempt if the site still rejects the renewed session.
const RETRY_AFTER_SECS: u64 = 10 * 60;
/// Expired markers are cheap to drop; keep the map bounded across long uptimes.
const MARKER_TTL_SECS: u64 = 24 * 60 * 60;
static ATTEMPTS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

fn attempts() -> &'static Mutex<HashMap<String, u64>> {
    ATTEMPTS.get_or_init(Mutex::default)
}

fn recently_attempted(provider_id: &str) -> bool {
    let now = crate::util::unix_secs();
    let Ok(attempts) = attempts().lock() else {
        // 锁不可用时保守跳过；下一轮刷新再试。
        return true;
    };
    attempts
        .get(provider_id)
        .is_some_and(|at| now.saturating_sub(*at) < RETRY_AFTER_SECS)
}

/// 只有真正创建出登录任务后才记冷却。否则任务因账号被签到占用、已有
/// 登录窗口等原因没能创建时，刷新失败通知会被误吞，自愈也要白等十分钟。
fn mark_attempt(provider_id: &str) {
    let now = crate::util::unix_secs();
    let Ok(mut attempts) = attempts().lock() else {
        return;
    };
    attempts.insert(provider_id.to_string(), now);
    attempts.retain(|_, at| now.saturating_sub(*at) < MARKER_TTL_SECS);
}

/// 这一批刷新期间是否已经接手自动重新认证。接手后失败会由重新认证链路
/// 单独通知，刷新失败的边沿通知不必抢先报一次（成功时更不该报）。
/// 没有接手（未绑定账号、浏览器组件不可用、仍在冷却）时返回 `false`，
/// 刷新失败通知照常发出。
pub fn attempted_since(provider_id: &str, since_secs: u64) -> bool {
    attempts()
        .lock()
        .ok()
        .and_then(|attempts| attempts.get(provider_id).copied())
        .is_some_and(|at| at >= since_secs)
}

/// Called after a refresh classified the failure as a credential rejection.
/// This stays silent on purpose: a refresh must never fail because the optional
/// background repair could not start.
pub fn after_credential_failure(provider_id: &str) {
    let provider = crate::host::core_state().data.read().ok().and_then(|data| {
        data.providers
            .iter()
            .find(|provider| provider.identity.id == provider_id && provider.runtime.enabled)
            .cloned()
    });
    let Some(provider) = provider else {
        return;
    };
    if !matches!(
        provider_domain::check_in::fresh_login_route(&provider),
        Ok(provider_domain::check_in::FreshLoginRoute::BrowserAccount(
            _
        ))
    ) {
        return;
    }
    if !browser_runtime::status(false)
        .map(|status| status.ready)
        .unwrap_or(false)
    {
        return;
    }
    if recently_attempted(&provider.identity.id) {
        return;
    }
    match start(&provider.identity.id) {
        Ok(_) => mark_attempt(&provider.identity.id),
        Err(error) => log::debug!("自动重新认证未能开始（{provider_id}）：{error}"),
    }
}

fn start(provider_id: &str) -> Result<ProviderBrowserLoginTask, String> {
    let provider = crate::host::core_state()
        .data
        .read()
        .map_err(|_| "读取中转站失败")?
        .providers
        .iter()
        .find(|provider| provider.identity.id == provider_id && provider.runtime.enabled)
        .cloned()
        .ok_or("中转站已删除或已停用")?;
    let account_id = match provider_domain::check_in::fresh_login_route(&provider) {
        Ok(provider_domain::check_in::FreshLoginRoute::BrowserAccount(id)) => id.to_string(),
        _ => return Err("当前账号没有可自动续期的绑定登录账号".into()),
    };
    let account = ProviderService::new().login_account(&account_id)?;
    if browser_profiles::account_busy(&account.id) {
        return Err("登录账号正在处理其他任务".into());
    }
    let (cancel, cancelled) = watch::channel(false);
    let task = {
        let mut runs = runs().lock().map_err(|_| "创建重新认证任务失败")?;
        if runs.values().any(|run| {
            run.task.finished_at.is_none() && run.task.provider_id.as_deref() == Some(provider_id)
        }) {
            return Err("该站点已有登录任务".into());
        }
        if runs
            .values()
            .filter(|run| run.task.finished_at.is_none())
            .count()
            >= 10
        {
            return Err("待处理登录任务较多".into());
        }
        let task = ProviderBrowserLoginTask {
            run_id: format!(
                "provider-reauth-{}-{}",
                unix_millis(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            provider_id: Some(provider.identity.id.clone()),
            provider_name: provider.display_label(),
            login_account_id: account.id.clone(),
            operation: "reauth".into(),
            phase: "queued".into(),
            message: "已发现登录失效，正在准备自动重新认证".into(),
            started_at: unix_millis() as u64,
            finished_at: None,
            error: None,
            can_cancel: true,
            can_show_window: false,
        };
        runs.insert(
            task.run_id.clone(),
            LoginRun {
                task: task.clone(),
                cancel,
                window: None,
            },
        );
        task
    };
    emit(&task);
    let run_id = task.run_id.clone();
    let account_id = account.id;
    let guarded_id = run_id.clone();
    crate::runtime::spawn(guarded(guarded_id, async move {
        execute(run_id, provider, account_id, cancelled).await;
    }));
    Ok(task)
}

/// A panic in the background attempt (adapter parsing, notification formatting)
/// would unwind out of the spawned task while its registry entry stays
/// unfinished: the site would keep answering “该站点正在自动重新认证”, manual
/// login for it would be refused, and the run would hold a task slot until the
/// app restarts. The import and account-window paths already convert an unwind
/// into a terminal state; reauthentication must do the same.
async fn guarded<F>(run_id: String, attempt: F)
where
    F: std::future::Future<Output = ()>,
{
    use futures_util::FutureExt;
    if std::panic::AssertUnwindSafe(attempt)
        .catch_unwind()
        .await
        .is_err()
    {
        log::error!("自动重新认证异常退出（{run_id}）");
        publish(
            &run_id,
            "failed",
            "自动重新认证异常退出；登录结果可能已保存，请打开站点凭据确认",
        );
    }
}

async fn execute(
    run_id: String,
    provider: Provider,
    account_id: String,
    mut cancelled: watch::Receiver<bool>,
) {
    let pending = tokio::select! {
        biased;
        _ = cancelled.changed() => Err(LoginError::cancelled("自动重新认证已取消")),
        result = login(&run_id, &provider, &account_id) => result,
    };
    let result = match pending {
        Err(error) => Err(error),
        Ok(pending) => {
            // From here on the new session exists; saving must finish even if a
            // cancel arrived while the browser phase was completing. Publishing
            // "saving" first also makes cancel() refuse new requests.
            publish(&run_id, "saving", "认证已恢复，正在保存重新登录结果");
            if *cancelled.borrow() {
                Err(LoginError::cancelled("自动重新认证已取消"))
            } else {
                pending.save().await.map_err(reauth_failure)
            }
        }
    };
    let provider_id = provider.identity.id.clone();
    match result {
        Ok(provider) => {
            publish(&run_id, "saving", "认证已恢复，正在刷新站点数据");
            let service = ProviderService::new();
            let refresh_result = service
                .refresh_by_ids(vec![provider.identity.id.clone()])
                .await;
            // 无论刷新成败都要让前端重读；否则卡片仍显示认证前的状态。
            crate::app_events::emit_providers_changed(std::slice::from_ref(&provider_id));
            match refresh_result {
                Ok(_) => {
                    let still_rejected = crate::host::core_state()
                        .data
                        .read()
                        .ok()
                        .and_then(|data| {
                            data.providers
                                .iter()
                                .find(|item| item.identity.id == provider.identity.id)
                                .map(|item| item.runtime.credential_rejected)
                        })
                        .unwrap_or(false);
                    if still_rejected {
                        // 会话续期成功但站点仍拒绝新凭据：不能报告“已恢复”。
                        publish(
                            &run_id,
                            "failed",
                            "认证已完成，但站点仍拒绝新会话，请手动重新登录",
                        );
                    } else {
                        publish(&run_id, "completed", "认证已恢复，站点数据已更新");
                    }
                }
                Err(error) => publish(
                    &run_id,
                    "failed",
                    &format!("认证已恢复，但站点数据刷新失败：{error}"),
                ),
            }
        }
        Err(error) => {
            let cancelled = error.is_cancelled();
            let message = error.message();
            publish(
                &run_id,
                if cancelled { "cancelled" } else { "failed" },
                message,
            );
            log::debug!("自动重新认证未完成（{provider_id}）：{message}");
            if !cancelled {
                // 自动修复失败时应用可能正在托盘里运行，用户只会看到卡片上的
                // 「重新登录」，却不知道它什么时候出现。失败时主动提醒一次；
                // 用户自己取消的任务不再打扰。
                notify_failure(&provider, message).await;
            }
        }
    }
}

async fn notify_failure(provider: &Provider, message: &str) {
    let settings = {
        let state = crate::host::core_state();
        let Ok(data) = state.data.read() else {
            return;
        };
        data.settings.clone()
    };
    let markdown = format!(
        "**中转站**：{}\n\n**结果**：{message}\n\n可在卡片或站点凭据里点击「重新登录 / 授权」完成验证。",
        provider.display_label()
    );
    let _ = tokio::time::timeout(
        Duration::from_secs(20),
        notifications::send_provider_notification(
            &settings,
            provider,
            NotificationEvent::RefreshFailure,
            "BalanceHub 自动重新认证未完成",
            markdown,
            false,
        ),
    )
    .await;
}

async fn login(
    run_id: &str,
    provider: &Provider,
    account_id: &str,
) -> Result<super::check_in::PendingLogin, LoginError> {
    let settings = crate::host::core_state()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let progress_id = run_id.to_string();
    let progress = Arc::new(move |phase: CheckInPhase| {
        let (state, message) = match phase {
            CheckInPhase::Opening => ("opening", "正在无窗口打开站点登录页"),
            CheckInPhase::LoggingIn => ("running", "正在使用绑定账号重新登录"),
            _ => return,
        };
        publish(&progress_id, state, message);
    });
    // The overall browser request is bounded by its own 90s timeout; this
    // outer bound also covers profile and runtime acquisition.
    tokio::time::timeout(
        Duration::from_secs(120),
        super::check_in::login(
            &settings, provider, account_id, true, 90_000, progress, None,
        ),
    )
    .await
    .map_err(|_| LoginError::failed("自动重新认证超时，请手动重新登录"))?
    .map_err(reauth_failure)
}

fn reauth_message(error: CheckInError) -> String {
    match error {
        CheckInError::WaitingHuman | CheckInError::WaitingLogin => {
            "自动重新认证需要人工完成登录或站点验证，请在站点凭据中点击“重新登录 / 授权”"
                .to_string()
        }
        CheckInError::WaitingBrowser(message)
        | CheckInError::Cancelled(message)
        | CheckInError::Unconfirmed(message) => message,
        CheckInError::LoginTimeout(message) | CheckInError::Failed(message) => {
            format!("自动重新认证失败：{message}")
        }
    }
}

/// 角色由错误类型决定，取消不再靠提示文案判断；通知策略同样只看这个类型。
fn reauth_failure(error: CheckInError) -> LoginError {
    let cancelled = matches!(error, CheckInError::Cancelled(_));
    let message = reauth_message(error);
    if cancelled {
        LoginError::cancelled(message)
    } else {
        LoginError::failed(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_steps_and_failures_stay_actionable() {
        for error in [CheckInError::WaitingHuman, CheckInError::WaitingLogin] {
            assert!(reauth_message(error).contains("重新登录"));
        }
        assert!(
            reauth_message(CheckInError::Failed("浏览器连接失败".into()))
                .starts_with("自动重新认证失败")
        );
        assert_eq!(
            reauth_message(CheckInError::Cancelled("已取消".into())),
            "已取消"
        );
    }

    #[test]
    fn cancellation_roles_come_from_the_error_type_not_the_message() {
        assert!(reauth_failure(CheckInError::Cancelled("会话已失效".into())).is_cancelled());
        // 站点自己的文案里出现「已取消」不等于用户在 App 里取消了任务。
        let failed = reauth_failure(CheckInError::Failed("站点返回「已取消」，请重试".into()));
        assert!(!failed.is_cancelled());
        assert!(failed.message().contains("已取消"));
        let timeout = reauth_failure(CheckInError::LoginTimeout("重新登录等待超时".into()));
        assert!(!timeout.is_cancelled());
        assert!(timeout.message().starts_with("自动重新认证失败"));
    }

    #[test]
    fn attempts_are_rate_limited_per_provider() {
        let mut provider = Provider::from_input(
            crate::models::ProviderInput::default(),
            "reauth-fixture".into(),
        );
        assert!(!recently_attempted(&provider.identity.id));
        mark_attempt(&provider.identity.id);
        assert!(recently_attempted(&provider.identity.id));
        provider.identity.id = "reauth-fixture-other".into();
        assert!(!recently_attempted(&provider.identity.id));
    }

    #[tokio::test]
    async fn a_panicking_reauthentication_attempt_still_releases_the_site() {
        crate::host::testing::install();
        let run_id = "provider-reauth-test-panic";
        let (cancel, _receiver) = watch::channel(false);
        runs().lock().unwrap().insert(
            run_id.to_string(),
            LoginRun {
                task: ProviderBrowserLoginTask {
                    run_id: run_id.to_string(),
                    provider_id: Some("provider-panic".into()),
                    provider_name: "fixture".into(),
                    login_account_id: "account-panic".into(),
                    operation: "reauth".into(),
                    phase: "running".into(),
                    message: String::new(),
                    started_at: 0,
                    finished_at: None,
                    error: None,
                    can_cancel: true,
                    can_show_window: false,
                },
                cancel,
                window: None,
            },
        );

        guarded(run_id.to_string(), async { panic!("fixture panic") }).await;

        let task = runs()
            .lock()
            .unwrap()
            .get(run_id)
            .expect("任务应保留在注册表里")
            .task
            .clone();
        // 未结束的任务会让站点一直回复“正在自动重新认证”，并拦住新的登录；
        // panic 必须收敛成一个可回看的失败终态。
        assert_eq!(task.phase, "failed");
        assert!(task.finished_at.is_some());
        assert!(task.error.is_some());
        assert!(!task.can_cancel);
        assert!(!task.can_show_window);
        runs().lock().unwrap().remove(run_id);
    }

    #[test]
    fn refresh_failure_notifications_can_tell_whether_reauth_took_over() {
        let provider = Provider::from_input(
            crate::models::ProviderInput::default(),
            "reauth-since".into(),
        );
        let since = crate::util::unix_secs();
        assert!(!attempted_since(&provider.identity.id, since));
        mark_attempt(&provider.identity.id);
        assert!(attempted_since(&provider.identity.id, since));
        // 冷却期内再次失败不会重新接手，通知需要照常发出。
        assert!(recently_attempted(&provider.identity.id));
        assert!(!attempted_since(&provider.identity.id, since + 1));
    }
}
