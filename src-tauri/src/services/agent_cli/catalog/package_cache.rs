//! Bounded live package reuse for list projection. Write plans always read afresh.
use super::package::{self, PackageSnapshot};
use crate::services::agent_cli::environment::verified_path::reopen_verified_path;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKAGES: usize = 256;

struct Entry {
    snapshot: Arc<PackageSnapshot>,
    bytes: usize,
    used: Instant,
}

#[derive(Default)]
pub(super) struct PackageCache {
    entries: BTreeMap<(PathBuf, PathBuf), Entry>,
    bytes: usize,
}

impl PackageCache {
    pub(super) fn read(
        &mut self,
        root: &Path,
        allowed_root: &Path,
    ) -> Result<Arc<PackageSnapshot>, String> {
        let key = (root.to_owned(), allowed_root.to_owned());
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry
                .snapshot
                .anchors
                .iter()
                .all(|anchor| reopen_verified_path(anchor).is_ok())
            {
                entry.used = Instant::now();
                return Ok(Arc::clone(&entry.snapshot));
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        let snapshot = Arc::new(package::read_package(root, allowed_root)?);
        let bytes = snapshot
            .files
            .values()
            .map(|file| file.bytes.len())
            .sum::<usize>();
        while !self.entries.is_empty()
            && (self.bytes + bytes > MAX_BYTES || self.entries.len() >= MAX_PACKAGES)
        {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            if let Some(entry) = oldest.and_then(|key| self.entries.remove(&key)) {
                self.bytes -= entry.bytes;
            }
        }
        if bytes <= MAX_BYTES {
            self.bytes += bytes;
            self.entries.insert(
                key,
                Entry {
                    snapshot: Arc::clone(&snapshot),
                    bytes,
                    used: Instant::now(),
                },
            );
        }
        Ok(snapshot)
    }
}
