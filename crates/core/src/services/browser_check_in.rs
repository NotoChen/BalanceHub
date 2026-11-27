use super::{
    browser_profiles::{self, ProfileKey},
    browser_runtime,
    check_in_tasks::CheckInContext,
    provider_service::ProviderRequestContext,
};
use crate::{
    adapters::{
        browser::BrowserSession, new_api::check_in_with_browser,
        protocol::contracts::ProviderOperationOutcome,
    },
    models::{
        AppSettings, CheckInError, CheckInPhase, CheckInVerificationRequest, Provider,
        ProviderCheckInResult,
    },
    network,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

/// 浏览器会话目录按站点、账号、代理与防护设置分桶。改过其中任何一项，
/// 旧桶都不会再被打开；只清理超过保留期且不在当前配置里的孤儿目录。
const PROFILE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
static LAST_PRUNE: AtomicU64 = AtomicU64::new(0);

pub async fn run(
    settings: &AppSettings,
    provider: &Provider,
    verification: CheckInVerificationRequest,
    task: &CheckInContext,
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    task.queued_for_browser();
    schedule_profile_prune();
    // 登录目录的孤儿清理与签到桶共用每日节拍；账号管理不常打开的宿主
    // 也能靠签到顺手清掉已删除账号的浏览器环境。
    crate::services::login_profiles::schedule_orphan_prune();
    // Queue time is not counted as browser execution time. The enclosing task
    // owns cancellation, including while this permit is pending.
    let _profile =
        browser_profiles::acquire(ProfileKey::CheckIn(provider.identity.id.clone())).await?;
    let runtime = browser_runtime::acquire()
        .await
        .map_err(CheckInError::WaitingBrowser)?;
    task.phase(CheckInPhase::Opening);
    let progress_task = task.clone();
    let mut session = BrowserSession::spawn(
        &runtime.directory,
        Arc::new(move |phase| {
            progress_task.phase(phase);
        }),
    )
    .map_err(CheckInError::WaitingBrowser)?;
    // 无界面宿主没有人能看窗口；不注册窗口控制，界面也不会给出“显示窗口”入口。
    let window = if crate::host::is_headless() {
        None
    } else {
        Some(task.attach_window(session.window_control())?)
    };
    let outcome = tokio::time::timeout(
        Duration::from_secs(480),
        execute(
            settings,
            provider,
            verification,
            task,
            &runtime.browser,
            &mut session,
        ),
    )
    .await
    .unwrap_or_else(|_| {
        Err(CheckInError::Unconfirmed(
            "签到验证超时，站点结果尚未确认；已停止自动重试，请查看站点记录".to_string(),
        ))
    });
    session.close().await;
    drop(window);
    // The task registry marks completion only after ProviderService persists.
    outcome
}

async fn execute(
    settings: &AppSettings,
    provider: &Provider,
    verification: CheckInVerificationRequest,
    task: &CheckInContext,
    executable: &std::path::Path,
    session: &mut BrowserSession,
) -> Result<ProviderOperationOutcome<ProviderCheckInResult>, CheckInError> {
    let proxy = network::resolve_proxy(settings, provider);
    let proxy_key = proxy.fingerprint();
    let context = ProviderRequestContext::capture(provider);
    let ensure_current = || {
        let state = crate::host::core_state();
        let data = state.data.read().map_err(|_| "读取当前账号状态失败")?;
        let current = data
            .providers
            .iter()
            .find(|item| context.matches_check_in(item))
            .filter(|item| item.runtime.enabled)
            .ok_or("账号配置已变更，已停止本次签到")?;
        if network::resolve_proxy(&data.settings, current).fingerprint() != proxy_key {
            return Err("代理配置已变更，请重新签到".to_string());
        }
        Ok(())
    };
    ensure_current()?;
    let url = status_url(provider)?;
    let profile_dir = crate::host::cache_dir()
        .map_err(|_| "无法获取浏览器会话目录")?
        .join("checkin-browser")
        .join(profile_key(provider, &url, &proxy_key));
    let cookies = crate::adapters::new_api::browser_cookie_header(provider)
        .split(';')
        .filter_map(|item| {
            let (name, value) = item.trim().split_once('=')?;
            Some(json!({ "name": name.trim(), "value": value.trim() }))
        })
        .collect::<Vec<_>>();
    session
        .request(
            "open",
            json!({
                "url": url.as_str(), "profileDir": profile_dir, "proxy": proxy.browser(&url)?,
                "providerName": provider.display_label(),
                "cookies": cookies, "executablePath": executable, "interactive": task.interactive,
                "autoShield": provider.automation.auto_shield,
                "headless": crate::host::is_headless(),
            }),
        )
        .await?;
    check_in_with_browser(session, provider, verification, &ensure_current).await
}

fn status_url(provider: &Provider) -> Result<reqwest::Url, String> {
    let base_url = provider.identity.base_url.trim_end_matches('/');
    reqwest::Url::parse(&format!("{base_url}/api/status")).map_err(|_| "站点地址无效".to_string())
}

/// 浏览器会话桶只由这些配置决定；任何一项变化都会让旧桶失去复用价值。
fn profile_key(provider: &Provider, url: &reqwest::Url, proxy_key: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}|{}|{}|{:?}|{}|{}",
                provider.identity.id,
                url.origin().ascii_serialization(),
                provider.auth.api_user,
                provider.auth.mode,
                proxy_key,
                provider.automation.auto_shield
            )
            .as_bytes()
        )
    )
}

/// 浏览器签到启动或后台每日维护时顺手清理一次（每天最多一次）。只删除超过
/// 保留期且不在当前配置里的桶；仍在使用的桶即使超过保留期也保留。配置读取
/// 失败时不猜哪些桶还有用，直接跳过并留到下一轮。
pub(crate) fn schedule_profile_prune() {
    let now = crate::util::unix_millis() as u64;
    let last = LAST_PRUNE.load(Ordering::Relaxed);
    if now.saturating_sub(last) < PRUNE_INTERVAL.as_millis() as u64 {
        return;
    }
    let state = crate::host::core_state();
    if state.load_error().is_some() {
        return;
    }
    let held: HashSet<String> = match state.data.read() {
        Ok(data) => data
            .providers
            .iter()
            .filter_map(|provider| {
                let url = status_url(provider).ok()?;
                let proxy = network::resolve_proxy(&data.settings, provider);
                Some(profile_key(provider, &url, &proxy.fingerprint()))
            })
            .collect(),
        Err(_) => return,
    };
    let Ok(root) = crate::host::cache_dir().map(|directory| directory.join("checkin-browser"))
    else {
        return;
    };
    if LAST_PRUNE
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let cutoff = SystemTime::now()
        .checked_sub(PROFILE_RETENTION)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    crate::runtime::spawn_blocking(move || prune_profiles(&root, &held, cutoff));
}

fn prune_profiles(root: &Path, held: &HashSet<String>, cutoff: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if held.contains(name) {
            continue;
        }
        // `DirEntry::metadata` 不跟随符号链接，只处理桶根下的真实目录。
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified >= cutoff {
            continue;
        }
        // Windows 上仍被 Chromium 占用的文件会让删除失败，下一轮再试。
        let _ = fs::remove_dir_all(entry.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "balancehub-prune-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn profile_pruning_removes_expired_orphans_and_keeps_current_buckets() {
        let root = temp_root("expired");
        fs::create_dir_all(root.join("current")).unwrap();
        fs::create_dir_all(root.join("orphan")).unwrap();
        fs::write(root.join("stray-file"), b"keep").unwrap();
        let held = HashSet::from(["current".to_string()]);
        prune_profiles(&root, &held, SystemTime::now() + Duration::from_secs(60));
        assert!(root.join("current").is_dir());
        assert!(!root.join("orphan").exists());
        assert!(root.join("stray-file").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recent_orphans_survive_until_retention_expires() {
        let root = temp_root("recent");
        fs::create_dir_all(root.join("orphan")).unwrap();
        prune_profiles(
            &root,
            &HashSet::new(),
            SystemTime::now() - Duration::from_secs(60),
        );
        assert!(root.join("orphan").is_dir());
        let _ = fs::remove_dir_all(root);
    }
}
