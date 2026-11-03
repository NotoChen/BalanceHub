//! 浏览器会话连接注册表。
//!
//! 每个浏览器标签页用一个稳定的客户端标识连到事件流。这里负责两件事：
//!
//! 1. **定向进度**：资产目录与资源正文的流式结果只发给发起请求的那个标签页，
//!    不再广播给所有连接。
//! 2. **身份回收**：标签页关闭、事件流断开一段时间后释放它的资产访问身份和
//!    会话读取配额，避免长期运行时只增不减。
//!
//! 断开有宽限期：网络闪断时前端会自动重连，宽限期内重连不会取消进行中的读取。

use crate::host::ServerEvent;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

/// 事件流断开后保留身份的时间；重连使用同一标识，不会重复创建身份。
pub const RELEASE_GRACE: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Entry {
    sender: Option<mpsc::UnboundedSender<ServerEvent>>,
    connections: usize,
    generation: u64,
}

#[derive(Default)]
pub struct ClientRegistry {
    inner: Mutex<HashMap<String, Entry>>,
}

impl ClientRegistry {
    /// 登记一条事件流连接，返回本次连接的代次。
    pub fn connect(&self, client: &str, sender: mpsc::UnboundedSender<ServerEvent>) -> u64 {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let entry = inner.entry(client.to_owned()).or_default();
        entry.connections += 1;
        entry.generation = entry.generation.wrapping_add(1);
        entry.sender = Some(sender);
        entry.generation
    }

    /// 注销一条连接；最后一个连接断开后安排延迟回收。
    pub fn disconnect(self: &Arc<Self>, client: &str, generation: u64) {
        let schedule = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            let Some(entry) = inner.get_mut(client) else {
                return;
            };
            entry.connections = entry.connections.saturating_sub(1);
            // 同一标识可能同时存在多条连接；代次不匹配说明已被更新的连接接管。
            if entry.generation != generation || entry.connections > 0 {
                return;
            }
            entry.sender = None;
            true
        };
        if !schedule {
            return;
        }
        let registry = Arc::clone(self);
        let client = client.to_owned();
        balancehub_core::runtime::spawn(async move {
            tokio::time::sleep(RELEASE_GRACE).await;
            registry.release_if_idle(&client, generation);
        });
    }

    /// 宽限期结束时仍无连接才真正回收；期间重连会让代次失效。
    fn release_if_idle(&self, client: &str, generation: u64) {
        let released = {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            match inner.get(client) {
                Some(entry) if entry.connections == 0 && entry.generation == generation => {
                    inner.remove(client);
                    true
                }
                _ => false,
            }
        };
        if released {
            balancehub_agent::release_window(client);
        }
    }

    /// 把事件送给该标签页当前的事件流；没有连接时返回 `false`。
    pub fn send(&self, client: &str, event: ServerEvent) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner
            .get(client)
            .and_then(|entry| entry.sender.as_ref())
            .is_some_and(|sender| sender.send(event).is_ok())
    }

    #[cfg(test)]
    fn connections(&self, client: &str) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(client)
            .map(|entry| entry.connections)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str) -> ServerEvent {
        ServerEvent {
            event: name.to_owned(),
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn progress_reaches_only_the_connected_client() {
        let registry = ClientRegistry::default();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        registry.connect("tab-a", sender);
        assert!(registry.send("tab-a", event("progress")));
        assert!(receiver.try_recv().is_ok(), "已连接的标签页应收到进度");
        assert!(
            !registry.send("tab-b", event("progress")),
            "其他标签页不应收到"
        );
    }

    #[tokio::test]
    async fn closing_one_of_two_connections_keeps_the_client_alive() {
        let registry = Arc::new(ClientRegistry::default());
        let (first, _first_rx) = mpsc::unbounded_channel();
        let (second, mut second_rx) = mpsc::unbounded_channel();
        let first_generation = registry.connect("tab", first);
        let second_generation = registry.connect("tab", second);
        registry.disconnect("tab", first_generation);
        assert_eq!(registry.connections("tab"), 1);
        assert!(
            registry.send("tab", event("progress")),
            "仍有连接时进度应继续送达"
        );
        assert!(second_rx.try_recv().is_ok());
        registry.disconnect("tab", second_generation);
        assert_eq!(registry.connections("tab"), 0);
        assert!(!registry.send("tab", event("progress")));
    }

    #[tokio::test]
    async fn a_stale_disconnect_never_releases_a_newer_connection() {
        let registry = Arc::new(ClientRegistry::default());
        let (old_sender, _old_rx) = mpsc::unbounded_channel();
        let old_generation = registry.connect("tab", old_sender);
        registry.disconnect("tab", old_generation);
        let (new_sender, mut new_rx) = mpsc::unbounded_channel();
        registry.connect("tab", new_sender);
        // 宽限期结束时旧连接早已被新连接取代。
        registry.release_if_idle("tab", old_generation);
        assert_eq!(registry.connections("tab"), 1);
        assert!(registry.send("tab", event("progress")));
        assert!(new_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn idle_client_is_released_only_after_the_grace_period_target() {
        let registry = Arc::new(ClientRegistry::default());
        let (sender, _rx) = mpsc::unbounded_channel();
        let generation = registry.connect("tab", sender);
        // 宽限期内重连会拿到新代次，旧代次不再回收。
        registry.release_if_idle("tab", generation);
        assert_eq!(registry.connections("tab"), 1, "仍有连接时不回收");
        registry.disconnect("tab", generation);
        registry.release_if_idle("tab", generation);
        assert_eq!(registry.connections("tab"), 0, "空闲且代次匹配时回收");
    }
}
