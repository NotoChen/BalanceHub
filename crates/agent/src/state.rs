//! Agent 服务的进程级注册表与访问身份。
//!
//! 服务本身不依赖窗口；数据目录、缓存目录与共享库根目录都来自宿主。
//! 访问身份（actor）区分「哪个窗口或哪个浏览器会话」发起的资产访问，
//! 桌面用窗口标签，服务端用固定的浏览器身份。

use crate::{
    services::agent_cli::{
        catalog::CatalogService,
        configuration::ConfigurationService,
        environment::{access_registry::AgentAssetAccessRegistry, mutation::MutationService},
        lifecycle::LifecycleService,
        maintenance::MaintenanceService,
        mcp_check::McpCheckService,
        overview::OverviewService,
    },
    services::cli_sessions::resume::AgentSessionResumeService,
};
use balancehub_core::models::AppData;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

/// 核心配置状态；Agent 服务与中转站服务共享同一份 `AppData`。
pub fn core_state() -> Arc<balancehub_core::state::CoreState> {
    balancehub_core::host::core_state()
}

pub fn app_data() -> AppData {
    core_state()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub struct AgentServices {
    pub asset_access: Arc<AgentAssetAccessRegistry>,
    pub asset_mutations: Arc<MutationService>,
    pub session_resumes: Arc<AgentSessionResumeService>,
    pub mcp_checks: Arc<McpCheckService>,
    pub maintenance: Arc<MaintenanceService>,
    configuration: OnceLock<Arc<ConfigurationService>>,
    catalog: OnceLock<Arc<CatalogService>>,
    lifecycle: OnceLock<Arc<LifecycleService>>,
    overview: OnceLock<Arc<OverviewService>>,
    actors: Mutex<BTreeMap<String, String>>,
}

impl Default for AgentServices {
    fn default() -> Self {
        Self {
            asset_access: Arc::default(),
            asset_mutations: Arc::default(),
            session_resumes: Arc::default(),
            mcp_checks: Arc::default(),
            maintenance: Arc::default(),
            configuration: OnceLock::new(),
            catalog: OnceLock::new(),
            lifecycle: OnceLock::new(),
            overview: OnceLock::new(),
            actors: Mutex::default(),
        }
    }
}

static SERVICES: OnceLock<AgentServices> = OnceLock::new();

pub fn services() -> &'static AgentServices {
    SERVICES.get_or_init(AgentServices::default)
}

fn core_dir(dir: Result<PathBuf, String>, label: &str) -> Result<PathBuf, String> {
    dir.map_err(|error| format!("无法获取{label}: {error}"))
}

impl AgentServices {
    /// 共享资产库：配置目录下的 `agent-asset-library`。
    pub fn catalog(&self) -> Result<Arc<CatalogService>, String> {
        if let Some(service) = self.catalog.get() {
            return Ok(Arc::clone(service));
        }
        let root = core_dir(balancehub_core::host::config_dir(), "应用资产库目录")?
            .join("agent-asset-library");
        let managed_hook_root = core_dir(balancehub_core::host::data_dir(), "会话接入目录")?;
        let display_cache_root =
            core_dir(balancehub_core::host::cache_dir(), "应用资产缓存目录")?.join("agent-catalog");
        Ok(Arc::clone(self.catalog.get_or_init(|| {
            Arc::new(
                CatalogService::new(root, Arc::clone(&self.asset_mutations))
                    .with_sync_signal(Arc::clone(balancehub_core::services::cloud_sync::signal()))
                    .with_managed_hook_root(managed_hook_root)
                    .with_display_cache(display_cache_root),
            )
        })))
    }

    pub fn configuration(
        &self,
    ) -> Result<Arc<ConfigurationService>, balancehub_core::models::AgentConfigurationError> {
        use balancehub_core::models::{AgentConfigurationError, AgentConfigurationErrorKind};
        if let Some(service) = self.configuration.get() {
            return Ok(Arc::clone(service));
        }
        let home = balancehub_core::services::cli_paths::user_home().ok_or_else(|| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::SourceUnavailable)
        })?;
        let root = balancehub_core::host::data_dir().map_err(|_| {
            AgentConfigurationError::new(AgentConfigurationErrorKind::InternalFailure)
        })?;
        Ok(Arc::clone(self.configuration.get_or_init(|| {
            Arc::new(ConfigurationService::new(
                home,
                root,
                Arc::clone(&self.asset_access),
            ))
        })))
    }

    pub fn lifecycle(&self) -> Arc<LifecycleService> {
        Arc::clone(
            self.lifecycle.get_or_init(|| {
                Arc::new(LifecycleService::new(self.asset_mutations.domain_locks()))
            }),
        )
    }

    pub fn overview(&self) -> Result<Arc<OverviewService>, String> {
        if let Some(service) = self.overview.get() {
            return Ok(Arc::clone(service));
        }
        let root =
            core_dir(balancehub_core::host::data_dir(), "Agent 摘要目录")?.join("agent-overview");
        Ok(Arc::clone(
            self.overview
                .get_or_init(|| Arc::new(OverviewService::new(root))),
        ))
    }
}

/// 访问身份由宿主提供（窗口标签或浏览器会话），新生命周期得到新的 actor。
pub fn asset_actor(key: &str) -> Result<String, ()> {
    let services = services();
    let mut actors = services.actors.lock().map_err(|_| ())?;
    if let Some(actor) = actors.get(key) {
        return Ok(actor.clone());
    }
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| ())?;
    let actor = format!("{key}:{:032x}", u128::from_ne_bytes(nonce));
    actors.insert(key.to_owned(), actor.clone());
    Ok(actor)
}

pub fn clear_asset_actor(key: &str) {
    let services = services();
    let actor = services
        .actors
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(key);
    if let Some(actor) = actor {
        services.mcp_checks.cancel_actor(&actor);
        services.maintenance.remove_actor(&actor);
        services.asset_access.remove_actor(&actor);
        if let Some(configuration) = services.configuration.get() {
            configuration.remove_actor(&actor);
        }
        services.asset_mutations.remove_actor(&actor);
        if let Some(catalog) = services.catalog.get() {
            catalog.remove_actor(&actor);
        }
        if let Some(lifecycle) = services.lifecycle.get() {
            lifecycle.remove_actor(&actor);
        }
    }
}

/// 启动后台服务：Agent runtime 持久化、自动升级策略、历史临时目录清扫。
///
/// 初始化失败只记录日志，不阻止进程继续提供其他能力。
pub fn start_background() {
    if let Err(error) = crate::initialize() {
        log::warn!("Agent 本机状态初始化未完成：{error}");
    }
    crate::services::agent_runtime::service::start();
    crate::services::agent_cli::lifecycle::start_worker();
    std::thread::spawn(crate::services::temporary_cli::cleanup_stale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_actor_is_bound_to_one_window_lifetime() {
        let main = asset_actor("main").unwrap();
        let other = asset_actor("other").unwrap();
        assert_eq!(main, asset_actor("main").unwrap());
        assert_ne!(main, other);
        clear_asset_actor("main");
        assert_ne!(main, asset_actor("main").unwrap());
        assert_eq!(other, asset_actor("other").unwrap());
    }
}
