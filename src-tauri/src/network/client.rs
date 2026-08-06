use super::proxy::{merge_no_proxy, resolve_global_proxy, EffectiveProxy};
use crate::{
    limits,
    models::{AppSettings, ProxyMode},
};
use lru::LruCache;
use reqwest::{redirect::Policy, Client, ClientBuilder, NoProxy, Proxy};
use std::{
    num::NonZeroUsize,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri_plugin_updater::UpdaterBuilder;

#[derive(Debug, Clone, Copy)]
enum HttpClientProfile {
    Business,
    Webhook,
}

#[derive(Debug, Clone, Copy)]
enum ProxyKind {
    Http,
    Https,
    All,
}

pub(crate) fn build_provider_client_with_proxy(proxy: EffectiveProxy) -> Result<Client, String> {
    build_cached_client(HttpClientProfile::Business, proxy)
}

pub(crate) async fn build_webhook_client(settings: &AppSettings) -> Result<Client, String> {
    let settings = settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        build_cached_client(HttpClientProfile::Webhook, resolve_global_proxy(&settings))
    })
    .await
    .map_err(|err| format!("初始化 Webhook 网络客户端任务异常: {err}"))?
}

fn build_cached_client(
    profile: HttpClientProfile,
    proxy: EffectiveProxy,
) -> Result<Client, String> {
    let cache_key = format!(
        "{profile:?}|{:?}|{}|{}|{}|{}|{}",
        proxy.mode,
        proxy.http_url.trim(),
        proxy.https_url.trim(),
        proxy.all_url.trim(),
        proxy.no_proxy.trim(),
        proxy.inherit_environment,
    );
    if let Some(client) = client_cache()
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(&cache_key).cloned())
    {
        return Ok(client);
    }

    let mut builder = match profile {
        HttpClientProfile::Business => Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(Policy::none()),
        HttpClientProfile::Webhook => Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5)),
    };
    builder = configure_reqwest_builder(builder, &proxy)?;
    let client = builder
        .build()
        .map_err(|err| format!("初始化 HTTP 客户端失败: {err}"))?;
    if let Ok(mut cache) = client_cache().lock() {
        cache.put(cache_key, client.clone());
    }
    Ok(client)
}

fn client_cache() -> &'static Mutex<LruCache<String, Client>> {
    static CACHE: OnceLock<Mutex<LruCache<String, Client>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(limits::MAX_HTTP_CLIENT_CACHE_ENTRIES)
                .expect("HTTP client cache capacity must be non-zero"),
        ))
    })
}

fn configure_reqwest_builder(
    builder: ClientBuilder,
    proxy: &EffectiveProxy,
) -> Result<ClientBuilder, String> {
    if matches!(proxy.mode, ProxyMode::NoProxy) {
        return Ok(builder.no_proxy());
    }
    if proxy.inherit_environment {
        // reqwest 的 system-proxy feature 负责读取当前平台和代理环境变量。
        return Ok(builder);
    }

    let no_proxy = merge_no_proxy(&proxy.no_proxy);
    explicit_proxy_entries(proxy)?.into_iter().try_fold(
        builder.no_proxy(),
        |builder, (kind, url)| {
            let explicit = match kind {
                ProxyKind::Http => Proxy::http(&url),
                ProxyKind::Https => Proxy::https(&url),
                ProxyKind::All => Proxy::all(&url),
            }
            .map_err(|err| format!("代理地址无效({url}): {err}"))?
            .no_proxy(NoProxy::from_string(&no_proxy));
            Ok(builder.proxy(explicit))
        },
    )
}

pub(crate) fn configure_updater_builder(
    builder: UpdaterBuilder,
    proxy: &EffectiveProxy,
) -> Result<UpdaterBuilder, String> {
    if matches!(proxy.mode, ProxyMode::NoProxy) {
        return Ok(builder.no_proxy());
    }
    if proxy.inherit_environment {
        return Ok(builder);
    }

    let no_proxy = merge_no_proxy(&proxy.no_proxy);
    let mut explicit = Vec::new();
    for (kind, url) in explicit_proxy_entries(proxy)? {
        let rule = match kind {
            ProxyKind::Http => reqwest_updater::Proxy::http(&url),
            ProxyKind::Https => reqwest_updater::Proxy::https(&url),
            ProxyKind::All => reqwest_updater::Proxy::all(&url),
        }
        .map_err(|err| format!("代理地址无效({url}): {err}"))?
        .no_proxy(reqwest_updater::NoProxy::from_string(&no_proxy));
        explicit.push(rule);
    }

    Ok(builder.configure_client(move |client| {
        explicit
            .iter()
            .cloned()
            .fold(client.no_proxy(), |client, proxy| client.proxy(proxy))
    }))
}

fn explicit_proxy_entries(proxy: &EffectiveProxy) -> Result<Vec<(ProxyKind, String)>, String> {
    if matches!(proxy.mode, ProxyMode::Custom) && proxy.all_url.trim().is_empty() {
        return Err("自定义代理地址不能为空".to_string());
    }

    let mut entries = Vec::new();
    push_proxy_entry(&mut entries, ProxyKind::Http, &proxy.http_url);
    push_proxy_entry(&mut entries, ProxyKind::Https, &proxy.https_url);
    push_proxy_entry(&mut entries, ProxyKind::All, &proxy.all_url);
    Ok(entries)
}

fn push_proxy_entry(entries: &mut Vec<(ProxyKind, String)>, kind: ProxyKind, url: &str) {
    let url = url.trim();
    if !url.is_empty() {
        entries.push((kind, url.to_string()));
    }
}
