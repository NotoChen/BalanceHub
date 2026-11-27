use super::transport::TransportResponse;
mod control;
use crate::{
    models::{CheckInError, CheckInPhase},
    platform::{
        paths::compatible_path,
        process::{configure_process_group, kill_process_tree},
    },
};
pub use control::BrowserWindowControl;
use reqwest::{header::HeaderMap, StatusCode, Url};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr, ChildStdout, Command},
};

const MAX_REPLY_BYTES: usize = 7 * 1024 * 1024;
/// Worker 退出时只把最后几行 stderr 带进错误提示，足够解释原因又不至于
/// 把整段堆栈塞进任务中心。
const STDERR_TAIL_LINES: usize = 8;
const STDERR_LINE_LIMIT: usize = 400;
pub type ProgressSink = Arc<dyn Fn(CheckInPhase) + Send + Sync>;

/// Private JSONL pipe to the optional browser executor. Credentials never enter
/// command arguments, app events, or process logs.
pub struct BrowserSession {
    child: Child,
    control: BrowserWindowControl,
    output: BufReader<ChildStdout>,
    browser_pid: Option<u32>,
    sequence: u64,
    progress: ProgressSink,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl BrowserSession {
    pub fn spawn(runtime: &Path, progress: ProgressSink) -> Result<Self, String> {
        let node = runtime.join(if cfg!(windows) { "node.exe" } else { "node" });
        let worker = runtime.join("worker.mjs");
        if !node.is_file() || !worker.is_file() {
            return Err("浏览器组件缺失，请在设置中安装或修复组件".to_string());
        }
        let mut command = std::process::Command::new(compatible_path(&node).as_ref());
        command
            .arg(compatible_path(&worker).as_ref())
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);
        let mut child = Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("无法启动浏览器辅助组件：{error}"))?;
        let input = child.stdin.take().ok_or("无法连接浏览器输入通道")?;
        let output = child.stdout.take().ok_or("无法连接浏览器输出通道")?;
        // Keep the last stderr lines so an unexpected exit explains itself.
        // The pipe must be drained continuously or a chatty worker could block.
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_reader(stderr, Arc::clone(&stderr_tail));
        }
        Ok(Self {
            child,
            control: BrowserWindowControl::new(input),
            output: BufReader::new(output),
            browser_pid: None,
            sequence: 0,
            progress,
            stderr_tail,
        })
    }

    pub fn phase(&self, phase: CheckInPhase) {
        (self.progress)(phase);
    }

    pub fn window_control(&self) -> BrowserWindowControl {
        self.control.clone()
    }

    pub async fn request(&mut self, op: &str, mut params: Value) -> Result<Value, CheckInError> {
        // These are filesystem paths crossing into Node/Playwright. Keep URLs,
        // cookies and the remaining protocol parameters untouched.
        for name in ["profileDir", "executablePath"] {
            if let Some(Value::String(path)) = params.get_mut(name) {
                *path = compatible_path(Path::new(path))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        // 外层超时只是最后的安全网：必须晚于 worker 自己的启动、导航和验证
        // 预算（启动 30s + 页面/验证 30s + 180s），否则用户看到的是笼统的
        // “浏览器操作超时”，而不是 worker 给出的具体原因。
        let seconds = if matches!(op, "login" | "account") {
            700
        } else if matches!(op, "open") {
            260
        } else if matches!(op, "navigate" | "verify") {
            230
        } else {
            60
        };
        tokio::time::timeout(Duration::from_secs(seconds), self.exchange(op, params))
            .await
            .map_err(|_| "浏览器操作超时，任务已停止".to_string())?
    }

    async fn exchange(&mut self, op: &str, params: Value) -> Result<Value, CheckInError> {
        self.sequence += 1;
        let id = self.sequence;
        let mut message = serde_json::to_vec(&json!({ "id": id, "op": op, "params": params }))
            .map_err(|_| "无法编码浏览器请求")?;
        message.push(b'\n');
        self.control.write(&message).await?;
        loop {
            let value = self.read_reply().await?;
            if self.control.accept_reply(&value) {
                continue;
            }
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
                return Err("浏览器响应顺序异常，任务已停止".into());
            }
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(value.get("data").cloned().unwrap_or(Value::Null));
            }
            if value.get("code").and_then(Value::as_str) == Some("needsHuman") {
                return Err(CheckInError::WaitingHuman);
            }
            if value.get("code").and_then(Value::as_str) == Some("loginTimeout") {
                return Err(CheckInError::LoginTimeout(
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("登录等待超时，请重新点击登录并导入")
                        .to_string(),
                ));
            }
            if value.get("code").and_then(Value::as_str) == Some("cancelled") {
                return Err(CheckInError::Cancelled(
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("签到已取消")
                        .to_string(),
                ));
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
                return Err(self.exit_reason());
            }
            let end = available.iter().position(|byte| *byte == b'\n');
            let count = end.map_or(available.len(), |index| index + 1);
            if line.len() + count > MAX_REPLY_BYTES {
                return Err("浏览器响应过长，任务已停止".to_string());
            }
            line.extend_from_slice(&available[..count]);
            self.output.consume(count);
            if end.is_some() {
                return serde_json::from_slice(&line).map_err(|_| "浏览器响应格式无效".to_string());
            }
        }
    }

    pub async fn fetch_with_body(
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

    pub async fn close(&mut self) {
        // EOF is handled independently of a pending verification by the worker.
        self.control.close().await;
        let child_pid = self.child.id();
        if tokio::time::timeout(Duration::from_secs(6), self.child.wait())
            .await
            .is_err()
        {
            self.kill();
            let _ = self.child.wait().await;
            return;
        }
        // The worker exits on its own after closing Chromium, but its 5 s
        // shutdown watchdog and unexpected crashes exit while Playwright may
        // still own a live browser. Once node is gone nothing else will release
        // the persistent profile, so stop both recorded process trees; killing
        // an already-exited pid or group is a no-op.
        if let Some(pid) = self.browser_pid.take() {
            kill_process_tree(pid);
        }
        if let Some(pid) = child_pid {
            kill_process_tree(pid);
        }
    }

    /// 进程管道关闭时说明原因。stderr 只摘录最近几行并去掉 userinfo，
    /// 代理凭据不会随错误进入事件或日志。
    fn exit_reason(&self) -> String {
        let detail = self
            .stderr_tail
            .lock()
            .ok()
            .and_then(|tail| stderr_reason(&tail));
        match detail {
            Some(detail) => format!("验证窗口或浏览器进程已关闭：{detail}"),
            None => "验证窗口或浏览器进程已关闭".to_string(),
        }
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
        self.control.abort();
        self.kill();
    }
}

fn spawn_stderr_reader(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    crate::runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(line) = stderr_excerpt(&line) else {
                continue;
            };
            if let Ok(mut tail) = tail.lock() {
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        }
    });
}

fn stderr_reason(tail: &VecDeque<String>) -> Option<String> {
    // Node 的版本尾巴和堆栈帧比错误本身更靠近文件末尾，优先取最近的
    // 错误行，找不到时退回最后一行。
    tail.iter()
        .rev()
        .find(|line| {
            ["Error", "error", "failed", "Failed"]
                .iter()
                .any(|needle| line.contains(needle))
        })
        .or_else(|| tail.back())
        .cloned()
}

fn stderr_excerpt(line: &str) -> Option<String> {
    let line = redact_userinfo(line.trim());
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(line.chars().take(STDERR_LINE_LIMIT).collect())
}

/// 代理地址可能以 `http://user:password@host` 出现在 worker 的参数和
/// 报错里。URL 的 userinfo 部分只存在于 `://` 之后、第一个 `/` 或空白
/// 之前的 `@`，按这个范围删除，正常路径里的 `@` 保持不变。
fn redact_userinfo(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("://") {
        let after = start + 3;
        output.push_str(&rest[..after]);
        let tail = &rest[after..];
        match tail.find(|character: char| {
            character == '@'
                || character == '/'
                || character.is_whitespace()
                || character == '"'
                || character == '\''
        }) {
            Some(index) if tail[index..].starts_with('@') => {
                output.push_str("***@");
                rest = &tail[index + 1..];
            }
            _ => rest = tail,
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_exit_reports_the_error_line_instead_of_the_node_version_footer() {
        let tail = VecDeque::from([
            "node:internal/modules/cjs/loader:1147".to_string(),
            "Error: Cannot find module 'playwright-core'".to_string(),
            "    at Module._resolveFilename (node:internal/modules/cjs/loader:1147:15)".to_string(),
            "Node.js v22.14.0".to_string(),
        ]);
        assert_eq!(
            stderr_reason(&tail).unwrap(),
            "Error: Cannot find module 'playwright-core'"
        );
        let quiet = VecDeque::from(["Target page, context or browser has been closed".to_string()]);
        assert_eq!(
            stderr_reason(&quiet).unwrap(),
            "Target page, context or browser has been closed"
        );
        assert_eq!(stderr_reason(&VecDeque::new()), None);
    }

    #[test]
    fn worker_stderr_excerpts_never_leak_proxy_credentials() {
        let line =
            stderr_excerpt("Error: --proxy-server=http://user:secret@127.0.0.1:7890 refused")
                .unwrap();
        assert!(!line.contains("secret"));
        assert!(!line.contains("user:"));
        assert!(line.contains("http://***@127.0.0.1:7890"));
        assert!(line.contains("refused"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closing_after_the_worker_exits_still_stops_its_browser_process_tree() {
        use std::os::unix::fs::PermissionsExt;

        let runtime = tempfile::tempdir().unwrap();
        // Like Chromium after Playwright detaches it, the fake browser leads
        // its own process group.
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        configure_process_group(&mut command);
        let mut browser = command.spawn().unwrap();
        let browser_pid = browser.id();
        std::fs::write(runtime.path().join("browser.pid"), browser_pid.to_string()).unwrap();
        // Exit like the shutdown watchdog or a crash: report the browser pid,
        // answer the request, then exit without stopping the browser.
        let worker = runtime.path().join("node");
        std::fs::write(
            &worker,
            r#"#!/bin/sh
bh_browser_pid=$(cat "$(dirname "$0")/browser.pid")
read bh_line
printf '%s\n' "{\"event\":\"browserStarted\",\"browserPid\":${bh_browser_pid}}"
printf '%s\n' '{"id":1,"ok":true,"data":null}'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(runtime.path().join("worker.mjs"), b"").unwrap();

        let mut session = BrowserSession::spawn(runtime.path(), Arc::new(|_| {})).unwrap();
        session.request("probe", json!({})).await.unwrap();
        assert_eq!(session.browser_pid, Some(browser_pid));
        session.close().await;

        let mut stopped = false;
        for _ in 0..100 {
            if browser.try_wait().unwrap().is_some() {
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            stopped,
            "a browser left behind by the worker must be stopped"
        );
    }

    #[test]
    fn worker_stderr_excerpts_keep_ordinary_diagnostics() {
        assert_eq!(
            stderr_excerpt("  Error: listen EADDRINUSE: address already in use  ").unwrap(),
            "Error: listen EADDRINUSE: address already in use"
        );
        assert_eq!(stderr_excerpt("   "), None);
        // 路径中的 @ 不是 userinfo，保持原样。
        assert_eq!(
            stderr_excerpt("open /tmp/a@b/cache").unwrap(),
            "open /tmp/a@b/cache"
        );
        let long = stderr_excerpt(&"x".repeat(STDERR_LINE_LIMIT + 50)).unwrap();
        assert_eq!(long.chars().count(), STDERR_LINE_LIMIT);
    }
}
