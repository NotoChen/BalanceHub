//! Bounded operation metadata only. Receipts never contain source/candidate
//! text or secret values, and recovery never replays a native write.
use super::{
    errors::{conflict, internal},
    operations::OperationCell,
    service::{at_after, digest},
    writer::{CommitResult, StagedFile},
    ConfigurationService,
};
use crate::{
    models::*,
    services::agent_cli::{
        config_support::directories,
        environment::{mutation::GuardedFile, verified_path::inspect_verified_path},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{atomic::Ordering, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileFingerprint {
    pub source_id: String,
    pub before: Option<String>,
    pub after: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
    version: u8,
    pub operation: AgentConfigurationOperation,
    pub files: Vec<FileFingerprint>,
    commit_attempted: bool,
}
impl Receipt {
    pub fn recovered(&self) -> AgentConfigurationOperation {
        let mut operation = self.operation.clone();
        if operation.phase != AgentAssetOperationPhase::Completed {
            operation.phase = AgentAssetOperationPhase::Completed;
            operation.can_cancel = false;
            operation.revision = operation.revision.saturating_add(1);
            if self.commit_attempted {
                operation.outcome = Some(AgentAssetOperationOutcome::OutcomeUnknown);
                operation.message = Some(
                    "应用重启后保存结果尚未确定；刷新来源可进行有界核对，不会自动重试".to_owned(),
                );
                for file in &mut operation.files {
                    if file.state != AgentConfigurationFileState::Applied {
                        file.state = AgentConfigurationFileState::Unknown;
                    }
                }
            } else {
                operation.outcome = Some(AgentAssetOperationOutcome::UnchangedFailure);
                operation.message = Some("操作在提交文件前中断，配置未写入".to_owned());
            }
        }
        operation
    }
}
pub(super) struct ReceiptStore {
    root: PathBuf,
    writing: Mutex<()>,
}
impl ReceiptStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            writing: Mutex::new(()),
        }
    }
    pub fn save(&self, cell: &OperationCell) -> Result<(), AgentConfigurationError> {
        let operation = cell.public.lock().map_err(|_| internal())?.clone();
        self.save_receipt(&Receipt {
            version: 1,
            operation,
            files: cell.fingerprints.clone(),
            commit_attempted: cell.committed.load(Ordering::Acquire),
        })
    }
    fn save_receipt(&self, receipt: &Receipt) -> Result<(), AgentConfigurationError> {
        let _writing = self.writing.lock().map_err(|_| internal())?;
        if !valid_id(&receipt.operation.id) {
            return Err(internal());
        }
        let parent = self.root.parent().ok_or_else(internal)?;
        let path = self.root.join(format!("{}.json", receipt.operation.id));
        let bytes = serde_json::to_vec(receipt).map_err(|_| internal())?;
        if bytes.len() > 1024 * 1024 {
            return Err(internal());
        }
        let mut source =
            GuardedFile::capture_path(parent, &path, 1024 * 1024).map_err(|_| conflict())?;
        let ancestor = source.creation_ancestor().map(std::path::Path::to_path_buf);
        if let Some(ancestor) = ancestor {
            directories::ensure_directory(&ancestor, &self.root).map_err(|_| internal())?;
            source
                .reanchor_after_parent_creation(&self.root)
                .map_err(|_| conflict())?;
        }
        let mut staged = StagedFile::prepare(&source, &bytes, || Ok(()))?;
        if matches!(staged.commit(|| Ok(()))?, CommitResult::AppliedNotSynced) {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::WriteFailed,
            ));
        }
        staged.verify()?;
        self.prune();
        Ok(())
    }
    pub fn load(&self) -> Vec<Receipt> {
        let Some(parent) = self.root.parent() else {
            return Vec::new();
        };
        let Ok(directory) = inspect_verified_path(
            &[parent],
            parent,
            &self.root,
            AgentAssetSourceKind::Directory,
        ) else {
            return Vec::new();
        };
        let Ok(entries) = directory.read_entries() else {
            return Vec::new();
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut receipts = Vec::new();
        let mut total = 0usize;
        for entry in entries.take(256).flatten() {
            if Instant::now() >= deadline || total >= super::MAX_PRIVATE_BYTES {
                break;
            }
            if entry.is_symlink || entry.source_kind != AgentAssetSourceKind::File {
                continue;
            }
            let path = self.root.join(&entry.name);
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if path.extension().and_then(|value| value.to_str()) != Some("json") || !valid_id(id) {
                continue;
            }
            let Ok(file) = GuardedFile::capture_path(parent, &path, 1024 * 1024) else {
                continue;
            };
            let Some(bytes) = file.bytes() else { continue };
            total = total.saturating_add(bytes.len());
            let Ok(receipt) = serde_json::from_slice::<Receipt>(bytes) else {
                continue;
            };
            if receipt.version == 1
                && receipt.operation.id == id
                && receipt.files.len() <= 4
                && receipt.operation.files.len() == receipt.files.len()
            {
                receipts.push(receipt);
            }
        }
        if directory.revalidate_identity().is_err() {
            return Vec::new();
        };
        receipts.sort_by(|a, b| b.operation.updated_at.cmp(&a.operation.updated_at));
        receipts.truncate(128);
        receipts
    }
    fn prune(&self) {
        let Some(parent) = self.root.parent() else {
            return;
        };
        let Ok(directory) = inspect_verified_path(
            &[parent],
            parent,
            &self.root,
            AgentAssetSourceKind::Directory,
        ) else {
            return;
        };
        let Ok(entries) = directory.read_entries() else {
            return;
        };
        let mut files = entries
            .take(256)
            .flatten()
            .filter_map(|entry| {
                if entry.is_symlink || entry.source_kind != AgentAssetSourceKind::File {
                    return None;
                }
                let path = self.root.join(&entry.name);
                let id = path.file_stem()?.to_str()?;
                if !valid_id(id)
                    || path.extension().and_then(|value| value.to_str()) != Some("json")
                {
                    return None;
                }
                let source = GuardedFile::capture_metadata_path(parent, &path).ok()?;
                let guard = source.reopen_metadata().ok()?;
                let modified = guard.metadata().ok()?.modified().ok();
                Some((modified, source))
            })
            .collect::<Vec<_>>();
        files.sort_by_key(|file| std::cmp::Reverse(file.0));
        for (_, source) in files.into_iter().skip(128) {
            if directory.revalidate_identity().is_err() || source.revalidate().is_err() {
                continue;
            }
            #[cfg(unix)]
            {
                let _ = rustix::fs::unlinkat(
                    directory.source_handle(),
                    source.path().file_name().unwrap_or_default(),
                    rustix::fs::AtFlags::empty(),
                );
            }
            #[cfg(windows)]
            {
                let _ = std::fs::remove_file(source.path());
            }
        }
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|value| value.is_ascii_hexdigit())
}

impl ConfigurationService {
    pub(super) fn cache_recovered_receipts(
        &self,
        receipts: &[Receipt],
    ) -> Result<(), AgentConfigurationError> {
        let mut state = self.state.lock().map_err(|_| internal())?;
        for receipt in receipts {
            if !state.operations.contains_key(&receipt.operation.id) {
                state
                    .recovered
                    .entry(receipt.operation.id.clone())
                    .or_insert_with(|| receipt.recovered());
            }
        }
        prune_recovered(&mut state.recovered);
        Ok(())
    }

    pub(super) fn reconcile_receipts(&self, actor: &str, snapshot: &AgentConfigurationSnapshot) {
        let receipts = self.receipts.load();
        if self.cache_recovered_receipts(&receipts).is_err() {
            return;
        }
        let live = self
            .state
            .lock()
            .map(|state| {
                state
                    .operations
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut hashes = BTreeMap::new();
        for source in &snapshot.sources {
            let AgentAssetAccess::Ready { access_id } = &source.access else {
                continue;
            };
            if let Ok(anchor) = self.access.resolve_configuration(
                actor,
                &AgentConfigurationSourceRequest {
                    source_id: source.source_id.clone(),
                    access_id: access_id.clone(),
                    environment_id: source.environment_id.clone(),
                    workspace: source.workspace.clone(),
                    expected_revision: source.revision.identity.clone(),
                },
            ) {
                hashes.insert(
                    source.source_id.clone(),
                    anchor.authority.file.bytes().map(digest),
                );
            }
        }
        for receipt in receipts {
            if live.contains(&receipt.operation.id)
                || receipt.operation.phase == AgentAssetOperationPhase::Completed
                || receipt.operation.agent_kind != snapshot.agent_kind
            {
                continue;
            }
            let mut operation = receipt.recovered();
            let mut verified = 0;
            let mut unchanged = 0;
            for fingerprint in &receipt.files {
                let Some(current) = hashes.get(&fingerprint.source_id) else {
                    continue;
                };
                if let Some(file) = operation
                    .files
                    .iter_mut()
                    .find(|file| file.source_id == fingerprint.source_id)
                {
                    if current.as_deref() == Some(&fingerprint.after) {
                        file.state = if fingerprint.before.as_deref() == Some(&fingerprint.after) {
                            AgentConfigurationFileState::Unchanged
                        } else {
                            AgentConfigurationFileState::Applied
                        };
                        verified += 1;
                    } else if current == &fingerprint.before {
                        file.state = AgentConfigurationFileState::Unchanged;
                        unchanged += 1;
                    } else {
                        file.state = AgentConfigurationFileState::Unknown;
                    }
                }
            }
            if verified + unchanged != receipt.files.len() {
                continue;
            }
            operation.phase = AgentAssetOperationPhase::Completed;
            operation.can_cancel = false;
            operation.outcome = Some(if verified == receipt.files.len() {
                AgentAssetOperationOutcome::AppliedUnverified
            } else if unchanged == receipt.files.len() {
                AgentAssetOperationOutcome::UnchangedFailure
            } else {
                AgentAssetOperationOutcome::AppliedUnverified
            });
            operation.message = Some(
                "已按当前原生来源核对重启前的保存结果；未重放写入，历史持久化状态仍以该结果为限"
                    .to_owned(),
            );
            // Queries reconcile metadata in memory. They never create receipt
            // directories or rewrite a historical transaction.
            if let Ok(mut state) = self.state.lock() {
                if state.operations.contains_key(&operation.id) {
                    continue;
                }
                let current = state
                    .recovered
                    .entry(operation.id.clone())
                    .or_insert_with(|| receipt.recovered());
                if !same_result(current, &operation) {
                    operation.revision = current
                        .revision
                        .max(receipt.operation.revision)
                        .saturating_add(1);
                    operation.updated_at = at_after(Duration::ZERO);
                    *current = operation;
                }
                prune_recovered(&mut state.recovered);
            }
        }
    }
}

fn same_result(left: &AgentConfigurationOperation, right: &AgentConfigurationOperation) -> bool {
    left.phase == right.phase
        && left.outcome == right.outcome
        && left.can_cancel == right.can_cancel
        && left.message == right.message
        && left.files.len() == right.files.len()
        && left.files.iter().zip(&right.files).all(|(left, right)| {
            left.source_id == right.source_id
                && left.state == right.state
                && left.message == right.message
        })
}

fn prune_recovered(operations: &mut BTreeMap<String, AgentConfigurationOperation>) {
    while operations.len() > 128 {
        let oldest = operations
            .values()
            .min_by(|a, b| a.updated_at.cmp(&b.updated_at))
            .map(|operation| operation.id.clone());
        let Some(oldest) = oldest else { break };
        operations.remove(&oldest);
    }
}
