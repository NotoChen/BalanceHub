//! Configuration choices describe Agents and native scopes. Comparison,
//! default destinations and write availability are all derived in Rust.
use super::projection::{self, source_path};
use crate::{
    models::*,
    services::agent_cli::{
        definition,
        environment::{mutation::MutationInventory, stable_id},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

struct Destination<'a> {
    target: &'a AgentCatalogTarget,
    source: &'a AgentAssetSource,
    prepared: Option<&'a AgentCatalogTargetPlan>,
    member_id: Option<&'a str>,
    preferred: bool,
    receipt: bool,
}

impl Destination<'_> {
    fn shared(&self) -> bool {
        self.source.origin == AgentAssetInstallationOrigin::SharedFiles
    }
    fn available(&self) -> bool {
        self.prepared.is_some_and(|row| row.available)
    }
    fn reason(&self) -> Option<String> {
        self.prepared
            .and_then(|row| row.reason.clone())
            .or_else(|| self.target.reason.clone())
    }
}

pub(super) fn configuration_choices(
    snapshot: &MutationInventory,
    item: &AgentCatalogAsset,
    entry: &super::repository::Entry,
    desired: &BTreeMap<AgentCliKind, String>,
    fingerprints: &BTreeMap<String, Option<String>>,
    catalog: &AgentAssetCatalog,
    rows: &[AgentCatalogTargetPlan],
) -> AgentCatalogConfigurationSelection {
    let destinations = catalog
        .targets
        .iter()
        .filter(|target| target.categories.contains(&item.category))
        .filter_map(|target| {
            let (source, member_id, preferred, receipt) = if item.category
                == AgentAssetCategory::Hook
            {
                let receipt = entry.receipts.get(&target.id);
                let resolved = super::hook_targets::resolve(snapshot, &target.id, receipt).ok()?;
                let member_id = resolved.binding.map(|asset| asset.stable_id.as_str());
                // Other rules in this document are not destinations for this definition.
                if member_id.is_some_and(|id| !item.bindings.iter().any(|binding| binding.id == id))
                {
                    return None;
                }
                let role = resolved
                    .destination
                    .role
                    .strip_prefix("workspace-")
                    .unwrap_or(&resolved.destination.role);
                (
                    resolved.source,
                    member_id,
                    role == resolved.adapter.default_role,
                    receipt.is_some(),
                )
            } else {
                let (_, source, _) = projection::observed_target(snapshot, &target.id).ok()?;
                (source, None, true, false)
            };
            Some(Destination {
                target,
                source,
                prepared: rows.iter().find(|row| row.target_id == target.id),
                member_id,
                preferred,
                receipt,
            })
        })
        .collect::<Vec<_>>();
    // A separately observed same-name installation is still an installation.
    // Identity/ownership checks in the write preparer continue to forbid takeover.
    let assets = snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| {
            asset.category == item.category
                && (asset.native_id == item.name
                    || asset.label == item.name
                    || item
                        .bindings
                        .iter()
                        .any(|binding| binding.id == asset.stable_id))
        })
        .collect::<Vec<_>>();
    let resource = match item.category {
        AgentAssetCategory::Skill => "Skill",
        AgentAssetCategory::Hook => "Hook",
        _ => "MCP",
    };
    let mut choices = Vec::new();
    let groups = destinations
        .iter()
        .map(|destination| {
            (
                destination.target.agent_kind,
                destination.target.scope,
                destination.target.context_id.as_str(),
            )
        })
        .chain(
            assets
                .iter()
                .filter(|asset| {
                    matches!(
                        asset.scope,
                        AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
                    )
                })
                .map(|asset| (asset.agent_kind, asset.scope, asset.context_id.as_str())),
        )
        .collect::<BTreeSet<_>>();
    for &(kind, scope, context_id) in &groups {
        let mut existing = assets
            .iter()
            .copied()
            .filter(|asset| {
                asset.agent_kind == kind && asset.scope == scope && asset.context_id == context_id
            })
            .collect::<Vec<_>>();
        // Bundled Skills are already supplied to this native context. A
        // missing user/project copy must not turn them into a new install.
        // A real local override remains the definition compared for its scope.
        let inherited_builtin = existing.is_empty() && item.category == AgentAssetCategory::Skill;
        if inherited_builtin {
            existing.extend(assets.iter().copied().filter(|asset| {
                asset.agent_kind == kind
                    && asset.context_id == context_id
                    && asset.scope == AgentAssetScope::System
                    && projection::definition_source(snapshot, asset).is_some_and(|source| {
                        source.origin == AgentAssetInstallationOrigin::Bundled
                    })
            }));
        }
        let inherited_builtin = inherited_builtin && !existing.is_empty();
        let usage = super::usage::single_group(catalog, &existing);
        let single_source = existing.len() == 1 || usage.is_some();
        let shared_destination = (item.category == AgentAssetCategory::Skill && single_source)
            .then(|| {
                let path = physical_source(snapshot, existing.first()?)?;
                destinations
                    .iter()
                    .filter(|destination| {
                        destination.shared()
                            && path.starts_with(source_path(snapshot, destination.source))
                    })
                    .min_by_key(|destination| destination.target.scope != scope)
            })
            .flatten();
        let shared_choice_id = shared_destination.map(|destination| {
            shared_choice_id(
                destination.target.scope,
                source_path(snapshot, destination.source),
            )
        });
        let desired = desired.get(&kind).map(String::as_str);
        let recorded = item
            .unresolved_targets
            .iter()
            .filter(|target| {
                target.agent_kind == kind
                    && target.scope == scope
                    && target.context_id == context_id
            })
            .collect::<Vec<_>>();
        let observation = item
            .application
            .observations
            .iter()
            .find(|observation| observation.agent_kind == kind);
        let state = if existing.is_empty()
            && recorded.is_empty()
            && observation.is_some_and(|observation| {
                observation.state == AgentCatalogObservationState::Unknown
            }) {
            AgentCatalogConfigurationState::Unknown
        } else if existing.is_empty()
            && recorded.len() == 1
            && item.category == AgentAssetCategory::Hook
        {
            let target = recorded[0];
            retained_comparison(target, entry.receipts.get(&target.target_id), desired)
        } else {
            comparison(&existing, desired, fingerprints)
        };
        let private = destinations
            .iter()
            .filter(|destination| {
                destination.target.agent_kind == kind
                    && destination.target.scope == scope
                    && destination.target.context_id == context_id
                    && !destination.shared()
            })
            .collect::<Vec<_>>();
        let original = private
            .iter()
            .copied()
            .filter(|destination| {
                existing.iter().any(|asset| {
                    if item.category == AgentAssetCategory::Hook {
                        return destination.member_id == Some(asset.stable_id.as_str())
                            || (destination.receipt
                                && destination.source.id == asset.inspection_source_id);
                    }
                    projection::definition_source(snapshot, asset).is_some_and(|source| {
                        if item.category == AgentAssetCategory::Skill {
                            Path::new(&source.path).starts_with(&destination.source.path)
                        } else {
                            source.id == destination.source.id
                        }
                    })
                })
            })
            .collect::<Vec<_>>();
        let candidates = if !recorded.is_empty() && item.category == AgentAssetCategory::Hook {
            private
                .iter()
                .copied()
                .filter(|destination| {
                    recorded
                        .iter()
                        .any(|record| record.target_id == destination.target.id)
                })
                .collect::<Vec<_>>()
        } else if existing.is_empty() {
            let defaults = private
                .iter()
                .copied()
                .filter(|destination| {
                    destination.member_id.is_none() && !destination.receipt && destination.preferred
                })
                .collect::<Vec<_>>();
            if defaults.is_empty() {
                private
                    .iter()
                    .copied()
                    .filter(|destination| destination.member_id.is_none() && !destination.receipt)
                    .collect()
            } else {
                defaults
            }
        } else {
            let receipts = original
                .iter()
                .copied()
                .filter(|destination| destination.receipt)
                .collect::<Vec<_>>();
            if receipts.is_empty() {
                original
            } else {
                receipts
            }
        };
        let destination = if shared_choice_id.is_some() {
            None
        } else if candidates.len() == 1 {
            Some(candidates[0])
        } else if usage.is_some() {
            // Verified aliases represent one definition. Use one writable
            // native entrance; the write preparer still validates its authority.
            candidates
                .iter()
                .copied()
                .find(|destination| destination.available())
                .or_else(|| candidates.first().copied())
        } else {
            None
        };
        let mut detail = match state {
            AgentCatalogConfigurationState::Current => format!("已有此 {resource}，内容一致"),
            AgentCatalogConfigurationState::Different => format!("已有同名 {resource}，内容不同"),
            AgentCatalogConfigurationState::Unknown => {
                format!("无法完整核对目标 {resource} 的配置与内容")
            }
            AgentCatalogConfigurationState::Missing => {
                "尚未配置，将写入所选范围的原生配置".to_owned()
            }
        };
        if inherited_builtin {
            detail = match state {
                AgentCatalogConfigurationState::Current => "Agent 内置提供，内容一致，无需重复配置",
                AgentCatalogConfigurationState::Different => {
                    "Agent 已内置同名 Skill，与所选内容不同"
                }
                _ => "Agent 已内置此 Skill，完整内容待核对",
            }
            .to_owned();
        }
        if shared_choice_id.is_some() {
            detail = match state {
                AgentCatalogConfigurationState::Current => "使用共享 Skill，内容一致，无需重复配置",
                AgentCatalogConfigurationState::Different => "使用共享 Skill，与所选内容不同",
                _ => "使用共享 Skill，完整内容暂时无法核对",
            }
            .to_owned();
        } else if let Some(usage) = usage {
            detail = format!("{detail}；{}", usage.detail);
        }
        if recorded
            .iter()
            .any(|target| target.state == AgentCatalogUnresolvedState::Suspended)
        {
            detail = if state == AgentCatalogConfigurationState::Different {
                "已暂停；更新保留的定义后仍保持暂停"
            } else {
                "已暂停并保留定义，可在资源详情中恢复启用"
            }
            .to_owned();
        }
        let reason = if item.category == AgentAssetCategory::Hook && desired.is_none() {
            Some("尚未提供此 Agent 的原生 Hook 定义，请先在编辑器中添加".to_owned())
        } else if recorded.len() > 1 {
            Some("存在多份恢复记录，请先在资源详情中核对".to_owned())
        } else if state == AgentCatalogConfigurationState::Current {
            None
        } else if state == AgentCatalogConfigurationState::Unknown {
            Some(if existing.is_empty() {
                observation
                    .and_then(|observation| observation.reason.clone())
                    .unwrap_or_else(|| "无法完整核对目标来源，请重新读取".to_owned())
            } else {
                "暂不能确认已有资源的完整内容，请先查看来源".to_owned()
            })
        } else if existing.len() > 1
            && !single_source
            && state == AgentCatalogConfigurationState::Different
        {
            Some("发现多个独立或尚未确认同源的同名来源，请先比较内容".to_owned())
        } else if inherited_builtin {
            Some(
                "此来源随 Agent 安装包提供，请先查看内置内容；不会直接覆盖安装包或创建重复副本"
                    .to_owned(),
            )
        } else if existing.iter().any(|asset| {
            !item
                .bindings
                .iter()
                .any(|binding| binding.id == asset.stable_id)
        }) {
            Some("目标已有独立的同名资源，请先比较内容；不会直接覆盖".to_owned())
        } else if shared_choice_id.is_some() {
            Some("请在共享配置中统一更新，引用它的 Agent 会一同受到影响".to_owned())
        } else if let Some(destination) = destination {
            destination.reason()
        } else if candidates.len() > 1 {
            Some("存在多个配置环境，请从对应环境的详情选择位置".to_owned())
        } else if !existing.is_empty() {
            Some(
                if item.category == AgentAssetCategory::Skill {
                    "请通过下方共享配置或资源的原始来源更新"
                } else {
                    "已有配置来自其他原生来源，请先在资源详情中核对"
                }
                .to_owned(),
            )
        } else {
            Some("未找到可用的默认配置位置".to_owned())
        };
        let available = matches!(
            state,
            AgentCatalogConfigurationState::Missing | AgentCatalogConfigurationState::Different
        ) && reason.is_none()
            && destination.is_some_and(Destination::available);
        choices.push(AgentCatalogConfigurationChoice {
            id: stable_id(
                "agent-configuration-choice",
                &[kind.key(), &format!("{scope:?}"), context_id],
            ),
            label: if groups
                .iter()
                .filter(|(agent, group_scope, _)| *agent == kind && *group_scope == scope)
                .count()
                > 1
            {
                snapshot
                    .inventory
                    .contexts
                    .iter()
                    .find(|context| context.id == context_id)
                    .map(|context| {
                        format!(
                            "{} · {} · {}",
                            definition(kind).label,
                            context.profile,
                            context.config_root
                        )
                    })
                    .unwrap_or_else(|| definition(kind).label.to_owned())
            } else {
                definition(kind).label.to_owned()
            },
            agent_kinds: vec![kind],
            scope,
            shared: false,
            shared_choice_id,
            related_asset_ids: related_assets(catalog, item, &existing),
            target_ids: destination
                .map(|destination| vec![destination.target.id.clone()])
                .unwrap_or_default(),
            state,
            available,
            detail,
            reason,
        });
    }
    let mut shared = BTreeMap::<(AgentAssetScope, &Path), Vec<&Destination<'_>>>::new();
    for destination in destinations
        .iter()
        .filter(|destination| destination.shared())
    {
        shared
            .entry((
                destination.target.scope,
                source_path(snapshot, destination.source),
            ))
            .or_default()
            .push(destination);
    }
    for ((scope, path), candidates) in shared {
        let existing = assets
            .iter()
            .copied()
            .filter(|asset| {
                physical_source(snapshot, asset).is_some_and(|source| source.starts_with(path))
            })
            .collect::<Vec<_>>();
        let state = comparison(
            &existing,
            candidates
                .first()
                .and_then(|candidate| desired.get(&candidate.target.agent_kind))
                .map(String::as_str),
            fingerprints,
        );
        let kinds = candidates
            .iter()
            .map(|candidate| candidate.target.agent_kind)
            .chain(existing.iter().map(|asset| asset.agent_kind))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        // One physical directory is one choice and one write, even when several
        // adapters discover it. A verified writer still must prepare the change.
        let destination = candidates
            .iter()
            .copied()
            .find(|destination| destination.available())
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|destination| destination.prepared.is_some())
            })
            .or_else(|| candidates.first().copied());
        let related_asset_ids = related_assets(catalog, item, &existing);
        let reason = match state {
            AgentCatalogConfigurationState::Current => None,
            AgentCatalogConfigurationState::Unknown => {
                Some("共享目录已有同名资源，完整内容待核对".to_owned())
            }
            AgentCatalogConfigurationState::Different if !related_asset_ids.is_empty() => {
                Some("共享位置已有独立的同名资源，请先比较内容；不会直接覆盖".to_owned())
            }
            _ => destination.and_then(Destination::reason),
        };
        let available = matches!(
            state,
            AgentCatalogConfigurationState::Missing | AgentCatalogConfigurationState::Different
        ) && reason.is_none()
            && destination.is_some_and(Destination::available);
        choices.push(AgentCatalogConfigurationChoice {
            id: shared_choice_id(scope, path),
            label: "共享给多个 Agent".to_owned(),
            detail: format!(
                "{} 会发现此共享位置",
                kinds
                    .iter()
                    .map(|kind| definition(*kind).label)
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            agent_kinds: kinds,
            scope,
            shared: true,
            shared_choice_id: None,
            related_asset_ids,
            target_ids: destination
                .map(|destination| vec![destination.target.id.clone()])
                .unwrap_or_default(),
            state,
            available,
            reason,
        });
    }
    AgentCatalogConfigurationSelection { choices }
}

fn shared_choice_id(scope: AgentAssetScope, path: &Path) -> String {
    stable_id(
        "shared-configuration-choice",
        &[&path.to_string_lossy(), &format!("{scope:?}")],
    )
}

fn related_assets(
    catalog: &AgentAssetCatalog,
    item: &AgentCatalogAsset,
    existing: &[&AgentAssetRecord],
) -> Vec<String> {
    catalog
        .assets
        .iter()
        .filter(|candidate| {
            candidate.id != item.id
                && candidate
                    .bindings
                    .iter()
                    .any(|binding| existing.iter().any(|native| native.stable_id == binding.id))
        })
        .map(|candidate| candidate.id.clone())
        .collect()
}

pub(super) fn comparison(
    assets: &[&AgentAssetRecord],
    desired: Option<&str>,
    fingerprints: &BTreeMap<String, Option<String>>,
) -> AgentCatalogConfigurationState {
    compare_fingerprints(
        assets.iter().map(|asset| {
            fingerprints
                .get(&asset.stable_id)
                .and_then(Option::as_deref)
        }),
        desired,
    )
}

pub(super) fn retained_comparison(
    target: &AgentCatalogUnresolvedTarget,
    receipt: Option<&super::repository::Receipt>,
    desired: Option<&str>,
) -> AgentCatalogConfigurationState {
    match target.state {
        AgentCatalogUnresolvedState::Suspended => {
            let fingerprint = receipt
                .and_then(|receipt| receipt.hook.as_ref())
                .map(|hook| super::hook_definition::fingerprint(&hook.desired));
            compare_fingerprints(std::iter::once(fingerprint.as_deref()), desired)
        }
        AgentCatalogUnresolvedState::Missing => AgentCatalogConfigurationState::Missing,
        AgentCatalogUnresolvedState::Unknown => AgentCatalogConfigurationState::Unknown,
    }
}

fn compare_fingerprints<'a>(
    mut current: impl Iterator<Item = Option<&'a str>>,
    desired: Option<&str>,
) -> AgentCatalogConfigurationState {
    let Some(first) = current.next() else {
        return AgentCatalogConfigurationState::Missing;
    };
    let Some(desired) = desired else {
        return AgentCatalogConfigurationState::Unknown;
    };
    let mut different = false;
    for fingerprint in std::iter::once(first).chain(current) {
        let Some(fingerprint) = fingerprint else {
            return AgentCatalogConfigurationState::Unknown;
        };
        different |= fingerprint != desired;
    }
    if different {
        AgentCatalogConfigurationState::Different
    } else {
        AgentCatalogConfigurationState::Current
    }
}

fn physical_source<'a>(
    snapshot: &'a MutationInventory,
    asset: &AgentAssetRecord,
) -> Option<&'a Path> {
    let source = projection::definition_source(snapshot, asset)?;
    Some(source_path(snapshot, source))
}

pub(super) fn shared_impact_notes(
    snapshot: &MutationInventory,
    targets: &[AgentCatalogTarget],
    ids: &[String],
) -> Vec<String> {
    let mut notes = BTreeSet::new();
    for id in ids {
        let Ok((_, source, _)) = projection::resolve_target(snapshot, id) else {
            continue;
        };
        if source.origin != AgentAssetInstallationOrigin::SharedFiles {
            continue;
        }
        let path = source_path(snapshot, source);
        let kinds = targets
            .iter()
            .filter_map(|target| {
                let (_, source, _) = projection::resolve_target(snapshot, &target.id).ok()?;
                (source_path(snapshot, source) == path).then_some(target.agent_kind)
            })
            .collect::<BTreeSet<_>>();
        notes.insert(format!(
            "共享配置：{} 会发现此共享位置。",
            kinds
                .iter()
                .map(|kind| definition(*kind).label)
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    notes.into_iter().collect()
}
