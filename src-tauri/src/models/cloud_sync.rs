use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncSettingsInput {
    pub server_url: String,
    pub username: String,
    /// None preserves the saved value; Some("") explicitly clears it.
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub remote_root: String,
    pub device_name: String,
    pub auto_sync: bool,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncSettingsView {
    pub server_url: String,
    pub username: String,
    pub remote_root: String,
    pub device_name: String,
    pub auto_sync: bool,
    pub has_password: bool,
    pub has_passphrase: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloudSyncPhase {
    #[default]
    Idle,
    Checking,
    Preparing,
    Review,
    Downloading,
    Uploading,
    Publishing,
    Applying,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncChange {
    pub key: String,
    pub title: String,
    pub category: String,
    pub conflict: bool,
    pub upload: bool,
    pub download: bool,
    pub local_deleted: bool,
    pub remote_deleted: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncReview {
    pub id: String,
    pub initial: bool,
    pub remote_device: String,
    pub remote_updated_at: Option<u64>,
    pub changes: Vec<CloudSyncChange>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncStatus {
    pub revision: u64,
    pub task_id: String,
    pub phase: CloudSyncPhase,
    pub message: String,
    pub running: bool,
    pub can_cancel: bool,
    pub progress: Option<f64>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub last_synced_at: Option<u64>,
    pub retry_at: Option<u64>,
    pub uploaded: usize,
    pub downloaded: usize,
    pub transferred_bytes: u64,
    pub review: Option<CloudSyncReview>,
    pub automatic: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncSnapshot {
    pub settings: CloudSyncSettingsView,
    pub status: CloudSyncStatus,
    pub has_recovery: bool,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CloudSyncSide {
    Local,
    Remote,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncResolution {
    pub key: String,
    pub side: CloudSyncSide,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncComparison {
    pub title: String,
    pub files: Vec<CloudSyncFileComparison>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncFileComparison {
    pub path: String,
    pub local_text: String,
    pub remote_text: String,
    pub binary: bool,
}
