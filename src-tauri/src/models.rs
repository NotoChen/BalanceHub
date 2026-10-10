use serde::{Deserialize, Serialize};

#[path = "models/api_key_management.rs"]
mod api_key_management;
pub use api_key_management::*;

#[path = "models/agent_asset_control.rs"]
mod agent_asset_control;
#[path = "models/agent_asset_provenance.rs"]
mod agent_asset_provenance;
#[path = "models/agent_catalog.rs"]
mod agent_catalog;
#[path = "models/agent_cli.rs"]
mod agent_cli;
#[path = "models/agent_configuration.rs"]
mod agent_configuration;
#[path = "models/agent_environment.rs"]
mod agent_environment;
#[path = "models/agent_hook.rs"]
mod agent_hook;
#[path = "models/agent_lifecycle.rs"]
mod agent_lifecycle;
#[path = "models/agent_overview.rs"]
mod agent_overview;
#[path = "models/agent_runtime.rs"]
mod agent_runtime;
#[path = "models/agent_session_resume.rs"]
mod agent_session_resume;
#[path = "models/agent_sessions.rs"]
mod agent_sessions;
#[path = "models/app_settings.rs"]
mod app_settings;
#[path = "models/check_in_task.rs"]
mod check_in_task;
#[path = "models/cli_sessions.rs"]
mod cli_sessions;
#[path = "models/cloud_sync.rs"]
mod cloud_sync;
#[path = "models/enums.rs"]
mod enums;
#[path = "models/liveness.rs"]
mod liveness;
#[path = "models/login_account.rs"]
mod login_account;
#[path = "models/provider.rs"]
mod provider;
#[path = "models/provider_domain.rs"]
pub mod provider_domain;
#[path = "models/provider_results.rs"]
mod provider_results;
#[path = "models/workspace.rs"]
mod workspace;

pub use agent_asset_control::*;
pub use agent_asset_provenance::*;
pub use agent_catalog::*;
pub use agent_cli::*;
pub use agent_configuration::*;
pub use agent_environment::*;
pub use agent_hook::*;
pub use agent_lifecycle::*;
pub use agent_overview::*;
pub use agent_runtime::*;
pub use agent_session_resume::*;
pub use agent_sessions::*;
pub(crate) use app_settings::{
    default_liveness_interval, default_liveness_placeholder_pools,
    default_liveness_random_min_interval, default_liveness_timeout,
    default_session_index_max_size_mib, default_true,
};
pub use app_settings::{
    AppSettings, LivenessPlaceholderPool, NotificationChannel, NotificationChannelKind,
};
pub use check_in_task::*;
pub use cli_sessions::{
    CliSessionDetail, CliSessionIndexAgentStats, CliSessionIndexState, CliSessionIndexStatus,
    CliSessionMessage, CliSessionMessageRole, CliSessionSummary,
};
pub use cloud_sync::*;
pub use enums::*;
pub use liveness::*;
pub use login_account::*;
pub use provider::*;
pub(crate) use provider_results::is_full_api_key_value;
pub use provider_results::*;
pub use workspace::*;

pub const CURRENT_SCHEMA_VERSION: u32 = 12;

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
    #[serde(default)]
    pub login_accounts: Vec<LoginAccount>,
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
            login_accounts: Vec::new(),
        }
    }
}

impl Default for AppData {
    fn default() -> Self {
        Self::new_current(Vec::new(), AppSettings::default())
    }
}
