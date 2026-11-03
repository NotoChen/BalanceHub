//! Agent 域服务：Agent CLI、运行时、会话、临时 CLI、WSL 与测活。
//!
//! 这里同时转发核心的网络域服务，使迁入的模块继续使用
//! `crate::services::provider_service::…` 这类既有路径。

pub mod agent_cli;
pub mod agent_runtime;
pub mod cli_runtime;
pub mod cli_sessions;
pub mod liveness;
pub mod temporary_cli;
pub mod wsl;

pub use balancehub_core::services::{
    cli_paths, cloud_sync, oauth_accounts, provider_service, workspaces,
};
