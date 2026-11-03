use super::{
    add_disabled, add_nested_definition, common, ensure_owned_leaf_parent, is_definitions_disabled,
    remove_disabled, remove_nested_definitions, standalone_document, Definition, GenericAgent,
    GenericHookService,
};
use crate::models::{AgentHookMutation, AgentHookOwnership};
use common::{ConfigStatus, JsonRead, PreparedHookConfiguration};
use serde_json::json;

impl GenericHookService {
    pub(super) fn prepare_configuration(
        &self,
        mutation: AgentHookMutation,
        config: &JsonRead,
        ownership: Option<&AgentHookOwnership>,
        definitions: &[Definition],
        structural_changes: bool,
    ) -> Result<PreparedHookConfiguration, String> {
        if matches!(
            config.status,
            ConfigStatus::Unsupported | ConfigStatus::Unsafe
        ) {
            return Err(config
                .diagnostic
                .clone()
                .unwrap_or_else(|| "Hook 配置格式不受支持".to_owned()));
        }
        let mut value = config
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.value.clone())
            .unwrap_or_else(|| json!({}));
        let mut remove = false;
        let write = match mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                let disabled = matches!(self.agent, GenericAgent::Gemini)
                    && is_definitions_disabled(&value, definitions);
                if self.agent.standalone() {
                    value = standalone_document(definitions);
                } else {
                    for definition in definitions {
                        add_nested_definition(&mut value, definition, self.agent)?;
                    }
                    if matches!(self.agent, GenericAgent::Gemini) {
                        remove_disabled(&mut value, definitions);
                    }
                }
                config.status == ConfigStatus::Missing || disabled || structural_changes
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                ownership.ok_or("缺少 Hook ownership manifest，未修改任何配置")?;
                if self.agent.standalone() {
                    remove = true;
                    config.status == ConfigStatus::Present
                } else if mutation == AgentHookMutation::Disable {
                    config.status == ConfigStatus::Present
                        && if matches!(self.agent, GenericAgent::Gemini) {
                            add_disabled(&mut value, definitions)?
                        } else {
                            remove_nested_definitions(&mut value, definitions)?
                        }
                } else {
                    let mut changed = remove_nested_definitions(&mut value, definitions)?;
                    if matches!(self.agent, GenericAgent::Gemini) {
                        changed |= remove_disabled(&mut value, definitions);
                    }
                    changed && config.status == ConfigStatus::Present
                }
            }
        };
        let after = if !write {
            config
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.text.clone())
        } else if remove {
            None
        } else {
            Some(
                String::from_utf8(common::encode_json(&value, self.agent.label())?)
                    .map_err(|error| error.to_string())?,
            )
        };
        Ok(PreparedHookConfiguration {
            value,
            after,
            write,
        })
    }

    pub(super) fn commit_configuration(
        &self,
        prepared: &PreparedHookConfiguration,
    ) -> Result<(), String> {
        if !prepared.write {
            return Ok(());
        }
        if let Some(text) = &prepared.after {
            if self.agent.standalone() {
                ensure_owned_leaf_parent(&self.config_path)?;
            } else {
                common::ensure_parent(&self.config_path)?;
            }
            common::write_atomic(&self.config_path, text.as_bytes())
        } else {
            common::ensure_parent(&self.config_path)?;
            std::fs::remove_file(&self.config_path)
                .map_err(|error| format!("删除 Hook 文件失败: {error}"))
        }
    }
}
