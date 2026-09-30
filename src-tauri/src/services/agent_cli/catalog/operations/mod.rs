pub(super) mod cache;
mod distribution;
mod execution;
mod hook_execution;
mod hook_guard;
mod hooks;
mod merge;
pub(super) mod prepare;
mod removal;
mod removal_execution;
mod remove_skill;
mod source_commit;

use super::{
    opaque_id,
    planning::{self, PreparedSource, PreviewPermit, ResolvedPlanRequest},
    CatalogService,
};
use crate::{
    models::*,
    services::agent_cli::environment::{
        config_document::ConfigDocumentFormat,
        mutation::{
            token::DEFAULT_PLAN_TTL, GuardedFile, MutationExecution, MutationInspector,
            PreparedMutation,
        },
    },
};
use distribution::PreparedDistribution;
use removal::PreparedRemoval;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

pub(super) fn hook_binding_actions(
    reader: &mut super::observation::DefinitionReader<'_>,
    asset: &AgentAssetRecord,
) -> Vec<AgentAssetAction> {
    hooks::binding_actions(reader, asset)
}

pub(super) fn hook_suspended_actions(
    snapshot: &crate::services::agent_cli::environment::mutation::MutationInventory,
    receipt: &super::repository::Receipt,
) -> Vec<AgentAssetAction> {
    hooks::suspended_actions(snapshot, receipt)
}

pub(super) struct CanonicalPlan {
    request: ResolvedPlanRequest,
    source: PreparedSource,
    targets: Vec<AgentCatalogTarget>,
    _permit: PreviewPermit,
    display: AgentCatalogPlan,
    inspector: Arc<dyn MutationInspector>,
    work: Vec<Work>,
}
enum Work {
    NativeFiles(Vec<NativeMember>),
    NativeCli(Vec<NativeMember>),
    Distribution(Vec<(usize, PreparedDistribution)>),
    Hooks(hooks::PreparedHookGroup),
    Removal(Vec<(usize, PreparedRemoval)>),
}
struct NativeMember {
    index: usize,
    request: AgentAssetPlanRequest,
    prepared: PreparedMutation,
}

pub(super) struct OperationCell {
    actor: String,
    plan: Arc<CanonicalPlan>,
    state: Mutex<OperationState>,
    canceled: AtomicBool,
}
struct OperationState {
    public: AgentCatalogOperation,
    started: bool,
    committed: BTreeSet<usize>,
}

impl CatalogService {
    #[cfg(all(test, unix))]
    pub(crate) fn plan(
        &self,
        actor: &str,
        request: AgentCatalogPlanRequest,
        inspector: Arc<dyn MutationInspector>,
    ) -> Result<AgentCatalogPlan, String> {
        self.plan_inner(actor, request, inspector, None, None)
    }

    pub(crate) fn plan_read(
        &self,
        actor: &str,
        request: AgentCatalogPlanRequest,
        inspector: Arc<dyn MutationInspector>,
        publication: &super::PublishedCatalog,
        control: &super::ReadControl,
    ) -> Result<AgentCatalogPlan, String> {
        self.plan_inner(actor, request, inspector, Some(publication), Some(control))
    }

    fn plan_inner(
        &self,
        actor: &str,
        request: AgentCatalogPlanRequest,
        inspector: Arc<dyn MutationInspector>,
        publication: Option<&super::PublishedCatalog>,
        control: Option<&super::ReadControl>,
    ) -> Result<AgentCatalogPlan, String> {
        if actor.is_empty()
            || request.target_ids.len() > 64
            || request.target_ids.iter().collect::<BTreeSet<_>>().len() != request.target_ids.len()
            || request.workspace.as_deref().map(std::path::Path::new) != inspector.workspace()
        {
            return Err("资产计划目标无效".to_owned());
        }
        self.prune_previews();
        let options_only = request.target_ids.is_empty();
        // Candidate previews reuse verified, current publication inputs. Execution
        // still revalidates real sources under the mutation lock.
        let snapshot = if let Some(publication) = publication {
            if let Some(control) = control {
                control.check()?;
            }
            let build = self.refresh_inventory(
                Some(publication),
                inspector.settings(),
                inspector.workspace(),
                control.map(|control| control.cancellation_flag()),
            )?;
            crate::services::agent_cli::environment::mutation::MutationInventory {
                inventory: build.inventory,
                source_anchors: build
                    .access_evidence
                    .into_iter()
                    .map(|evidence| (evidence.source_id, evidence.anchor))
                    .collect(),
            }
        } else {
            match control {
                Some(control) => inspector.inspect_for_read(control.cancellation_flag()),
                None => inspector.inspect(),
            }
            .map_err(|_| "原生资产盘点失败")?
        };
        if let Some(control) = control {
            control.check()?;
        }
        let (source, catalog, resolved) =
            planning::prepare(self, &request, &snapshot, publication)?;
        if let Some(control) = control {
            control.check()?;
        }
        let prepared = if options_only {
            prepare::PreparedWork {
                rows: super::options::target_options(
                    &snapshot,
                    &source.item,
                    &catalog.targets,
                    resolved.action,
                    &resolved.target_ids,
                )?,
                work: Vec::new(),
                notes: Vec::new(),
            }
        } else {
            prepare::prepare_work(
                self,
                &snapshot,
                inspector.as_ref(),
                (&source.item, &source.entry),
                &catalog.targets,
                &resolved,
                control,
            )?
        };
        if let Some(control) = control {
            control.check()?;
        }
        let mut display = AgentCatalogPlan {
            token: None,
            plan_id: None,
            asset_id: resolved.asset_id.clone(),
            action: resolved.action,
            version: resolved.expected_version,
            expires_at: (chrono::Local::now()
                + chrono::Duration::seconds(DEFAULT_PLAN_TTL.as_secs() as i64))
            .to_rfc3339(),
            targets: prepared.rows,
            notes: vec!["按明确选择的目标执行并分别验证；跨文件与 CLI 不承诺原子成功。".to_owned()],
            definition_change: source.change.clone(),
            selection: None,
        };
        display.notes.extend(prepared.notes);
        display.notes.extend(super::selection::shared_impact_notes(
            &snapshot,
            &catalog.targets,
            &resolved.target_ids,
        ));
        if options_only
            || display.targets.is_empty()
            || display.targets.iter().any(|target| !target.available)
        {
            return Ok(display);
        }
        if source.change.is_some() {
            display.notes.push("最终确认后先保存此次共享版本，再应用选中目标；后续应用失败或取消时保留已保存版本。".to_owned());
        }
        display.plan_id = Some(opaque_id()?);
        let private_bytes = source
            .estimate_bytes()
            .saturating_add(cache::work_bytes(&prepared.work))
            .saturating_add(cache::encoded_bytes(&display))
            .saturating_add(cache::encoded_bytes(&catalog.targets))
            .saturating_add(cache::encoded_bytes(inspector.settings()));
        let permit = planning::reserve(&self.preview_bytes, private_bytes)?;
        let canonical = Arc::new(CanonicalPlan {
            request: resolved,
            source,
            targets: catalog.targets,
            _permit: permit,
            display: display.clone(),
            inspector,
            work: prepared.work,
        });
        let token = self
            .plans
            .issue(
                actor,
                &display.asset_id,
                display.action,
                canonical,
                Instant::now(),
                DEFAULT_PLAN_TTL,
            )
            .map_err(|error| error.message)?;
        display.token = Some(token);
        Ok(display)
    }

    pub(crate) fn start(
        &self,
        actor: &str,
        request: &AgentCatalogApplyRequest,
    ) -> Result<AgentCatalogOperation, String> {
        self.prune_previews();
        let mut operations = self.operations.lock().map_err(|_| "后台任务不可用")?;
        if operations
            .values()
            .filter(|operation| {
                operation.state.lock().map_or(true, |state| {
                    state.public.phase != AgentAssetOperationPhase::Completed
                })
            })
            .count()
            >= 8
        {
            return Err("已有 8 个资产后台任务，请稍后重试".to_owned());
        }
        let bound = self
            .plans
            .consume_bound(
                actor,
                &request.plan_token,
                &request.asset_id,
                &request.action,
                Instant::now(),
            )
            .map_err(|error| error.message)?;
        let id = opaque_id()?;
        let now = chrono::Local::now().to_rfc3339();
        let public = AgentCatalogOperation {
            id: id.clone(),
            plan_id: bound
                .value
                .display
                .plan_id
                .clone()
                .ok_or("确认计划缺少关联标识")?,
            asset_id: request.asset_id.clone(),
            action: request.action,
            phase: AgentAssetOperationPhase::Preparing,
            revision: 1,
            can_cancel: true,
            created_at: now.clone(),
            updated_at: now,
            definition_change: bound.value.source.change.as_ref().map(|change| {
                AgentCatalogDefinitionChangeResult {
                    kind: change.kind,
                    state: AgentCatalogDefinitionChangeState::Pending,
                    version: change.before_version,
                    message: None,
                }
            }),
            targets: bound
                .value
                .display
                .targets
                .iter()
                .map(|target| AgentCatalogTargetResult {
                    target_id: target.target_id.clone(),
                    label: target.label.clone(),
                    phase: if target.available {
                        AgentAssetOperationPhase::Preparing
                    } else {
                        AgentAssetOperationPhase::Completed
                    },
                    outcome: (!target.available)
                        .then_some(AgentAssetOperationOutcome::UnchangedFailure),
                    message: target.reason.clone(),
                    native_operation_id: None,
                })
                .collect(),
        };
        operations.insert(
            id,
            Arc::new(OperationCell {
                actor: actor.to_owned(),
                plan: bound.value,
                state: Mutex::new(OperationState {
                    public: public.clone(),
                    started: false,
                    committed: BTreeSet::new(),
                }),
                canceled: AtomicBool::new(false),
            }),
        );
        Ok(public)
    }

    pub(crate) fn operation(&self, actor: &str, id: &str) -> Result<AgentCatalogOperation, String> {
        self.prune_previews();
        if let Some(cell) = self
            .operations
            .lock()
            .map_err(|_| "任务状态不可用")?
            .get(id)
            .filter(|cell| cell.actor == actor)
            .cloned()
        {
            return cell
                .state
                .lock()
                .map(|state| state.public.clone())
                .map_err(|_| "任务状态不可用".to_owned());
        }
        self.completed_operations
            .lock()
            .map_err(|_| "任务状态不可用")?
            .get(id)
            .filter(|(owner, _)| owner == actor)
            .map(|(_, public)| public.clone())
            .ok_or_else(|| "该任务不存在或不属于当前窗口".to_owned())
    }
    pub(crate) fn operations(&self, actor: &str) -> Vec<AgentCatalogOperation> {
        self.prune_previews();
        let mut results = self
            .operations
            .lock()
            .map(|operations| {
                operations
                    .values()
                    .filter(|cell| cell.actor == actor)
                    .filter_map(|cell| {
                        cell.state
                            .lock()
                            .ok()
                            .map(|state| (state.public.id.clone(), state.public.clone()))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        if let Ok(completed) = self.completed_operations.lock() {
            results.extend(
                completed
                    .iter()
                    .filter(|(_, (owner, _))| owner == actor)
                    .map(|(id, (_, public))| (id.clone(), public.clone())),
            );
        }
        results.into_values().collect()
    }
    pub(crate) fn cancel(&self, actor: &str, id: &str) -> Result<AgentCatalogOperation, String> {
        let cell = match self.cell(actor, id) {
            Ok(cell) => cell,
            Err(_) => return self.operation(actor, id),
        };
        let children = {
            let mut state = cell.state.lock().map_err(|_| "任务状态不可用")?;
            if state.public.phase != AgentAssetOperationPhase::Completed {
                cell.canceled.store(true, Ordering::Release);
                state.public.can_cancel = false;
                state.public.revision += 1;
                state.public.updated_at = chrono::Local::now().to_rfc3339();
            }
            state
                .public
                .targets
                .iter()
                .filter(|target| target.phase != AgentAssetOperationPhase::Completed)
                .filter_map(|target| target.native_operation_id.clone())
                .collect::<BTreeSet<_>>()
        };
        // Native cancellation uses the same native commit boundary as ordinary
        // actions. Do not hold the parent state lock while crossing services.
        for child in children {
            let _ = self.native.cancel(actor, &child);
        }
        self.operation(actor, id)
    }

    fn cell(&self, actor: &str, id: &str) -> Result<Arc<OperationCell>, String> {
        self.operations
            .lock()
            .map_err(|_| "任务状态不可用")?
            .get(id)
            .filter(|cell| cell.actor == actor)
            .cloned()
            .ok_or_else(|| "该任务不存在或不属于当前窗口".to_owned())
    }

    pub(crate) fn run_operation(&self, id: &str) {
        let Some(cell) = self
            .operations
            .lock()
            .ok()
            .and_then(|operations| operations.get(id).cloned())
        else {
            return;
        };
        {
            let Ok(mut state) = cell.state.lock() else {
                return;
            };
            if state.started {
                return;
            }
            state.started = true;
        }
        if catch_unwind(AssertUnwindSafe(|| {
            if self.commit_plan_source(&cell) {
                self.run(&cell);
            }
        }))
        .is_err()
        {
            cell.finish_failed("任务异常停止；请刷新原生状态核实");
        }

        cell.finish_pending(
            AgentAssetOperationOutcome::CanceledBeforeCommit,
            "已取消尚未执行的目标",
        );
        cell.update(|state| {
            state.public.phase = AgentAssetOperationPhase::Completed;
            state.public.can_cancel = false;
            if let Some(change) = &mut state.public.definition_change {
                if change.state == AgentCatalogDefinitionChangeState::Pending {
                    change.state = AgentCatalogDefinitionChangeState::Unchanged;
                    change.message = Some("共享定义尚未提交".to_owned());
                }
            }
        });
        // Finished history keeps only public results. Dropping the canonical
        // plan releases all private definition, target and guard buffers.
        if let Ok(mut operations) = self.operations.lock() {
            if let (Ok(state), Ok(mut completed)) =
                (cell.state.lock(), self.completed_operations.lock())
            {
                completed.insert(id.to_owned(), (cell.actor.clone(), state.public.clone()));
                operations.remove(id);
                while completed.len() > 128 {
                    let oldest = completed
                        .iter()
                        .min_by(|left, right| left.1 .1.created_at.cmp(&right.1 .1.created_at))
                        .map(|(id, _)| id.clone());
                    if let Some(id) = oldest {
                        completed.remove(&id);
                    } else {
                        break;
                    }
                }
            }
        }
    }
}

impl OperationCell {
    pub(super) fn overlaps(
        &self,
        asset_ids: &BTreeSet<String>,
        binding_ids: &BTreeSet<String>,
        domains: &[String],
    ) -> bool {
        if self
            .state
            .lock()
            .is_ok_and(|state| state.public.phase == AgentAssetOperationPhase::Completed)
        {
            return false;
        }
        asset_ids.contains(&self.plan.request.asset_id)
            || self
                .plan
                .request
                .target_ids
                .iter()
                .any(|id| binding_ids.contains(id))
            || self
                .plan
                .display
                .targets
                .iter()
                .flat_map(|target| &target.affected_asset_ids)
                .any(|id| binding_ids.contains(id))
            || self
                .plan
                .domains()
                .iter()
                .any(|domain| domains.contains(domain))
    }

    fn update(&self, action: impl FnOnce(&mut OperationState)) {
        if let Ok(mut state) = self.state.lock() {
            action(&mut state);
            state.public.revision += 1;
            state.public.updated_at = chrono::Local::now().to_rfc3339();
        }
    }
    fn phase(&self, indexes: &[usize], phase: AgentAssetOperationPhase) {
        self.update(|state| {
            state.public.phase = phase;
            for index in indexes {
                state.public.targets[*index].phase = phase;
            }
        });
    }
    fn before_commit(&self, indexes: &[usize]) -> Result<(), AgentAssetMutationError> {
        let mut state = self.state.lock().map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::InternalFailure)
        })?;
        // One Skill package is one target. Once its first write commits, finish
        // that target and verify it; unrelated files/targets still honor cancel.
        if self.canceled.load(Ordering::Acquire)
            && !indexes.iter().all(|index| state.committed.contains(index))
        {
            return Err(AgentAssetMutationError::new(
                AgentAssetMutationErrorKind::SourceConflict,
            ));
        }
        state.committed.extend(indexes.iter().copied());
        state.public.revision += 1;
        state.public.updated_at = chrono::Local::now().to_rfc3339();
        Ok(())
    }
    fn committed(&self, indexes: &[usize]) -> bool {
        self.state.lock().map_or(true, |state| {
            indexes.iter().any(|index| state.committed.contains(index))
        })
    }
    fn finish_failed(&self, message: &str) {
        self.update(|state| {
            for (index, target) in state.public.targets.iter_mut().enumerate() {
                if target.phase == AgentAssetOperationPhase::Completed {
                    continue;
                }
                target.phase = AgentAssetOperationPhase::Completed;
                target.outcome = Some(
                    if state.committed.contains(&index) || target.native_operation_id.is_some() {
                        AgentAssetOperationOutcome::OutcomeUnknown
                    } else {
                        AgentAssetOperationOutcome::UnchangedFailure
                    },
                );
                target.message = Some(message.to_owned());
            }
        });
    }
    fn result(
        &self,
        indexes: &[usize],
        outcome: AgentAssetOperationOutcome,
        message: Option<String>,
    ) {
        self.update(|state| {
            for index in indexes {
                let target = &mut state.public.targets[*index];
                target.phase = AgentAssetOperationPhase::Completed;
                target.outcome = Some(outcome);
                target.message.clone_from(&message);
            }
        });
    }
    fn finish_pending(&self, outcome: AgentAssetOperationOutcome, message: &str) {
        self.update(|state| {
            for target in &mut state.public.targets {
                if target.phase != AgentAssetOperationPhase::Completed {
                    target.phase = AgentAssetOperationPhase::Completed;
                    target.outcome = Some(outcome);
                    target.message = Some(message.to_owned());
                }
            }
        });
    }
}

fn native_action(action: AgentCatalogAction) -> Result<AgentAssetActionKind, String> {
    match action {
        AgentCatalogAction::Enable => Ok(AgentAssetActionKind::Enable),
        AgentCatalogAction::Disable => Ok(AgentAssetActionKind::Disable),
        _ => Err("不是原生启停操作".to_owned()),
    }
}

struct NativeWrite {
    file: GuardedFile,
    bytes: Vec<u8>,
    indexes: Vec<usize>,
}

fn compose_files(members: &[NativeMember]) -> Result<Vec<NativeWrite>, String> {
    let mut by_path = BTreeMap::<String, (GuardedFile, Vec<Vec<u8>>, Vec<usize>)>::new();
    for member in members {
        let MutationExecution::AtomicFile {
            source_id,
            replacement,
        } = &member.prepared.execution
        else {
            return Err("非文件原生动作不能参与复合写入".to_owned());
        };
        let file = member
            .prepared
            .files
            .iter()
            .find(|file| &file.source_id == source_id)
            .ok_or("原生写入目标缺失")?;
        let entry = by_path
            .entry(file.path().to_string_lossy().into_owned())
            .or_insert_with(|| (file.clone(), Vec::new(), Vec::new()));
        if entry.0.bytes() != file.bytes() || entry.0.domain() != file.domain() {
            return Err("同一文件计划的读取代际不一致".to_owned());
        }
        entry.1.push(replacement.clone());
        entry.2.push(member.index);
    }
    by_path
        .into_values()
        .map(|(file, replacements, indexes)| {
            let format = if file
                .path()
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                ConfigDocumentFormat::Toml
            } else {
                ConfigDocumentFormat::Json
            };
            let bytes = if replacements.len() == 1 {
                replacements[0].clone()
            } else {
                merge::compose(file.bytes().unwrap_or_default(), &replacements, format)?
            };
            Ok(NativeWrite {
                file,
                bytes,
                indexes,
            })
        })
        .collect()
}
