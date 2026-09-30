//! Exact target preparation for confirmation previews and commit rechecks.
use super::{
    compose_files, hooks, native_action, NativeMember, PreparedDistribution, PreparedRemoval, Work,
};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::{
            action_state::action_owner, planning::ResolvedPlanRequest, repository::Entry,
            CatalogService,
        },
        environment::mutation::{
            prepare_request, MutationExecution, MutationInspector, MutationInventory,
        },
    },
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct PreparedWork {
    pub rows: Vec<AgentCatalogTargetPlan>,
    pub work: Vec<Work>,
    pub notes: Vec<String>,
}

pub(super) fn prepare_work(
    service: &CatalogService,
    snapshot: &MutationInventory,
    inspector: &dyn MutationInspector,
    asset: (&AgentCatalogAsset, &Entry),
    targets: &[AgentCatalogTarget],
    request: &ResolvedPlanRequest,
    control: Option<&super::super::ReadControl>,
) -> Result<PreparedWork, String> {
    let (item, entry) = asset;
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    let mut work = Vec::new();
    let mut native = Vec::new();
    let mut hook_members = BTreeMap::<String, Vec<hooks::HookMember>>::new();
    let mut distributions = BTreeMap::<String, Vec<(usize, PreparedDistribution)>>::new();
    let mut removals = BTreeMap::<String, Vec<(usize, PreparedRemoval)>>::new();
    for (index, target_id) in request.target_ids.iter().enumerate() {
        if let Some(control) = control {
            control.check()?;
        }
        let mut row = target_row(item, targets, target_id)?;
        if let Some(reason) =
            super::super::action_state::toggle_blocker(snapshot, item, target_id, request.action)
        {
            unavailable(&mut row, reason);
            rows.push(row);
            continue;
        }
        if item.category == AgentAssetCategory::Hook {
            match hooks::HookMember::prepare(
                snapshot,
                item,
                entry,
                target_id,
                request.action,
                index,
            ) {
                Ok(member) => {
                    if item.hook_source(target_id).is_none() {
                        row.label.clone_from(&member.label);
                    }
                    row.changes.clone_from(&member.changes);
                    hook_members
                        .entry(format!(
                            "{}:{}",
                            member.context.agent_kind.key(),
                            member.context.config_root
                        ))
                        .or_default()
                        .push(member);
                }
                Err(reason) => unavailable(&mut row, reason),
            }
        } else if request.action == AgentCatalogAction::ApplyDefinition {
            let result = (|| {
                let target = targets
                    .iter()
                    .find(|target| &target.id == target_id)
                    .ok_or("应用目标不存在")?;
                let definition = entry.current().ok_or("共享定义不存在")?;
                PreparedDistribution::prepare(
                    snapshot,
                    inspector,
                    item,
                    target,
                    &entry.name,
                    definition,
                    entry.receipts.get(target_id),
                )
            })();
            match result {
                Ok(prepared) => {
                    row.changes.clone_from(&prepared.changes);
                    row.affected_asset_ids
                        .clone_from(&prepared.affected_asset_ids);
                    distributions
                        .entry(prepared.group_key())
                        .or_default()
                        .push((index, prepared));
                }
                Err(reason) => unavailable(&mut row, reason),
            }
        } else if request.action == AgentCatalogAction::RemoveBinding {
            match PreparedRemoval::prepare(snapshot, inspector, item, target_id) {
                Ok(prepared) => {
                    row.changes.clone_from(&prepared.changes);
                    row.affected_asset_ids
                        .clone_from(&prepared.affected_asset_ids);
                    removals
                        .entry(prepared.group_key())
                        .or_default()
                        .push((index, prepared));
                }
                Err(reason) => unavailable(&mut row, reason),
            }
        } else if let Some(binding) = item
            .bindings
            .iter()
            .find(|binding| &binding.id == target_id)
        {
            let direct_action = native_action(request.action)?;
            let owner = action_owner(snapshot, binding, direct_action);
            if owner.stable_id != binding.id {
                let note = format!(
                    "{} 的启停将作用于所属插件 {}，并影响该插件提供的其他资源",
                    binding.native.label, owner.label
                );
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
            let native_request = AgentAssetPlanRequest {
                asset_id: owner.stable_id.clone(),
                action: direct_action,
                workspace: request.workspace.clone(),
                expected_revision: owner.revision.identity.clone(),
                installation_id: owner.selected_action_installation_id.clone(),
            };
            match prepare_request(inspector, snapshot, &native_request) {
                Ok((prepared, display)) => {
                    row.changes = display.changes;
                    row.affected_asset_ids = display.affected_asset_ids;
                    native.push(NativeMember {
                        index,
                        request: native_request,
                        prepared,
                    });
                }
                Err(error) => unavailable(&mut row, error.message),
            }
        } else {
            unavailable(&mut row, "此保留目标没有当前可执行的原生绑定".to_owned());
        }
        rows.push(row);
    }
    // File preparers can be composed. CLI side effects cannot be merged by
    // guessing new preconditions; reject overlapping CLI groups explicitly.
    let blocked: BTreeSet<usize> = native
        .iter()
        .enumerate()
        .filter(|(index, member)| {
            native.iter().enumerate().any(|(other_index, other)| {
                index != &other_index
                    && member.request.asset_id != other.request.asset_id
                    && member
                        .prepared
                        .domains
                        .iter()
                        .any(|domain| other.prepared.domains.contains(domain))
                    && (matches!(member.prepared.execution, MutationExecution::ExactCli(_))
                        || matches!(other.prepared.execution, MutationExecution::ExactCli(_)))
            })
        })
        .map(|(_, member)| member.index)
        .collect();
    let mut atomic_members = Vec::new();
    let mut cli_owners = BTreeMap::<String, Vec<NativeMember>>::new();
    for member in native {
        if blocked.contains(&member.index) {
            rows[member.index].available = false;
            rows[member.index].reason =
                Some("该组 CLI 与另一目标共享写域，无法构造无损复合计划，请分别执行".to_owned());
            continue;
        }
        if matches!(
            member.prepared.execution,
            MutationExecution::AtomicFile { .. }
        ) {
            atomic_members.push(member);
        } else {
            cli_owners
                .entry(member.request.asset_id.clone())
                .or_default()
                .push(member);
        }
    }
    work.extend(cli_owners.into_values().map(Work::NativeCli));
    if !atomic_members.is_empty() {
        // Validate composability before issuing the confirmation token.
        compose_files(&atomic_members)?;
        work.push(Work::NativeFiles(atomic_members));
    }
    for targets in distributions.into_values() {
        match PreparedDistribution::validate_group(&targets) {
            Ok(()) => work.push(Work::Distribution(targets)),
            Err(reason) => {
                for (index, _) in targets {
                    rows[index].available = false;
                    rows[index].reason = Some(reason.clone());
                }
            }
        }
    }
    for targets in removals.into_values() {
        match PreparedRemoval::validate_group(&targets) {
            Ok(()) => work.push(Work::Removal(targets)),
            Err(reason) => {
                for (index, _) in targets {
                    unavailable(&mut rows[index], reason.clone());
                }
            }
        }
    }
    for members in hook_members.into_values() {
        let indexes = members
            .iter()
            .map(|member| member.index)
            .collect::<Vec<_>>();
        match hooks::PreparedHookGroup::prepare(service, snapshot, inspector, members) {
            Ok(group) => {
                for index in indexes {
                    rows[index].affected_asset_ids = group.affected_asset_ids.clone();
                }
                notes.extend(group.notes.clone());
                work.push(Work::Hooks(group));
            }
            Err(reason) => {
                for index in indexes {
                    rows[index].available = false;
                    rows[index].reason = Some(reason.clone());
                }
            }
        }
    }
    Ok(PreparedWork { rows, work, notes })
}

fn unavailable(row: &mut AgentCatalogTargetPlan, reason: String) {
    row.available = false;
    row.reason = Some(reason);
}

pub(in crate::services::agent_cli::catalog) fn target_row(
    item: &AgentCatalogAsset,
    targets: &[AgentCatalogTarget],
    id: &str,
) -> Result<AgentCatalogTargetPlan, String> {
    let (label, agent_kind, context_id, scope, target_kind) =
        if let Some(binding) = item.bindings.iter().find(|binding| binding.id == id) {
            (
                item.hook_source(&binding.id)
                    .map(|source| format!("{} · {}", source.label, item.name))
                    .unwrap_or_else(|| binding.native.label.clone()),
                binding.native.agent_kind,
                binding.native.context_id.clone(),
                binding.native.scope,
                AgentCatalogTargetKind::Binding,
            )
        } else if let Some(target) = item
            .unresolved_targets
            .iter()
            .find(|target| target.target_id == id)
        {
            (
                item.hook_source(&target.target_id)
                    .map(|source| format!("{} · {}", source.label, item.name))
                    .unwrap_or_else(|| item.name.clone()),
                target.agent_kind,
                target.context_id.clone(),
                target.scope,
                AgentCatalogTargetKind::Retained,
            )
        } else if let Some(target) = targets.iter().find(|target| target.id == id) {
            (
                target.label.clone(),
                target.agent_kind,
                target.context_id.clone(),
                target.scope,
                AgentCatalogTargetKind::Destination,
            )
        } else {
            return Err("选中目标不属于当前资产或配置范围".to_owned());
        };
    Ok(AgentCatalogTargetPlan {
        target_id: id.to_owned(),
        label,
        agent_kind,
        context_id,
        scope,
        target_kind,
        changes: Vec::new(),
        affected_asset_ids: Vec::new(),
        available: true,
        reason: None,
    })
}
