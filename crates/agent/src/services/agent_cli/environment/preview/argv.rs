use super::{safe_url, AgentPreviewScalarKind, REDACTED};
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub(crate) enum AgentArgvValueKind {
    Integer,
    Host,
    Path,
    Url,
    Enum(&'static [&'static str]),
}

#[derive(Debug)]
pub(crate) struct AgentArgvValueRule {
    pub(crate) flag: &'static str,
    pub(crate) kind: AgentArgvValueKind,
}

#[derive(Debug)]
pub(crate) struct AgentArgvPolicy {
    pub(crate) command_field: &'static str,
    /// Only these known launcher grammars may expose positional arguments.
    pub(crate) launchers: &'static [&'static str],
    pub(crate) boolean_flags: &'static [&'static str],
    pub(crate) value_flags: &'static [AgentArgvValueRule],
    pub(crate) credential_flags: &'static [&'static str],
}

pub(super) fn redact_argv(
    value: &Value,
    command: Option<&str>,
    grammar: &AgentArgvPolicy,
) -> (Value, Vec<String>) {
    let mut secrets = Vec::new();
    let Some(values) = value.as_array() else {
        collect(value, &mut secrets);
        return (Value::String(REDACTED.to_string()), secrets);
    };
    let values = match values.iter().map(Value::as_str).collect::<Option<Vec<_>>>() {
        Some(values) => values,
        None => {
            collect(value, &mut secrets);
            return (Value::String(REDACTED.to_string()), secrets);
        }
    };
    let program = command.and_then(|value| value.rsplit(['/', '\\']).next());
    let known_launcher = program.is_some_and(|value| grammar.launchers.contains(&value));
    let mut output = Vec::<Value>::with_capacity(values.len());
    let mut index = 0;
    let mut leading_positional = true;
    while index < values.len() {
        let value = values[index];
        let (flag, inline) = value
            .split_once('=')
            .map_or((value, None), |(flag, value)| (flag, Some(value)));
        if grammar.credential_flags.contains(&flag) {
            if let Some(inline) = inline {
                secrets.push(inline.to_string());
                output.push(Value::String(format!("{flag}={REDACTED}")));
            } else {
                output.push(Value::String(flag.to_string()));
                if let Some(value) = values.get(index + 1) {
                    secrets.push((*value).to_string());
                    output.push(Value::String(REDACTED.to_string()));
                    index += 1;
                }
            }
        } else if known_launcher && inline.is_none() && grammar.boolean_flags.contains(&flag) {
            output.push(Value::String(value.to_string()));
        } else if let Some(rule) = known_launcher
            .then(|| grammar.value_flags.iter().find(|rule| rule.flag == flag))
            .flatten()
        {
            if let Some(inline) = inline {
                if safe_value(inline, rule.kind) {
                    output.push(Value::String(value.to_string()));
                } else {
                    secrets.push(inline.to_string());
                    output.push(Value::String(format!("{flag}={REDACTED}")));
                }
            } else {
                output.push(Value::String(flag.to_string()));
                if let Some(value) = values.get(index + 1) {
                    if safe_value(value, rule.kind) {
                        output.push(Value::String((*value).to_string()));
                    } else {
                        secrets.push((*value).to_string());
                        output.push(Value::String(REDACTED.to_string()));
                    }
                    index += 1;
                }
            }
        } else if known_launcher
            && leading_positional
            && safe_leading_argument(program.unwrap_or_default(), value)
        {
            output.push(Value::String(value.to_string()));
            leading_positional = false;
        } else {
            secrets.push(value.to_string());
            if let Some(inline) = inline {
                secrets.push(inline.to_string());
            }
            output.push(Value::String(REDACTED.to_string()));
            leading_positional = false;
            if value.starts_with('-') && inline.is_none() {
                if let Some(next) = values.get(index + 1).filter(|next| !next.starts_with('-')) {
                    secrets.push((*next).to_string());
                    output.push(Value::String(REDACTED.to_string()));
                    index += 1;
                }
            }
        }
        index += 1;
    }
    (Value::Array(output), secrets)
}

fn safe_value(value: &str, kind: AgentArgvValueKind) -> bool {
    match kind {
        AgentArgvValueKind::Integer => value.parse::<u32>().is_ok(),
        AgentArgvValueKind::Host => {
            value == "localhost" || value.parse::<std::net::IpAddr>().is_ok()
        }
        AgentArgvValueKind::Path => super::scalar_is_safe(
            &Value::String(value.to_string()),
            AgentPreviewScalarKind::Path,
        ),
        AgentArgvValueKind::Url => safe_url(value, &[]),
        AgentArgvValueKind::Enum(values) => values.contains(&value),
    }
}

fn safe_leading_argument(program: &str, value: &str) -> bool {
    if value.starts_with('-')
        || value.len() > 4096
        || value
            .chars()
            .any(|ch| ch.is_control() || " \t;&|$<>\"'(){}=?#".contains(ch) || ch == '\u{0060}')
    {
        return false;
    }
    match program {
        "node" | "node.exe" => [".js", ".mjs", ".cjs"]
            .iter()
            .any(|suffix| value.ends_with(suffix)),
        "python" | "python3" | "python.exe" | "python3.exe" => value.ends_with(".py"),
        "npx" | "npx.cmd" | "uvx" | "uvx.exe" => {
            !value.is_empty()
                && value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "@/_-.".contains(ch))
        }
        _ => false,
    }
}

fn collect(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect(value, output);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect(value, output);
            }
        }
        _ => {}
    }
}
