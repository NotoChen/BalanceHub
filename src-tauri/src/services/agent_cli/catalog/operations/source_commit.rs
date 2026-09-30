//! Definition persistence belongs to a registered operation. Rechecks and the
//! cancellation boundary precede the small library CAS; native work follows.
use super::{prepare, CanonicalPlan, OperationCell, Work};
use crate::{
    models::*,
    services::agent_cli::catalog::{digest, CatalogService},
};
use std::{
    collections::BTreeSet,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

impl CatalogService {
    pub(super) fn commit_plan_source(&self, cell: &OperationCell) -> bool {
        let indexes = (0..cell.plan.display.targets.len()).collect::<Vec<_>>();
        cell.phase(&indexes, AgentAssetOperationPhase::Revalidating);
        let result = (|| {
            if cell.canceled.load(Ordering::Acquire) {
                return Err("已取消；共享定义和原生目标均未提交".to_owned());
            }
            let Some(change) = &cell.plan.source.change else {
                // Existing definitions keep the library ownership guard, while
                // each native write domain revalidates its own prepared work.
                // One changed target must not reject independent batch targets.
                cell.plan
                    .source
                    .guard
                    .revalidate()
                    .map_err(|_| "共享库在预览后已变化，请重新计划".to_owned())?;
                return Ok(());
            };
            let locks = self.native.domain_locks();
            let _domains = locks
                .acquire(
                    &cell.plan.domains(),
                    &cell.canceled,
                    Instant::now() + Duration::from_secs(15),
                )
                .map_err(|_| "等待源计划写域超时或已取消".to_owned())?;
            let snapshot = cell
                .plan
                .inspector
                .inspect()
                .map_err(|_| "确认前原生盘点失败".to_owned())?;
            cell.plan.source.revalidate(&snapshot)?;
            let fresh = prepare::prepare_work(
                self,
                &snapshot,
                cell.plan.inspector.as_ref(),
                (&cell.plan.source.item, &cell.plan.source.entry),
                &cell.plan.targets,
                &cell.plan.request,
                None,
            )?;
            if fresh.rows.iter().any(|row| !row.available)
                || signature(&fresh.work)? != signature(&cell.plan.work)?
            {
                return Err(
                    "选中目标的安装、能力、信任、归属或内容已变化，未保存此次版本".to_owned(),
                );
            }
            // cancel() uses this same state mutex. Cancellation observed before
            // admission leaves no version; after admission it cannot erase a
            // confirmed commit. No native target is marked committed here.
            let mut state = cell.state.lock().map_err(|_| "任务状态不可用".to_owned())?;
            if cell.canceled.load(Ordering::Acquire) {
                return Err("已取消；共享定义和原生目标均未提交".to_owned());
            }
            if let Some(public) = &mut state.public.definition_change {
                public.state = AgentCatalogDefinitionChangeState::Unknown;
                public.version = None;
                public.message = Some("正在确认共享版本保存结果".to_owned());
            }
            let result = self
                .repository
                .guarded_transact(&cell.plan.source.guard, |library| {
                    cell.plan.source.save_into(library)
                });
            match result {
                Ok(()) => {
                    self.clear_unpersisted_observations();
                    state.public.definition_change = Some(AgentCatalogDefinitionChangeResult {
                        kind: change.kind,
                        state: AgentCatalogDefinitionChangeState::Saved,
                        version: Some(change.after_version),
                        message: Some("共享版本已保存".to_owned()),
                    });
                }
                Err(message) => {
                    // Replacement may have succeeded before a directory fsync
                    // or recapture failed. Read back exact private content;
                    // never retry or continue native writes after this error.
                    let readback = self.repository.read_snapshot();
                    let desired = cell.plan.source.entry.current().ok_or("计划缺少完整定义")?;
                    let (saved_state, version, explanation) = match readback {
                        Ok(snapshot)
                            if snapshot
                                .library
                                .entries
                                .get(&cell.plan.request.asset_id)
                                .is_some_and(|entry| {
                                    entry.versions.iter().any(|version| {
                                        version.version == desired.version
                                            && version.payload.fingerprint()
                                                == desired.payload.fingerprint()
                                    })
                                }) =>
                        {
                            (
                                AgentCatalogDefinitionChangeState::Saved,
                                Some(change.after_version),
                                "共享版本已写入；保存确认出现错误，原生应用已停止",
                            )
                        }
                        Ok(snapshot)
                            if snapshot.guard.bytes() == cell.plan.source.guard.bytes() =>
                        {
                            (
                                AgentCatalogDefinitionChangeState::Unchanged,
                                change.before_version,
                                "共享定义未变化，原生应用已停止",
                            )
                        }
                        _ => (
                            AgentCatalogDefinitionChangeState::Unknown,
                            None,
                            "无法确认共享版本保存结果；请刷新检查，未继续原生应用",
                        ),
                    };
                    state.public.definition_change = Some(AgentCatalogDefinitionChangeResult {
                        kind: change.kind,
                        state: saved_state,
                        version,
                        message: Some(explanation.to_owned()),
                    });
                    if saved_state != AgentCatalogDefinitionChangeState::Unchanged {
                        self.clear_unpersisted_observations();
                    }
                    state.public.revision += 1;
                    state.public.updated_at = chrono::Local::now().to_rfc3339();
                    return Err(message);
                }
            }
            state.public.revision += 1;
            state.public.updated_at = chrono::Local::now().to_rfc3339();
            Ok(())
        })();
        if let Err(message) = result {
            cell.update(|state| {
                if let Some(change) = &mut state.public.definition_change {
                    if change.state == AgentCatalogDefinitionChangeState::Pending {
                        change.state = AgentCatalogDefinitionChangeState::Unchanged;
                        change.message = Some(message.clone());
                    }
                }
            });
            cell.finish_pending(
                if cell.canceled.load(Ordering::Acquire) {
                    AgentAssetOperationOutcome::CanceledBeforeCommit
                } else {
                    AgentAssetOperationOutcome::UnchangedConflict
                },
                &message,
            );
            return false;
        }
        true
    }
}

impl CanonicalPlan {
    pub(super) fn domains(&self) -> Vec<String> {
        self.work
            .iter()
            .flat_map(|work| match work {
                Work::NativeFiles(members) | Work::NativeCli(members) => members
                    .iter()
                    .flat_map(|member| member.prepared.domains.clone())
                    .collect(),
                Work::Removal(targets) => targets
                    .iter()
                    .flat_map(|(_, target)| target.domains.clone())
                    .collect(),
                Work::Distribution(targets) => targets
                    .iter()
                    .flat_map(|(_, target)| target.domains.clone())
                    .collect(),
                Work::Hooks(group) => group.domains.clone(),
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

fn signature(work: &[Work]) -> Result<String, String> {
    let parts = work
        .iter()
        .map(|work| match work {
            Work::NativeFiles(members) => (
                "files",
                members
                    .iter()
                    .map(|member| member.prepared.signature.clone())
                    .collect::<Vec<_>>(),
            ),
            Work::NativeCli(members) => (
                "cli",
                members
                    .iter()
                    .map(|member| member.prepared.signature.clone())
                    .collect(),
            ),
            Work::Distribution(targets) => (
                "distribution",
                targets
                    .iter()
                    .map(|(_, target)| target.signature.clone())
                    .collect(),
            ),
            Work::Removal(targets) => (
                "removal",
                targets
                    .iter()
                    .map(|(_, target)| target.signature.clone())
                    .collect(),
            ),
            Work::Hooks(group) => ("hooks", vec![group.signature.clone()]),
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&parts)
        .map(|bytes| digest(&bytes))
        .map_err(|_| "计划签名无法编码".to_owned())
}
