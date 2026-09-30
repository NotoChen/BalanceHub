//! Resolve reading sources with the same identities as the resource catalog.
use super::definition::{public_definition, DefinitionPayload};
use super::{CatalogService, PublishedCatalog};
use crate::models::*;

pub(crate) struct CatalogContentSources {
    pub asset: AgentCatalogAsset,
    pub shared: Option<AgentCatalogDefinition>,
    pub mcp_connection: Option<serde_json::Value>,
}

impl CatalogService {
    pub(crate) fn content_sources(
        &self,
        asset_id: &str,
        publication: &PublishedCatalog,
    ) -> Result<CatalogContentSources, String> {
        let asset = publication
            .catalog
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .cloned()
            .ok_or("此资源已变化，请刷新列表后重新打开")?;
        let (shared, mcp_connection) = if asset.version.is_some() {
            self.repository.read_entry(asset_id, |entry| {
                let current = entry.current().ok_or("共享定义已变化，请刷新后重新打开")?;
                if Some(current.version) != asset.version {
                    return Err("共享定义已变化，请刷新后重新打开".to_owned());
                }
                let connection = match &current.payload {
                    DefinitionPayload::Mcp(value) => Some(value.connection_document()),
                    _ => None,
                };
                Ok((
                    Some(public_definition(asset_id, &entry.name, current)),
                    connection,
                ))
            })?
        } else {
            (None, None)
        };
        Ok(CatalogContentSources {
            asset,
            shared,
            mcp_connection,
        })
    }
}
