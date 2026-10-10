use super::{
    config::Settings,
    core::{Control, Prepared, Replica, TransferStats},
    Action, CloudSyncService, Outcome,
};
use crate::models::CloudSyncPhase;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tauri::AppHandle;

impl CloudSyncService {
    pub fn start_worker(self: &Arc<Self>, app: AppHandle) {
        if self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(self);
        let signal = Arc::clone(&self.signal);
        tauri::async_runtime::spawn(async move {
            loop {
                let changed = tokio::select! {
                    _ = signal.notify.notified() => true,
                    _ = tokio::time::sleep(Duration::from_secs(60)) => false,
                };
                // Coalesce a burst of saves without starving continuous edits.
                if changed {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
                    loop {
                        tokio::select! {
                            _ = signal.notify.notified() => {},
                            _ = tokio::time::sleep(Duration::from_secs(1)) => break,
                            _ = tokio::time::sleep_until(deadline) => break,
                        }
                    }
                }
                let Some(service) = weak.upgrade() else {
                    break;
                };
                let should_run = {
                    let inner = service.lock();
                    inner.settings.auto_sync
                        && inner.active.is_none()
                        && inner.pending.is_none()
                        && inner.config_error.is_none()
                        && inner.settings.ready().is_ok()
                        && inner
                            .status
                            .retry_at
                            .is_none_or(|time| time <= super::format::now())
                        && (changed || inner.status.phase != CloudSyncPhase::Cancelled)
                };
                if should_run {
                    let _ = service.start(app.clone(), Action::Sync, true);
                }
            }
        });
    }

    pub(super) async fn execute(
        self: &Arc<Self>,
        app: &AppHandle,
        task_id: &str,
        settings: Settings,
        control: Arc<Control>,
        action: Action,
        pending: Option<Arc<Prepared>>,
    ) -> Result<Outcome, String> {
        let operation = self.run_action(
            app,
            task_id,
            settings,
            Arc::clone(&control),
            action,
            pending,
        );
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => result,
            _ = control.cancelled.notified() => Err("同步已取消，已提交的完整版本仍然保留".to_owned()),
            _ = tokio::time::sleep(Duration::from_secs(900)) => {
                if control.expire() { Err("本次同步超过 15 分钟，已停止；可以稍后重试".to_owned()) }
                else { operation.await }
            }
        }
    }

    async fn run_action(
        self: &Arc<Self>,
        app: &AppHandle,
        task_id: &str,
        settings: Settings,
        control: Arc<Control>,
        action: Action,
        pending: Option<Arc<Prepared>>,
    ) -> Result<Outcome, String> {
        if matches!(action, Action::Restore) {
            let replica = Arc::new(super::local::AppReplica { app: app.clone() });
            let reader = Arc::clone(&replica);
            let (current, recovery) = tauri::async_runtime::spawn_blocking(move || {
                let recovery = reader.recovery()?.ok_or("同步恢复点不存在")?;
                reader.validate(&recovery)?;
                Ok::<_, String>((reader.snapshot()?, recovery))
            })
            .await
            .map_err(|_| "读取同步恢复点任务异常")??;
            control.begin_commit()?;
            self.progress(
                app,
                task_id,
                CloudSyncPhase::Applying,
                "正在恢复同步前的本机配置",
                TransferStats::default(),
            );
            tauri::async_runtime::spawn_blocking(move || replica.apply(&current, &recovery))
                .await
                .map_err(|_| "恢复同步配置任务异常")??;
            return Ok(Outcome::Restored);
        }
        let stamp = self.local_stamp();
        let automatic = self.lock().status.automatic;
        let engine = self
            .engine(app, task_id, &settings, Arc::clone(&control))
            .await?;
        control.check()?;
        let verification = settings.verification_id()?;
        let needs_verification =
            matches!(action, Action::Test) || self.lock().verified.as_ref() != Some(&verification);
        if needs_verification {
            self.progress(
                app,
                task_id,
                CloudSyncPhase::Checking,
                "正在验证 WebDAV 读写与多设备同步能力",
                TransferStats::default(),
            );
            engine.dav.test().await?;
            self.lock().verified = Some(verification);
        }
        if matches!(action, Action::Test) {
            return Ok(Outcome::Tested);
        }
        // A quiet automatic check reads only the remote ETag and local library
        // metadata, without rescanning assets or deriving the encryption key.
        let unchanged_local = stamp.is_some() && self.lock().reconciled == stamp;
        if automatic
            && matches!(action, Action::Sync)
            && unchanged_local
            && engine.remote_unchanged().await?
            && stamp == self.local_stamp()
        {
            return Ok(Outcome::Synced(TransferStats::default()));
        }
        if let Action::Confirm { resolutions, .. } = action {
            let prepared = pending.ok_or("同步预览已失效，请重新同步")?;
            let stats = engine.commit(prepared, &resolutions).await?;
            if stamp == self.local_stamp() {
                self.lock().reconciled = stamp;
            }
            return Ok(Outcome::Synced(stats));
        }
        let prepared = Arc::new(engine.prepare().await?);
        let review = prepared.review()?;
        if (review.initial && !review.changes.is_empty())
            || review.changes.iter().any(|change| change.conflict)
        {
            Ok(Outcome::Review(prepared))
        } else {
            let stats = engine.commit(prepared, &[]).await?;
            if stamp == self.local_stamp() {
                self.lock().reconciled = stamp;
            }
            Ok(Outcome::Synced(stats))
        }
    }
}
