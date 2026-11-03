//! Translate references, never resolve environment values or execute helpers.
use super::{mcp_options, NativeCatalogAdapter};
use crate::services::agent_cli::catalog::definition::McpDefinition;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::LazyLock};

static VARIABLE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:-[^}]*)?\}|\$([A-Za-z_][A-Za-z0-9_]*)")
        .expect("constant environment reference pattern")
});

fn expands(agent: &str, field: &str) -> bool {
    match agent {
        "claude" | "grok" => matches!(field, "command" | "args" | "env" | "url" | "headers"),
        "gemini" => matches!(field, "env" | "headers"),
        _ => false,
    }
}

pub(super) fn supports_reference_option(target: &NativeCatalogAdapter, key: &str) -> bool {
    match key {
        "bearerTokenEnvVar" => {
            matches!(target.key, "codex" | "grok") || expands(target.key, "headers")
        }
        "headerEnvironment" => target.key == "codex" || expands(target.key, "headers"),
        "environmentVariables" => target.key == "codex" || expands(target.key, "env"),
        _ => false,
    }
}

fn template(agent: &str, field: &str, value: &str) -> Option<String> {
    if !expands(agent, field) {
        return None;
    }
    let mut found = false;
    let normalized = VARIABLE.replace_all(value, |captures: &regex::Captures<'_>| {
        if let Some(name) = captures.get(1) {
            found = true;
            format!(
                "${{{}{}}}",
                name.as_str(),
                captures.get(2).map_or("", |value| value.as_str())
            )
        } else if agent == "gemini" {
            found = true;
            format!("${{{}}}", &captures[3])
        } else {
            captures[0].to_owned()
        }
    });
    found.then(|| normalized.into_owned())
}

pub(super) fn document_value(agent: &str, field: &str, value: &str) -> Value {
    if agent == "grok" && field == "headers" && value.contains("{{session_id}}") {
        return serde_json::json!({"grokSessionTemplate": value});
    }
    template(agent, field, value).map_or_else(
        || Value::String(value.to_owned()),
        |value| serde_json::json!({"template": value}),
    )
}

pub(crate) fn validation_url(agent: &str, value: &str, websocket: bool) -> String {
    let Some(template) = template(agent, "url", value) else {
        return value.to_owned();
    };
    // Substitute syntax markers only. Never read the process environment.
    let starts_with_variable = template.starts_with("${");
    let mut first = true;
    VARIABLE
        .replace_all(&template, |_: &regex::Captures<'_>| {
            let replacement = if first && starts_with_variable {
                if websocket {
                    "wss://balancehub.invalid"
                } else {
                    "https://balancehub.invalid"
                }
            } else {
                "balancehub-variable"
            };
            first = false;
            replacement
        })
        .into_owned()
}

fn convert_value(source: &str, target: &str, field: &str, value: &str) -> Result<String, String> {
    // A newly authored definition has no source dialect; its raw strings use
    // the selected target's syntax. Imported definitions retain their dialect.
    if source.is_empty() {
        return Ok(value.to_owned());
    }
    if field == "headers"
        && value.contains("{{session_id}}")
        && (source == "grok" || target == "grok")
    {
        return Err("headers 使用 Grok 动态会话 ID，其他 Agent 没有等价的会话变量".to_owned());
    }
    match (template(source, field, value), template(target, field, value)) {
        (None, None) => Ok(value.to_owned()),
        (Some(normalized), _) if expands(target, field) => {
            if target == "gemini" && normalized.contains(":-") {
                return Err(format!("{field} 使用带默认值的环境变量，Gemini 的该字段没有等价展开语法"));
            }
            Ok(normalized)
        }
        _ => Err(format!("{field} 的环境变量展开规则在 {source} 与 {target} 中不同，无法原样转换；请使用目标支持的变量引用")),
    }
}

pub(super) fn convert(
    target: &NativeCatalogAdapter,
    value: &McpDefinition,
) -> Result<(McpDefinition, Map<String, Value>), String> {
    let source = value.native_adapter.as_deref().unwrap_or("");
    let options = value.connection_options();
    if source == target.key {
        return Ok((value.clone(), options.into_iter().collect()));
    }
    let mut converted = value.clone();
    let mut common = mcp_options::normalize(source, &options);
    if let Some(helper) = common.get("headersHelper").and_then(Value::as_str) {
        if !source.is_empty()
            && ["$CLAUDE_", "${CLAUDE_", "$CODEX_", "${CODEX_"]
                .iter()
                .any(|prefix| helper.contains(prefix))
        {
            return Err(
                "动态请求头命令引用了来源 Agent 的专用变量，请先改为目标 Agent 可用的命令"
                    .to_owned(),
            );
        }
    }
    for (field, destination) in [
        ("command", &mut converted.command),
        ("url", &mut converted.url),
        ("cwd", &mut converted.cwd),
    ] {
        if let Some(value) = destination {
            *value = convert_value(source, target.key, field, value)?;
        }
    }
    for arg in &mut converted.args {
        *arg = convert_value(source, target.key, "args", arg)?;
    }
    converted.environment.clear();
    let mut inherited = common
        .remove("environmentVariables")
        .map(|value| value.as_array().cloned().ok_or("env_vars 必须是数组"))
        .transpose()?
        .unwrap_or_default();
    for (name, content) in &value.environment {
        if target.key == "codex"
            && template(source, "env", content)
                .as_deref()
                .and_then(single_variable)
                == Some(name.as_str())
        {
            inherited.push(Value::String(name.clone()));
        } else {
            converted.environment.insert(
                name.clone(),
                convert_value(source, target.key, "env", content)?,
            );
        }
    }
    if !inherited.is_empty() {
        common.insert("environmentVariables".to_owned(), Value::Array(inherited));
    }
    let mut header_environment = common
        .remove("headerEnvironment")
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or("env_http_headers 必须是对象")
        })
        .transpose()?
        .unwrap_or_default();
    let mut bearer = common.remove("bearerTokenEnvVar");
    converted.headers.clear();
    for (name, content) in &value.headers {
        let reference = template(source, "headers", content);
        if target.key == "codex" {
            if let Some(reference) = reference.as_deref() {
                if let Some(variable) = single_variable(reference) {
                    header_environment.insert(name.clone(), Value::String(variable.to_owned()));
                    continue;
                }
                if name.eq_ignore_ascii_case("authorization") {
                    if let Some(variable) =
                        reference.strip_prefix("Bearer ").and_then(single_variable)
                    {
                        if bearer.is_some() {
                            return Err("Authorization 包含多个环境变量凭据来源".to_owned());
                        }
                        bearer = Some(Value::String(variable.to_owned()));
                        continue;
                    }
                }
            }
        }
        converted.headers.insert(
            name.clone(),
            convert_value(source, target.key, "headers", content)?,
        );
    }
    if target.key == "codex" {
        if !header_environment.is_empty() {
            common.insert(
                "headerEnvironment".to_owned(),
                Value::Object(header_environment),
            );
        }
    } else {
        for (name, variable) in header_environment {
            let variable = variable.as_str().ok_or("请求头环境变量名必须是字符串")?;
            insert_header(&mut converted.headers, &name, format!("${{{variable}}}"))?;
        }
    }
    if let Some(bearer) = bearer {
        if matches!(target.key, "codex" | "grok") {
            common.insert("bearerTokenEnvVar".to_owned(), bearer);
        } else {
            let variable = bearer.as_str().ok_or("Bearer 环境变量名必须是字符串")?;
            insert_header(
                &mut converted.headers,
                "Authorization",
                format!("Bearer ${{{variable}}}"),
            )?;
        }
    }
    if target.key != "codex" {
        if let Some(variables) = common.remove("environmentVariables") {
            for variable in variables.as_array().ok_or("env_vars 必须是数组")? {
                let name = variable
                    .as_str()
                    .or_else(|| {
                        (variable
                            .get("source")
                            .and_then(Value::as_str)
                            .unwrap_or("local")
                            == "local")
                            .then(|| variable.get("name").and_then(Value::as_str))
                            .flatten()
                    })
                    .ok_or("env_vars 引用了远程执行环境，目标没有等价的远程变量来源")?;
                converted
                    .environment
                    .entry(name.to_owned())
                    .or_insert_with(|| format!("${{{name}}}"));
            }
        }
    }
    Ok((converted, mcp_options::convert(target, common)?))
}

fn single_variable(value: &str) -> Option<&str> {
    let name = value.strip_prefix("${")?.strip_suffix('}')?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_'))
    .then_some(name)
}

fn insert_header(
    headers: &mut BTreeMap<String, String>,
    name: &str,
    content: String,
) -> Result<(), String> {
    if headers
        .iter()
        .any(|(key, value)| key.eq_ignore_ascii_case(name) && *value != content)
    {
        return Err(format!(
            "请求头 {name} 同时声明了静态值与动态凭据，无法无损合并"
        ));
    }
    headers.insert(name.to_owned(), content);
    Ok(())
}

pub(super) fn comparison_fields(
    value: &McpDefinition,
    common: &mut BTreeMap<String, Value>,
) -> (Map<String, Value>, Map<String, Value>) {
    let source = value.native_adapter.as_deref().unwrap_or("");
    let mut headers: Map<String, Value> = value
        .headers
        .iter()
        .map(|(key, value)| {
            (
                key.to_ascii_lowercase(),
                document_value(source, "headers", value),
            )
        })
        .collect();
    if let Some(Value::Object(variables)) = common.remove("headerEnvironment") {
        for (key, value) in variables {
            if let Some(variable) = value.as_str() {
                insert_comparison_header(
                    &mut headers,
                    &key,
                    serde_json::json!({"template": format!("${{{variable}}}")}),
                );
            }
        }
    }
    if let Some(Value::String(variable)) = common.remove("bearerTokenEnvVar") {
        insert_comparison_header(
            &mut headers,
            "authorization",
            serde_json::json!({"template": format!("Bearer ${{{variable}}}")}),
        );
    }
    let mut environment: Map<String, Value> = value
        .environment
        .iter()
        .map(|(key, value)| (key.clone(), document_value(source, "env", value)))
        .collect();
    if let Some(Value::Array(variables)) = common.get("environmentVariables") {
        let names = variables
            .iter()
            .map(|value| {
                value.as_str().or_else(|| {
                    (value
                        .get("source")
                        .and_then(Value::as_str)
                        .unwrap_or("local")
                        == "local")
                        .then(|| value.get("name").and_then(Value::as_str))
                        .flatten()
                })
            })
            .collect::<Option<Vec<_>>>();
        if let Some(names) = names {
            for name in names {
                environment
                    .entry(name.to_owned())
                    .or_insert_with(|| serde_json::json!({"template": format!("${{{name}}}")}));
            }
            common.remove("environmentVariables");
        }
    }
    (headers, environment)
}

fn insert_comparison_header(headers: &mut Map<String, Value>, name: &str, dynamic: Value) {
    let key = name.to_ascii_lowercase();
    let value = match headers.remove(&key) {
        Some(previous) if previous != dynamic => {
            serde_json::json!({"configured": previous, "dynamic": dynamic})
        }
        _ => dynamic,
    };
    headers.insert(key, value);
}
