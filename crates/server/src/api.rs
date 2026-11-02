//! HTTP RPC、事件流与 Web 前端静态文件。

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
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

use crate::host::ServerHost;

#[derive(Clone)]
pub struct AppState {
    pub host: Arc<ServerHost>,
}

pub async fn serve(
    addr: std::net::SocketAddr,
    host: Arc<ServerHost>,
    web_dir: Option<std::path::PathBuf>,
) -> Result<(), String> {
    let state = AppState { host: host.clone() };
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/rpc", post(rpc))
        .route("/events", get(events))
        .with_state(state);

    router = match web_dir {
        Some(dir) if dir.is_dir() => {
            let index = dir.join("index.html");
            router.fallback_service(ServeDir::new(&dir).not_found_service(ServeFile::new(index)))
        }
        _ => router.fallback(get(index)),
    };

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("监听 {addr} 失败: {error}"))?;
    log::info!("BalanceHub 服务端已启动：http://{addr}");
    axum::serve(listener, router)
        .await
        .map_err(|error| format!("服务端退出: {error}"))
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

/// 单次请求可以带一条命令，也可以带一批；批处理让首屏只发一个请求。
async fn rpc(
    State(_state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
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
        let result = balancehub_core::api::dispatch(&request.command, request.args).await;
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

async fn events(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| event_stream(socket, state))
}

async fn event_stream(mut socket: WebSocket, state: AppState) {
    let mut receiver = state.host.subscribe();
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
                let Ok(text) = serde_json::to_string(&event) else { continue };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
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
