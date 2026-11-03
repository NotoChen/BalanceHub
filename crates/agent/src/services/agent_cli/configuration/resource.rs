//! Resource contents reuse native file authority and configuration transactions.
use super::{
    contracts::AgentConfigurationSourceSpec,
    resource_read,
    selection::{rejected, DocumentSelection},
    service::EditInput,
    sources::{self, ConfigurationSourceAuthority},
    ConfigurationService,
};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::ReadControl, contracts::AgentAssetSourceSpec,
        environment::mutation::MutationInventory,
    },
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

impl ConfigurationService {
    pub(crate) fn begin_resource_edit(
        &self,
        actor: &str,
        snapshot: &MutationInventory,
        asset_id: &str,
        document_id: Option<&str>,
        control: &ReadControl,
    ) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
        let asset = snapshot
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == asset_id)
            .ok_or_else(|| rejected("此资源已变化，请刷新列表后重新打开".to_owned()))?;
        let context = snapshot
            .inventory
            .contexts
            .iter()
            .find(|context| context.id == asset.context_id)
            .ok_or_else(|| rejected("资源所属 Agent 配置范围已变化".to_owned()))?;
        let content = resource_read::read(snapshot, asset, control)?;
        if document_id.is_some_and(|id| {
            !content
                .documents
                .iter()
                .any(|document| document.source.id == id)
        }) {
            return Err(rejected(
                "所选文件已消失或无法读取，请返回内容重新读取后编辑".to_owned(),
            ));
        }
        let mut inputs = Vec::new();
        for document in content.documents {
            control.check().map_err(rejected)?;
            let resource_read::ResourceDocument {
                source,
                file,
                text,
                format,
                selection,
                read_only_reason,
                ..
            } = document;
            let path = PathBuf::from(&source.path);
            let writable = read_only_reason.is_none();
            let native = AgentAssetSourceSpec {
                native_source_key: format!("resource:{}", source.id),
                label: match &selection {
                    DocumentSelection::Structured { .. } => asset.label.clone(),
                    DocumentSelection::Whole => path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                },
                scope: source.scope,
                origin: source.origin,
                provider: AgentAssetProviderOrigin::Unknown,
                path: path.clone(),
                allowed_root: PathBuf::from(&source.allowed_root),
                path_policy: Default::default(),
                verified_physical_path: None,
                precedence: source.precedence,
                writable,
                sensitive: source.sensitive,
                source_kind: AgentAssetSourceKind::File,
                categories: vec![asset.category],
                hook_definition_source: asset.category == AgentAssetCategory::Hook,
                allowed_logical_origins: Vec::new(),
            };
            let spec = AgentConfigurationSourceSpec {
                native,
                format,
                may_create: false,
                profile: None,
                load_rule: "资源当前配置".to_owned(),
                reload_hint: "已保存到原生文件；Agent 可能需要重新加载或开启新会话".to_owned(),
                version_requirement: None,
                initial_text: None,
            };
            let mut public = sources::public_source(
                context,
                &spec,
                source.id,
                snapshot.inventory.workspace.as_deref().map(Path::new),
            );
            public.revision = file.revision();
            public.actions = sources::actions(true, writable, false);
            let authority = Arc::new(ConfigurationSourceAuthority {
                source: public,
                context: context.clone(),
                spec,
                file,
            });
            inputs.push(EditInput {
                source: authority,
                text,
                selection,
                read_only_reason,
            });
        }
        control.check().map_err(rejected)?;
        let mut edit =
            self.admit_inputs(actor, asset.agent_kind, inputs, Some(content.resource))?;
        if let Err(message) = control.check() {
            self.discard_edit(actor, &edit.edit_id)?;
            return Err(rejected(message));
        }
        edit.diagnostics.extend(content.diagnostics);
        Ok(edit)
    }
}
