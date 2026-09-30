use super::{OperationCell, PreparedRemoval};
use crate::{models::*, services::agent_cli::catalog::CatalogService};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

impl CatalogService {
    pub(super) fn run_removal(&self, cell: &OperationCell, targets: &[(usize, PreparedRemoval)]) {
        let indexes = targets.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        let domains = targets
            .iter()
            .flat_map(|(_, target)| target.domains.clone())
            .collect::<Vec<_>>();
        let locks = self.native.domain_locks();
        let deadline = Instant::now() + Duration::from_secs(60);
        cell.phase(&indexes, AgentAssetOperationPhase::WaitingForLock);
        let result = (|| {
            let _guard = locks
                .acquire(
                    &domains,
                    &cell.canceled,
                    Instant::now() + Duration::from_secs(15),
                )
                .map_err(|_| "等待移除写域超时或已取消")?;
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
                .ok_or("资产来源已变化，请刷新后重新选择")?;
            for (index, saved) in targets {
                let fresh = PreparedRemoval::prepare(
                    &snapshot,
                    cell.plan.inspector.as_ref(),
                    item,
                    &cell.plan.request.target_ids[*index],
                )?;
                if fresh.signature != saved.signature {
                    return Err("移除内容、配置来源或共享影响范围已变化，请重新预览".to_owned());
                }
            }
            // Serialize receipt cleanup with library edits. Removing a native
            // binding never creates or removes a shared definition/version.
            self.repository.transact(|library| {
                let entry = library.entries.get(&item.id).ok_or("资产已离开目录")?;
                if entry.current().map(|value| value.version) != cell.plan.request.expected_version
                {
                    return Err("共享定义版本已变化，请重新预览".to_owned());
                }
                cell.phase(&indexes, AgentAssetOperationPhase::Applying);
                PreparedRemoval::apply_group(targets, || {
                    if Instant::now() >= deadline {
                        return Err(AgentAssetMutationError::new(
                            AgentAssetMutationErrorKind::PreparationFailed,
                        ));
                    }
                    cell.before_commit(&indexes)
                })?;
                cell.phase(&indexes, AgentAssetOperationPhase::Verifying);
                for (_, target) in targets {
                    if !target.verify()? {
                        return Err("尚未确认所选配置已完整移除，请刷新核实".to_owned());
                    }
                }
                for entry in library
                    .entries
                    .values_mut()
                    .filter(|entry| entry.category == item.category)
                {
                    entry.receipts.retain(|_, receipt| {
                        !targets
                            .iter()
                            .any(|(_, target)| target.matches_receipt(receipt))
                    });
                }
                Ok(())
            })
        })();
        match result {
            Ok(()) => {
                self.clear_unpersisted_observations();
                cell.result(
                    &indexes,
                    AgentAssetOperationOutcome::AppliedVerified,
                    Some("所选原生配置已移除并验证；共享库中的定义保留".to_owned()),
                );
            }
            Err(message) => {
                let committed = cell.committed(&indexes);
                if committed {
                    self.clear_unpersisted_observations();
                }
                let outcome = if committed {
                    // A Skill package can be partially removed. Never report
                    // unchanged or retry automatically after a commit boundary.
                    AgentAssetOperationOutcome::AppliedUnverified
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
