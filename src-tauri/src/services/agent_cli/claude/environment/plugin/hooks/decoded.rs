//! Decode a complete native Hook source once before emitting typed observations.
use super::{guard_safe, schema};
use serde_json::{Map, Value};

#[derive(Clone, PartialEq)]
pub(crate) struct HookRule {
    pub(in crate::services::agent_cli::claude::environment) event: String,
    pub(in crate::services::agent_cli::claude::environment) group_index: usize,
    pub(in crate::services::agent_cli::claude::environment) handler_index: usize,
    definition: Value,
}

#[derive(Clone, PartialEq)]
pub(crate) struct HookDocument {
    pub(in crate::services::agent_cli::claude::environment) loadable: bool,
    pub(in crate::services::agent_cli::claude::environment) rules: Vec<HookRule>,
    pub(super) diagnostics: Vec<&'static str>,
}

pub(super) fn inline_document(events: &Map<String, Value>) -> HookDocument {
    decode_document(&serde_json::json!({ "hooks": events }))
}

pub(super) fn decode_document(root: &Value) -> HookDocument {
    let mut document = HookDocument {
        loadable: guard_safe(root),
        rules: Vec::new(),
        diagnostics: Vec::new(),
    };
    if !document.loadable {
        document.diagnostics.push("hooks.unloadableGuard");
        return document;
    }
    if root
        .get("modules")
        .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
    {
        document.diagnostics.push("hooks.modules.runtimeExpansion");
    }
    let Some(events) = root.get("hooks") else {
        return document;
    };
    let Some(events) = events.as_object() else {
        document.diagnostics.push("hooks.invalidEvents");
        return document;
    };
    for (event, groups) in events {
        if !schema::valid_hook_event(event) {
            document.diagnostics.push("hooks.unknownEvent");
            continue;
        }
        let Some(groups) = groups.as_array() else {
            document.diagnostics.push("hooks.invalidGroups");
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(group) = group
                .as_object()
                .filter(|group| schema::valid_hook_matcher(group))
            else {
                document.diagnostics.push("hooks.invalidMatcher");
                continue;
            };
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                document.diagnostics.push("hooks.invalidHandlers");
                continue;
            };
            for (handler_index, handler) in handlers.iter().enumerate() {
                if !schema::valid_hook_entry(handler) {
                    document.diagnostics.push("hooks.invalidHandler");
                    continue;
                }
                let mut selected = group.clone();
                selected.insert("hooks".to_owned(), Value::Array(vec![handler.clone()]));
                document.rules.push(HookRule {
                    event: event.clone(),
                    group_index,
                    handler_index,
                    definition: Value::Object(selected),
                });
            }
        }
    }
    document
}
