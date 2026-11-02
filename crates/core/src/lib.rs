//! BalanceHub 的网络协议、数据模型与调度核心。
//!
//! 这里只放与窗口无关的能力：中转站协议、额度与用量、签到、OAuth/CPA、
//! 浏览器运行时、测活纯规则和本地数据存储。桌面专属能力（Agent 配置、
//! 终端启动、会话阅读、云同步引擎）留在 `src-tauri`，通过 [`host::Host`]
//! 把数据目录、事件和系统集成能力注入进来。

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
pub mod runtime;
pub mod services;
pub mod state;
pub mod storage;
pub mod terminal_catalog;
pub mod util;
