pub(crate) mod agent_cli;
pub mod agent_runtime;
pub mod app_updater;
pub mod cli_runtime;
pub mod cli_sessions;
pub mod liveness;
pub(crate) mod logging;
pub(crate) mod provider_app_import;
pub mod temporary_cli;
pub(crate) mod wsl;

// 网络域服务由 `balancehub-core` 实现；桌面命令和 Agent 功能通过这里
// 继续使用 `crate::services::provider_service` 等路径。
pub use balancehub_core::services::{
    browser_runtime, cli_paths, cloud_sync, oauth_accounts, provider_browser_login,
    provider_service, scheduler,
};

// 测活的纯规则在核心；桌面自己的 `liveness` 模块负责启动本地 Agent CLI。

pub(crate) use balancehub_core::services::workspaces;
