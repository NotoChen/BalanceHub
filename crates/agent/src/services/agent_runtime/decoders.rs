use super::hook::{
    HookDecodeError, HookEventDecoder, NormalizedHookEvent, NormalizedHookLifecycle,
};
use crate::models::{AgentCliKind, AgentRuntimeScope};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Selects the decoder used by the short-lived helper.  The helper receives
/// the Agent key from its Hook configuration, so this is deliberately the
/// only dispatch table for vendor payloads.
pub fn decoder_for_agent(agent: &str) -> Option<&'static dyn HookEventDecoder> {
    match agent.trim().to_ascii_lowercase().as_str() {
        "codex" => Some(&CodexHookDecoder),
        "claudecode" | "claude_code" | "claude" => Some(&ClaudeHookDecoder),
        "gemini" | "gemini_cli" => Some(&GeminiHookDecoder),
        "grok" | "grok_build" => Some(&GrokHookDecoder),
        _ => None,
    }
}

/// The Codex Hook contract is intentionally separate from the fixture adapter
/// below. Codex sends snake_case fields and does not provide a stable event ID.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct CodexHookInput {
    #[serde(alias = "hookEventName")]
    hook_event_name: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    #[serde(alias = "transcriptPath")]
    transcript_path: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(alias = "observedAt")]
    observed_at: Option<i64>,
    #[serde(alias = "schemaVersion")]
    schema_version: Option<u16>,
}

pub struct CodexHookDecoder;

impl HookEventDecoder for CodexHookDecoder {
    fn decode(
        &self,
        payload: &[u8],
        received_at: i64,
    ) -> Result<NormalizedHookEvent, HookDecodeError> {
        decode_codex_payload(payload, received_at)
    }
}

fn decode_codex_payload(
    payload: &[u8],
    received_at: i64,
) -> Result<NormalizedHookEvent, HookDecodeError> {
    let input: CodexHookInput =
        serde_json::from_slice(payload).map_err(|_| HookDecodeError::InvalidPayload)?;
    let schema_version = supported_schema(input.schema_version)?;
    let native_event = native_event(input.hook_event_name)?;
    let session_id = clean(input.session_id);
    let transcript_path = clean(input.transcript_path);
    let cwd = clean(input.cwd);
    let model = clean(input.model);
    let observed_at = input.observed_at.unwrap_or(received_at);
    let event_id = generated_event_id(
        AgentCliKind::Codex,
        &native_event,
        session_id.as_deref(),
        transcript_path.as_deref(),
        cwd.as_deref(),
        observed_at,
    );
    Ok(NormalizedHookEvent {
        schema_version,
        event_id,
        agent_kind: AgentCliKind::Codex,
        runtime_scope: AgentRuntimeScope::default(),
        native_event: native_event.clone(),
        lifecycle: lifecycle_for_native_event(&native_event)?,
        session_id,
        balancehub_instance_id: None,
        cwd,
        transcript_path,
        model,
        title: None,
        observed_at,
        received_at,
    })
}

/// Claude Code sends the common Hook fields in snake_case.  The optional
/// title and model fields are intentionally limited to fields documented for
/// lifecycle/model-switch events; prompt, transcript content and tool fields
/// are not represented here and therefore cannot reach the spool.
#[derive(Debug, Deserialize)]
struct ClaudeHookInput {
    #[serde(alias = "hookEventName")]
    hook_event_name: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    #[serde(alias = "transcriptPath")]
    transcript_path: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(alias = "sessionTitle")]
    session_title: Option<String>,
    #[serde(alias = "observedAt")]
    observed_at: Option<HookTimestamp>,
    timestamp: Option<HookTimestamp>,
    #[serde(alias = "schemaVersion")]
    schema_version: Option<u16>,
}

pub struct ClaudeHookDecoder;

impl HookEventDecoder for ClaudeHookDecoder {
    fn decode(
        &self,
        payload: &[u8],
        received_at: i64,
    ) -> Result<NormalizedHookEvent, HookDecodeError> {
        decode_claude_payload(payload, received_at)
    }
}

fn decode_claude_payload(
    payload: &[u8],
    received_at: i64,
) -> Result<NormalizedHookEvent, HookDecodeError> {
    let input: ClaudeHookInput =
        serde_json::from_slice(payload).map_err(|_| HookDecodeError::InvalidPayload)?;
    let schema_version = supported_schema(input.schema_version)?;
    let native_event = native_event(input.hook_event_name)?;
    let session_id = clean(input.session_id);
    let transcript_path = clean(input.transcript_path);
    let cwd = clean(input.cwd);
    let model = clean(input.model);
    let title = clean(input.session_title);
    let observed_at = timestamp_or_received(input.observed_at.or(input.timestamp), received_at);
    let event_id = generated_event_id(
        AgentCliKind::ClaudeCode,
        &native_event,
        session_id.as_deref(),
        transcript_path.as_deref(),
        cwd.as_deref(),
        observed_at,
    );
    Ok(NormalizedHookEvent {
        schema_version,
        event_id,
        agent_kind: AgentCliKind::ClaudeCode,
        runtime_scope: AgentRuntimeScope::default(),
        native_event: native_event.clone(),
        lifecycle: lifecycle_for_native_event(&native_event)?,
        session_id,
        balancehub_instance_id: None,
        cwd,
        transcript_path,
        model,
        title,
        observed_at,
        received_at,
    })
}

/// Gemini lifecycle Hook input contains no stable title/model field.  Do not
/// infer either value from the transcript or prompt: session adapters may
/// enrich those values separately after the normalized event is persisted.
#[derive(Debug, Deserialize)]
struct GeminiHookInput {
    #[serde(alias = "hookEventName")]
    hook_event_name: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    #[serde(alias = "transcriptPath")]
    transcript_path: Option<String>,
    cwd: Option<String>,
    timestamp: Option<HookTimestamp>,
    #[serde(alias = "observedAt")]
    observed_at: Option<HookTimestamp>,
    #[serde(alias = "schemaVersion")]
    schema_version: Option<u16>,
}

pub struct GeminiHookDecoder;

impl HookEventDecoder for GeminiHookDecoder {
    fn decode(
        &self,
        payload: &[u8],
        received_at: i64,
    ) -> Result<NormalizedHookEvent, HookDecodeError> {
        decode_gemini_payload(payload, received_at)
    }
}

fn decode_gemini_payload(
    payload: &[u8],
    received_at: i64,
) -> Result<NormalizedHookEvent, HookDecodeError> {
    let input: GeminiHookInput =
        serde_json::from_slice(payload).map_err(|_| HookDecodeError::InvalidPayload)?;
    let schema_version = supported_schema(input.schema_version)?;
    let native_event = native_event(input.hook_event_name)?;
    let session_id = clean(input.session_id);
    let transcript_path = clean(input.transcript_path);
    let cwd = clean(input.cwd);
    let observed_at = timestamp_or_received(input.observed_at.or(input.timestamp), received_at);
    let event_id = generated_event_id(
        AgentCliKind::Gemini,
        &native_event,
        session_id.as_deref(),
        transcript_path.as_deref(),
        cwd.as_deref(),
        observed_at,
    );
    Ok(NormalizedHookEvent {
        schema_version,
        event_id,
        agent_kind: AgentCliKind::Gemini,
        runtime_scope: AgentRuntimeScope::default(),
        native_event: native_event.clone(),
        lifecycle: lifecycle_for_native_event(&native_event)?,
        session_id,
        balancehub_instance_id: None,
        cwd,
        transcript_path,
        model: None,
        title: None,
        observed_at,
        received_at,
    })
}

/// Grok Build uses camelCase names and exposes workspaceRoot in addition to
/// cwd.  workspaceRoot is only a fallback for the normalized working
/// directory, never a provider/session identity.
#[derive(Debug, Deserialize)]
struct GrokHookInput {
    #[serde(alias = "hookEventName")]
    hook_event_name: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    #[serde(alias = "workspaceRoot")]
    workspace_root: Option<String>,
    timestamp: Option<HookTimestamp>,
    #[serde(alias = "observedAt", alias = "observed_at")]
    observed_at: Option<HookTimestamp>,
    #[serde(alias = "schemaVersion", alias = "schema_version")]
    schema_version: Option<u16>,
}

pub struct GrokHookDecoder;

impl HookEventDecoder for GrokHookDecoder {
    fn decode(
        &self,
        payload: &[u8],
        received_at: i64,
    ) -> Result<NormalizedHookEvent, HookDecodeError> {
        decode_grok_payload(payload, received_at)
    }
}

fn decode_grok_payload(
    payload: &[u8],
    received_at: i64,
) -> Result<NormalizedHookEvent, HookDecodeError> {
    let input: GrokHookInput =
        serde_json::from_slice(payload).map_err(|_| HookDecodeError::InvalidPayload)?;
    let schema_version = supported_schema(input.schema_version)?;
    let native_event = native_event(input.hook_event_name)?;
    let session_id = clean(input.session_id);
    let transcript_path = None;
    let cwd = clean(input.cwd).or_else(|| clean(input.workspace_root));
    let observed_at = timestamp_or_received(input.observed_at.or(input.timestamp), received_at);
    let event_id = generated_event_id(
        AgentCliKind::Grok,
        &native_event,
        session_id.as_deref(),
        transcript_path.as_deref(),
        cwd.as_deref(),
        observed_at,
    );
    Ok(NormalizedHookEvent {
        schema_version,
        event_id,
        agent_kind: AgentCliKind::Grok,
        runtime_scope: AgentRuntimeScope::default(),
        native_event: native_event.clone(),
        lifecycle: lifecycle_for_native_event(&native_event)?,
        session_id,
        balancehub_instance_id: None,
        cwd,
        transcript_path,
        model: None,
        title: None,
        observed_at,
        received_at,
    })
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HookTimestamp {
    Number(i64),
    Text(String),
}

fn supported_schema(schema_version: Option<u16>) -> Result<u16, HookDecodeError> {
    let schema_version = schema_version.unwrap_or(super::hook::NORMALIZED_HOOK_SCHEMA_VERSION);
    if schema_version != super::hook::NORMALIZED_HOOK_SCHEMA_VERSION {
        return Err(HookDecodeError::UnsupportedSchema(schema_version));
    }
    Ok(schema_version)
}

fn native_event(value: Option<String>) -> Result<String, HookDecodeError> {
    value
        .and_then(|value| clean(Some(value)))
        .ok_or(HookDecodeError::InvalidField("hook_event_name"))
}

fn timestamp_or_received(timestamp: Option<HookTimestamp>, received_at: i64) -> i64 {
    match timestamp {
        Some(HookTimestamp::Number(value)) => {
            if value.unsigned_abs() < 100_000_000_000 {
                value.saturating_mul(1_000)
            } else {
                value
            }
        }
        Some(HookTimestamp::Text(value)) => chrono::DateTime::parse_from_rfc3339(value.trim())
            .map(|date| date.timestamp_millis())
            .unwrap_or(received_at),
        None => received_at,
    }
}

fn generated_event_id(
    agent_kind: AgentCliKind,
    native_event: &str,
    session_id: Option<&str>,
    transcript_path: Option<&str>,
    cwd: Option<&str>,
    observed_at: i64,
) -> String {
    let mut hasher = Sha256::new();
    hash_component(&mut hasher, agent_kind.key());
    hash_component(&mut hasher, native_event);
    hash_component(&mut hasher, session_id.unwrap_or_default());
    hash_component(&mut hasher, transcript_path.unwrap_or_default());
    hash_component(&mut hasher, cwd.unwrap_or_default());
    hasher.update(observed_at.to_be_bytes());
    format!("hook:{}:{:x}", agent_kind.key(), hasher.finalize())
}

fn hash_component(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn lifecycle_for_native_event(
    native_event: &str,
) -> Result<NormalizedHookLifecycle, HookDecodeError> {
    let normalized = native_event
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");
    let lifecycle = match normalized.as_str() {
        "start" | "session_start" | "sessionstart" | "startup" | "conversation_start" => {
            NormalizedHookLifecycle::Start
        }
        "before_agent" | "beforeagent" | "turn_start" | "prompt_start" | "assistant_start"
        | "agent_start" | "user_prompt_submit" | "userpromptsubmit" => {
            NormalizedHookLifecycle::Busy
        }
        "stop" | "interrupt" | "turn_end" | "after_agent" | "assistant_stop" | "agent_stop"
        | "afteragent" | "idle" | "stop_failure" | "stopfailure" => NormalizedHookLifecycle::Idle,
        "session_end" | "sessionend" | "conversation_end" | "shutdown" | "exit" => {
            NormalizedHookLifecycle::Ended
        }
        "unknown" => NormalizedHookLifecycle::Unknown,
        _ => {
            return Err(HookDecodeError::UnsupportedNativeEvent(
                native_event.to_string(),
            ))
        }
    };
    Ok(lifecycle)
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().chars().take(4096).collect::<String>())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_uses_the_canonical_agent_keys() {
        assert!(decoder_for_agent("codex").is_some());
        assert!(decoder_for_agent("claudeCode").is_some());
        assert!(decoder_for_agent("gemini").is_some());
        assert!(decoder_for_agent("grok").is_some());
        assert!(decoder_for_agent("unknown-agent").is_none());
    }

    #[test]
    fn lifecycle_mapping_accepts_camel_case_vendor_event_names() {
        assert_eq!(
            lifecycle_for_native_event("StopFailure").unwrap(),
            NormalizedHookLifecycle::Idle
        );
        assert_eq!(
            lifecycle_for_native_event("UserPromptSubmit").unwrap(),
            NormalizedHookLifecycle::Busy
        );
        assert_eq!(
            lifecycle_for_native_event("SessionEnd").unwrap(),
            NormalizedHookLifecycle::Ended
        );
    }

    #[test]
    fn claude_decoder_keeps_only_allow_listed_metadata() {
        let payload = br#"{
            "hook_event_name":"SessionStart",
            "session_id":"claude-session",
            "transcript_path":"/tmp/claude.jsonl",
            "cwd":"/workspace",
            "model":"claude-sonnet",
            "session_title":"Build feature",
            "prompt":"private prompt",
            "tool_input":{"secret":"private tool input"},
            "timestamp":"2026-09-04T10:20:30Z"
        }"#;
        let event = ClaudeHookDecoder.decode(payload, 1).unwrap();
        assert_eq!(event.agent_kind, AgentCliKind::ClaudeCode);
        assert_eq!(event.lifecycle, NormalizedHookLifecycle::Start);
        assert_eq!(event.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(event.title.as_deref(), Some("Build feature"));
        assert_eq!(event.observed_at, 1_788_517_230_000);
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("private prompt"));
        assert!(!serialized.contains("private tool input"));
    }

    #[test]
    fn gemini_decoder_does_not_infer_model_or_title_from_prompt_data() {
        let payload = br#"{
            "hook_event_name":"BeforeAgent",
            "session_id":"gemini-session",
            "transcript_path":"/tmp/gemini.json",
            "cwd":"/workspace",
            "model":"must-not-be-read",
            "session_title":"must-not-be-read",
            "prompt":"must-not-be-read",
            "timestamp":1725444030
        }"#;
        let event = GeminiHookDecoder.decode(payload, 1).unwrap();
        assert_eq!(event.agent_kind, AgentCliKind::Gemini);
        assert_eq!(event.lifecycle, NormalizedHookLifecycle::Busy);
        assert_eq!(event.model, None);
        assert_eq!(event.title, None);
        assert_eq!(event.observed_at, 1_725_444_030_000);
    }

    #[test]
    fn grok_decoder_accepts_camel_case_and_workspace_fallback() {
        let payload = br#"{
            "hookEventName":"SessionStart",
            "sessionId":"grok-session",
            "workspaceRoot":"/workspace/grok",
            "prompt":"must-not-be-read"
        }"#;
        let event = GrokHookDecoder.decode(payload, 42).unwrap();
        assert_eq!(event.agent_kind, AgentCliKind::Grok);
        assert_eq!(event.lifecycle, NormalizedHookLifecycle::Start);
        assert_eq!(event.session_id.as_deref(), Some("grok-session"));
        assert_eq!(event.cwd.as_deref(), Some("/workspace/grok"));
        assert_eq!(event.transcript_path, None);
        assert_eq!(event.model, None);
        assert_eq!(event.title, None);
    }
}
