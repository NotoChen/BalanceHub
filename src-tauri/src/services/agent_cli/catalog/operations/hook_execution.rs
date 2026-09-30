use super::super::{
    digest,
    hook_receipts::{self, HookIntent},
    opaque_id, package, CatalogService,
};
use super::{
    hooks::{HookMember, PreparedHookGroup},
    OperationCell,
};
#[cfg(unix)]
use crate::services::agent_cli::environment::mutation::atomic;
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::mutation::{
            classify_outcome, ApplyEvidence, MutationInventory, WriteObservation,
        },
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

impl CatalogService {
    pub(super) fn run_hooks(&self, cell: &OperationCell, saved: &PreparedHookGroup) {
        let indexes = saved
            .members
            .iter()
            .map(|member| member.index)
            .collect::<Vec<_>>();
        cell.phase(&indexes, AgentAssetOperationPhase::WaitingForLock);
        let locks = self.native.domain_locks();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut verified_native = false;
        #[cfg(unix)]
        let mut replaced = BTreeSet::<usize>::new();
        #[cfg(not(unix))]
        let replaced = BTreeSet::<usize>::new();
        #[cfg(unix)]
        let mut not_synced = BTreeSet::<usize>::new();
        #[cfg(not(unix))]
        let not_synced = BTreeSet::<usize>::new();
        let result = (|| {
            let _guard = locks
                .acquire(
                    &saved.domains,
                    &cell.canceled,
                    Instant::now() + Duration::from_secs(15),
                )
                .map_err(|_| "等待 Hook 写域超时或已取消")?;
            if cell.canceled.load(Ordering::Acquire) {
                return Err("已取消尚未提交的 Hook 操作".to_owned());
            }
            cell.phase(&indexes, AgentAssetOperationPhase::Revalidating);
            saved.revalidate()?;
            let snapshot = cell
                .plan
                .inspector
                .inspect()
                .map_err(|_| "提交前 Hook 盘点失败")?;
            before_deadline(deadline)?;
            let catalog = self.catalog(&snapshot)?;
            let item = catalog
                .assets
                .iter()
                .find(|asset| asset.id == cell.plan.request.asset_id)
                .ok_or("Hook 全局资产已变化")?;
            let entry = self.repository.transact(|library| {
                library
                    .entries
                    .get(&item.id)
                    .cloned()
                    .ok_or_else(|| "Hook 资产已变化".to_owned())
            })?;
            if entry.current().map(|value| value.version) != cell.plan.request.expected_version {
                return Err("共享 Hook 版本已变化".to_owned());
            }
            let members = saved
                .members
                .iter()
                .map(|member| {
                    HookMember::prepare(
                        &snapshot,
                        item,
                        &entry,
                        &member.target_id,
                        cell.plan.request.action,
                        member.index,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut fresh =
                PreparedHookGroup::prepare(self, &snapshot, cell.plan.inspector.as_ref(), members)?;
            if fresh.signature != saved.signature {
                return Err("Hook 原生配置、能力或恢复记录已变化，请重新计划".to_owned());
            }
            fresh.revalidate()?;
            before_deadline(deadline)?;
            // Native documents first; auxiliary ownership follows under the
            // same locks. A partial outcome retains the durable complete intent.
            #[cfg(not(unix))]
            if !fresh.writes.is_empty() {
                return Err("当前平台不支持原子写入 Hook 配置".to_owned());
            }
            fresh.writes.sort_by_key(|write| write.source_id.is_none());
            self.repository.checkpointed(|library, checkpoint| {
                let entry = library.entries.get(&item.id).ok_or("Hook 全局资产已变化")?;
                if entry.current().map(|value| value.version) != cell.plan.request.expected_version
                {
                    return Err("共享 Hook 版本已变化".to_owned());
                }
                for member in &fresh.members {
                    if let Some(change) = &member.transition {
                        let current = entry.receipts.get(&change.key);
                        if serde_json::to_vec(&current).map_err(|_| "Hook 恢复记录无效")?
                            != serde_json::to_vec(&change.before.as_ref())
                                .map_err(|_| "Hook 计划记录无效")?
                        {
                            return Err("Hook 恢复记录已变化".to_owned());
                        }
                    }
                }
                if !fresh.writes.is_empty() {
                    let intent_id = opaque_id()?;
                    let entry = library
                        .entries
                        .get_mut(&item.id)
                        .ok_or("Hook 全局资产已变化")?;
                    for member in &fresh.members {
                        if let Some(change) = &member.transition {
                            let files = fresh.evidence(member.index);
                            if files.is_empty() {
                                continue;
                            }
                            let mut pending = change.carrier.clone();
                            pending.hook.as_mut().ok_or("Hook 恢复副本缺失")?.pending =
                                Some(HookIntent {
                                    id: intent_id.clone(),
                                    before: change.before.clone().map(Box::new),
                                    after: change.after.clone().map(Box::new),
                                    files,
                                });
                            entry.receipts.insert(change.key.clone(), pending);
                        }
                    }
                    // A failing durable intent never reaches a native write.
                    checkpoint(library)?;
                }
                cell.phase(&indexes, AgentAssetOperationPhase::Applying);
                #[cfg(unix)]
                let mut completed_domains = BTreeMap::<String, Vec<u8>>::new();
                #[cfg(unix)]
                for write in &fresh.writes {
                    before_deadline(deadline)?;
                    revalidate_reads(&fresh, &completed_domains).map_err(|error| error.message)?;
                    let parent = write.file.path().parent().ok_or("Hook 目标目录无效")?;
                    package::ensure_directory_before(&write.root, parent, || {
                        before_deadline(deadline)?;
                        cell.before_commit(&write.indexes)
                            .map_err(|error| error.message)
                    })?;
                    for guard in fresh.reads.values_mut() {
                        // Committed aliases are checked against their written
                        // bytes, not reinterpreted as still-missing files.
                        if !guard
                            .domain()
                            .is_some_and(|domain| completed_domains.contains_key(&domain))
                        {
                            guard.reanchor_after_parent_creation(parent)?;
                        }
                    }
                    let current = package::capture_file(
                        &write.root,
                        write.file.path(),
                        package::MAX_FILE_BYTES,
                    )?;
                    if current.bytes() != write.file.bytes() {
                        return Err("Hook 目标在提交前被外部修改；已提交文件保留".to_owned());
                    }
                    checkpoint(library)?;
                    let result = atomic::replace(&current, &write.bytes, || {
                        revalidate_reads(&fresh, &completed_domains)?;
                        before_deadline(deadline).map_err(|_| {
                            AgentAssetMutationError::new(
                                AgentAssetMutationErrorKind::PreparationFailed,
                            )
                        })?;
                        cell.before_commit(&write.indexes)
                    })
                    .map_err(|error| error.message)?;
                    #[cfg(unix)]
                    {
                        match result {
                            atomic::AtomicWriteResult::Unchanged => {}
                            atomic::AtomicWriteResult::Replaced => replaced.extend(&write.indexes),
                            atomic::AtomicWriteResult::ReplacedNotSynced => {
                                replaced.extend(&write.indexes);
                                not_synced.extend(&write.indexes);
                            }
                        }
                        completed_domains.insert(write.file.domain(), write.bytes.clone());
                        #[cfg(test)]
                        if let Some(after_write) = self.after_hook_write.lock().unwrap().take() {
                            after_write();
                        }
                    }
                }
                let library_only = indexes
                    .iter()
                    .copied()
                    .filter(|index| {
                        !fresh
                            .writes
                            .iter()
                            .any(|write| write.indexes.contains(index))
                    })
                    .collect::<Vec<_>>();
                if !library_only.is_empty() {
                    // Applying v2 to a suspended binding modifies only its
                    // retained desired definition. It never re-enables it.
                    before_deadline(deadline)?;
                    cell.before_commit(&library_only)
                        .map_err(|error| error.message)?;
                }
                cell.phase(&indexes, AgentAssetOperationPhase::Verifying);
                let observed = cell
                    .plan
                    .inspector
                    .inspect()
                    .map_err(|_| "Hook 提交后无法完整盘点；恢复记录已保留")?;
                before_deadline(deadline)?;
                if !verify(&fresh, &observed) {
                    return Err(
                        "Hook 写入后的原生规则或状态未通过完整验证；不会自动重放".to_owned()
                    );
                }
                verified_native = true;
                let entry = library.entries.get_mut(&item.id).ok_or("Hook 资产已变化")?;
                for member in &fresh.members {
                    if let Some(change) = &member.transition {
                        if let Some(after) = &change.after {
                            entry.receipts.insert(change.key.clone(), after.clone());
                        } else {
                            entry.receipts.remove(&change.key);
                        }
                    }
                }
                hook_receipts::reconcile(
                    library,
                    &mut super::super::observation::DefinitionReader::new(&observed),
                );
                checkpoint(library)
            })
        })();
        match result {
            Ok(()) => {
                for index in &indexes {
                    cell.result(
                        &[*index],
                        if not_synced.contains(index) {
                            AgentAssetOperationOutcome::AppliedUnverified
                        } else {
                            AgentAssetOperationOutcome::AppliedVerified
                        },
                        Some(
                            if not_synced.contains(index) {
                                "Hook 已写入并检查，但目录同步失败；请刷新核实"
                            } else {
                                "所选 Hook 变更已保存并验证；原生信任和其他规则保持原有约束"
                            }
                            .to_owned(),
                        ),
                    );
                }
            }
            Err(message) => {
                for index in &indexes {
                    let observation = observe(saved, *index);
                    let outcome = if verified_native || replaced.contains(index) {
                        AgentAssetOperationOutcome::AppliedUnverified
                    } else if cell.canceled.load(Ordering::Acquire) && !cell.committed(&[*index]) {
                        AgentAssetOperationOutcome::CanceledBeforeCommit
                    } else if cell.committed(&[*index]) {
                        classify_outcome(ApplyEvidence::KnownFailure, false, observation)
                    } else {
                        AgentAssetOperationOutcome::UnchangedConflict
                    };
                    cell.result(&[*index], outcome, Some(message.clone()));
                }
            }
        }
    }
}

#[cfg(unix)]
fn revalidate_reads(
    group: &PreparedHookGroup,
    completed_domains: &BTreeMap<String, Vec<u8>>,
) -> Result<(), AgentAssetMutationError> {
    for guard in group.reads.values() {
        guard
            .domain()
            .as_ref()
            .and_then(|domain| completed_domains.get(domain))
            .map_or_else(
                || guard.revalidate(),
                |bytes| guard.revalidate_after_write(bytes),
            )
            .map_err(|_| {
                AgentAssetMutationError::new(AgentAssetMutationErrorKind::SourceConflict)
            })?;
    }
    Ok(())
}

fn verify(group: &PreparedHookGroup, snapshot: &MutationInventory) -> bool {
    let mut sources = BTreeMap::new();
    for (kind, expectation) in &group.expectations {
        let key = (*kind, expectation.source_id.clone());
        if !sources.contains_key(&key) {
            let Some(adapter) = definition(*kind).environment.hook_adapter() else {
                return false;
            };
            let Ok(rules) = (adapter.inspect_source)(snapshot, &expectation.source_id) else {
                return false;
            };
            sources.insert(key.clone(), rules);
        }
        let Some(rules) = sources.get(&key) else {
            return false;
        };
        let matches = rules
            .iter()
            .filter(|rule| rule.definition == expectation.definition)
            .collect::<Vec<_>>();
        if matches.len() != expectation.occurrences
            || expectation
                .enabled
                .is_some_and(|enabled| matches.iter().any(|rule| rule.enabled != enabled))
        {
            return false;
        }
    }
    // Auxiliary ownership is also part of the confirmed operation. Native
    // success alone cannot certify a stale session-integration manifest.
    group
        .writes
        .iter()
        .filter(|write| write.source_id.is_none())
        .all(|write| {
            package::capture_file(&write.root, write.file.path(), package::MAX_FILE_BYTES)
                .is_ok_and(|file| {
                    file.bytes()
                        .is_some_and(|bytes| digest(bytes) == digest(&write.bytes))
                })
        })
}

fn before_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Hook 操作超时；已提交的原生文件保留，未提交目标不再写入".to_owned())
    } else {
        Ok(())
    }
}

fn observe(group: &PreparedHookGroup, index: usize) -> WriteObservation {
    let observations = group
        .writes
        .iter()
        .filter(|write| write.indexes.contains(&index))
        .map(|write| write.file.observe_write())
        .collect::<Vec<_>>();
    if observations.contains(&WriteObservation::Changed) {
        WriteObservation::Changed
    } else if observations.contains(&WriteObservation::Unknown) {
        WriteObservation::Unknown
    } else {
        WriteObservation::Unchanged
    }
}
