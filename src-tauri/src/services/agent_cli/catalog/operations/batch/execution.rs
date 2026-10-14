use super::*;
use crate::services::agent_cli::{
    catalog::hook_receipts::HookIntent, environment::mutation::MutationVerification,
};
use std::time::Duration;

impl CatalogService {
    pub(super) fn run_batch(
        &self,
        mut batch: Batch,
        inspector: Arc<dyn MutationInspector>,
        context: &maintenance::TaskContext,
    ) -> Result<(), String> {
        let actor = format!("batch:{}", context.id());
        let locks = self.native.domain_locks();
        let file_result = (|| {
            let _guard = if batch.domains.is_empty() {
                None
            } else {
                Some(
                    locks
                        .acquire(&batch.domains, context.canceled(), context.deadline())
                        .map_err(|_| "等待批量配置可写超时或已取消")?,
                )
            };
            context.check()?;
            context.progress("核对所有目标的当前配置");
            let snapshot = inspector.inspect().map_err(|error| error.message)?;
            for asset in &batch.assets {
                asset.source.revalidate(&snapshot)?;
                let fresh = prepare::prepare_work(
                    self,
                    &snapshot,
                    inspector.as_ref(),
                    (&asset.source.item, &asset.source.entry),
                    &batch.targets,
                    &asset.resolved,
                    None,
                )?;
                if fresh.rows.iter().any(|row| !row.available)
                    || super::super::source_commit::signature(&fresh.work)? != asset.signature
                {
                    return Err(format!(
                        "{} 的来源、目标或能力已变化，请重新预览",
                        asset.source.item.name
                    ));
                }
            }
            if let Some(hooks) = &batch.hooks {
                hooks.revalidate()?;
            }
            self.repository.checkpointed(|library, checkpoint| {
                for asset in &batch.assets {
                    asset.source.save_into(library)?;
                    library
                        .entries
                        .entry(asset.source.item.id.clone())
                        .or_insert_with(|| asset.source.entry.clone());
                }
                if let Some(hooks) = &batch.hooks {
                    for member in &hooks.members {
                        if let Some(change) = &member.transition {
                            let files = hooks.evidence(member.index);
                            if files.is_empty() {
                                continue;
                            }
                            let mut pending = change.carrier.clone();
                            pending.hook.as_mut().ok_or("Hook 恢复记录缺失")?.pending =
                                Some(HookIntent {
                                    id: opaque_id()?,
                                    before: change.before.clone().map(Box::new),
                                    after: change.after.clone().map(Box::new),
                                    files,
                                });
                            library
                                .entries
                                .get_mut(&batch.owners[member.index].0)
                                .ok_or("Hook 资产不存在")?
                                .receipts
                                .insert(change.key.clone(), pending);
                        }
                    }
                }
                context.check()?;
                checkpoint(library)?;
                let applied = (|| {
                    files::apply_locked(&mut batch.writes, context)?;
                    for targets in batch.removals.values() {
                        if PreparedRemoval::batch_write(targets)?.is_none() {
                            PreparedRemoval::apply_group(targets, || {
                                context.check().map_err(|_| {
                                    AgentAssetMutationError::new(
                                        AgentAssetMutationErrorKind::PreparationFailed,
                                    )
                                })?;
                                context.changed();
                                Ok(())
                            })?;
                        }
                    }
                    Ok(())
                })();
                // Read back even after a partial failure; successful targets
                // retain receipts while unfinished hook intents remain durable.
                let observed = inspector.inspect().map_err(|error| error.message)?;
                for member in &batch.native {
                    let verified = (member.prepared.verify)(MutationVerification {
                        inventory: &observed.inventory,
                        asset_id: &member.request.asset_id,
                        action: member.request.action,
                        affected_asset_ids: &member.prepared.affected_asset_ids,
                    });
                    result_item(context, &batch.owners, member.index, verified);
                }
                for (index, target) in &batch.distributions {
                    let verified = target.verify().unwrap_or(false);
                    if verified {
                        library
                            .entries
                            .get_mut(&batch.owners[*index].0)
                            .ok_or("资产不存在")?
                            .receipts
                            .insert(batch.owners[*index].1.clone(), target.receipt());
                    }
                    result_item(context, &batch.owners, *index, verified);
                }
                for (index, target) in batch.removals.values().flatten() {
                    let verified = target.verify().unwrap_or(false);
                    if verified {
                        for entry in library.entries.values_mut() {
                            entry
                                .receipts
                                .retain(|_, receipt| !target.matches_receipt(receipt));
                        }
                    }
                    result_item(context, &batch.owners, *index, verified);
                }
                if let Some(hooks) = &batch.hooks {
                    let verified = super::super::hook_execution::verify(hooks, &observed);
                    for member in &hooks.members {
                        if verified {
                            if let Some(change) = &member.transition {
                                let entry = library
                                    .entries
                                    .get_mut(&batch.owners[member.index].0)
                                    .ok_or("Hook 资产不存在")?;
                                if let Some(after) = &change.after {
                                    entry.receipts.insert(change.key.clone(), after.clone());
                                } else {
                                    entry.receipts.remove(&change.key);
                                }
                            }
                        }
                        result_item(context, &batch.owners, member.index, verified);
                    }
                }
                checkpoint(library)?;
                applied
            })
        })();
        file_result?;
        // Native CLIs keep their own process timeout, cancellation and verifier.
        // Replan after our confirmed writes, allowing only the known file states.
        let mut known = batch
            .writes
            .iter()
            .map(|write| (write.source.pathname_domain(), write.after.clone()))
            .collect::<BTreeMap<_, _>>();
        for member in batch.cli {
            context.check()?;
            let key = &batch.owners[member.index].2;
            context.item(key, AgentMaintenanceItemState::Running, None);
            let result = (|| {
                for file in &member.prepared.files {
                    let current = GuardedFile::capture_path(
                        file.allowed_root(),
                        file.path(),
                        crate::services::agent_cli::catalog::package::MAX_FILE_BYTES,
                    )
                    .map_err(|error| error.message)?;
                    let expected = known
                        .get(&file.pathname_domain())
                        .map(|bytes| bytes.as_deref())
                        .unwrap_or(file.bytes());
                    if current.bytes() != expected {
                        return Err("原生 CLI 所依赖的配置被其他操作修改，请重新预览".to_owned());
                    }
                }
                let snapshot = inspector.inspect().map_err(|error| error.message)?;
                let mut request = member.request.clone();
                request.expected_revision = snapshot
                    .inventory
                    .assets
                    .iter()
                    .find(|asset| asset.stable_id == request.asset_id)
                    .ok_or("原生目标已不存在")?
                    .revision
                    .identity
                    .clone();
                let (fresh, _) =
                    crate::services::agent_cli::environment::mutation::prepare_request(
                        inspector.as_ref(),
                        &snapshot,
                        &request,
                    )
                    .map_err(|error| error.message)?;
                let (MutationExecution::ExactCli(old), MutationExecution::ExactCli(next)) =
                    (&member.prepared.execution, &fresh.execution)
                else {
                    return Err("原生操作方式已变化".to_owned());
                };
                if old.argv != next.argv
                    || old.cwd != next.cwd
                    || old.environment != next.environment
                    || member.prepared.affected_asset_ids != fresh.affected_asset_ids
                {
                    return Err("原生 CLI 命令或影响范围已变化".to_owned());
                }
                let plan = self
                    .native
                    .plan_pinned(&actor, request, Arc::clone(&inspector), &fresh.signature)
                    .map_err(|error| error.message)?;
                let operation = self
                    .native
                    .start(
                        &actor,
                        &AgentAssetApplyRequest {
                            plan_token: plan.token,
                            asset_id: plan.asset_id,
                            action: plan.action,
                        },
                    )
                    .map_err(|error| error.message)?;
                std::thread::scope(|scope| {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let child_id = &operation.id;
                    let history_id = context.id();
                    scope.spawn(move || {
                        let _history =
                            crate::services::agent_cli::config_support::native_history::scope(
                                &history_id,
                                "批量更改资产配置",
                            );
                        self.native.run_operation(child_id);
                        let _ = sender.send(());
                    });
                    while matches!(
                        receiver.recv_timeout(Duration::from_millis(100)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        if context.check().is_err() {
                            let _ = self.native.cancel(&actor, &operation.id);
                        }
                    }
                });
                let result = self
                    .native
                    .operation(&actor, &operation.id)
                    .map_err(|error| error.message)?;
                if matches!(
                    result.outcome,
                    Some(
                        AgentAssetOperationOutcome::AppliedVerified
                            | AgentAssetOperationOutcome::AppliedUnverified
                            | AgentAssetOperationOutcome::OutcomeUnknown
                    )
                ) {
                    context.changed();
                }
                if result.outcome != Some(AgentAssetOperationOutcome::AppliedVerified) {
                    return Err(result
                        .message
                        .unwrap_or_else(|| "原生操作未能确认完成".to_owned()));
                }
                for file in &fresh.files {
                    let current = GuardedFile::capture_path(
                        file.allowed_root(),
                        file.path(),
                        crate::services::agent_cli::catalog::package::MAX_FILE_BYTES,
                    )
                    .map_err(|error| error.message)?;
                    known.insert(file.pathname_domain(), current.bytes().map(<[u8]>::to_vec));
                }
                Ok(())
            })();
            match result {
                Ok(()) => context.item(key, AgentMaintenanceItemState::Completed, None),
                Err(message) => {
                    context.item(
                        key,
                        AgentMaintenanceItemState::Failed,
                        Some(message.clone()),
                    );
                    return Err(message);
                }
            }
        }
        Ok(())
    }
}
fn result_item(
    context: &maintenance::TaskContext,
    owners: &[(String, String, String)],
    index: usize,
    verified: bool,
) {
    context.item(
        &owners[index].2,
        if verified {
            AgentMaintenanceItemState::Completed
        } else {
            AgentMaintenanceItemState::Failed
        },
        (!verified).then(|| "目标尚未通过写入后核对，请刷新查看当前状态".to_owned()),
    );
}
