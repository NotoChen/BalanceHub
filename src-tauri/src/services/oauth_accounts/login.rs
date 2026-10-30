use super::{login_callback as callback, login_device as device, random_id};
use crate::{
    adapters::oauth::{http, tokens},
    models::*,
    services::provider_service::ProviderService,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;
use tokio::{
    net::TcpListener,
    sync::{mpsc, watch},
};

const EVENT: &str = "oauth-login-task";
struct Session {
    task: OAuthLoginTask,
    state: String,
    redirect: Option<String>,
    cancel: watch::Sender<bool>,
    input: mpsc::Sender<String>,
    pending: Option<ProviderAuth>,
    target: Option<(String, u64)>,
}
fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(Mutex::default)
}

fn update(app: &AppHandle, id: &str, apply: impl FnOnce(&mut OAuthLoginTask)) {
    let task = {
        let mut sessions = sessions().lock().unwrap_or_else(|e| e.into_inner());
        let Some(session) = sessions.get_mut(id) else {
            return;
        };
        apply(&mut session.task);
        session.task.revision = session.task.revision.saturating_add(1);
        session.task.updated_at = chrono::Utc::now().timestamp();
        session.task.clone()
    };
    let _ = app.emit(EVENT, task);
}

pub(crate) fn list() -> Vec<OAuthLoginTask> {
    let mut result: Vec<_> = sessions()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .map(|s| s.task.clone())
        .collect();
    result.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    result
}

pub(crate) async fn start(
    app: AppHandle,
    vendor: OAuthVendor,
    provider_id: Option<String>,
) -> Result<OAuthLoginTask, String> {
    let snapshot = ProviderService::new(&app).load_app_data_async().await?;
    let target = provider_id
        .map(|id| {
            let provider = snapshot
                .providers
                .iter()
                .find(|p| p.identity.id == id)
                .ok_or("账号已移除")?;
            if provider
                .auth
                .oauth
                .as_ref()
                .is_none_or(|o| o.vendor != vendor)
            {
                return Err("原账号类型与授权服务不一致");
            }
            Ok((id, provider.auth.credential_revision))
        })
        .transpose()?;
    let id = random_id()?;
    let state = random_id()?;
    let verifier = random_id()?;
    let (cancel_tx, mut cancel) = watch::channel(false);
    let (input_tx, input) = mpsc::channel(1);
    let now = chrono::Utc::now().timestamp();
    let task = OAuthLoginTask {
        updated_at: now,
        revision: 0,
        id: id.clone(),
        vendor,
        phase: OAuthLoginPhase::Starting,
        authorization_url: None,
        user_code: None,
        accepts_callback: false,
        expires_at: now + 900,
        created_at: now,
        message: "正在准备授权".into(),
        provider_id: target.as_ref().map(|(id, _)| id.clone()),
        can_cancel: true,
        can_retry_save: false,
    };
    {
        let mut sessions = sessions().lock().map_err(|_| "授权任务状态不可用")?;
        if let Some(existing) = sessions.values().find(|s| {
            s.task.vendor == vendor
                && matches!(
                    s.task.phase,
                    OAuthLoginPhase::Starting
                        | OAuthLoginPhase::Waiting
                        | OAuthLoginPhase::Exchanging
                )
        }) {
            return Ok(existing.task.clone());
        }
        sessions.retain(|_, s| {
            s.pending.is_some()
                || now - s.task.created_at < 3600
                || s.task.can_cancel
                || s.task.phase == OAuthLoginPhase::Exchanging
        });
        if sessions.len() >= 32 {
            return Err("授权任务过多，请先处理未保存的授权后重启应用".into());
        }
        sessions.insert(
            id.clone(),
            Session {
                task: task.clone(),
                state: state.clone(),
                redirect: None,
                cancel: cancel_tx,
                input: input_tx,
                pending: None,
                target: target.clone(),
            },
        );
    }
    let _ = app.emit(EVENT, &task);
    tauri::async_runtime::spawn(async move {
        let mut provider = Provider::from_input(ProviderInput::default(), String::new());
        provider.identity.base_url = vendor.base_url().into();
        if let Some((id, _)) = &target {
            if let Some(existing) = snapshot.providers.iter().find(|p| &p.identity.id == id) {
                provider.proxy = existing.proxy.clone();
            }
        }
        let result = async {
            let client = tokio::select! {
                result = http::client(&snapshot.settings, &provider) => result?,
                _ = cancel.changed() => return Err("授权已取消".into()),
            };
            if vendor == OAuthVendor::Grok {
                grok_flow(&app, &id, &client, &mut cancel).await
            } else {
                browser_flow(
                    &app,
                    &id,
                    vendor,
                    &client,
                    (&state, &verifier),
                    input,
                    &mut cancel,
                )
                .await
            }
        }
        .await;
        match result {
            Ok(auth) => save_result(&app, &id, auth, target).await,
            Err(error) => update(&app, &id, |task| {
                task.phase = if *cancel.borrow() {
                    OAuthLoginPhase::Canceled
                } else {
                    OAuthLoginPhase::Failed
                };
                task.can_cancel = false;
                task.authorization_url = None;
                task.accepts_callback = false;
                task.user_code = None;
                task.message = error;
            }),
        }
    });
    Ok(task)
}

async fn open_browser(app: &AppHandle, id: &str, url: String) {
    let opener = app.clone();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        tauri::async_runtime::spawn_blocking(move || opener.opener().open_url(url, None::<&str>)),
    )
    .await;
    if !matches!(result, Ok(Ok(Ok(())))) {
        update(app, id, |task| {
            if task.phase == OAuthLoginPhase::Waiting {
                task.message = "浏览器未自动打开，请点击打开授权页面".into();
            }
        });
    }
}

fn exchange_boundary(app: &AppHandle, id: &str) -> Result<(), String> {
    let task = {
        let mut sessions = sessions().lock().map_err(|_| "授权任务状态不可用")?;
        let session = sessions.get_mut(id).ok_or("授权任务已过期")?;
        if *session.cancel.borrow() {
            return Err("授权已取消".into());
        }
        session.task.phase = OAuthLoginPhase::Exchanging;
        session.task.revision = session.task.revision.saturating_add(1);
        session.task.updated_at = chrono::Utc::now().timestamp();
        session.task.can_cancel = false;
        session.task.accepts_callback = false;
        session.task.message = "正在确认授权并保存账号".into();
        session.task.clone()
    };
    let _ = app.emit(EVENT, task);
    Ok(())
}

/// 在可用范围内挑一个本机回调端口。端口由上游登记决定，不能随便挑：
/// `auth.openai.com` 只接受 1455 / 1457，其它端口直接 `invalid_authorize_request`，
/// 所以 Codex 只在这两个端口上换地址族；Claude 接受任意 loopback 端口，
/// 首选端口被占用时（含两族地址都被占用）改由系统分配，仍然自动回调。
pub(super) async fn bind_loopback(
    path: &str,
    ports: callback::CallbackPorts,
) -> Option<(TcpListener, String)> {
    let candidates: Vec<u16> = match ports {
        callback::CallbackPorts::Registered(ports) => ports.to_vec(),
        // 0 交给系统分配：上游不校验具体端口。
        callback::CallbackPorts::Flexible(port) => vec![port, 0],
    };
    for port in candidates {
        for address in [
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
        ] {
            if let Ok(listener) = TcpListener::bind(address).await {
                // 用实际绑定的端口回填回调地址，系统分配（0）时也能拿到真实值。
                let Ok(local) = listener.local_addr() else {
                    continue;
                };
                return Some((listener, callback::loopback_redirect(path, local.port())));
            }
        }
    }
    None
}

async fn browser_flow(
    app: &AppHandle,
    id: &str,
    vendor: OAuthVendor,
    client: &reqwest::Client,
    proof: (&str, &str),
    mut input: mpsc::Receiver<String>,
    cancel: &mut watch::Receiver<bool>,
) -> Result<ProviderAuth, String> {
    let (state, verifier) = proof;
    if vendor == OAuthVendor::Grok {
        return Err("Grok 使用设备授权".into());
    }
    let (listener, redirect, message) = match callback::loopback_spec(vendor) {
        Some((path, ports)) => match bind_loopback(path, ports).await {
            Some((listener, redirect)) => (
                Some(listener),
                redirect,
                "请在浏览器完成授权，完成后会自动保存账号".to_string(),
            ),
            // 登记的端口全被占用且上游不接受任意端口时，退回让用户粘贴，
            // 而不是让整次授权直接失败。回调地址仍按首选端口发起，粘贴回来的
            // 地址要能和它逐字段对上。
            None => (
                None,
                callback::loopback_redirect(path, ports.preferred()),
                format!(
                    "本机授权回调端口 {} 均已被其他程序占用；请在浏览器完成授权后，把地址栏中的完整链接粘贴到下方",
                    ports.label()
                ),
            ),
        },
        None => (
            None,
            callback::manual_redirect(vendor),
            "在浏览器完成 Google 授权后，粘贴页面上的授权码".to_string(),
        ),
    };
    if let Ok(mut sessions) = sessions().lock() {
        if let Some(session) = sessions.get_mut(id) {
            session.redirect = Some(redirect.clone());
        }
    }
    let url = callback::authorization_url(vendor, state, verifier, &redirect)?;
    update(app, id, |task| {
        task.phase = OAuthLoginPhase::Waiting;
        task.authorization_url = Some(url.clone());
        task.accepts_callback = true;
        task.message = message;
    });
    let opener = app.clone();
    let task_id = id.to_owned();
    tauri::async_runtime::spawn(async move {
        open_browser(&opener, &task_id, url).await;
    });
    let expires = tokio::time::sleep(Duration::from_secs(900));
    tokio::pin!(expires);
    let code = loop {
        tokio::select! {
            biased;
            _ = cancel.changed() => return Err("授权已取消".into()),
            _ = &mut expires => return Err("授权已超时，请重新发起".into()),
            code = input.recv() => break code.ok_or("授权输入已关闭")?,
            result = async { match &listener { Some(listener) => listener.accept().await, None => std::future::pending().await } } => {
                let (socket, address) = result.map_err(|_| "读取授权回调失败")?;
                if !address.ip().is_loopback() { continue; }
                let code = tokio::select! {
                    _ = cancel.changed() => return Err("授权已取消".into()),
                    code = callback::receive(socket, vendor, state, &redirect) => code,
                };
                if let Some(code) = code { break code; }
            }
        }
    };
    exchange_boundary(app, id)?;
    tokens::exchange(client, vendor, &code, &redirect, verifier, state).await
}

async fn grok_flow(
    app: &AppHandle,
    id: &str,
    client: &reqwest::Client,
    cancel: &mut watch::Receiver<bool>,
) -> Result<ProviderAuth, String> {
    let mut device = tokio::select! { result = device::begin(client) => result?, _ = cancel.changed() => return Err("授权已取消".into()) };
    update(app, id, |task| {
        task.phase = OAuthLoginPhase::Waiting;
        task.authorization_url = Some(device.url.clone());
        task.user_code = Some(device.user_code.clone());
        task.expires_at = device.expires;
        task.message = "请在浏览器确认设备码并授权".into();
    });
    let opener = app.clone();
    let task_id = id.to_owned();
    let url = device.url.clone();
    tauri::async_runtime::spawn(async move {
        open_browser(&opener, &task_id, url).await;
    });
    loop {
        tokio::select! { _ = cancel.changed() => return Err("授权已取消".into()), _ = tokio::time::sleep(Duration::from_secs(device.interval)) => {} }
        if chrono::Utc::now().timestamp() >= device.expires {
            return Err("设备授权已过期，请重新发起".into());
        }
        // A poll may issue a single-use refresh token. Once sent, finish reading and
        // saving its result even if the modal is closed during the request.
        exchange_boundary(app, id)?;
        match device::poll(client, &device).await? {
            device::Poll::Ready(auth) => return Ok(*auth),
            device::Poll::SlowDown => device.interval = device.interval.saturating_add(5).min(300),
            device::Poll::Pending => {}
        }
        update(app, id, |task| {
            task.phase = OAuthLoginPhase::Waiting;
            task.can_cancel = true;
            task.message = "等待浏览器完成设备授权".into();
        });
    }
}

async fn save_result(app: &AppHandle, id: &str, auth: ProviderAuth, target: Option<(String, u64)>) {
    // Retain issued tokens until storage confirms success; retrying this step never
    // repeats the token exchange or consumes a refresh token again.
    if let Ok(mut sessions) = sessions().lock() {
        if let Some(session) = sessions.get_mut(id) {
            session.pending = Some(auth.clone());
        }
    }
    match ProviderService::new(app)
        .accept_oauth_login(auth, target)
        .await
    {
        Ok(provider) => {
            if let Ok(mut sessions) = sessions().lock() {
                if let Some(session) = sessions.get_mut(id) {
                    session.pending = None;
                }
            }
            update(app, id, |task| {
                task.phase = OAuthLoginPhase::Completed;
                task.provider_id = Some(provider.identity.id.clone());
                task.can_cancel = false;
                task.can_retry_save = false;
                task.accepts_callback = false;
                task.authorization_url = None;
                task.user_code = None;
                task.message = "账号已保存".into();
            });
        }
        Err(error) => update(app, id, |task| {
            task.phase = OAuthLoginPhase::Failed;
            task.can_cancel = false;
            task.can_retry_save = true;
            task.accepts_callback = false;
            task.authorization_url = None;
            task.user_code = None;
            task.message = format!("授权已取得，但保存失败：{error}。请重试保存，不需要再次登录。");
        }),
    }
}

pub(crate) fn cancel(id: &str) -> Result<(), String> {
    let sessions = sessions().lock().map_err(|_| "授权任务状态不可用")?;
    let session = sessions.get(id).ok_or("授权任务不存在")?;
    if !session.task.can_cancel {
        return Err("正在保存授权结果，完成后可以移除此账号".into());
    }
    session
        .cancel
        .send(true)
        .map_err(|_| "授权任务已结束".into())
}

pub(crate) fn submit(id: &str, value: &str) -> Result<(), String> {
    let sessions = sessions().lock().map_err(|_| "授权任务状态不可用")?;
    let session = sessions.get(id).ok_or("授权任务不存在")?;
    if !session.task.accepts_callback || *session.cancel.borrow() {
        return Err("当前任务不接受授权码".into());
    }
    let redirect = session
        .redirect
        .clone()
        .ok_or("授权任务尚未准备完成，请稍候")?;
    let code = callback::parse_code(value, session.task.vendor, &session.state, &redirect)?;
    session
        .input
        .try_send(code)
        .map_err(|_| "正在处理授权码，请等待结果".into())
}

pub(crate) fn retry_save(app: AppHandle, id: String) -> Result<(), String> {
    let (auth, target) = {
        let mut sessions = sessions().lock().map_err(|_| "授权任务状态不可用")?;
        let session = sessions.get_mut(&id).ok_or("授权任务不存在")?;
        if !session.task.can_retry_save {
            return Err("当前任务无需重试保存".into());
        }
        let auth = session
            .pending
            .clone()
            .ok_or("未保存的授权已失效，请重新授权")?;
        session.task.can_retry_save = false;
        session.task.phase = OAuthLoginPhase::Exchanging;
        session.task.revision = session.task.revision.saturating_add(1);
        session.task.updated_at = chrono::Utc::now().timestamp();
        (auth, session.target.clone())
    };
    tauri::async_runtime::spawn(async move {
        save_result(&app, &id, auth, target).await;
    });
    Ok(())
}

pub(crate) async fn reopen(app: AppHandle, id: String) -> Result<(), String> {
    let url = sessions()
        .lock()
        .map_err(|_| "授权任务状态不可用")?
        .get(&id)
        .filter(|s| s.task.phase == OAuthLoginPhase::Waiting)
        .and_then(|s| s.task.authorization_url.clone())
        .ok_or("授权页面已失效，请重新发起")?;
    open_browser(&app, &id, url).await;
    Ok(())
}
