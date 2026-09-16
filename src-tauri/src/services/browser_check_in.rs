use super::{
    browser_runtime, check_in_tasks::CheckInContext, provider_service::ProviderRequestContext,
};
use crate::{
    adapters::{
        browser::BrowserSession, new_api::check_in_with_browser,
        protocol::contracts::ProviderOperationOutcome,
    },
    models::{
        AppSettings, AuthMode, CheckInError, CheckInPhase, Provider, ProviderCheckInResult,
        ProviderCheckInVerification,
    },
    network,
    state::AppState,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

static BROWSER_SLOT: Semaphore = Semaphore::const_new(1);

pub(crate) async fn run(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    verification: ProviderCheckInVerification,
    task: &CheckInContext,
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    // Queue time is not counted as browser execution time. The enclosing task
    // owns cancellation, including while this permit is pending.
    task.phase(app, CheckInPhase::Queued);
    let _slot = BROWSER_SLOT
        .acquire()
        .await
        .map_err(|_| "浏览器队列不可用")?;
    let runtime = browser_runtime::acquire(app)
        .await
        .map_err(CheckInError::WaitingBrowser)?;
    task.phase(app, CheckInPhase::Opening);
    let progress_app = app.clone();
    let progress_task = task.clone();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            progress_task.phase(&progress_app, phase);
        }),
    )
    .map_err(CheckInError::WaitingBrowser)?;
    let outcome = tokio::time::timeout(
        Duration::from_secs(480),
        execute(
            app,
            settings,
            provider,
            verification,
            task,
            &runtime.browser,
            &mut session,
        ),
    )
    .await
    .unwrap_or_else(|_| {
        Err(CheckInError::Unconfirmed(
            "签到验证超时，站点结果尚未确认；已停止自动重试，请查看站点记录".to_string(),
        ))
    });
    session.close().await;
    // The task registry marks completion only after ProviderService persists.
    outcome
}

async fn execute(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    verification: ProviderCheckInVerification,
    task: &CheckInContext,
    executable: &std::path::Path,
    session: &mut BrowserSession,
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    let proxy = network::resolve_proxy(settings, provider);
    let proxy_key = proxy.fingerprint();
    let context = ProviderRequestContext::capture(provider);
    let ensure_current = || {
        let state = app.state::<AppState>();
        let data = state.data.read().map_err(|_| "读取当前账号状态失败")?;
        let current = data
            .providers
            .iter()
            .find(|item| context.matches(item))
            .filter(|item| item.runtime.enabled)
            .ok_or("账号配置已变更，已停止本次签到")?;
        if network::resolve_proxy(&data.settings, current).fingerprint() != proxy_key {
            return Err("代理配置已变更，请重新签到".to_string());
        }
        Ok(())
    };
    ensure_current()?;
    let base_url = provider.identity.base_url.trim_end_matches('/');
    let url = reqwest::Url::parse(&format!("{base_url}/api/status")).map_err(|_| "站点地址无效")?;
    let profile_key = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}|{}|{}|{:?}|{}",
                provider.identity.id,
                url.origin().ascii_serialization(),
                provider.auth.api_user,
                provider.auth.mode,
                proxy_key
            )
            .as_bytes()
        )
    );
    let profile_dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法获取浏览器会话目录")?
        .join("checkin-browser")
        .join(profile_key);
    let cookies = if matches!(provider.auth.mode, AuthMode::Session | AuthMode::Password) {
        provider
            .auth
            .session_cookie
            .split(';')
            .filter_map(|item| {
                let (name, value) = item.trim().split_once('=')?;
                Some(json!({ "name": name.trim(), "value": value.trim() }))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    session
        .request(
            "open",
            json!({
                "url": url.as_str(), "profileDir": profile_dir, "proxy": proxy.browser(&url)?,
                "cookies": cookies, "executablePath": executable, "interactive": task.interactive,
            }),
        )
        .await?;
    if crate::models::provider_domain::capabilities::uses_login_check_in(provider) {
        crate::adapters::new_api::login_check_in_with_browser(
            session,
            provider,
            verification,
            &ensure_current,
        )
        .await
    } else {
        check_in_with_browser(session, provider, verification, &ensure_current)
            .await
            .map(ProviderOperationOutcome::unchanged)
    }
}
