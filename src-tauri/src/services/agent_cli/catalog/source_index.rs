//! Source/declaration lookup and exact-definition selection, with no I/O or
//! dependency on catalog projection, native codecs or mutation preparation.
use crate::{models::*, services::agent_cli::environment::mutation::MutationInventory};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct DefinitionIndex<'a> {
    declarations: BTreeMap<&'a str, &'a AgentAssetDeclaration>,
    sources: BTreeMap<&'a str, &'a AgentAssetSource>,
    contexts: BTreeMap<&'a str, &'a AgentConfigurationContext>,
}

impl<'a> DefinitionIndex<'a> {
    pub(super) fn new(snapshot: &'a MutationInventory) -> Self {
        Self {
            declarations: snapshot
                .inventory
                .declarations
                .iter()
                .map(|item| (item.id.as_str(), item))
                .collect(),
            sources: snapshot
                .inventory
                .sources
                .iter()
                .map(|item| (item.id.as_str(), item))
                .collect(),
            contexts: snapshot
                .inventory
                .contexts
                .iter()
                .map(|item| (item.id.as_str(), item))
                .collect(),
        }
    }

    pub(super) fn anchor(&self, asset: &AgentAssetRecord) -> Option<&'a AgentAssetDeclaration> {
        let mut seen = BTreeSet::new();
        select_definition_anchor(
            asset,
            asset
                .represented_declaration_ids
                .iter()
                .filter(|id| seen.insert(id.as_str()))
                .filter_map(|id| self.declarations.get(id.as_str()).copied()),
        )
    }

    pub(super) fn source(&self, asset: &AgentAssetRecord) -> Option<&'a AgentAssetSource> {
        self.sources
            .get(self.anchor(asset)?.source_id.as_str())
            .copied()
    }

    pub(super) fn context(&self, id: &str) -> Option<&'a AgentConfigurationContext> {
        self.contexts.get(id).copied()
    }
}

pub(super) fn definition_source<'a>(
    snapshot: &'a MutationInventory,
    asset: &AgentAssetRecord,
) -> Option<&'a AgentAssetSource> {
    let anchor = definition_anchor(snapshot, asset)?;
    snapshot
        .inventory
        .sources
        .iter()
        .find(|source| source.id == anchor.source_id)
}

/// The native projector has already proved this anchor. Represented Definitions
/// can also contain overwritten entries; their ordering must never select bytes
/// for adoption or physical grouping. A merged result has no single-file payload.
pub(crate) fn definition_anchor<'a>(
    snapshot: &'a MutationInventory,
    asset: &AgentAssetRecord,
) -> Option<&'a AgentAssetDeclaration> {
    select_definition_anchor(
        asset,
        snapshot
            .inventory
            .declarations
            .iter()
            .filter(|declaration| asset.represented_declaration_ids.contains(&declaration.id)),
    )
}

fn select_definition_anchor<'a>(
    asset: &AgentAssetRecord,
    declarations: impl Iterator<Item = &'a AgentAssetDeclaration>,
) -> Option<&'a AgentAssetDeclaration> {
    if asset.resolution.relation == AgentAssetResolutionRelation::Merged {
        return None;
    }
    let definitions = declarations
        .filter(|declaration| {
            declaration.role == AgentAssetDeclarationRole::Definition
                && declaration.native_kind == asset.category
        })
        .collect::<Vec<_>>();
    if asset.resolution.relation == AgentAssetResolutionRelation::Unknown && definitions.len() != 1
    {
        return None;
    }
    let mut anchors = definitions.into_iter().filter(|declaration| {
        declaration.source_id == asset.inspection_source_id
            && declaration.context_id == asset.context_id
            && declaration.scope == asset.scope
            && declaration.native_id == asset.native_id
    });
    let anchor = anchors.next()?;
    anchors.next().is_none().then_some(anchor)
}
