use crate::{
    models::{AppSettings, Provider, ProxyMode},
    network,
};
use reqwest::{Client, Proxy};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub(crate) const USER_AGENT_VALUE: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/131.0.0.0 Safari/537.36"
);

pub(crate) fn build_client(settings: &AppSettings, provider: &Provider) -> Result<Client, String> {
    let proxy = network::resolve_proxy(settings, provider);
    build_client_for_proxy(proxy.mode, &proxy.url)
}

pub(crate) fn build_client_for_proxy(
    proxy_mode: ProxyMode,
    proxy_url: &str,
) -> Result<Client, String> {
    let cache_key = format!("{proxy_mode:?}|{}", proxy_url.trim());
    if let Some(client) = client_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return Ok(client);
    }

    let client = build_client_uncached(proxy_mode, proxy_url)?;
    if let Ok(mut cache) = client_cache().lock() {
        cache.insert(cache_key, client.clone());
    }
    Ok(client)
}

fn client_cache() -> &'static Mutex<HashMap<String, Client>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn build_client_uncached(proxy_mode: ProxyMode, proxy_url: &str) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none());

    builder = match proxy_mode {
        ProxyMode::System => builder,
        ProxyMode::NoProxy => builder.no_proxy(),
        ProxyMode::Custom => {
            let proxy_url = proxy_url.trim();
            if proxy_url.is_empty() {
                return Err("自定义代理地址不能为空".to_string());
            }
            let proxy = Proxy::all(proxy_url).map_err(|err| format!("代理地址无效: {err}"))?;
            builder.proxy(proxy)
        }
    };

    builder
        .build()
        .map_err(|err| format!("初始化 HTTP 客户端失败: {err}"))
}
