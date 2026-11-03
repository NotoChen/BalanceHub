use super::{repository::Library, CatalogService};
use crate::{
    models::{AgentCatalogDefinitionRemoval, AgentCatalogDeleteRequest},
    services::agent_cli::environment::mutation::MutationInspector,
};
use std::{collections::BTreeSet, sync::Arc};

pub(super) fn definition_removal(library: &Library, id: &str) -> AgentCatalogDefinitionRemoval {
    let reason = match library.entries.get(id) {
        None => Some("共享定义已不存在"),
        Some(entry) if entry.current().is_none() => Some("该资源尚未保存到共享库"),
        Some(entry) if !entry.aliases.is_empty() => Some("仍有关联的原生来源，请先处理绑定"),
        Some(entry) if !entry.receipts.is_empty() => {
            Some("仍有 Agent 应用或恢复记录，请先处理绑定")
        }
        Some(_) if library.relations.has_association(id) => Some("仍有手动合并关系，请先解除关联"),
        Some(_) => None,
    };
    AgentCatalogDefinitionRemoval {
        available: reason.is_none(),
        reason: reason.map(str::to_owned),
    }
}

impl CatalogService {
    pub(crate) fn delete_definition(
        &self,
        request: AgentCatalogDeleteRequest,
        inspector: Arc<dyn MutationInspector>,
    ) -> Result<(), String> {
        let snapshot = inspector
            .inspect()
            .map_err(|_| "无法核对当前资源绑定，请刷新后重试")?;
        let (catalog, base) = self.read_catalog(&snapshot)?;
        if catalog.revision != request.expected_revision {
            return Err("资产目录已变化，请刷新后重新确认删除".to_owned());
        }
        let item = catalog
            .assets
            .iter()
            .find(|item| item.id == request.asset_id)
            .ok_or("共享定义已不存在")?;
        if item.version != Some(request.expected_version) {
            return Err("共享定义已更新，请刷新后重新确认删除".to_owned());
        }
        if !item.definition_removal.available {
            return Err(item
                .definition_removal
                .reason
                .clone()
                .unwrap_or_else(|| "当前资源不能删除共享定义".to_owned()));
        }
        // Serialize with task admission; an unconfirmed plan is invalidated by
        // the library revision, while an admitted operation must finish first.
        let operations = self.operations.lock().map_err(|_| "后台任务状态不可用")?;
        let asset_ids = BTreeSet::from([request.asset_id.clone()]);
        if operations
            .values()
            .any(|operation| operation.overlaps(&asset_ids, &BTreeSet::new(), &[]))
        {
            return Err("此资源正在执行后台任务，请完成后再删除".to_owned());
        }
        self.repository.guarded_transact(&base.guard, |library| {
            let capability = definition_removal(library, &request.asset_id);
            if !capability.available {
                return Err(capability
                    .reason
                    .unwrap_or_else(|| "共享定义当前不能删除".to_owned()));
            }
            if library
                .entries
                .get(&request.asset_id)
                .and_then(|entry| entry.current())
                .map(|value| value.version)
                != Some(request.expected_version)
            {
                return Err("共享定义在确认后已变化，未删除".to_owned());
            }
            library.entries.remove(&request.asset_id);
            library.relations.forget_separations(&request.asset_id);
            Ok(())
        })?;
        self.clear_unpersisted_observations();
        Ok(())
    }
}
