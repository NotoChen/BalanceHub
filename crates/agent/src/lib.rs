//! BalanceHub Agent 能力：Agent CLI、会话、临时 CLI、WSL 与本机工具。
//!
//! 这里不依赖任何窗口框架。工具发现、配置读写、会话读取与资产库都直接作用
//! 于当前进程所在机器；窗口、系统打开与终端启动通过宿主扩展点注入，桌面和
//! 无界面后端安装各自的实现。

pub mod api;
mod host;
pub(crate) mod services;
pub(crate) mod state;
#[cfg(test)]
mod test_support;

// 与桌面 crate 相同的路径别名：迁入的模块继续使用 `crate::models::…`
// 这类路径，不必逐文件区分类型来自哪个 crate。
pub use balancehub_core::{
    agent_cli_catalog, app_events, contracts, host as core_host, limits, models, network, platform,
    storage, terminal_catalog, util,
};

pub use crate::host::{install_file_opener, FileOpener};
pub use crate::services::agent_cli::{normalize_base_url, provider_raw_base_url};

/// 托管 Hook 助手进程的入口：作为子进程启动时直接跑完并返回退出码。
pub fn hook_helper_exit_code() -> Option<i32> {
    crate::services::agent_runtime::managed_hook::helper_from_process_args()
}
/// 启动 Agent 后台服务：本机状态初始化、runtime 持久化、自动升级与清扫。
pub fn start_background() {
    crate::state::start_background();
}

/// 发送一个宿主事件。桌面宿主把它转成 Tauri 事件，服务端转成 WebSocket 帧。
pub fn emit(event: &str, payload: impl serde::Serialize) {
    crate::core_host::emit(
        event,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    );
}

/// 标记当前构建是否提供可见窗口能力（终端启动、系统文件打开等）。
pub fn has_desktop_session() -> bool {
    crate::host::desktop_session()
}

/// 启动前的本机状态初始化：原生历史库、CLI 发现缓存与升级记录。
pub fn initialize() -> Result<(), String> {
    let data_root = crate::core_host::data_dir()?;
    let config_root = crate::core_host::config_dir()?;
    crate::services::agent_cli::config_support::native_history::initialize(
        &data_root,
        &config_root,
    )?;
    crate::services::agent_cli::initialize_discovery_cache(data_root.join("agent-discovery-cache"));
    crate::services::agent_cli::lifecycle::initialize_journal(data_root.join("agent-upgrades"));
    Ok(())
}

/// 关闭一个窗口或浏览器会话时释放其会话读取配额与资产访问身份。
pub fn release_window(key: &str) {
    crate::services::cli_sessions::workbench::release_window(key);
    crate::state::clear_asset_actor(key);
}

/// 本机测活执行器：启动已安装的 Agent CLI。
pub fn liveness_executor(
) -> std::sync::Arc<dyn balancehub_core::services::liveness::LivenessExecutor> {
    std::sync::Arc::new(crate::services::liveness::LocalLivenessExecutor)
}

/// 共享资产库接入 WebDAV 同步的扩展点。
///
/// 只有资产库目录已经存在时才参与同步：刚从零启动的服务端不该用一份空的
/// 共享库去和桌面同步，否则第一次同步就会把桌面资产当成「被删除了」。
pub fn cloud_library(
) -> Option<std::sync::Arc<dyn balancehub_core::services::cloud_sync::library::CloudLibrary>> {
    let root = crate::core_host::config_dir()
        .ok()?
        .join("agent-asset-library");
    if !root.is_dir() {
        return None;
    }
    let catalog = crate::state::services().catalog().ok()?;
    Some(std::sync::Arc::new(CatalogCloudLibrary(catalog)))
}

struct CatalogCloudLibrary(std::sync::Arc<crate::services::agent_cli::catalog::CatalogService>);

impl balancehub_core::services::cloud_sync::library::CloudLibrary for CatalogCloudLibrary {
    fn validate(
        &self,
        documents: &balancehub_core::services::cloud_sync::SyncDocuments,
    ) -> Result<(), String> {
        self.0.validate_cloud_snapshot(documents)
    }

    fn snapshot(&self) -> Result<balancehub_core::services::cloud_sync::SyncDocuments, String> {
        self.0.cloud_snapshot()
    }

    fn apply(
        &self,
        expected: &balancehub_core::services::cloud_sync::SyncDocuments,
        desired: &balancehub_core::services::cloud_sync::SyncDocuments,
        commit: balancehub_core::services::cloud_sync::library::LibraryCommit<'_>,
    ) -> Result<Option<String>, String> {
        self.0
            .apply_cloud_snapshot(expected, desired, |before, after, publish| {
                commit(before, after, publish)
            })
    }
}
