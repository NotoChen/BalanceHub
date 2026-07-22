use serde::{Deserialize, Serialize};

#[path = "models/app_settings.rs"]
mod app_settings;
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

pub(crate) use app_settings::{
    default_liveness_interval, default_liveness_placeholder_pools,
    default_liveness_random_min_interval, default_liveness_timeout, default_true,
};
pub use app_settings::{
    AppSettings, LivenessPlaceholderPool, NotificationChannel, NotificationChannelKind,
};
pub use enums::*;
pub use liveness::*;
pub use provider::*;
pub use provider_results::*;
pub use workspace::*;

pub const CURRENT_SCHEMA_VERSION: u32 = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
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
