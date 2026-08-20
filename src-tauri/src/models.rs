use serde::{Deserialize, Serialize};

#[path = "models/agent_cli.rs"]
mod agent_cli;
#[path = "models/app_settings.rs"]
mod app_settings;
#[path = "models/cli_sessions.rs"]
mod cli_sessions;
#[path = "models/enums.rs"]
mod enums;
#[path = "models/liveness.rs"]
mod liveness;
#[path = "models/provider.rs"]
mod provider;
#[path = "models/provider_domain.rs"]
pub mod provider_domain;
#[path = "models/provider_results.rs"]
mod provider_results;
#[path = "models/workspace.rs"]
mod workspace;

pub use agent_cli::*;
pub(crate) use app_settings::{
    default_liveness_interval, default_liveness_placeholder_pools,
    default_liveness_random_min_interval, default_liveness_timeout,
    default_session_index_max_size_mib, default_true,
};
pub use app_settings::{
    AppSettings, LivenessPlaceholderPool, NotificationChannel, NotificationChannelKind,
};
pub use cli_sessions::{
    CliSessionDetail, CliSessionIndexAgentStats, CliSessionIndexState, CliSessionIndexStatus,
    CliSessionMessage, CliSessionMessageRole, CliSessionSearchResponse, CliSessionSearchResult,
    CliSessionSummary,
};
pub use enums::*;
pub use liveness::*;
pub use provider::*;
pub(crate) use provider_results::is_full_api_key_value;
pub use provider_results::*;
pub use workspace::*;

pub const CURRENT_SCHEMA_VERSION: u32 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    /// 仅用于当前进程内 IPC 快照排序，不写入本地配置或导出文件。
    #[serde(skip)]
    pub revision: u64,
    #[serde(default)]
    pub schema_version: u32,
    pub providers: Vec<Provider>,
    pub settings: AppSettings,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub temporary_cli_preferences: Vec<TemporaryCliPreference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataTransferResult {
    pub path: String,
    pub schema_version: u32,
    pub provider_count: usize,
}

impl AppData {
    pub fn new_current(providers: Vec<Provider>, settings: AppSettings) -> Self {
        Self {
            revision: 0,
            schema_version: CURRENT_SCHEMA_VERSION,
            providers,
            settings,
            workspaces: Vec::new(),
            temporary_cli_preferences: Vec::new(),
        }
    }
}

impl Default for AppData {
    fn default() -> Self {
        Self::new_current(Vec::new(), AppSettings::default())
    }
}
