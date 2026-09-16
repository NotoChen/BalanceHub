use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckInPhase {
    Queued,
    Checking,
    Opening,
    Verifying,
    WaitingHuman,
    WaitingBrowser,
    Requesting,
    VerifyingResult,
    Saving,
    Completed,
    Failed,
    Cancelled,
    Unconfirmed,
}

impl CheckInPhase {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Queued => "已加入签到队列",
            Self::Checking => "正在读取今日签到状态",
            Self::Opening => "正在打开签到验证窗口",
            Self::Verifying => "正在完成站点验证",
            Self::WaitingHuman => "需要人工验证，点击继续验证",
            Self::WaitingBrowser => "需要安装或修复浏览器签到组件",
            Self::Requesting => "正在提交签到",
            Self::VerifyingResult => "正在向站点确认签到结果",
            Self::Saving => "正在保存签到记录",
            Self::Completed => "签到结果已确认并保存",
            Self::Failed => "签到未完成",
            Self::Cancelled => "签到已取消",
            Self::Unconfirmed => "签到结果尚未确认，已停止自动重试",
        }
    }

    pub(crate) fn finished(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Unconfirmed
        )
    }

    pub(crate) fn waiting(self) -> bool {
        matches!(self, Self::WaitingHuman | Self::WaitingBrowser)
    }

    pub(crate) fn may_have_submitted(self) -> bool {
        matches!(
            self,
            Self::Requesting | Self::VerifyingResult | Self::Saving
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckInSource {
    Manual,
    Batch,
    Automatic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckInTask {
    pub run_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub batch_id: Option<String>,
    pub source: CheckInSource,
    pub phase: CheckInPhase,
    pub message: String,
    pub revision: u64,
    pub finished: bool,
    pub can_resume: bool,
    pub can_cancel: bool,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckInBatch {
    pub batch_id: String,
    pub tasks: Vec<CheckInTask>,
    pub skipped: usize,
}

/// Control flow stays typed from the browser pipe through the task registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckInError {
    WaitingHuman,
    WaitingBrowser(String),
    Unconfirmed(String),
    Failed(String),
}

impl From<String> for CheckInError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for CheckInError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_string())
    }
}

impl std::fmt::Display for CheckInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WaitingHuman => f.write_str(CheckInPhase::WaitingHuman.message()),
            Self::WaitingBrowser(message) | Self::Unconfirmed(message) | Self::Failed(message) => {
                f.write_str(message)
            }
        }
    }
}
