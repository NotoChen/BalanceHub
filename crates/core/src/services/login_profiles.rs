//! Local browser profile storage. Paths are derived solely from validated IDs.
use crate::{
    models::{valid_login_account_id, LoginPlatform},
    util::read_text_file_limited,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

/// 通过配置导入、云同步或手动编辑 `config.json` 移除的账号不会经过账号
/// 管理的删除流程；删除账号时若中途崩溃还会留下 `removed-*` 目录。与签到
/// 会话桶一致，只清理超过保留期且不在当前配置里的目录。
const PROFILE_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
static LAST_PRUNE: AtomicU64 = AtomicU64::new(0);

fn root() -> Result<PathBuf, String> {
    Ok(crate::host::data_dir()
        .map_err(|_| "无法定位登录数据目录")?
        .join("login-browser"))
}

pub fn directory(id: &str) -> Result<PathBuf, String> {
    if !valid_login_account_id(id) {
        return Err("登录账号标识无效".into());
    }
    Ok(root()?.join(id))
}

/// 账号管理页打开、浏览器签到启动或后台每日维护时顺手清理一次（每天最多一次）。
/// 配置读取失败或运行状态被阻塞时不猜哪些账号已删除，直接跳过。
pub fn schedule_orphan_prune() {
    let now = crate::util::unix_millis() as u64;
    let last = LAST_PRUNE.load(Ordering::Relaxed);
    if now.saturating_sub(last) < PRUNE_INTERVAL.as_millis() as u64 {
        return;
    }
    let Ok(root) = root() else {
        return;
    };
    let held: HashSet<String> = {
        let state = crate::host::core_state();
        if state.load_error().is_some() {
            return;
        }
        let Ok(data) = state.data.read() else {
            return;
        };
        data.login_accounts
            .iter()
            .map(|account| account.id.clone())
            .collect()
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
    crate::runtime::spawn_blocking(move || prune_orphans(&root, &held, cutoff));
}

fn prune_orphans(root: &Path, held: &HashSet<String>, cutoff: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // 旧版共用的 `profile` 目录由账号管理自动接管，永远保留。
        if name == "profile" || held.contains(name) {
            continue;
        }
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
}

impl ProfileCookie {
    pub fn id(&self) -> String {
        serde_json::json!([self.domain, self.path, self.name]).to_string()
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySnapshot {
    #[serde(default)]
    pub platform: LoginPlatform,
    pub identity: Option<String>,
    pub observed_at: Option<i64>,
}

pub fn cookies(id: &str) -> Result<Vec<ProfileCookie>, String> {
    let path = directory(id)?.join("identity-cookies.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = read_text_file_limited(&path, 4 * 1024 * 1024, "读取账号 Cookie")?;
    serde_json::from_str(&text).map_err(|_| "本地账号 Cookie 文件格式无效".into())
}

pub fn identity(id: &str) -> Result<IdentitySnapshot, String> {
    let path = directory(id)?.join("identity-state.json");
    if !path.exists() {
        return Ok(IdentitySnapshot::default());
    }
    let text = read_text_file_limited(&path, 64 * 1024, "读取账号身份")?;
    serde_json::from_str(&text).map_err(|_| "本地账号身份文件格式无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_ids_cannot_escape_the_login_directory() {
        for value in ["", "..", "../profile", "a/b", "a\\b", "/tmp", "a.b"] {
            assert!(!valid_login_account_id(value));
        }
        assert!(valid_login_account_id("account-1-2"));
        assert!(valid_login_account_id("profile"));
    }

    #[test]
    fn expired_orphan_profiles_are_removed_and_held_ones_survive() {
        let root = tempfile::tempdir().unwrap();
        for name in ["account-held", "account-gone", "profile", "removed-a-1"] {
            fs::create_dir_all(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("stray-file"), b"keep").unwrap();
        let held = HashSet::from(["account-held".to_string()]);
        prune_orphans(
            root.path(),
            &held,
            SystemTime::now() + Duration::from_secs(60),
        );
        assert!(root.path().join("account-held").is_dir());
        assert!(root.path().join("profile").is_dir());
        assert!(!root.path().join("account-gone").exists());
        assert!(!root.path().join("removed-a-1").exists());
        assert!(root.path().join("stray-file").is_file());
    }

    #[test]
    fn recent_orphan_profiles_survive_until_retention_expires() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("account-gone")).unwrap();
        prune_orphans(
            root.path(),
            &HashSet::new(),
            SystemTime::now() - Duration::from_secs(60),
        );
        assert!(root.path().join("account-gone").is_dir());
    }
}
