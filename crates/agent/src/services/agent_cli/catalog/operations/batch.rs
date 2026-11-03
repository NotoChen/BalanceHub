use super::{hooks::PreparedHookGroup, *};
use crate::services::agent_cli::{
    catalog::{PublishedCatalog, ReadControl},
    maintenance::{
        self,
        files::{self, FileWrite},
        MaintenanceService,
    },
};
mod execution;

struct PreparedAsset {
    source: PreparedSource,
    resolved: planning::ResolvedPlanRequest,
    signature: String,
}
struct Batch {
    assets: Vec<PreparedAsset>,
    targets: Vec<AgentCatalogTarget>,
    owners: Vec<(String, String, String)>,
    native: Vec<NativeMember>,
    cli: Vec<NativeMember>,
    distributions: Vec<(usize, PreparedDistribution)>,
    removals: BTreeMap<String, Vec<(usize, PreparedRemoval)>>,
    hooks: Option<PreparedHookGroup>,
    writes: Vec<FileWrite>,
    domains: Vec<String>,
}

impl CatalogService {
    pub(crate) fn batch_options(
        &self,
        ids: Vec<String>,
        action: AgentCatalogAction,
        publication: &PublishedCatalog,
        control: &ReadControl,
    ) -> Result<Vec<AgentBatchOption>, String> {
        if ids.is_empty()
            || ids.len() > 128
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err("请选择 1 至 128 个不同资产".to_owned());
        }
        ids.into_iter()
            .map(|id| {
                control.check()?;
                let asset = publication
                    .catalog
                    .assets
                    .iter()
                    .find(|item| item.id == id)
                    .ok_or("所选资产已不存在")?;
                let source = if action == AgentCatalogAction::ApplyDefinition {
                    asset.application.source_binding_id.as_ref().map(|binding| {
                        AgentCatalogPlanSource::NativeBinding {
                            asset_id: id.clone(),
                            binding_id: binding.clone(),
                        }
                    })
                } else {
                    None
                }
                .unwrap_or_else(|| AgentCatalogPlanSource::Catalog {
                    asset_id: id.clone(),
                    expected_version: asset.version,
                });
                let request = AgentCatalogPlanRequest {
                    source,
                    action,
                    target_ids: Vec::new(),
                    expected_revision: publication.catalog.revision.clone(),
                    workspace: publication.catalog.inventory.workspace.clone(),
                };
                let result = self.plan_options(&request, publication, control);
                control.check()?;
                let (plan, reason) = match result {
                    Ok(plan) => (Some(plan), None),
                    Err(reason) => (None, Some(reason)),
                };
                Ok(AgentBatchOption {
                    asset_id: id,
                    name: asset.name.clone(),
                    request,
                    plan,
                    reason,
                })
            })
            .collect()
    }
    pub(crate) fn plan_batch(
        self: &Arc<Self>,
        actor: &str,
        service: &MaintenanceService,
        requests: Vec<AgentCatalogPlanRequest>,
        inspector: Arc<dyn MutationInspector>,
        publication: &PublishedCatalog,
        control: &ReadControl,
    ) -> Result<AgentMaintenancePlan, String> {
        if requests.is_empty() || requests.len() > 128 {
            return Err("请选择 1 至 128 个资产".to_owned());
        }
        if requests.iter().any(|request| {
            request.target_ids.is_empty()
                || request.target_ids.len() > 64
                || request.workspace.as_deref().map(std::path::Path::new) != inspector.workspace()
        }) {
            return Err("批量目标或配置范围无效".to_owned());
        }
        let snapshot = inspector
            .inspect_for_read(control.cancellation_flag())
            .map_err(|error| error.message)?;
        let mut batch = Batch {
            assets: Vec::new(),
            targets: publication.catalog.targets.clone(),
            owners: Vec::new(),
            native: Vec::new(),
            cli: Vec::new(),
            distributions: Vec::new(),
            removals: BTreeMap::new(),
            hooks: None,
            writes: Vec::new(),
            domains: Vec::new(),
        };
        let mut hook_members = Vec::new();
        let mut items = Vec::new();
        let mut selected = BTreeSet::new();
        let mut affected_agents = BTreeSet::new();
        for request in requests {
            control.check()?;
            let (source, catalog, resolved) =
                planning::prepare(self, &request, &snapshot, Some(publication))?;
            if !selected.insert(resolved.asset_id.clone()) {
                return Err("同一资产不能在一个批次中重复选择".to_owned());
            }
            let prepared = prepare::prepare_work(
                self,
                &snapshot,
                inspector.as_ref(),
                (&source.item, &source.entry),
                &catalog.targets,
                &resolved,
                Some(control),
            )?;
            if let Some(row) = prepared.rows.iter().find(|row| !row.available) {
                return Err(format!(
                    "{} · {}：{}",
                    source.item.name,
                    row.label,
                    row.reason.as_deref().unwrap_or("当前不可执行")
                ));
            }
            let signature = super::source_commit::signature(&prepared.work)?;
            let offset = batch.owners.len();
            for row in &prepared.rows {
                affected_agents.insert(row.agent_kind);
                let key = format!("{}:{}", source.item.id, row.target_id);
                batch
                    .owners
                    .push((source.item.id.clone(), row.target_id.clone(), key.clone()));
                items.push(maintenance::item(
                    key,
                    format!("{} → {}", source.item.name, row.label),
                ));
            }
            for work in prepared.work {
                match work {
                    Work::NativeFiles(mut members) => {
                        for member in &mut members {
                            member.index += offset;
                        }
                        batch.native.extend(members);
                    }
                    Work::NativeCli(mut members) => {
                        for member in &mut members {
                            member.index += offset;
                        }
                        batch.cli.extend(members);
                    }
                    Work::Distribution(targets) => batch.distributions.extend(
                        targets
                            .into_iter()
                            .map(|(index, target)| (index + offset, target)),
                    ),
                    Work::Removal(targets) => {
                        for (index, target) in targets {
                            batch
                                .removals
                                .entry(target.group_key())
                                .or_default()
                                .push((index + offset, target));
                        }
                    }
                    Work::Hooks(group) => {
                        for mut member in group.members {
                            member.index += offset;
                            hook_members.push(member);
                        }
                    }
                }
            }
            batch.assets.push(PreparedAsset {
                source,
                resolved,
                signature,
            });
        }
        if !hook_members.is_empty() {
            batch.hooks = Some(PreparedHookGroup::prepare(
                self,
                &snapshot,
                inspector.as_ref(),
                hook_members,
            )?);
        }
        batch
            .writes
            .extend(PreparedDistribution::batch_writes(&batch.distributions)?);
        for write in compose_files(&batch.native)? {
            batch.writes.push(FileWrite {
                id: write.file.pathname_domain(),
                source: write.file,
                after: Some(write.bytes),
                executable: None,
                mode: None,
            });
        }
        for targets in batch.removals.values() {
            if let Some(write) = PreparedRemoval::batch_write(targets)? {
                batch.writes.push(write);
            }
        }
        if let Some(hooks) = &batch.hooks {
            batch
                .writes
                .extend(hooks.writes.iter().map(|write| FileWrite {
                    id: write.file.pathname_domain(),
                    source: write.file.clone(),
                    after: Some(write.bytes.clone()),
                    executable: None,
                    mode: None,
                }));
        }
        batch.writes = compose_writes(batch.writes)?;
        if let Some(hooks) = &mut batch.hooks {
            for hook in &mut hooks.writes {
                let write = batch
                    .writes
                    .iter()
                    .find(|write| write.source.pathname_domain() == hook.file.pathname_domain())
                    .ok_or("Hook 合并文件不存在")?;
                hook.bytes = write.after.clone().ok_or("Hook 合并结果不能删除文件")?;
            }
        }
        batch.domains.extend(
            batch
                .native
                .iter()
                .flat_map(|member| member.prepared.domains.clone()),
        );
        batch.domains.extend(
            batch
                .distributions
                .iter()
                .flat_map(|(_, target)| target.domains.clone()),
        );
        batch.domains.extend(
            batch
                .removals
                .values()
                .flatten()
                .flat_map(|(_, target)| target.domains.clone()),
        );
        if let Some(hooks) = &batch.hooks {
            batch.domains.extend(hooks.domains.clone());
        }
        let mut changes = batch
            .writes
            .iter()
            .map(FileWrite::change)
            .collect::<Vec<_>>();
        for targets in batch.removals.values() {
            if PreparedRemoval::batch_write(targets)?.is_none() {
                changes.extend(
                    targets
                        .iter()
                        .flat_map(|(_, target)| target.changes.clone()),
                );
            }
        }
        changes.extend(
            batch
                .cli
                .iter()
                .flat_map(|member| member.prepared.changes.clone()),
        );
        let mut display = maintenance::preview(
            AgentMaintenanceKind::Batch,
            format!("批量处理 {} 个资产", batch.assets.len()),
            changes,
            items,
        );
        display.refresh.agents = affected_agents.into_iter().collect();
        display.notes.push("共用文件的变更合并后写入；每个目标分别核对结果。取消只停止未完成的项目，已完成的内容可在配置历史中恢复。".to_owned());
        let bytes = batch
            .assets
            .iter()
            .map(|asset| asset.source.estimate_bytes())
            .chain(batch.writes.iter().map(FileWrite::bytes))
            .chain(
                batch
                    .native
                    .iter()
                    .chain(&batch.cli)
                    .map(|member| super::cache::native_bytes(&member.prepared)),
            )
            .chain(
                batch
                    .distributions
                    .iter()
                    .map(|(_, target)| target.retained_private_bytes()),
            )
            .chain(
                batch
                    .removals
                    .values()
                    .flatten()
                    .map(|(_, target)| target.retained_private_bytes()),
            )
            .chain(
                batch
                    .hooks
                    .iter()
                    .map(PreparedHookGroup::retained_private_bytes),
            )
            .fold(0usize, usize::saturating_add);
        let catalog = Arc::clone(self);
        control.check()?;
        service.plan(actor, display, bytes, move |context| {
            catalog.run_batch(batch, inspector, context)
        })
    }
}
fn compose_writes(writes: Vec<FileWrite>) -> Result<Vec<FileWrite>, String> {
    let mut groups = BTreeMap::<String, Vec<FileWrite>>::new();
    for write in writes {
        groups
            .entry(write.source.pathname_domain())
            .or_default()
            .push(write);
    }
    groups
        .into_values()
        .map(|mut writes| {
            let first = writes.remove(0);
            if writes.is_empty() {
                return Ok(first);
            }
            if writes.iter().any(|write| {
                write.source.bytes() != first.source.bytes()
                    || write.executable != first.executable
                    || write.mode != first.mode
                    || write.after.is_none()
                    || first.after.is_none()
            }) {
                return Err("同一物理文件存在不同的写入条件".to_owned());
            }
            let mut replacements = vec![first.after.clone().ok_or("写入内容为空")?];
            replacements.extend(writes.into_iter().filter_map(|write| write.after));
            if replacements.iter().all(|bytes| bytes == &replacements[0]) {
                return Ok(first);
            }
            let format = if first
                .source
                .path()
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                ConfigDocumentFormat::Toml
            } else {
                ConfigDocumentFormat::Json
            };
            let after = super::merge::compose(
                first.source.bytes().unwrap_or_default(),
                &replacements,
                format,
            )?;
            Ok(FileWrite {
                after: Some(after),
                ..first
            })
        })
        .collect()
}
