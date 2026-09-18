use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::ChildStdin,
    sync::{oneshot, Mutex as AsyncMutex},
};

type WindowReply = oneshot::Sender<Result<(), String>>;

struct ControlState {
    input: AsyncMutex<Option<ChildStdin>>,
    sequence: AtomicU64,
    waiting: Mutex<HashMap<u64, WindowReply>>,
}

/// A bounded control channel; the active browser request remains the only reader.
#[derive(Clone)]
pub(crate) struct BrowserWindowControl(Arc<ControlState>);

impl BrowserWindowControl {
    pub(super) fn new(input: ChildStdin) -> Self {
        Self(Arc::new(ControlState {
            input: AsyncMutex::new(Some(input)),
            sequence: AtomicU64::new(0),
            waiting: Mutex::new(HashMap::new()),
        }))
    }

    pub(super) async fn write(&self, message: &[u8]) -> Result<(), String> {
        self.0
            .input
            .lock()
            .await
            .as_mut()
            .ok_or("浏览器通道已关闭")?
            .write_all(message)
            .await
            .map_err(|_| "浏览器通道已关闭".into())
    }

    pub(crate) async fn show(&self) -> Result<(), String> {
        let id = self.0.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let (send, receive) = oneshot::channel();
        self.0
            .waiting
            .lock()
            .map_err(|_| "无法显示登录窗口")?
            .insert(id, send);
        let message = format!("{}\n", json!({ "controlId": id, "op": "showWindow" }));
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            self.write(message.as_bytes()).await?;
            receive.await.map_err(|_| "登录窗口已关闭".to_string())?
        })
        .await
        .map_err(|_| "显示登录窗口超时，请稍后重试".to_string())
        .and_then(|result| result);
        if let Ok(mut waiting) = self.0.waiting.lock() {
            waiting.remove(&id);
        }
        result
    }

    pub(super) fn accept_reply(&self, value: &Value) -> bool {
        let Some(id) = value.get("controlId").and_then(Value::as_u64) else {
            return false;
        };
        if let Ok(mut waiting) = self.0.waiting.lock() {
            if let Some(reply) = waiting.remove(&id) {
                let result = if value.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(())
                } else {
                    Err(value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("无法显示登录窗口")
                        .into())
                };
                let _ = reply.send(result);
            }
        }
        true
    }

    pub(super) async fn close(&self) {
        self.abort();
        self.0.input.lock().await.take();
    }

    pub(super) fn abort(&self) {
        if let Ok(mut waiting) = self.0.waiting.lock() {
            for (_, reply) in waiting.drain() {
                let _ = reply.send(Err("登录窗口已关闭".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn control() -> BrowserWindowControl {
        BrowserWindowControl(Arc::new(ControlState {
            input: AsyncMutex::new(None),
            sequence: AtomicU64::new(0),
            waiting: Mutex::new(HashMap::new()),
        }))
    }

    #[tokio::test]
    async fn window_replies_do_not_consume_the_login_response() {
        let control = control();
        let (send, receive) = oneshot::channel();
        control.0.waiting.lock().unwrap().insert(1, send);
        assert!(!control.accept_reply(&json!({ "id": 1, "ok": true })));
        assert!(control.accept_reply(&json!({ "controlId": 1, "ok": true })));
        assert!(receive.await.unwrap().is_ok());
        assert!(control.accept_reply(&json!({ "controlId": 1, "ok": true })));
    }

    #[tokio::test]
    async fn closing_releases_waiters_and_failed_sends_leave_no_pending_control() {
        let control = control();
        let (send, receive) = oneshot::channel();
        control.0.waiting.lock().unwrap().insert(1, send);
        control.close().await;
        assert!(receive.await.unwrap().is_err());
        assert!(control.show().await.is_err());
        assert!(control.0.waiting.lock().unwrap().is_empty());
    }
}
