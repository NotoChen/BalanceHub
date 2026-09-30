//! Documented connection option names. Local policy never enters this mapping.
use super::NativeCatalogAdapter;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

struct Field {
    agent: &'static str,
    native: &'static str,
    common: &'static str,
}

const FIELDS: &[Field] = &[
    Field {
        agent: "claude",
        native: "oauth.clientId",
        common: "oauth.clientId",
    },
    Field {
        agent: "claude",
        native: "oauth.callbackPort",
        common: "oauth.callbackPort",
    },
    Field {
        agent: "claude",
        native: "oauth.scopes",
        common: "oauth.scopes",
    },
    Field {
        agent: "claude",
        native: "headersHelper",
        common: "headersHelper",
    },
    Field {
        agent: "codex",
        native: "oauth.client_id",
        common: "oauth.clientId",
    },
    Field {
        agent: "codex",
        native: "oauth.callback_port",
        common: "oauth.callbackPort",
    },
    Field {
        agent: "codex",
        native: "oauth.callback_url",
        common: "oauth.callbackUrl",
    },
    Field {
        agent: "codex",
        native: "scopes",
        common: "oauth.scopes",
    },
    Field {
        agent: "codex",
        native: "http_headers_helper",
        common: "headersHelper",
    },
    Field {
        agent: "codex",
        native: "bearer_token_env_var",
        common: "bearerTokenEnvVar",
    },
    Field {
        agent: "codex",
        native: "env_http_headers",
        common: "headerEnvironment",
    },
    Field {
        agent: "codex",
        native: "env_vars",
        common: "environmentVariables",
    },
    Field {
        agent: "gemini",
        native: "oauth.clientId",
        common: "oauth.clientId",
    },
    Field {
        agent: "gemini",
        native: "oauth.redirectUri",
        common: "oauth.callbackUrl",
    },
    Field {
        agent: "gemini",
        native: "oauth.scopes",
        common: "oauth.scopes",
    },
    Field {
        agent: "grok",
        native: "oauth_client_id",
        common: "oauth.clientId",
    },
    Field {
        agent: "grok",
        native: "oauth_scopes",
        common: "oauth.scopes",
    },
    Field {
        agent: "grok",
        native: "bearer_token_env_var",
        common: "bearerTokenEnvVar",
    },
];

pub(super) fn form_fields() -> Vec<&'static str> {
    FIELDS
        .iter()
        .map(|field| field.common)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn supports_form_field(adapter: &NativeCatalogAdapter, key: &str) -> bool {
    FIELDS
        .iter()
        .any(|field| field.agent == adapter.key && field.common == key)
        || super::mcp_variables::supports_reference_option(adapter, key)
}

pub(super) fn validate(
    adapter: &NativeCatalogAdapter,
    options: &BTreeMap<String, Value>,
) -> Result<(), String> {
    for (key, value) in options {
        let valid = match key.as_str() {
            "oauth" => value.as_object().is_some_and(|object| {
                object.iter().all(|(key, value)| {
                    value.is_null()
                        || match key.as_str() {
                            "enabled" => value.is_boolean(),
                            "callbackPort" | "callback_port" => {
                                value.as_u64().is_some_and(|value| value <= u16::MAX as u64)
                            }
                            "scopes" if adapter.key == "claude" => value.is_string(),
                            "scopes" | "audiences" => strings(value),
                            "clientId"
                            | "client_id"
                            | "clientSecret"
                            | "callback_url"
                            | "redirectUri"
                            | "authServerMetadataUrl"
                            | "authorizationUrl"
                            | "tokenUrl"
                            | "issuer"
                            | "tokenParamName" => value.is_string(),
                            _ => true,
                        }
                })
            }),
            "env_http_headers" => value
                .as_object()
                .is_some_and(|object| object.values().all(Value::is_string)),
            "env_vars" => value.as_array().is_some_and(|array| {
                array.iter().all(|value| {
                    value.is_string()
                        || (value.get("name").is_some_and(Value::is_string)
                            && value.get("source").is_none_or(|value| {
                                value
                                    .as_str()
                                    .is_some_and(|value| matches!(value, "local" | "remote"))
                            }))
                })
            }),
            "scopes" | "oauth_scopes" => strings(value),
            _ => value.is_string(),
        };
        if !valid {
            return Err(format!(
                "MCP 连接字段 {key} 的类型无效；依据：{}",
                adapter.documentation
            ));
        }
    }
    Ok(())
}

fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

pub(super) fn validate_shared(options: &BTreeMap<String, Value>) -> Result<(), String> {
    for (key, value) in options {
        let field = FIELDS.iter().find(|field| field.common == key);
        let agent = field
            .map(|field| field.agent)
            .or_else(|| key.split_once('.').map(|(agent, _)| agent));
        let adapter = crate::services::agent_cli::definitions()
            .iter()
            .filter_map(|definition| definition.environment.catalog_adapter())
            .find(|adapter| Some(adapter.key) == agent)
            .ok_or_else(|| format!("无法识别 connectionOptions 中的连接选项 {key}"))?;
        let native = field
            .map(|field| field.native)
            .or_else(|| key.split_once('.').map(|(_, native)| native))
            .ok_or_else(|| format!("无法识别连接选项 {key}"))?;
        if !adapter
            .connection_fields
            .contains(&native.split('.').next().unwrap_or(native))
        {
            return Err(format!(
                "{key} 不是共享连接字段，请在 Agent 原生配置中编辑本地策略"
            ));
        }
        let converted = convert(adapter, BTreeMap::from([(key.clone(), value.clone())]))?;
        validate(adapter, &converted.into_iter().collect())?;
    }
    Ok(())
}

pub(super) fn normalize(agent: &str, options: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    for (key, value) in options {
        if key == "oauth" {
            if let Some(object) = value.as_object() {
                for (key, value) in object {
                    insert_normalized(&mut result, agent, &format!("oauth.{key}"), value);
                }
                continue;
            }
        }
        insert_normalized(&mut result, agent, key, value);
    }
    result
}

fn insert_normalized(result: &mut BTreeMap<String, Value>, agent: &str, key: &str, value: &Value) {
    if value.is_null()
        || value.as_object().is_some_and(Map::is_empty)
        || value.as_array().is_some_and(Vec::is_empty)
        || (agent == "gemini" && key == "oauth.enabled" && value == &Value::Bool(true))
    {
        return;
    }
    let field = FIELDS
        .iter()
        .find(|field| field.agent == agent && field.native == key);
    let common = if agent.is_empty() {
        key.to_owned()
    } else {
        field.map_or_else(|| format!("{agent}.{key}"), |field| field.common.to_owned())
    };
    let value = if common == "oauth.scopes" {
        let scopes = value
            .as_str()
            .map(|value| {
                value
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .or_else(|| {
                value
                    .as_array()?
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect()
            });
        scopes
            .map(|mut scopes| {
                scopes.sort();
                scopes.dedup();
                serde_json::json!(scopes)
            })
            .unwrap_or_else(|| value.clone())
    } else {
        value.clone()
    };
    result.insert(common, value);
}

pub(super) fn convert(
    target: &NativeCatalogAdapter,
    common: BTreeMap<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut result = Map::new();
    let mut unsupported = Vec::new();
    for (key, value) in common {
        if let Some(field) = FIELDS
            .iter()
            .find(|field| field.agent == target.key && field.common == key)
        {
            let value = if target.key == "claude" && key == "oauth.scopes" {
                let scopes = value.as_array().ok_or("OAuth scopes 必须是字符串列表")?;
                Value::String(
                    scopes
                        .iter()
                        .map(|item| item.as_str().ok_or("OAuth scope 必须是字符串"))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(" "),
                )
            } else {
                value
            };
            insert_path(&mut result, field.native, value)?;
        } else if let Some(native) = key.strip_prefix(&format!("{}.", target.key)) {
            insert_path(&mut result, native, value)?;
        } else {
            unsupported.push(key);
        }
    }
    if !unsupported.is_empty() {
        return Err(format!(
            "连接依赖 {}，尚无到 {} 的等价配置映射；请在预览前调整该连接配置。依据：{}",
            unsupported.join("、"),
            target.key,
            target.documentation
        ));
    }
    if target.key == "gemini" && result.contains_key("oauth") {
        result
            .get_mut("oauth")
            .and_then(Value::as_object_mut)
            .ok_or("OAuth 配置必须是对象")?
            .entry("enabled")
            .or_insert(Value::Bool(true));
    }
    Ok(result)
}

fn insert_path(result: &mut Map<String, Value>, path: &str, value: Value) -> Result<(), String> {
    if let Some((parent, child)) = path.split_once('.') {
        result
            .entry(parent.to_owned())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or("MCP 连接选项的层级冲突")?
            .insert(child.to_owned(), value);
    } else {
        result.insert(path.to_owned(), value);
    }
    Ok(())
}
