use super::inventory::now_string;
use crate::{
    models::{
        AgentEnvironmentInventory, AgentInstallationChannel, AgentVersionCheckResult,
        AgentVersionSource, AgentVersionState,
    },
    services::agent_cli,
};
use chrono::{DateTime, Local};
use futures_util::{future::BoxFuture, FutureExt};
use std::{collections::BTreeMap, time::SystemTime};

const VERSION_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
const VERSION_FAILURE_BACKOFF_BASE: std::time::Duration = std::time::Duration::from_secs(30);
const VERSION_FAILURE_BACKOFF_MAX: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
struct VersionFailureEntry {
    message: String,
    failed_at: SystemTime,
    attempts: u32,
}

#[derive(Default)]
struct VersionCache {
    entries: BTreeMap<String, VersionCacheEntry>,
    failures: BTreeMap<String, VersionFailureEntry>,
    in_flight: BTreeMap<String, InFlightVersion>,
    next_request_id: u64,
}

fn version_cache() -> &'static std::sync::Mutex<VersionCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<VersionCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(VersionCache::default()))
}

pub(crate) async fn check_latest_versions(
    settings: &crate::models::AppSettings,
    mut inventory: AgentEnvironmentInventory,
) -> Result<AgentVersionCheckResult, String> {
    let proxy = crate::network::resolve_global_proxy(settings);
    let client = match crate::network::build_provider_client_with_proxy(proxy) {
        Ok(client) => client,
        Err(error) => {
            for installation in &mut inventory.installations {
                let package = agent_cli::definition(installation.agent_kind)
                    .environment()
                    .package_name();
                apply_version_failure(installation, package, &error);
            }
            return Ok(AgentVersionCheckResult {
                installations: inventory.installations,
                checked_at: now_string(),
            });
        }
    };
    let mut checked_at = None;
    let checks = inventory
        .installations
        .iter()
        .enumerate()
        .map(|(index, installation)| {
            let package = agent_cli::definition(installation.agent_kind)
                .environment()
                .package_name();
            let client = client.clone();
            async move { (index, latest_for_package(&client, package).await) }
        });
    for (index, result) in futures_util::future::join_all(checks).await {
        let installation = &mut inventory.installations[index];
        let package = agent_cli::definition(installation.agent_kind)
            .environment()
            .package_name();
        match result {
            Ok((version, checked)) => {
                installation.latest_stable_version = Some(version.clone());
                installation.latest_version_source = AgentVersionSource::NpmRegistry;
                installation.version_checked_at = Some(format_system_time(checked));
                installation.version_state =
                    version_state(installation.installed_version.as_deref(), Some(&version));
                checked_at =
                    Some(checked_at.map_or(checked, |current: SystemTime| current.max(checked)));
            }
            Err(error) => {
                apply_version_failure(installation, package, &error);
            }
        }
    }
    Ok(AgentVersionCheckResult {
        installations: inventory.installations,
        checked_at: checked_at
            .map(format_system_time)
            .unwrap_or_else(now_string),
    })
}

async fn latest_for_package(
    client: &reqwest::Client,
    package: &str,
) -> Result<(String, SystemTime), String> {
    if let Some(cached) = cached_version(package) {
        return Ok((cached.version, cached.checked_at));
    }
    let (request_id, future) = {
        let mut cache = version_cache()
            .lock()
            .map_err(|_| "Agent 版本缓存锁已损坏".to_string())?;
        if let Some(failure) = active_failure(&cache, package) {
            return Err(failure.message);
        }
        if let Some(in_flight) = cache.in_flight.get(package) {
            (in_flight.id, in_flight.future.clone())
        } else {
            cache.next_request_id = cache.next_request_id.wrapping_add(1);
            let request_id = cache.next_request_id;
            let request_client = client.clone();
            let request_package = package.to_string();
            let future =
                async move { fetch_latest_package(&request_client, &request_package).await }
                    .boxed()
                    .shared();
            cache.in_flight.insert(
                package.to_string(),
                InFlightVersion {
                    id: request_id,
                    future: future.clone(),
                },
            );
            (request_id, future)
        }
    };
    let result = future.await;
    if let Ok(mut cache) = version_cache().lock() {
        let owns_completion = cache
            .in_flight
            .get(package)
            .is_some_and(|current| current.id == request_id);
        if owns_completion {
            match &result {
                Ok(entry) => {
                    cache.entries.insert(package.to_string(), entry.clone());
                    cache.failures.remove(package);
                }
                Err(message) => {
                    let attempts = cache
                        .failures
                        .get(package)
                        .map_or(1, |failure| failure.attempts.saturating_add(1));
                    cache.failures.insert(
                        package.to_string(),
                        VersionFailureEntry {
                            message: message.clone(),
                            failed_at: SystemTime::now(),
                            attempts,
                        },
                    );
                }
            }
            cache.in_flight.remove(package);
        }
    }
    result.map(|entry| (entry.version, entry.checked_at))
}

fn active_failure(cache: &VersionCache, package: &str) -> Option<VersionFailureEntry> {
    let failure = cache.failures.get(package)?.clone();
    let elapsed = failure.failed_at.elapsed().ok()?;
    (elapsed < failure_backoff(failure.attempts)).then_some(failure)
}

pub(super) fn failure_backoff(attempts: u32) -> std::time::Duration {
    let multiplier = 1_u32 << attempts.saturating_sub(1).min(6);
    VERSION_FAILURE_BACKOFF_BASE
        .saturating_mul(multiplier)
        .min(VERSION_FAILURE_BACKOFF_MAX)
}

fn apply_version_failure(
    installation: &mut crate::models::AgentInstallation,
    package: &str,
    error: &str,
) {
    if let Some(cached) = last_known_version(package) {
        installation.latest_stable_version = Some(cached.version);
        installation.latest_version_source = AgentVersionSource::NpmRegistry;
        installation.version_checked_at = Some(format_system_time(cached.checked_at));
        installation.version_state = version_state(
            installation.installed_version.as_deref(),
            installation.latest_stable_version.as_deref(),
        );
    } else {
        installation.latest_version_source = AgentVersionSource::Unknown;
        installation.version_state = AgentVersionState::Unavailable;
    }
    installation.diagnostic = Some(match installation.diagnostic.take() {
        Some(existing) => format!("{existing}；{error}"),
        None => error.to_string(),
    });
}

async fn fetch_latest_package(
    client: &reqwest::Client,
    package: &str,
) -> Result<VersionCacheEntry, String> {
    let encoded_package = package.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{encoded_package}");
    let payload = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("读取 npm 版本失败: {error}"))?
        .error_for_status()
        .map_err(|error| format!("npm 版本响应失败: {error}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("解析 npm 版本响应失败: {error}"))?;
    let version = latest_stable_version(&payload)?;
    let checked_at = SystemTime::now();
    Ok(VersionCacheEntry {
        version,
        checked_at,
    })
}

fn cached_version(package: &str) -> Option<VersionCacheEntry> {
    let cache = version_cache().lock().ok()?;
    let entry = cache.entries.get(package)?.clone();
    if entry.checked_at.elapsed().ok()? <= VERSION_CACHE_TTL {
        Some(entry)
    } else {
        // Expired entries remain available as a last-known-good fallback, but are not returned
        // by latest_for_package because a new check should be attempted first.
        None
    }
}

fn last_known_version(package: &str) -> Option<VersionCacheEntry> {
    version_cache().lock().ok()?.entries.get(package).cloned()
}

pub(super) fn latest_stable_version(payload: &serde_json::Value) -> Result<String, String> {
    let value = payload
        .get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| is_stable_version(value))
        .ok_or_else(|| "npm 响应缺少稳定版 latest".to_string())?;
    Ok(value.to_string())
}

fn is_stable_version(value: &str) -> bool {
    parse_version(value).is_some_and(|version| version.prerelease.is_none())
}

pub(super) fn version_state(
    installed: Option<&str>,
    latest: Option<&str>,
) -> crate::models::AgentVersionState {
    let (Some(installed), Some(latest)) =
        (installed.filter(|value| !value.trim().is_empty()), latest)
    else {
        return crate::models::AgentVersionState::Unknown;
    };
    let (Some(installed), Some(latest)) = (parse_version(installed), parse_version(latest)) else {
        return AgentVersionState::Unknown;
    };
    match installed.cmp(&latest) {
        std::cmp::Ordering::Equal => crate::models::AgentVersionState::UpToDate,
        std::cmp::Ordering::Less => crate::models::AgentVersionState::UpdateAvailable,
        std::cmp::Ordering::Greater => crate::models::AgentVersionState::AheadOfStable,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    core: [u64; 3],
    prerelease: Option<String>,
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (Some(left), Some(right)) => compare_prerelease(left, right),
            })
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version(value: &str) -> Option<ParsedVersion> {
    let value = value.trim();
    let start = value
        .char_indices()
        .find_map(|(index, character)| character.is_ascii_digit().then_some(index))?;
    let candidate = &value[start..];
    let token = candidate
        .split_whitespace()
        .next()?
        .trim_end_matches([',', ')', ']']);
    let without_build = token.split_once('+').map_or(token, |(core, _)| core);
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, pre)| (core, Some(pre)));
    let mut parts = core.split('.');
    let parsed = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    if parts.next().is_some()
        || prerelease.is_some_and(|value| {
            value.is_empty()
                || !value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
                })
        })
    {
        return None;
    }
    Some(ParsedVersion {
        core: parsed,
        prerelease: prerelease.map(str::to_ascii_lowercase),
    })
}

fn compare_prerelease(left: &str, right: &str) -> std::cmp::Ordering {
    let left = left.split('.').collect::<Vec<_>>();
    let right = right.split('.').collect::<Vec<_>>();
    for index in 0..left.len().max(right.len()) {
        let Some(left) = left.get(index) else {
            return std::cmp::Ordering::Less;
        };
        let Some(right) = right.get(index) else {
            return std::cmp::Ordering::Greater;
        };
        let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right),
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

pub(super) fn version_channel(value: &str) -> AgentInstallationChannel {
    let Some(version) = parse_version(value) else {
        return AgentInstallationChannel::Unknown;
    };
    match version.prerelease.as_deref() {
        None => AgentInstallationChannel::Stable,
        Some(value) if value.contains("nightly") => AgentInstallationChannel::Nightly,
        Some(_) => AgentInstallationChannel::Preview,
    }
}

fn format_system_time(value: SystemTime) -> String {
    DateTime::<Local>::from(value).to_rfc3339()
}
