//! The use panel reads the same fingerprints and destination choices as the
//! configuration chooser. Opening it does not read packages or prepare writes.
use super::super::{
    options,
    repository::{Library, Receipt},
    selection, ReadControl,
};
use crate::{
    models::*,
    services::agent_cli::{definition, environment::mutation::MutationInventory},
};
use std::collections::BTreeMap;

pub(super) struct PanelContent {
    pub receipts: BTreeMap<String, Receipt>,
    pub source_label: Option<String>,
    selection: Option<AgentCatalogConfigurationSelection>,
    desired: BTreeMap<AgentCliKind, String>,
    fingerprints: BTreeMap<String, Option<String>>,
    comparable: bool,
}

impl PanelContent {
    pub fn read(
        library: &Library,
        snapshot: &MutationInventory,
        catalog: &AgentAssetCatalog,
        item: &AgentCatalogAsset,
        rows: &[AgentCatalogTargetPlan],
        control: &ReadControl,
    ) -> Result<Self, String> {
        let entry = library
            .entries
            .get(&item.id)
            .ok_or("该资产已离开目录，请刷新列表")?;
        if entry.current().map(|value| value.version) != item.version
            || entry.category != item.category
        {
            return Err("资产定义已变化，请刷新后重新查看".to_owned());
        }
        let comparable = matches!(
            item.category,
            AgentAssetCategory::Skill | AgentAssetCategory::Mcp | AgentAssetCategory::Hook
        );
        let (desired, source_label) = if !comparable {
            (BTreeMap::new(), None)
        } else if let Some(current) = entry.current() {
            (
                options::desired_fingerprints(&current.payload),
                Some(format!("共享库 v{}", current.version)),
            )
        } else if let Some(binding) = item.application.source_binding_id.as_ref().and_then(|id| {
            item.bindings
                .iter()
                .find(|binding| binding.id == *id && binding.can_adopt)
        }) {
            (
                options::native_fingerprints(item, entry, binding),
                Some(format!(
                    "{} · {}",
                    definition(binding.native.agent_kind).label,
                    binding
                        .native
                        .path
                        .as_deref()
                        .unwrap_or(&binding.native.native_id)
                )),
            )
        } else {
            (BTreeMap::new(), None)
        };
        let fingerprints = if comparable {
            options::observed_fingerprints(library, catalog, item, control)?
        } else {
            BTreeMap::new()
        };
        let selection = comparable.then(|| {
            selection::configuration_choices(
                snapshot,
                item,
                entry,
                &desired,
                &fingerprints,
                catalog,
                rows,
            )
        });
        Ok(Self {
            receipts: entry.receipts.clone(),
            source_label,
            selection,
            desired,
            fingerprints,
            comparable,
        })
    }

    pub fn binding_state(
        &self,
        binding: &AgentCatalogBinding,
    ) -> Option<AgentCatalogConfigurationState> {
        self.comparable.then(|| {
            selection::comparison(
                &[&binding.native],
                self.desired
                    .get(&binding.native.agent_kind)
                    .map(String::as_str),
                &self.fingerprints,
            )
        })
    }

    pub fn retained_state(
        &self,
        target: &AgentCatalogUnresolvedTarget,
    ) -> Option<AgentCatalogConfigurationState> {
        self.comparable.then(|| {
            selection::retained_comparison(
                target,
                self.receipts.get(&target.target_id),
                self.desired.get(&target.agent_kind).map(String::as_str),
            )
        })
    }

    pub fn choice(&self, target_id: &str) -> Option<&AgentCatalogConfigurationChoice> {
        self.selection
            .as_ref()?
            .choices
            .iter()
            .find(|choice| choice.target_ids.iter().any(|id| id == target_id))
    }
}
