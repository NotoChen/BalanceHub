use super::{connection::Input, CheckError, Connection, Result, MESSAGE_LIMIT};
use reqwest::{header::HeaderMap, Client, Response, Url};
use serde_json::Value;
use std::time::Duration;

struct Events {
    response: Response,
    buffer: Vec<u8>,
    offset: usize,
    data: String,
    kind: String,
}
impl Events {
    fn new(response: Response) -> Self {
        Self {
            response,
            buffer: Vec::new(),
            offset: 0,
            data: String::new(),
            kind: String::new(),
        }
    }
    async fn next(&mut self) -> Result<(String, String)> {
        loop {
            if let Some(end) = self.buffer[self.offset..]
                .iter()
                .position(|byte| *byte == b'\n')
            {
                let end = self.offset + end + 1;
                let line = std::str::from_utf8(&self.buffer[self.offset..end])
                    .map_err(|_| CheckError::failed("SSE 响应不是 UTF-8"))?
                    .trim_end_matches(['\r', '\n']);
                self.offset = end;
                if line.is_empty() {
                    if !self.data.is_empty() {
                        let data = std::mem::take(&mut self.data);
                        let kind = std::mem::take(&mut self.kind);
                        return Ok((kind, data.trim_end_matches('\n').to_owned()));
                    }
                    self.kind.clear();
                } else if let Some(value) = line.strip_prefix("data:") {
                    self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    self.data.push('\n');
                    if self.data.len() > MESSAGE_LIMIT {
                        return Err(CheckError::failed("SSE 消息超过 2 MiB"));
                    }
                } else if let Some(value) = line.strip_prefix("event:") {
                    self.kind = value.trim().to_owned();
                }
                continue;
            }
            if self.offset > 0 {
                self.buffer.drain(..self.offset);
                self.offset = 0;
            }
            let chunk = self
                .response
                .chunk()
                .await
                .map_err(network_error)?
                .ok_or_else(|| CheckError::failed("服务在协议响应完成前关闭了 SSE 连接"))?;
            if self.buffer.len().saturating_add(chunk.len()) > MESSAGE_LIMIT {
                return Err(CheckError::failed("SSE 单行超过 2 MiB"));
            }
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

pub(super) struct HttpConnection {
    client: Client,
    url: Url,
    headers: HeaderMap,
    session: Option<String>,
    protocol: Option<String>,
    stream: Option<Events>,
    legacy_sse: bool,
}
impl HttpConnection {
    pub async fn open(input: &Input, client: Client) -> Result<Self> {
        let url = input
            .url
            .clone()
            .ok_or_else(|| CheckError::failed("MCP 地址缺失"))?;
        let mut connection = Self {
            client,
            url: url.clone(),
            headers: input.headers.clone(),
            session: None,
            protocol: None,
            stream: None,
            legacy_sse: input.transport == "sse",
        };
        if input.transport == "sse" {
            let response = check_status(
                connection
                    .client
                    .get(url.clone())
                    .headers(connection.headers.clone())
                    .header("Accept", "text/event-stream")
                    .send()
                    .await
                    .map_err(network_error)?,
            )?;
            let mut events = Events::new(response);
            loop {
                let (kind, data) = events.next().await?;
                if kind != "endpoint" {
                    continue;
                }
                let endpoint = url
                    .join(&data)
                    .map_err(|_| CheckError::failed("SSE 服务消息端点无效"))?;
                if endpoint.origin() != url.origin()
                    || !endpoint.username().is_empty()
                    || endpoint.password().is_some()
                {
                    return Err(CheckError::failed(
                        "SSE 消息端点跨域，未向其它站点转发认证信息",
                    ));
                }
                connection.url = endpoint;
                connection.stream = Some(events);
                break;
            }
        }
        Ok(connection)
    }

    async fn post(&self, value: &Value) -> Result<Response> {
        let mut request = self
            .client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .header("Accept", "application/json, text/event-stream")
            .json(value);
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(protocol) = &self.protocol {
            request = request.header("MCP-Protocol-Version", protocol);
        }
        check_status(request.send().await.map_err(network_error)?)
    }

    async fn reply(&self, events: &mut Events, id: &Value) -> Result<Value> {
        loop {
            let (_, data) = events.next().await?;
            let value: Value = serde_json::from_str(&data)
                .map_err(|_| CheckError::failed("SSE 数据不是 JSON-RPC"))?;
            if value.get("id") == Some(id) && value.get("method").is_none() {
                return Ok(value);
            }
            if let Some(response) = super::connection::server_response(&value) {
                self.post(&response).await?;
            }
        }
    }
}
#[async_trait::async_trait]
impl Connection for HttpConnection {
    async fn send(&mut self, value: Value) -> Result<Option<Value>> {
        let response = self.post(&value).await?;
        if let Some(session) = response.headers().get("mcp-session-id") {
            let session = session
                .to_str()
                .map_err(|_| CheckError::failed("MCP 会话标识无效"))?;
            if session.len() > 4096 {
                return Err(CheckError::failed("MCP 会话标识过长"));
            }
            self.session = Some(session.to_owned());
        }
        let Some(id) = value.get("id") else {
            return Ok(None);
        };
        if let Some(mut stream) = self.stream.take() {
            let result = self.reply(&mut stream, id).await;
            self.stream = Some(stream);
            return result.map(Some);
        }
        if response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|kind| kind.starts_with("text/event-stream"))
        {
            return self.reply(&mut Events::new(response), id).await.map(Some);
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len().saturating_add(chunk.len()) > MESSAGE_LIMIT {
                return Err(CheckError::failed("协议响应超过 2 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let message: Value = serde_json::from_slice(&bytes).map_err(|_| {
            CheckError::failed("接口返回的不是 JSON-RPC，请检查是否填写了正确的 MCP 端点")
        })?;
        if message.get("id") != Some(id) {
            return Err(CheckError::failed("服务响应的请求编号不匹配"));
        }
        Ok(Some(message))
    }
    fn negotiated(&mut self, version: &str) {
        self.protocol = Some(version.to_owned());
    }
    async fn close(&mut self) {
        if self.legacy_sse {
            return;
        }
        if let Some(session) = self.session.take() {
            let mut request = self
                .client
                .delete(self.url.clone())
                .headers(self.headers.clone())
                .header("Mcp-Session-Id", session);
            if let Some(protocol) = &self.protocol {
                request = request.header("MCP-Protocol-Version", protocol);
            }
            let _ = tokio::time::timeout(Duration::from_secs(2), request.send()).await;
        }
    }
}

impl Drop for HttpConnection {
    fn drop(&mut self) {
        if self.legacy_sse {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        let mut request = self
            .client
            .delete(self.url.clone())
            .headers(self.headers.clone())
            .header("Mcp-Session-Id", session);
        if let Some(protocol) = &self.protocol {
            request = request.header("MCP-Protocol-Version", protocol);
        }
        // Cancellation also tears down the negotiated remote session, bounded
        // independently so closing the UI never waits for server cleanup.
        tauri::async_runtime::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(2), request.send()).await;
        });
    }
}

fn network_error(error: reqwest::Error) -> CheckError {
    if error.is_timeout() {
        CheckError::timeout()
    } else if error.is_connect() {
        CheckError::failed("无法连接服务，请检查地址、网络、代理和证书")
    } else {
        CheckError::failed("连接在传输过程中中断")
    }
}
fn check_status(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if matches!(status.as_u16(), 401 | 403) {
        return Err(CheckError { outcome: "authenticationRequired", message: format!("服务要求认证（HTTP {}）。请核对认证请求头；Agent 的 OAuth 登录状态不能直接由应用代用。", status.as_u16()) });
    }
    Err(CheckError::failed(format!(
        "服务返回 HTTP {}{}",
        status.as_u16(),
        if status.is_redirection() {
            "，请使用最终 MCP 地址"
        } else {
            ""
        }
    )))
}
