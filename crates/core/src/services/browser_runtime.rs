//! Optional browser support. No runtime is read from repository build caches.
mod detection;
mod install;
mod manifest;
mod probe;
mod selection;
pub use selection::BrowserSelection;

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
use tokio::sync::{watch, OwnedRwLockReadGuard, RwLock};

const EVENT: &str = "balancehub://browser-runtime";
const DETECTION_CACHE: Duration = Duration::from_secs(300);
static REVISION: AtomicU64 = AtomicU64::new(0);
static STATE: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
static ACCESS: OnceLock<Arc<RwLock<()>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub name: String,
    pub path: PathBuf,
    pub managed: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRuntimeStatus {
    pub phase: String,
    pub message: String,
    pub ready: bool,
    pub installed: bool,
    pub browser: Option<BrowserInfo>,
    pub system_browsers: Vec<BrowserInfo>,
    pub selection: BrowserSelection,
    pub runtime_ready: bool,
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

fn root_dir() -> Result<PathBuf, String> {
    crate::host::data_dir()
        .map(|path| path.join("components/checkin-browser"))
        .map_err(|_| "无法定位浏览器组件目录".to_string())
}

pub fn status(force: bool) -> Result<BrowserRuntimeStatus, String> {
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
    let installed = detection::installed();
    let mut system_browsers = detection::system_browsers();
    let managed = installed
        .as_ref()
        .and_then(|(_, path)| detection::managed_browser(path));
    let selection = installed
        .as_ref()
        .and_then(|(installed, _)| installed.selected.clone())
        .unwrap_or_else(|| {
            if managed.is_some() {
                BrowserSelection::Managed
            } else {
                system_browsers
                    .first()
                    .map(|browser| BrowserSelection::System {
                        path: browser.path.clone(),
                    })
                    .unwrap_or(BrowserSelection::Managed)
            }
        });
    let mut browser = match &selection {
        BrowserSelection::Managed => managed.clone(),
        BrowserSelection::System { path } => detection::inspect(path).ok(),
    };
    if let Some(browser) = browser.as_ref().filter(|browser| !browser.managed) {
        if !system_browsers.iter().any(|item| item.path == browser.path) {
            system_browsers.push(browser.clone());
        }
    }
    let core_ready = installed
        .as_ref()
        .is_some_and(|(_, path)| detection::core_ready(path));
    let compatible = installed
        .as_ref()
        .is_some_and(|(installed, _)| installed.version == manifest::manifest().version);
    let runtime_ready = core_ready && compatible;
    let validated = installed.as_ref().and_then(|(installed, directory)| {
        installed.validation.as_ref().filter(|validation| {
            browser
                .as_ref()
                .is_some_and(|browser| validation.matches(directory, browser))
        })
    });
    if let (Some(browser), Some(validation)) = (&mut browser, validated) {
        browser.version = Some(validation.browser_version.clone());
    }
    let (phase, message) = if selected.is_err() {
        ("unsupported", "当前系统架构暂不支持浏览器组件")
    } else if !core_ready {
        ("notInstalled", "浏览器辅助组件尚未安装")
    } else if !compatible {
        ("needsUpdate", "组件版本需要更新，请确认后下载")
    } else if browser.is_none() {
        (
            "needsBrowser",
            "未找到所选浏览器，请选择本机浏览器或安装独立 Chromium",
        )
    } else if validated.is_none() {
        ("needsCheck", "已找到浏览器，完成启动验证后即可使用")
    } else {
        ("ready", "浏览器已通过启动验证")
    };
    let snapshot = BrowserRuntimeStatus {
        phase: phase.to_string(),
        message: message.to_string(),
        ready: phase == "ready",
        installed: installed.is_some(),
        browser,
        system_browsers,
        selection,
        runtime_ready,
        expected_version: manifest::manifest().version.clone(),
        installed_version: installed.map(|(installed, _)| installed.version),
        progress: None,
        revision: REVISION.fetch_add(1, Ordering::Relaxed) + 1,
        detected_at: crate::util::unix_millis() as u64,
        can_install: selected.is_ok(),
        can_uninstall: root_dir()?.exists(),
        runtime_download_bytes: selected
            .as_ref()
            .map(|target| {
                if runtime_ready {
                    0
                } else {
                    target.node.size + manifest::manifest().playwright.size
                }
            })
            .unwrap_or(0),
        browser_download_bytes: selected
            .as_ref()
            .map(|target| {
                if compatible && managed.is_some() {
                    0
                } else {
                    target.browser.size
                }
            })
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

pub struct RuntimeSession {
    pub directory: PathBuf,
    pub browser: PathBuf,
    _guard: OwnedRwLockReadGuard<()>,
}

pub async fn acquire() -> Result<RuntimeSession, String> {
    let guard = access()
        .try_read_owned()
        .map_err(|_| "组件正在安装或卸载，完成后可继续登录或验证")?;
    let snapshot = status(true)?;
    if !snapshot.runtime_ready || snapshot.browser.is_none() {
        return Err(snapshot.message);
    }
    let (_, directory) = detection::installed().ok_or("浏览器组件缺失")?;
    let browser = snapshot.browser.ok_or("未找到可用浏览器")?;
    // The small worker ships as source inside the App, and follows its IPC ABI.
    // Executables and Playwright remain exclusively in the optional directory.
    manifest::write_worker_files(&directory)?;
    if !snapshot.ready {
        static CHECK_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _check = CHECK_GATE.lock().await;
        if !status(true)?.ready {
            let (_sender, mut cancelled) = watch::channel(false);
            let version = probe::run(&directory, &browser, &mut cancelled).await?;
            let validation = selection::Validation::capture(&directory, &browser, version)
                .ok_or("浏览器文件已变更，请重新验证")?;
            selection::save(snapshot.selection, validation)?;
            let refreshed = status(true)?;
            crate::host::emit(EVENT, &refreshed);
        }
    }
    Ok(RuntimeSession {
        directory,
        browser: browser.path,
        _guard: guard,
    })
}

pub fn start_install(
    selection: BrowserSelection,
    repair: bool,
) -> Result<BrowserRuntimeStatus, String> {
    start_operation(selection, Some(repair))
}

pub fn select_browser(selection: BrowserSelection) -> Result<BrowserRuntimeStatus, String> {
    start_operation(selection, None)
}

pub fn inspect_browser(path: &std::path::Path) -> Result<BrowserInfo, String> {
    detection::inspect(path)
}

fn selected_browser(
    selection: &BrowserSelection,
    directory: &std::path::Path,
) -> Result<BrowserInfo, String> {
    match selection {
        BrowserSelection::Managed => {
            detection::managed_browser(directory).ok_or_else(|| "请先安装独立 Chromium".into())
        }
        BrowserSelection::System { path } => detection::inspect(path),
    }
}

fn start_operation(
    selection: BrowserSelection,
    install: Option<bool>,
) -> Result<BrowserRuntimeStatus, String> {
    let guard = access()
        .try_write_owned()
        .map_err(|_| "浏览器任务正在使用组件，请结束后再安装")?;
    let snapshot = status(true)?;
    if let BrowserSelection::System { path } = &selection {
        detection::inspect(path)?;
    }
    if install.is_none() {
        if !snapshot.runtime_ready {
            return Err("请先安装或更新浏览器辅助组件".into());
        }
        let (_, directory) = detection::installed().ok_or("浏览器辅助组件缺失")?;
        selected_browser(&selection, &directory)?;
    }
    let target = manifest::target()?.clone();
    let settings = crate::host::core_state()
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
    if install.is_some() {
        publish("installing", "正在准备浏览器辅助组件", Some(0.0));
    } else {
        publish("checking", "正在启动所选浏览器进行验证", None);
    }
    let initial = state()
        .lock()
        .map_err(|_| "读取组件状态失败")?
        .snapshot
        .clone()
        .ok_or("组件状态未就绪")?;
    crate::runtime::spawn(async move {
        let result = async {
            if let Some(repair) = install {
                install::run(&settings, &target, selection, repair, &mut cancelled).await
            } else {
                let (_, directory) = detection::installed().ok_or("浏览器辅助组件缺失")?;
                manifest::write_worker_files(&directory)?;
                let browser = selected_browser(&selection, &directory)?;
                let version = probe::run(&directory, &browser, &mut cancelled).await?;
                if *cancelled.borrow() {
                    return Err("浏览器检查已取消".to_string());
                }
                let validation = selection::Validation::capture(&directory, &browser, version)
                    .ok_or("浏览器文件已变更，请重新验证")?;
                selection::save(selection, validation)
            }
        }
        .await;
        if let Ok(mut state) = state().lock() {
            state.cancel = None;
            state.checked_at = None;
        }
        let detected = status(true);
        match result {
            Ok(()) => match detected {
                Ok(snapshot) if snapshot.ready => publish(
                    "ready",
                    "浏览器已通过启动验证，可以继续登录或验证",
                    Some(1.0),
                ),
                Ok(snapshot) => publish(&snapshot.phase, &snapshot.message, None),
                Err(message) => publish("failed", &message, None),
            },
            Err(_) if *cancelled.borrow() => {
                publish("cancelled", "操作已取消，原有浏览器选择和组件保留", None)
            }
            Err(message) => publish("failed", &message, None),
        }
        // Keep exclusive ownership until the terminal event is published, so a
        // newly started operation cannot be overwritten by this operation's result.
        drop(guard);
    });
    Ok(initial)
}

pub fn cancel_install() -> Result<(), String> {
    let state = state().lock().map_err(|_| "读取组件状态失败")?;
    if let Some(cancel) = &state.cancel {
        let _ = cancel.send(true);
    }
    Ok(())
}

pub fn uninstall() -> Result<BrowserRuntimeStatus, String> {
    let _guard = access()
        .try_write_owned()
        .map_err(|_| "组件正在使用中，请先取消登录、签到或安装任务")?;
    let root = root_dir()?;
    if root.exists() {
        fs::remove_dir_all(root).map_err(|_| "无法卸载浏览器组件")?;
    }
    let profiles = crate::host::cache_dir()
        .map_err(|_| "无法定位浏览器会话")?
        .join("checkin-browser");
    if profiles.exists() {
        fs::remove_dir_all(profiles).map_err(|_| "组件已卸载，但验证会话清理失败")?;
    }
    // Saved login accounts have an independent lifecycle in account management.
    let result = status(true)?;
    crate::host::emit(EVENT, &result);
    Ok(result)
}

fn publish(phase: &str, message: &str, progress: Option<f64>) {
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
        snapshot.ready = phase == "ready";
        let busy = matches!(phase, "installing" | "checking");
        snapshot.can_install = !busy && manifest::target().is_ok();
        snapshot.can_uninstall = snapshot.installed && !busy;
        snapshot.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        snapshot.clone()
    };
    crate::host::emit(EVENT, snapshot);
}
