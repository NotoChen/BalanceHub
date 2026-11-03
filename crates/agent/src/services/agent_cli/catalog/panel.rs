//! Agent use panels read published capabilities; confirmation prepares changes.
use super::{options::target_options, projection, CatalogService, ReadControl};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{MutationInspector, MutationInventory},
};
use std::sync::Arc;
mod actions;
mod content;
use actions::{action_label, batch_actions, panel_action, visible_action};
use content::PanelContent;

impl CatalogService {
    pub(crate) fn agent_panel(
        &self,
        request: AgentCatalogAgentPanelRequest,
        inspector: Arc<dyn MutationInspector>,
        catalog: &AgentAssetCatalog,
        snapshot: &MutationInventory,
        control: &ReadControl,
    ) -> Result<AgentCatalogAgentPanel, String> {
        if request.workspace.as_deref().map(std::path::Path::new) != inspector.workspace() {
            return Err("Agent 面板的工作目录与盘点范围不一致".to_owned());
        }
        control.check()?;
        if catalog.revision != request.expected_revision {
            return Err("资产目录已变化，请刷新后重新查看".to_owned());
        }
        let item = catalog
            .assets
            .iter()
            .find(|asset| asset.id == request.asset_id)
            .ok_or("该资产已离开当前目录")?;
        let ids = item
            .application
            .targets
            .iter()
            .filter(|target| {
                request
                    .agent_kind
                    .is_none_or(|kind| target.agent_kind == kind)
            })
            .map(|target| target.id.clone())
            .collect::<Vec<_>>();
        let application_rows = target_options(
            snapshot,
            item,
            &catalog.targets,
            AgentCatalogAction::ApplyDefinition,
            &ids,
        )?;
        let content = self.repository.read_view(|library| {
            PanelContent::read(library, snapshot, catalog, item, &application_rows, control)
        })?;
        let receipts = &content.receipts;
        let observation = request.agent_kind.map(|kind| {
            item.application
                .observations
                .iter()
                .find(|observation| observation.agent_kind == kind)
                .cloned()
                .unwrap_or(AgentCatalogAgentObservation {
                    agent_kind: kind,
                    state: AgentCatalogObservationState::Unknown,
                    reason: Some(
                        "当前没有完整观察证据，请查看该 Agent 的原生来源和诊断".to_owned(),
                    ),
                })
        });
        let mut entries = item
            .bindings
            .iter()
            .filter(|binding| {
                request
                    .agent_kind
                    .is_none_or(|kind| binding.native.agent_kind == kind)
            })
            .map(|binding| AgentCatalogAgentPanelEntry {
                target_id: binding.id.clone(),
                usage: binding.usage.clone(),
                target_kind: AgentCatalogTargetKind::Binding,
                context_id: binding.native.context_id.clone(),
                scope: binding.native.scope,
                label: item
                    .hook_source(&binding.id)
                    .map(|source| source.label.clone())
                    .unwrap_or_else(|| binding.native.label.clone()),
                path: binding.native.path.clone(),
                state_label: match binding.native.effective_state {
                    AgentAssetState::Enabled => "已启用",
                    AgentAssetState::Disabled => "已停用",
                    AgentAssetState::NotInstalled => "未配置",
                    AgentAssetState::Shadowed if binding.usage.is_some() => "同源入口，当前未采用",
                    AgentAssetState::Shadowed => "已被其他范围覆盖",
                    AgentAssetState::Blocked => "受原生策略限制",
                    AgentAssetState::Invalid => "配置无效",
                    AgentAssetState::Unknown => "状态待核对",
                }
                .to_owned(),
                reason: binding.reason.clone().or_else(|| {
                    binding.native.relationships.action_owner.as_ref().map(|_| {
                        "此资源由父级控制，请查看操作涉及的父插件和完整影响范围".to_owned()
                    })
                }),
                diagnostics: binding_diagnostics(&snapshot.inventory, &binding.native),
                sync_state: content.binding_state(binding),
                actions: Vec::new(),
            })
            .collect::<Vec<_>>();
        entries.extend(
            item.unresolved_targets
                .iter()
                .filter(|target| {
                    request
                        .agent_kind
                        .is_none_or(|kind| target.agent_kind == kind)
                })
                .map(|target| AgentCatalogAgentPanelEntry {
                    target_id: target.target_id.clone(),
                    usage: None,
                    target_kind: AgentCatalogTargetKind::Retained,
                    context_id: target.context_id.clone(),
                    scope: target.scope,
                    label: item
                        .hook_source(&target.target_id)
                        .map(|source| source.label.clone())
                        .unwrap_or_else(|| item.name.clone()),
                    path: receipts
                        .get(&target.target_id)
                        .map(|receipt| receipt.path.clone()),
                    state_label: match target.state {
                        AgentCatalogUnresolvedState::Missing => "已缺失",
                        AgentCatalogUnresolvedState::Unknown => "状态待核对",
                        AgentCatalogUnresolvedState::Suspended => "已停用，可恢复",
                    }
                    .to_owned(),
                    reason: Some(target.message.clone()),
                    diagnostics: Vec::new(),
                    sync_state: content.retained_state(target),
                    actions: Vec::new(),
                }),
        );
        let existing_ids = entries
            .iter()
            .map(|entry| entry.target_id.clone())
            .collect::<Vec<_>>();
        for action in [
            AgentCatalogAction::Enable,
            AgentCatalogAction::Disable,
            AgentCatalogAction::RemoveBinding,
        ] {
            control.check()?;
            for target in target_options(snapshot, item, &catalog.targets, action, &existing_ids)? {
                if !visible_action(snapshot, item, &target.target_id, action) {
                    continue;
                }
                let parent = item
                    .bindings
                    .iter()
                    .find(|binding| binding.id == target.target_id)
                    .and_then(|binding| {
                        binding
                            .native
                            .relationships
                            .action_owner
                            .clone()
                            .or_else(|| binding.native.relationships.provided_by.clone())
                    });
                if let Some(row) = entries
                    .iter_mut()
                    .find(|row| row.target_id == target.target_id)
                {
                    row.actions.push(panel_action(
                        action,
                        &target,
                        parent,
                        action_label(snapshot, item, &target.target_id, action),
                    ));
                }
            }
        }
        for mut target in application_rows {
            control.check()?;
            let Some(choice) = content.choice(&target.target_id) else {
                continue;
            };
            target.available &= choice.available;
            target.reason = choice.reason.clone().or(target.reason);
            let index = if let Some(index) =
                application_entry_index(snapshot, item, &entries, &target, receipts)
            {
                index
            } else {
                let path = if item.category == AgentAssetCategory::Hook {
                    super::hook_targets::resolve(
                        snapshot,
                        &target.target_id,
                        receipts.get(&target.target_id),
                    )
                    .ok()
                    .map(|target| target.source.path.clone())
                } else {
                    projection::observed_target(snapshot, &target.target_id)
                        .ok()
                        .map(|(_, source, _)| source.path.clone())
                };
                entries.push(AgentCatalogAgentPanelEntry {
                    usage: None,
                    target_id: target.target_id.clone(),
                    target_kind: AgentCatalogTargetKind::Destination,
                    context_id: target.context_id.clone(),
                    scope: target.scope,
                    label: target.label.clone(),
                    path,
                    state_label: match choice.state {
                        AgentCatalogConfigurationState::Missing => "未配置",
                        AgentCatalogConfigurationState::Current => "已配置",
                        AgentCatalogConfigurationState::Different => "已有配置",
                        AgentCatalogConfigurationState::Unknown => "配置待核对",
                    }
                    .to_owned(),
                    sync_state: Some(choice.state),
                    reason: target.reason.clone(),
                    diagnostics: Vec::new(),
                    actions: Vec::new(),
                });
                entries.len() - 1
            };
            let row = &mut entries[index];
            match row.sync_state {
                Some(AgentCatalogConfigurationState::Current) => {}
                Some(AgentCatalogConfigurationState::Unknown) | None => {
                    if row.reason.is_none() {
                        row.reason = target.reason;
                    }
                }
                Some(state) => {
                    let label = if state == AgentCatalogConfigurationState::Missing {
                        "配置到此处"
                    } else if item.version.is_some() {
                        "应用共享版本"
                    } else {
                        "同步来源内容"
                    };
                    row.actions.push(panel_action(
                        AgentCatalogAction::ApplyDefinition,
                        &target,
                        None,
                        label.to_owned(),
                    ));
                }
            }
        }
        Ok(AgentCatalogAgentPanel {
            asset_id: item.id.clone(),
            agent_kind: request.agent_kind,
            revision: catalog.revision.clone(),
            observation,
            batch_actions: batch_actions(&entries),
            entries,
            sync_source: content.source_label,
        })
    }
}

fn application_entry_index(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    entries: &[AgentCatalogAgentPanelEntry],
    target: &AgentCatalogTargetPlan,
    receipts: &std::collections::BTreeMap<String, super::repository::Receipt>,
) -> Option<usize> {
    // Retained IDs already identify their exact application destination.
    if let Some(index) = entries
        .iter()
        .position(|entry| entry.target_id == target.target_id)
    {
        return Some(index);
    }
    if item.category == AgentAssetCategory::Hook {
        let receipt = receipts.get(&target.target_id);
        let resolved = super::hook_targets::resolve(snapshot, &target.target_id, receipt).ok()?;
        let binding_id = resolved
            .binding
            .map(|binding| binding.stable_id.as_str())
            .or_else(|| {
                receipt
                    .and_then(|receipt| receipt.hook.as_ref())
                    .and_then(|hook| hook.rule.native_asset_id.as_deref())
            })?;
        return entries.iter().position(|entry| {
            entry.target_kind == AgentCatalogTargetKind::Binding && entry.target_id == binding_id
        });
    }
    // Fold only the exact published binding at this destination. This is
    // display grouping, never evidence that a future write is authorized.
    let (context, source, _) = projection::observed_target(snapshot, &target.target_id).ok()?;
    let bindings =
        projection::bindings_at_target(snapshot, item, &context.id, target.scope, source);
    let [binding] = bindings.as_slice() else {
        return None;
    };
    entries.iter().position(|entry| {
        entry.target_kind == AgentCatalogTargetKind::Binding && entry.target_id == binding.id
    })
}

fn binding_diagnostics(
    inventory: &AgentEnvironmentInventory,
    native: &AgentAssetRecord,
) -> Vec<AgentAssetDiagnostic> {
    let mut diagnostics = native.diagnostics.clone();
    for diagnostic in inventory
        .sources
        .iter()
        .filter(|source| {
            source.context_id == native.context_id
                && (source.id == native.inspection_source_id
                    || native.source_ids.contains(&source.id))
        })
        .flat_map(|source| &source.diagnostics)
    {
        if !diagnostics.contains(diagnostic) {
            diagnostics.push(diagnostic.clone());
        }
    }
    diagnostics
}
