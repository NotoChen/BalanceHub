//! Per-scan native absence evidence shared by retained receipts and unbound
//! Agent controls. Write qualification never proves configuration absence.
use super::{native::NativeCatalogAdapter, package, projection};
use crate::{
    models::*,
    services::agent_cli::environment::{
        mutation::{GuardedDirectory, GuardedFile, MutationInventory},
        verified_path::reopen_verified_path,
    },
};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
    time::Instant,
};

#[derive(Clone)]
enum Absence {
    Missing,
    Unknown(&'static str),
}

enum SourceObservation {
    Missing,
    Mcp(serde_json::Value),
    Skill(BTreeMap<String, (AgentAssetSourceKind, bool)>),
    Unknown,
}

type NativeTarget<'a> = (
    &'a AgentConfigurationContext,
    &'a AgentAssetSource,
    &'static NativeCatalogAdapter,
);

pub(super) struct NativeObservations<'a> {
    snapshot: &'a MutationInventory,
    deadline: Instant,
    targets: BTreeMap<String, NativeTarget<'a>>,
    sources: BTreeMap<String, SourceObservation>,
    lookups: BTreeMap<(String, String, String, String), Absence>,
}

impl<'a> NativeObservations<'a> {
    pub(super) fn new(
        snapshot: &'a MutationInventory,
        targets: &[AgentCatalogTarget],
        deadline: Instant,
    ) -> Self {
        Self {
            snapshot,
            deadline,
            targets: targets
                .iter()
                .filter_map(|target| {
                    projection::resolve_target(snapshot, &target.id)
                        .ok()
                        .map(|native| (target.id.clone(), native))
                })
                .collect(),
            sources: BTreeMap::new(),
            lookups: BTreeMap::new(),
        }
    }

    fn source(
        &mut self,
        source: &AgentAssetSource,
        adapter: &NativeCatalogAdapter,
    ) -> &SourceObservation {
        self.sources
            .entry(source.id.clone())
            .or_insert_with(|| read_source(self.snapshot, source, adapter, self.deadline))
    }

    fn absence(
        &mut self,
        target: &AgentCatalogTarget,
        category: AgentAssetCategory,
        name: &str,
        receipt_path: Option<&str>,
    ) -> Absence {
        let key = (
            target.id.clone(),
            category.key().to_owned(),
            name.to_owned(),
            receipt_path.unwrap_or_default().to_owned(),
        );
        if let Some(observation) = self.lookups.get(&key) {
            return observation.clone();
        }
        let observation = self.read_absence(target, category, name, receipt_path);
        self.lookups.insert(key, observation.clone());
        observation
    }

    fn read_absence(
        &mut self,
        target: &AgentCatalogTarget,
        category: AgentAssetCategory,
        name: &str,
        receipt_path: Option<&str>,
    ) -> Absence {
        let unknown = Absence::Unknown("原生来源无法完整核验，当前盘点未知");
        if Instant::now() >= self.deadline {
            return Absence::Unknown("当前盘点达到读取预算，尚不能确认未配置");
        }
        let Some(&(context, source, adapter)) = self.targets.get(&target.id) else {
            return unknown;
        };
        if !target.categories.contains(&category) {
            return unknown;
        }
        let owned_path = if category == AgentAssetCategory::Skill {
            let path = receipt_path
                .map(Path::new)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| Path::new(&source.path).join(name));
            let Ok(relative) = path.strip_prefix(&source.path) else {
                return unknown;
            };
            if relative.components().count() != 1
                || !matches!(relative.components().next(), Some(Component::Normal(_)))
            {
                return unknown;
            }
            Some(path)
        } else {
            if receipt_path.is_some_and(|path| path != source.path) {
                return unknown;
            }
            None
        };
        match self.source(source, adapter) {
            SourceObservation::Missing => Absence::Missing,
            SourceObservation::Mcp(root) if category == AgentAssetCategory::Mcp => {
                match adapter.mcp_node_checked(root, context, target.scope, name) {
                    Ok(None) => Absence::Missing,
                    Ok(Some(_)) => {
                        Absence::Unknown("原生来源存在同名配置，尚未归属于此资产，请先核对")
                    }
                    Err(_) => unknown,
                }
            }
            SourceObservation::Skill(entries) if category == AgentAssetCategory::Skill => {
                let Some(path) = owned_path else {
                    return unknown;
                };
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    return unknown;
                };
                match entries.get(name) {
                    None => Absence::Missing,
                    Some((AgentAssetSourceKind::Directory, false)) => {
                        match package::capture_file(
                            Path::new(&source.allowed_root),
                            &path.join("SKILL.md"),
                            package::MAX_FILE_BYTES,
                        ) {
                            Ok(file) if file.bytes().is_none() => Absence::Missing,
                            Ok(_) => Absence::Unknown(
                                "原生目录存在同名 Skill，尚未归属于此资产，请先核对",
                            ),
                            Err(_) => unknown,
                        }
                    }
                    _ => unknown,
                }
            }
            _ => unknown,
        }
    }

    pub(super) fn receipt_missing(
        &mut self,
        target: &AgentCatalogTarget,
        category: AgentAssetCategory,
        receipt: &super::repository::Receipt,
    ) -> bool {
        target.scope == receipt.scope
            && target.context_id == receipt.context_id
            && target.agent_kind == receipt.agent_kind
            && matches!(
                self.absence(target, category, &receipt.name, Some(&receipt.path)),
                Absence::Missing
            )
    }

    pub(super) fn agents(
        &mut self,
        asset: &AgentCatalogAsset,
        targets: &[AgentCatalogTarget],
    ) -> Vec<AgentCatalogAgentObservation> {
        AgentCliKind::ALL
            .iter()
            .copied()
            .filter_map(|kind| {
                if asset
                    .bindings
                    .iter()
                    .any(|binding| binding.native.agent_kind == kind)
                    || asset
                        .unresolved_targets
                        .iter()
                        .any(|target| target.agent_kind == kind)
                {
                    return Some(AgentCatalogAgentObservation {
                        agent_kind: kind,
                        state: AgentCatalogObservationState::Observed,
                        reason: None,
                    });
                }
                // Native packages have no cross-Agent installation identity.
                // An absent binding does not establish compatibility or an
                // unknown installation in another ecosystem.
                if matches!(
                    asset.category,
                    AgentAssetCategory::Plugin | AgentAssetCategory::Extension
                ) {
                    return None;
                }
                let observation = self.unbound_agent(asset, kind, targets);
                Some(match observation {
                    Absence::Missing => AgentCatalogAgentObservation {
                        agent_kind: kind,
                        state: AgentCatalogObservationState::Missing,
                        reason: None,
                    },
                    Absence::Unknown(reason) => AgentCatalogAgentObservation {
                        agent_kind: kind,
                        state: AgentCatalogObservationState::Unknown,
                        reason: Some(reason.to_owned()),
                    },
                })
            })
            .collect()
    }

    fn unbound_agent(
        &mut self,
        asset: &AgentCatalogAsset,
        kind: AgentCliKind,
        targets: &[AgentCatalogTarget],
    ) -> Absence {
        let inventory = &self.snapshot.inventory;
        let contexts = inventory
            .contexts
            .iter()
            .filter(|context| context.agent_kind == kind)
            .collect::<Vec<_>>();
        if contexts.is_empty() {
            return Absence::Unknown("尚未完整盘点该 Agent 的配置上下文");
        }
        if inventory.assets.iter().any(|native| {
            native.agent_kind == kind
                && native.category == asset.category
                && (native.native_id == asset.name || native.label == asset.name)
        }) {
            return Absence::Unknown("该 Agent 存在同名但未关联的配置，请先核对归属");
        }
        if inventory.diagnostics.iter().any(|diagnostic| {
            scan_gap(diagnostic) || discovery_gap(diagnostic, kind, asset.category)
        }) || inventory.sources.iter().any(|source| {
            contexts
                .iter()
                .any(|context| context.id == source.context_id)
                && ((source.categories.contains(&asset.category)
                    && ((!source.revision.is_missing
                        && !self.snapshot.source_anchors.contains_key(&source.id))
                        || source.diagnostics.iter().any(source_gap)))
                    || source
                        .diagnostics
                        .iter()
                        .any(|diagnostic| discovery_gap(diagnostic, kind, asset.category)))
        }) {
            return Absence::Unknown("该 Agent 的相关来源未完整读取，当前盘点未知");
        }
        if asset.category == AgentAssetCategory::Hook {
            return if inventory
                .hook_rule_counts
                .iter()
                .any(|count| count.agent_kind == kind && count.rule_count == Some(0))
            {
                Absence::Missing
            } else {
                Absence::Unknown("尚不能完整确认该 Agent 的原生 Hook 配置")
            };
        }
        let destinations = targets
            .iter()
            .filter(|target| {
                target.agent_kind == kind && target.categories.contains(&asset.category)
            })
            .collect::<Vec<_>>();
        if destinations.is_empty()
            || contexts.iter().any(|context| {
                !destinations
                    .iter()
                    .any(|target| target.context_id == context.id)
            })
        {
            return Absence::Unknown("当前上下文没有可完整核验的原生配置目标");
        }
        for target in destinations {
            let observation = self.absence(target, asset.category, &asset.name, None);
            if !matches!(observation, Absence::Missing) {
                return observation;
            }
        }
        Absence::Missing
    }
}

fn read_source(
    snapshot: &MutationInventory,
    source: &AgentAssetSource,
    adapter: &NativeCatalogAdapter,
    deadline: Instant,
) -> SourceObservation {
    if Instant::now() >= deadline || source.revision.is_symlink {
        return SourceObservation::Unknown;
    }
    if source.revision.is_missing {
        let missing = match source.source_kind {
            AgentAssetSourceKind::File => package::capture_file(
                Path::new(&source.allowed_root),
                Path::new(&source.path),
                package::MAX_FILE_BYTES,
            )
            .is_ok_and(|file| file.bytes().is_none()),
            AgentAssetSourceKind::Directory => matches!(
                GuardedDirectory::capture_path(Path::new(&source.path)),
                Ok(GuardedDirectory::Missing { .. })
            ),
        };
        return if missing {
            SourceObservation::Missing
        } else {
            SourceObservation::Unknown
        };
    }
    if source.source_kind == AgentAssetSourceKind::File {
        let Ok(file) = GuardedFile::capture(source, &snapshot.source_anchors) else {
            return SourceObservation::Unknown;
        };
        let Some(root) = file.bytes().and_then(|bytes| adapter.parse_document(bytes)) else {
            return SourceObservation::Unknown;
        };
        return if file.revalidate().is_ok() {
            SourceObservation::Mcp(root)
        } else {
            SourceObservation::Unknown
        };
    }
    let Some(anchor) = snapshot.source_anchors.get(&source.id) else {
        return SourceObservation::Unknown;
    };
    let Ok(guard) = reopen_verified_path(anchor) else {
        return SourceObservation::Unknown;
    };
    let Ok(entries) = guard.read_entries() else {
        return SourceObservation::Unknown;
    };
    let mut observed = BTreeMap::new();
    for (index, entry) in entries.enumerate() {
        if index >= snapshot.inventory.limits.first_level_entries || Instant::now() >= deadline {
            return SourceObservation::Unknown;
        }
        let Ok(entry) = entry else {
            return SourceObservation::Unknown;
        };
        let Some(name) = entry.name.to_str() else {
            return SourceObservation::Unknown;
        };
        observed.insert(name.to_owned(), (entry.source_kind, entry.is_symlink));
    }
    if guard.revalidate().is_ok() {
        SourceObservation::Skill(observed)
    } else {
        SourceObservation::Unknown
    }
}

fn scan_gap(diagnostic: &AgentAssetDiagnostic) -> bool {
    match diagnostic {
        AgentAssetDiagnostic::Truncated {
            limit:
                AgentAssetLimitKind::CandidatePathsPerAgent
                | AgentAssetLimitKind::InstallationsPerAgent
                | AgentAssetLimitKind::CliOutput
                | AgentAssetLimitKind::CliConcurrency,
            ..
        } => false,
        AgentAssetDiagnostic::Truncated { .. } | AgentAssetDiagnostic::BudgetExceeded { .. } => {
            true
        }
        _ => false,
    }
}

fn source_gap(diagnostic: &AgentAssetDiagnostic) -> bool {
    scan_gap(diagnostic)
        || matches!(
            diagnostic,
            AgentAssetDiagnostic::Malformed { .. }
                | AgentAssetDiagnostic::ReadFailed { .. }
                | AgentAssetDiagnostic::SymlinkRejected { .. }
                | AgentAssetDiagnostic::SourceOutsideAllowedRoot { .. }
                | AgentAssetDiagnostic::SourceTypeMismatch { .. }
                | AgentAssetDiagnostic::InvalidProjection { .. }
                | AgentAssetDiagnostic::InvalidResolution { .. }
        )
}

fn discovery_gap(
    diagnostic: &AgentAssetDiagnostic,
    kind: AgentCliKind,
    category: AgentAssetCategory,
) -> bool {
    matches!(diagnostic, AgentAssetDiagnostic::DiscoveryIncomplete {
        agent_kind,
        category: affected,
        reason: AgentAssetDiscoveryIncompleteReason::SourceUnavailable
            | AgentAssetDiscoveryIncompleteReason::UnsupportedEntryPoint,
    } if *agent_kind == kind && *affected == category)
}
