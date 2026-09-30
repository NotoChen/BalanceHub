//! Claude's native matcher decoder and matching algorithm, shared by typed decisions.
use super::control::{ListFamily, ListState, PolicyMatcher};
use crate::models::AgentMcpTransport;
use crate::services::agent_cli::contracts::{AgentAssetNativePayload, AgentMcpMatcherIdentity};
use serde_json::{Map, Value};

fn valid_policy_matcher(value: &Value, strict_name: bool) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 1 {
        return false;
    }
    let fields = ["serverName", "serverCommand", "serverUrl"];
    let Some(field) = fields
        .iter()
        .copied()
        .find(|field| object.contains_key(*field))
    else {
        return false;
    };
    if fields
        .iter()
        .filter(|candidate| object.contains_key(**candidate))
        .count()
        != 1
    {
        return false;
    }
    match field {
        "serverName" => object
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| {
                !value.is_empty()
                    && value == value.trim()
                    && (!strict_name
                        || value
                            .chars()
                            .all(|char| char.is_ascii_alphanumeric() || char == '_' || char == '-'))
            }),
        "serverUrl" => object
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty()),
        "serverCommand" => object
            .get(field)
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string)),
        _ => false,
    }
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut states = vec![false; text.chars().count() + 1];
    states[0] = true;
    for token in pattern.chars() {
        let mut next = vec![false; states.len()];
        if token == '*' {
            for (index, state) in states.iter().enumerate() {
                if *state {
                    for slot in next.iter_mut().skip(index) {
                        *slot = true;
                    }
                }
            }
        } else {
            for (index, character) in text.chars().enumerate() {
                if states[index] && character == token {
                    next[index + 1] = true;
                }
            }
        }
        states = next;
    }
    states[text.chars().count()]
}

pub(super) struct DecodedPolicyRules {
    pub(super) entries: Vec<Option<PolicyMatcher>>,
    pub(super) state: ListState,
}

fn decode_policy_rule(value: &Value, strict_name: bool) -> Option<PolicyMatcher> {
    if !valid_policy_matcher(value, strict_name) {
        return None;
    }
    let object = value.as_object()?;
    if let Some(name) = object.get("serverName") {
        return Some(PolicyMatcher::Name(name.as_str()?.to_owned()));
    }
    if let Some(command) = object.get("serverCommand") {
        return Some(PolicyMatcher::Command(
            command
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        ));
    }
    Some(PolicyMatcher::Url(
        object.get("serverUrl")?.as_str()?.to_owned(),
    ))
}

pub(super) fn decode_policy_rules(
    root: &Map<String, Value>,
    family: ListFamily,
) -> DecodedPolicyRules {
    let invalid_state = if family == ListFamily::Allowed {
        ListState::NativeFailClosed
    } else {
        ListState::Invalid
    };
    let Some(value) = root.get(family.field()) else {
        return DecodedPolicyRules {
            entries: Vec::new(),
            state: ListState::Absent,
        };
    };
    let Some(values) = value.as_array() else {
        return DecodedPolicyRules {
            entries: Vec::new(),
            state: invalid_state,
        };
    };
    // Each original slot survives decoding, including invalid members. The
    // completion count derives from this complete array before bounded emit.
    let entries = values
        .iter()
        .map(|value| decode_policy_rule(value, family == ListFamily::Allowed))
        .collect::<Vec<_>>();
    let state = if entries.iter().all(Option::is_some) {
        ListState::Valid
    } else {
        invalid_state
    };
    DecodedPolicyRules { entries, state }
}

pub(super) fn policy_rule_matches(
    matcher: &PolicyMatcher,
    id: &str,
    transport: AgentMcpTransport,
    payload: &AgentAssetNativePayload,
) -> bool {
    let identity = match payload {
        AgentAssetNativePayload::McpDefinition(definition)
        | AgentAssetNativePayload::ClaudePlugin(super::plugin::ClaudePluginPayload::Mcp {
            definition,
            ..
        }) => Some(&definition.identity),
        _ => None,
    };
    match matcher {
        PolicyMatcher::Name(name) => name == id,
        PolicyMatcher::Url(pattern) => matches!(identity,
            Some(AgentMcpMatcherIdentity::Remote { url }) if glob_matches(pattern, url)),
        PolicyMatcher::Command(expected) => {
            transport == AgentMcpTransport::Stdio
                && matches!(identity, Some(AgentMcpMatcherIdentity::Stdio { argv }) if expected == argv)
        }
    }
}

pub(super) fn possibly_equal_mcp_identity(
    left: &AgentMcpMatcherIdentity,
    right: &AgentMcpMatcherIdentity,
) -> bool {
    match (left, right) {
        (
            AgentMcpMatcherIdentity::Stdio { argv: left },
            AgentMcpMatcherIdentity::Stdio { argv: right },
        ) => left == right,
        (
            AgentMcpMatcherIdentity::Remote { url: left },
            AgentMcpMatcherIdentity::Remote { url: right },
        ) => {
            if left == right {
                return true;
            }
            let (Ok(mut left), Ok(mut right)) =
                (reqwest::Url::parse(left), reqwest::Url::parse(right))
            else {
                return false;
            };
            left.set_fragment(None);
            right.set_fragment(None);
            left.as_str().trim_end_matches('/') == right.as_str().trim_end_matches('/')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_match_is_anchored() {
        assert!(glob_matches(
            "https://*.example.test/*",
            "https://api.example.test/v1"
        ));
        assert!(!glob_matches(
            "https://*.example.test/*",
            "xhttps://api.example.test/v1"
        ));
    }

    #[test]
    fn policy_matcher_requires_one_recognized_field() {
        assert!(valid_policy_matcher(
            &serde_json::json!({"serverName": "alpha"}),
            true
        ));
        assert!(!valid_policy_matcher(
            &serde_json::json!({"serverName": "alpha", "note": "extra"}),
            true
        ));
        assert!(!valid_policy_matcher(
            &serde_json::json!({"serverName": "alpha", "serverUrl": "https://example.test"}),
            false
        ));
    }
}
