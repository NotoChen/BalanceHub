//! Normalize connection semantics across native MCP config formats. Local
//! enablement, timeout, tool and trust policies stay in each Agent's file.
use super::{mcp_options, mcp_variables, NativeCatalogAdapter};
use crate::{
    models::*,
    services::agent_cli::catalog::definition::{
        inferred_mcp_transport, validate_mcp, McpDefinition,
    },
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

impl NativeCatalogAdapter {
    pub(in crate::services::agent_cli::catalog) fn form_option_keys() -> Vec<&'static str> {
        mcp_options::form_fields()
    }

    pub(in crate::services::agent_cli::catalog) fn supports_form_field(&self, key: &str) -> bool {
        match key {
            "command" | "args" | "env" | "url" | "headers" => true,
            "cwd" => self.supports_cwd,
            _ => mcp_options::supports_form_field(self, key),
        }
    }
    pub(crate) fn decode(&self, value: &Value) -> Result<McpDefinition, String> {
        let mut object = value.as_object().cloned().ok_or("MCP 条目不是对象")?;
        let command = optional_string(object.remove("command"))?;
        let mut url = optional_string(object.remove(self.http_url))?;
        let explicit_type = optional_string(object.remove("type"))?;
        let explicit_transport = optional_string(object.remove("transport"))?;
        let parse_transport = |value: &str| match value {
            "stdio" => Ok(AgentMcpTransport::Stdio),
            "http" | "streamable-http" => Ok(AgentMcpTransport::Http),
            "sse" => Ok(AgentMcpTransport::Sse),
            "ws" | "websocket" => Ok(AgentMcpTransport::WebSocket),
            _ => Err("该 MCP 自定义传输尚无配置映射，请保留原文并使用其原生客户端".to_owned()),
        };
        let explicit_type = explicit_type.as_deref().map(parse_transport).transpose()?;
        let explicit_transport = explicit_transport
            .as_deref()
            .map(parse_transport)
            .transpose()?;
        if explicit_type
            .zip(explicit_transport)
            .is_some_and(|(a, b)| a != b)
        {
            return Err("MCP 的 type 与 transport 声明冲突".to_owned());
        }
        let mut transport = inferred_mcp_transport(command.as_deref(), self.implicit_url_transport);
        if self.key == "grok" && url.is_none() {
            for field in ["urlTemplate", "url_template"] {
                if let Some(alias) = optional_string(object.remove(field))? {
                    if url.is_some() {
                        return Err("MCP 同时声明多个传输地址".to_owned());
                    }
                    url = Some(alias);
                }
            }
        }
        if self.http_url != "url" {
            let sse = optional_string(object.remove("url"))?;
            if url.is_some() && sse.is_some() {
                return Err("MCP 同时声明多个传输地址".to_owned());
            }
            if sse.is_some() {
                transport = AgentMcpTransport::Sse;
                url = sse;
            }
        }
        if let Some(explicit) = explicit_type.or(explicit_transport) {
            transport = explicit;
        } else if self.key == "grok"
            && command.is_none()
            && url.as_deref().is_some_and(|url| url.ends_with("/sse"))
        {
            transport = AgentMcpTransport::Sse;
        }
        if transport == AgentMcpTransport::Unknown && url.is_some() {
            return Err("远程 MCP 缺少 type；请按服务端说明填写 http、sse 或 ws".to_owned());
        }
        let args = object
            .remove("args")
            .filter(|value| !value.is_null())
            .map(|value| {
                serde_json::from_value::<Vec<String>>(value)
                    .map_err(|_| "MCP args 必须是字符串数组".to_owned())
            })
            .transpose()?
            .unwrap_or_default();
        let environment = string_map(object.remove("env"))?;
        let headers = string_map(object.remove(self.headers))?;
        let cwd = optional_string(object.remove("cwd"))?;
        let native_options = self.connection_options(&object.into_iter().collect());
        let definition = McpDefinition {
            transport,
            command,
            args,
            url,
            cwd,
            environment,
            headers,
            // The dialect also determines variable expansion, even without
            // extra options. Never discard it just because options are empty.
            native_adapter: Some(self.key.to_owned()),
            native_options,
        };
        validate_mcp(&definition)?;
        self.validate_transport(definition.transport)?;
        mcp_options::validate(self, &definition.native_options)?;
        Ok(definition)
    }

    pub(super) fn connection_options(
        &self,
        options: &BTreeMap<String, Value>,
    ) -> BTreeMap<String, Value> {
        options
            .iter()
            .filter(|(key, value)| {
                self.connection_fields.contains(&key.as_str())
                    && !value.is_null()
                    && !matches!(
                        (key.as_str(), value.as_str()),
                        ("environment_id" | "experimental_environment", Some("local"))
                            | ("auth", Some("oauth"))
                            | ("authProviderType", Some("dynamic_discovery"))
                    )
                    && !(matches!(
                        key.as_str(),
                        "env_vars" | "env_http_headers" | "oauth_scopes" | "scopes"
                    ) && (value.as_array().is_some_and(Vec::is_empty)
                        || value.as_object().is_some_and(Map::is_empty)))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub(crate) fn encode(&self, value: &McpDefinition) -> Result<Value, String> {
        validate_mcp(value)?;
        self.validate_transport(value.transport)?;
        let expected = value
            .native_adapter
            .as_ref()
            .map(|_| value.connection_document());
        if value.cwd.is_some() && !self.supports_cwd {
            return Err("此 Agent 不能保留 MCP cwd，已拒绝有损转换".to_owned());
        }
        let (value, mut result) = mcp_variables::convert(self, value)?;
        if self.explicit_type || (self.key == "gemini" && value.transport == AgentMcpTransport::Sse)
        {
            result.insert(
                "type".to_owned(),
                Value::String(
                    match value.transport {
                        AgentMcpTransport::Stdio => "stdio",
                        AgentMcpTransport::Sse => "sse",
                        AgentMcpTransport::WebSocket => "ws",
                        _ => "http",
                    }
                    .to_owned(),
                ),
            );
        }
        if let Some(command) = &value.command {
            result.insert("command".to_owned(), command.clone().into());
            result.insert(
                "args".to_owned(),
                serde_json::to_value(&value.args).map_err(|_| "MCP 参数无效")?,
            );
        }
        if let Some(url) = &value.url {
            result.insert(
                if value.transport == AgentMcpTransport::Sse {
                    "url"
                } else {
                    self.http_url
                }
                .to_owned(),
                url.clone().into(),
            );
        }
        if let Some(cwd) = &value.cwd {
            result.insert("cwd".to_owned(), cwd.clone().into());
        }
        if !value.environment.is_empty() {
            result.insert(
                "env".to_owned(),
                serde_json::to_value(&value.environment).map_err(|_| "MCP 环境变量无效")?,
            );
        }
        if !value.headers.is_empty() {
            result.insert(
                self.headers.to_owned(),
                serde_json::to_value(&value.headers).map_err(|_| "MCP 请求头无效")?,
            );
        }
        let encoded = Value::Object(result);
        let decoded = self.decode(&encoded)?;
        if let Some(expected) = expected {
            let actual = decoded.connection_document();
            if actual != expected {
                let keys = expected
                    .as_object()
                    .into_iter()
                    .flat_map(|value| value.keys())
                    .chain(
                        actual
                            .as_object()
                            .into_iter()
                            .flat_map(|value| value.keys()),
                    )
                    .filter(|key| expected.get(*key) != actual.get(*key))
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                return Err(format!(
                    "MCP 转换会改变 {}，尚不能等价写入 {}；请调整这些连接字段。依据：{}",
                    keys.into_iter().collect::<Vec<_>>().join("、"),
                    self.key,
                    self.documentation
                ));
            }
        }
        Ok(encoded)
    }

    fn validate_transport(&self, transport: AgentMcpTransport) -> Result<(), String> {
        if transport == AgentMcpTransport::Sse && !self.supports_sse {
            return Err(format!("{} 官方配置仅列出 stdio 和 Streamable HTTP，无法直接写入旧版 SSE；需要服务端提供 HTTP 地址。依据：{}", self.key, self.documentation));
        }
        if transport == AgentMcpTransport::WebSocket && !self.supports_websocket {
            return Err(format!("{} 尚无官方 WebSocket 配置映射；可使用支持 ws 的 Claude，或填写服务端提供的 HTTP 地址。依据：{}", self.key, self.documentation));
        }
        Ok(())
    }
}

fn optional_string(value: Option<Value>) -> Result<Option<String>, String> {
    value
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "MCP 字段类型无效".to_owned())
        })
        .transpose()
}
fn string_map(value: Option<Value>) -> Result<BTreeMap<String, String>, String> {
    value
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value(value).map_err(|_| "MCP 敏感字段必须是字符串映射".to_owned())
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

impl McpDefinition {
    pub(crate) fn connection_options(&self) -> BTreeMap<String, Value> {
        if self.native_adapter.is_none() {
            return self.native_options.clone();
        }
        self.native_adapter
            .as_deref()
            .and_then(|key| {
                crate::services::agent_cli::definitions()
                    .iter()
                    .filter_map(|definition| definition.environment.catalog_adapter())
                    .find(|adapter| adapter.key == key)
            })
            .map(|adapter| adapter.connection_options(&self.native_options))
            .unwrap_or_default()
    }

    pub(crate) fn shared_connection_options(&self) -> BTreeMap<String, Value> {
        mcp_options::normalize(
            self.native_adapter.as_deref().unwrap_or(""),
            &self.connection_options(),
        )
    }

    pub(crate) fn set_shared_connection_options(
        &mut self,
        common: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        mcp_options::validate_shared(common)?;
        self.native_options = if let Some(key) = self.native_adapter.as_deref() {
            let adapter = crate::services::agent_cli::definitions()
                .iter()
                .filter_map(|definition| definition.environment.catalog_adapter())
                .find(|adapter| adapter.key == key)
                .ok_or("找不到此 MCP 来源的配置格式")?;
            let portable = McpDefinition {
                native_adapter: None,
                native_options: common.clone(),
                ..self.clone()
            };
            let (converted, options) = mcp_variables::convert(adapter, &portable)?;
            let options = options.into_iter().collect();
            mcp_options::validate(adapter, &options)?;
            self.headers = converted.headers;
            self.environment = converted.environment;
            options
        } else {
            common.clone()
        };
        Ok(())
    }

    /// The same representation drives identity, comparisons and the detail view.
    /// Only real connection dependencies carry an Agent namespace.
    pub(crate) fn connection_document(&self) -> Value {
        let mut value = Map::new();
        let source = self.native_adapter.as_deref().unwrap_or("");
        value.insert(
            "type".to_owned(),
            serde_json::to_value(self.transport).unwrap_or(Value::Null),
        );
        if let Some(command) = &self.command {
            value.insert(
                "command".to_owned(),
                mcp_variables::document_value(source, "command", command),
            );
        }
        if !self.args.is_empty() {
            value.insert(
                "args".to_owned(),
                Value::Array(
                    self.args
                        .iter()
                        .map(|arg| mcp_variables::document_value(source, "args", arg))
                        .collect(),
                ),
            );
        }
        if let Some(url) = &self.url {
            let rendered = mcp_variables::document_value(source, "url", url);
            let rendered = if rendered.is_string() {
                reqwest::Url::parse(url)
                    .map(|url| Value::String(url.to_string()))
                    .unwrap_or(rendered)
            } else {
                rendered
            };
            value.insert("url".to_owned(), rendered);
        }
        if let Some(cwd) = &self.cwd {
            value.insert("cwd".to_owned(), cwd.clone().into());
        }
        let mut dependencies = self.shared_connection_options();
        let (headers, environment) = mcp_variables::comparison_fields(self, &mut dependencies);
        if !environment.is_empty() {
            value.insert("env".to_owned(), Value::Object(environment));
        }
        if !headers.is_empty() {
            value.insert("headers".to_owned(), Value::Object(headers));
        }
        if !dependencies.is_empty() {
            value.insert(
                "connectionOptions".to_owned(),
                serde_json::json!(dependencies),
            );
        }
        Value::Object(value)
    }
}
