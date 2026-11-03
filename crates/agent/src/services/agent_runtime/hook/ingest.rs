//! Fail-open Hook input boundary. It performs no network, GUI or index work.

use super::{
    event::HookEventDecoder,
    spool::{HookIngestResult, HookSpoolRepository, SpoolDiagnostic},
};
use crate::models::AgentRuntimeScope;
use std::{
    env,
    time::{Duration, Instant},
};

#[cfg(test)]
use std::io::Read;

const BALANCEHUB_CLI_INSTANCE_ID: &str = "BALANCEHUB_CLI_INSTANCE_ID";

/// Trusted context supplied by the Hook process boundary rather than by the
/// Agent payload. This is the only place where a launch association enters a
/// normalized Hook event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookIngestContext {
    pub runtime_scope: AgentRuntimeScope,
    pub balancehub_instance_id: Option<String>,
}

impl Default for HookIngestContext {
    fn default() -> Self {
        Self {
            runtime_scope: AgentRuntimeScope::Native,
            balancehub_instance_id: None,
        }
    }
}

impl HookIngestContext {
    /// Reads the launch correlation ID only after the caller has verified it
    /// against its authoritative launch repository. The Hook payload cannot
    /// provide this association itself.
    pub(crate) fn from_process_env_with_validator(
        is_known_instance: impl FnOnce(&str) -> bool,
    ) -> Self {
        let balancehub_instance_id = env::var(BALANCEHUB_CLI_INSTANCE_ID)
            .ok()
            .and_then(|value| instance_id_from_value(value, is_known_instance));
        Self {
            runtime_scope: AgentRuntimeScope::Native,
            balancehub_instance_id,
        }
    }
}

#[cfg(test)]
pub fn ingest_payload(
    decoder: &dyn HookEventDecoder,
    payload: &[u8],
    received_at: i64,
    spool: &HookSpoolRepository,
) -> HookIngestResult {
    ingest_payload_with_context(
        decoder,
        payload,
        received_at,
        spool,
        &HookIngestContext::default(),
    )
}

pub fn ingest_payload_with_context(
    decoder: &dyn HookEventDecoder,
    payload: &[u8],
    received_at: i64,
    spool: &HookSpoolRepository,
    context: &HookIngestContext,
) -> HookIngestResult {
    ingest_payload_with_budget_and_context(
        decoder,
        payload,
        received_at,
        spool,
        Duration::from_secs(2),
        context,
    )
}

fn ingest_payload_with_budget_and_context(
    decoder: &dyn HookEventDecoder,
    payload: &[u8],
    received_at: i64,
    spool: &HookSpoolRepository,
    budget: Duration,
    context: &HookIngestContext,
) -> HookIngestResult {
    let started = Instant::now();
    if payload.len() > spool.limits().max_event_bytes {
        return failed(SpoolDiagnostic::PayloadTooLarge);
    }
    let mut event = match decoder.decode(payload, received_at) {
        Ok(event) => event,
        Err(super::event::HookDecodeError::UnsupportedSchema(_)) => {
            return failed(SpoolDiagnostic::UnsupportedSchema)
        }
        Err(_) => return failed(SpoolDiagnostic::InvalidPayload),
    };
    event.runtime_scope = context.runtime_scope.clone();
    event.balancehub_instance_id = context
        .balancehub_instance_id
        .as_deref()
        .filter(|value| valid_instance_id(value))
        .map(str::trim)
        .map(str::to_string);
    if started.elapsed() > budget {
        return failed(SpoolDiagnostic::Unavailable);
    }
    spool.append(&event)
}

#[cfg(test)]
pub fn ingest_stdin(
    decoder: &dyn HookEventDecoder,
    stdin: &mut dyn Read,
    received_at: i64,
    spool: &HookSpoolRepository,
) -> HookIngestResult {
    ingest_stdin_with_context(
        decoder,
        stdin,
        received_at,
        spool,
        &HookIngestContext::default(),
    )
}

#[cfg(test)]
fn ingest_stdin_with_context(
    decoder: &dyn HookEventDecoder,
    stdin: &mut dyn Read,
    received_at: i64,
    spool: &HookSpoolRepository,
    context: &HookIngestContext,
) -> HookIngestResult {
    let limit = spool.limits().max_event_bytes;
    let mut payload = Vec::with_capacity(limit.min(16 * 1024));
    let read_result = stdin
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut payload);
    if read_result.is_err() || payload.len() > limit {
        return failed(SpoolDiagnostic::PayloadTooLarge);
    }
    ingest_payload_with_budget_and_context(
        decoder,
        &payload,
        received_at,
        spool,
        Duration::from_secs(2),
        context,
    )
}

fn valid_instance_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 256
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn instance_id_from_value(
    value: String,
    is_known_instance: impl FnOnce(&str) -> bool,
) -> Option<String> {
    let value = value.trim();
    (valid_instance_id(value) && is_known_instance(value)).then(|| value.to_string())
}

fn failed(diagnostic: SpoolDiagnostic) -> HookIngestResult {
    HookIngestResult {
        accepted: false,
        event_id: None,
        diagnostic: Some(diagnostic),
    }
}

#[cfg(test)]
mod tests {
    use super::instance_id_from_value;

    #[test]
    fn launch_id_requires_valid_format_and_authoritative_presence() {
        assert_eq!(
            instance_id_from_value(" known-1 ".to_string(), |id| id == "known-1"),
            Some("known-1".to_string())
        );
        assert_eq!(
            instance_id_from_value("unknown".to_string(), |_| false),
            None
        );
        assert_eq!(
            instance_id_from_value("../escape".to_string(), |_| true),
            None
        );
    }
}
