use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{
        AppData, AppSettings, AuthMode, Provider, ProviderBatchDetails, ProviderBatchOperation,
        ProviderBatchProgressEvent, ProviderBatchProgressItem, ProviderBatchStatus,
        ProviderProtocol, ProviderStatus, RefreshResult,
    },
    state::AppState,
};
use std::{collections::HashSet, sync::Arc};
use tauri::{ipc::Channel, Manager};

use super::{ProviderRequestContext, ProviderService};

pub(super) struct RefreshedProvider {
    pub(super) provider: Provider,
    pub(super) request_context: ProviderRequestContext,
}

impl<'a> ProviderService<'a> {
    pub async fn refresh_all_with_progress(
        &self,
        channel: Channel<ProviderBatchProgressEvent>,
    ) -> Result<RefreshResult, String> {
        let state = self.app.state::<AppState>();
        let _gate = state.refresh_gate.lock().await;
        self.refresh_all_inner(channel).await
    }

    pub async fn refresh_by_ids(&self, ids: Vec<String>) -> Result<RefreshResult, String> {
        let state = self.app.state::<AppState>();
        let _gate = state.refresh_gate.lock().await;
        self.refresh_by_ids_inner(ids).await
    }

    /// 调度器专用：刷新闸门被手动刷新占用时返回 `None` 跳过本轮，避免重复打请求。
    pub async fn try_refresh_by_ids(
        &self,
        ids: Vec<String>,
    ) -> Option<Result<RefreshResult, String>> {
        let state = self.app.state::<AppState>();
        let Ok(_gate) = state.refresh_gate.try_lock() else {
            return None;
        };
        Some(self.refresh_by_ids_inner(ids).await)
    }

    async fn refresh_all_inner(
        &self,
        progress: Channel<ProviderBatchProgressEvent>,
    ) -> Result<RefreshResult, String> {
        let data = self.snapshot_async().await?;
        let mut progress_items = data
            .providers
            .iter()
            .map(|provider| {
                if provider.runtime.enabled {
                    ProviderBatchProgressItem::pending(provider)
                } else {
                    ProviderBatchProgressItem::skipped(provider, "中转站已停用")
                }
            })
            .collect::<Vec<_>>();
        send_progress(
            Some(&progress),
            ProviderBatchProgressEvent::Started {
                operation: ProviderBatchOperation::Refresh,
                total: progress_items.len(),
                items: progress_items.clone(),
            },
        );
        let settings = Arc::new(data.settings);
        let refreshed = refresh_providers_concurrently(
            settings,
            data.providers,
            |_| true,
            Some(progress.clone()),
        )
        .await?;
        for result in &refreshed {
            let item = refresh_progress_item(&result.provider);
            if let Some(slot) = progress_items
                .iter_mut()
                .find(|item| item.provider_id == result.provider.identity.id)
            {
                *slot = item;
            }
        }
        let providers = self
            .mutate_async(move |data| {
                apply_refreshed(data, refreshed);
                data.providers.clone()
            })
            .await?;
        send_progress(
            Some(&progress),
            ProviderBatchProgressEvent::Completed {
                operation: ProviderBatchOperation::Refresh,
                summary: crate::models::ProviderBatchSummary::from_items(&progress_items),
            },
        );
        Ok(RefreshResult { providers })
    }

    async fn refresh_by_ids_inner(&self, ids: Vec<String>) -> Result<RefreshResult, String> {
        let data = self.snapshot_async().await?;
        let settings = Arc::new(data.settings);
        let id_set: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let refreshed = refresh_providers_concurrently(
            settings,
            data.providers,
            |provider| id_set.contains(provider.identity.id.as_str()),
            None,
        )
        .await?;
        let providers = self
            .mutate_async(move |data| {
                apply_refreshed(data, refreshed);
                data.providers.clone()
            })
            .await?;
        Ok(RefreshResult { providers })
    }
}

/// 把刷新结果按 id 合并回最新状态。
///
/// 只合并 [`apply_refresh_owned_fields`] 列出的「刷新拥有」字段，而非整体替换结构体：
/// 刷新是后台常态操作，网络往返期间用户可能正在编辑凭据/名称/自动化配置并保存，
/// 整体替换会把这些并发编辑静默回滚。期间被删除的中转站不会重新插入，新增的不受影响。
pub(super) fn apply_refreshed(data: &mut AppData, refreshed: Vec<RefreshedProvider>) {
    for refreshed in refreshed {
        let RefreshedProvider {
            provider: next,
            request_context,
        } = refreshed;
        if let Some(slot) = data
            .providers
            .iter_mut()
            .find(|provider| provider.identity.id == next.identity.id)
        {
            apply_refresh_owned_fields(slot, next, &request_context);
        }
    }
}

/// 刷新流程「拥有」的字段集合，须与具体协议适配器的刷新写入面保持一致：
/// 配额全量、站点自报的展示信息（含站点名）、同步时间、运行状态与错误信息。
/// 签到成功后的静默刷新（check_in.rs）复用同一份清单，避免两处合并语义漂移。
pub(super) fn apply_refresh_owned_fields(
    provider: &mut Provider,
    refreshed: Provider,
    request_context: &ProviderRequestContext,
) {
    // 身份、额度和凭据必须作为同一个原子结果写回。只校验 URL/协议会让同站点旧账号
    // 的刷新结果覆盖新账号；完整请求上下文变化时，本次网络结果全部作废。
    if !request_context.matches(provider) {
        return;
    }
    provider.quota = refreshed.quota;
    provider.identity.name = refreshed.identity.name;
    provider.identity.display_name = refreshed.identity.display_name;
    provider.identity.username = refreshed.identity.username;
    provider.identity.user_id = refreshed.identity.user_id;
    provider.identity.site_logo = refreshed.identity.site_logo;
    // 账号密码模式的登录会在刷新时产生可复用 Session。
    if matches!(provider.auth.mode, AuthMode::Password)
        && !refreshed.auth.session_cookie.trim().is_empty()
    {
        provider.auth.session_cookie = refreshed.auth.session_cookie;
        provider.auth.api_user = refreshed.auth.api_user;
    }

    // Sub2API account login yields a JWT instead of a Cookie.
    if matches!(provider.identity.protocol, ProviderProtocol::Sub2Api)
        && matches!(
            provider.auth.mode,
            AuthMode::Password | AuthMode::AccessToken
        )
    {
        // 刷新链断裂时适配器会显式清空三项令牌。这里必须连同空值一起写回，
        // 否则存储中的失效 refresh_token 会被永久保留，并在每轮刷新时重复提交。
        provider.auth.access_token = refreshed.auth.access_token;
        provider.auth.refresh_token = refreshed.auth.refresh_token;
        provider.auth.access_token_expires_at = refreshed.auth.access_token_expires_at;
    }

    // 用户名由 Cookie/访问令牌认证后的用户信息派生，只补空值，不改用户手动填写的账号。
    if request_context.login_username.trim().is_empty()
        && provider.auth.login_username.trim().is_empty()
        && !refreshed.auth.login_username.trim().is_empty()
    {
        provider.auth.login_username = refreshed.auth.login_username;
    }
    if !matches!(
        refreshed.runtime.status,
        crate::models::ProviderStatus::Error
    ) {
        provider.capabilities.available_models = refreshed.capabilities.available_models;
    }
    provider.automation.last_synced_at = refreshed.automation.last_synced_at;
    provider.runtime.status = refreshed.runtime.status;
    provider.runtime.error_message = refreshed.runtime.error_message;
}

/// 并发刷新中转站：启用且满足条件的并发拉取，返回值只包含实际刷新过的中转站。
/// 未刷新的不再原样返回 —— 它们没有新数据，参与合并只会用旧快照覆盖并发编辑。
///
/// 并发用信号量做滚动窗口（最多 6 个在飞），而非固定分批：分批会被批内最慢的
/// 一个（最长 20s 超时）拖住整批，滚动窗口下一个完成立刻补位。
pub(super) async fn refresh_providers_concurrently(
    settings: Arc<AppSettings>,
    providers: Vec<Provider>,
    should_refresh: impl Fn(&Provider) -> bool,
    progress: Option<Channel<ProviderBatchProgressEvent>>,
) -> Result<Vec<RefreshedProvider>, String> {
    const MAX_CONCURRENT_REFRESH: usize = 6;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REFRESH));
    let mut handles = Vec::new();
    for provider in providers
        .into_iter()
        .filter(|provider| provider.runtime.enabled && should_refresh(provider))
    {
        let provider_for_progress = provider.clone();
        send_progress(
            progress.as_ref(),
            ProviderBatchProgressEvent::ProviderStarted {
                operation: ProviderBatchOperation::Refresh,
                item: ProviderBatchProgressItem::new(
                    &provider_for_progress,
                    ProviderBatchStatus::Running,
                    refresh_started_message(&provider_for_progress),
                    None,
                ),
            },
        );
        let settings = Arc::clone(&settings);
        let semaphore = Arc::clone(&semaphore);
        let request_context = ProviderRequestContext::capture(&provider);
        let progress = progress.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            // 信号量只在本函数生命周期内使用、从不 close，acquire 不会失败。
            let _permit = semaphore.acquire().await.expect("refresh semaphore closed");
            let refreshed = ProtocolAdapter
                .refresh_provider(settings.as_ref(), &provider)
                .await;
            let status = if matches!(refreshed.runtime.status, ProviderStatus::Error) {
                ProviderBatchStatus::Failed
            } else {
                ProviderBatchStatus::Success
            };
            let message = refreshed.runtime.error_message.clone().unwrap_or_else(|| {
                if matches!(status, ProviderBatchStatus::Success) {
                    refresh_finished_message(&refreshed)
                } else {
                    "刷新失败".to_string()
                }
            });
            send_progress(
                progress.as_ref(),
                ProviderBatchProgressEvent::ProviderFinished {
                    operation: ProviderBatchOperation::Refresh,
                    item: ProviderBatchProgressItem::new(
                        &refreshed,
                        status,
                        message,
                        Some(ProviderBatchDetails::from_provider(&refreshed, None)),
                    ),
                },
            );
            RefreshedProvider {
                provider: refreshed,
                request_context,
            }
        }));
    }

    let mut refreshed = Vec::with_capacity(handles.len());
    for handle in handles {
        refreshed.push(handle.await.map_err(|err| format!("刷新任务异常: {err}"))?);
    }

    Ok(refreshed)
}

fn refresh_progress_item(provider: &Provider) -> ProviderBatchProgressItem {
    let status = if matches!(provider.runtime.status, ProviderStatus::Error) {
        ProviderBatchStatus::Failed
    } else {
        ProviderBatchStatus::Success
    };
    let message = provider.runtime.error_message.clone().unwrap_or_else(|| {
        if matches!(status, ProviderBatchStatus::Success) {
            refresh_finished_message(provider)
        } else {
            "刷新失败".to_string()
        }
    });
    ProviderBatchProgressItem::new(
        provider,
        status,
        message,
        Some(ProviderBatchDetails::from_provider(provider, None)),
    )
}

fn is_model_refresh_provider(provider: &Provider) -> bool {
    matches!(provider.identity.protocol, ProviderProtocol::Api)
        || matches!(provider.auth.mode, AuthMode::ApiKey)
}

fn refresh_started_message(provider: &Provider) -> String {
    if is_model_refresh_provider(provider) {
        "正在获取最新模型列表".to_string()
    } else {
        "正在刷新额度和账号能力".to_string()
    }
}

fn refresh_finished_message(provider: &Provider) -> String {
    if is_model_refresh_provider(provider) {
        format!(
            "模型列表已更新（{} 个）",
            provider.capabilities.available_models.len()
        )
    } else {
        "刷新完成".to_string()
    }
}

fn send_progress(
    channel: Option<&Channel<ProviderBatchProgressEvent>>,
    event: ProviderBatchProgressEvent,
) {
    if let Some(channel) = channel {
        let _ = channel.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderProtocol, ProviderStatus};

    fn provider(id: &str) -> Provider {
        Provider::from_input(ProviderInput::default(), id.to_string())
    }

    fn refreshed_provider(provider: &Provider) -> RefreshedProvider {
        RefreshedProvider {
            provider: provider.clone(),
            request_context: ProviderRequestContext::capture(provider),
        }
    }

    #[test]
    fn apply_refreshed_merges_only_refresh_owned_fields() {
        let stored = provider("p1");
        let request_context = ProviderRequestContext::capture(&stored);
        let mut stored = stored;
        stored.identity.name = "用户改名".to_string();
        stored.automation.check_in_time = "09:30".to_string();

        // 刷新结果基于旧快照：站点报了新名字和新配额，但 api_key/签到时间是旧值。
        let mut refreshed = provider("p1");
        refreshed.identity.name = "站点自报名".to_string();
        refreshed.automation.check_in_time = String::new();
        refreshed.quota.available = 42.0;
        refreshed.runtime.status = ProviderStatus::Ok;
        refreshed.automation.last_synced_at = Some("1700000000".to_string());

        let mut data = AppData::default();
        data.providers.push(stored);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context,
            }],
        );

        let merged = &data.providers[0];
        // 刷新拥有的字段：跟随刷新结果。
        assert_eq!(merged.identity.name, "站点自报名");
        assert_eq!(merged.quota.available, 42.0);
        assert_eq!(
            merged.automation.last_synced_at,
            Some("1700000000".to_string())
        );
        // 用户拥有的字段：并发编辑不被覆盖。
        assert_eq!(merged.automation.check_in_time, "09:30");
    }

    #[test]
    fn apply_refreshed_skips_providers_removed_meanwhile() {
        let mut data = AppData::default();
        data.providers.push(provider("kept"));

        let removed = provider("removed");
        apply_refreshed(&mut data, vec![refreshed_provider(&removed)]);

        assert_eq!(data.providers.len(), 1);
        assert_eq!(data.providers[0].identity.id, "kept");
    }

    #[test]
    fn refresh_backfills_empty_login_username() {
        let stored = provider("p1");
        let mut refreshed = stored.clone();
        refreshed.auth.login_username = "alice".to_string();

        let mut data = AppData::default();
        data.providers.push(stored);
        let request_context = ProviderRequestContext::capture(&data.providers[0]);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context,
            }],
        );

        assert_eq!(data.providers[0].auth.login_username, "alice");
    }

    #[test]
    fn refresh_does_not_overwrite_concurrent_login_username_edit() {
        let snapshot_provider = provider("p1");
        let mut current_provider = snapshot_provider.clone();
        current_provider.auth.login_username = "manual-account".to_string();
        let mut refreshed = snapshot_provider.clone();
        refreshed.auth.login_username = "alice".to_string();

        let mut data = AppData::default();
        data.providers.push(current_provider);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context: ProviderRequestContext::capture(&snapshot_provider),
            }],
        );

        assert_eq!(data.providers[0].auth.login_username, "manual-account");
    }

    #[test]
    fn sub2api_refresh_persists_refreshed_access_token() {
        let mut stored = provider("p1");
        stored.identity.protocol = ProviderProtocol::Sub2Api;
        stored.auth.mode = AuthMode::Password;
        stored.auth.login_username = "user@example.com".to_string();
        stored.auth.login_password = "password".to_string();
        stored.auth.access_token = "old-token".to_string();

        let mut refreshed = stored.clone();
        refreshed.auth.access_token = "new-token".to_string();

        let snapshot = ProviderRequestContext::capture(&stored);
        let mut data = AppData::default();
        data.providers.push(stored);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context: snapshot,
            }],
        );

        assert_eq!(data.providers[0].auth.access_token, "new-token");
    }

    #[test]
    fn sub2api_refresh_clears_broken_refresh_chain() {
        let mut stored = provider("p1");
        stored.identity.protocol = ProviderProtocol::Sub2Api;
        stored.auth.mode = AuthMode::AccessToken;
        stored.auth.access_token = "expired-access-token".to_string();
        stored.auth.refresh_token = "revoked-refresh-token".to_string();
        stored.auth.access_token_expires_at = Some(100);

        let request_context = ProviderRequestContext::capture(&stored);
        let mut refreshed = stored.clone();
        refreshed.auth.access_token.clear();
        refreshed.auth.refresh_token.clear();
        refreshed.auth.access_token_expires_at = None;
        refreshed.runtime.status = ProviderStatus::Error;

        let mut data = AppData::default();
        data.providers.push(stored);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context,
            }],
        );

        assert!(data.providers[0].auth.access_token.is_empty());
        assert!(data.providers[0].auth.refresh_token.is_empty());
        assert_eq!(data.providers[0].auth.access_token_expires_at, None);
    }

    #[test]
    fn refresh_ignores_result_when_protocol_changed() {
        let mut current = provider("p1");
        current.identity.protocol = ProviderProtocol::Sub2Api;
        current.quota.available = 99.0;

        let mut old = current.clone();
        old.identity.protocol = ProviderProtocol::NewApi;
        old.quota.available = 1.0;
        let snapshot = ProviderRequestContext::capture(&old);

        let mut data = AppData::default();
        data.providers.push(current);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: old,
                request_context: snapshot,
            }],
        );

        assert_eq!(data.providers[0].quota.available, 99.0);
    }

    #[test]
    fn refresh_ignores_identity_and_quota_when_auth_changed() {
        let old = provider("p1");
        let request_context = ProviderRequestContext::capture(&old);
        let mut current = old.clone();
        current.auth.api_key = "sk-new-account".to_string();
        current.identity.username = "new-user".to_string();
        current.quota.available = 99.0;

        let mut refreshed = old;
        refreshed.identity.username = "old-user".to_string();
        refreshed.quota.available = 1.0;

        let mut data = AppData::default();
        data.providers.push(current);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context,
            }],
        );

        assert_eq!(data.providers[0].identity.username, "new-user");
        assert_eq!(data.providers[0].quota.available, 99.0);
    }

    #[test]
    fn refresh_ignores_result_when_refresh_token_changed() {
        let mut old = provider("p1");
        old.identity.protocol = ProviderProtocol::Sub2Api;
        old.auth.mode = AuthMode::AccessToken;
        old.auth.access_token = "access-token".to_string();
        old.auth.refresh_token = "old-refresh".to_string();
        old.auth.access_token_expires_at = Some(100);
        let request_context = ProviderRequestContext::capture(&old);

        let mut current = old.clone();
        current.auth.refresh_token = "new-refresh".to_string();
        current.auth.access_token_expires_at = Some(200);
        current.quota.available = 99.0;

        let mut refreshed = old;
        refreshed.quota.available = 1.0;
        refreshed.auth.refresh_token = "rotated-old-refresh".to_string();

        let mut data = AppData::default();
        data.providers.push(current);
        apply_refreshed(
            &mut data,
            vec![RefreshedProvider {
                provider: refreshed,
                request_context,
            }],
        );

        assert_eq!(data.providers[0].auth.refresh_token, "new-refresh");
        assert_eq!(data.providers[0].auth.access_token_expires_at, Some(200));
        assert_eq!(data.providers[0].quota.available, 99.0);
    }
}
