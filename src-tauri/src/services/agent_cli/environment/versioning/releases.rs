//! Shared release facts. Re-scanning local installations never discards a checked release.
use super::{latest_stable_version, version_state};
use crate::models::{
    AgentLifecycleVersion, AgentLifecycleVersionRefresh, AgentLifecycleVersionSource,
    AgentLifecycleVersionState, AppSettings,
};
use chrono::{DateTime, Local};
use futures_util::{future::BoxFuture, FutureExt};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

const VERSION_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const VERSION_FAILURE_BACKOFF_BASE: Duration = Duration::from_secs(30);
const VERSION_FAILURE_BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum ReleaseSource {
    Homebrew { package: String, cask: bool },
    Npm(String),
    Vendor(String),
}

impl ReleaseSource {
    pub(crate) fn kind(&self) -> AgentLifecycleVersionSource {
        match self {
            Self::Homebrew { .. } => AgentLifecycleVersionSource::Homebrew,
            Self::Npm(_) => AgentLifecycleVersionSource::NpmRegistry,
            Self::Vendor(_) => AgentLifecycleVersionSource::VendorRelease,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct VersionCacheEntry {
    version: String,
    checked_at: SystemTime,
}

type SharedVersionFuture =
    futures_util::future::Shared<BoxFuture<'static, Result<VersionCacheEntry, String>>>;

struct InFlightVersion {
    id: u64,
    future: SharedVersionFuture,
}

#[derive(Clone, Serialize, Deserialize)]
struct VersionFailureEntry {
    message: String,
    failed_at: SystemTime,
    attempts: u32,
}

#[derive(Default)]
struct VersionCache {
    entries: BTreeMap<ReleaseSource, VersionCacheEntry>,
    failures: BTreeMap<ReleaseSource, VersionFailureEntry>,
    in_flight: BTreeMap<ReleaseSource, InFlightVersion>,
    next_request_id: u64,
}

static CACHE_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Default, Serialize, Deserialize)]
struct SavedVersions {
    entries: Vec<(ReleaseSource, VersionCacheEntry)>,
    failures: Vec<(ReleaseSource, VersionFailureEntry)>,
}

pub(crate) fn initialize(root: PathBuf) {
    let path = root.join("releases.json");
    if CACHE_PATH.set(path.clone()).is_err() {
        return;
    }
    if let Some(saved) = crate::services::agent_cli::cache::read::<SavedVersions>(&path) {
        let mut cache = version_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.entries.extend(saved.entries);
        cache.failures.extend(saved.failures);
    }
}

fn persist(cache: &VersionCache) {
    if let Some(path) = CACHE_PATH.get() {
        let saved = SavedVersions {
            entries: cache
                .entries
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            failures: cache
                .failures
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        };
        if let Err(error) = crate::services::agent_cli::cache::write(path, &saved) {
            eprintln!("Unable to persist release cache: {error}");
        }
    }
}

fn version_cache() -> &'static Mutex<VersionCache> {
    static CACHE: OnceLock<Mutex<VersionCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VersionCache::default()))
}

pub(crate) async fn check(
    settings: &AppSettings,
    source: &ReleaseSource,
    installed: Option<&str>,
    refresh: AgentLifecycleVersionRefresh,
) -> AgentLifecycleVersion {
    let (request_id, future) = {
        let mut cache = version_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let snapshot = snapshot(&cache, source, installed);
        let due = next_check(&cache, source) <= SystemTime::now();
        if refresh == AgentLifecycleVersionRefresh::Cached
            || (refresh == AgentLifecycleVersionRefresh::IfStale && !due)
        {
            return snapshot;
        }
        if let Some(in_flight) = cache.in_flight.get(source) {
            (in_flight.id, in_flight.future.clone())
        } else {
            cache.next_request_id = cache.next_request_id.wrapping_add(1);
            let request_id = cache.next_request_id;
            let settings = settings.clone();
            let request_source = source.clone();
            // The timeout belongs to the shared request, including response-body reads.
            let future = async move { fetch(&settings, &request_source).await }
                .boxed()
                .shared();
            cache.in_flight.insert(
                source.clone(),
                InFlightVersion {
                    id: request_id,
                    future: future.clone(),
                },
            );
            (request_id, future)
        }
    };
    let result = future.await;
    let mut cache = version_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if cache
        .in_flight
        .get(source)
        .is_some_and(|current| current.id == request_id)
    {
        match result {
            Ok(entry) => {
                cache.entries.insert(source.clone(), entry);
                cache.failures.remove(source);
            }
            Err(message) => {
                let attempts = cache
                    .failures
                    .get(source)
                    .map_or(1, |failure| failure.attempts.saturating_add(1));
                cache.failures.insert(
                    source.clone(),
                    VersionFailureEntry {
                        message,
                        failed_at: SystemTime::now(),
                        attempts,
                    },
                );
            }
        }
        cache.in_flight.remove(source);
        persist(&cache);
    }
    snapshot(&cache, source, installed)
}

pub(in crate::services::agent_cli::environment) fn failure_backoff(attempts: u32) -> Duration {
    let multiplier = 1_u32 << attempts.saturating_sub(1).min(6);
    VERSION_FAILURE_BACKOFF_BASE
        .saturating_mul(multiplier)
        .min(VERSION_FAILURE_BACKOFF_MAX)
}

fn next_check(cache: &VersionCache, source: &ReleaseSource) -> SystemTime {
    if let Some(failure) = cache.failures.get(source) {
        failure.failed_at + failure_backoff(failure.attempts)
    } else if let Some(entry) = cache.entries.get(source) {
        entry.checked_at + VERSION_CACHE_TTL
    } else {
        SystemTime::now()
    }
}

fn snapshot(
    cache: &VersionCache,
    source: &ReleaseSource,
    installed: Option<&str>,
) -> AgentLifecycleVersion {
    let entry = cache.entries.get(source);
    let failure = cache.failures.get(source);
    let next = next_check(cache, source);
    let stale = failure.is_some() || entry.is_none() || next <= SystemTime::now();
    AgentLifecycleVersion {
        state: if failure.is_some() {
            AgentLifecycleVersionState::CheckFailed
        } else if let Some(entry) = entry {
            version_state(installed, Some(&entry.version))
        } else {
            AgentLifecycleVersionState::NotChecked
        },
        source: source.kind(),
        latest_version: entry.map(|entry| entry.version.clone()),
        checked_at: failure
            .map(|failure| failure.failed_at)
            .or_else(|| entry.map(|entry| entry.checked_at))
            .map(format_time),
        last_success_at: entry.map(|entry| format_time(entry.checked_at)),
        next_check_at: Some(format_time(next)),
        stale,
        message: failure
            .map(|failure| {
                if entry.is_some() {
                    format!(
                        "{}；已保留上次成功结果，可点击检查更新重试",
                        failure.message
                    )
                } else {
                    format!("{}，可点击检查更新重试", failure.message)
                }
            })
            .or_else(|| {
                (stale && entry.is_some()).then(|| "上次检查结果已过期，等待重新检查".to_owned())
            }),
    }
}

fn format_time(time: SystemTime) -> String {
    DateTime::<Local>::from(time).to_rfc3339()
}

async fn fetch(
    settings: &AppSettings,
    source: &ReleaseSource,
) -> Result<VersionCacheEntry, String> {
    let client = crate::network::build_provider_client_with_proxy(
        crate::network::resolve_global_proxy(settings),
    )
    .map_err(|_| "无法创建版本检查连接".to_owned())?;
    let version = match source {
        ReleaseSource::Homebrew { package, cask } => {
            let kind = if *cask { "cask" } else { "formula" };
            let url = format!("https://formulae.brew.sh/api/{kind}/{package}.json");
            let bytes = fetch_bounded(&client, &url, 1024 * 1024).await?;
            let document: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| "无法解析 Homebrew 版本信息".to_owned())?;
            if document
                .get(if *cask { "token" } else { "name" })
                .and_then(|value| value.as_str())
                != Some(package.as_str())
            {
                return Err("Homebrew 包身份与请求不一致".to_owned());
            }
            let value = if *cask {
                document.get("version")
            } else {
                document.pointer("/versions/stable")
            }
            .and_then(|value| value.as_str())
            .ok_or("Homebrew 未提供稳定版本")?;
            semver::Version::parse(value)
                .map_err(|_| "Homebrew 版本格式暂不支持自动升级".to_owned())?;
            value.to_owned()
        }

        ReleaseSource::Npm(package) => {
            // Fetch only the latest manifest, not the package's entire release history.
            let url = format!(
                "https://registry.npmjs.org/{}/latest",
                package.replace('/', "%2F")
            );
            let bytes = fetch_bounded(&client, &url, 512 * 1024).await?;
            let document: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "无法解析 npm 版本响应".to_owned())?;
            if document.get("name").and_then(|value| value.as_str()) != Some(package.as_str()) {
                return Err("npm 版本响应与请求的 Agent 不一致".to_owned());
            }
            latest_stable_version(&document)?
        }
        ReleaseSource::Vendor(url) => {
            let bytes = fetch_bounded(&client, url, 256).await?;
            let version = std::str::from_utf8(&bytes)
                .map_err(|_| "无法解析官方版本响应".to_owned())?
                .trim();
            semver::Version::parse(version).map_err(|_| "官方响应缺少有效版本号".to_owned())?;
            version.to_owned()
        }
    };
    Ok(VersionCacheEntry {
        version,
        checked_at: SystemTime::now(),
    })
}

pub(crate) async fn fetch_bounded(
    client: &reqwest::Client,
    url: &str,
    cap: usize,
) -> Result<Vec<u8>, String> {
    tokio::time::timeout(REQUEST_TIMEOUT, async {
        let mut response = client
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send()
            .await
            .map_err(|_| "读取官方版本信息失败".to_owned())?
            .error_for_status()
            .map_err(|_| "官方版本服务响应失败".to_owned())?;
        if response
            .content_length()
            .is_some_and(|length| length > cap as u64)
        {
            return Err("官方版本响应超出大小限制".to_owned());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "读取官方版本响应失败".to_owned())?
        {
            if bytes.len().saturating_add(chunk.len()) > cap {
                return Err("官方版本响应超出大小限制".to_owned());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| "版本检查超时".to_owned())?
}
