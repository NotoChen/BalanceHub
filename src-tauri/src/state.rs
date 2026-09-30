use crate::models::AppData;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock, RwLock,
    },
};
use tauri::Manager;

use crate::services::agent_cli::{catalog::CatalogService, lifecycle::LifecycleService};

/// 内存中的应用状态。
///
/// 启动时从磁盘加载一次，之后所有读写都走内存，变更时再原子落盘。
/// 用 `RwLock` 串行化写入，避免并发命令（自动刷新、保存设置、签到……）
/// 各自「读磁盘 → 改 → 写磁盘」时互相覆盖导致丢更新。
#[derive(Default)]
pub struct AppState {
    pub data: RwLock<AppData>,
    /// 网络认证互斥闸门：手动刷新、调度刷新以及可能轮换 Sub2API
    /// `refresh_token` 的交互操作不能同时运行。手动路径 `lock().await` 排队，
    /// 调度器 `try_lock` 拿不到直接跳过本 tick（下个 tick 重新评估到期），
    /// 两边都不会饿死。
    pub refresh_gate: tokio::sync::Mutex<()>,
    /// 串行化“基于快照修改并落盘”的事务。持有该锁时不会长期占用 `data` 写锁，
    /// 因此 JSON 序列化和磁盘替换期间读取方仍可继续读取上一份完整状态。
    pub mutation_gate: Mutex<()>,
    pub(crate) agent_asset_access:
        Arc<crate::services::agent_cli::environment::access_registry::AgentAssetAccessRegistry>,
    pub(crate) agent_asset_mutations:
        Arc<crate::services::agent_cli::environment::mutation::MutationService>,
    pub(crate) agent_session_resumes:
        Arc<crate::services::cli_sessions::resume::AgentSessionResumeService>,
    agent_configuration:
        OnceLock<Arc<crate::services::agent_cli::configuration::ConfigurationService>>,
    agent_catalog: OnceLock<Arc<CatalogService>>,
    agent_lifecycle: OnceLock<Arc<LifecycleService>>,
    agent_overview: OnceLock<Arc<crate::services::agent_cli::overview::OverviewService>>,
    asset_actors: Mutex<BTreeMap<String, String>>,
    revision: AtomicU64,
    load_error: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(mut data: AppData, load_error: Option<String>) -> Self {
        data.revision = 1;
        for provider in &mut data.providers {
            provider.revision = 1;
        }
        Self {
            data: RwLock::new(data),
            refresh_gate: tokio::sync::Mutex::new(()),
            mutation_gate: Mutex::new(()),
            agent_asset_access: Arc::default(),
            agent_asset_mutations: Arc::default(),
            agent_session_resumes: Arc::default(),
            agent_configuration: OnceLock::new(),
            agent_catalog: OnceLock::new(),
            agent_lifecycle: OnceLock::new(),
            agent_overview: OnceLock::new(),
            asset_actors: Mutex::default(),
            revision: AtomicU64::new(1),
            load_error: RwLock::new(load_error),
        }
    }

    pub fn load_error(&self) -> Option<String> {
        self.load_error
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// The managed library is isolated from provider configuration and exports.
    /// Construction does not adopt or mutate any native Agent configuration.
    pub(crate) fn agent_catalog(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Arc<CatalogService>, String> {
        let root = app
            .path()
            .app_config_dir()
            .map_err(|_| "无法获取应用资产库目录".to_owned())?
            .join("agent-asset-library");
        let managed_hook_root = app
            .path()
            .app_data_dir()
            .map_err(|_| "无法获取会话接入目录".to_owned())?;
        let display_cache_root = app
            .path()
            .app_cache_dir()
            .map_err(|_| "无法获取应用资产缓存目录".to_owned())?
            .join("agent-catalog");
        Ok(Arc::clone(self.agent_catalog.get_or_init(|| {
            Arc::new(
                CatalogService::new(root, Arc::clone(&self.agent_asset_mutations))
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
        let root = app.path().app_data_dir().map_err(|_| {
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
        let root = app
            .path()
            .app_data_dir()
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

    pub fn clear_load_error(&self) {
        *self
            .load_error
            .write()
            .unwrap_or_else(|err| err.into_inner()) = None;
    }

    pub fn next_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_are_monotonic_within_the_process() {
        let state = AppState::new(AppData::default());
        assert_eq!(state.next_revision(), 2);
        assert_eq!(state.next_revision(), 3);
        assert_eq!(state.current_revision(), 3);
    }

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
