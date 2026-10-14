//! Explicit, cancellable connection diagnostics. Listing assets never starts MCP.
mod connection;
mod http;
mod stdio;

use crate::{
    models::{AgentMcpCheck, AgentMcpTool, AppSettings},
    network,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, Semaphore};

const PROTOCOL: &str = "2025-06-18";
const MESSAGE_LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const MAX_TOOLS: usize = 5_000;

pub(crate) struct McpCheckService {
    jobs: Mutex<BTreeMap<String, Arc<Job>>>,
    slots: Arc<Semaphore>,
}
impl Default for McpCheckService {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
            slots: Arc::new(Semaphore::new(4)),
        }
    }
}

struct Job {
    actor: String,
    started: Instant,
    state: Mutex<AgentMcpCheck>,
    canceled: AtomicBool,
    notify: Notify,
}
impl Job {
    fn update(&self, phase: &str, message: &str) {
        if let Ok(mut state) = self.state.lock() {
            phase.clone_into(&mut state.phase);
            message.clone_into(&mut state.message);
            state.elapsed_ms = self.started.elapsed().as_millis() as u64;
        }
    }
    fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
    async fn cancelled(&self) {
        if self.canceled.load(Ordering::SeqCst) {
            return;
        }
        self.notify.notified().await;
    }
    fn snapshot(&self) -> Result<AgentMcpCheck, String> {
        let mut state = self.state.lock().map_err(|_| "连接检查状态不可用")?.clone();
        if state.outcome.is_none() {
            state.elapsed_ms = self.started.elapsed().as_millis() as u64;
        }
        Ok(state)
    }
}

#[derive(Debug)]
struct CheckError {
    outcome: &'static str,
    message: String,
}
impl CheckError {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            outcome: "failed",
            message: message.into(),
        }
    }
    fn timeout() -> Self {
        Self {
            outcome: "timedOut",
            message: "此阶段等待超时，可重试或在原生 Agent 中核对服务启动情况".to_owned(),
        }
    }
}
type Result<T, E = CheckError> = std::result::Result<T, E>;

#[async_trait::async_trait]
trait Connection: Send {
    async fn send(&mut self, value: Value) -> Result<Option<Value>>;
    fn negotiated(&mut self, _version: &str) {}
    async fn close(&mut self) {}
}

impl McpCheckService {
    pub(crate) fn start(
        self: &Arc<Self>,
        actor: String,
        text: String,
        settings: AppSettings,
    ) -> std::result::Result<AgentMcpCheck, String> {
        let input = connection::Input::parse(&text).map_err(|error| error.message)?;
        let id = crate::services::agent_cli::environment::mutation::token::opaque_id()
            .map_err(|_| "无法创建检查任务")?;
        let state = AgentMcpCheck {
            id: id.clone(),
            phase: "queued".to_owned(),
            outcome: None,
            message: "等待连接检查".to_owned(),
            server_name: None,
            protocol_version: None,
            tools: Vec::new(),
            elapsed_ms: 0,
        };
        let job = Arc::new(Job {
            actor,
            state: Mutex::new(state.clone()),
            started: Instant::now(),
            canceled: AtomicBool::new(false),
            notify: Notify::new(),
        });
        {
            let mut jobs = self.jobs.lock().map_err(|_| "连接检查状态不可用")?;
            jobs.retain(|_, value| {
                value.started.elapsed() < Duration::from_secs(30 * 60)
                    || value.snapshot().is_ok_and(|state| state.outcome.is_none())
            });
            if jobs.len() >= 128 {
                if let Some(oldest) = jobs
                    .iter()
                    .filter(|(_, job)| job.snapshot().is_ok_and(|state| state.outcome.is_some()))
                    .min_by_key(|(_, job)| job.started)
                    .map(|(id, _)| id.clone())
                {
                    jobs.remove(&oldest);
                } else {
                    return Err("连接检查任务过多，请先取消正在等待的检查".to_owned());
                }
            }
            jobs.insert(id, Arc::clone(&job));
        }
        let slots = Arc::clone(&self.slots);
        tauri::async_runtime::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = job.cancelled() => Err(CheckError { outcome: "canceled", message: "检查已取消".to_owned() }),
                result = tokio::time::timeout(Duration::from_secs(240), async {
                    let _permit = slots.acquire_owned().await.map_err(|_| CheckError::failed("连接检查已关闭"))?;
                    run(&job, input, &settings).await
                }) => result.unwrap_or_else(|_| Err(CheckError::timeout())),
            };
            if let Ok(mut state) = job.state.lock() {
                let stage = state.phase.clone();
                state.phase = "completed".to_owned();
                state.elapsed_ms = job.started.elapsed().as_millis() as u64;
                match result {
                    Ok(()) => {
                        state.outcome = Some("passed".to_owned());
                        state.message = format!("连接正常，已读取 {} 个工具", state.tools.len());
                    }
                    Err(error) => {
                        state.outcome = Some(error.outcome.to_owned());
                        state.message = format!("{}：{}", stage_label(&stage), error.message);
                    }
                }
            };
        });
        Ok(state)
    }

    pub(crate) fn get(
        &self,
        actor: &str,
        id: &str,
        cancel: bool,
    ) -> std::result::Result<AgentMcpCheck, String> {
        let job = self
            .jobs
            .lock()
            .map_err(|_| "检查状态不可用")?
            .get(id)
            .cloned()
            .filter(|job| job.actor == actor)
            .ok_or("连接检查不存在或已过期")?;
        if cancel {
            job.cancel();
        }
        job.snapshot()
    }
    pub(crate) fn cancel_actor(&self, actor: &str) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values().filter(|job| job.actor == actor) {
                job.cancel();
            }
        }
    }
}

fn stage_label(stage: &str) -> &'static str {
    match stage {
        "connecting" => "建立连接",
        "initializing" => "协议初始化",
        "listing" => "读取工具",
        _ => "连接检查",
    }
}

async fn request(
    connection: &mut dyn Connection,
    id: u64,
    method: &str,
    params: Value,
    budget: Duration,
) -> Result<Value> {
    let value = tokio::time::timeout(
        budget,
        connection.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})),
    )
    .await
    .map_err(|_| CheckError::timeout())??
    .ok_or_else(|| CheckError::failed("服务没有返回 JSON-RPC 响应"))?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("id") != Some(&Value::from(id))
        || value.get("method").is_some()
    {
        return Err(CheckError::failed("服务返回了不匹配的 JSON-RPC 响应"));
    }
    if let Some(error) = value.get("error") {
        return Err(CheckError::failed(format!(
            "服务返回 JSON-RPC 错误（代码 {}）",
            error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        )));
    }
    value
        .get("result")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| CheckError::failed("服务响应缺少 result 对象"))
}

async fn run(job: &Job, input: connection::Input, settings: &AppSettings) -> Result<()> {
    let budget = Duration::from_secs(if input.transport == "stdio" { 30 } else { 60 });
    job.update("connecting", "正在建立连接");
    let mut connection: Box<dyn Connection> = tokio::time::timeout(budget, async {
        if input.transport == "stdio" {
            Ok(
                Box::new(stdio::StdioConnection::open(&input, settings).await?)
                    as Box<dyn Connection>,
            )
        } else {
            Ok(Box::new(
                http::HttpConnection::open(
                    &input,
                    network::build_mcp_client(settings).map_err(CheckError::failed)?,
                )
                .await?,
            ) as Box<dyn Connection>)
        }
    })
    .await
    .map_err(|_| CheckError::timeout())??;
    job.update("initializing", "正在协商 MCP 协议");
    let initialize = request(connection.as_mut(), 1, "initialize", json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"BalanceHub","version":env!("CARGO_PKG_VERSION")}}), budget).await?;
    let version = initialize
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| CheckError::failed("初始化响应没有协议版本"))?;
    if !["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"].contains(&version) {
        return Err(CheckError::failed(format!(
            "服务选择了尚未支持的协议版本：{}",
            version.chars().take(32).collect::<String>()
        )));
    }
    connection.negotiated(version);
    if let Ok(mut state) = job.state.lock() {
        state.protocol_version = Some(version.to_owned());
        state.server_name = initialize
            .pointer("/serverInfo/name")
            .and_then(Value::as_str)
            .map(|name| name.chars().take(256).collect());
    }
    tokio::time::timeout(
        budget,
        connection.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
    )
    .await
    .map_err(|_| CheckError::timeout())??;
    if initialize.pointer("/capabilities/tools").is_none() {
        connection.close().await;
        return Ok(());
    }
    job.update("listing", "正在读取工具列表");
    let mut cursor = None::<String>;
    let mut cursors = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut id = 2;
    loop {
        let result = request(
            connection.as_mut(),
            id,
            "tools/list",
            cursor
                .as_ref()
                .map_or_else(|| json!({}), |cursor| json!({"cursor":cursor})),
            budget,
        )
        .await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| CheckError::failed("工具列表格式无效"))?;
        let mut page = Vec::new();
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty() && name.len() <= 512)
                .ok_or_else(|| CheckError::failed("工具缺少有效名称"))?;
            if names.insert(name.to_owned()) {
                page.push(AgentMcpTool {
                    name: name.to_owned(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .chars()
                        .take(1024)
                        .collect(),
                });
            }
        }
        if names.len() > MAX_TOOLS {
            return Err(CheckError::failed(
                "服务工具超过单次检查容量，已保留读取到的部分",
            ));
        }
        if let Ok(mut state) = job.state.lock() {
            state.tools.extend(page);
            state.message = format!("已读取 {} 个工具", state.tools.len());
        }
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);
        let Some(next) = &cursor else { break };
        if next.len() > 8192 || !cursors.insert(next.clone()) {
            return Err(CheckError::failed("工具列表游标重复或无效"));
        }
        id += 1;
    }
    connection.close().await;
    Ok(())
}
