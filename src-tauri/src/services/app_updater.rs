use crate::{limits, network, state::AppState};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant as StdInstant},
};
use tauri::{ipc::Channel, AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::{sync::watch, time::Instant as TokioInstant};

const VISIBLE_RELAUNCH_ENV: &str = "BALANCEHUB_VISIBLE_RELAUNCH";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(200);
const PROGRESS_REPORT_BYTES: u64 = 256 * 1024;

const PHASE_DOWNLOADING: u8 = 0;
const PHASE_VERIFYING: u8 = 1;
const PHASE_INSTALLING: u8 = 2;

static INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub struct AppUpdaterState {
    pending: Mutex<Option<Update>>,
    active: Mutex<Option<ActiveInstall>>,
    check_gate: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct ActiveInstall {
    id: u64,
    cancel: watch::Sender<bool>,
    phase: Arc<AtomicU8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
    pub raw_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum AppUpdateDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Verifying,
    Installing,
    Finished,
}

pub async fn check(app: &AppHandle) -> Result<Option<AppUpdateInfo>, String> {
    let updater_state = app.state::<AppUpdaterState>();
    if updater_state.has_active_install() {
        return Err("更新正在下载或安装，请稍候".to_string());
    }
    let _check_guard = updater_state
        .check_gate
        .try_lock()
        .map_err(|_| "正在检查更新，请稍候".to_string())?;

    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .settings
        .clone();
    let proxy = network::resolve_global_proxy(&settings);
    let builder = network::configure_updater_builder(app.updater_builder(), &proxy)?;
    let updater = builder
        .build()
        .map_err(|err| format!("初始化更新客户端失败: {err}"))?;
    let update = tokio::time::timeout(UPDATE_CHECK_TIMEOUT, updater.check())
        .await
        .map_err(|_| "检查更新超时，请稍后重试".to_string())?
        .map_err(|err| format!("检查更新失败: {err}"))?;

    let Some(update) = update else {
        clear_pending(app)?;
        return Ok(None);
    };
    let info = AppUpdateInfo {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: update.date.map(|value| value.to_string()),
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
    };
    *updater_state
        .pending
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = Some(update);
    Ok(Some(info))
}

pub async fn install(
    app: &AppHandle,
    on_event: Channel<AppUpdateDownloadEvent>,
) -> Result<(), String> {
    let updater_state = app.state::<AppUpdaterState>();
    let update = updater_state
        .pending
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
        .ok_or_else(|| "没有待安装的更新，请重新检查".to_string())?;
    let (install_id, cancel, phase) = updater_state.begin_install()?;

    let result = install_inner(update, on_event, cancel, phase).await;
    updater_state.finish_install(install_id);
    if result.is_ok() {
        clear_pending(app)?;
    }
    result
}

async fn install_inner(
    update: Update,
    on_event: Channel<AppUpdateDownloadEvent>,
    mut cancel: watch::Receiver<bool>,
    phase: Arc<AtomicU8>,
) -> Result<(), String> {
    let installer = update.clone();
    let _ = on_event.send(AppUpdateDownloadEvent::Started {
        content_length: None,
    });

    let (heartbeat_sender, mut heartbeat) = watch::channel(0_u64);
    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let progress_channel = on_event.clone();
    let verify_channel = on_event.clone();
    let verify_phase = Arc::clone(&phase);
    let callback_bytes = Arc::clone(&downloaded_bytes);
    let mut first_chunk = true;
    let mut reported_bytes = 0_u64;
    let mut last_report = StdInstant::now();

    let download = update.download(
        move |chunk_length, content_length| {
            let downloaded = callback_bytes
                .fetch_add(chunk_length as u64, Ordering::Relaxed)
                .saturating_add(chunk_length as u64);
            heartbeat_sender.send_replace(downloaded);

            if first_chunk {
                first_chunk = false;
                let _ = progress_channel.send(AppUpdateDownloadEvent::Started { content_length });
            }
            if downloaded.saturating_sub(reported_bytes) >= PROGRESS_REPORT_BYTES
                || last_report.elapsed() >= PROGRESS_REPORT_INTERVAL
            {
                let delta = downloaded.saturating_sub(reported_bytes);
                reported_bytes = downloaded;
                last_report = StdInstant::now();
                let _ = progress_channel.send(AppUpdateDownloadEvent::Progress {
                    chunk_length: usize::try_from(delta).unwrap_or(usize::MAX),
                });
            }
        },
        move || {
            verify_phase.store(PHASE_VERIFYING, Ordering::Release);
            let _ = verify_channel.send(AppUpdateDownloadEvent::Verifying);
        },
    );
    tokio::pin!(download);

    let idle_timeout = tokio::time::sleep(DOWNLOAD_IDLE_TIMEOUT);
    let total_timeout = tokio::time::sleep(DOWNLOAD_TOTAL_TIMEOUT);
    tokio::pin!(idle_timeout);
    tokio::pin!(total_timeout);

    let bytes = loop {
        tokio::select! {
            result = &mut download => {
                break result.map_err(|err| format!("更新下载或签名校验失败: {err}"))?;
            }
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    return Err("更新下载已取消".to_string());
                }
            }
            changed = heartbeat.changed() => {
                if changed.is_err() {
                    return Err("更新下载状态通道异常".to_string());
                }
                let downloaded = *heartbeat.borrow_and_update();
                if downloaded > limits::MAX_UPDATE_PACKAGE_BYTES {
                    return Err(update_size_error());
                }
                idle_timeout.as_mut().reset(TokioInstant::now() + DOWNLOAD_IDLE_TIMEOUT);
            }
            () = &mut idle_timeout => {
                return Err("更新下载超过 45 秒没有进度，已停止下载".to_string());
            }
            () = &mut total_timeout => {
                return Err("更新下载超过 20 分钟，已停止下载".to_string());
            }
        }
    };

    if bytes.len() as u64 > limits::MAX_UPDATE_PACKAGE_BYTES
        || downloaded_bytes.load(Ordering::Acquire) > limits::MAX_UPDATE_PACKAGE_BYTES
    {
        return Err(update_size_error());
    }

    phase.store(PHASE_INSTALLING, Ordering::Release);
    let _ = on_event.send(AppUpdateDownloadEvent::Installing);
    prepare_visible_relaunch();

    // 系统安装器不具备可靠的中途回滚语义，因此这里只把阻塞安装移到 blocking
    // 线程，不在超时后强杀。下载和校验均已完成后，宁可等待系统安装器给出结果，
    // 也不能制造“前端已重试、旧安装器仍在后台写文件”的竞态。
    let install_result =
        match tauri::async_runtime::spawn_blocking(move || installer.install(bytes)).await {
            Ok(result) => result.map_err(|err| format!("更新安装失败: {err}")),
            Err(err) => Err(format!("更新安装任务异常: {err}")),
        };
    if let Err(err) = install_result {
        cancel_visible_relaunch();
        return Err(err);
    }

    let _ = on_event.send(AppUpdateDownloadEvent::Finished);
    Ok(())
}

fn update_size_error() -> String {
    format!(
        "更新包超过 {} MiB 安全上限，已停止下载",
        limits::MAX_UPDATE_PACKAGE_BYTES / 1024 / 1024
    )
}

pub fn cancel_download(app: &AppHandle) -> Result<(), String> {
    app.state::<AppUpdaterState>().cancel_download()
}

pub fn clear_pending(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppUpdaterState>();
    if state.has_active_install() {
        return Err("更新正在下载或安装，无法清除".to_string());
    }
    *state.pending.lock().unwrap_or_else(|err| err.into_inner()) = None;
    Ok(())
}

impl AppUpdaterState {
    fn has_active_install(&self) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_some()
    }

    fn begin_install(&self) -> Result<(u64, watch::Receiver<bool>, Arc<AtomicU8>), String> {
        let mut active = self.active.lock().unwrap_or_else(|err| err.into_inner());
        if active.is_some() {
            return Err("更新正在下载或安装，请稍候".to_string());
        }

        let id = INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let (cancel, receiver) = watch::channel(false);
        let phase = Arc::new(AtomicU8::new(PHASE_DOWNLOADING));
        *active = Some(ActiveInstall {
            id,
            cancel,
            phase: Arc::clone(&phase),
        });
        Ok((id, receiver, phase))
    }

    fn finish_install(&self, id: u64) {
        let mut active = self.active.lock().unwrap_or_else(|err| err.into_inner());
        if active.as_ref().is_some_and(|install| install.id == id) {
            *active = None;
        }
    }

    fn cancel_download(&self) -> Result<(), String> {
        let active = self.active.lock().unwrap_or_else(|err| err.into_inner());
        let Some(active) = active.as_ref() else {
            return Err("当前没有正在下载的更新".to_string());
        };
        match active.phase.load(Ordering::Acquire) {
            PHASE_DOWNLOADING => active
                .cancel
                .send(true)
                .map_err(|_| "更新下载已经结束".to_string()),
            PHASE_VERIFYING => Err("更新已进入签名校验阶段，无法取消".to_string()),
            PHASE_INSTALLING => Err("更新已进入系统安装阶段，无法取消".to_string()),
            _ => Err("更新状态异常，无法取消".to_string()),
        }
    }
}

pub fn prepare_visible_relaunch() {
    std::env::set_var(VISIBLE_RELAUNCH_ENV, "1");
}

pub fn cancel_visible_relaunch() {
    std::env::remove_var(VISIBLE_RELAUNCH_ENV);
}

pub fn consume_visible_relaunch() -> bool {
    let requested = std::env::var_os(VISIBLE_RELAUNCH_ENV).is_some();
    if requested {
        std::env::remove_var(VISIBLE_RELAUNCH_ENV);
    }
    requested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_download_can_be_cancelled() {
        let state = AppUpdaterState::default();
        let (id, receiver, _) = state.begin_install().expect("install should start");

        state.cancel_download().expect("download should cancel");

        assert!(*receiver.borrow());
        state.finish_install(id);
        assert!(!state.has_active_install());
    }

    #[test]
    fn verifying_and_installing_phases_reject_cancellation() {
        for (phase_value, expected) in [
            (PHASE_VERIFYING, "签名校验"),
            (PHASE_INSTALLING, "系统安装"),
        ] {
            let state = AppUpdaterState::default();
            let (id, _, phase) = state.begin_install().expect("install should start");
            phase.store(phase_value, Ordering::Release);

            let error = state
                .cancel_download()
                .expect_err("phase should reject cancellation");
            assert!(error.contains(expected));
            state.finish_install(id);
        }
    }
}
