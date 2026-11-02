//! 进程内配置状态。
//!
//! 启动时从磁盘加载一次，之后所有读写都走内存，变更时再原子落盘。
//! 用 `RwLock` 串行化写入，避免并发任务（自动刷新、保存设置、签到……）
//! 各自「读磁盘 → 改 → 写磁盘」时互相覆盖导致丢更新。

use crate::{models::AppData, services::model_catalog::ModelCatalogService};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock, RwLock,
};

pub struct CoreState {
    pub data: RwLock<AppData>,
    /// 网络认证互斥闸门：手动刷新、调度刷新以及可能轮换 Sub2API
    /// `refresh_token` 的交互操作不能同时运行。手动路径 `lock().await` 排队，
    /// 调度器 `try_lock` 拿不到直接跳过本 tick（下个 tick 重新评估到期），
    /// 两边都不会饿死。
    pub refresh_gate: tokio::sync::Mutex<()>,
    /// 串行化“基于快照修改并落盘”的事务。持有该锁时不会长期占用 `data` 写锁，
    /// 因此 JSON 序列化和磁盘替换期间读取方仍可继续读取上一份完整状态。
    pub mutation_gate: Mutex<()>,
    model_catalog: OnceLock<Arc<ModelCatalogService>>,
    revision: AtomicU64,
    load_error: RwLock<Option<String>>,
}

impl CoreState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(mut data: AppData, load_error: Option<String>) -> Self {
        data.revision = 1;
        for provider in &mut data.providers {
            provider.revision = 1;
        }
        Self {
            data: RwLock::new(data),
            refresh_gate: tokio::sync::Mutex::new(()),
            mutation_gate: Mutex::new(()),
            model_catalog: OnceLock::new(),
            revision: AtomicU64::new(1),
            load_error: RwLock::new(load_error),
        }
    }

    pub fn model_catalog(&self) -> Result<Arc<ModelCatalogService>, String> {
        let path = crate::host::data_dir()
            .map_err(|_| "模型资料缓存目录不可用")?
            .join("model-catalog.json");
        Ok(Arc::clone(
            self.model_catalog
                .get_or_init(|| Arc::new(ModelCatalogService::new(path))),
        ))
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

    pub fn block_storage(&self, error: String) {
        *self
            .load_error
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(error);
    }

    pub fn next_revision(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_are_monotonic_within_the_process() {
        crate::host::testing::install();
        let state = CoreState::new(AppData::default());
        assert_eq!(state.next_revision(), 2);
        assert_eq!(state.next_revision(), 3);
        assert_eq!(state.current_revision(), 3);
    }
}
