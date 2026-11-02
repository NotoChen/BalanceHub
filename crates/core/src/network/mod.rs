#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod windows;

mod client;
mod proxy;
mod response;
pub mod shield;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub use client::{
    build_download_client, build_mcp_client, build_provider_client_with_proxy, build_sync_client,
    build_webhook_client, proxy_plan, ProxyKind, ProxyPlan,
};
use proxy::SystemProxyConfig;
pub use proxy::{
    apply_proxy_env, resolve_global_proxy, resolve_proxy, EffectiveProxy, ProxyEnvironment,
};
pub use response::{read_http_text, read_webhook_text};
