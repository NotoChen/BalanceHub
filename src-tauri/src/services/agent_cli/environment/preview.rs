//! Schema-owned preview rules. Parsing and redaction always precede display truncation.

mod argv;

use super::config_document as document;
pub(crate) use super::config_document::ConfigDocumentFormat as AgentPreviewFormat;

use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) use argv::{AgentArgvPolicy, AgentArgvValueKind, AgentArgvValueRule};

pub(crate) const REDACTED: &str = "[已隐藏]";
const MAX_PREVIEW_BYTES: usize = 128 * 1024;
const MAX_DOCUMENT_NODES: usize = 32_768;
const MAX_SECRET_VALUES: usize = 4_096;

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentSourcePreviewPolicy {
    MetadataOnly,
    Structured(&'static AgentPreviewPolicy),
}

impl AgentSourcePreviewPolicy {
    pub(crate) const fn metadata_only() -> Self {
        Self::MetadataOnly
    }

    pub(crate) const fn structured(policy: &'static AgentPreviewPolicy) -> Self {
        Self::Structured(policy)
    }

    pub(crate) fn identity(self) -> String {
        match self {
            Self::MetadataOnly => "metadata-only:v1".to_string(),
            Self::Structured(policy) => format!(
                "{}:{}:{}:{}:{:?}",
                policy.schema_id,
                policy.schema_version,
                policy.policy_id,
                policy.policy_version,
                policy.format
            ),
        }
    }
}

#[derive(Debug)]
pub(crate) struct AgentPreviewPolicy {
    pub(crate) schema_id: &'static str,
    pub(crate) schema_version: u32,
    pub(crate) policy_id: &'static str,
    pub(crate) policy_version: u32,
    pub(crate) format: AgentPreviewFormat,
    pub(crate) rules: &'static [AgentPreviewRule],
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentPreviewPathSegment {
    Key(&'static str),
    AnyKey,
    AnyIndex,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentPreviewScalarKind {
    Boolean,
    Number,
    Identifier,
    Executable,
    Path,
    Enum(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentPreviewRuleAction {
    RevealScalar(AgentPreviewScalarKind),
    RevealArgv(&'static AgentArgvPolicy),
    RevealSafeUrl(&'static [&'static str]),
    RedactValue,
    RedactDescendantValuesKeepKeys,
}

#[derive(Debug)]
pub(crate) struct AgentPreviewRule {
    pub(crate) path: &'static [AgentPreviewPathSegment],
    pub(crate) action: AgentPreviewRuleAction,
}

impl AgentPreviewRule {
    pub(crate) const fn scalar(
        path: &'static [AgentPreviewPathSegment],
        kind: AgentPreviewScalarKind,
    ) -> Self {
        Self {
            path,
            action: AgentPreviewRuleAction::RevealScalar(kind),
        }
    }

    pub(crate) const fn argv(
        path: &'static [AgentPreviewPathSegment],
        policy: &'static AgentArgvPolicy,
    ) -> Self {
        Self {
            path,
            action: AgentPreviewRuleAction::RevealArgv(policy),
        }
    }

    pub(crate) const fn url(
        path: &'static [AgentPreviewPathSegment],
        allowed_query_keys: &'static [&'static str],
    ) -> Self {
        Self {
            path,
            action: AgentPreviewRuleAction::RevealSafeUrl(allowed_query_keys),
        }
    }

    pub(crate) const fn hide(path: &'static [AgentPreviewPathSegment]) -> Self {
        Self {
            path,
            action: AgentPreviewRuleAction::RedactValue,
        }
    }

    pub(crate) const fn hide_values(path: &'static [AgentPreviewPathSegment]) -> Self {
        Self {
            path,
            action: AgentPreviewRuleAction::RedactDescendantValuesKeepKeys,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPreviewMetadataReason {
    UnsupportedSchema,
    InvalidDocument,
    ReadLimit,
}

#[derive(Debug)]
pub(crate) struct AgentPreviewOutput {
    pub(crate) content: Option<String>,
    pub(crate) redacted: bool,
    pub(crate) truncated: bool,
    pub(crate) metadata_reason: Option<AgentPreviewMetadataReason>,
}

impl AgentPreviewOutput {
    pub(crate) fn metadata(reason: AgentPreviewMetadataReason) -> Self {
        Self {
            content: None,
            redacted: false,
            truncated: false,
            metadata_reason: Some(reason),
        }
    }
}

#[derive(Clone)]
enum PathPart {
    Key(String),
    Index,
}

#[derive(Default)]
struct PreviewState {
    secrets: BTreeSet<String>,
    nodes: usize,
    redacted: bool,
}

pub(crate) fn render_preview(policy: AgentSourcePreviewPolicy, bytes: &[u8]) -> AgentPreviewOutput {
    let AgentSourcePreviewPolicy::Structured(policy) = policy else {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::UnsupportedSchema);
    };
    if bytes.len() > crate::models::AgentAssetLimits::HARD_CAP.bytes_per_source {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::ReadLimit);
    }
    if policy.schema_id.is_empty()
        || policy.schema_version == 0
        || policy.policy_id.is_empty()
        || policy.policy_version == 0
    {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::UnsupportedSchema);
    }
    let Some(original) = document::parse(bytes, policy.format) else {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::InvalidDocument);
    };
    let Some((redacted, was_redacted)) = redact_document(policy, &original) else {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::ReadLimit);
    };
    let Some(mut content) = document::serialize(&redacted, policy.format) else {
        return AgentPreviewOutput::metadata(AgentPreviewMetadataReason::InvalidDocument);
    };
    let truncated = content.len() > MAX_PREVIEW_BYTES;
    if truncated {
        let mut boundary = MAX_PREVIEW_BYTES;
        while !content.is_char_boundary(boundary) {
            boundary -= 1;
        }
        content.truncate(boundary);
    }
    AgentPreviewOutput {
        content: Some(content),
        redacted: was_redacted,
        truncated,
        metadata_reason: None,
    }
}

/// Full private structured projection used to locate lossless editing spans.
/// Does not serialize or truncate the original source.
pub(crate) fn redact_document(
    policy: &AgentPreviewPolicy,
    original: &Value,
) -> Option<(Value, bool)> {
    let mut state = PreviewState::default();
    let mut redacted = redact_node(
        original,
        original,
        &mut Vec::new(),
        policy,
        &mut state,
        false,
    )?;
    taint_revealed_values(&mut redacted, &state.secrets, &mut state.redacted);
    Some((redacted, state.redacted))
}

fn redact_node(
    value: &Value,
    root: &Value,
    path: &mut Vec<PathPart>,
    policy: &AgentPreviewPolicy,
    state: &mut PreviewState,
    inherited_hide: bool,
) -> Option<Value> {
    state.nodes += 1;
    if state.nodes > MAX_DOCUMENT_NODES || path.len() > 64 {
        return None;
    }
    let matched = policy
        .rules
        .iter()
        .filter(|rule| path_matches(rule.path, path))
        .collect::<Vec<_>>();
    let hide_value = matched
        .iter()
        .any(|rule| matches!(rule.action, AgentPreviewRuleAction::RedactValue));
    let hide_descendants = inherited_hide
        || matched.iter().any(|rule| {
            matches!(
                rule.action,
                AgentPreviewRuleAction::RedactDescendantValuesKeepKeys
            )
        });
    if hide_value {
        collect_secrets(value, state)?;
        state.redacted = true;
        return Some(Value::String(REDACTED.to_string()));
    }
    if !hide_descendants {
        for rule in &matched {
            match rule.action {
                AgentPreviewRuleAction::RevealScalar(kind) if scalar_is_safe(value, kind) => {
                    return Some(value.clone())
                }
                AgentPreviewRuleAction::RevealSafeUrl(keys)
                    if value.as_str().is_some_and(|value| safe_url(value, keys)) =>
                {
                    return Some(value.clone());
                }
                AgentPreviewRuleAction::RevealArgv(grammar) => {
                    let command =
                        sibling_value(root, path, grammar.command_field).and_then(Value::as_str);
                    let (rendered, hidden) = argv::redact_argv(value, command, grammar);
                    for secret in hidden {
                        add_secret(&secret, state)?;
                    }
                    state.redacted |= rendered != *value;
                    return Some(rendered);
                }
                _ => {}
            }
        }
    }
    match value {
        Value::Object(values) => {
            let mut output = serde_json::Map::new();
            for (key, value) in values {
                path.push(PathPart::Key(key.clone()));
                let result = redact_node(value, root, path, policy, state, hide_descendants)?;
                path.pop();
                output.insert(key.clone(), result);
            }
            Some(Value::Object(output))
        }
        Value::Array(values) => {
            let mut output = Vec::with_capacity(values.len());
            for value in values {
                path.push(PathPart::Index);
                output.push(redact_node(
                    value,
                    root,
                    path,
                    policy,
                    state,
                    hide_descendants,
                )?);
                path.pop();
            }
            Some(Value::Array(output))
        }
        _ => {
            collect_secrets(value, state)?;
            state.redacted = true;
            Some(Value::String(REDACTED.to_string()))
        }
    }
}

fn path_matches(rule: &[AgentPreviewPathSegment], path: &[PathPart]) -> bool {
    rule.len() == path.len()
        && rule
            .iter()
            .zip(path)
            .all(|(rule, part)| match (rule, part) {
                (AgentPreviewPathSegment::Key(expected), PathPart::Key(actual)) => {
                    *expected == actual
                }
                (AgentPreviewPathSegment::AnyKey, PathPart::Key(_))
                | (AgentPreviewPathSegment::AnyIndex, PathPart::Index) => true,
                _ => false,
            })
}

fn sibling_value<'a>(root: &'a Value, path: &[PathPart], key: &str) -> Option<&'a Value> {
    let mut value = root;
    for part in path.iter().take(path.len().saturating_sub(1)) {
        match part {
            PathPart::Key(key) => value = value.get(key)?,
            PathPart::Index => return None,
        }
    }
    value.get(key)
}

fn scalar_is_safe(value: &Value, kind: AgentPreviewScalarKind) -> bool {
    match kind {
        AgentPreviewScalarKind::Boolean => value.is_boolean(),
        AgentPreviewScalarKind::Number => value.is_number(),
        AgentPreviewScalarKind::Enum(allowed) => {
            value.as_str().is_some_and(|value| allowed.contains(&value))
        }
        AgentPreviewScalarKind::Identifier => value.as_str().is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 256
                && value
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || "._-/:@".contains(ch))
        }),
        AgentPreviewScalarKind::Executable | AgentPreviewScalarKind::Path => {
            value.as_str().is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 4096
                    && !value.contains("://")
                    && !value.chars().any(|ch| {
                        ch.is_control()
                            || " \t\r\n;&|$<>\"'(){}=?#".contains(ch)
                            || ch == '\u{0060}'
                    })
                    && (matches!(kind, AgentPreviewScalarKind::Executable)
                        || std::path::Path::new(value).is_absolute())
            })
        }
    }
}

fn safe_url(value: &str, allowed_query_keys: &[&str]) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https" | "ws" | "wss")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url
                .query_pairs()
                .all(|(key, _)| allowed_query_keys.contains(&key.as_ref()))
    })
}

fn collect_secrets(value: &Value, state: &mut PreviewState) -> Option<()> {
    collect_secret_values(value, state, 0)
}

fn collect_secret_values(value: &Value, state: &mut PreviewState, depth: usize) -> Option<()> {
    state.nodes += 1;
    if state.nodes > MAX_DOCUMENT_NODES || depth > 64 {
        return None;
    }
    match value {
        Value::String(value) => add_secret(value, state),
        Value::Number(value) => add_secret(&value.to_string(), state),
        Value::Array(values) => {
            for value in values {
                collect_secret_values(value, state, depth + 1)?;
            }
            Some(())
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_secret_values(value, state, depth + 1)?;
            }
            Some(())
        }
        _ => Some(()),
    }
}

fn add_secret(value: &str, state: &mut PreviewState) -> Option<()> {
    if !value.is_empty() {
        state.secrets.insert(value.to_string());
        if let Ok(url) = reqwest::Url::parse(value) {
            for (_, value) in url.query_pairs() {
                if !value.is_empty() {
                    state.secrets.insert(value.into_owned());
                }
            }
            if let Some(password) = url.password().filter(|value| !value.is_empty()) {
                state.secrets.insert(password.to_string());
            }
        }
    }
    (state.secrets.len() <= MAX_SECRET_VALUES).then_some(())
}

fn taint_revealed_values(value: &mut Value, secrets: &BTreeSet<String>, redacted: &mut bool) {
    match value {
        Value::String(value) if value != REDACTED => {
            let decoded = percent_decoded(value);
            if secrets
                .iter()
                .any(|secret| value.contains(secret) || decoded.contains(secret))
            {
                *value = REDACTED.to_string();
                *redacted = true;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                taint_revealed_values(value, secrets, redacted);
            }
        }
        Value::Array(values) => {
            for value in values {
                taint_revealed_values(value, secrets, redacted);
            }
        }
        _ => {}
    }
}

fn percent_decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            ) {
                output.push((a * 16 + b) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests;
