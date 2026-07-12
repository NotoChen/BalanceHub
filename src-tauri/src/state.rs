use crate::models::AppData;
use std::sync::RwLock;

/// 内存中的应用状态。
///
/// 启动时从磁盘加载一次，之后所有读写都走内存，变更时再原子落盘。
/// 用 `RwLock` 串行化写入，避免并发命令（自动刷新、保存设置、签到……）
/// 各自「读磁盘 → 改 → 写磁盘」时互相覆盖导致丢更新。
#[derive(Default)]
pub struct AppState {
    pub data: RwLock<AppData>,
    /// 刷新互斥闸门：手动刷新与调度刷新并发时会对同一批站点重复打请求，
    /// 结果又按 id 合并互相覆盖。手动路径 `lock().await` 排队，调度器 `try_lock`
    /// 拿不到直接跳过本 tick（下个 tick 重新评估到期），两边都不会饿死。
    pub refresh_gate: tokio::sync::Mutex<()>,
    load_error: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(data: AppData, load_error: Option<String>) -> Self {
        Self {
            data: RwLock::new(data),
            refresh_gate: tokio::sync::Mutex::new(()),
            load_error: RwLock::new(load_error),
        }
    }

    pub fn load_error(&self) -> Option<String> {
        self.load_error
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn clear_load_error(&self) {
        *self
            .load_error
            .write()
            .unwrap_or_else(|err| err.into_inner()) = None;
    }
}
