//! BalanceHub 的网络协议、数据模型与调度核心。
//!
//! 这里只放与窗口无关的能力：中转站协议、额度与用量、签到、OAuth/CPA、
//! 浏览器运行时、站点登录账号、测活纯规则、云同步引擎和本地数据存储。
//! Agent CLI、会话和临时 CLI 在 `balancehub-agent`；窗口、终端与系统集成
//! 由宿主通过 [`host::Host`] 注入数据目录、事件和系统能力。

pub mod adapters;
pub mod agent_cli_catalog;
pub mod api;
pub mod app_events;
pub mod contracts;
#[cfg(debug_assertions)]
pub mod dev_sample;
pub mod host;
pub mod limits;
pub mod models;
pub mod network;
pub mod platform;
pub mod provider_protocol_catalog;
pub mod quota_expression;
pub mod quota_history;
pub mod runtime;
pub mod services;
pub mod state;
pub mod storage;
pub mod terminal_catalog;
pub mod util;
