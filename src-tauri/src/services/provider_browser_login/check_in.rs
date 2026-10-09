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
    state::AppState,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};

pub(crate) async fn run(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    account_id: &str,
    task: &CheckInContext,
) -> Result<Provider, CheckInError> {
    let service = ProviderService::new(app);
    let account = service.login_account(account_id)?;
    // Scheduled work leaves login to a user-triggered resume. Both a direct
    // check-in and a user-triggered batch keep the window until completion.
    if !task.interactive {
        return Err(CheckInError::WaitingLogin);
    }
    task.queued_for_browser(app);
    let _profile = browser_profiles::acquire(ProfileKey::Account(account_id.into())).await?;
    let account = service.check_login_account(&account)?;
    let runtime = browser_runtime::acquire(app)
        .await
        .map_err(CheckInError::WaitingBrowser)?;
    let context = ProviderRequestContext::capture(provider);
    let proxy = network::resolve_proxy(settings, provider);
    let ensure_current = || {
        let state = app.state::<AppState>();
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
    let directory = login_profiles::directory(app, account_id)?;
    let expected_user = if provider.auth.api_user.trim().is_empty() {
        provider.identity.user_id.trim()
    } else {
        provider.auth.api_user.trim()
    };
    task.phase(app, CheckInPhase::Opening);
    let submitted = Arc::new(AtomicBool::new(false));
    let progress_submitted = submitted.clone();
    let progress_app = app.clone();
    let progress_task = task.clone();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase.may_have_submitted() {
                progress_submitted.store(true, Ordering::Relaxed);
            }
            progress_task.phase(&progress_app, phase);
        }),
    )
    .map_err(CheckInError::WaitingBrowser)?;
    let window = task.attach_window(app, session.window_control())?;
    let outcome = session.request("login", json!({
        "url": provider.identity.base_url, "providerName": provider.display_label(),
        "profileDir": directory, "proxy": proxy.browser(&url)?, "executablePath": runtime.browser,
        "accountName": account.name, "expectedPlatform": account.platform, "expectedIdentity": account.identity,
        "expectedUserId": expected_user, "requireFreshLogin": true, "timeoutMs": 600_000,
    })).await;
    session.close().await;
    drop(window);
    let value = outcome.map_err(|error| login_error(error, submitted.load(Ordering::Relaxed)))?;
    let credentials =
        LoginCredentials::parse(value).map_err(|error| login_error(error.into(), true))?;
    ensure_current().map_err(|error| login_error(error.into(), true))?;
    task.phase(app, CheckInPhase::Saving);
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
}
