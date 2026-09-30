//! Versioned native Hook variants and local library edits.
use super::native::hooks::HookNativeDefinition;
use crate::{
    models::{AgentCatalogHookInput, AgentCatalogHookVariantInput, AgentCliKind},
    services::agent_cli::{definition, environment::config_document},
};
use std::collections::BTreeMap;

pub(crate) type HookDefinitions = BTreeMap<AgentCliKind, HookNativeDefinition>;

pub(super) fn input(input: &AgentCatalogHookInput) -> Result<HookDefinitions, String> {
    if input.variants.is_empty() || input.variants.len() > AgentCliKind::ALL.len() {
        return Err("Hook 至少需要一种 Agent 原生定义，且每个 Agent 只能有一份".to_owned());
    }
    let mut result = BTreeMap::new();
    for variant in &input.variants {
        if result.contains_key(&variant.agent_kind) {
            return Err("同一 Hook 不能包含重复的 Agent 变体".to_owned());
        }
        if variant.group_json.len() > super::package::MAX_FILE_BYTES {
            return Err("Hook 定义超过 512 KiB".to_owned());
        }
        let group = config_document::parse(
            variant.group_json.as_bytes(),
            config_document::ConfigDocumentFormat::Json,
        )
        .ok_or("Hook 原生片段必须是无重复字段的有效 JSON")?;
        let value = HookNativeDefinition {
            event: variant.event.clone(),
            group,
        };
        validate(variant.agent_kind, &value)?;
        result.insert(variant.agent_kind, value);
    }
    Ok(result)
}

pub(super) fn validate(kind: AgentCliKind, value: &HookNativeDefinition) -> Result<(), String> {
    if value.event.contains(['\0', '\n', '\r'])
        || value.event.is_empty()
        || value.event.len() > 128
        || serde_json::to_vec(value)
            .map_err(|_| "Hook 定义无效")?
            .len()
            > super::package::MAX_FILE_BYTES
    {
        return Err("Hook 事件或原生定义无效/超出限制".to_owned());
    }
    let adapter = definition(kind)
        .environment()
        .hook_adapter()
        .ok_or("该 Agent 没有已注册的 Hook 原生定义能力")?;
    (adapter.validate_definition)(value)
}

pub(super) fn public(values: &HookDefinitions) -> AgentCatalogHookInput {
    AgentCatalogHookInput {
        variants: values
            .iter()
            .map(|(kind, value)| AgentCatalogHookVariantInput {
                agent_kind: *kind,
                event: value.event.clone(),
                group_json: serde_json::to_string_pretty(&value.group).unwrap_or_default(),
            })
            .collect(),
    }
}

pub(super) fn fingerprint(value: &HookNativeDefinition) -> String {
    super::digest(&serde_json::to_vec(value).unwrap_or_default())
}
