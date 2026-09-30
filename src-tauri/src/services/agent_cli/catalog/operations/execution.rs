use super::{
    compose_files, CanonicalPlan, NativeMember, OperationCell, PreparedDistribution, Work,
};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::CatalogService,
        environment::mutation::{
            atomic, classify_outcome, prepare_request, ApplyEvidence, MutationVerification,
        },
    },
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

impl CatalogService {
    pub(super) fn run(&self, cell: &OperationCell) {
        for work in &cell.plan.work {
            if cell.canceled.load(Ordering::Acquire) {
                break;
            }
            if !self.version_matches(&cell.plan) {
                cell.finish_pending(
                    AgentAssetOperationOutcome::UnchangedConflict,
                    "共享库版本已变化，尚未执行目标保持不变",
                );
                break;
            }
            match work {
                Work::NativeCli(members) => self.run_cli(cell, members),
                Work::NativeFiles(members) => self.run_files(cell, members),
                Work::Distribution(targets) => self.run_distribution(cell, targets),
                Work::Hooks(group) => self.run_hooks(cell, group),
                Work::Removal(targets) => self.run_removal(cell, targets),
            }
        }
    }

    fn version_matches(&self, plan: &CanonicalPlan) -> bool {
        plan.request.expected_version.is_none_or(|expected| {
            self.repository
                .current(&plan.request.asset_id)
                .is_ok_and(|(_, current)| current.version == expected)
        })
    }

    fn run_cli(&self, cell: &OperationCell, members: &[NativeMember]) {
        let Some(member) = members.first() else {
            return;
        };
        let indexes = members
            .iter()
            .map(|member| member.index)
            .collect::<Vec<_>>();
        cell.phase(&indexes, AgentAssetOperationPhase::Revalidating);
        let operation = (|| {
            member
                .prepared
                .revalidate()
                .map_err(|error| error.message)?;
            let plan = self
                .native
                .plan_pinned(
                    &cell.actor,
                    member.request.clone(),
                    Arc::clone(&cell.plan.inspector),
                    &member.prepared.signature,
                )
                .map_err(|error| error.message)?;
            let operation = self
                .native
                .start(
                    &cell.actor,
                    &AgentAssetApplyRequest {
                        plan_token: plan.token,
                        asset_id: plan.asset_id,
                        action: plan.action,
                    },
                )
                .map_err(|error| error.message)?;
            // Every binding with the same native action owner observes this
            // one child. Publishing precedes the cancel check, closing the race
            // with cancel() reading children from the parent state.
            cell.update(|state| {
                for index in &indexes {
                    state.public.targets[*index].native_operation_id = Some(operation.id.clone());
                }
            });
            if cell.canceled.load(Ordering::Acquire) {
                let _ = self.native.cancel(&cell.actor, &operation.id);
            }
            self.native.run_operation(&operation.id);
            self.native
                .operation(&cell.actor, &operation.id)
                .map_err(|error| error.message)
        })();
        match operation {
            Ok(operation) => cell.result(
                &indexes,
                operation
                    .outcome
                    .unwrap_or(AgentAssetOperationOutcome::OutcomeUnknown),
                operation.message,
            ),
            Err(message) => {
                let started = cell.state.lock().map_or(true, |state| {
                    indexes
                        .iter()
                        .any(|index| state.public.targets[*index].native_operation_id.is_some())
                });
                cell.result(
                    &indexes,
                    if started {
                        AgentAssetOperationOutcome::OutcomeUnknown
                    } else if cell.canceled.load(Ordering::Acquire) {
                        AgentAssetOperationOutcome::CanceledBeforeCommit
                    } else {
                        AgentAssetOperationOutcome::UnchangedConflict
                    },
                    Some(message),
                );
            }
        }
    }

    fn run_files(&self, cell: &OperationCell, members: &[NativeMember]) {
        let indexes = members
            .iter()
            .map(|member| member.index)
            .collect::<Vec<_>>();
        cell.phase(&indexes, AgentAssetOperationPhase::WaitingForLock);
        let locks = self.native.domain_locks();
        let domains = members
            .iter()
            .flat_map(|member| member.prepared.domains.clone())
            .collect::<Vec<_>>();
        let guard = locks.acquire(
            &domains,
            &cell.canceled,
            Instant::now() + Duration::from_secs(15),
        );
        let Ok(_guard) = guard else {
            cell.result(
                &indexes,
                if cell.canceled.load(Ordering::Acquire) {
                    AgentAssetOperationOutcome::CanceledBeforeCommit
                } else {
                    AgentAssetOperationOutcome::UnchangedFailure
                },
                Some("等待原生写域超时或已取消".to_owned()),
            );
            return;
        };
        cell.phase(&indexes, AgentAssetOperationPhase::Revalidating);
        let prepared = (|| {
            let current = cell.plan.inspector.inspect().map_err(|_| "原生盘点失败")?;
            for member in members {
                member
                    .prepared
                    .revalidate()
                    .map_err(|_| "原生源文件已变化")?;
                let (fresh, _) =
                    prepare_request(cell.plan.inspector.as_ref(), &current, &member.request)
                        .map_err(|_| "原生能力或内容已变化")?;
                if fresh.signature != member.prepared.signature {
                    return Err("原生计划已变化".to_owned());
                }
            }
            compose_files(members)
        })();
        let writes = match prepared {
            Ok(writes) => writes,
            Err(message) => {
                cell.result(
                    &indexes,
                    if cell.canceled.load(Ordering::Acquire) {
                        AgentAssetOperationOutcome::CanceledBeforeCommit
                    } else {
                        AgentAssetOperationOutcome::UnchangedConflict
                    },
                    Some(message),
                );
                return;
            }
        };
        let mut applied = Vec::new();
        for write in writes {
            if cell.canceled.load(Ordering::Acquire) {
                break;
            }
            cell.phase(&write.indexes, AgentAssetOperationPhase::Applying);
            let result = atomic::replace(&write.file, &write.bytes, || {
                // Recheck every dependency immediately before this file's
                // commit. Previous writes never authorize stale later ones.
                for member in members
                    .iter()
                    .filter(|member| write.indexes.contains(&member.index))
                {
                    member.prepared.revalidate()?;
                }
                cell.before_commit(&write.indexes)
            });
            let (evidence, message) = match result {
                #[cfg(unix)]
                Ok(atomic::AtomicWriteResult::Unchanged) => (ApplyEvidence::Noop, None),
                #[cfg(unix)]
                Ok(atomic::AtomicWriteResult::Replaced) => (ApplyEvidence::FileReplaced, None),
                #[cfg(unix)]
                Ok(atomic::AtomicWriteResult::ReplacedNotSynced) => (
                    ApplyEvidence::FileReplacedNotSynced,
                    Some("文件已替换但目录同步失败，请重新检查".to_owned()),
                ),
                Err(error)
                    if !cell.committed(&write.indexes)
                        && (cell.canceled.load(Ordering::Acquire)
                            || error.kind == AgentAssetMutationErrorKind::SourceConflict) =>
                {
                    cell.result(
                        &write.indexes,
                        if cell.canceled.load(Ordering::Acquire) {
                            AgentAssetOperationOutcome::CanceledBeforeCommit
                        } else {
                            AgentAssetOperationOutcome::UnchangedConflict
                        },
                        Some(error.message),
                    );
                    continue;
                }
                Err(error) => (ApplyEvidence::KnownFailure, Some(error.message)),
            };
            cell.phase(&write.indexes, AgentAssetOperationPhase::Verifying);
            applied.push((write, evidence, message));
        }
        if applied.is_empty() {
            return;
        }
        // One bounded final inventory verifies only files actually attempted.
        // Untouched later targets keep their own cancellation/conflict outcome.
        let snapshot = cell.plan.inspector.inspect().ok();
        for (write, evidence, message) in applied {
            let observation = write.file.observe_write();
            for member in members
                .iter()
                .filter(|member| write.indexes.contains(&member.index))
            {
                let verified = snapshot.as_ref().is_some_and(|snapshot| {
                    (member.prepared.verify)(MutationVerification {
                        inventory: &snapshot.inventory,
                        asset_id: &member.request.asset_id,
                        action: member.request.action,
                        affected_asset_ids: &member.prepared.affected_asset_ids,
                    })
                });
                cell.result(
                    &[member.index],
                    classify_outcome(evidence, verified, observation),
                    message.clone(),
                );
            }
        }
    }

    fn run_distribution(&self, cell: &OperationCell, targets: &[(usize, PreparedDistribution)]) {
        let indexes = targets.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        let Some((_, first)) = targets.first() else {
            return;
        };
        cell.phase(&indexes, AgentAssetOperationPhase::WaitingForLock);
        let locks = self.native.domain_locks();
        let domains = targets
            .iter()
            .flat_map(|(_, plan)| plan.domains.clone())
            .collect::<Vec<_>>();
        let mut native_verified = false;
        let result = (|| {
            let _guard = locks
                .acquire(
                    &domains,
                    &cell.canceled,
                    Instant::now() + Duration::from_secs(15),
                )
                .map_err(|_| "等待写域超时或已取消")?;
            cell.phase(&indexes, AgentAssetOperationPhase::Revalidating);
            for (_, target) in targets {
                target.revalidate()?;
            }
            let snapshot = cell
                .plan
                .inspector
                .inspect()
                .map_err(|_| "提交前原生盘点失败")?;
            let catalog = self.catalog(&snapshot)?;
            let item = catalog
                .assets
                .iter()
                .find(|asset| asset.id == cell.plan.request.asset_id)
                .ok_or("共享资产已变化")?;
            let source = self
                .repository
                .distribution_source(&cell.plan.request.asset_id)?;
            let definition = &source.definition;
            if Some(definition.version) != cell.plan.request.expected_version {
                return Err("共享定义版本已变化".to_owned());
            }
            for (index, saved) in targets {
                let target = catalog
                    .targets
                    .iter()
                    .find(|target| target.id == cell.plan.request.target_ids[*index])
                    .ok_or("目标已离开当前配置上下文")?;
                let fresh = PreparedDistribution::prepare(
                    &snapshot,
                    cell.plan.inspector.as_ref(),
                    item,
                    target,
                    &source.name,
                    definition,
                    source.receipts.get(&target.id),
                )?;
                if fresh.signature != saved.signature {
                    return Err("目标安装、信任、能力或内容已变化，请重新计划".to_owned());
                }
            }
            // Hold the private library transaction through native commit and
            // receipt publication so a concurrent editor cannot change the
            // pinned version between the check and the actual writes.
            self.repository.transact(|library| {
                let entry = library
                    .entries
                    .get_mut(&cell.plan.request.asset_id)
                    .ok_or("共享定义不存在")?;
                if !entry.current().is_some_and(|current| {
                    current.version == definition.version
                        && current.payload.fingerprint() == definition.payload.fingerprint()
                }) {
                    return Err("共享定义版本已变化".to_owned());
                }
                cell.phase(&indexes, AgentAssetOperationPhase::Applying);
                PreparedDistribution::apply_group(targets, || cell.before_commit(&indexes))?;
                cell.phase(&indexes, AgentAssetOperationPhase::Verifying);
                for (_, target) in targets {
                    if !target.verify()? {
                        return Err("写入后完整定义验证不一致".to_owned());
                    }
                }
                native_verified = true;
                for (index, target) in targets {
                    entry.receipts.insert(
                        cell.plan.request.target_ids[*index].clone(),
                        target.receipt(),
                    );
                }
                Ok(())
            })
        })();
        match result {
            Ok(()) => cell.result(
                &indexes,
                AgentAssetOperationOutcome::AppliedVerified,
                Some("原生定义已写入并验证；生效与信任状态由 Agent 原生策略决定".to_owned()),
            ),
            Err(message) => {
                let outcome = if native_verified {
                    AgentAssetOperationOutcome::AppliedUnverified
                } else if cell.committed(&indexes) {
                    classify_outcome(ApplyEvidence::KnownFailure, false, first.observe_write())
                } else if cell.canceled.load(Ordering::Acquire) {
                    AgentAssetOperationOutcome::CanceledBeforeCommit
                } else {
                    AgentAssetOperationOutcome::UnchangedConflict
                };
                cell.result(&indexes, outcome, Some(message));
            }
        }
    }
}
