//! Bounded physical-source snapshots from one verified component handle chain.
mod display_cache;

use crate::{
    models::{
        AgentAssetDiagnostic, AgentAssetIoErrorKind, AgentAssetLimitKind, AgentAssetLimits,
        AgentAssetRevision, AgentAssetSourceKind,
    },
    services::agent_cli::contracts::{
        AgentAssetDirectoryEntry, AgentAssetSnapshot, AgentAssetSourcePathPolicy,
        AgentAssetSourceSpec,
    },
};
use chrono::{DateTime, Local};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

#[cfg(test)]
use std::ffi::OsString;

use super::{
    diagnostics::DiagnosticOwner,
    identity::{lexical_identity, stable_id},
    run::AgentInventoryRun,
    verified_path::{
        inspect_readonly_skill_link, inspect_verified_path, DirectoryEntryCandidate,
        VerifiedPathAnchor, VerifiedPathError, VerifiedPathGuard,
    },
};

pub(super) struct SnapshotRequest<'a> {
    pub source: &'a AgentAssetSourceSpec,
    pub source_id: &'a str,
    pub trusted_roots: &'a [&'a Path],
}

pub(super) trait SnapshotPort: Send + Sync {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot;

    fn access_anchor(&self, _revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        None
    }
}

/// A refresh-local collector, bounded independently of directory/file handles.
/// Tests supplying snapshots do not manufacture authorizing filesystem evidence.
#[derive(Debug, Default)]
pub(super) struct RealSnapshotPort {
    anchors: Mutex<BTreeMap<String, VerifiedPathAnchor>>,
    reuse_files: bool,
    hits: std::sync::atomic::AtomicUsize,
    reads: std::sync::atomic::AtomicUsize,
    read_bytes: std::sync::atomic::AtomicUsize,
}

impl RealSnapshotPort {
    pub(super) fn for_display() -> Self {
        Self {
            reuse_files: true,
            ..Self::default()
        }
    }
    pub(super) fn metrics(&self) -> (usize, usize, usize) {
        use std::sync::atomic::Ordering::Relaxed;
        (
            self.hits.load(Relaxed),
            self.reads.load(Relaxed),
            self.read_bytes.load(Relaxed),
        )
    }
}

impl SnapshotPort for RealSnapshotPort {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        let manifest_anchor = match &request.source.path_policy {
            AgentAssetSourcePathPolicy::ReadonlySkillLink {
                manifest_revision, ..
            } => self.access_anchor(manifest_revision),
            _ => None,
        };
        let cache_key = self
            .reuse_files
            .then(|| display_cache::key(&request))
            .flatten();
        let cached = cache_key
            .as_deref()
            .and_then(|key| display_cache::get(key, run));
        let (snapshot, anchor) = if let Some(cached) = cached {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            cached
        } else {
            let product = snapshot_source_with_anchor(
                request.source,
                request.source_id,
                request.trusted_roots,
                manifest_anchor.as_ref(),
                run,
            );
            if let AgentAssetSnapshot::File { bytes, .. } = &product.0 {
                self.reads
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.read_bytes
                    .fetch_add(bytes.len(), std::sync::atomic::Ordering::Relaxed);
                if let Some(key) = cache_key {
                    display_cache::put(key, product.clone());
                }
            }
            product
        };
        if let Some(anchor) = anchor {
            if let Ok(mut anchors) = self.anchors.lock() {
                // Additional sources remain useful inventory rows; lack of
                // retained evidence makes only their access unavailable.
                if anchors.len() < 8_192 {
                    anchors.insert(anchor.revision().identity.clone(), anchor);
                }
            }
        }
        snapshot
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.anchors.lock().ok()?.get(&revision.identity).cloned()
    }
}

type SnapshotProduct = (AgentAssetSnapshot, Option<VerifiedPathAnchor>);

#[cfg(test)]
pub(crate) fn snapshot_source(
    source: &AgentAssetSourceSpec,
    source_id: &str,
    trusted_roots: &[&Path],
    run: &mut AgentInventoryRun,
) -> AgentAssetSnapshot {
    snapshot_source_with_anchor(source, source_id, trusted_roots, None, run).0
}

fn snapshot_source_with_anchor(
    source: &AgentAssetSourceSpec,
    source_id: &str,
    trusted_roots: &[&Path],
    manifest_anchor: Option<&VerifiedPathAnchor>,
    run: &mut AgentInventoryRun,
) -> SnapshotProduct {
    if let Some(diagnostic) =
        run.deadline_diagnostic(DiagnosticOwner::Source(source_id.to_string()))
    {
        return (
            blocked_without_access(&source.path, diagnostic, false),
            None,
        );
    }
    // Exhausted refreshes do not open or enumerate another physical source.
    if let Err(diagnostic) = run.can_start_read(0) {
        return (
            blocked_without_access(&source.path, diagnostic, false),
            None,
        );
    }
    let readonly_link = matches!(
        source.path_policy,
        AgentAssetSourcePathPolicy::ReadonlySkillLink { .. }
    );
    let inspected = match &source.path_policy {
        AgentAssetSourcePathPolicy::ReadonlySkillLink {
            shared_root,
            manifest_path,
            manifest_revision,
            entry_name,
        } => match manifest_anchor {
            Some(anchor)
                if !source.writable
                    && source.source_kind == AgentAssetSourceKind::File
                    && anchor.display_path() == manifest_path
                    && anchor.allowed_root() == source.allowed_root
                    && anchor.revision().identity == manifest_revision.identity
                    && source.path == manifest_path.join(entry_name).join("SKILL.md") =>
            {
                inspect_readonly_skill_link(trusted_roots, anchor, entry_name, shared_root)
            }
            _ => Err(VerifiedPathError::SourceChanged),
        },
        _ => inspect_verified_path(
            trusted_roots,
            &source.allowed_root,
            &source.path,
            source.source_kind,
        ),
    };
    let guard = match inspected {
        Ok(guard) => guard,
        Err(VerifiedPathError::Io(error))
            if !readonly_link && error.kind() == std::io::ErrorKind::NotFound =>
        {
            return (
                AgentAssetSnapshot::Missing {
                    revision: revision_for_missing(&source.path),
                },
                None,
            );
        }
        Err(error) => {
            let symlink = matches!(error, VerifiedPathError::SymlinkRejected);
            let diagnostic = path_open_diagnostic(source_id, source.source_kind, error);
            return (
                blocked_without_access(&source.path, diagnostic, symlink),
                None,
            );
        }
    };
    let metadata = match guard.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            return (
                blocked_without_access(
                    &source.path,
                    path_open_diagnostic(source_id, source.source_kind, error),
                    false,
                ),
                None,
            );
        }
    };
    match source.source_kind {
        AgentAssetSourceKind::File => snapshot_file(&guard, source_id, &metadata, run),
        AgentAssetSourceKind::Directory => snapshot_directory(&guard, source_id, &metadata, run),
    }
}

fn snapshot_file(
    guard: &VerifiedPathGuard,
    source_id: &str,
    metadata: &fs::Metadata,
    run: &mut AgentInventoryRun,
) -> SnapshotProduct {
    let path = guard.display_path();
    if metadata.len() > run.limits().bytes_per_source as u64 {
        return (
            blocked_with_metadata(
                path,
                metadata,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::BytesPerSource,
                    accepted: run.limits().bytes_per_source as u64,
                    observed_at_least: metadata.len(),
                },
            ),
            None,
        );
    }
    if let Err(diagnostic) = run.can_start_read(metadata.len() as usize) {
        return (blocked_with_metadata(path, metadata, diagnostic), None);
    }
    let read_limit = run.limits().bytes_per_source.min(run.remaining_bytes());
    let bytes = match guard.read_file_bounded(read_limit) {
        Ok(bytes) => bytes,
        Err(VerifiedPathError::TooLarge) => {
            let limit = if read_limit == run.limits().bytes_per_source {
                AgentAssetLimitKind::BytesPerSource
            } else {
                AgentAssetLimitKind::BytesPerRefresh
            };
            return (
                blocked_with_metadata(
                    path,
                    metadata,
                    AgentAssetDiagnostic::Truncated {
                        limit,
                        accepted: read_limit as u64,
                        observed_at_least: read_limit.saturating_add(1) as u64,
                    },
                ),
                None,
            );
        }
        Err(error) => {
            return (
                blocked_with_metadata(
                    path,
                    metadata,
                    path_open_diagnostic(source_id, AgentAssetSourceKind::File, error),
                ),
                None,
            );
        }
    };
    run.commit_read(bytes.len());
    let revision = revision_from_bytes(guard, metadata, &bytes);
    let anchor = guard.anchor(&revision, Some(&bytes));
    (AgentAssetSnapshot::File { bytes, revision }, Some(anchor))
}

fn snapshot_directory(
    guard: &VerifiedPathGuard,
    source_id: &str,
    metadata: &fs::Metadata,
    run: &mut AgentInventoryRun,
) -> SnapshotProduct {
    let path = guard.display_path();
    let entries = match guard.read_entries() {
        Ok(entries) => entries,
        Err(error) => {
            return (
                blocked_with_metadata(
                    path,
                    metadata,
                    AgentAssetDiagnostic::ReadFailed {
                        source_id: source_id.to_string(),
                        error_kind: io_error_kind(&error),
                    },
                ),
                None,
            );
        }
    };
    let limits = run.limits().clone();
    let (entries, complete) =
        match collect_directory_entries_with_completion(entries, source_id, &limits, |diagnostic| {
            match diagnostic {
                Some(diagnostic) => {
                    run.emit(DiagnosticOwner::Source(source_id.to_string()), diagnostic);
                    None
                }
                None => run.deadline_diagnostic(DiagnosticOwner::Source(source_id.to_string())),
            }
        }) {
            Ok(entries) => entries,
            Err(diagnostic) => return (blocked_with_metadata(path, metadata, diagnostic), None),
        };
    if let Err(error) = guard.revalidate() {
        return (
            blocked_with_metadata(
                path,
                metadata,
                path_open_diagnostic(source_id, AgentAssetSourceKind::Directory, error),
            ),
            None,
        );
    }
    let manifest_bytes = entries
        .iter()
        .map(|entry| entry.name.len().saturating_add(2))
        .sum::<usize>();
    if manifest_bytes > run.limits().bytes_per_source {
        return (
            blocked_with_metadata(
                path,
                metadata,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::BytesPerSource,
                    accepted: run.limits().bytes_per_source as u64,
                    observed_at_least: manifest_bytes as u64,
                },
            ),
            None,
        );
    }
    if let Err(diagnostic) = run.can_start_read(manifest_bytes) {
        return (blocked_with_metadata(path, metadata, diagnostic), None);
    }
    run.commit_read(manifest_bytes);
    let revision = revision_from_manifest(guard, metadata, &entries);
    let anchor = guard.anchor(&revision, None);
    (
        AgentAssetSnapshot::DirectoryManifest {
            entries,
            revision,
            complete,
        },
        Some(anchor),
    )
}

#[cfg(test)]
fn collect_directory_entries<I, F>(
    entries: I,
    source_id: &str,
    limits: &AgentAssetLimits,
    report: F,
) -> Result<Vec<AgentAssetDirectoryEntry>, AgentAssetDiagnostic>
where
    I: Iterator<Item = std::io::Result<DirectoryEntryCandidate>>,
    F: FnMut(Option<AgentAssetDiagnostic>) -> Option<AgentAssetDiagnostic>,
{
    collect_directory_entries_with_completion(entries, source_id, limits, report)
        .map(|(entries, _)| entries)
}

fn collect_directory_entries_with_completion<I, F>(
    mut entries: I,
    source_id: &str,
    limits: &AgentAssetLimits,
    mut report: F,
) -> Result<(Vec<AgentAssetDirectoryEntry>, bool), AgentAssetDiagnostic>
where
    I: Iterator<Item = std::io::Result<DirectoryEntryCandidate>>,
    F: FnMut(Option<AgentAssetDiagnostic>) -> Option<AgentAssetDiagnostic>,
{
    let mut selected = BTreeMap::<String, AgentAssetDirectoryEntry>::new();
    let mut observed = 0_usize;
    loop {
        if let Some(diagnostic) = report(None) {
            // Until the platform reader proves EOF, an unseen name could sort
            // before the retained set, so a deadline must fail this source closed.
            return Err(diagnostic);
        }
        let Some(entry) = entries.next() else {
            break;
        };
        let entry = entry.map_err(|error| AgentAssetDiagnostic::ReadFailed {
            source_id: source_id.to_string(),
            error_kind: io_error_kind(&error),
        })?;
        observed = observed.saturating_add(1);
        let name = match entry.name.into_string() {
            Ok(name) => name,
            Err(_) => {
                report(Some(AgentAssetDiagnostic::ReadFailed {
                    source_id: source_id.to_string(),
                    error_kind: AgentAssetIoErrorKind::InvalidData,
                }));
                continue;
            }
        };
        let can_enter_retained_set = selected.len() < limits.first_level_entries
            || selected
                .last_key_value()
                .is_some_and(|(largest, _)| name < *largest);
        if !can_enter_retained_set {
            continue;
        }
        selected.insert(
            name.clone(),
            AgentAssetDirectoryEntry {
                name,
                source_kind: entry.source_kind,
                is_symlink: entry.is_symlink,
            },
        );
        if selected.len() > limits.first_level_entries {
            selected.pop_last();
        }
    }
    if let Some(diagnostic) = report(None) {
        return Err(diagnostic);
    }
    if observed > limits.first_level_entries {
        report(Some(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::FirstLevelEntries,
            accepted: selected.len() as u64,
            observed_at_least: observed as u64,
        }));
    }
    Ok((
        selected.into_values().collect(),
        observed <= limits.first_level_entries,
    ))
}

fn blocked_without_access(
    path: &Path,
    diagnostic: AgentAssetDiagnostic,
    is_symlink: bool,
) -> AgentAssetSnapshot {
    AgentAssetSnapshot::Blocked {
        revision: AgentAssetRevision {
            identity: stable_id(
                "blocked",
                &[
                    &lexical_identity(path),
                    if is_symlink { "symlink" } else { "unknown" },
                ],
            ),
            observed_at: now_string(),
            size_bytes: None,
            is_missing: false,
            is_directory: false,
            is_symlink,
        },
        diagnostic,
    }
}

fn blocked_with_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    diagnostic: AgentAssetDiagnostic,
) -> AgentAssetSnapshot {
    AgentAssetSnapshot::Blocked {
        revision: revision_from_hash(metadata, revision_hasher(path, metadata)),
        diagnostic,
    }
}

fn path_open_diagnostic(
    source_id: &str,
    expected: AgentAssetSourceKind,
    error: VerifiedPathError,
) -> AgentAssetDiagnostic {
    match error {
        VerifiedPathError::SymlinkRejected => AgentAssetDiagnostic::SymlinkRejected {
            source_id: source_id.to_string(),
        },
        VerifiedPathError::OutsideAllowedRoot => AgentAssetDiagnostic::SourceOutsideAllowedRoot {
            source_id: source_id.to_string(),
        },
        VerifiedPathError::TypeMismatch { actual } => AgentAssetDiagnostic::SourceTypeMismatch {
            source_id: source_id.to_string(),
            expected,
            actual,
        },
        error => AgentAssetDiagnostic::ReadFailed {
            source_id: source_id.to_string(),
            error_kind: io_error_kind(&error.into_io_error()),
        },
    }
}

pub(crate) fn revision_for_missing(path: &Path) -> AgentAssetRevision {
    AgentAssetRevision {
        identity: stable_id("missing", &[&lexical_identity(path)]),
        observed_at: now_string(),
        size_bytes: None,
        is_missing: true,
        is_directory: false,
        is_symlink: false,
    }
}

pub(crate) fn snapshot_revision_of(snapshot: &AgentAssetSnapshot) -> AgentAssetRevision {
    match snapshot {
        AgentAssetSnapshot::Missing { revision }
        | AgentAssetSnapshot::File { revision, .. }
        | AgentAssetSnapshot::DirectoryManifest { revision, .. }
        | AgentAssetSnapshot::Blocked { revision, .. } => revision.clone(),
    }
}

fn revision_from_bytes(
    guard: &VerifiedPathGuard,
    metadata: &fs::Metadata,
    bytes: &[u8],
) -> AgentAssetRevision {
    let mut hasher = revision_hasher(guard.display_path(), metadata);
    guard.update_revision(&mut hasher);
    hasher.update(bytes);
    revision_from_hash(metadata, hasher)
}

fn revision_from_manifest(
    guard: &VerifiedPathGuard,
    metadata: &fs::Metadata,
    entries: &[AgentAssetDirectoryEntry],
) -> AgentAssetRevision {
    let mut hasher = revision_hasher(guard.display_path(), metadata);
    guard.update_revision(&mut hasher);
    for entry in entries {
        hasher.update(entry.name.as_bytes());
        hasher.update([0, entry.source_kind as u8, entry.is_symlink as u8]);
    }
    revision_from_hash(metadata, hasher)
}

fn revision_hasher(path: &Path, metadata: &fs::Metadata) -> Sha256 {
    let mut hasher = Sha256::new();
    hasher.update(lexical_identity(path).as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    hasher.update([
        metadata.is_dir() as u8,
        metadata.file_type().is_symlink() as u8,
    ]);
    hasher
}

fn revision_from_hash(metadata: &fs::Metadata, hasher: Sha256) -> AgentAssetRevision {
    AgentAssetRevision {
        identity: format!("revision:{:x}", hasher.finalize()),
        observed_at: now_string(),
        size_bytes: Some(metadata.len()),
        is_missing: false,
        is_directory: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
    }
}

#[cfg(test)]
pub(crate) fn snapshot_revision(path: &Path) -> Option<AgentAssetRevision> {
    let root = path.parent()?;
    let guard = match inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File) {
        Ok(guard) => guard,
        Err(VerifiedPathError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(revision_for_missing(path))
        }
        Err(_) => return None,
    };
    let metadata = guard.metadata().ok()?;
    let bytes = guard
        .read_file_bounded(AgentAssetLimits::HARD_CAP.bytes_per_source)
        .ok()?;
    Some(revision_from_bytes(&guard, &metadata, &bytes))
}

pub(crate) fn path_has_symlink_component(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

#[cfg(all(test, any(unix, windows)))]
pub(crate) fn directory_names_from_open_handle_after<F>(
    path: &Path,
    after_open: F,
) -> std::io::Result<Vec<String>>
where
    F: FnOnce(),
{
    directory_probe_from_open_handle_after(path, after_open).map(|probe| probe.names)
}

#[cfg(all(test, any(unix, windows)))]
pub(crate) struct DirectoryHandleProbe {
    pub(crate) names: Vec<String>,
    pub(crate) revalidated: bool,
}

#[cfg(all(test, any(unix, windows)))]
pub(crate) fn directory_probe_from_open_handle_after<F>(
    path: &Path,
    after_open: F,
) -> std::io::Result<DirectoryHandleProbe>
where
    F: FnOnce(),
{
    let directory = inspect_verified_path(&[path], path, path, AgentAssetSourceKind::Directory)
        .map_err(VerifiedPathError::into_io_error)?;
    after_open();
    let mut names = directory
        .read_entries()?
        .map(|entry| {
            entry.and_then(|entry| {
                entry
                    .name
                    .into_string()
                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    Ok(DirectoryHandleProbe {
        names,
        revalidated: directory.revalidate().is_ok(),
    })
}

pub(crate) fn system_time_millis(value: SystemTime) -> Option<u128> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

pub(crate) fn now_string() -> String {
    DateTime::<Local>::from(SystemTime::now()).to_rfc3339()
}

fn io_error_kind(error: &std::io::Error) -> AgentAssetIoErrorKind {
    match error.kind() {
        std::io::ErrorKind::NotFound => AgentAssetIoErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => AgentAssetIoErrorKind::PermissionDenied,
        std::io::ErrorKind::InvalidData => AgentAssetIoErrorKind::InvalidData,
        _ => AgentAssetIoErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn candidate(name: &str) -> std::io::Result<DirectoryEntryCandidate> {
        Ok(DirectoryEntryCandidate {
            name: OsString::from(name),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        })
    }

    #[test]
    fn collector_fails_closed_when_deadline_arrives_before_eof() {
        let checks = Cell::new(0_usize);
        let result = collect_directory_entries(
            vec![candidate("alpha"), candidate("bravo")].into_iter(),
            "source:test",
            &AgentAssetLimits::DEFAULT,
            |_| {
                let check = checks.get();
                checks.set(check + 1);
                (check == 1).then_some(AgentAssetDiagnostic::BudgetExceeded {
                    elapsed_ms: 1,
                    budget_ms: 1,
                })
            },
        );
        assert!(matches!(
            result,
            Err(AgentAssetDiagnostic::BudgetExceeded { .. })
        ));
    }

    #[test]
    fn collector_fails_closed_on_platform_reader_error() {
        let result = collect_directory_entries(
            vec![
                candidate("alpha"),
                Err(std::io::Error::from(std::io::ErrorKind::InvalidData)),
            ]
            .into_iter(),
            "source:test",
            &AgentAssetLimits::DEFAULT,
            |_| None,
        );
        assert!(matches!(
            result,
            Err(AgentAssetDiagnostic::ReadFailed {
                error_kind: AgentAssetIoErrorKind::InvalidData,
                ..
            })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_open_error_maps_to_symlink_rejected_diagnostic() {
        assert!(matches!(
            path_open_diagnostic("source:reparse", AgentAssetSourceKind::Directory, VerifiedPathError::SymlinkRejected),
            AgentAssetDiagnostic::SymlinkRejected { source_id }
                if source_id == "source:reparse"
        ));
    }
}
