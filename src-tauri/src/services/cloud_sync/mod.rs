mod comparison;
mod config;
mod core;
mod crypto;
mod files;
mod format;
mod local;
mod merge;
mod projection;
#[cfg(test)]
mod tests;
mod transport;
mod worker;

pub(crate) use format::{SyncDocument, SyncDocuments};
pub(crate) use local::recover;
pub(crate) use projection::app_changed;

use crate::{models::*, state::AppState};
use config::Settings;
use core::{Control, Engine, Prepared, TransferStats};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub(crate) struct SyncSignal {
    enabled: AtomicBool,
    generation: AtomicU64,
    notify: tokio::sync::Notify,
}

impl SyncSignal {
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if self.enabled() {
            self.notify.notify_one();
        }
    }
    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        self.changed();
        self.notify.notify_one();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct LocalStamp {
    generation: u64,
    library: Option<(u64, SystemTime)>,
}

struct Inner {
    settings: Settings,
    status: CloudSyncStatus,
    config_error: Option<String>,
    active: Option<Arc<Control>>,
    pending: Option<Arc<Prepared>>,
    verified: Option<String>,
    failures: u32,
    reconciled: Option<LocalStamp>,
}

pub(crate) struct CloudSyncService {
    root: PathBuf,
    inner: Mutex<Inner>,
    signal: Arc<SyncSignal>,
    worker_started: AtomicBool,
}

pub(crate) enum Action {
    Sync,
    Test,
    Confirm {
        review_id: String,
        resolutions: Vec<CloudSyncResolution>,
    },
    Restore,
}

enum Outcome {
    Tested,
    Synced(TransferStats),
    Review(Arc<Prepared>),
    Restored,
}

impl CloudSyncService {
    pub fn new(root: PathBuf, signal: Arc<SyncSignal>) -> Self {
        let (settings, config_error) = match Settings::load(&root) {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        };
        let mut status: CloudSyncStatus = files::read_json(&root.join("status.json"))
            .ok()
            .flatten()
            .unwrap_or_default();
        status.revision = status.revision.saturating_add(1);
        if status.running || status.review.is_some() {
            status.phase = CloudSyncPhase::Cancelled;
            status.message = "上次任务在应用退出时中断，请重新同步".to_owned();
            status.finished_at = Some(format::now());
        }
        status.running = false;
        status.can_cancel = false;
        status.review = None;
        status.retry_at = None;
        if let Some(error) = &config_error {
            status.phase = CloudSyncPhase::Failed;
            status.message.clone_from(error);
        }
        signal.set_enabled(settings.auto_sync && config_error.is_none());
        Self {
            root,
            signal,
            worker_started: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                settings,
                status,
                config_error,
                active: None,
                pending: None,
                verified: None,
                failures: 0,
                reconciled: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn snapshot_locked(&self, inner: &Inner) -> CloudSyncSnapshot {
        CloudSyncSnapshot {
            settings: inner.settings.view(),
            status: inner.status.clone(),
            has_recovery: self.root.join("recovery.json").is_file(),
        }
    }

    pub fn snapshot(&self) -> CloudSyncSnapshot {
        self.snapshot_locked(&self.lock())
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit("cloud-sync-state", self.snapshot());
    }

    fn local_stamp(&self) -> Option<LocalStamp> {
        let path = self.root.parent()?.join("agent-asset-library/library.json");
        let library = match std::fs::metadata(path) {
            Ok(meta) => Some((meta.len(), meta.modified().ok()?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return None,
        };
        Some(LocalStamp {
            generation: self.signal.generation.load(Ordering::Acquire),
            library,
        })
    }

    pub fn save(
        &self,
        app: &AppHandle,
        input: CloudSyncSettingsInput,
    ) -> Result<CloudSyncSnapshot, String> {
        {
            let mut inner = self.lock();
            if let Some(error) = &inner.config_error {
                return Err(error.clone());
            }
            if inner.active.is_some() {
                return Err("同步任务仍在执行，请先取消或等待完成后保存连接设置".to_owned());
            }
            let settings = inner.settings.update(input)?;
            let same_space = settings.space_id().ok() == inner.settings.space_id().ok();
            settings.save(&self.root)?;
            inner.settings = settings;
            inner.pending = None;
            inner.verified = None;
            inner.failures = 0;
            inner.reconciled = None;
            inner.status = CloudSyncStatus {
                revision: inner.status.revision.saturating_add(1),
                message: "同步设置已保存".to_owned(),
                last_synced_at: same_space.then_some(inner.status.last_synced_at).flatten(),
                ..CloudSyncStatus::default()
            };
            if let Err(error) = files::write_json(&self.root.join("status.json"), &inner.status) {
                inner.status.message = format!("同步设置已保存；{error}");
            }
            self.signal.set_enabled(inner.settings.auto_sync);
        }
        self.emit(app);
        Ok(self.snapshot())
    }

    /// Only reserves a task and schedules work. No network, crypto or catalog
    /// scan is awaited by the initiating IPC or the settings window.
    pub fn start(
        self: &Arc<Self>,
        app: AppHandle,
        action: Action,
        automatic: bool,
    ) -> Result<CloudSyncSnapshot, String> {
        let (settings, control, task_id, pending) = {
            let mut inner = self.lock();
            if let Some(error) = &inner.config_error {
                return Err(error.clone());
            }
            if inner.active.is_some() {
                return Err("已有同步任务正在执行，可在任务中心查看进度".to_owned());
            }
            if !matches!(action, Action::Restore | Action::Test) {
                inner.settings.ready()?;
            }
            if matches!(action, Action::Test) {
                transport::normalize_url(&inner.settings.server_url, &inner.settings.remote_root)?;
            }
            let pending = if let Action::Confirm {
                review_id,
                resolutions,
            } = &action
            {
                let prepared = inner
                    .pending
                    .as_ref()
                    .filter(|value| value.id == *review_id)
                    .ok_or("同步预览已失效，请重新同步")?;
                if merge::merge(
                    &prepared.baseline,
                    &prepared.local,
                    &prepared.remote,
                    &prepared.manifest,
                    resolutions,
                )?
                .unresolved
                {
                    return Err("请先处理所有同步冲突".to_owned());
                }
                Some(Arc::clone(prepared))
            } else {
                None
            };
            if matches!(action, Action::Restore) {
                if !self.root.join("recovery.json").is_file() {
                    return Err("还没有可恢复的同步前配置".to_owned());
                }
                let mut settings = inner.settings.clone();
                settings.auto_sync = false;
                settings.save(&self.root)?;
                inner.settings = settings;
                self.signal.set_enabled(false);
            }
            let control = Arc::new(Control::default());
            let task_id = format!("cloud-sync-{}", format::random_id()?);
            let status = CloudSyncStatus {
                revision: inner.status.revision.saturating_add(1),
                task_id: task_id.clone(),
                phase: CloudSyncPhase::Checking,
                message: "正在准备同步任务".to_owned(),
                running: true,
                can_cancel: true,
                started_at: Some(format::now()),
                last_synced_at: inner.status.last_synced_at,
                automatic,
                ..CloudSyncStatus::default()
            };
            files::write_json(&self.root.join("status.json"), &status)?;
            inner.status = status;
            inner.active = Some(Arc::clone(&control));
            inner.pending = None;
            (inner.settings.clone(), control, task_id, pending)
        };
        self.emit(&app);
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(service.execute(
                &app,
                &task_id,
                settings,
                Arc::clone(&control),
                action,
                pending,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err("同步后台任务异常，已释放操作状态；请重新同步".to_owned()));
            service.finish(&app, &task_id, &control, result);
        });
        Ok(self.snapshot())
    }

    pub fn cancel(&self, app: &AppHandle, task_id: &str) -> Result<CloudSyncSnapshot, String> {
        {
            let mut inner = self.lock();
            if inner.status.task_id != task_id {
                return Err("任务已变化，请刷新后重试".to_owned());
            }
            if let Some(control) = &inner.active {
                if !control.cancel() {
                    return Err("正在提交完整版本，请等待当前事务完成".to_owned());
                }
                inner.status.message = "正在取消同步".to_owned();
                inner.status.can_cancel = false;
            } else if inner.pending.take().is_some() {
                inner.status.phase = CloudSyncPhase::Cancelled;
                inner.status.message = "已取消本次同步预览".to_owned();
                inner.status.review = None;
                inner.status.can_cancel = false;
                inner.status.finished_at = Some(format::now());
            }
            inner.status.revision = inner.status.revision.saturating_add(1);
        }
        self.emit(app);
        Ok(self.snapshot())
    }

    pub fn compare(&self, review_id: &str, key: &str) -> Result<CloudSyncComparison, String> {
        let pending = self
            .lock()
            .pending
            .as_ref()
            .filter(|value| value.id == review_id)
            .cloned()
            .ok_or("同步预览已失效，请重新同步")?;
        comparison::compare(&pending, key)
    }

    fn progress(
        &self,
        app: &AppHandle,
        task_id: &str,
        phase: CloudSyncPhase,
        message: &str,
        stats: TransferStats,
    ) {
        {
            let mut inner = self.lock();
            if inner.status.task_id != task_id || inner.active.is_none() {
                return;
            }
            inner.status.can_cancel = inner
                .active
                .as_ref()
                .is_some_and(|control| control.can_cancel());
            inner.status.revision = inner.status.revision.saturating_add(1);
            inner.status.phase = phase;
            inner.status.message = message.to_owned();
            inner.status.progress =
                (stats.total > 0).then(|| stats.done as f64 / stats.total as f64);
            inner.status.uploaded = stats.uploaded;
            inner.status.downloaded = stats.downloaded;
            inner.status.transferred_bytes = stats.bytes;
        }
        self.emit(app);
    }

    fn finish(
        &self,
        app: &AppHandle,
        task_id: &str,
        control: &Control,
        result: Result<Outcome, String>,
    ) {
        {
            let mut inner = self.lock();
            if inner.status.task_id != task_id {
                return;
            }
            inner.active = None;
            inner.status.running = false;
            inner.status.can_cancel = false;
            inner.status.progress = None;
            inner.status.finished_at = Some(format::now());
            inner.status.retry_at = None;
            inner.status.revision = inner.status.revision.saturating_add(1);
            // Cancellation can arrive after execute resolves but before this
            // task acquires the status lock. An accepted cancel owns the final
            // state and must not be replaced by a newly prepared review.
            let result = if control.is_cancelled() {
                Err("同步已取消，已提交的完整版本仍然保留".to_owned())
            } else {
                result
            };
            let result = match result {
                Ok(Outcome::Review(prepared)) => match prepared.review() {
                    Ok(review) => {
                        inner.status.phase = CloudSyncPhase::Review;
                        inner.status.message = if review.initial {
                            "首次同步，请核对两端差异"
                        } else {
                            "两端修改了相同配置，请处理冲突"
                        }
                        .to_owned();
                        inner.status.review = Some(review);
                        inner.status.can_cancel = true;
                        inner.status.finished_at = None;
                        inner.pending = Some(prepared);
                        inner.failures = 0;
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Ok(outcome) => {
                    inner.status.phase = CloudSyncPhase::Completed;
                    inner.failures = 0;
                    inner.status.message = match outcome {
                        Outcome::Tested => "连接正常，读写及多设备条件写入验证通过".to_owned(),
                        Outcome::Restored => "已恢复同步前的本机配置，自动同步已暂停".to_owned(),
                        Outcome::Synced(stats) => {
                            inner.status.last_synced_at = Some(format::now());
                            inner.status.uploaded = stats.uploaded;
                            inner.status.downloaded = stats.downloaded;
                            inner.status.transferred_bytes = stats.bytes;
                            if stats.uploaded == 0 && stats.downloaded == 0 {
                                "两端配置已一致，无需传输新资源".to_owned()
                            } else {
                                format!(
                                    "同步完成 · 上传 {} 项，下载 {} 项",
                                    stats.uploaded, stats.downloaded
                                )
                            }
                        }
                        Outcome::Review(_) => unreachable!(),
                    };
                    Ok(())
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                inner.status.phase = if control.is_cancelled() {
                    CloudSyncPhase::Cancelled
                } else {
                    CloudSyncPhase::Failed
                };
                inner.status.message = error;
                if inner.settings.auto_sync && !control.is_cancelled() {
                    inner.failures = inner.failures.saturating_add(1);
                    let seconds = 30u64.saturating_mul(1 << inner.failures.min(6));
                    inner.status.retry_at = Some(format::now() + seconds.min(1800) * 1000);
                }
            }
            if let Err(error) = files::write_json(&self.root.join("status.json"), &inner.status) {
                inner.status.message = format!("{}；{error}", inner.status.message);
            }
            if inner.settings.auto_sync
                && inner.status.phase == CloudSyncPhase::Completed
                && inner.reconciled != self.local_stamp()
            {
                self.signal.notify.notify_one();
            }
        }
        self.emit(app);
    }

    async fn engine(
        self: &Arc<Self>,
        app: &AppHandle,
        task_id: &str,
        settings: &Settings,
        control: Arc<Control>,
    ) -> Result<Engine, String> {
        let task_app = app.clone();
        let settings = settings.clone();
        let root = self.root.join("replicas").join(settings.space_id()?);
        let passphrase = settings.passphrase.clone();
        let device = settings.device_name.clone();
        let dav = tauri::async_runtime::spawn_blocking(move || {
            let app_state = task_app.state::<AppState>();
            let preferences = app_state
                .data
                .read()
                .map_err(|_| "本机网络设置不可用")?
                .settings
                .clone();
            transport::DavClient::new(
                crate::network::build_sync_client(&preferences)?,
                &settings.server_url,
                &settings.remote_root,
                &settings.username,
                &settings.password,
            )
        })
        .await
        .map_err(|_| "初始化同步网络任务异常")??;
        let weak = Arc::downgrade(self);
        let progress_app = app.clone();
        let progress_id = task_id.to_owned();
        Ok(Engine {
            root,
            dav,
            passphrase,
            device,
            control,
            replica: Arc::new(local::AppReplica { app: app.clone() }),
            progress: Arc::new(move |phase, message, stats| {
                if let Some(service) = weak.upgrade() {
                    service.progress(&progress_app, &progress_id, phase, message, stats);
                }
            }),
        })
    }
}
