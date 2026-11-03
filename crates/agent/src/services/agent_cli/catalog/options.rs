//! Selection and button state use published observations. Only an explicit
//! preview prepares file/CLI mutations; these rows never carry a write token.
use super::{
    action_state::action_owner, definition, operations::prepare::target_row, repository::Entry,
    CatalogService, PublishedCatalog, ReadControl,
};
use crate::models::*;
use crate::services::agent_cli::environment::mutation::MutationInventory;
use std::collections::BTreeMap;

impl CatalogService {
    pub(crate) fn plan_options(
        &self,
        request: &AgentCatalogPlanRequest,
        publication: &PublishedCatalog,
        control: &ReadControl,
    ) -> Result<AgentCatalogPlan, String> {
        control.check()?;
        let catalog = &publication.catalog;
        if request.expected_revision != catalog.revision || !request.target_ids.is_empty() {
            return Err("资产范围已变化，请重新打开".to_owned());
        }
        let (id, draft) = match &request.source {
            AgentCatalogPlanSource::Catalog { asset_id, .. }
            | AgentCatalogPlanSource::NativeBinding { asset_id, .. } => {
                (Some(asset_id.as_str()), None)
            }
            AgentCatalogPlanSource::Draft { definition } => {
                (definition.asset_id.as_deref(), Some(definition.as_ref()))
            }
        };
        let existing = id
            .map(|id| {
                catalog
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .ok_or("资产已变化，请刷新列表")
            })
            .transpose()?;
        self.repository.read_view(|library| {
            let new_entry;
            let entry = if let Some(item) = existing {
                library
                    .entries
                    .get(&item.id)
                    .ok_or("该资产已离开共享库，请刷新列表")?
            } else {
                let draft = draft.ok_or("配置来源无效")?;
                new_entry = Entry::new(draft.name.clone(), draft.category);
                &new_entry
            };
            if existing
                .is_some_and(|item| item.version != entry.current().map(|value| value.version))
            {
                return Err("共享定义已变化，请刷新列表".to_owned());
            }
            let mut item = existing.cloned().unwrap_or_else(|| empty_asset(entry));
            let desired = match &request.source {
                AgentCatalogPlanSource::Catalog {
                    expected_version, ..
                } => {
                    if item.version != *expected_version {
                        return Err("共享定义版本已变化".to_owned());
                    }
                    entry
                        .current()
                        .map(|value| desired_fingerprints(&value.payload))
                        .unwrap_or_default()
                }
                AgentCatalogPlanSource::NativeBinding { binding_id, .. } => {
                    if request.action != AgentCatalogAction::ApplyDefinition
                        || item.version.is_some()
                        || !item
                            .bindings
                            .iter()
                            .any(|binding| binding.id == *binding_id && binding.can_adopt)
                    {
                        return Err("原生来源已变化，请重新选择".to_owned());
                    }
                    let binding = item
                        .bindings
                        .iter()
                        .find(|binding| binding.id == *binding_id)
                        .ok_or("原生来源已变化")?;
                    native_fingerprints(&item, entry, binding)
                }
                AgentCatalogPlanSource::Draft { definition: draft } => {
                    if request.action != AgentCatalogAction::ApplyDefinition
                        || entry.category != draft.category
                        || entry.current().map(|value| value.version) != draft.expected_version
                    {
                        return Err("编辑版本或类别已变化，请重新读取".to_owned());
                    }
                    let payload = definition::input_payload(draft, entry.current())?;
                    item.name.clone_from(&draft.name);
                    item.application.targets = super::projection::application_targets(
                        Ok(&payload),
                        &item.name,
                        item.category,
                        &catalog.targets,
                    );
                    item.application.available =
                        existing.is_none_or(|item| item.application.available);
                    desired_fingerprints(&payload)
                }
            };
            let ids = if request.action == AgentCatalogAction::ApplyDefinition {
                item.application
                    .targets
                    .iter()
                    .map(|target| target.id.clone())
                    .collect::<Vec<_>>()
            } else {
                item.bindings
                    .iter()
                    .map(|binding| binding.id.clone())
                    .chain(
                        item.unresolved_targets
                            .iter()
                            .map(|target| target.target_id.clone()),
                    )
                    .collect()
            };
            let mut targets = target_options(
                &publication.snapshot,
                &item,
                &catalog.targets,
                request.action,
                &ids,
            )?;
            let selection = if request.action == AgentCatalogAction::ApplyDefinition
                && matches!(
                    item.category,
                    AgentAssetCategory::Skill | AgentAssetCategory::Mcp | AgentAssetCategory::Hook
                ) {
                let fingerprints = observed_fingerprints(library, catalog, &item, control)?;
                Some(super::selection::configuration_choices(
                    &publication.snapshot,
                    &item,
                    entry,
                    &desired,
                    &fingerprints,
                    catalog,
                    &targets,
                ))
            } else {
                None
            };
            if let Some(selection) = &selection {
                for target in &mut targets {
                    let choice = selection
                        .choices
                        .iter()
                        .find(|choice| choice.target_ids.contains(&target.target_id));
                    target.available &= choice.is_some_and(|choice| choice.available);
                    if !target.available {
                        target.reason = choice
                            .and_then(|choice| choice.reason.clone())
                            .or(target.reason.take())
                            .or_else(|| {
                                Some(
                                    if choice.is_some_and(|choice| {
                                        choice.state == AgentCatalogConfigurationState::Current
                                    }) {
                                        "与同步来源内容一致，无需重复配置"
                                    } else {
                                        "此位置不在当前可配置范围内，请查看配置来源"
                                    }
                                    .to_owned(),
                                )
                            });
                    }
                }
            }
            control.check()?;
            // No definition adoption/save occurs merely by opening this chooser.
            Ok(AgentCatalogPlan {
                token: None,
                plan_id: None,
                asset_id: item.id,
                action: request.action,
                version: item.version,
                expires_at: String::new(),
                targets,
                notes: Vec::new(),
                definition_change: None,
                selection,
            })
        })
    }
}

pub(super) fn target_options(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    targets: &[AgentCatalogTarget],
    action: AgentCatalogAction,
    ids: &[String],
) -> Result<Vec<AgentCatalogTargetPlan>, String> {
    ids.iter()
        .map(|id| {
            let mut row = target_row(item, targets, id)?;
            if action == AgentCatalogAction::ApplyDefinition {
                let target = item
                    .application
                    .targets
                    .iter()
                    .find(|target| target.id == *id);
                row.available =
                    item.application.available && target.is_some_and(|target| target.available);
                row.reason = item
                    .application
                    .reason
                    .clone()
                    .or_else(|| target.and_then(|target| target.reason.clone()));
            } else {
                let kind = match action {
                    AgentCatalogAction::Enable => AgentAssetActionKind::Enable,
                    AgentCatalogAction::Disable => AgentAssetActionKind::Disable,
                    _ => AgentAssetActionKind::Remove,
                };
                let actions = item
                    .bindings
                    .iter()
                    .find(|binding| binding.id == *id)
                    .map(|binding| {
                        if item.category == AgentAssetCategory::Hook
                            || action == AgentCatalogAction::RemoveBinding
                        {
                            &binding.actions
                        } else {
                            &action_owner(snapshot, binding, kind).actions
                        }
                    })
                    .or_else(|| {
                        item.unresolved_targets
                            .iter()
                            .find(|target| target.target_id == *id)
                            .map(|target| &target.actions)
                    });
                let capability = actions
                    .and_then(|actions| actions.iter().find(|candidate| candidate.action == kind));
                row.available = capability.is_some_and(|capability| capability.available);
                row.reason =
                    capability
                        .filter(|capability| !capability.available)
                        .map(|capability| {
                            (item.category == AgentAssetCategory::Hook
                                || action == AgentCatalogAction::RemoveBinding)
                                .then(|| capability.reload_effect.clone())
                                .flatten()
                                .unwrap_or_else(|| {
                                    AgentAssetMutationError::unavailable(
                                        capability.reason.unwrap_or(
                                            AgentAssetActionUnavailableReason::NoOfficialMechanism,
                                        ),
                                    )
                                    .message
                                })
                        });
            }
            if let Some(reason) = super::action_state::toggle_blocker(snapshot, item, id, action) {
                row.available = false;
                row.reason = Some(reason);
            }
            if !row.available && row.reason.is_none() {
                row.reason = Some("当前没有可用的操作入口".to_owned());
            }
            // The chooser identifies the Agent; paths and native changes belong
            // to the explicit preview. Do not repeat resource names in every row.
            row.label = crate::services::agent_cli::definition(row.agent_kind)
                .label
                .to_owned();
            Ok(row)
        })
        .collect()
}

fn empty_asset(entry: &Entry) -> AgentCatalogAsset {
    AgentCatalogAsset {
        id: String::new(),
        content_revision: String::new(),
        name: entry.name.clone(),
        category: entry.category,
        created_at: None,
        modified_at: None,
        hook: None,
        provenance: AgentAssetProvenanceSummary::default(),
        ownership: AgentCatalogOwnership::Observed,
        version: None,
        variants: Vec::new(),
        bindings: Vec::new(),
        unresolved_targets: Vec::new(),
        candidate_ids: Vec::new(),
        separated_asset_ids: Vec::new(),
        manual_associations: Vec::new(),
        application: AgentCatalogApplication::default(),
        definition_removal: AgentCatalogDefinitionRemoval::default(),
    }
}

pub(super) fn desired_fingerprints(
    payload: &definition::DefinitionPayload,
) -> BTreeMap<AgentCliKind, String> {
    AgentCliKind::ALL
        .iter()
        .filter_map(|kind| {
            payload
                .binding_fingerprint(*kind)
                .map(|value| (*kind, value))
        })
        .collect()
}

pub(super) fn native_fingerprints(
    item: &AgentCatalogAsset,
    entry: &Entry,
    binding: &AgentCatalogBinding,
) -> BTreeMap<AgentCliKind, String> {
    let fingerprint = entry
        .aliases
        .get(&binding.id)
        .and_then(|observation| observation.fingerprint.as_ref());
    AgentCliKind::ALL
        .iter()
        .filter(|kind| {
            item.category != AgentAssetCategory::Hook || **kind == binding.native.agent_kind
        })
        .filter_map(|kind| fingerprint.map(|value| (*kind, value.clone())))
        .collect()
}

pub(super) fn observed_fingerprints(
    library: &super::repository::Library,
    catalog: &AgentAssetCatalog,
    item: &AgentCatalogAsset,
    control: &ReadControl,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut fingerprints = BTreeMap::new();
    for asset in catalog.assets.iter().filter(|asset| {
        asset.category == item.category
            && (asset.id == item.id
                || asset.name == item.name
                || asset.bindings.iter().any(|binding| {
                    binding.native.native_id == item.name || binding.native.label == item.name
                }))
    }) {
        control.check()?;
        let observations = &library
            .entries
            .get(&asset.id)
            .ok_or("该资产已离开共享库，请刷新列表")?
            .aliases;
        fingerprints.extend(
            observations
                .iter()
                .map(|(id, value)| (id.clone(), value.fingerprint.clone())),
        );
    }
    Ok(fingerprints)
}
