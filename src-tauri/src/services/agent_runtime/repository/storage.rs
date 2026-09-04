//! Durable runtime projection storage and reducer replay helpers.

use super::super::reducer::{reduce, AgentRuntimeEvent, AgentRuntimeProjection};
use super::{
    AgentRuntimeRepository, AgentRuntimeRepositoryError, AgentRuntimeSnapshot,
    StoredRuntimeProjection, MAX_RUNTIME_PROJECTION_BYTES, RUNTIME_REPOSITORY_SCHEMA_VERSION,
};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn snapshot_from_stored(stored: &StoredRuntimeProjection) -> AgentRuntimeSnapshot {
    let projection = reduce_events(&stored.events);
    AgentRuntimeSnapshot {
        schema_version: stored.schema_version,
        revision: stored.revision,
        updated_at: stored.updated_at,
        sessions: projection.sessions().cloned().collect(),
    }
}

pub(super) fn reduce_events(events: &[AgentRuntimeEvent]) -> AgentRuntimeProjection {
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.observed_at
            .cmp(&right.observed_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    ordered
        .into_iter()
        .fold(AgentRuntimeProjection::default(), |projection, event| {
            reduce(projection, event.clone())
        })
}

pub(super) fn append_unique_event(
    events: &mut Vec<AgentRuntimeEvent>,
    event_ids: &mut BTreeSet<String>,
    event: AgentRuntimeEvent,
) -> bool {
    if event.event_id.trim().is_empty() || !event_ids.insert(event.event_id.clone()) {
        return false;
    }
    events.push(event);
    true
}

pub(super) fn trim_event_history(events: &mut Vec<AgentRuntimeEvent>, max_events: usize) {
    if events.len() <= max_events {
        return;
    }
    events.sort_by(|left, right| {
        left.observed_at
            .cmp(&right.observed_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    let remove = events.len() - max_events;
    events.drain(..remove);
}

pub(super) fn reject_symlink_or_non_directory(
    path: &Path,
) -> Result<(), AgentRuntimeRepositoryError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| AgentRuntimeRepositoryError::InvalidRoot)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AgentRuntimeRepositoryError::UnsafePath);
    }
    Ok(())
}

pub(super) fn ensure_directory_chain(path: &Path) -> Result<(), AgentRuntimeRepositoryError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(part) => {
                current.push(part);
                reject_symlink_or_non_directory(&current)?;
            }
            Component::CurDir | Component::ParentDir => {
                return Err(AgentRuntimeRepositoryError::UnsafePath)
            }
        }
    }
    Ok(())
}

pub(super) fn read_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
    (&mut file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    Ok(bytes)
}

fn write_atomic(temporary: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    restrict_file_to_owner(&file)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    replace_file(temporary, destination)
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let backup = destination.with_extension(format!("bak-{}", next_nonce()));
    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &backup)?;
    }
    match fs::rename(source, destination) {
        Ok(()) => {
            if had_destination {
                let _ = fs::remove_file(backup);
            }
            Ok(())
        }
        Err(error) => {
            if had_destination {
                let _ = fs::rename(&backup, destination);
            }
            Err(error)
        }
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

pub(super) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn next_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        now_millis(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl AgentRuntimeRepository {
    pub(super) fn read_stored_projection(
        &self,
    ) -> Result<StoredRuntimeProjection, AgentRuntimeRepositoryError> {
        match fs::symlink_metadata(&self.projection_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AgentRuntimeRepositoryError::UnsafePath);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(StoredRuntimeProjection::default())
            }
            Err(_) => return Err(AgentRuntimeRepositoryError::Io),
        }
        let bytes = read_limited(&self.projection_path, MAX_RUNTIME_PROJECTION_BYTES)
            .map_err(|_| AgentRuntimeRepositoryError::Io)?;
        let stored = serde_json::from_slice::<StoredRuntimeProjection>(&bytes)
            .map_err(|_| AgentRuntimeRepositoryError::CorruptProjection)?;
        if stored.schema_version != RUNTIME_REPOSITORY_SCHEMA_VERSION {
            return Err(AgentRuntimeRepositoryError::UnsupportedSchema(
                stored.schema_version,
            ));
        }
        Ok(stored)
    }

    pub(super) fn write_stored_projection(
        &self,
        stored: &StoredRuntimeProjection,
    ) -> Result<(), AgentRuntimeRepositoryError> {
        let bytes =
            serde_json::to_vec(stored).map_err(|_| AgentRuntimeRepositoryError::Serialization)?;
        if bytes.len() > MAX_RUNTIME_PROJECTION_BYTES {
            return Err(AgentRuntimeRepositoryError::Serialization);
        }
        let sequence = next_nonce();
        let temporary = self
            .projection_path
            .with_extension(format!("tmp-{sequence}"));
        let result = write_atomic(&temporary, &self.projection_path, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|_| AgentRuntimeRepositoryError::Io)
    }
}
