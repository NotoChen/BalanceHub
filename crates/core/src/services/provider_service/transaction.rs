use crate::{
    limits,
    models::{AppData, Provider},
    storage,
};

use super::ProviderService;

/// A storage transaction must say whether it actually changed persisted data.
///
/// `Unchanged` prevents rejected stale results, duplicate-save conflicts, and
/// read-only authenticated requests from rewriting the JSON store or advancing
/// the process-local IPC revision.
pub(super) enum MutationDecision<R> {
    Changed(R),
    Unchanged(R),
}

impl<R> MutationDecision<R> {
    pub(super) fn changed(value: R) -> Self {
        Self::Changed(value)
    }

    pub(super) fn unchanged(value: R) -> Self {
        Self::Unchanged(value)
    }
}

impl ProviderService {
    /// 读取内存状态的快照（克隆）。
    pub(super) fn snapshot(&self) -> AppData {
        crate::host::core_state()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Clone the in-memory state without making an async worker wait on a
    /// synchronous lock. A provider can carry sizeable model and history data,
    /// so that wait stays on the blocking pool.
    pub(super) async fn snapshot_async(&self) -> Result<AppData, String> {
        crate::runtime::spawn_blocking(move || ProviderService.snapshot())
            .await
            .map_err(|err| format!("读取配置快照任务异常: {err}"))
    }

    /// Load configuration with the same protection checks as the startup
    /// command while keeping the synchronous path off the async runtime.
    pub(in crate::services) async fn load_app_data_async(&self) -> Result<AppData, String> {
        crate::runtime::spawn_blocking(move || ProviderService.load_app_data())
            .await
            .map_err(|err| format!("加载应用配置任务异常: {err}"))?
    }

    pub(super) async fn providers_by_ids_async(
        &self,
        ids: &[String],
    ) -> Result<Vec<Provider>, String> {
        let ids = ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        crate::runtime::spawn_blocking(move || {
            crate::host::core_state()
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .providers
                .iter()
                .filter(|provider| ids.contains(&provider.identity.id))
                .cloned()
                .collect()
        })
        .await
        .map_err(|err| format!("读取中转站快照任务异常: {err}"))
    }

    pub(super) fn storage_protection_error(&self) -> Option<String> {
        let state = crate::host::core_state();
        state.load_error().map(|err| {
            // 启动解析失败可以靠导入或修复文件恢复；运行期阻塞和启动时的
            // 云同步回滚失败都还留着磁盘上的事务状态，只能重启。
            let recovery = if state.can_retry_load() {
                "请先导入有效配置或修复 config.json 后重试"
            } else {
                "请重启应用完成恢复"
            };
            format!(
                "本地配置加载失败，为避免覆盖原配置，已暂停保存类操作。{recovery}。原始错误：{err}"
            )
        })
    }

    pub(super) fn ensure_storage_ready(&self) -> Result<(), String> {
        self.storage_protection_error().map_or(Ok(()), Err)
    }

    /// 启动期解析失败允许用导入恢复；存储被阻塞时磁盘上还有未完成的
    /// 云同步事务，导入会与恢复流程冲突，必须先重启。
    pub(super) fn ensure_import_ready(&self) -> Result<(), String> {
        let state = crate::host::core_state();
        if state.load_error().is_some() && !state.can_retry_load() {
            return Err("本地配置处于保护状态，请重启应用完成恢复后再导入配置".to_string());
        }
        Ok(())
    }

    pub(super) fn mutate<R>(&self, apply: impl FnOnce(&mut AppData) -> R) -> Result<R, String> {
        self.mutate_decided(|data| Ok(MutationDecision::changed(apply(data))))
    }

    /// 开发版演示卡片：在串行事务里把演示卡片并入当前配置，真实站点原样保留。
    #[cfg(debug_assertions)]
    pub fn merge_dev_sample(&self) -> Result<usize, String> {
        self.mutate_decided(|data| {
            Ok(MutationDecision::changed(crate::dev_sample::merge_into(
                data,
            )))
        })
    }

    /// 在串行事务锁下基于克隆状态修改并原子落盘，落盘成功后才提交到内存。
    ///
    /// 闭包内严禁 `.await`：持锁跨越 await 会序列化所有网络请求并有死锁风险。
    /// 异步流程先取快照，在锁外完成网络调用，再把结果按 id 合并回最新状态。
    pub(super) fn mutate_decided<R>(
        &self,
        apply: impl FnOnce(&mut AppData) -> Result<MutationDecision<R>, String>,
    ) -> Result<R, String> {
        self.mutate_decided_with_revision(apply)
            .map(|(result, _revision)| result)
    }

    pub(super) fn mutate_decided_with_revision<R>(
        &self,
        apply: impl FnOnce(&mut AppData) -> Result<MutationDecision<R>, String>,
    ) -> Result<(R, u64), String> {
        self.ensure_storage_ready()?;
        let state = crate::host::core_state();
        let _transaction = state
            .mutation_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut next_data = state
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let result = match apply(&mut next_data)? {
            MutationDecision::Changed(result) => result,
            MutationDecision::Unchanged(result) => {
                return Ok((result, state.current_revision()));
            }
        };
        limits::normalize_app_data(&mut next_data);
        storage::save_app_data(&next_data)?;
        let revision = state.next_revision();
        next_data.revision = revision;
        for provider in &mut next_data.providers {
            provider.revision = revision;
        }
        let previous_data = {
            let mut current = state.data.write().unwrap_or_else(|err| err.into_inner());
            crate::host::app_data_changed(&current, &next_data);
            std::mem::replace(&mut *current, next_data)
        };
        drop(previous_data);
        Ok((result, revision))
    }

    /// Run a synchronous storage transaction off the async worker.
    pub(super) async fn mutate_decided_async<R, F>(&self, apply: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: FnOnce(&mut AppData) -> Result<MutationDecision<R>, String> + Send + 'static,
    {
        crate::runtime::spawn_blocking(move || ProviderService.mutate_decided(apply))
            .await
            .map_err(|err| format!("配置事务任务异常: {err}"))?
    }

    pub fn load_app_data(&self) -> Result<AppData, String> {
        let state = crate::host::core_state();
        if state.load_error().is_none() {
            return Ok(self.snapshot());
        }
        if !state.can_retry_load() {
            return Err(self
                .storage_protection_error()
                .unwrap_or_else(|| "本地配置不可用，请重启应用".to_string()));
        }
        // 启动读取失败时内存里只有默认配置。用户可能已在外部修复或替换
        // config.json，「重新读取」必须真正回到磁盘，而不是重复抛出启动时的
        // 旧错误。运行期 `block_storage` 已关闭重试，会走上面的快速失败。
        let _transaction = state
            .mutation_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if state.load_error().is_none() {
            return Ok(self.snapshot());
        }
        match storage::load_app_data() {
            Ok(data) => Ok(self.install_loaded_app_data(data)),
            Err(error) => {
                // 保留重试入口：用户可能正在外部修复同一份 config.json。
                state.update_load_error(error.clone());
                Err(error)
            }
        }
    }
}
