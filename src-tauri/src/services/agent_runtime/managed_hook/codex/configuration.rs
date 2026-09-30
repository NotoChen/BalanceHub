use super::super::common::PreparedHookConfiguration;
use super::{
    format::{add_definition, definitions, remove_owned_definitions},
    ownership::{encode_config, ConfigRead, ConfigStatus},
    CodexHookService,
};
use crate::models::{AgentHookMutation, AgentHookOwnership};
use serde_json::json;

impl CodexHookService {
    pub(super) fn prepare_configuration(
        &self,
        mutation: AgentHookMutation,
        config: &ConfigRead,
        ownership: Option<&AgentHookOwnership>,
        structural_changes: bool,
    ) -> Result<PreparedHookConfiguration, String> {
        if matches!(
            config.status,
            ConfigStatus::Unsupported | ConfigStatus::Unsafe
        ) {
            return Err(config
                .diagnostic
                .clone()
                .unwrap_or_else(|| "Codex Hook 配置格式不受支持".to_owned()));
        }
        let mut value = config
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.value.clone())
            .unwrap_or_else(|| json!({}));
        let write = match mutation {
            AgentHookMutation::Install | AgentHookMutation::Enable => {
                for definition in definitions(&self.helper_path, &self.spool_root) {
                    add_definition(&mut value, &definition)?;
                }
                structural_changes
            }
            AgentHookMutation::Disable | AgentHookMutation::Remove => {
                if let Some(ownership) = ownership {
                    remove_owned_definitions(&mut value, ownership)?;
                    config.status == ConfigStatus::Present
                        && (mutation != AgentHookMutation::Remove || ownership.enabled)
                } else if config.status == ConfigStatus::Missing {
                    false
                } else {
                    return Err("缺少 Hook ownership manifest，未删除任何配置".to_owned());
                }
            }
        };
        let after = if write {
            Some(String::from_utf8(encode_config(&value)?).map_err(|error| error.to_string())?)
        } else {
            config
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.text.clone())
        };
        Ok(PreparedHookConfiguration {
            value,
            after,
            write,
        })
    }
}
