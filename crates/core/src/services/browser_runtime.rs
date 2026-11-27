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
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::sync::{watch, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

const EVENT: &str = "balancehub://browser-runtime";
const DETECTION_CACHE: Duration = Duration::from_secs(300);
/// 组件安装或升级被强杀后会留下半成品目录，单个可达数百 MB。每天最多
/// 清理一次，且只在没有安装、卸载或会话占用组件时进行。
const ORPHAN_PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
static REVISION: AtomicU64 = AtomicU64::new(0);
static LAST_ORPHAN_PRUNE: AtomicU64 = AtomicU64::new(0);
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
    /// 安装、验证进度或终态每发布一次就递增。扫描在锁外进行，开始后
    /// 如果有发布，这份扫描结果就不再代表最新状态。
    published: u64,
    /// 已分配的扫描序号与最近一次提交的序号。两个扫描重叠时，后开始
    /// 的结果优先，先开始的慢扫描不能覆盖它。
    scan_issued: u64,
    scan_committed: u64,
}

impl RuntimeState {
    /// 提交一次锁外扫描的结果。扫描开始后如果有发布、安装/验证已经开始，
    /// 或者更晚开始的扫描已经提交，就丢弃这份结果并返回当前状态。
    fn commit(
        &mut self,
        published: u64,
        ticket: u64,
        snapshot: BrowserRuntimeStatus,
    ) -> Result<BrowserRuntimeStatus, String> {
        if self.cancel.is_some() || self.published != published || ticket <= self.scan_committed {
            return self
                .snapshot
                .clone()
                .ok_or_else(|| "组件状态未就绪".to_string());
        }
        self.snapshot = Some(snapshot.clone());
        self.checked_at = Some(Instant::now());
        self.scan_committed = ticket;
        Ok(snapshot)
    }

    fn publish(
        &mut self,
        phase: &str,
        message: &str,
        progress: Option<f64>,
    ) -> Option<BrowserRuntimeStatus> {
        let snapshot = self.snapshot.as_mut()?;
        snapshot.phase = phase.to_string();
        snapshot.message = message.to_string();
        snapshot.progress = progress;
        snapshot.ready = phase == "ready";
        let busy = matches!(phase, "installing" | "checking");
        snapshot.can_install = !busy && manifest::target().is_ok();
        snapshot.can_uninstall = snapshot.installed && !busy;
        snapshot.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
        let snapshot = snapshot.clone();
        self.published += 1;
        Some(snapshot)
    }
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
    let (published, ticket) = {
        let mut state = state().lock().map_err(|_| "读取组件状态失败")?;
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
        state.scan_issued += 1;
        (state.published, state.scan_issued)
    };
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
    // 安装可能已经在扫描期间开始，启动验证也可能刚刚完成；两种情况都由
    // 各自的提交方持有更新状态，这份扫描结果不能覆盖它们。
    let snapshot = state.commit(published, ticket, snapshot)?;
    drop(state);
    schedule_orphan_prune();
    Ok(snapshot)
}

/// 安装或升级被强制退出时，`.staging-*` 和未被 `active.json` 引用的
/// `runtime-*` 目录会一直留在组件目录里。状态扫描或后台每日维护时顺手
/// 清理，避免每次中断都累积一份完整的 Chromium 体积。
pub(crate) fn schedule_orphan_prune() {
    let now = crate::util::unix_millis() as u64;
    let elapsed = |last: u64| now.saturating_sub(last) >= ORPHAN_PRUNE_INTERVAL.as_millis() as u64;
    if !elapsed(LAST_ORPHAN_PRUNE.load(Ordering::Relaxed)) {
        return;
    }
    let Ok(root) = root_dir() else {
        return;
    };
    // 安装、卸载或任一浏览器会话仍在引用组件时不能动目录：安装自己
    // 持有写锁，会话持有读锁。拿不到锁时不算用掉今天的机会。
    let Ok(guard) = access().try_write_owned() else {
        return;
    };
    if state()
        .lock()
        .map(|state| state.cancel.is_some())
        .unwrap_or(true)
    {
        return;
    }
    let last = LAST_ORPHAN_PRUNE.load(Ordering::Relaxed);
    if !elapsed(last)
        || LAST_ORPHAN_PRUNE
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    let active = detection::installed().map(|(_, directory)| directory);
    // 目录必须在持锁期间枚举：解锁后新安装可能创建自己的 staging。
    let mut orphans = orphan_directories(&root, active.as_deref());
    // 启动验证的临时 profile 同样只在崩溃时残留；独占锁保证没有检查在跑。
    if let Ok(probes) = crate::host::cache_dir().map(|cache| cache.join("browser-probes")) {
        if let Ok(entries) = fs::read_dir(probes) {
            orphans.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    drop(guard);
    if orphans.is_empty() {
        return;
    }
    crate::runtime::spawn_blocking(move || {
        for path in orphans {
            // Windows 上仍被占用的文件会让删除失败，下一轮再试。
            let _ = fs::remove_dir_all(path);
        }
    });
}

fn orphan_directories(root: &Path, active: Option<&Path>) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        // 只处理真实目录：符号链接与散落的文件都不动。
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let orphan = name.starts_with(".staging-")
            || (name.starts_with("runtime-") && active.is_some_and(|active| active != path));
        if orphan {
            orphans.push(path);
        }
    }
    orphans
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

/// 安装前按下载包体积估算峰值占用：解压后的组件约为压缩包的数倍，再留出
/// 系统与临时文件的余量。查询不到可用空间时不拦截，交给写入失败时的提示兜底。
const INSTALL_SPACE_FACTOR: u64 = 3;
const INSTALL_SPACE_MARGIN: u64 = 192 * 1024 * 1024;

fn required_install_space(download_bytes: u64) -> u64 {
    download_bytes
        .saturating_mul(INSTALL_SPACE_FACTOR)
        .saturating_add(INSTALL_SPACE_MARGIN)
}

fn space_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{} MiB", bytes / (1024 * 1024))
    }
}

fn install_space_error(required: u64, available: Option<u64>) -> Option<String> {
    let available = available?;
    (available < required).then(|| {
        format!(
            "磁盘剩余空间不足：安装浏览器组件约需 {} 可用空间，当前仅剩 {}",
            space_label(required),
            space_label(available)
        )
    })
}

fn ensure_install_space(
    selection: &BrowserSelection,
    target: &manifest::Target,
) -> Result<(), String> {
    let mut download = target
        .node
        .size
        .saturating_add(manifest::manifest().playwright.size);
    if matches!(selection, BrowserSelection::Managed) {
        download = download.saturating_add(target.browser.size);
    }
    // 组件目录可能还没创建；数据目录与组件目录在同一个文件系统上。
    let available = crate::platform::disk::available_space(&crate::host::data_dir()?);
    match install_space_error(required_install_space(download), available) {
        Some(message) => Err(message),
        None => Ok(()),
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
    if install.is_some() {
        ensure_install_space(&selection, &target)?;
    }
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
        // 安装和验证在后台任务里运行；未预期的 panic 不能让状态永久停在
        // “进行中”，否则取消按钮和重新安装都会失效。
        use futures_util::FutureExt;
        let result = std::panic::AssertUnwindSafe(async {
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
        })
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            log::error!("浏览器组件操作异常退出");
            Err("浏览器组件操作异常退出，请重试".to_string())
        });
        complete_operation(guard, result, &cancelled);
    });
    Ok(initial)
}

/// Publish the terminal state and release the operation marker. Runs for every
/// outcome, including a caught panic, so a failed operation can always be
/// retried and cancelled.
fn complete_operation(
    guard: OwnedRwLockWriteGuard<()>,
    result: Result<(), String>,
    cancelled: &watch::Receiver<bool>,
) {
    // 即使 panic 发生在持锁期间让互斥量中毒，也要释放进行中标记。
    let mut state = state().lock().unwrap_or_else(|error| error.into_inner());
    state.cancel = None;
    state.checked_at = None;
    drop(state);
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
        state.publish(phase, message, progress)
    };
    if let Some(snapshot) = snapshot {
        crate::host::emit(EVENT, snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(phase: &str, revision: u64) -> BrowserRuntimeStatus {
        BrowserRuntimeStatus {
            phase: phase.to_string(),
            message: String::new(),
            ready: phase == "ready",
            installed: true,
            browser: None,
            system_browsers: Vec::new(),
            selection: BrowserSelection::Managed,
            runtime_ready: true,
            expected_version: "test".to_string(),
            installed_version: Some("test".to_string()),
            progress: None,
            revision,
            detected_at: 0,
            can_install: true,
            can_uninstall: true,
            runtime_download_bytes: 0,
            browser_download_bytes: 0,
        }
    }

    #[test]
    fn installs_are_stopped_before_download_only_when_space_is_clearly_short() {
        assert_eq!(required_install_space(0), INSTALL_SPACE_MARGIN);
        assert_eq!(
            install_space_error(600 * 1024 * 1024, None),
            None,
            "an unreadable free-space probe must never block an install"
        );
        assert_eq!(
            install_space_error(600 * 1024 * 1024, Some(600 * 1024 * 1024)),
            None
        );
        let message = install_space_error(600 * 1024 * 1024, Some(512 * 1024 * 1024)).unwrap();
        assert!(message.contains("600 MiB"));
        assert!(message.contains("512 MiB"));
        assert_eq!(space_label(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn a_failed_or_cancelled_operation_releases_the_install_state() {
        crate::host::testing::install();
        for (cancel_requested, expected) in [(false, "failed"), (true, "cancelled")] {
            let (sender, receiver) = watch::channel(cancel_requested);
            {
                let mut state = state().lock().unwrap();
                state.cancel = Some(sender);
                state.checked_at = Some(Instant::now());
                state.snapshot = Some(snapshot("installing", 1));
                state.published = 1;
                state.scan_issued = 1;
            }
            let guard = access().try_write_owned().unwrap();
            complete_operation(guard, Err("操作异常退出".to_string()), &receiver);
            let state = state().lock().unwrap();
            assert!(
                state.cancel.is_none(),
                "a new operation must be able to start"
            );
            // status(true) after the operation has rescanned, so only the
            // released marker and the terminal phase are asserted here.
            assert_eq!(state.snapshot.as_ref().unwrap().phase, expected);
        }
        *state().lock().unwrap() = RuntimeState::default();
    }

    #[test]
    fn a_scan_started_before_a_publish_loses_to_the_published_state() {
        let mut state = RuntimeState::default();
        state.commit(0, 1, snapshot("needsCheck", 1)).unwrap();
        let ready = state.publish("ready", "启动验证完成", Some(1.0)).unwrap();
        assert_eq!(ready.phase, "ready");
        // 扫描在 publish 之前开始，即使更晚提交也不能覆盖 ready。
        let returned = state.commit(0, 2, snapshot("needsCheck", 3)).unwrap();
        assert_eq!(returned.phase, "ready");
        assert_eq!(state.snapshot.as_ref().unwrap().phase, "ready");
    }

    #[test]
    fn the_later_scan_wins_even_when_an_earlier_one_commits_first() {
        let mut state = RuntimeState::default();
        // 两个扫描同时在跑：先开始的先提交了较旧的结果。
        state.commit(0, 1, snapshot("needsCheck", 1)).unwrap();
        // 后开始的扫描在验证完成后才开始，提交时应覆盖旧结果。
        let returned = state.commit(0, 2, snapshot("ready", 2)).unwrap();
        assert_eq!(returned.phase, "ready");
        assert_eq!(state.snapshot.as_ref().unwrap().phase, "ready");
    }

    #[test]
    fn an_earlier_scan_cannot_overwrite_a_later_committed_one() {
        let mut state = RuntimeState::default();
        state.commit(0, 2, snapshot("ready", 1)).unwrap();
        let returned = state.commit(0, 1, snapshot("needsCheck", 2)).unwrap();
        assert_eq!(returned.phase, "ready");
        assert_eq!(state.snapshot.as_ref().unwrap().phase, "ready");
    }

    #[test]
    fn a_scan_result_never_overwrites_an_operation_in_progress() {
        let mut state = RuntimeState::default();
        state.commit(0, 1, snapshot("needsCheck", 1)).unwrap();
        let (sender, _receiver) = watch::channel(false);
        state.cancel = Some(sender);
        let returned = state.commit(0, 2, snapshot("ready", 2)).unwrap();
        assert_eq!(returned.phase, "needsCheck");
        assert_eq!(state.snapshot.as_ref().unwrap().phase, "needsCheck");
    }

    #[test]
    fn interrupted_installs_leave_only_unreferenced_directories_for_pruning() {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("runtime-active");
        let stale = root.path().join("runtime-stale");
        let staging = root.path().join(".staging-1");
        for directory in [&active, &stale, &staging] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::write(root.path().join("active.json"), b"{}").unwrap();
        fs::create_dir_all(root.path().join("unrelated")).unwrap();
        fs::write(root.path().join(".staging-file"), b"keep").unwrap();

        let orphans = orphan_directories(root.path(), Some(&active));
        assert!(orphans.contains(&stale));
        assert!(orphans.contains(&staging));
        assert!(!orphans.contains(&active));
        assert!(!orphans.contains(&root.path().join("unrelated")));
        assert!(!orphans.contains(&root.path().join(".staging-file")));
    }

    #[test]
    fn without_an_active_directory_only_staging_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime-1");
        let staging = root.path().join(".staging-1");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&staging).unwrap();
        // active.json 不可读时不猜哪个 runtime 目录仍可能被引用。
        assert_eq!(orphan_directories(root.path(), None), vec![staging]);
    }
}
