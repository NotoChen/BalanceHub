//! Native action qualification by observed context and adapter schema. This module never opens files or
//! interprets native configuration keys; adapters own the catalog and plans.

use crate::models::*;
use std::collections::BTreeSet;

#[derive(Debug)]
pub(crate) enum AgentMechanismEvaluation<'a> {
    Matched(&'a AgentAssetMechanismRecord),
    Unavailable {
        reason: AgentAssetActionUnavailableReason,
    },
}

impl AgentMechanismEvaluation<'_> {
    pub(crate) fn rank(&self) -> u8 {
        use AgentAssetActionUnavailableReason::*;
        match self {
            Self::Matched(_) => 12,
            Self::Unavailable { reason, .. } => match reason {
                NoCompatibleInstallation => 1,
                InstallationUnavailable => 2,
                NoOfficialMechanism => 3,
                UnsupportedScope => 4,
                UnsupportedPlatform => 5,
                UnsupportedSchema => 9,
                AmbiguousMechanism => 10,
                _ => 0,
            },
        }
    }
}

pub(crate) fn evaluate_mechanism<'a>(
    records: &'a [AgentAssetMechanismRecord],
    installation: &AgentInstallation,
    context: &AgentConfigurationContext,
    asset: &AgentAssetRecord,
    action: AgentAssetActionKind,
    current_target: AgentNativeTarget,
) -> AgentMechanismEvaluation<'a> {
    use AgentAssetActionUnavailableReason::*;
    let unavailable = |reason| AgentMechanismEvaluation::Unavailable { reason };
    if let Some(reason) = asset_installation_blocker(&asset.details) {
        return unavailable(reason);
    }
    if installation.agent_kind != context.agent_kind
        || asset.agent_kind != context.agent_kind
        || installation.environment_id != context.environment_id
        || asset.environment_id != context.environment_id
        || asset.context_id != context.id
        || !context
            .compatible_installation_ids
            .contains(&installation.id)
        || !asset.compatible_installation_ids.contains(&installation.id)
    {
        return unavailable(NoCompatibleInstallation);
    }
    if installation.availability != AgentInstallationAvailability::Available
        || installation.executable_identity.is_none()
        || installation.executable_revision.is_none()
    {
        return unavailable(InstallationUnavailable);
    }
    let mut candidates: Vec<_> = records
        .iter()
        .filter(|record| {
            record.agent_kind == asset.agent_kind
                && record.category == asset.category
                && record.action == action
                && validate_record(record).is_ok()
        })
        .collect();
    macro_rules! retain {
        ($predicate:expr, $reason:ident) => {
            candidates.retain($predicate);
            if candidates.is_empty() {
                return unavailable($reason);
            }
        };
    }
    if candidates.is_empty() {
        return unavailable(NoOfficialMechanism);
    }
    retain!(
        |record| record.scopes.contains(&asset.scope),
        UnsupportedScope
    );
    retain!(
        |record| record.platforms.contains(&current_target.platform),
        UnsupportedPlatform
    );
    retain!(
        |record| record.adapter_schema_version == context.parser_version,
        UnsupportedSchema
    );
    if candidates.len() != 1 {
        return unavailable(AmbiguousMechanism);
    }
    AgentMechanismEvaluation::Matched(candidates[0])
}

fn asset_installation_blocker(
    details: &AgentAssetDetails,
) -> Option<AgentAssetActionUnavailableReason> {
    use AgentAssetActionUnavailableReason::{AssetInstallationUnknown, AssetNotInstalled};
    match details {
        AgentAssetDetails::Plugin { install_state, .. }
        | AgentAssetDetails::Extension { install_state, .. } => match install_state {
            AgentAssetInstallState::Installed => None,
            AgentAssetInstallState::NotInstalled => Some(AssetNotInstalled),
            AgentAssetInstallState::Unknown => Some(AssetInstallationUnknown),
        },
        _ => None,
    }
}

fn validate_record(record: &AgentAssetMechanismRecord) -> Result<(), &'static str> {
    if record.id.0.trim().is_empty()
        || !matches!(
            record.action,
            AgentAssetActionKind::Enable
                | AgentAssetActionKind::Disable
                | AgentAssetActionKind::Remove
        )
        || record.platforms.is_empty()
        || record.scopes.is_empty()
        || record.adapter_schema_version == 0
        || (record.source_schema.as_deref().is_none_or(str::is_empty)
            && record.executable_argv.is_empty())
        || record.inspection.trim().is_empty()
        || record.commit_point.trim().is_empty()
        || record.redaction_rules.is_empty()
    {
        return Err("invalid mechanism record");
    }
    Ok(())
}

pub(crate) fn validate_catalog(records: &[AgentAssetMechanismRecord]) -> Result<(), &'static str> {
    let mut ids = BTreeSet::new();
    for record in records {
        validate_record(record)?;
        if !ids.insert(&record.id) {
            return Err("duplicate mechanism identity");
        }
    }
    Ok(())
}

/// The projector's semantic blockers win. The sole native matcher then chooses
/// a compatible installation deterministically; the frontend receives the result.
pub(crate) fn materialize_actions(
    inventory: &mut AgentEnvironmentInventory,
    definitions: &[crate::services::agent_cli::AgentCliDefinition],
) {
    let catalog_valid = validate_catalog(&inventory.mechanisms).is_ok();
    let target = AgentNativeTarget {
        platform: inventory.environment.host_platform,
        architecture: inventory.environment.host_architecture,
    };
    for asset in &mut inventory.assets {
        let state = asset.effective_state;
        asset
            .actions
            .retain(|action| super::toggle_action_is_relevant(state, action.action));
        let Some(context) = inventory
            .contexts
            .iter()
            .find(|context| context.id == asset.context_id)
        else {
            continue;
        };
        let mut installations: Vec<_> = inventory
            .installations
            .iter()
            .filter(|installation| asset.compatible_installation_ids.contains(&installation.id))
            .collect();
        installations.sort_by(|a, b| a.id.cmp(&b.id));
        for kind in [
            AgentAssetActionKind::Enable,
            AgentAssetActionKind::Disable,
            AgentAssetActionKind::Remove,
        ] {
            let Some(index) = asset
                .actions
                .iter()
                .position(|action| action.action == kind)
            else {
                continue;
            };
            if asset.actions[index].reason.is_some_and(|reason| {
                matches!(
                    reason,
                    AgentAssetActionUnavailableReason::ChildOwnedByParent
                        | AgentAssetActionUnavailableReason::Shadowed
                        | AgentAssetActionUnavailableReason::PolicyBlocked
                        | AgentAssetActionUnavailableReason::SourceUnavailable
                        | AgentAssetActionUnavailableReason::TrustRequired
                        | AgentAssetActionUnavailableReason::ScopeAmbiguous
                        | AgentAssetActionUnavailableReason::Unknown
                )
            }) {
                continue;
            }
            if !catalog_valid {
                asset.actions[index].available = false;
                asset.actions[index].reason =
                    Some(AgentAssetActionUnavailableReason::UnsupportedSchema);
                asset.actions[index].mechanism_id = None;
                asset.actions[index].selected_installation_id = None;
                continue;
            }
            if let Some(reason) = asset_installation_blocker(&asset.details) {
                let action = &mut asset.actions[index];
                action.available = false;
                action.reason = Some(reason);
                action.mechanism_id = None;
                action.selected_installation_id = None;
                continue;
            }
            if !inventory.mechanisms.iter().any(|record| {
                record.agent_kind == asset.agent_kind
                    && record.category == asset.category
                    && record.action == kind
            }) {
                let action = &mut asset.actions[index];
                action.available = false;
                action.reason = Some(
                    definitions
                        .iter()
                        .find(|definition| definition.kind == asset.agent_kind)
                        .map(|definition| {
                            definition
                                .environment()
                                .native_unavailable_reason(asset.category, kind)
                        })
                        .unwrap_or(AgentAssetActionUnavailableReason::NoOfficialMechanism),
                );
                action.mechanism_id = None;
                action.selected_installation_id = None;
                continue;
            }
            let selected = installations
                .iter()
                .map(|installation| {
                    (
                        *installation,
                        evaluate_mechanism(
                            &inventory.mechanisms,
                            installation,
                            context,
                            asset,
                            kind,
                            target,
                        ),
                    )
                })
                .max_by(|(a, ea), (b, eb)| ea.rank().cmp(&eb.rank()).then_with(|| b.id.cmp(&a.id)));
            let action = &mut asset.actions[index];
            action.available = false;
            action.selected_installation_id = None;
            action.mechanism_id = None;
            match selected {
                Some((installation, AgentMechanismEvaluation::Matched(record))) => {
                    action.available = true;
                    action.reason = None;
                    action.mechanism_id = Some(record.id.0.clone());
                    action.selected_installation_id = Some(installation.id.clone());
                    action.reload_effect = record.reload_effect.clone();
                }
                Some((_, AgentMechanismEvaluation::Unavailable { reason })) => {
                    action.reason = Some(reason);
                }
                None => {
                    action.reason =
                        Some(AgentAssetActionUnavailableReason::NoCompatibleInstallation);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
