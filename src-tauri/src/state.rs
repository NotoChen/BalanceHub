use crate::models::AppData;
use balancehub_core::state::CoreState;
use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
};
use tauri::Manager;

use crate::services::agent_cli::{catalog::CatalogService, lifecycle::LifecycleService};

/// 桌面进程的应用状态。
///
/// 配置数据、刷新闸门和版本号属于 `balancehub-core`；这里只额外持有
/// 桌面专属的 Agent 服务。`Deref` 让现有 `state.data`、`state.next_revision()`
/// 之类的写法继续成立。
pub struct AppState {
    pub core: Arc<CoreState>,
    pub(crate) agent_asset_access:
        Arc<crate::services::agent_cli::environment::access_registry::AgentAssetAccessRegistry>,
    pub(crate) agent_asset_mutations:
        Arc<crate::services::agent_cli::environment::mutation::MutationService>,
    pub(crate) agent_session_resumes:
        Arc<crate::services::cli_sessions::resume::AgentSessionResumeService>,
    agent_configuration:
        OnceLock<Arc<crate::services::agent_cli::configuration::ConfigurationService>>,
    agent_catalog: OnceLock<Arc<CatalogService>>,
    pub(crate) mcp_checks: Arc<crate::services::agent_cli::mcp_check::McpCheckService>,
    pub(crate) agent_maintenance: Arc<crate::services::agent_cli::maintenance::MaintenanceService>,
    agent_lifecycle: OnceLock<Arc<LifecycleService>>,
    cloud_sync: OnceLock<Arc<crate::services::cloud_sync::CloudSyncService>>,
    pub(crate) cloud_sync_signal: Arc<crate::services::cloud_sync::SyncSignal>,
    agent_overview: OnceLock<Arc<crate::services::agent_cli::overview::OverviewService>>,
    asset_actors: Mutex<BTreeMap<String, String>>,
}

impl Deref for AppState {
    type Target = CoreState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl AppState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(data: AppData, load_error: Option<String>) -> Self {
        Self {
            core: Arc::new(CoreState::with_load_error(data, load_error)),
            agent_asset_access: Arc::default(),
            agent_asset_mutations: Arc::default(),
            agent_session_resumes: Arc::default(),
            agent_configuration: OnceLock::new(),
            agent_catalog: OnceLock::new(),
            mcp_checks: Arc::default(),
            agent_maintenance: Arc::default(),
            agent_lifecycle: OnceLock::new(),
            cloud_sync: OnceLock::new(),
            cloud_sync_signal: Arc::default(),
            agent_overview: OnceLock::new(),
            asset_actors: Mutex::default(),
        }
    }

    /// The managed library is isolated from provider configuration and exports.
    /// Construction does not adopt or mutate any native Agent configuration.
    pub(crate) fn agent_catalog(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Arc<CatalogService>, String> {
        let root = crate::desktop::config_dir(app)
            .map_err(|_| "无法获取应用资产库目录".to_owned())?
            .join("agent-asset-library");
        let managed_hook_root =
            crate::desktop::data_dir(app).map_err(|_| "无法获取会话接入目录".to_owned())?;
        let display_cache_root = app
            .path()
            .app_cache_dir()
            .map_err(|_| "无法获取应用资产缓存目录".to_owned())?
            .join("agent-catalog");
        Ok(Arc::clone(self.agent_catalog.get_or_init(|| {
            Arc::new(
                CatalogService::new(root, Arc::clone(&self.agent_asset_mutations))
                    .with_sync_signal(Arc::clone(&self.cloud_sync_signal))
                    .with_managed_hook_root(managed_hook_root)
                    .with_display_cache(display_cache_root),
            )
        })))
    }

    pub(crate) fn agent_configuration(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<
        Arc<crate::services::agent_cli::configuration::ConfigurationService>,
        crate::models::AgentConfigurationError,
    > {
        use crate::models::{AgentConfigurationError, AgentConfigurationErrorKind};
        let home = crate::services::cli_paths::user_home().ok_or_else(|| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
        })?;
        let root = crate::desktop::data_dir(app).map_err(|_| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::InternalFailure)
        })?;
        Ok(Arc::clone(self.agent_configuration.get_or_init(|| {
            Arc::new(
                crate::services::agent_cli::configuration::ConfigurationService::new(
                    home,
                    root,
                    Arc::clone(&self.agent_asset_access),
                ),
            )
        })))
    }

    pub(crate) fn agent_lifecycle(&self) -> Arc<LifecycleService> {
        Arc::clone(self.agent_lifecycle.get_or_init(|| {
            Arc::new(LifecycleService::new(
                self.agent_asset_mutations.domain_locks(),
            ))
        }))
    }

    pub(crate) fn agent_overview(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Arc<crate::services::agent_cli::overview::OverviewService>, String> {
        let root = crate::desktop::data_dir(app)
            .map_err(|_| "无法获取 Agent 摘要目录")?
            .join("agent-overview");
        Ok(Arc::clone(self.agent_overview.get_or_init(|| {
            Arc::new(crate::services::agent_cli::overview::OverviewService::new(
                root,
            ))
        })))
    }

    /// The window label is supplied by Tauri, and a new window lifetime gets a
    /// new actor even if it reuses the same label. Actors never come from IPC.
    pub(crate) fn asset_actor(&self, window_label: &str) -> Result<String, ()> {
        let mut actors = self.asset_actors.lock().map_err(|_| ())?;
        if let Some(actor) = actors.get(window_label) {
            return Ok(actor.clone());
        }
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| ())?;
        let actor = format!("{window_label}:{:032x}", u128::from_ne_bytes(nonce));
        actors.insert(window_label.to_owned(), actor.clone());
        Ok(actor)
    }

    pub(crate) fn clear_asset_actor(&self, window_label: &str) {
        let actor = self
            .asset_actors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(window_label);
        if let Some(actor) = actor {
            self.mcp_checks.cancel_actor(&actor);
            self.agent_maintenance.remove_actor(&actor);
            self.agent_asset_access.remove_actor(&actor);
            if let Some(configuration) = self.agent_configuration.get() {
                configuration.remove_actor(&actor);
            }
            self.agent_asset_mutations.remove_actor(&actor);
            if let Some(catalog) = self.agent_catalog.get() {
                catalog.remove_actor(&actor);
            }
            if let Some(lifecycle) = self.agent_lifecycle.get() {
                lifecycle.remove_actor(&actor);
            }
        }
    }

    pub(crate) fn cloud_sync(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Arc<crate::services::cloud_sync::CloudSyncService>, String> {
        let root = crate::desktop::config_dir(app)
            .map_err(|_| "无法获取同步配置目录")?
            .join("cloud-sync");
        Ok(Arc::clone(self.cloud_sync.get_or_init(|| {
            Arc::new(crate::services::cloud_sync::CloudSyncService::new(
                root,
                Arc::clone(&self.cloud_sync_signal),
            ))
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_actor_is_bound_to_one_window_lifetime() {
        let state = AppState::new(AppData::default());
        let main = state.asset_actor("main").unwrap();
        let other = state.asset_actor("other").unwrap();
        assert_eq!(main, state.asset_actor("main").unwrap());
        assert_ne!(main, other);
        state.clear_asset_actor("main");
        assert_ne!(main, state.asset_actor("main").unwrap());
        assert_eq!(other, state.asset_actor("other").unwrap());
    }
}
