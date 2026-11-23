//! Reauthentication remains part of the check-in task, never a detached import.
use super::LoginCredentials;
use crate::{
    adapters::browser::BrowserSession,
    models::{AppSettings, CheckInError, CheckInPhase, Provider},
    network,
    services::{
        browser_profiles::{self, ProfileKey},
        browser_runtime,
        check_in_tasks::CheckInContext,
        login_profiles,
        provider_service::{ProviderRequestContext, ProviderService},
    },
};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub async fn run(
    settings: &AppSettings,
    provider: &Provider,
    account_id: &str,
    task: &CheckInContext,
) -> Result<Provider, CheckInError> {
    // User-triggered work keeps the window until completion. Scheduled work
    // first tries a headless relogin: the bound profile and automatic
    // authorization can renew the session without interrupting the user.
    // A missing platform session or a CAPTCHA parks the task at WaitingLogin
    // for a one-click resume instead of holding a hidden login for minutes.
    let interactive = task.interactive;
    if interactive {
        task.queued_for_browser();
    }
    let progress_task = task.clone();
    let progress = Arc::new(move |phase: CheckInPhase| progress_task.phase(phase));
    let attempt = relogin(
        settings,
        provider,
        account_id,
        !interactive,
        if interactive { 600_000 } else { 90_000 },
        progress,
        interactive.then_some(task),
    );
    if interactive {
        attempt.await
    } else {
        // Scheduled work must not sit behind another task's browser profile
        // forever. The browser request itself is bounded to 90s; this outer
        // bound also covers the profile lease and runtime acquisition.
        tokio::time::timeout(Duration::from_secs(150), attempt)
            .await
            .unwrap_or(Err(CheckInError::WaitingLogin))
    }
}

/// The one place that turns a browser profile back into a saved relay session.
/// Check-in and refresh-triggered reauthentication only differ in whether they
/// keep a window and how they report progress; data and safety rules stay here.
pub(super) async fn relogin(
    settings: &AppSettings,
    provider: &Provider,
    account_id: &str,
    headless: bool,
    timeout_ms: u64,
    progress: Arc<dyn Fn(CheckInPhase) + Send + Sync>,
    attach: Option<&CheckInContext>,
) -> Result<Provider, CheckInError> {
    let service = ProviderService::new();
    let account = service.login_account(account_id)?;
    let _profile = browser_profiles::acquire(ProfileKey::Account(account_id.into())).await?;
    let account = service.check_login_account(&account)?;
    let runtime = browser_runtime::acquire()
        .await
        .map_err(CheckInError::WaitingBrowser)?;
    let context = ProviderRequestContext::capture(provider);
    let proxy = network::resolve_proxy(settings, provider);
    let ensure_current = || {
        let state = crate::host::core_state();
        let data = state.data.read().map_err(|_| "读取当前账号状态失败")?;
        let current = data
            .providers
            .iter()
            .find(|item| item.runtime.enabled && context.matches_check_in(item))
            .ok_or("账号配置已变更，已停止本次重新登录")?;
        if network::resolve_proxy(&data.settings, current).fingerprint() != proxy.fingerprint() {
            return Err("代理配置已变更，请重新签到".to_string());
        }
        Ok(())
    };
    ensure_current()?;
    let url = reqwest::Url::parse(&provider.identity.base_url).map_err(|_| "中转站地址无效")?;
    let directory = login_profiles::directory(account_id)?;
    let expected_user = if provider.auth.api_user.trim().is_empty() {
        provider.identity.user_id.trim()
    } else {
        provider.auth.api_user.trim()
    };
    progress(CheckInPhase::Opening);
    let submitted = Arc::new(AtomicBool::new(false));
    let progress_submitted = submitted.clone();
    let progress_session = progress.clone();
    let _progress_app = crate::host::current();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase.may_have_submitted() {
                progress_submitted.store(true, Ordering::Relaxed);
            }
            progress_session(phase);
        }),
    )
    .map_err(CheckInError::WaitingBrowser)?;
    // The guard detaches the window from the task when it drops, which happens
    // after the session closes even on early error paths.
    let _window = match attach {
        Some(task) => Some(task.attach_window(session.window_control())?),
        None => None,
    };
    let outcome = session.request("login", json!({
        "url": provider.identity.base_url, "providerName": provider.display_label(),
        "profileDir": directory, "proxy": proxy.browser(&url)?, "executablePath": runtime.browser,
        "accountName": account.name, "expectedPlatform": account.platform, "expectedIdentity": account.identity,
        "expectedUserId": expected_user, "requireFreshLogin": true,
        "headless": headless, "timeoutMs": timeout_ms,
    })).await;
    session.close().await;
    drop(_window);
    let value = outcome.map_err(|error| {
        let submitted = submitted.load(Ordering::Relaxed);
        automatic_login_error(login_error(error, submitted), submitted, attach.is_some())
    })?;
    let credentials =
        LoginCredentials::parse(value).map_err(|error| login_error(error.into(), true))?;
    ensure_current().map_err(|error| login_error(error.into(), true))?;
    progress(CheckInPhase::Saving);
    service
        .save_check_in_login(provider.clone(), credentials, account)
        .await
        .map_err(|error| {
            CheckInError::Unconfirmed(format!(
                "重新登录已返回，但结果未保存：{error}；请查看站点记录，已停止自动重试"
            ))
        })
}

fn login_error(error: CheckInError, submitted: bool) -> CheckInError {
    if submitted {
        CheckInError::Unconfirmed(format!(
            "登录请求可能已完成：{error}；请查看站点记录，已停止自动重试"
        ))
    } else {
        error
    }
}

/// A headless attempt that never reached the site's login endpoint did not
/// fail on the site's side: it needs a human to log in or clear verification.
/// Keep the task resumable instead of burning the automatic retry budget.
fn automatic_login_error(error: CheckInError, submitted: bool, interactive: bool) -> CheckInError {
    if interactive || submitted {
        return error;
    }
    match error {
        CheckInError::WaitingHuman | CheckInError::WaitingLogin => CheckInError::WaitingLogin,
        CheckInError::Failed(message) if message.contains("等待超时") => {
            CheckInError::WaitingLogin
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_login_never_retries_after_a_possible_submission() {
        assert!(matches!(
            login_error("登录等待超时".into(), true),
            CheckInError::Unconfirmed(_)
        ));
        assert!(matches!(
            login_error("登录窗口已关闭".into(), false),
            CheckInError::Failed(_)
        ));
        let cancelled = CheckInError::Cancelled("登录窗口已关闭".into());
        assert!(matches!(
            login_error(cancelled.clone(), false),
            CheckInError::Cancelled(_)
        ));
        assert!(matches!(
            login_error(cancelled, true),
            CheckInError::Unconfirmed(_)
        ));
    }

    #[test]
    fn headless_login_parks_for_a_human_only_before_any_submission() {
        assert!(matches!(
            automatic_login_error(CheckInError::WaitingHuman, false, false),
            CheckInError::WaitingLogin
        ));
        assert!(matches!(
            automatic_login_error("重新登录等待超时".into(), false, false),
            CheckInError::WaitingLogin
        ));
        assert!(matches!(
            automatic_login_error(CheckInError::WaitingHuman, true, false),
            CheckInError::WaitingHuman
        ));
        assert!(matches!(
            automatic_login_error(CheckInError::WaitingHuman, false, true),
            CheckInError::WaitingHuman
        ));
        assert!(matches!(
            automatic_login_error("浏览器连接失败".into(), false, false),
            CheckInError::Failed(_)
        ));
    }
}
