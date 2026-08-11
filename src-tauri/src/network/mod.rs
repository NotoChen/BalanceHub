#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod windows;

mod client;
mod proxy;
mod response;
pub(crate) mod shield;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub(crate) use client::{
    build_provider_client_with_proxy, build_webhook_client, configure_updater_builder,
};
use proxy::SystemProxyConfig;
pub(crate) use proxy::{apply_proxy_env, resolve_global_proxy, resolve_proxy, ProxyEnvironment};
pub(crate) use response::{read_http_text, read_webhook_text};
