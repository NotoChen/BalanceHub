//! Memory-only file reuse for display scans. Mutation scans never use this cache.
use super::{AgentAssetSnapshot, SnapshotProduct, SnapshotRequest};
use crate::services::agent_cli::{
    cache,
    contracts::AgentAssetSourcePathPolicy,
    environment::{run::AgentInventoryRun, verified_path::reopen_verified_path},
};
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Mutex, OnceLock},
};
const MAX_BYTES: usize = 16 * 1024 * 1024;
struct Files {
    entries: LruCache<String, SnapshotProduct>,
    bytes: usize,
}
fn files() -> &'static Mutex<Files> {
    static FILES: OnceLock<Mutex<Files>> = OnceLock::new();
    FILES.get_or_init(|| {
        Mutex::new(Files {
            entries: LruCache::new(NonZeroUsize::new(2048).unwrap()),
            bytes: 0,
        })
    })
}
pub(super) fn key(request: &SnapshotRequest<'_>) -> Option<String> {
    (request.source.path_policy == AgentAssetSourcePathPolicy::NoFollow).then(|| {
        cache::key(
            format!(
                "{:?}:{:?}:{:?}",
                request.source.path, request.source.allowed_root, request.trusted_roots
            )
            .as_bytes(),
        )
    })
}
fn size(snapshot: &SnapshotProduct) -> usize {
    match &snapshot.0 {
        AgentAssetSnapshot::File { bytes, .. } => bytes.len(),
        _ => 0,
    }
}
pub(super) fn get(key: &str, run: &mut AgentInventoryRun) -> Option<SnapshotProduct> {
    let product = files().lock().ok()?.entries.get(key)?.clone();
    let bytes = size(&product);
    if bytes > run.limits().bytes_per_source || run.can_start_read(bytes).is_err() {
        return None;
    }
    // Re-open the handle chain and compare the full source stamp, including ctime,
    // permissions and identity. Changed/deleted/replaced paths cannot hit the cache.
    let anchor = product.1.as_ref()?;
    if reopen_verified_path(anchor).is_err() {
        return None;
    }
    run.commit_read(bytes);
    Some(product)
}
pub(super) fn put(key: String, product: SnapshotProduct) {
    let bytes = size(&product);
    if bytes == 0 || bytes > MAX_BYTES || product.1.is_none() {
        return;
    }
    let Ok(mut files) = files().lock() else {
        return;
    };
    if let Some(old) = files.entries.pop(&key) {
        files.bytes -= size(&old);
    }
    while files.bytes + bytes > MAX_BYTES || files.entries.len() == files.entries.cap().get() {
        if let Some((_, old)) = files.entries.pop_lru() {
            files.bytes -= size(&old);
        } else {
            break;
        }
    }
    files.bytes += bytes;
    files.entries.put(key, product);
}
