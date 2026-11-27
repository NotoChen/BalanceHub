//! 进程内配置状态。
//!
//! 启动时从磁盘加载一次，之后所有读写都走内存，变更时再原子落盘。
//! 用 `RwLock` 串行化写入，避免并发任务（自动刷新、保存设置、签到……）
//! 各自「读磁盘 → 改 → 写磁盘」时互相覆盖导致丢更新。

use crate::{models::AppData, services::model_catalog::ModelCatalogService};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
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
    /// 每次刷新批次开始时递增。取消标记只记录「取消发生在第几批」，
    /// 新批次拿到更大的编号后不会继承上一批的取消，避免一次点击永久
    /// 关掉后续所有卡片刷新。
    refresh_generation: AtomicU64,
    refresh_cancel_generation: AtomicU64,
    model_catalog: OnceLock<Arc<ModelCatalogService>>,
    revision: AtomicU64,
    load_error: RwLock<Option<String>>,
    /// 启动时读取失败只意味着内存里是一份默认配置，用户修好 config.json 后
    /// 可以「重新读取」。运行期 `block_storage`（例如云同步应用失败留下
    /// 事务日志）必须重启交给启动恢复处理，不能就地重读。
    retry_load: AtomicBool,
}

impl CoreState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(mut data: AppData, load_error: Option<String>) -> Self {
        // revision 只服务于进程内 IPC 排序，但浏览器版前端会跨后端重启一直
        // 开着：从固定 1 重新计数会让重启后的快照和定向视图都小于前端记下的
        // 高水位，被当成晚到的旧响应丢掉。起始值改用墙钟毫秒，重启后的版本号
        // 自然大于上一进程发出去的任何值。
        let base = crate::util::unix_millis().min(u64::MAX as u128) as u64;
        data.revision = base;
        for provider in &mut data.providers {
            provider.revision = base;
        }
        let retry_load = load_error.is_some();
        Self {
            data: RwLock::new(data),
            refresh_gate: tokio::sync::Mutex::new(()),
            mutation_gate: Mutex::new(()),
            refresh_generation: AtomicU64::new(0),
            refresh_cancel_generation: AtomicU64::new(0),
            model_catalog: OnceLock::new(),
            revision: AtomicU64::new(base),
            load_error: RwLock::new(load_error),
            retry_load: AtomicBool::new(retry_load),
        }
    }

    /// 启动恢复流程本身失败（例如云同步事务回滚未完成）时，内存同样是
    /// 默认配置，但磁盘上还有待恢复的事务状态：只能重启重走恢复，不能
    /// 通过「重新读取」或导入绕过。
    pub fn with_storage_block(error: String) -> Self {
        let state = Self::with_load_error(AppData::default(), Some(error));
        state.retry_load.store(false, Ordering::SeqCst);
        state
    }

    /// 开始一个刷新批次，返回只属于这一批的编号。
    pub fn begin_refresh_batch(&self) -> u64 {
        self.refresh_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 取消当前正在执行的批次；没有批次运行时标记落在旧编号上，
    /// 下一批拿到新编号后自然不受影响。
    pub fn request_refresh_cancel(&self) {
        let generation = self.refresh_generation.load(Ordering::SeqCst);
        self.refresh_cancel_generation
            .store(generation, Ordering::SeqCst);
    }

    /// 只取消用户点下的那一批。手动刷新可能先登记批次再排队等闸门，
    /// 此时占用闸门的自动刷新属于更早的批次，不能因为这次取消一起停掉。
    pub fn refresh_cancelled(&self, generation: u64) -> bool {
        self.refresh_cancel_generation.load(Ordering::SeqCst) == generation
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
        self.retry_load.store(false, Ordering::SeqCst);
    }

    /// 启动读取失败允许用户重试；运行期阻塞必须重启恢复，所以这里同时
    /// 关掉重试开关。
    pub fn block_storage(&self, error: String) {
        *self
            .load_error
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(error);
        self.retry_load.store(false, Ordering::SeqCst);
    }

    /// 重新读取仍然失败时只更新错误文案，保留「重新读取」入口——用户可能
    /// 正在外部修复同一份 config.json。
    pub fn update_load_error(&self, error: String) {
        *self
            .load_error
            .write()
            .unwrap_or_else(|err| err.into_inner()) = Some(error);
    }

    pub fn can_retry_load(&self) -> bool {
        self.retry_load.load(Ordering::SeqCst)
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
        let first = state.next_revision();
        assert!(first > 1, "版本号从墙钟时间起步，不再从 1 开始");
        assert_eq!(state.next_revision(), first + 1);
        assert_eq!(state.current_revision(), first + 1);
    }

    #[test]
    fn a_restart_does_not_restart_revision_counting() {
        crate::host::testing::install();
        let first = CoreState::new(AppData::default());
        let high = first.next_revision();
        // 真实重启至少跨越一个毫秒；这里等一小会儿复现同样的墙钟前进。
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = CoreState::new(AppData::default());
        assert!(second.current_revision() >= high);
        assert!(second.next_revision() > high);
    }

    #[test]
    fn refresh_cancel_only_applies_to_the_batch_it_was_requested_for() {
        crate::host::testing::install();
        let state = CoreState::new(AppData::default());
        let first = state.begin_refresh_batch();
        assert!(!state.refresh_cancelled(first));
        state.request_refresh_cancel();
        assert!(state.refresh_cancelled(first));

        // 批次结束后才到达的取消不能留给下一批；否则之后每次卡片刷新
        // 都会被标记为已取消。
        let second = state.begin_refresh_batch();
        assert!(!state.refresh_cancelled(second));

        state.request_refresh_cancel();
        assert!(state.refresh_cancelled(second));
        let third = state.begin_refresh_batch();
        assert!(!state.refresh_cancelled(third));
    }

    #[test]
    fn only_a_startup_load_failure_can_be_retried_in_place() {
        crate::host::testing::install();
        let startup = CoreState::with_load_error(AppData::default(), Some("解析失败".into()));
        assert!(startup.can_retry_load());

        // 运行期阻塞（例如云同步应用失败留下事务日志）必须重启恢复；
        // 就地重读会绕过启动时的恢复流程。
        startup.block_storage("云同步应用失败".into());
        assert!(startup.load_error().is_some());
        assert!(!startup.can_retry_load());

        // 启动恢复流程自己失败时磁盘上仍有待恢复的事务，导入和就地重读
        // 都会绕过恢复流程。
        let blocked = CoreState::with_storage_block("云同步回滚未完成".into());
        assert!(blocked.load_error().is_some());
        assert!(!blocked.can_retry_load());

        let recovered = CoreState::with_load_error(AppData::default(), Some("解析失败".into()));
        recovered.update_load_error("仍然解析失败".into());
        assert!(recovered.can_retry_load());
        recovered.clear_load_error();
        assert!(recovered.load_error().is_none());
        assert!(!recovered.can_retry_load());
    }

    #[test]
    fn cancelling_a_queued_batch_does_not_stop_the_running_one() {
        crate::host::testing::install();
        let state = CoreState::new(AppData::default());
        let running = state.begin_refresh_batch();
        let queued = state.begin_refresh_batch();
        assert!(queued > running);
        // 手动刷新排队期间点取消：只命中排队中的那一批，正在占闸门的
        // 自动刷新继续收尾，不会被一起停掉。
        state.request_refresh_cancel();
        assert!(state.refresh_cancelled(queued));
        assert!(!state.refresh_cancelled(running));
    }
}
