use super::{connection::Input, CheckError, Connection, Result, MESSAGE_LIMIT};
use crate::{models::AppSettings, network, platform::process};
use serde_json::Value;
use std::{
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

pub(super) struct StdioConnection {
    child: Option<Child>,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    drain: tokio::task::JoinHandle<()>,
}
impl StdioConnection {
    fn prepare(input: &Input, settings: &AppSettings) -> Command {
        let program = if std::path::Path::new(&input.command).components().count() == 1
            && !input
                .environment
                .keys()
                .any(|key| key.eq_ignore_ascii_case("PATH"))
        {
            crate::services::agent_cli::discovery::paths::runtime_binary_candidates(
                &input.command,
                None,
            )
            .into_iter()
            .next()
            .unwrap_or_else(|| (&input.command).into())
        } else {
            (&input.command).into()
        };
        let mut command = Command::new(&program);
        command
            .args(&input.args)
            .envs(&input.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &input.cwd {
            command.current_dir(cwd);
        }
        if !input
            .environment
            .keys()
            .any(|key| key.eq_ignore_ascii_case("PATH"))
        {
            if let Some(path) = crate::services::agent_cli::runtime_path_for(&program) {
                command.env("PATH", path);
            }
        }
        let proxy = network::resolve_global_proxy(settings).environment();
        for key in proxy.removed_names() {
            command.env_remove(key);
        }
        command.envs(proxy.variables());
        process::configure_process_group(&mut command);
        command
    }

    pub async fn open(input: &Input, settings: &AppSettings) -> Result<Self> {
        let input = input.clone();
        let settings = settings.clone();
        // GUI PATH discovery may invoke the login shell. Cancellation drops the
        // awaiting future; the blocking preparation cannot start an MCP process.
        let command =
            tauri::async_runtime::spawn_blocking(move || Self::prepare(&input, &settings))
                .await
                .map_err(|_| CheckError::failed("读取 MCP 启动环境失败"))?;
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            CheckError::failed(match error.kind() {
                std::io::ErrorKind::NotFound => "找不到启动命令，请检查可执行文件路径及所需运行时",
                std::io::ErrorKind::PermissionDenied => "启动命令没有执行权限",
                _ => "无法启动 MCP 服务，请检查命令、参数和工作目录",
            })
        })?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| CheckError::failed("无法打开服务输入"))?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| CheckError::failed("无法读取服务输出"))?,
        );
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| CheckError::failed("无法读取服务诊断"))?;
        // Drain continuously so verbose startup cannot deadlock the handshake.
        // Server stderr can contain credentials; it is never retained in logs.
        let drain = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });
        Ok(Self {
            child: Some(child),
            input,
            output,
            drain,
        })
    }
    async fn line(&mut self) -> Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            let buffer = self
                .output
                .fill_buf()
                .await
                .map_err(|_| CheckError::failed("读取服务输出失败"))?;
            if buffer.is_empty() {
                let code = if let Some(child) = self.child.take() {
                    cleanup(child)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|status| status.code())
                } else {
                    None
                };
                return Err(CheckError::failed(format!(
                    "服务已关闭连接{}",
                    code.map(|code| format!("（退出码 {code}）"))
                        .unwrap_or_default()
                )));
            }
            let end = buffer
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| offset + 1);
            let count = end.unwrap_or(buffer.len());
            if line.len().saturating_add(count) > MESSAGE_LIMIT {
                return Err(CheckError::failed("服务单条消息超过 2 MiB"));
            }
            line.extend_from_slice(&buffer[..count]);
            self.output.consume(count);
            if end.is_some() {
                return Ok(line);
            }
        }
    }
}
#[async_trait::async_trait]
impl Connection for StdioConnection {
    async fn send(&mut self, value: Value) -> Result<Option<Value>> {
        let mut bytes =
            serde_json::to_vec(&value).map_err(|_| CheckError::failed("请求编码失败"))?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|_| CheckError::failed("服务输入已关闭"))?;
        self.input
            .flush()
            .await
            .map_err(|_| CheckError::failed("发送协议消息失败"))?;
        let Some(id) = value.get("id") else {
            return Ok(None);
        };
        loop {
            let line = self.line().await?;
            if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                continue;
            }
            let message: Value = serde_json::from_slice(&line).map_err(|_| {
                CheckError::failed("服务标准输出包含非 JSON-RPC 内容，请将启动日志输出到 stderr")
            })?;
            if message.get("id") == Some(id) && message.get("method").is_none() {
                return Ok(Some(message));
            }
            if let Some(response) = super::connection::server_response(&message) {
                let mut bytes = serde_json::to_vec(&response)
                    .map_err(|_| CheckError::failed("响应编码失败"))?;
                bytes.push(b'\n');
                self.input
                    .write_all(&bytes)
                    .await
                    .map_err(|_| CheckError::failed("服务输入已关闭"))?;
                self.input
                    .flush()
                    .await
                    .map_err(|_| CheckError::failed("发送协议响应失败"))?;
            }
        }
    }
    async fn close(&mut self) {
        let _ = self.input.shutdown().await;
        if let Some(child) = self.child.take() {
            let _ = cleanup(child).await;
        }
    }
}
impl Drop for StdioConnection {
    fn drop(&mut self) {
        self.drain.abort();
        if let Some(child) = self.child.take() {
            std::mem::drop(cleanup(child));
        }
    }
}

fn cleanup(mut child: Child) -> tauri::async_runtime::JoinHandle<Option<std::process::ExitStatus>> {
    // This task retains the handle even if its caller is cancelled. Reaping or
    // dropping the child before delayed PID-based cleanup risks killing a reused
    // PID; Windows taskkill must also stay off the async protocol worker.
    tauri::async_runtime::spawn(async move {
        if let Some(pid) = child.id() {
            let _ =
                tauri::async_runtime::spawn_blocking(move || process::kill_process_tree(pid)).await;
        }
        let _ = child.start_kill();
        tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .ok()?
            .ok()
    })
}
