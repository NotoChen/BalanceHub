use serde::Serialize;

/// 前端收到后重新读取 Provider 视图，包括运行状态和内存中的挑战状态。
pub(crate) const PROVIDERS_CHANGED_EVENT: &str = "providers-changed";

/// 后台调度器向面板报告当前自动任务，避免网络请求期间看起来像应用卡死。
pub(crate) const BACKGROUND_TASK_EVENT: &str = "background-task";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundTaskEvent {
    pub task_id: String,
    pub kind: String,
    pub status: String,
    pub title: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_task_event_uses_frontend_field_names() {
        let value = serde_json::to_value(BackgroundTaskEvent {
            task_id: "scheduler-refresh".to_string(),
            kind: "autoRefresh".to_string(),
            status: "running".to_string(),
            title: "自动刷新中转站".to_string(),
            detail: "正在同步".to_string(),
            progress: None,
            started_at: 1,
            finished_at: None,
            error: None,
        })
        .expect("background task event should serialize");

        assert_eq!(value["taskId"], "scheduler-refresh");
        assert_eq!(value["startedAt"], 1);
        assert!(value.get("task_id").is_none());
    }
}
