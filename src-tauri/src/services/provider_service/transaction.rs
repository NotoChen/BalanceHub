use crate::{
    limits,
    models::{AppData, Provider},
    state::AppState,
    storage,
};
use tauri::Manager;

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

impl ProviderService<'_> {
    /// 读取内存状态的快照（克隆）。
    pub(super) fn snapshot(&self) -> AppData {
        self.app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Clone the in-memory state without making an async worker wait on a
    /// synchronous lock. A provider can carry sizeable model and history data,
    /// so that wait stays on the blocking pool.
    pub(super) async fn snapshot_async(&self) -> Result<AppData, String> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || ProviderService { app: &app }.snapshot())
            .await
            .map_err(|err| format!("读取配置快照任务异常: {err}"))
    }

    /// Load configuration with the same protection checks as the startup
    /// command while keeping the synchronous path off the async runtime.
    pub(in crate::services) async fn load_app_data_async(&self) -> Result<AppData, String> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || ProviderService { app: &app }.load_app_data())
            .await
            .map_err(|err| format!("加载应用配置任务异常: {err}"))?
    }

    pub(super) async fn providers_by_ids_async(
        &self,
        ids: &[String],
    ) -> Result<Vec<Provider>, String> {
        let app = self.app.clone();
        let ids = ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        tauri::async_runtime::spawn_blocking(move || {
            app.state::<AppState>()
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
        self.app.state::<AppState>().load_error().map(|err| {
            format!(
                "本地配置加载失败，为避免覆盖原配置，已暂停保存类操作。请先导入有效配置或修复 data.json 后重启。原始错误：{err}"
            )
        })
    }

    pub(super) fn ensure_storage_ready(&self) -> Result<(), String> {
        self.storage_protection_error().map_or(Ok(()), Err)
    }

    pub(super) fn mutate<R>(&self, apply: impl FnOnce(&mut AppData) -> R) -> Result<R, String> {
        self.mutate_decided(|data| Ok(MutationDecision::changed(apply(data))))
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
        let state = self.app.state::<AppState>();
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
        storage::save_app_data(self.app, &next_data)?;
        let revision = state.next_revision();
        next_data.revision = revision;
        for provider in &mut next_data.providers {
            provider.revision = revision;
        }
        let previous_data = {
            let mut current = state.data.write().unwrap_or_else(|err| err.into_inner());
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
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            ProviderService { app: &app }.mutate_decided(apply)
        })
        .await
        .map_err(|err| format!("配置事务任务异常: {err}"))?
    }

    pub fn load_app_data(&self) -> Result<AppData, String> {
        if let Some(err) = self.storage_protection_error() {
            return Err(err);
        }
        Ok(self.snapshot())
    }
}
