//! HTTP RPC、事件流与 Web 前端静态文件。

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Query, State,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    clients::ClientRegistry,
    host::{ServerEvent, ServerHost},
};

#[derive(Clone)]
pub struct AppState {
    pub host: Arc<ServerHost>,
    pub clients: Arc<ClientRegistry>,
}

pub async fn serve(
    addr: std::net::SocketAddr,
    host: Arc<ServerHost>,
    web_dir: Option<std::path::PathBuf>,
) -> Result<(), String> {
    let router = router(host, web_dir);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("监听 {addr} 失败: {error}"))?;
    log::info!("BalanceHub 服务端已启动：http://{addr}");
    axum::serve(listener, router)
        .await
        .map_err(|error| format!("服务端退出: {error}"))
}

fn router(host: Arc<ServerHost>, web_dir: Option<std::path::PathBuf>) -> Router {
    state_router(
        AppState {
            host,
            clients: Arc::new(ClientRegistry::default()),
        },
        web_dir,
    )
}

fn state_router(state: AppState, web_dir: Option<std::path::PathBuf>) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health))
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
) -> impl IntoResponse {
    let client = query
        .client
        .filter(|value| !value.is_empty() && value.len() <= 64)
        .unwrap_or_else(|| "web".to_owned());
    ws.on_upgrade(move |socket| event_stream(socket, state, client))
}

async fn event_stream(mut socket: WebSocket, state: AppState, client: String) {
    let mut receiver = state.host.subscribe();
    let (sender, mut targeted) = tokio::sync::mpsc::unbounded_channel();
    let generation = state.clients.connect(&client, sender);
    loop {
        tokio::select! {
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

    /// 准备一个装了核心状态的测试宿主；返回路由和它的客户端注册表。
    fn test_state() -> (AppState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("balancehub-server-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["data", "config", "cache"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let host = Arc::new(ServerHost::new(
            root.join("data"),
            root.join("config"),
            root.join("cache"),
        ));
        // 同一进程里只允许安装一次宿主；重复调用时沿用已安装的那个。
        let _ = core_host::install(host.clone());
        let _ = runtime::install(tokio::runtime::Handle::current());
        host.attach_core(Arc::new(CoreState::new(AppData::default())));
        (
            AppState {
                host,
                clients: Arc::new(ClientRegistry::default()),
            },
            root,
        )
    }

    /// 两条事件流分别属于不同标签页：定向进度只到达发起请求的那一条。
    #[tokio::test]
    async fn targeted_progress_reaches_only_the_requesting_tab() {
        let (state, root) = test_state();
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
        let _ = std::fs::remove_dir_all(root);
    }

    /// `/rpc` 必须同时接受单条命令与批处理，并把失败限定在对应命令上。
    #[tokio::test]
    async fn rpc_batch_reports_each_command_independently() {
        let root =
            std::env::temp_dir().join(format!("balancehub-server-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let data_dir = root.join("data");
        let config_dir = root.join("config");
        let cache_dir = root.join("cache");
        for dir in [&data_dir, &config_dir, &cache_dir] {
            std::fs::create_dir_all(dir).unwrap();
        }

        let host = Arc::new(ServerHost::new(data_dir.clone(), config_dir, cache_dir));
        // 同一进程里只允许安装一次宿主：另一个测试可能已经装过，沿用即可。
        let _ = core_host::install(host.clone());
        let _ = runtime::install(tokio::runtime::Handle::current());
        host.attach_core(Arc::new(CoreState::new(AppData::default())));
        let app = router(host, None);

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

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 站点浏览器登录与登录账号管理必须由服务端分发，而不是只在桌面注册。
    #[tokio::test]
    async fn rpc_dispatches_browser_login_and_account_commands() {
        let root =
            std::env::temp_dir().join(format!("balancehub-server-login-{}", std::process::id()));
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
        let _ = runtime::install(tokio::runtime::Handle::current());
        host.attach_core(Arc::new(CoreState::new(AppData::default())));
        let app = router(host, None);

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

        let _ = std::fs::remove_dir_all(root);
    }
}
