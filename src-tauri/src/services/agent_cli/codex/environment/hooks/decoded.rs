//! Native decoding shared by parsing and independent state assessment.
use super::CodexHookDefinitionEvidence;
use serde_json::Value;

pub(in crate::services::agent_cli::codex::environment) struct DecodedHookRule {
    pub current_hash: String,
    pub builtin: bool,
}

pub(in crate::services::agent_cli::codex::environment) fn decode_definition(
    evidence: &CodexHookDefinitionEvidence,
) -> Option<DecodedHookRule> {
    let handler = evidence.group.get("hooks")?.get(evidence.handler_index)?;
    if !super::valid_event(&evidence.event)
        || !super::valid_group(&evidence.group)
        || !super::valid_handler(handler)
        || !super::valid_matcher(&evidence.event, &evidence.group)
        || (evidence.event == "SessionEnd"
            && handler.get("type").and_then(Value::as_str) == Some("mcp_tool"))
    {
        return None;
    }
    Some(DecodedHookRule {
        current_hash: super::normalized_hash(&evidence.event, &evidence.group, handler)?,
        builtin: evidence.plugin.as_ref().is_some_and(|binding| {
            super::super::native::plugin_hooks::builtin(
                &binding.plugin_id,
                &evidence.event,
                &evidence.group,
                handler,
            )
        }),
    })
}
