//! HTTP RPC、事件流与 Web 前端静态文件。

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use balancehub_core::app_events::BackgroundTaskEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    clients::ClientRegistry,
    host::{ServerEvent, ServerHost},
};

#[derive(Clone)]
pub struct AppState {
    pub host: Arc<ServerHost>,
    pub clients: Arc<ClientRegistry>,
    pub guard: RequestGuard,
}

/// 浏览器发来的请求必须同时通过 Host/Origin 与 JSON Content-Type 检查。
///
/// 服务端没有内置登录，这两个检查不能替代鉴权，但能挡住最常见的一类
/// 跨站请求：任意网页用 `text/plain` 简单请求直接调用 `/rpc`，不触发预检
/// 就能产生副作用；DNS rebinding 则通过伪造 Host 绕过同源限制。
#[derive(Clone, Default)]
pub struct RequestGuard {
    /// `None` 表示不做 Host 校验，只用于进程内测试与嵌入场景。
    allowed_hosts: Option<Arc<Vec<String>>>,
    allowed_origins: Arc<Vec<String>>,
    /// 监听 `0.0.0.0` / `::` 时用户会用局域网 IP 访问；IP 直连不需要事先
    /// 配置，DNS 主机名仍必须显式列出。DNS rebinding 用的正是主机名，
    /// 因此放行 IP 字面量不会绕过护栏。
    allow_ip_literal_hosts: bool,
}

impl RequestGuard {
    pub fn from_env(addr: std::net::SocketAddr) -> Self {
        let port = addr.port();
        let mut hosts = vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ];
        if !addr.ip().is_unspecified() {
            hosts.push(addr.to_string().to_ascii_lowercase());
        }
        hosts.extend(env_list("BALANCEHUB_ALLOWED_HOSTS"));
        let origins = env_list("BALANCEHUB_ALLOWED_ORIGINS")
            .into_iter()
            .map(|value| value.trim_end_matches('/').to_ascii_lowercase())
            .collect();
        Self {
            allowed_hosts: Some(Arc::new(hosts)),
            allowed_origins: Arc::new(origins),
            allow_ip_literal_hosts: addr.ip().is_unspecified(),
        }
    }

    #[cfg(test)]
    fn permissive() -> Self {
        Self::default()
    }

    pub fn check(&self, headers: &HeaderMap) -> Result<(), ApiError> {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok());
        if let Some(origin) = headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let origin = origin.trim_end_matches('/').to_ascii_lowercase();
            if origin == "null" {
                return Err(ApiError::forbidden("请求来源不被允许".to_string()));
            }
            if self
                .allowed_origins
                .iter()
                .any(|allowed| allowed == &origin)
            {
                return Ok(());
            }
            let same_host = origin_authority(&origin)
                .zip(host)
                .is_some_and(|(authority, host)| authority.eq_ignore_ascii_case(host));
            if !same_host {
                return Err(ApiError::forbidden("请求来源不被允许".to_string()));
            }
        }
        let Some(allowed_hosts) = &self.allowed_hosts else {
            return Ok(());
        };
        if allowed_hosts.iter().any(|allowed| allowed == "*") {
            return Ok(());
        }
        // HTTP/2 可能以 :authority 传递 Host；没有 Host 时不做此项校验。
        let Some(host) = host else {
            return Ok(());
        };
        let host_allowed = allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
            || (self.allow_ip_literal_hosts && is_ip_literal_host(host));
        if !host_allowed {
            return Err(ApiError::forbidden("请求 Host 不被允许".to_string()));
        }
        Ok(())
    }
}

/// `192.168.1.5:8787`、`[::1]:8787` 这类 Host 说明浏览器是直接用 IP 访问。
fn is_ip_literal_host(host: &str) -> bool {
    let bare = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else if let Some((name, port)) = host.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
    } else {
        host
    };
    bare.parse::<std::net::IpAddr>().is_ok()
}

fn env_list(name: &str) -> Vec<String> {
    std::env::var(name)
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn origin_authority(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    Some(rest.split('/').next().unwrap_or(rest))
}

fn require_json_content_type(headers: &HeaderMap) -> Result<(), ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    if media_type.eq_ignore_ascii_case("application/json") {
        Ok(())
    } else {
        Err(ApiError::unsupported_media_type(
            "请求必须使用 application/json".to_string(),
        ))
    }
}

/// 服务端进程启动时间，用于状态接口报告运行时长。
static STARTED: OnceLock<Instant> = OnceLock::new();
/// 退出时通知所有事件流主动断开；否则 `with_graceful_shutdown` 会一直等
/// 浏览器标签页关闭 WebSocket，容器只能超时强杀。
static SHUTDOWN: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub async fn serve(
    addr: std::net::SocketAddr,
    host: Arc<ServerHost>,
    web_dir: Option<std::path::PathBuf>,
) -> Result<(), String> {
    STARTED.get_or_init(Instant::now);
    let router = router_with_guard(host, web_dir, RequestGuard::from_env(addr));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("监听 {addr} 失败: {error}"))?;
    log::info!("BalanceHub 服务端已启动：http://{addr}");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("服务端退出: {error}"))
}

/// 容器和 systemd 用 SIGTERM 停止进程；没有它时正在写入的同步任务会被直接
/// 截断。桌面版没有这个入口，服务端收到中断后让在途请求先结束再退出。
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            log::warn!("监听中断信号失败：{error}");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                log::warn!("监听 SIGTERM 失败：{error}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    log::info!("收到退出信号，正在关闭服务端");
    SHUTDOWN.notify_waiters();
}

#[cfg(test)]
fn router(host: Arc<ServerHost>, web_dir: Option<std::path::PathBuf>) -> Router {
    // 进程内测试不走浏览器，没有 Host/Origin 需要校验；生产入口是 `serve`。
    router_with_guard(host, web_dir, RequestGuard::permissive())
}

fn router_with_guard(
    host: Arc<ServerHost>,
    web_dir: Option<std::path::PathBuf>,
    guard: RequestGuard,
) -> Router {
    state_router(
        AppState {
            host,
            clients: Arc::new(ClientRegistry::default()),
            guard,
        },
        web_dir,
    )
}

fn state_router(state: AppState, web_dir: Option<std::path::PathBuf>) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        // 配置导入会把整份 JSON 放进请求体；默认 2 MiB 会让较大的备份在服务端被拒。
        // 上限与核心配置文件上限保持同一量级，并给 JSON 字符串转义留出余量。
        .route(
            "/rpc",
            post(rpc).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/events", get(events))
        .with_state(state);

    router = match web_dir {
        Some(dir) if dir.is_dir() => {
            let index = dir.join("index.html");
            router.fallback_service(ServeDir::new(&dir).not_found_service(ServeFile::new(index)))
        }
        _ => router.fallback(get(index)),
    };
    router
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    ok: bool,
    version: &'static str,
    uptime_seconds: u64,
    data_revision: u64,
    clients: usize,
    /// 最近一次后台任务状态；失败原因已经按事件结构保存，不额外暴露配置内容。
    tasks: Vec<BackgroundTaskEvent>,
}

async fn status(State(state): State<AppState>) -> Json<Status> {
    Json(Status {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: STARTED
            .get()
            .map_or(0, |started| started.elapsed().as_secs()),
        data_revision: state.host.data_revision(),
        clients: state.clients.online(),
        tasks: state.host.task_journal(),
    })
}

#[derive(Deserialize)]
struct RpcRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Deserialize)]
struct RpcBatch {
    #[serde(default)]
    commands: Vec<RpcRequest>,
}

/// 一个批次的命令数上限。首屏只带十几条命令；没有上限时，一个请求就能让
/// 服务端串行执行任意多条命令，既占住连接也放大误操作的影响。
const MAX_RPC_BATCH: usize = 64;

/// 流式结果命令对应的事件名；与前端 `channelEvents` 保持一致。
const PROGRESS_EVENTS: &[(&str, &str)] = &[
    ("get_agent_asset_catalog", "agent-asset-catalog-progress"),
    (
        "read_agent_catalog_content",
        "agent-catalog-content-progress",
    ),
];

/// 把流式结果只发给发起请求的那个标签页。
struct EventSink {
    event: &'static str,
    client: String,
    clients: Arc<ClientRegistry>,
}

impl balancehub_agent::api::ProgressSink for EventSink {
    fn send(&self, payload: Value) -> Result<(), String> {
        // 事件流断开时进度无处可送；请求的最终结果仍会正常返回。
        self.clients.send(
            &self.client,
            ServerEvent {
                event: self.event.to_owned(),
                payload,
            },
        );
        Ok(())
    }
}

fn progress_sink(
    command: &str,
    client: &str,
    clients: &Arc<ClientRegistry>,
) -> Option<Arc<dyn balancehub_agent::api::ProgressSink>> {
    PROGRESS_EVENTS
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, event)| {
            Arc::new(EventSink {
                event,
                client: client.to_owned(),
                clients: Arc::clone(clients),
            }) as Arc<dyn balancehub_agent::api::ProgressSink>
        })
}

async fn dispatch(
    command: &str,
    args: Value,
    client: &str,
    clients: &Arc<ClientRegistry>,
) -> Result<Value, Value> {
    if balancehub_agent::api::COMMANDS.contains(&command) {
        return balancehub_agent::api::dispatch(
            command,
            args,
            client,
            progress_sink(command, client, clients),
        )
        .await;
    }
    balancehub_core::api::dispatch(command, args)
        .await
        .map_err(Value::String)
}

/// 单次请求可以带一条命令，也可以带一批；批处理让首屏只发一个请求。
async fn rpc(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    state.guard.check(&headers)?;
    require_json_content_type(&headers)?;
    // 浏览器会话标识用于区分标签页的资产访问身份；缺失时归入同一默认身份。
    let client = headers
        .get("x-balancehub-client")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .unwrap_or("web")
        .to_owned();
    let body: Value = serde_json::from_slice(&body)
        .map_err(|error| ApiError::bad_request(format!("请求格式无效：{error}")))?;
    let requests = if body.get("commands").is_some() {
        serde_json::from_value::<RpcBatch>(body)
            .map_err(|error| ApiError::bad_request(format!("请求格式无效：{error}")))?
            .commands
    } else {
        vec![serde_json::from_value::<RpcRequest>(body)
            .map_err(|error| ApiError::bad_request(format!("请求格式无效：{error}")))?]
    };
    if requests.len() > MAX_RPC_BATCH {
        return Err(ApiError::bad_request(format!(
            "一次最多执行 {MAX_RPC_BATCH} 条命令"
        )));
    }

    let mut results = Vec::with_capacity(requests.len());
    for request in requests {
        let result = dispatch(&request.command, request.args, &client, &state.clients).await;
        results.push(match result {
            Ok(data) => serde_json::json!({ "command": request.command, "ok": true, "data": data }),
            Err(error) => {
                serde_json::json!({ "command": request.command, "ok": false, "error": error })
            }
        });
    }

    if results.len() == 1 {
        return Ok(Json(results.remove(0)));
    }
    Ok(Json(serde_json::json!({ "results": results })))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    client: Option<String>,
}

async fn events(
    ws: WebSocketUpgrade,
    Query(query): Query<EventsQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    state.guard.check(&headers)?;
    let client = query
        .client
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .unwrap_or_else(|| "web".to_owned());
    Ok(ws.on_upgrade(move |socket| event_stream(socket, state, client)))
}

async fn event_stream(mut socket: WebSocket, state: AppState, client: String) {
    let mut receiver = state.host.subscribe();
    let (sender, mut targeted) = tokio::sync::mpsc::unbounded_channel();
    let generation = state.clients.connect(&client, sender);
    // 浏览器拿不到 WebSocket 控制帧；用普通文本心跳让前端能判断连接是否
    // 还活着，同时让反代和系统看到持续流量。
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(25));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _ = SHUTDOWN.notified() => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            _ = heartbeat.tick() => {
                let event = ServerEvent {
                    event: "heartbeat".to_owned(),
                    payload: Value::Null,
                };
                if send_event(&mut socket, &event).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    // 事件流是单向的；客户端发来的内容只用来探测连接。
                    Some(Ok(_)) => {}
                }
            }
            event = receiver.recv() => {
                let Ok(event) = event else { continue };
                if send_event(&mut socket, &event).await.is_err() {
                    break;
                }
            }
            event = targeted.recv() => {
                let Some(event) = event else { break };
                if send_event(&mut socket, &event).await.is_err() {
                    break;
                }
            }
        }
    }
    state.clients.disconnect(&client, generation);
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(event) else {
        return Ok(());
    };
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

pub struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: serde_json::json!({ "error": message }),
        }
    }

    fn forbidden(message: String) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: serde_json::json!({ "error": message }),
        }
    }

    fn unsupported_media_type(message: String) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            body: serde_json::json!({ "error": message }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use balancehub_core::{host as core_host, models::AppData, runtime, state::CoreState};
    use futures_util::StreamExt;
    use std::time::Duration;
    use tower::ServiceExt;

    /// 进程级宿主只能安装一次；每个测试各建一个会让后安装的测试静默沿用
    /// 前一个测试的目录，前一个测试清理后又把命令指向已删除的配置。
    /// 这里统一建一份共享宿主与独立运行时，测试只替换护栏和客户端注册表。
    fn shared_host() -> Arc<ServerHost> {
        static HOST: std::sync::OnceLock<Arc<ServerHost>> = std::sync::OnceLock::new();
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        HOST.get_or_init(|| {
            let root = std::env::temp_dir()
                .join(format!("balancehub-server-shared-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for dir in ["data", "config", "cache"] {
                std::fs::create_dir_all(root.join(dir)).unwrap();
            }
            let host = Arc::new(ServerHost::new(
                root.join("data"),
                root.join("config"),
                root.join("cache"),
            ));
            let _ = core_host::install(host.clone());
            let runtime = RUNTIME.get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap()
            });
            let _ = runtime::install(runtime.handle().clone());
            host.attach_core(Arc::new(CoreState::new(AppData::default())));
            host
        })
        .clone()
    }

    fn shared_state(guard: RequestGuard) -> AppState {
        AppState {
            host: shared_host(),
            clients: Arc::new(ClientRegistry::default()),
            guard,
        }
    }

    /// 两条事件流分别属于不同标签页：定向进度只到达发起请求的那一条。
    #[tokio::test]
    async fn targeted_progress_reaches_only_the_requesting_tab() {
        let state = shared_state(RequestGuard::permissive());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = state_router(state.clone(), None);
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        type Socket = tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >;
        async fn connect(addr: std::net::SocketAddr, client: &str) -> Socket {
            let (socket, _) =
                tokio_tungstenite::connect_async(format!("ws://{addr}/events?client={client}"))
                    .await
                    .unwrap();
            socket
        }
        async fn next_event(socket: &mut Socket) -> Option<String> {
            loop {
                let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
                    .await
                    .ok()??
                    .ok()?;
                if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
                    return Some(text.to_string());
                }
            }
        }

        let mut tab_a = connect(addr, "tab-a").await;
        let mut tab_b = connect(addr, "tab-b").await;
        // 连接刚建立时服务端可能还在登记，给两个标签页一点时间。
        tokio::time::sleep(Duration::from_millis(200)).await;

        let sink = progress_sink("get_agent_asset_catalog", "tab-a", &state.clients)
            .expect("该命令应有进度通道");
        sink.send(serde_json::json!({ "revision": "r1" })).unwrap();

        let frame = next_event(&mut tab_a)
            .await
            .expect("发起请求的标签页应收到进度");
        assert!(frame.contains("agent-asset-catalog-progress"));
        assert!(
            tokio::time::timeout(Duration::from_millis(500), next_event(&mut tab_b))
                .await
                .is_err(),
            "其他标签页不应收到这次进度"
        );

        // 断开后身份在宽限期结束前保留，期间仍可定向投递。
        drop(tab_a);
        drop(tab_b);
    }

    /// `/rpc` 必须同时接受单条命令与批处理，并把失败限定在对应命令上。
    #[tokio::test]
    async fn rpc_batch_reports_each_command_independently() {
        let app = router(shared_host(), None);

        let health = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let body = serde_json::json!({
            "commands": [
                { "command": "load_app_data", "args": {} },
                { "command": "not_a_command", "args": {} }
            ]
        })
        .to_string();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let results = value["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["command"], "load_app_data");
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[1]["ok"], false);
        assert!(results[1]["error"].as_str().unwrap().contains("不支持"));

        // 存储层在测试里没有写过文件，确认单条命令的错误同样按约定返回。
        let single = serde_json::json!({ "command": "load_app_data" }).to_string();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(single))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["command"], "load_app_data");
        assert_eq!(value["ok"], true);

        // 配置导入可能远大于默认 2 MiB 请求体上限；验证 /rpc 接受较大的请求。
        let large = serde_json::json!({
            "command": "load_app_data",
            "args": { "padding": "x".repeat(3 * 1024 * 1024) }
        })
        .to_string();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(large))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// 站点浏览器登录与登录账号管理必须由服务端分发，而不是只在桌面注册。
    #[tokio::test]
    async fn rpc_dispatches_browser_login_and_account_commands() {
        let app = router(shared_host(), None);

        async fn rpc(app: &axum::Router, command: &str, args: Value) -> Value {
            let body = serde_json::json!({ "command": command, "args": args }).to_string();
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/rpc")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        }

        let capabilities = rpc(&app, "get_host_capabilities", Value::Null).await;
        let commands = capabilities["data"]["commands"].as_array().unwrap();
        assert!(commands.iter().any(|item| item == "list_login_accounts"));
        assert!(commands
            .iter()
            .any(|item| item == "read_login_account_cookie"));
        assert!(capabilities["data"]["version"].is_string());

        let accounts = rpc(&app, "list_login_accounts", Value::Null).await;
        assert_eq!(accounts["ok"], true);
        assert!(accounts["data"].is_array());

        // 参数名按前端 IPC 的 camelCase 传递；缺失账号时应走到业务校验，
        // 而不是报「参数无效」或「命令不支持」。
        let cookie = rpc(
            &app,
            "read_login_account_cookie",
            serde_json::json!({ "id": "missing", "cookieId": "x" }),
        )
        .await;
        assert_eq!(cookie["ok"], false);
        assert!(cookie["error"].as_str().unwrap().contains("登录账号"));

        let login = rpc(
            &app,
            "start_provider_browser_login",
            serde_json::json!({ "input": {}, "loginAccountId": "missing" }),
        )
        .await;
        assert_eq!(login["ok"], false);
        assert!(login["error"].as_str().unwrap().contains("登录账号"));
    }

    /// 状态接口不依赖全局宿主；用独立宿主避免与其它测试争夺
    /// 进程级 `install` 的单例。
    fn standalone_state(prefix: &str) -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["data", "config", "cache"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let host = Arc::new(ServerHost::new(
            root.join("data"),
            root.join("config"),
            root.join("cache"),
        ));
        host.attach_core(Arc::new(CoreState::new(AppData::default())));
        (
            AppState {
                host,
                clients: Arc::new(ClientRegistry::default()),
                guard: RequestGuard::permissive(),
            },
            root,
        )
    }

    /// 无界面后端没有任务面板；状态接口要能回答最近一次后台任务的结果。
    #[tokio::test]
    async fn status_reports_runtime_and_background_tasks() {
        use balancehub_core::host::Host;

        let (state, root) = standalone_state("balancehub-server-status");
        state.host.emit(
            balancehub_core::app_events::BACKGROUND_TASK_EVENT,
            serde_json::json!({
                "taskId": "scheduler-refresh",
                "kind": "autoRefresh",
                "status": "failed",
                "title": "自动刷新中转站",
                "detail": "刷新失败",
                "progress": 1.0,
                "startedAt": 1,
                "finishedAt": 2,
                "error": "站点不可达"
            }),
        );
        let app = state_router(state, None);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["dataRevision"], 1);
        assert_eq!(value["clients"], 0);
        assert_eq!(value["tasks"][0]["taskId"], "scheduler-refresh");
        assert_eq!(value["tasks"][0]["status"], "failed");
        assert_eq!(value["tasks"][0]["error"], "站点不可达");
        let _ = std::fs::remove_dir_all(root);
    }

    fn guard(hosts: &[&str], origins: &[&str]) -> RequestGuard {
        RequestGuard {
            allowed_hosts: Some(Arc::new(
                hosts.iter().map(|value| value.to_string()).collect(),
            )),
            allowed_origins: Arc::new(origins.iter().map(|value| value.to_string()).collect()),
            allow_ip_literal_hosts: false,
        }
    }

    /// 监听 0.0.0.0 时浏览器用局域网 IP 直接访问；IP 字面量放行，域名仍拦。
    #[test]
    fn ip_literal_hosts_are_accepted_only_when_configured() {
        assert!(is_ip_literal_host("192.168.1.5:8787"));
        assert!(is_ip_literal_host("[::1]:8787"));
        assert!(is_ip_literal_host("10.0.0.8"));
        assert!(!is_ip_literal_host("evil.example:8787"));
        assert!(!is_ip_literal_host("localhost:8787"));

        let mut lan = guard(&["127.0.0.1:8787"], &[]);
        lan.allow_ip_literal_hosts = true;
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "192.168.1.5:8787".parse().unwrap());
        assert!(lan.check(&headers).is_ok());
        headers.insert(header::HOST, "evil.example:8787".parse().unwrap());
        assert_eq!(
            lan.check(&headers).unwrap_err().status,
            StatusCode::FORBIDDEN
        );
    }

    /// 服务端没有登录，跨站简单请求、DNS rebinding 和伪造 Host 是三条
    /// 不经过浏览器的写入口；请求护栏要分别挡住。
    #[tokio::test]
    async fn rpc_guard_rejects_cross_site_rebinding_and_non_json_requests() {
        let state = shared_state(guard(&["127.0.0.1:8787"], &[]));
        let app = state_router(state, None);
        let call = |headers: &[(&str, &str)], body: String| {
            let mut builder = axum::http::Request::builder().method("POST").uri("/rpc");
            for (name, value) in headers {
                builder = builder.header(*name, *value);
            }
            app.clone()
                .oneshot(builder.body(axum::body::Body::from(body)).unwrap())
        };

        // 简单请求：text/plain 不触发预检，任何网页都能发出来。
        let response = call(
            &[("host", "127.0.0.1:8787"), ("content-type", "text/plain")],
            serde_json::json!({ "command": "load_app_data" }).to_string(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // 跨站页面即使能发 JSON，也过不了 Origin 检查。
        let response = call(
            &[
                ("host", "127.0.0.1:8787"),
                ("origin", "https://evil.example"),
                ("content-type", "application/json"),
            ],
            serde_json::json!({ "command": "load_app_data" }).to_string(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // DNS rebinding 把 Host 换成攻击者域名。
        let response = call(
            &[
                ("host", "evil.example:8787"),
                ("content-type", "application/json"),
            ],
            serde_json::json!({ "command": "load_app_data" }).to_string(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // 同源请求照常执行。
        let response = call(
            &[
                ("host", "127.0.0.1:8787"),
                ("origin", "http://127.0.0.1:8787"),
                ("content-type", "application/json"),
            ],
            serde_json::json!({ "command": "load_app_data" }).to_string(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// 批处理没有上限时，一个请求就能串行执行任意多条命令。
    #[tokio::test]
    async fn rpc_batch_is_bounded() {
        let app = state_router(shared_state(RequestGuard::permissive()), None);
        let commands: Vec<Value> = (0..=MAX_RPC_BATCH)
            .map(|_| serde_json::json!({ "command": "load_app_data", "args": {} }))
            .collect();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({ "commands": commands }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
