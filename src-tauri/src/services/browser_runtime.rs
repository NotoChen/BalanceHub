//! Optional browser support. No runtime is read from repository build caches.
mod detection;
mod install;
mod manifest;

use crate::state::AppState;
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{watch, OwnedRwLockReadGuard, RwLock};

const EVENT: &str = "balancehub://browser-runtime";
const DETECTION_CACHE: Duration = Duration::from_secs(300);
static REVISION: AtomicU64 = AtomicU64::new(0);
static STATE: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
static ACCESS: OnceLock<Arc<RwLock<()>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserInfo {
    pub name: String,
    pub path: PathBuf,
    pub managed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserRuntimeStatus {
    pub phase: String,
    pub message: String,
    pub ready: bool,
    pub installed: bool,
    pub browser: Option<BrowserInfo>,
    pub system_browser: Option<BrowserInfo>,
    pub expected_version: String,
    pub installed_version: Option<String>,
    pub progress: Option<f64>,
    pub revision: u64,
    pub detected_at: u64,
    pub can_install: bool,
    pub can_uninstall: bool,
    pub runtime_download_bytes: u64,
    pub browser_download_bytes: u64,
}

#[derive(Default)]
struct RuntimeState {
    snapshot: Option<BrowserRuntimeStatus>,
    checked_at: Option<Instant>,
    cancel: Option<watch::Sender<bool>>,
}

fn state() -> &'static Mutex<RuntimeState> {
    STATE.get_or_init(Mutex::default)
}
fn access() -> Arc<RwLock<()>> {
    ACCESS.get_or_init(|| Arc::new(RwLock::new(()))).clone()
}

fn root_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("components/checkin-browser"))
        .map_err(|_| "无法定位浏览器签到组件目录".to_string())
}

pub(crate) fn status(app: &AppHandle, force: bool) -> Result<BrowserRuntimeStatus, String> {
    {
        let state = state().lock().map_err(|_| "读取组件状态失败")?;
        if state.cancel.is_some()
            || !force
                && state
                    .checked_at
                    .is_some_and(|time| time.elapsed() < DETECTION_CACHE)
        {
            if let Some(snapshot) = &state.snapshot {
                return Ok(snapshot.clone());
            }
        }
    }
    let selected = manifest::target();
    let installed = detection::installed(app);
    let system_browser = detection::system_browser();
    let browser = installed
        .as_ref()
        .and_then(|(_, path)| detection::managed_browser(path))
        .or_else(|| system_browser.clone());
    let core_ready = installed
        .as_ref()
        .is_some_and(|(_, path)| detection::core_ready(path));
    let compatible = installed
        .as_ref()
        .is_some_and(|(installed, _)| installed.version == manifest::manifest().version);
    let (phase, message) = if selected.is_err() {
        ("unsupported", "当前系统架构暂不支持浏览器签到组件")
    } else if !core_ready {
        ("notInstalled", "浏览器签到组件尚未安装")
    } else if !compatible {
        ("needsUpdate", "组件版本需要更新，请确认后下载")
    } else if browser.is_none() {
        ("needsBrowser", "未找到可用浏览器，可安装独立 Chromium")
    } else {
        ("ready", "浏览器签到组件可用")
    };
    let snapshot = BrowserRuntimeStatus {
        phase: phase.to_string(),
        message: message.to_string(),
        ready: phase == "ready",
        installed: installed.is_some(),
        browser,
        system_browser,
        expected_version: manifest::manifest().version.clone(),
        installed_version: installed.map(|(installed, _)| installed.version),
        progress: None,
        revision: REVISION.fetch_add(1, Ordering::Relaxed) + 1,
        detected_at: crate::util::unix_millis() as u64,
        can_install: selected.is_ok(),
        can_uninstall: root_dir(app)?.exists(),
        runtime_download_bytes: selected
            .as_ref()
            .map(|target| target.node.size + manifest::manifest().playwright.size)
            .unwrap_or(0),
        browser_download_bytes: selected
            .as_ref()
            .map(|target| target.browser.size)
            .unwrap_or(0),
    };
    let mut state = state().lock().map_err(|_| "读取组件状态失败")?;
    // An installation may have started while the filesystem was being inspected.
    if state.cancel.is_some() {
        return state
            .snapshot
            .clone()
            .ok_or_else(|| "组件状态未就绪".to_string());
    }
    state.snapshot = Some(snapshot.clone());
    state.checked_at = Some(Instant::now());
    Ok(snapshot)
}

pub(crate) struct RuntimeSession {
    pub directory: PathBuf,
    pub browser: PathBuf,
    _guard: OwnedRwLockReadGuard<()>,
}

pub(crate) async fn acquire(app: &AppHandle) -> Result<RuntimeSession, String> {
    let guard = access()
        .try_read_owned()
        .map_err(|_| "组件正在安装或卸载，完成后可继续签到")?;
    let snapshot = status(app, true)?;
    if !snapshot.ready {
        return Err(snapshot.message);
    }
    let (_, directory) = detection::installed(app).ok_or("浏览器签到组件缺失")?;
    let browser = snapshot.browser.ok_or("未找到可用浏览器")?.path;
    // The small worker ships as source inside the App, and follows its IPC ABI.
    // Executables and Playwright remain exclusively in the optional directory.
    fs::write(directory.join("worker.mjs"), manifest::WORKER).map_err(|_| "无法准备签到执行器")?;
    Ok(RuntimeSession {
        directory,
        browser,
        _guard: guard,
    })
}

pub(crate) fn start_install(
    app: &AppHandle,
    include_browser: bool,
) -> Result<BrowserRuntimeStatus, String> {
    let guard = access()
        .try_write_owned()
        .map_err(|_| "浏览器签到任务正在使用组件，请结束后再安装")?;
    let snapshot = status(app, true)?;
    if !include_browser && snapshot.system_browser.is_none() {
        return Err("未找到本机浏览器，请选择同时安装独立浏览器".to_string());
    }
    let target = manifest::target()?.clone();
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .map_err(|_| "读取代理设置失败")?
        .settings
        .clone();
    let (sender, mut cancelled) = watch::channel(false);
    {
        let mut state = state().lock().map_err(|_| "读取组件状态失败")?;
        if state.cancel.is_some() {
            return Err("组件安装已在进行中".to_string());
        }
        state.cancel = Some(sender);
    }
    publish(app, "installing", "正在准备安装浏览器签到组件", Some(0.0));
    let app = app.clone();
    let initial = state()
        .lock()
        .map_err(|_| "读取组件状态失败")?
        .snapshot
        .clone()
        .ok_or("组件状态未就绪")?;
    tauri::async_runtime::spawn(async move {
        let result = install::run(&app, &settings, &target, include_browser, &mut cancelled).await;
        if let Ok(mut state) = state().lock() {
            state.cancel = None;
            state.checked_at = None;
        }
        drop(guard);
        let detected = status(&app, true);
        match result {
            Ok(()) => match detected {
                Ok(snapshot) if snapshot.ready => {
                    publish(&app, "ready", "安装完成，可以继续签到", Some(1.0))
                }
                Ok(snapshot) => publish(&app, &snapshot.phase, &snapshot.message, None),
                Err(message) => publish(&app, "failed", &message, None),
            },
            Err(_) if *cancelled.borrow() => {
                publish(&app, "cancelled", "安装已取消，可随时重试", None)
            }
            Err(message) => publish(&app, "failed", &message, None),
        }
    });
    Ok(initial)
}

pub(crate) fn cancel_install() -> Result<(), String> {
    let state = state().lock().map_err(|_| "读取组件状态失败")?;
    if let Some(cancel) = &state.cancel {
        let _ = cancel.send(true);
    }
    Ok(())
}

pub(crate) fn uninstall(app: &AppHandle) -> Result<BrowserRuntimeStatus, String> {
    let _guard = access()
        .try_write_owned()
        .map_err(|_| "组件正在使用中，请先取消签到或安装任务")?;
    let root = root_dir(app)?;
    if root.exists() {
        fs::remove_dir_all(root).map_err(|_| "无法卸载浏览器签到组件")?;
    }
    let profiles = app
        .path()
        .app_cache_dir()
        .map_err(|_| "无法定位浏览器会话")?
        .join("checkin-browser");
    if profiles.exists() {
        fs::remove_dir_all(profiles).map_err(|_| "组件已卸载，但验证会话清理失败")?;
    }
    let result = status(app, true)?;
    let _ = app.emit(EVENT, &result);
    Ok(result)
}

fn publish(app: &AppHandle, phase: &str, message: &str, progress: Option<f64>) {
    let snapshot = {
        let Ok(mut state) = state().lock() else {
            return;
        };
        let Some(snapshot) = state.snapshot.as_mut() else {
            return;
        };
        snapshot.phase = phase.to_string();
        snapshot.message = message.to_string();
        snapshot.progress = progress;
        snapshot.can_install = phase != "installing";
        snapshot.can_uninstall = snapshot.installed && phase != "installing";
        snapshot.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        snapshot.clone()
    };
    let _ = app.emit(EVENT, snapshot);
}
