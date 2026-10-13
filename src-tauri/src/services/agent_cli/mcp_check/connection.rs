use super::{CheckError, Result};
use crate::models::AgentMcpTransport;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(super) fn server_response(message: &Value) -> Option<Value> {
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return None;
    }
    let id = message.get("id")?;
    let method = message.get("method")?.as_str()?;
    Some(if method == "ping" {
        json!({"jsonrpc":"2.0","id":id,"result":{}})
    } else {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Diagnostic client does not implement this method"}})
    })
}

#[derive(Clone)]
pub(super) struct Input {
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<Url>,
    pub headers: HeaderMap,
}

impl Input {
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 512 * 1024 {
            return Err(CheckError::failed("连接定义超过 512 KiB"));
        }
        let value = crate::services::agent_cli::environment::config_document::parse(
            text.as_bytes(),
            crate::services::agent_cli::environment::config_document::ConfigDocumentFormat::Json,
        )
        .ok_or_else(|| CheckError::failed("连接定义不是有效 JSON"))?;
        let transport: AgentMcpTransport =
            serde_json::from_value(value.get("type").cloned().unwrap_or_else(|| {
                Value::String(
                    if value.get("command").is_some() {
                        "stdio"
                    } else {
                        "http"
                    }
                    .to_owned(),
                )
            }))
            .map_err(|_| CheckError::failed("连接方式无法识别"))?;
        let transport = match transport {
            AgentMcpTransport::Stdio => "stdio",
            AgentMcpTransport::Http => "http",
            AgentMcpTransport::Sse => "sse",
            AgentMcpTransport::WebSocket => {
                return Err(CheckError::failed(
                    "此服务使用 Agent 扩展的 WebSocket 传输，请在所属 Agent 中检查连接",
                ))
            }
            AgentMcpTransport::Unknown => return Err(CheckError::failed("连接方式无法识别")),
        }
        .to_owned();
        let command = value
            .get("command")
            .map(resolve)
            .transpose()?
            .unwrap_or_default();
        let args = match value.get("args") {
            Some(Value::Array(values)) if values.len() <= 512 => {
                values.iter().map(resolve).collect::<Result<Vec<_>>>()?
            }
            None => Vec::new(),
            _ => return Err(CheckError::failed("启动参数必须是数组，最多 512 项")),
        };
        let mut environment = mapping(value.get("env"))?;
        let cwd = value
            .get("cwd")
            .map(resolve)
            .transpose()?
            .filter(|value| !value.is_empty());
        let mut headers = HeaderMap::new();
        for (name, value) in mapping(value.get("headers"))? {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| CheckError::failed("请求头名称无效"))?,
                HeaderValue::from_str(&value)
                    .map_err(|_| CheckError::failed("请求头包含无效字符"))?,
            );
        }
        let options = value.get("connectionOptions").and_then(Value::as_object);
        if let Some(options) = options {
            if options.get("headersHelper").is_some() {
                return Err(CheckError::failed(
                    "此定义依赖 Agent 动态认证或运行环境，请先在所属 Agent 中完成认证",
                ));
            }
            if let Some(variables) = options.get("environmentVariables") {
                let names = variables
                    .as_array()
                    .ok_or_else(|| CheckError::failed("继承的环境变量必须是数组"))?;
                for name in names {
                    let name = name
                        .as_str()
                        .ok_or_else(|| CheckError::failed("环境变量名称无效"))?;
                    if !environment.contains_key(name) {
                        environment.insert(name.into(), variable_value(name, None)?);
                    }
                }
            }
            if let Some(variable) = options.get("bearerTokenEnvVar") {
                let name = variable
                    .as_str()
                    .ok_or_else(|| CheckError::failed("认证环境变量名无效"))?;
                let token = variable_value(name, None)?;
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .map_err(|_| CheckError::failed("认证环境变量包含无效字符"))?,
                );
            }
            for (name, variable) in mapping(options.get("headerEnvironment"))? {
                headers.insert(
                    HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| CheckError::failed("请求头名称无效"))?,
                    HeaderValue::from_str(&variable_value(&variable, None)?)
                        .map_err(|_| CheckError::failed("请求头环境变量无效"))?,
                );
            }
        }
        let url = value
            .get("url")
            .map(resolve)
            .transpose()?
            .map(|url| Url::parse(&url).map_err(|_| CheckError::failed("服务 URL 无效")))
            .transpose()?;
        if transport == "stdio" {
            if command.trim().is_empty() {
                return Err(CheckError::failed("请填写 MCP 启动命令"));
            }
        } else if url.as_ref().is_none_or(|url| {
            !matches!(url.scheme(), "https" | "http")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
        }) {
            return Err(CheckError::failed("远程 MCP 需要有效的 HTTP 或 HTTPS 地址"));
        }
        Ok(Self {
            transport,
            command,
            args,
            environment,
            cwd,
            url,
            headers,
        })
    }
}

fn mapping(value: Option<&Value>) -> Result<BTreeMap<String, String>> {
    match value {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(values)) if values.len() <= 256 => values
            .iter()
            .map(|(name, value)| Ok((name.clone(), resolve(value)?)))
            .collect(),
        _ => Err(CheckError::failed(
            "环境变量或请求头必须是对象，最多 256 项",
        )),
    }
}
fn variable_value(name: &str, fallback: Option<&str>) -> Result<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(CheckError::failed("环境变量名称无效"));
    }
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.map(str::to_owned))
        .ok_or_else(|| CheckError {
            outcome: "configurationRequired",
            message: format!("当前应用进程没有环境变量 {name}，请配置后重试"),
        })
}
fn resolve(value: &Value) -> Result<String> {
    if let Some(text) = value.as_str() {
        if text.contains('\0') {
            return Err(CheckError::failed("连接字段包含无效字符"));
        }
        return Ok(text.to_owned());
    }
    let template = value
        .get("template")
        .and_then(Value::as_str)
        .ok_or_else(|| CheckError::failed("连接依赖 Agent 运行时变量，当前无法独立检查"))?;
    let mut text = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("${") {
        text.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| CheckError::failed("环境变量模板不完整"))?;
        let (name, fallback) = tail[..end]
            .split_once(":-")
            .map_or((&tail[..end], None), |(name, value)| (name, Some(value)));
        text.push_str(&variable_value(name, fallback)?);
        remaining = &tail[end + 1..];
    }
    text.push_str(remaining);
    Ok(text)
}
