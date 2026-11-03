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
    let state = AppState { host };
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

#[cfg(test)]
mod tests {
    use super::*;
    use balancehub_core::{host as core_host, models::AppData, runtime, state::CoreState};
    use tower::ServiceExt;

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
        core_host::install(host.clone()).unwrap();
        runtime::install(tokio::runtime::Handle::current()).unwrap();
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

        let _ = std::fs::remove_dir_all(&root);
    }
}
