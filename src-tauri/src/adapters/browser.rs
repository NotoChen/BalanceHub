use super::transport::TransportResponse;
use crate::{
    models::{CheckInError, CheckInPhase},
    platform::process::{configure_process_group, kill_process_tree},
};
use reqwest::{header::HeaderMap, StatusCode, Url};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

const MAX_REPLY_BYTES: usize = 7 * 1024 * 1024;
pub(crate) type ProgressSink = Arc<dyn Fn(CheckInPhase) + Send + Sync>;

/// Private JSONL pipe to the optional browser executor. Credentials never enter
/// command arguments, app events, or process logs.
pub(crate) struct BrowserSession {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    browser_pid: Option<u32>,
    sequence: u64,
    progress: ProgressSink,
}

impl BrowserSession {
    pub(crate) fn spawn(runtime: &Path, progress: ProgressSink) -> Result<Self, String> {
        let node = runtime.join(if cfg!(windows) { "node.exe" } else { "node" });
        let worker = runtime.join("worker.mjs");
        if !node.is_file() || !worker.is_file() {
            return Err("签到浏览器组件缺失，请在设置中安装或修复组件".to_string());
        }
        let mut command = std::process::Command::new(node);
        command
            .arg(worker)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let mut child = Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "无法启动签到浏览器组件".to_string())?;
        let input = child.stdin.take().ok_or("无法连接浏览器输入通道")?;
        let output = child.stdout.take().ok_or("无法连接浏览器输出通道")?;
        Ok(Self {
            child,
            input: Some(input),
            output: BufReader::new(output),
            browser_pid: None,
            sequence: 0,
            progress,
        })
    }

    pub(crate) fn phase(&self, phase: CheckInPhase) {
        (self.progress)(phase);
    }

    pub(crate) async fn request(&mut self, op: &str, params: Value) -> Result<Value, CheckInError> {
        let seconds = if matches!(op, "open" | "navigate" | "verify") {
            220
        } else {
            35
        };
        tokio::time::timeout(Duration::from_secs(seconds), self.exchange(op, params))
            .await
            .map_err(|_| "浏览器操作超时，签到已停止".to_string())?
    }

    async fn exchange(&mut self, op: &str, params: Value) -> Result<Value, CheckInError> {
        self.sequence += 1;
        let id = self.sequence;
        let mut message = serde_json::to_vec(&json!({ "id": id, "op": op, "params": params }))
            .map_err(|_| "无法编码浏览器请求")?;
        message.push(b'\n');
        self.input
            .as_mut()
            .ok_or("浏览器通道已关闭")?
            .write_all(&message)
            .await
            .map_err(|_| "浏览器通道已关闭")?;
        loop {
            let value = self.read_reply().await?;
            if value.get("event").and_then(Value::as_str) == Some("browserStarted") {
                self.browser_pid = value
                    .get("browserPid")
                    .and_then(Value::as_u64)
                    .and_then(|pid| u32::try_from(pid).ok())
                    .filter(|pid| *pid > 1);
                continue;
            }
            if value.get("event").and_then(Value::as_str) == Some("progress") {
                if let Some(phase) = value
                    .get("phase")
                    .and_then(|v| serde_json::from_value::<CheckInPhase>(v.clone()).ok())
                {
                    self.phase(phase);
                }
                continue;
            }
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                return Err("浏览器响应顺序异常，签到已停止".into());
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(value.get("data").cloned().unwrap_or(Value::Null));
            }
            if value.get("code").and_then(Value::as_str) == Some("needsHuman") {
                return Err(CheckInError::WaitingHuman);
            }
            return Err(value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("浏览器操作失败")
                .to_string()
                .into());
        }
    }

    async fn read_reply(&mut self) -> Result<Value, String> {
        let mut line = Vec::new();
        loop {
            let available = self
                .output
                .fill_buf()
                .await
                .map_err(|_| "读取浏览器响应失败")?;
            if available.is_empty() {
                return Err("验证窗口或浏览器进程已关闭".to_string());
            }
            let end = available.iter().position(|byte| *byte == b'\n');
            let count = end.map_or(available.len(), |index| index + 1);
            if line.len() + count > MAX_REPLY_BYTES {
                return Err("浏览器响应过长，签到已停止".to_string());
            }
            line.extend_from_slice(&available[..count]);
            self.output.consume(count);
            if end.is_some() {
                return serde_json::from_slice(&line).map_err(|_| "浏览器响应格式无效".to_string());
            }
        }
    }

    pub(crate) async fn fetch_with_body(
        &mut self,
        path: &str,
        method: &str,
        headers: &Value,
        body: Option<String>,
    ) -> Result<TransportResponse, CheckInError> {
        let value = self
            .request(
                "fetch",
                json!({ "path": path, "method": method, "headers": headers, "body": body }),
            )
            .await?;
        let status = value
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok())
            .and_then(|status| StatusCode::from_u16(status).ok())
            .ok_or("浏览器未返回有效 HTTP 状态")?;
        let mut response_headers = HeaderMap::new();
        if let Some(headers) = value.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if let (Ok(name), Some(value)) = (
                    name.parse::<reqwest::header::HeaderName>(),
                    value
                        .as_str()
                        .and_then(|v| v.parse::<reqwest::header::HeaderValue>().ok()),
                ) {
                    response_headers.insert(name, value);
                }
            }
        }
        Ok(TransportResponse {
            status,
            headers: response_headers,
            body: value
                .get("body")
                .and_then(Value::as_str)
                .ok_or("浏览器响应缺少正文")?
                .to_string(),
            url: Url::parse(
                value
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or("浏览器响应缺少地址")?,
            )
            .map_err(|_| "浏览器响应地址无效")?,
        })
    }

    pub(crate) async fn close(&mut self) {
        // EOF is handled independently of a pending verification by the worker.
        self.input.take();
        if tokio::time::timeout(Duration::from_secs(6), self.child.wait())
            .await
            .is_err()
        {
            self.kill();
            let _ = self.child.wait().await;
        }
        self.browser_pid = None;
    }

    fn kill(&mut self) {
        if let Some(pid) = self.browser_pid.take() {
            kill_process_tree(pid);
        }
        if let Some(pid) = self.child.id() {
            kill_process_tree(pid);
            let _ = self.child.start_kill();
        }
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.kill();
    }
}
