mod account;
mod api_keys;
mod capabilities;
mod check_in;
mod codex_models;
mod credentials;
mod liveness;
mod quota;
mod refresh;
mod usage;

use crate::{
    models::{
        AppData, AppDataTransferResult, AppSettings, Provider, ProviderInput, ProviderStatus,
    },
    state::AppState,
    storage,
    util::unix_millis as current_timestamp_millis,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub struct ProviderService<'a> {
    app: &'a AppHandle,
}

impl<'a> ProviderService<'a> {
    pub fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    /// 读取内存状态的快照（克隆）。
    fn snapshot(&self) -> AppData {
        self.app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn storage_protection_error(&self) -> Option<String> {
        self.app.state::<AppState>().load_error().map(|err| {
            format!(
                "本地配置加载失败，为避免覆盖原配置，已暂停保存类操作。请先导入有效配置或修复 data.json 后重启。原始错误：{err}"
            )
        })
    }

    fn ensure_storage_ready(&self) -> Result<(), String> {
        self.storage_protection_error().map_or(Ok(()), Err)
    }

    /// 在写锁下修改内存状态并原子落盘。
    ///
    /// 闭包内严禁 `.await`：持锁跨越 await 会序列化所有网络请求并有死锁风险。
    /// 异步流程一律先用 [`snapshot`](Self::snapshot) 取数据、在锁外完成网络调用，
    /// 再用本方法把结果按 id 合并回最新状态。
    fn mutate<R>(&self, apply: impl FnOnce(&mut AppData) -> R) -> Result<R, String> {
        self.ensure_storage_ready()?;
        let state = self.app.state::<AppState>();
        let mut guard = state.data.write().unwrap_or_else(|err| err.into_inner());
        let result = apply(&mut guard);
        storage::save_app_data(self.app, &guard)?;
        Ok(result)
    }

    pub fn load_app_data(&self) -> Result<AppData, String> {
        if let Some(err) = self.storage_protection_error() {
            return Err(err);
        }
        Ok(self.snapshot())
    }

    pub fn save_provider(&self, input: ProviderInput) -> Result<Vec<Provider>, String> {
        self.mutate(|data| {
            if let Some(id) = input.id.clone() {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| provider.identity.id == id)
                {
                    provider.apply_input(input);
                } else {
                    data.providers.push(Provider::from_input(input, id));
                }
            } else {
                let id = format!("provider-{}", current_timestamp_millis());
                data.providers.push(Provider::from_input(input, id));
            }
            data.providers.clone()
        })
    }

    pub fn remove_provider(&self, id: String) -> Result<Vec<Provider>, String> {
        self.mutate(|data| {
            data.providers.retain(|provider| provider.identity.id != id);
            data.providers.clone()
        })
    }

    pub fn reorder_providers(&self, ids: Vec<String>) -> Result<Vec<Provider>, String> {
        self.mutate(|data| {
            let order: std::collections::HashMap<&str, usize> = ids
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            let fallback = ids.len();
            data.providers.sort_by_key(|provider| {
                order
                    .get(provider.identity.id.as_str())
                    .copied()
                    .unwrap_or(fallback)
            });
            data.providers.clone()
        })
    }

    pub fn save_settings(&self, settings: AppSettings) -> Result<AppSettings, String> {
        self.mutate(|data| {
            data.settings = settings;
            data.settings.clone()
        })
    }

    pub fn mark_auto_check_in_failure(&self, id: String, message: String) -> Result<(), String> {
        self.mutate(|data| {
            if let Some(provider) = data
                .providers
                .iter_mut()
                .find(|provider| provider.identity.id == id)
            {
                provider.runtime.status = ProviderStatus::Error;
                provider.runtime.error_message = Some(message);
            }
        })
    }

    pub fn export_app_data(&self, path: String) -> Result<AppDataTransferResult, String> {
        self.ensure_storage_ready()?;
        let target = PathBuf::from(path);
        let data = self.snapshot();
        storage::export_app_data(&target, &data)?;
        Ok(AppDataTransferResult {
            path: target.display().to_string(),
            schema_version: data.schema_version,
            provider_count: data.providers.len(),
        })
    }

    pub fn import_app_data(
        &self,
        path: String,
    ) -> Result<(AppData, AppDataTransferResult), String> {
        let source = PathBuf::from(path);
        let data = storage::import_app_data(self.app, &source)?;
        let state = self.app.state::<AppState>();
        let mut guard = state.data.write().unwrap_or_else(|err| err.into_inner());
        *guard = data.clone();
        state.clear_load_error();
        let result = AppDataTransferResult {
            path: source.display().to_string(),
            schema_version: data.schema_version,
            provider_count: data.providers.len(),
        };
        Ok((data, result))
    }
}

fn find_provider(data: &AppData, id: &str) -> Result<Provider, String> {
    data.providers
        .iter()
        .find(|provider| provider.identity.id == id)
        .cloned()
        .ok_or_else(|| "中转站不存在".to_string())
}
