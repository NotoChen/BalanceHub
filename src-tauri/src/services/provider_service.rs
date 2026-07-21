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
mod workspaces;

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

    /// 在写锁下基于克隆状态修改并原子落盘，落盘成功后才提交到内存。
    ///
    /// 闭包内严禁 `.await`：持锁跨越 await 会序列化所有网络请求并有死锁风险。
    /// 异步流程一律先用 [`snapshot`](Self::snapshot) 取数据、在锁外完成网络调用，
    /// 再用本方法把结果按 id 合并回最新状态。
    fn mutate<R>(&self, apply: impl FnOnce(&mut AppData) -> R) -> Result<R, String> {
        self.ensure_storage_ready()?;
        let state = self.app.state::<AppState>();
        let mut guard = state.data.write().unwrap_or_else(|err| err.into_inner());
        let mut next_data = guard.clone();
        let result = apply(&mut next_data);
        storage::save_app_data(self.app, &next_data)?;
        *guard = next_data;
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
            data.temporary_cli_preferences
                .retain(|preference| preference.provider_id != id);
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
            data.settings = merge_saved_settings(&data.settings, settings);
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

/// 合并前端整份提交的设置：后端拥有的字段以存量为准。
///
/// 测活授权时间戳只能经 acknowledge/revoke 命令变更 —— 设置抽屉的草稿可能是在
/// 授权动作之前克隆的旧副本，直接放行会把刚写入的授权静默抹掉（或把已重置的授权复活）。
fn merge_saved_settings(current: &AppSettings, mut incoming: AppSettings) -> AppSettings {
    incoming.liveness_consent_accepted_at = current.liveness_consent_accepted_at.clone();
    incoming
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_settings_preserves_backend_owned_consent() {
        let current = AppSettings {
            liveness_consent_accepted_at: Some("1700000000000".to_string()),
            ..AppSettings::default()
        };
        let incoming = AppSettings {
            liveness_consent_accepted_at: None,
            check_in_time: "09:00".to_string(),
            ..AppSettings::default()
        };

        let merged = merge_saved_settings(&current, incoming);

        // 旧草稿带不走已写入的授权……
        assert_eq!(
            merged.liveness_consent_accepted_at,
            Some("1700000000000".to_string())
        );
        // ……但用户可编辑字段照常生效。
        assert_eq!(merged.check_in_time, "09:00");
    }

    #[test]
    fn save_settings_cannot_resurrect_revoked_consent() {
        let current = AppSettings::default();
        let incoming = AppSettings {
            liveness_consent_accepted_at: Some("1700000000000".to_string()),
            ..AppSettings::default()
        };

        let merged = merge_saved_settings(&current, incoming);

        assert_eq!(merged.liveness_consent_accepted_at, None);
    }
}
