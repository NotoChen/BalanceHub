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

#[derive(Debug, Clone, Copy)]
enum HttpClientProfile {
    Business,
    Webhook,
}

#[derive(Debug, Clone, Copy)]
pub enum ProxyKind {
    Http,
    Https,
    All,
}

/// 业务请求的超时与代理都来自设置：改设置后下一次取客户端即生效，
/// 缓存键里带上超时值，避免旧超时的客户端被继续复用。
pub fn build_provider_client_with_proxy(
    settings: &AppSettings,
    proxy: EffectiveProxy,
) -> Result<Client, String> {
    build_cached_client(
        HttpClientProfile::Business,
        proxy,
        settings.http_timeout_secs,
    )
}

pub async fn build_webhook_client(settings: &AppSettings) -> Result<Client, String> {
    let settings = settings.clone();
    crate::runtime::spawn_blocking(move || {
        build_cached_client(
            HttpClientProfile::Webhook,
            resolve_global_proxy(&settings),
            settings.http_timeout_secs,
        )
    })
    .await
    .map_err(|err| format!("初始化 Webhook 网络客户端任务异常: {err}"))?
}

pub fn build_download_client(settings: &AppSettings) -> Result<Client, String> {
    configure_reqwest_builder(
        Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(600))
            .redirect(Policy::limited(5)),
        &resolve_global_proxy(settings),
    )?
    .build()
    .map_err(|_| "初始化组件下载网络失败".to_string())
}

pub fn build_sync_client(settings: &AppSettings) -> Result<Client, String> {
    configure_reqwest_builder(
        Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirect(Policy::none()),
        &resolve_global_proxy(settings),
    )?
    .build()
    .map_err(|_| "无法创建同步网络客户端".to_owned())
}

pub fn build_mcp_client(settings: &AppSettings) -> Result<Client, String> {
    configure_reqwest_builder(
        Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .redirect(Policy::none()),
        &resolve_global_proxy(settings),
    )?
    .build()
    .map_err(|_| "无法创建 MCP 网络客户端".to_owned())
}

fn build_cached_client(
    profile: HttpClientProfile,
    proxy: EffectiveProxy,
    http_timeout_secs: u64,
) -> Result<Client, String> {
    let timeout_secs = match profile {
        HttpClientProfile::Business => http_timeout_secs,
        HttpClientProfile::Webhook => 10,
    };
    let cache_key = format!(
        "{profile:?}|{timeout_secs}|{:?}|{}|{}|{}|{}|{}",
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
            .timeout(Duration::from_secs(timeout_secs))
            .redirect(Policy::none()),
        HttpClientProfile::Webhook => Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
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

/// 更新器与业务客户端各自的 reqwest 版本不同，这里只给出与库无关的代理结论。
pub enum ProxyPlan {
    /// 明确不走代理。
    NoProxy,
    /// 继承系统与环境代理，交给 reqwest 的 system-proxy。
    Inherit,
    /// 显式代理规则；`no_proxy` 已合并。
    Rules {
        rules: Vec<(ProxyKind, String)>,
        no_proxy: String,
    },
}

pub fn proxy_plan(proxy: &EffectiveProxy) -> Result<ProxyPlan, String> {
    if matches!(proxy.mode, ProxyMode::NoProxy) {
        return Ok(ProxyPlan::NoProxy);
    }
    if proxy.inherit_environment {
        return Ok(ProxyPlan::Inherit);
    }
    Ok(ProxyPlan::Rules {
        rules: explicit_proxy_entries(proxy)?,
        no_proxy: merge_no_proxy(&proxy.no_proxy),
    })
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
