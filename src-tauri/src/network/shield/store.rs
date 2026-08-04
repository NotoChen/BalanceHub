use super::{ttl_secs, CacheKey, ChallengeState, ShieldCredential};
use crate::{limits, util::unix_secs};
use lru::LruCache;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::Mutex as AsyncMutex;

const CHALLENGE_STATE_TTL: u64 = 30 * 60;

fn credential_cache() -> &'static Mutex<LruCache<CacheKey, ShieldCredential>> {
    static CACHE: OnceLock<Mutex<LruCache<CacheKey, ShieldCredential>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(limits::MAX_SHIELD_CACHE_ENTRIES)
                .expect("shield cache capacity must be non-zero"),
        ))
    })
}

pub(super) fn cached(key: &CacheKey) -> Option<ShieldCredential> {
    let now = unix_secs();
    let mut cache = credential_cache().lock().ok()?;
    let credential = cache.get(key).cloned()?;
    if now.saturating_sub(credential.acquired_at) >= ttl_secs(key.kind) {
        cache.pop(key);
        return None;
    }
    Some(credential)
}

pub(super) fn store(key: CacheKey, credential: ShieldCredential) {
    if let Ok(mut cache) = credential_cache().lock() {
        cache.put(key, credential);
    }
}

pub(super) fn invalidate_if_matches(key: &CacheKey, applied: &ShieldCredential) {
    if let Ok(mut cache) = credential_cache().lock() {
        let matches = cache
            .peek(key)
            .is_some_and(|current| current.same_material(applied));
        if matches {
            cache.pop(key);
        }
    }
}

fn challenge_cache() -> &'static Mutex<LruCache<String, ChallengeState>> {
    static CACHE: OnceLock<Mutex<LruCache<String, ChallengeState>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(limits::MAX_PROVIDERS)
                .expect("challenge cache capacity must be non-zero"),
        ))
    })
}

pub(super) fn mark_challenge(state: ChallengeState) -> bool {
    if let Ok(mut cache) = challenge_cache().lock() {
        let changed = cache
            .peek(&state.provider_id)
            .is_none_or(|current| current.kind != state.kind || current.url != state.url);
        cache.put(state.provider_id.clone(), state);
        changed
    } else {
        false
    }
}

pub(super) fn challenge_for(provider_id: &str) -> Option<ChallengeState> {
    let mut cache = challenge_cache().lock().ok()?;
    let state = cache.get(provider_id).cloned()?;
    if unix_secs().saturating_sub(state.recorded_at) >= CHALLENGE_STATE_TTL {
        cache.pop(provider_id);
        return None;
    }
    Some(state)
}

pub(super) fn clear_challenge(provider_id: &str) -> bool {
    if let Ok(mut cache) = challenge_cache().lock() {
        cache.pop(provider_id).is_some()
    } else {
        false
    }
}

/// Keep keyed locks only while a solver is active. A weak entry avoids the
/// unbounded host-lock map that previously retained every provider forever.
pub(super) fn lock_for(key: &CacheKey) -> Arc<AsyncMutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<CacheKey, Weak<AsyncMutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = locks.lock().unwrap_or_else(|error| error.into_inner());
    guard.retain(|_, lock| lock.upgrade().is_some());
    if let Some(lock) = guard.get(key).and_then(Weak::upgrade) {
        return lock;
    }

    let lock = Arc::new(AsyncMutex::new(()));
    guard.insert(key.clone(), Arc::downgrade(&lock));
    lock
}
