//! Atomic, bounded Hook event spool. Valid records are removed only after ack.

use super::event::{NormalizedHookEvent, NORMALIZED_HOOK_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_SPOOL_MAX_FILES: usize = 5_000;
pub const DEFAULT_SPOOL_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const DEFAULT_SPOOL_MAX_AGE_MILLIS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const DEFAULT_HOOK_PAYLOAD_MAX_BYTES: usize = 256 * 1024;
pub const DEFAULT_SPOOL_BATCH_SIZE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoolDiagnostic {
    InvalidRoot,
    Unavailable,
    CapacityFiles,
    CapacityBytes,
    PayloadTooLarge,
    InvalidPayload,
    UnsupportedSchema,
    CorruptEvent,
    UnsafePath,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookIngestResult {
    pub accepted: bool,
    pub event_id: Option<String>,
    pub diagnostic: Option<SpoolDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSpoolLimits {
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_age_millis: i64,
    pub max_event_bytes: usize,
}

impl Default for HookSpoolLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_SPOOL_MAX_FILES,
            max_bytes: DEFAULT_SPOOL_MAX_BYTES,
            max_age_millis: DEFAULT_SPOOL_MAX_AGE_MILLIS,
            max_event_bytes: DEFAULT_HOOK_PAYLOAD_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookSpoolRepository {
    root: PathBuf,
    limits: HookSpoolLimits,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolEventRecord {
    pub path: PathBuf,
    pub event: NormalizedHookEvent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookSpoolBatch {
    pub events: Vec<SpoolEventRecord>,
    pub duplicate_paths: Vec<PathBuf>,
    pub quarantined_paths: Vec<PathBuf>,
}

impl HookSpoolRepository {
    pub fn new(app_data_root: impl AsRef<Path>) -> Result<Self, SpoolDiagnostic> {
        Self::with_limits(app_data_root, HookSpoolLimits::default())
    }

    pub fn with_limits(
        app_data_root: impl AsRef<Path>,
        limits: HookSpoolLimits,
    ) -> Result<Self, SpoolDiagnostic> {
        let root = app_data_root.as_ref();
        validate_existing_root(root)?;
        let root = fs::canonicalize(root).map_err(|_| SpoolDiagnostic::InvalidRoot)?;
        let spool = root.join("hook-spool");
        create_owned_directory(&spool)?;
        create_owned_directory(&spool.join("incoming"))?;
        create_owned_directory(&spool.join("quarantine"))?;
        Ok(Self {
            root,
            limits,
            lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn incoming_dir(&self) -> PathBuf {
        self.root.join("hook-spool").join("incoming")
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join("hook-spool").join("quarantine")
    }

    pub fn limits(&self) -> HookSpoolLimits {
        self.limits
    }

    pub fn append(&self, event: &NormalizedHookEvent) -> HookIngestResult {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        if event.schema_version != NORMALIZED_HOOK_SCHEMA_VERSION || !valid_event(event) {
            return failed(SpoolDiagnostic::InvalidPayload);
        }
        let Ok(bytes) = serde_json::to_vec(event) else {
            return failed(SpoolDiagnostic::InvalidPayload);
        };
        if bytes.len() > self.limits.max_event_bytes {
            return failed(SpoolDiagnostic::PayloadTooLarge);
        }
        purge_expired(
            &self.incoming_dir(),
            self.limits.max_age_millis,
            event.received_at,
        );
        let (file_count, total_bytes) = incoming_usage(&self.incoming_dir());
        if file_count >= self.limits.max_files {
            return failed(SpoolDiagnostic::CapacityFiles);
        }
        if total_bytes.saturating_add(bytes.len() as u64) > self.limits.max_bytes {
            return failed(SpoolDiagnostic::CapacityBytes);
        }
        if ensure_directory_chain(&self.incoming_dir()).is_err() {
            return failed(SpoolDiagnostic::Unavailable);
        }
        let nonce = next_file_nonce();
        let temp_path = self.incoming_dir().join(format!(".event-{nonce}.tmp"));
        let final_path = self.incoming_dir().join(format!("event-{nonce}.json"));
        if write_atomic_event(&temp_path, &final_path, &bytes).is_err() {
            let _ = fs::remove_file(&temp_path);
            return failed(SpoolDiagnostic::Io);
        }
        HookIngestResult {
            accepted: true,
            event_id: Some(event.event_id.clone()),
            diagnostic: None,
        }
    }

    pub fn read_batch(
        &self,
        max_events: usize,
        known_event_ids: &BTreeSet<String>,
    ) -> Result<HookSpoolBatch, SpoolDiagnostic> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut paths = regular_files(&self.incoming_dir())?;
        paths.sort();
        let mut batch = HookSpoolBatch::default();
        let max_events = if max_events == 0 {
            DEFAULT_SPOOL_BATCH_SIZE
        } else {
            max_events
        };
        for path in paths.into_iter().take(max_events) {
            let Ok(bytes) = read_limited(&path, self.limits.max_event_bytes) else {
                batch
                    .quarantined_paths
                    .push(quarantine_file(&path, &self.quarantine_dir()));
                continue;
            };
            let Ok(event) = serde_json::from_slice::<NormalizedHookEvent>(&bytes) else {
                batch
                    .quarantined_paths
                    .push(quarantine_file(&path, &self.quarantine_dir()));
                continue;
            };
            if event.schema_version != NORMALIZED_HOOK_SCHEMA_VERSION || !valid_event(&event) {
                batch
                    .quarantined_paths
                    .push(quarantine_file(&path, &self.quarantine_dir()));
                continue;
            }
            if known_event_ids.contains(&event.event_id)
                || batch
                    .events
                    .iter()
                    .any(|record| record.event.event_id == event.event_id)
            {
                batch.duplicate_paths.push(path);
            } else {
                batch.events.push(SpoolEventRecord { path, event });
            }
        }
        Ok(batch)
    }

    pub fn acknowledge(&self, batch: &HookSpoolBatch) -> Result<(), SpoolDiagnostic> {
        let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        for record in &batch.events {
            remove_incoming_file(&record.path)?;
        }
        for path in &batch.duplicate_paths {
            remove_incoming_file(path)?;
        }
        Ok(())
    }
}

fn failed(diagnostic: SpoolDiagnostic) -> HookIngestResult {
    HookIngestResult {
        accepted: false,
        event_id: None,
        diagnostic: Some(diagnostic),
    }
}

fn validate_existing_root(root: &Path) -> Result<(), SpoolDiagnostic> {
    if !root.is_absolute() || !root.is_dir() {
        return Err(SpoolDiagnostic::InvalidRoot);
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| SpoolDiagnostic::InvalidRoot)?;
    if metadata.file_type().is_symlink() {
        return Err(SpoolDiagnostic::UnsafePath);
    }
    Ok(())
}

fn create_owned_directory(path: &Path) -> Result<(), SpoolDiagnostic> {
    fs::create_dir_all(path).map_err(|_| SpoolDiagnostic::Unavailable)?;
    ensure_directory_chain(path)
}

fn ensure_directory_chain(path: &Path) -> Result<(), SpoolDiagnostic> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(part) => {
                current.push(part);
                let metadata =
                    fs::symlink_metadata(&current).map_err(|_| SpoolDiagnostic::Unavailable)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(SpoolDiagnostic::UnsafePath);
                }
            }
            Component::CurDir | Component::ParentDir => return Err(SpoolDiagnostic::UnsafePath),
        }
    }
    Ok(())
}

fn regular_files(directory: &Path) -> Result<Vec<PathBuf>, SpoolDiagnostic> {
    ensure_directory_chain(directory)?;
    Ok(fs::read_dir(directory)
        .map_err(|_| SpoolDiagnostic::Unavailable)?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            (metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json"))
            .then_some(path)
        })
        .collect())
}

fn incoming_usage(directory: &Path) -> (usize, u64) {
    regular_files(directory)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| fs::metadata(path).ok())
        .fold((0, 0), |(count, bytes), metadata| {
            (count + 1, bytes.saturating_add(metadata.len()))
        })
}

fn purge_expired(directory: &Path, max_age_millis: i64, now_millis: i64) {
    if max_age_millis <= 0 {
        return;
    }
    for path in regular_files(directory).unwrap_or_default() {
        let expired = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(system_time_millis)
            .is_some_and(|modified| now_millis.saturating_sub(modified) >= max_age_millis);
        if expired {
            let _ = remove_incoming_file(&path);
        }
    }
}

fn system_time_millis(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn write_atomic_event(temp: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    restrict_file_to_owner(&file)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, destination)
}

fn read_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
    std::io::Read::by_ref(&mut file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "event too large",
        ));
    }
    Ok(bytes)
}

fn remove_incoming_file(path: &Path) -> Result<(), SpoolDiagnostic> {
    let metadata = fs::symlink_metadata(path).map_err(|_| SpoolDiagnostic::Unavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SpoolDiagnostic::UnsafePath);
    }
    fs::remove_file(path).map_err(|_| SpoolDiagnostic::Io)
}

fn quarantine_file(path: &Path, quarantine: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default();
    let destination = quarantine.join(name);
    if fs::rename(path, &destination).is_ok() {
        destination
    } else {
        path.to_path_buf()
    }
}

fn restrict_file_to_owner(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o600);
        file.set_permissions(permissions)?;
    }
    Ok(())
}

fn next_file_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!(
        "{millis}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn valid_event(event: &NormalizedHookEvent) -> bool {
    !event.event_id.trim().is_empty()
        && !event.native_event.trim().is_empty()
        && event.event_id.chars().count() <= 512
        && event.native_event.chars().count() <= 256
}
