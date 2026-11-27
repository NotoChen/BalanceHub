//! Cancellable browser jobs with exclusive access to each durable account profile.
mod account;
mod check_in;
mod reauth;
use crate::{
    adapters::browser::{BrowserSession, BrowserWindowControl},
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT},
    models::{
        provider_domain::auth::browser_login_supported, BrowserLoginMechanism, CheckInError,
        CheckInPhase, LoginAccount, LoginPlatform, Provider, ProviderInput,
    },
    network,
    services::{
        browser_profiles::{self, ProfileKey},
        browser_runtime,
        provider_service::ProviderService,
    },
    util::unix_millis,
};
pub use account::start_account;
pub use check_in::run as check_in;
pub(crate) use reauth::{after_credential_failure, attempted_since};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tokio::sync::watch;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RUNS: OnceLock<Mutex<HashMap<String, LoginRun>>> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBrowserLoginTask {
    pub run_id: String,
    pub provider_id: Option<String>,
    pub provider_name: String,
    pub login_account_id: String,
    pub operation: String,
    pub phase: String,
    pub message: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub can_cancel: bool,
    pub can_show_window: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCredentials {
    pub cookie_header: String,
    pub access_token: String,
    pub access_expires_at: Option<i64>,
    pub refresh_cookie: String,
    pub session_id: String,
    pub user: LoginUser,
    #[serde(default)]
    pub mechanism: BrowserLoginMechanism,
    #[serde(default)]
    pub platform: LoginPlatform,
    #[serde(default)]
    pub platform_identity: Option<String>,
    #[serde(default)]
    pub fresh_login: bool,
}

impl LoginCredentials {
    fn parse(value: serde_json::Value) -> Result<Self, String> {
        let credentials: Self =
            serde_json::from_value(value).map_err(|_| "登录响应不完整，请重新登录")?;
        let has_session = if credentials.refresh_cookie.is_empty() {
            !credentials.cookie_header.is_empty()
        } else {
            !credentials.access_token.is_empty()
                && !credentials.session_id.is_empty()
                && credentials
                    .access_expires_at
                    .is_some_and(|expiry| expiry > 0)
        };
        if credentials.user.id.trim().is_empty() || !has_session {
            return Err("登录响应不完整，请重新登录".into());
        }
        Ok(credentials)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

struct LoginRun {
    task: ProviderBrowserLoginTask,
    cancel: watch::Sender<bool>,
    window: Option<BrowserWindowControl>,
}

/// 登录、账号窗口和自动重新认证共用的失败类型。是否属于取消由错误来源
/// 决定；界面状态和通知策略不再从提示文案里推断。
#[derive(Debug)]
enum LoginError {
    Cancelled(String),
    Failed(String),
}

impl LoginError {
    fn cancelled(message: impl Into<String>) -> Self {
        Self::Cancelled(message.into())
    }

    fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    fn message(&self) -> &str {
        match self {
            Self::Cancelled(message) | Self::Failed(message) => message,
        }
    }
}

impl From<String> for LoginError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for LoginError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_string())
    }
}

impl From<CheckInError> for LoginError {
    fn from(error: CheckInError) -> Self {
        match error {
            CheckInError::Cancelled(message) => Self::Cancelled(message),
            error => Self::Failed(error.to_string()),
        }
    }
}

fn runs() -> &'static Mutex<HashMap<String, LoginRun>> {
    RUNS.get_or_init(Mutex::default)
}

pub fn list() -> Result<Vec<ProviderBrowserLoginTask>, String> {
    let mut tasks = runs()
        .lock()
        .map_err(|_| "读取登录任务失败")?
        .values()
        .map(|run| run.task.clone())
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| task.started_at);
    Ok(tasks)
}

pub fn start(
    mut input: ProviderInput,
    login_account_id: String,
) -> Result<ProviderBrowserLoginTask, String> {
    let account = ProviderService::new().login_account(&login_account_id)?;
    if !browser_login_supported(input.identity.protocol) {
        return Err("当前中转站协议暂不支持登录导入".to_string());
    }
    let url = reqwest::Url::parse(input.identity.base_url.trim())
        .map_err(|_| "请先填写有效的中转站地址")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("请填写完整的中转站地址，不要附带账号或查询参数".to_string());
    }
    input.identity.base_url = url.as_str().trim_end_matches('/').to_string();
    if !browser_runtime::status(true)?.ready {
        return Err("请先安装浏览器组件，再点击登录并导入".to_string());
    }
    let state = crate::host::core_state();
    if state.load_error().is_some() {
        return Err("本地配置尚未恢复，暂不能导入账号".to_string());
    }
    let expected = {
        let data = state.data.read().map_err(|_| "读取中转站失败")?;
        input
            .id
            .as_ref()
            .map(|id| {
                data.providers
                    .iter()
                    .find(|provider| &provider.identity.id == id)
                    .cloned()
                    .ok_or("中转站已不存在，请重新打开编辑窗口".to_string())
            })
            .transpose()?
    };
    if expected.as_ref().is_some_and(|provider| {
        provider.identity.base_url.trim_end_matches('/') != input.identity.base_url
    }) {
        return Err("中转站地址已变更，请先保存地址或新建中转站再登录".to_string());
    }
    if expected
        .as_ref()
        .is_some_and(|provider| provider.auth.credential_revision != input.auth.credential_revision)
    {
        return Err("凭据已更新，请重新打开编辑窗口再登录".into());
    }
    let provider = Provider::from_input(input.clone(), "browser-login".into());
    let (cancel, cancelled) = watch::channel(false);
    let task = {
        let mut runs = runs().lock().map_err(|_| "创建登录任务失败")?;
        if let Some(existing) = runs.values().find(|run| {
            run.task.finished_at.is_none() && input.id.is_some() && run.task.provider_id == input.id
        }) {
            if existing.task.operation != "import" {
                return Err("该站点正在自动重新认证，请稍后再试".into());
            }
            if existing.task.login_account_id != account.id {
                return Err("该中转站正在使用另一个账号登录，请先取消已有任务".into());
            }
            return Ok(existing.task.clone());
        }
        if runs
            .values()
            .filter(|run| run.task.finished_at.is_none())
            .count()
            >= 10
        {
            return Err("待登录任务较多，请先完成或取消已有任务".to_string());
        }
        runs.retain(|_, run| {
            run.task
                .finished_at
                .is_none_or(|time| (unix_millis() as u64).saturating_sub(time) < 3_600_000)
        });
        let task = ProviderBrowserLoginTask {
            run_id: format!(
                "provider-login-{}-{}",
                unix_millis(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            provider_id: input.id.clone(),
            provider_name: provider.display_label(),
            login_account_id: account.id.clone(),
            operation: "import".into(),
            phase: "queued".into(),
            message: "正在等待可用登录窗口，同一账号的任务依次处理".into(),
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
    crate::runtime::spawn(async move {
        execute(run_id, input, expected, account, cancelled).await;
    });
    Ok(task)
}

pub fn account_busy(id: &str) -> bool {
    browser_profiles::account_busy(id)
        || runs()
            .lock()
            .map(|runs| {
                runs.values()
                    .any(|run| run.task.login_account_id == id && run.task.finished_at.is_none())
            })
            .unwrap_or(true)
}

pub fn cancel(run_id: &str) -> Result<(), String> {
    cancel_with_message(run_id, "登录已取消")
}

fn cancel_with_message(run_id: &str, message: &str) -> Result<(), String> {
    {
        let runs = runs().lock().map_err(|_| "取消登录失败")?;
        let Some(run) = runs.get(run_id) else {
            return Ok(());
        };
        if run.task.finished_at.is_some() {
            return Ok(());
        }
        if run.task.phase == "saving" {
            return Err("正在保存登录结果，请稍候".to_string());
        }
        if !run.task.can_cancel {
            return Ok(());
        }
        let _ = run.cancel.send(true);
    }
    // 先发布带原因的终态，再让后台任务收尾：取消信号到达后它自己也会
    // 尝试发布「登录已取消」，这里的具体原因不能被那条消息覆盖。
    publish(run_id, "cancelled", message);
    Ok(())
}

/// 站点被删除或停用后，指向它的登录窗口和自动重新认证不会再有可写入的
/// 卡片；登录账号被配置导入、云同步或手动编辑移除后，账号窗口也没有可
/// 保存的目标。与签到任务一样主动收口，避免已失效的窗口一直留在任务中心，
/// 或让用户在窗口里登录完才在保存时失败。
pub fn cancel_outdated() {
    let (providers, accounts) = match crate::host::core_state().data.read() {
        Ok(data) => (
            data.providers.clone(),
            data.login_accounts
                .iter()
                .map(|account| account.id.clone())
                .collect::<HashSet<String>>(),
        ),
        Err(_) => return,
    };
    let (removed_accounts, outdated_providers) = {
        let runs = runs().lock().unwrap_or_else(|err| err.into_inner());
        (
            runs_with_removed_accounts(&runs, &accounts),
            outdated_run_ids(&runs, &providers),
        )
    };
    for run_id in removed_accounts {
        let _ = cancel_with_message(&run_id, "登录账号已删除，登录已取消");
    }
    for run_id in outdated_providers {
        let _ = cancel_with_message(&run_id, "中转站已删除或停用，登录已取消");
    }
}

fn runs_with_removed_accounts(
    runs: &HashMap<String, LoginRun>,
    accounts: &HashSet<String>,
) -> Vec<String> {
    runs.values()
        .filter(|run| {
            run.task.finished_at.is_none() && !accounts.contains(&run.task.login_account_id)
        })
        .map(|run| run.task.run_id.clone())
        .collect()
}

fn outdated_run_ids(runs: &HashMap<String, LoginRun>, providers: &[Provider]) -> Vec<String> {
    runs.values()
        .filter(|run| {
            run.task.finished_at.is_none()
                && run.task.provider_id.is_some()
                && !providers.iter().any(|provider| {
                    Some(provider.identity.id.as_str()) == run.task.provider_id.as_deref()
                        && provider.runtime.enabled
                })
        })
        .map(|run| run.task.run_id.clone())
        .collect()
}

pub async fn show_window(run_id: &str) -> Result<(), String> {
    let control = {
        let runs = runs().lock().map_err(|_| "无法显示登录窗口")?;
        let run = runs.get(run_id).ok_or("登录任务已结束")?;
        if !run.task.can_show_window || *run.cancel.borrow() {
            return Err("登录窗口尚未打开或任务已结束，请查看任务进度".into());
        }
        run.window.clone().ok_or("登录窗口已关闭")?
    };
    control.show().await
}

fn attach_window(run_id: &str, control: BrowserWindowControl) -> Result<(), String> {
    let mut runs = runs().lock().map_err(|_| "无法连接登录窗口")?;
    let run = runs.get_mut(run_id).ok_or("登录任务已结束")?;
    run.window = Some(control);
    Ok(())
}

fn publish(run_id: &str, phase: &str, message: &str) {
    let task = {
        let Ok(mut runs) = runs().lock() else {
            return;
        };
        let Some(run) = runs.get_mut(run_id) else {
            return;
        };
        if run.task.finished_at.is_some() {
            return;
        }
        run.task.phase = phase.into();
        run.task.message = message.into();
        // Reauthentication has no visible window, so cancelling the background
        // attempt is its only control and stays available until saving starts.
        run.task.can_cancel = if run.task.operation == "reauth" {
            !matches!(phase, "saving" | "completed" | "failed" | "cancelled")
        } else {
            matches!(phase, "queued" | "opening" | "waitingLogin")
        };
        run.task.can_show_window = phase == "waitingLogin" && run.window.is_some();
        if matches!(phase, "completed" | "failed" | "cancelled") {
            run.task.finished_at = Some(unix_millis() as u64);
            run.window = None;
        }
        run.task.error = (phase == "failed").then(|| message.to_string());
        run.task.clone()
    };
    emit(&task);
}

fn begin_saving(run_id: &str) -> bool {
    let task = {
        let Ok(mut runs) = runs().lock() else {
            return false;
        };
        let Some(run) = runs.get_mut(run_id) else {
            return false;
        };
        if *run.cancel.borrow() {
            return false;
        }
        run.task.phase = "saving".into();
        run.task.message = "已确认账号，正在保存登录结果".into();
        run.task.can_cancel = false;
        run.task.can_show_window = false;
        run.window = None;
        run.task.clone()
    };
    emit(&task);
    true
}

fn emit(task: &ProviderBrowserLoginTask) {
    let status = match task.phase.as_str() {
        "queued" | "waitingLogin" => "waiting",
        "completed" => "success",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "running",
    };
    crate::host::emit(
        BACKGROUND_TASK_EVENT,
        BackgroundTaskEvent {
            can_cancel: Some(task.can_cancel),
            can_show_window: Some(task.can_show_window),
            login_account_id: Some(task.login_account_id.clone()),
            provider_id: task.provider_id.clone(),
            task_id: task.run_id.clone(),
            kind: "providerLogin".into(),
            status: status.into(),
            title: format!(
                "{} · {}",
                match task.operation.as_str() {
                    "account" => "管理登录账号",
                    "reauth" => "重新认证",
                    _ => "登录并导入",
                },
                task.provider_name
            ),
            detail: task.message.clone(),
            progress: None,
            started_at: task.started_at,
            finished_at: task.finished_at,
            error: task.error.clone(),
        },
    );
}

async fn execute(
    run_id: String,
    input: ProviderInput,
    expected: Option<Provider>,
    account: LoginAccount,
    mut cancelled: watch::Receiver<bool>,
) {
    // 登录链路里任何一次未预期的 panic 都不能让任务停在「进行中」：
    // 窗口随 future 一起被丢弃，这里把异常收敛为有提示的失败终态。
    use futures_util::FutureExt;
    let result =
        std::panic::AssertUnwindSafe(run(&run_id, input, expected, account, &mut cancelled))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                log::error!("登录任务异常退出（{run_id}）");
                Err(LoginError::failed("登录过程异常退出，请重新登录"))
            });
    match result {
        Ok(provider) => {
            if let Ok(mut runs) = runs().lock() {
                if let Some(run) = runs.get_mut(&run_id) {
                    run.task.provider_id = Some(provider.identity.id.clone());
                }
            }
            publish(&run_id, "completed", "账号已导入，登录窗口已关闭");
            let provider_id = provider.identity.id.clone();
            crate::app_events::emit_providers_changed(std::slice::from_ref(&provider_id));
            let service = ProviderService::new();
            let _ = service.refresh_by_ids(vec![provider_id.clone()]).await;
            let _ = service.probe_capabilities(provider_id.clone()).await;
            crate::app_events::emit_providers_changed(std::slice::from_ref(&provider_id));
        }
        Err(error) => {
            // 取消信号可能比错误先到（例如运行时获取失败在前、取消在后），
            // 保留这个兜底；除此之外只认类型，不看文案。
            let phase = if *cancelled.borrow() || error.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            };
            publish(&run_id, phase, error.message());
        }
    }
}

async fn run(
    run_id: &str,
    input: ProviderInput,
    expected: Option<Provider>,
    account: LoginAccount,
    cancelled: &mut watch::Receiver<bool>,
) -> Result<Provider, LoginError> {
    let _slot = tokio::select! {
        biased;
        _ = cancelled.changed() => return Err(LoginError::cancelled("登录已取消")),
        slot = browser_profiles::acquire(ProfileKey::Account(account.id.clone())) => slot?,
    };
    if *cancelled.borrow() {
        return Err(LoginError::cancelled("登录已取消"));
    }
    let account = ProviderService::new().check_login_account(&account)?;
    let runtime = browser_runtime::acquire().await?;
    let settings = crate::host::core_state()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let provider = Provider::from_input(input.clone(), "browser-login".into());
    let url = reqwest::Url::parse(&input.identity.base_url).map_err(|_| "中转站地址无效")?;
    let proxy = network::resolve_proxy(&settings, &provider).browser(&url)?;
    let profile = crate::services::login_profiles::directory(&account.id)?;
    publish(run_id, "opening", "正在打开站点登录窗口");
    let progress_id = run_id.to_string();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase == CheckInPhase::WaitingHuman {
                publish(
                    &progress_id,
                    "waitingLogin",
                    "请在小窗中登录，支持站点提供的第三方登录；完成后自动导入",
                );
            }
        }),
    )?;
    attach_window(run_id, session.window_control())?;
    let login_path = if crate::models::provider_domain::check_in::effective_method(&provider)
        == crate::models::ProviderCheckInMethod::SessionSignIn
    {
        "/sign-in"
    } else {
        "/login"
    };
    let result = tokio::select! {
        biased;
        _ = cancelled.changed() => Err(LoginError::cancelled("登录已取消")),
        result = session.request("login", json!({
            "url": input.identity.base_url, "providerName": provider.display_label(), "profileDir": profile,
            "proxy": proxy, "executablePath": runtime.browser, "loginPath": login_path, "timeoutMs": 600_000,
            "accountName": account.name, "expectedPlatform": account.platform, "expectedIdentity": account.identity,
        })) => result.map_err(LoginError::from),
    };
    let result = match result {
        Ok(value) if begin_saving(run_id) => match LoginCredentials::parse(value) {
            Ok(credentials) => ProviderService::new()
                .import_browser_login(input, expected, credentials, account)
                .await
                .map_err(LoginError::from),
            Err(message) => Err(LoginError::failed(message)),
        },
        Ok(_) => Err(LoginError::cancelled("登录已取消")),
        Err(error) => Err(error),
    };
    session.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login_run(
        run_id: &str,
        provider_id: Option<&str>,
        finished: bool,
        can_cancel: bool,
    ) -> LoginRun {
        LoginRun {
            task: ProviderBrowserLoginTask {
                run_id: run_id.to_string(),
                provider_id: provider_id.map(str::to_string),
                provider_name: "fixture".to_string(),
                login_account_id: "account-fixture".to_string(),
                operation: "import".to_string(),
                phase: if finished {
                    "completed"
                } else {
                    "waitingLogin"
                }
                .to_string(),
                message: String::new(),
                started_at: 0,
                finished_at: finished.then_some(1),
                error: None,
                can_cancel,
                can_show_window: false,
            },
            cancel: watch::channel(false).0,
            window: None,
        }
    }

    #[test]
    fn outdated_runs_follow_removed_or_disabled_providers() {
        let live = Provider::from_input(
            crate::models::ProviderInput::default(),
            "provider-live".to_string(),
        );
        let renamed = {
            let mut provider = live.clone();
            provider.identity.id = "provider-renamed".to_string();
            provider
        };
        let mut disabled = live.clone();
        disabled.identity.id = "provider-disabled".to_string();
        disabled.runtime.enabled = false;
        let runs = HashMap::from([
            (
                "live".to_string(),
                login_run("live", Some("provider-live"), false, true),
            ),
            (
                "disabled".to_string(),
                login_run("disabled", Some("provider-disabled"), false, true),
            ),
            (
                "removed".to_string(),
                login_run("removed", Some("provider-removed"), false, true),
            ),
            // 已经结束的任务不再收口；账号窗口没有目标站点，也不受影响。
            (
                "finished".to_string(),
                login_run("finished", Some("provider-removed"), true, false),
            ),
            (
                "account".to_string(),
                login_run("account", None, false, true),
            ),
        ]);
        let mut outdated = outdated_run_ids(&runs, &[live, renamed, disabled]);
        outdated.sort();
        assert_eq!(outdated, ["disabled", "removed"]);
    }

    #[test]
    fn login_windows_close_when_their_account_is_removed_from_the_configuration() {
        let runs = HashMap::from([
            ("live".to_string(), login_run("live", None, false, true)),
            (
                "finished".to_string(),
                login_run("finished", None, true, false),
            ),
        ]);
        assert!(
            runs_with_removed_accounts(&runs, &HashSet::from(["account-fixture".to_string()]))
                .is_empty()
        );
        // 账号被配置导入、云同步或手动编辑移除后，窗口里再完成登录也没有
        // 可保存的目标；只取消仍在进行的窗口。
        assert_eq!(runs_with_removed_accounts(&runs, &HashSet::new()), ["live"]);
    }

    #[test]
    fn outdated_cancellation_keeps_its_reason_when_the_run_finishes_later() {
        crate::host::testing::install();
        let run_id = "provider-login-test-outdated-reason";
        runs().lock().unwrap().insert(
            run_id.to_string(),
            login_run(run_id, Some("provider-x"), false, true),
        );

        cancel_with_message(run_id, "中转站已删除或停用，登录已取消").unwrap();
        // 取消信号到达后后台任务也会发布自己的「登录已取消」；带原因的终态
        // 已经落库，后来的发布不能把它改回去。
        publish(run_id, "cancelled", "登录已取消");

        let task = runs()
            .lock()
            .unwrap()
            .get(run_id)
            .expect("任务应保留在注册表里")
            .task
            .clone();
        assert_eq!(task.phase, "cancelled");
        assert_eq!(task.message, "中转站已删除或停用，登录已取消");
        assert!(task.finished_at.is_some());
        runs().lock().unwrap().remove(run_id);
    }
}
