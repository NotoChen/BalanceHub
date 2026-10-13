//! WSL discovery is bounded, cached and never wakes a stopped distribution.
mod paths;
mod probe;
mod running;
pub(crate) use paths::{guest_path, host_path, recorded_path, split_host_path};
pub(crate) mod launch;
use crate::models::{WslAgent, WslDistribution, WslInventory};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use tauri::Emitter;

#[derive(Default)]
struct Cache {
    updated: Option<Instant>,
    value: Option<WslInventory>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static SCAN: Mutex<()> = Mutex::new(());
static BACKGROUND: AtomicBool = AtomicBool::new(false);
fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

pub(crate) fn cached_distributions() -> Vec<WslDistribution> {
    cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .value
        .as_ref()
        .map(|value| value.distributions.clone())
        .unwrap_or_default()
}

/// Native session lists can render immediately while the WSL probe completes.
pub(crate) fn refresh_background(app: &tauri::AppHandle) {
    if !cfg!(windows)
        || cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .updated
            .is_some_and(|updated| updated.elapsed() < Duration::from_secs(30))
        || BACKGROUND.swap(true, Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        refresh(&app, false);
        BACKGROUND.store(false, Ordering::Release);
    });
}

fn emit_changes(app: &tauri::AppHandle, previous: &[WslDistribution], value: &WslInventory) {
    if serde_json::to_vec(previous).ok() != serde_json::to_vec(&value.distributions).ok() {
        let _ = app.emit("agent-session-sources-changed", ());
    }
}

pub(crate) fn refresh(app: &tauri::AppHandle, force: bool) -> WslInventory {
    let previous = cached_distributions();
    let value = inventory(force);
    emit_changes(app, &previous, &value);
    value
}

fn inventory(force: bool) -> WslInventory {
    if !cfg!(windows) {
        return WslInventory {
            supported: false,
            distributions: Vec::new(),
            error: None,
        };
    }
    let previous = {
        let state = cache().lock().unwrap_or_else(|error| error.into_inner());
        if !force
            && state
                .updated
                .is_some_and(|updated| updated.elapsed() < Duration::from_secs(30))
        {
            if let Some(value) = &state.value {
                return value.clone();
            }
        }
        state.value.clone()
    };
    let Ok(_scan) = SCAN.try_lock() else {
        return previous.unwrap_or(WslInventory {
            supported: true,
            distributions: Vec::new(),
            error: Some("正在检测 WSL，请稍后刷新".into()),
        });
    };
    let result = (|| {
        let mut names = probe::names(false)?;
        // An earlier slow distribution cannot starve later ones on each retry.
        names.sort_by_key(|name| {
            previous
                .as_ref()
                .and_then(|value| value.distributions.iter().find(|item| &item.name == name))
                .is_some_and(|item| item.home.is_some() && item.error.is_none())
        });
        let running = running::names(true)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut distributions = Vec::new();
        for name in names {
            let old = previous
                .as_ref()
                .and_then(|value| value.distributions.iter().find(|item| item.name == name));
            let active = running.contains(&name);
            let mut item = probe::distribution(name, old, active);
            if active {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining < Duration::from_secs(1) {
                    item.error = Some("本次检测时间已用完，请再次刷新此发行版".into());
                } else {
                    match probe::inspect(&item.name, remaining.min(Duration::from_secs(10))) {
                        Ok(info) => {
                            item.home = Some(info.home);
                            item.agents = info.agents;
                            item.network_mode = info.network_mode;
                            item.host_gateway = info.host_gateway;
                        }
                        Err(error) => item.error = Some(error),
                    }
                }
            }
            distributions.push(item);
        }
        // Retry priority must not change scope identity or UI order.
        distributions.sort_by(|left, right| left.id.cmp(&right.id));
        Ok::<_, String>(WslInventory {
            supported: true,
            distributions,
            error: None,
        })
    })();
    let value = result.unwrap_or_else(|error| {
        let mut value = previous.unwrap_or(WslInventory {
            supported: true,
            distributions: Vec::new(),
            error: None,
        });
        value.error = Some(error);
        // Old facts remain visible but must never grant file access.
        for item in &mut value.distributions {
            item.running = false;
        }
        value
    });
    *cache().lock().unwrap_or_else(|error| error.into_inner()) = Cache {
        updated: Some(Instant::now()),
        value: Some(value.clone()),
    };
    value
}

pub(crate) fn running(id: &str) -> Result<WslDistribution, String> {
    let item = cached_distributions()
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| "WSL 发行版已不存在，请刷新环境列表".to_owned())?;
    if !item.running {
        return Err("此 WSL 发行版已停止，请先启动并检测".into());
    }
    if let Some(error) = &item.error {
        return Err(error.clone());
    }
    Ok(item)
}

/// Used at file/launch admission, independent of the display cache TTL.
pub(crate) fn require_running(id: &str) -> Result<WslDistribution, String> {
    let item = running(id)?;
    if !running::names(true)?.contains(&item.name) {
        return Err("WSL 发行版已经停止，请刷新后重试".into());
    }
    Ok(item)
}

/// UNC metadata/canonicalization can wake a distribution. Scope construction
/// keeps its remembered logical identity while that source is unavailable.
pub(crate) fn can_access_path(path: &std::path::Path) -> bool {
    let Some((name, _)) = split_host_path(path) else {
        return true;
    };
    cfg!(windows)
        && running(&name.to_lowercase()).is_ok()
        && running::names(false).is_ok_and(|names| {
            names
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name))
        })
}

pub(crate) fn start(app: &tauri::AppHandle, id: &str) -> Result<WslInventory, String> {
    let _scan = SCAN.lock().map_err(|_| "WSL 环境检测暂不可用")?;
    let snapshot = cached_distributions();
    let item = snapshot
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| "WSL 发行版不存在，请刷新列表".to_owned())?;
    if !probe::names(false)?.contains(&item.name) {
        return Err("WSL 发行版已移除".into());
    }
    // This is the only path allowed to start a stopped distribution.
    let info = probe::inspect(&item.name, Duration::from_secs(15))?;
    let mut distributions = snapshot.clone();
    if let Some(item) = distributions.iter_mut().find(|item| item.id == id) {
        item.running = true;
        item.home = Some(info.home);
        item.agents = info.agents;
        item.network_mode = info.network_mode;
        item.host_gateway = info.host_gateway;
        item.error = None;
    }
    let value = WslInventory {
        supported: true,
        distributions,
        error: None,
    };
    // Only the chosen distribution was probed. Do not refresh the timestamp of
    // other distributions or run all their Agent --version commands again.
    cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .value = Some(value.clone());
    emit_changes(app, &snapshot, &value);
    Ok(value)
}
