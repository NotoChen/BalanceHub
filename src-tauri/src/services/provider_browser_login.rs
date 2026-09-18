//! Serialized, cancellable browser jobs with an explicit durable account profile.
mod account;
use crate::{
    adapters::browser::{BrowserSession, BrowserWindowControl},
    app_events::{BackgroundTaskEvent, BACKGROUND_TASK_EVENT, PROVIDERS_CHANGED_EVENT},
    models::{
        provider_domain::auth::browser_login_supported, BrowserLoginMechanism, CheckInPhase,
        LoginAccount, LoginPlatform, Provider, ProviderInput,
    },
    network,
    services::{browser_runtime, provider_service::ProviderService},
    state::AppState,
    util::unix_millis,
};
pub(crate) use account::start_account;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{watch, Semaphore};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static SLOT: Semaphore = Semaphore::const_new(1);
static RUNS: OnceLock<Mutex<HashMap<String, LoginRun>>> = OnceLock::new();

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderBrowserLoginTask {
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
pub(crate) struct LoginCredentials {
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

struct LoginRun {
    task: ProviderBrowserLoginTask,
    cancel: watch::Sender<bool>,
    window: Option<BrowserWindowControl>,
}

fn runs() -> &'static Mutex<HashMap<String, LoginRun>> {
    RUNS.get_or_init(Mutex::default)
}

pub(crate) fn list() -> Result<Vec<ProviderBrowserLoginTask>, String> {
    let mut tasks = runs()
        .lock()
        .map_err(|_| "读取登录任务失败")?
        .values()
        .map(|run| run.task.clone())
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| task.started_at);
    Ok(tasks)
}

pub(crate) fn start(
    app: &AppHandle,
    mut input: ProviderInput,
    login_account_id: String,
) -> Result<ProviderBrowserLoginTask, String> {
    let account = ProviderService::new(app).login_account(&login_account_id)?;
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
    if !browser_runtime::status(app, true)?.ready {
        return Err("请先安装浏览器组件，再点击登录并导入".to_string());
    }
    let state = app.state::<AppState>();
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
            message: "等待登录窗口，前一个中转站完成后自动打开".into(),
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
    emit(app, &task);
    let app = app.clone();
    let run_id = task.run_id.clone();
    tauri::async_runtime::spawn(async move {
        execute(app, run_id, input, expected, account, cancelled).await;
    });
    Ok(task)
}

pub(crate) fn account_busy(id: &str) -> bool {
    runs()
        .lock()
        .map(|runs| {
            runs.values()
                .any(|run| run.task.login_account_id == id && run.task.finished_at.is_none())
        })
        .unwrap_or(true)
}

pub(crate) fn idle_slot() -> Result<tokio::sync::SemaphorePermit<'static>, String> {
    SLOT.try_acquire()
        .map_err(|_| "浏览器任务正在进行，请结束后再清除登录环境".into())
}

pub(crate) fn cancel(run_id: &str) -> Result<(), String> {
    let runs = runs().lock().map_err(|_| "取消登录失败")?;
    if let Some(run) = runs.get(run_id) {
        if run.task.phase == "saving" {
            return Err("正在保存登录结果，请稍候".to_string());
        }
        if run.task.can_cancel {
            let _ = run.cancel.send(true);
        }
    }
    Ok(())
}

pub(crate) async fn show_window(run_id: &str) -> Result<(), String> {
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

fn publish(app: &AppHandle, run_id: &str, phase: &str, message: &str) {
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
        run.task.can_cancel = matches!(phase, "queued" | "opening" | "waitingLogin");
        run.task.can_show_window = phase == "waitingLogin" && run.window.is_some();
        if matches!(phase, "completed" | "failed" | "cancelled") {
            run.task.finished_at = Some(unix_millis() as u64);
            run.window = None;
        }
        run.task.error = (phase == "failed").then(|| message.to_string());
        run.task.clone()
    };
    emit(app, &task);
}

fn begin_saving(app: &AppHandle, run_id: &str) -> bool {
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
    emit(app, &task);
    true
}

fn emit(app: &AppHandle, task: &ProviderBrowserLoginTask) {
    let status = match task.phase.as_str() {
        "queued" | "waitingLogin" => "waiting",
        "completed" => "success",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "running",
    };
    let _ = app.emit(
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
                if task.operation == "account" {
                    "管理登录账号"
                } else {
                    "登录并导入"
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
    app: AppHandle,
    run_id: String,
    input: ProviderInput,
    expected: Option<Provider>,
    account: LoginAccount,
    mut cancelled: watch::Receiver<bool>,
) {
    let result = run(&app, &run_id, input, expected, account, &mut cancelled).await;
    match result {
        Ok(provider) => {
            if let Ok(mut runs) = runs().lock() {
                if let Some(run) = runs.get_mut(&run_id) {
                    run.task.provider_id = Some(provider.identity.id.clone());
                }
            }
            publish(&app, &run_id, "completed", "账号已导入，登录窗口已关闭");
            let _ = app.emit(PROVIDERS_CHANGED_EVENT, ());
            let service = ProviderService::new(&app);
            let _ = service
                .refresh_by_ids(vec![provider.identity.id.clone()])
                .await;
            let _ = service.probe_capabilities(provider.identity.id).await;
            let _ = app.emit(PROVIDERS_CHANGED_EVENT, ());
        }
        Err(message) => {
            let phase =
                if *cancelled.borrow() || message.contains("已关闭") || message.contains("已取消")
                {
                    "cancelled"
                } else {
                    "failed"
                };
            publish(&app, &run_id, phase, &message);
        }
    }
}

async fn run(
    app: &AppHandle,
    run_id: &str,
    input: ProviderInput,
    expected: Option<Provider>,
    account: LoginAccount,
    cancelled: &mut watch::Receiver<bool>,
) -> Result<Provider, String> {
    let _slot = tokio::select! {
        biased;
        _ = cancelled.changed() => return Err("登录已取消".into()),
        slot = SLOT.acquire() => slot.map_err(|_| "登录队列不可用")?,
    };
    if *cancelled.borrow() {
        return Err("登录已取消".into());
    }
    let account = ProviderService::new(app).check_login_account(&account)?;
    let runtime = browser_runtime::acquire(app).await?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let provider = Provider::from_input(input.clone(), "browser-login".into());
    let url = reqwest::Url::parse(&input.identity.base_url).map_err(|_| "中转站地址无效")?;
    let proxy = network::resolve_proxy(&settings, &provider).browser(&url)?;
    let profile = crate::services::login_profiles::directory(app, &account.id)?;
    publish(app, run_id, "opening", "正在打开站点登录窗口");
    let progress_app = app.clone();
    let progress_id = run_id.to_string();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            if phase == CheckInPhase::WaitingHuman {
                publish(
                    &progress_app,
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
        _ = cancelled.changed() => Err("登录已取消".to_string()),
        result = session.request("login", json!({
            "url": input.identity.base_url, "providerName": provider.display_label(), "profileDir": profile,
            "proxy": proxy, "executablePath": runtime.browser, "loginPath": login_path, "timeoutMs": 600_000,
            "accountName": account.name, "expectedPlatform": account.platform, "expectedIdentity": account.identity,
        })) => result.map_err(|error| error.to_string()),
    };
    let result = match result {
        Ok(value) if begin_saving(app, run_id) => {
            match serde_json::from_value::<LoginCredentials>(value) {
                Ok(credentials)
                    if !credentials.user.id.is_empty()
                        && (if credentials.refresh_cookie.is_empty() {
                            !credentials.cookie_header.is_empty()
                        } else {
                            !credentials.access_token.is_empty()
                                && !credentials.session_id.is_empty()
                                && credentials
                                    .access_expires_at
                                    .is_some_and(|expiry| expiry > 0)
                        }) =>
                {
                    ProviderService::new(app)
                        .import_browser_login(input, expected, credentials, account)
                        .await
                }
                _ => Err("登录响应不完整，请重新登录导入".to_string()),
            }
        }
        Ok(_) => Err("登录已取消".to_string()),
        Err(message) => Err(message),
    };
    session.close().await;
    result
}
