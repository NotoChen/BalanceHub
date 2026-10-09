use super::{runs, CheckInContext, CheckInPhase, Run, EVENT, REVISION};
use crate::adapters::browser::BrowserWindowControl;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};

pub(super) fn can_show(run: &Run) -> bool {
    run.executing
        && !run.task.finished
        && !*run.cancel.borrow()
        && run.window.is_some()
        && !matches!(
            run.task.phase,
            CheckInPhase::Queued | CheckInPhase::Opening | CheckInPhase::Saving
        )
}

pub(crate) async fn show(run_id: &str) -> Result<(), String> {
    let control = {
        let runs = runs().lock().map_err(|_| "无法显示签到窗口")?;
        let run = runs.get(run_id).ok_or("签到任务已结束")?;
        if !can_show(run) {
            return Err("签到窗口尚未打开或任务已结束，请查看任务进度".into());
        }
        run.window.clone().ok_or("签到窗口已关闭")?
    };
    control.show().await
}

pub(crate) struct CheckInWindow {
    app: AppHandle,
    run_id: String,
}

impl CheckInContext {
    pub(crate) fn attach_window(
        &self,
        app: &AppHandle,
        control: BrowserWindowControl,
    ) -> Result<CheckInWindow, String> {
        let mut runs = runs().lock().map_err(|_| "无法连接签到窗口")?;
        let run = runs.get_mut(&self.run_id).ok_or("签到任务已结束")?;
        if run.task.finished || !run.executing || *run.cancel.borrow() {
            return Err("签到已取消".into());
        }
        run.window = Some(control);
        Ok(CheckInWindow {
            app: app.clone(),
            run_id: self.run_id.clone(),
        })
    }
}

// Dropping an in-flight future on cancellation must also detach its control,
// so a late UI action cannot focus a closed window or reopen the task.
impl Drop for CheckInWindow {
    fn drop(&mut self) {
        let task = {
            let Ok(mut runs) = runs().lock() else {
                return;
            };
            let Some(run) = runs.get_mut(&self.run_id) else {
                return;
            };
            run.window = None;
            if !run.task.can_show_window {
                return;
            }
            run.task.can_show_window = false;
            run.task.revision = REVISION.fetch_add(1, Ordering::Relaxed) + 1;
            run.task.clone()
        };
        let _ = self.app.emit(EVENT, task);
    }
}
