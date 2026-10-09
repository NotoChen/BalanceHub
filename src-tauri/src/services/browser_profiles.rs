//! Bounded browser concurrency with exclusive access to each persistent profile.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, Semaphore, SemaphorePermit};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum ProfileKey {
    Account(String),
    CheckIn(String),
}

pub(crate) struct ProfileLease<'a> {
    _profile: OwnedMutexGuard<()>,
    _slot: SemaphorePermit<'a>,
}

struct ProfilePool {
    slots: Semaphore,
    profiles: Mutex<HashMap<ProfileKey, Weak<AsyncMutex<()>>>>,
}

impl ProfilePool {
    fn new(capacity: usize) -> Self {
        Self {
            slots: Semaphore::new(capacity),
            profiles: Mutex::new(HashMap::new()),
        }
    }

    fn profile(&self, key: &ProfileKey) -> Result<Arc<AsyncMutex<()>>, String> {
        let mut profiles = self.profiles.lock().map_err(|_| "读取浏览器队列失败")?;
        profiles.retain(|_, profile| profile.strong_count() > 0);
        if let Some(profile) = profiles.get(key).and_then(Weak::upgrade) {
            return Ok(profile);
        }
        let profile = Arc::new(AsyncMutex::new(()));
        profiles.insert(key.clone(), Arc::downgrade(&profile));
        Ok(profile)
    }

    async fn acquire(&self, key: &ProfileKey) -> Result<ProfileLease<'_>, String> {
        // Wait for the account first: queued work sharing it must not consume
        // window slots needed by independent accounts and verification profiles.
        let profile = self.profile(key)?.lock_owned().await;
        let slot = self.slots.acquire().await.map_err(|_| "浏览器队列不可用")?;
        Ok(ProfileLease {
            _profile: profile,
            _slot: slot,
        })
    }

    fn idle(&self, key: &ProfileKey) -> Result<OwnedMutexGuard<()>, String> {
        self.profile(key)?
            .try_lock_owned()
            .map_err(|_| "该账号的浏览器任务正在进行，请结束后再清除登录环境".into())
    }
}

fn pool() -> &'static ProfilePool {
    static POOL: OnceLock<ProfilePool> = OnceLock::new();
    POOL.get_or_init(|| ProfilePool::new(3))
}

pub(crate) async fn acquire(key: ProfileKey) -> Result<ProfileLease<'static>, String> {
    pool().acquire(&key).await
}

pub(crate) fn account_busy(id: &str) -> bool {
    idle_account(id).is_err()
}

pub(crate) fn idle_account(id: &str) -> Result<OwnedMutexGuard<()>, String> {
    pool().idle(&ProfileKey::Account(id.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn independent_profiles_open_together_but_the_same_account_waits() {
        let pool = ProfilePool::new(3);
        let account = ProfileKey::Account("fixture-a".into());
        let first = pool.acquire(&account).await.unwrap();
        assert!(pool.idle(&account).is_err());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), pool.acquire(&account))
                .await
                .is_err()
        );
        let second = pool
            .acquire(&ProfileKey::Account("fixture-b".into()))
            .await
            .unwrap();
        let third = pool
            .acquire(&ProfileKey::CheckIn("fixture-a".into()))
            .await
            .unwrap();
        assert_eq!(pool.slots.available_permits(), 0);
        drop((first, second, third));
        assert!(pool.idle(&account).is_ok());
        assert_eq!(pool.slots.available_permits(), 3);
    }

    #[tokio::test]
    async fn cancellation_releases_a_queued_profile_and_all_browser_capacity() {
        let pool = ProfilePool::new(1);
        let active = pool
            .acquire(&ProfileKey::Account("active".into()))
            .await
            .unwrap();
        let queued = ProfileKey::Account("queued".into());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), pool.acquire(&queued))
                .await
                .is_err()
        );
        // Cancelling work waiting for capacity cannot leave its profile locked.
        assert!(pool.idle(&queued).is_ok());
        assert!(pool.idle(&ProfileKey::Account("unrelated".into())).is_ok());
        drop(active);
        let next = pool.acquire(&queued).await.unwrap();
        drop(next);
        assert_eq!(pool.slots.available_permits(), 1);
    }
}
