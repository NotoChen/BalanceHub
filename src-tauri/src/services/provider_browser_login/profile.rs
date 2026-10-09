//! Imports, account management and check-in share the same persistent profiles.
use std::sync::Mutex;
use tokio::sync::{Semaphore, SemaphorePermit};

static SLOT: Semaphore = Semaphore::const_new(1);
static ACTIVE_ACCOUNT: Mutex<Option<String>> = Mutex::new(None);

pub(super) struct ProfileLease {
    _slot: SemaphorePermit<'static>,
}

pub(super) async fn acquire(account_id: &str) -> Result<ProfileLease, String> {
    let slot = SLOT.acquire().await.map_err(|_| "登录队列不可用")?;
    *ACTIVE_ACCOUNT.lock().map_err(|_| "读取登录账号状态失败")? = Some(account_id.into());
    Ok(ProfileLease { _slot: slot })
}

pub(super) fn busy(id: &str) -> bool {
    ACTIVE_ACCOUNT
        .lock()
        .map(|account| account.as_deref() == Some(id))
        .unwrap_or(true)
}

pub(crate) fn idle_slot() -> Result<SemaphorePermit<'static>, String> {
    SLOT.try_acquire()
        .map_err(|_| "浏览器任务正在进行，请结束后再清除登录环境".into())
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        if let Ok(mut account) = ACTIVE_ACCOUNT.lock() {
            *account = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn check_in_and_import_share_a_cancellable_exclusive_profile_lease() {
        let lease = acquire("fixture-a").await.unwrap();
        assert!(busy("fixture-a"));
        assert!(idle_slot().is_err());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), acquire("fixture-b"))
                .await
                .is_err()
        );
        drop(lease);
        assert!(!busy("fixture-a"));
        let next = acquire("fixture-b").await.unwrap();
        assert!(busy("fixture-b"));
        drop(next);
        assert!(idle_slot().is_ok());
    }
}
