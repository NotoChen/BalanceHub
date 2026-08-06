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
    limits,
    models::{
        provider_duplicate_kind, AppData, AppDataTransferResult, AppSettings, AuthMode, AuthSource,
        Provider, ProviderDuplicateKind, ProviderInput, ProviderProtocol, ProviderSaveConflict,
        ProviderSaveOptions, ProviderSaveResult, ProviderStatus,
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

/// 异步 Provider 请求发出时的完整认证上下文。
///
/// 网络返回后必须再次与最新存储状态比对；只按 provider id 或 URL 判断，会让旧账号的
/// 身份、额度、签到、能力等结果写进用户刚切换的新账号。API Key 列表本身是派生元数据，
/// 不参与上下文判断，避免一次纯列表刷新无意义地取消同账号请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderRequestContext {
    provider_id: String,
    base_url: String,
    protocol: ProviderProtocol,
    auth_mode: AuthMode,
    auth_source: AuthSource,
    api_key: String,
    api_key_token_id: String,
    access_token: String,
    session_cookie: String,
    api_user: String,
    login_username: String,
    login_password: String,
    refresh_token: String,
    access_token_expires_at: Option<i64>,
}

impl ProviderRequestContext {
    pub(super) fn capture(provider: &Provider) -> Self {
        Self {
            provider_id: provider.identity.id.clone(),
            base_url: provider.identity.base_url.clone(),
            protocol: provider.identity.protocol,
            auth_mode: provider.auth.mode,
            auth_source: provider.auth.source,
            api_key: provider.auth.api_key.clone(),
            api_key_token_id: provider.auth.api_key_token_id.clone(),
            access_token: provider.auth.access_token.clone(),
            session_cookie: provider.auth.session_cookie.clone(),
            api_user: provider.auth.api_user.clone(),
            login_username: provider.auth.login_username.clone(),
            login_password: provider.auth.login_password.clone(),
            refresh_token: provider.auth.refresh_token.clone(),
            access_token_expires_at: provider.auth.access_token_expires_at,
        }
    }

    pub(super) fn matches(&self, provider: &Provider) -> bool {
        self.provider_id == provider.identity.id
            && self.base_url == provider.identity.base_url
            && self.protocol == provider.identity.protocol
            && self.auth_mode == provider.auth.mode
            && self.auth_source == provider.auth.source
            && self.api_key == provider.auth.api_key
            && self.api_key_token_id == provider.auth.api_key_token_id
            && self.access_token == provider.auth.access_token
            && self.session_cookie == provider.auth.session_cookie
            && self.api_user == provider.auth.api_user
            && self.login_username == provider.auth.login_username
            && self.login_password == provider.auth.login_password
            && self.refresh_token == provider.auth.refresh_token
            && self.access_token_expires_at == provider.auth.access_token_expires_at
    }
}

impl<'a> ProviderService<'a> {
    pub fn new(app: &'a AppHandle) -> Self {
        Self { app }
    }

    pub fn background(app: &'a AppHandle) -> Self {
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

    /// Clone the in-memory state without ever making an async worker wait on a
    /// synchronous lock. The state is normally tiny, but a provider can carry
    /// model, usage, and liveness history; a writer may also briefly hold the
    /// lock while committing a snapshot. Keep that wait on the blocking pool.
    pub(super) async fn snapshot_async(&self) -> Result<AppData, String> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || ProviderService { app: &app }.snapshot())
            .await
            .map_err(|err| format!("读取配置快照任务异常: {err}"))
    }

    /// Load the configuration with the same protection checks as the startup
    /// command, while keeping the synchronous state/configuration path off the
    /// async runtime.
    pub(super) async fn load_app_data_async(&self) -> Result<AppData, String> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || ProviderService { app: &app }.load_app_data())
            .await
            .map_err(|err| format!("加载应用配置任务异常: {err}"))?
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

    /// 在串行事务锁下基于克隆状态修改并原子落盘，落盘成功后才提交到内存。
    ///
    /// 闭包内严禁 `.await`：持锁跨越 await 会序列化所有网络请求并有死锁风险。
    /// 异步流程一律先用 [`snapshot`](Self::snapshot) 取数据、在锁外完成网络调用，
    /// 再用本方法把结果按 id 合并回最新状态。
    fn mutate<R>(&self, apply: impl FnOnce(&mut AppData) -> R) -> Result<R, String> {
        self.mutate_fallible(|data| Ok(apply(data)))
    }

    fn mutate_fallible<R>(
        &self,
        apply: impl FnOnce(&mut AppData) -> Result<R, String>,
    ) -> Result<R, String> {
        self.ensure_storage_ready()?;
        let state = self.app.state::<AppState>();
        let transaction = state
            .mutation_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut next_data = state
            .data
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let result = apply(&mut next_data)?;
        limits::normalize_app_data(&mut next_data);
        storage::save_app_data(self.app, &next_data)?;
        let previous_data = {
            let mut current = state.data.write().unwrap_or_else(|err| err.into_inner());
            std::mem::replace(&mut *current, next_data)
        };
        drop(transaction);
        drop(previous_data);
        Ok(result)
    }

    /// Run a synchronous storage transaction off the async worker.
    ///
    /// Network-facing service methods must not hold the runtime thread while
    /// waiting on the mutation gate or writing the JSON store. The app handle
    /// is owned by the blocking task so the borrowed service can return before
    /// the task starts.
    pub(super) async fn mutate_async<R, F>(&self, apply: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: FnOnce(&mut AppData) -> R + Send + 'static,
    {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || ProviderService { app: &app }.mutate(apply))
            .await
            .map_err(|err| format!("配置事务任务异常: {err}"))?
    }

    pub fn load_app_data(&self) -> Result<AppData, String> {
        if let Some(err) = self.storage_protection_error() {
            return Err(err);
        }
        Ok(self.snapshot())
    }

    pub fn save_provider(
        &self,
        input: ProviderInput,
        options: ProviderSaveOptions,
    ) -> Result<ProviderSaveResult, String> {
        self.mutate_fallible(|data| {
            let conflict = data.providers.iter().find_map(|provider| {
                if Some(provider.identity.id.as_str()) == input.id.as_deref() {
                    return None;
                }
                provider_duplicate_kind(provider, &input).map(|kind| ProviderSaveConflict {
                    kind,
                    existing_provider_id: provider.identity.id.clone(),
                    existing_provider_name: provider.identity.name.clone(),
                })
            });

            if let Some(conflict) = conflict {
                if conflict.kind == ProviderDuplicateKind::UrlDifferentApiKey
                    && options.merge_api_key_into_provider_id.as_deref()
                        == Some(conflict.existing_provider_id.as_str())
                {
                    let provider = data
                        .providers
                        .iter_mut()
                        .find(|provider| provider.identity.id == conflict.existing_provider_id)
                        .ok_or_else(|| "目标中转站已不存在，请重新保存".to_string())?;
                    provider.add_api_key(&input.auth.api_key)?;
                    return Ok(ProviderSaveResult {
                        providers: data.providers.clone(),
                        saved: true,
                        saved_provider_id: Some(conflict.existing_provider_id.clone()),
                        conflict: None,
                    });
                }

                if matches!(
                    conflict.kind,
                    ProviderDuplicateKind::Account | ProviderDuplicateKind::ApiKey
                ) && options.overwrite_provider_id.as_deref()
                    == Some(conflict.existing_provider_id.as_str())
                {
                    let mut overwrite_input = input.clone();
                    overwrite_input.id = Some(conflict.existing_provider_id.clone());
                    let provider = data
                        .providers
                        .iter_mut()
                        .find(|provider| provider.identity.id == conflict.existing_provider_id)
                        .ok_or_else(|| "目标中转站已不存在，请重新保存".to_string())?;
                    provider.apply_input(overwrite_input);
                    return Ok(ProviderSaveResult {
                        providers: data.providers.clone(),
                        saved: true,
                        saved_provider_id: Some(conflict.existing_provider_id.clone()),
                        conflict: None,
                    });
                }

                return Ok(ProviderSaveResult {
                    providers: data.providers.clone(),
                    saved: false,
                    saved_provider_id: None,
                    conflict: Some(conflict),
                });
            }
            let saved_provider_id = if let Some(id) = input.id.clone() {
                if let Some(provider) = data
                    .providers
                    .iter_mut()
                    .find(|provider| provider.identity.id == id)
                {
                    provider.apply_input(input);
                } else {
                    if data.providers.len() >= limits::MAX_PROVIDERS {
                        return Err(format!(
                            "中转站数量已达到上限（{} 个）",
                            limits::MAX_PROVIDERS
                        ));
                    }
                    data.providers.push(Provider::from_input(input, id.clone()));
                }
                Some(id)
            } else {
                if data.providers.len() >= limits::MAX_PROVIDERS {
                    return Err(format!(
                        "中转站数量已达到上限（{} 个）",
                        limits::MAX_PROVIDERS
                    ));
                }
                let id = format!("provider-{}", current_timestamp_millis());
                data.providers.push(Provider::from_input(input, id.clone()));
                Some(id)
            };
            Ok(ProviderSaveResult {
                providers: data.providers.clone(),
                saved: true,
                saved_provider_id,
                conflict: None,
            })
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

    pub fn save_settings(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        limits::normalize_settings(&mut settings);
        if settings.notification_channels.len() > limits::MAX_NOTIFICATION_CHANNELS {
            return Err(format!(
                "通知渠道数量超过上限（最多 {} 个）",
                limits::MAX_NOTIFICATION_CHANNELS
            ));
        }
        self.mutate(|data| {
            data.settings = settings;
            data.settings.clone()
        })
    }

    pub async fn mark_auto_check_in_failure(
        &self,
        provider: &Provider,
        message: String,
    ) -> Result<(), String> {
        let request_context = ProviderRequestContext::capture(provider);
        self.mutate_async(move |data| {
            if let Some(provider) = data
                .providers
                .iter_mut()
                .find(|provider| request_context.matches(provider))
            {
                provider.runtime.status = ProviderStatus::Error;
                provider.runtime.error_message = Some(message);
            }
        })
        .await
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
        let state = self.app.state::<AppState>();
        let transaction = state
            .mutation_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let data = storage::import_app_data(self.app, &source)?;
        let previous_data = {
            let mut current = state.data.write().unwrap_or_else(|err| err.into_inner());
            std::mem::replace(&mut *current, data.clone())
        };
        state.clear_load_error();
        drop(transaction);
        drop(previous_data);
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
