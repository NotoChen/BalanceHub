use crate::{
    adapters::newapi::NewApiAdapter,
    models::{AppData, AppSettings, Provider, RefreshResult},
    state::AppState,
};
use std::{collections::HashSet, sync::Arc};
use tauri::Manager;

use super::ProviderService;

impl<'a> ProviderService<'a> {
    pub async fn refresh_all(&self) -> Result<RefreshResult, String> {
        let state = self.app.state::<AppState>();
        let _gate = state.refresh_gate.lock().await;
        self.refresh_all_inner().await
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

    async fn refresh_all_inner(&self) -> Result<RefreshResult, String> {
        let data = self.snapshot();
        let settings = Arc::new(data.settings);
        let refreshed = refresh_providers_concurrently(settings, data.providers, |_| true).await?;
        let providers = self.mutate(|data| {
            apply_refreshed(data, refreshed);
            data.providers.clone()
        })?;
        Ok(RefreshResult { providers })
    }

    async fn refresh_by_ids_inner(&self, ids: Vec<String>) -> Result<RefreshResult, String> {
        let data = self.snapshot();
        let settings = Arc::new(data.settings);
        let id_set: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let refreshed = refresh_providers_concurrently(settings, data.providers, |provider| {
            id_set.contains(provider.identity.id.as_str())
        })
        .await?;
        let providers = self.mutate(|data| {
            apply_refreshed(data, refreshed);
            data.providers.clone()
        })?;
        Ok(RefreshResult { providers })
    }
}

/// 把刷新结果按 id 合并回最新状态。
///
/// 只合并 [`apply_refresh_owned_fields`] 列出的「刷新拥有」字段，而非整体替换结构体：
/// 刷新是后台常态操作，网络往返期间用户可能正在编辑凭据/名称/自动化配置并保存，
/// 整体替换会把这些并发编辑静默回滚。期间被删除的中转站不会重新插入，新增的不受影响。
pub(super) fn apply_refreshed(data: &mut AppData, refreshed: Vec<Provider>) {
    for next in refreshed {
        if let Some(slot) = data
            .providers
            .iter_mut()
            .find(|provider| provider.identity.id == next.identity.id)
        {
            apply_refresh_owned_fields(slot, next);
        }
    }
}

/// 刷新流程「拥有」的字段集合，须与 `newapi_quota::refresh_provider` 的写入面保持一致：
/// 配额全量、站点自报的展示信息（含站点名）、同步时间、运行状态与错误信息。
/// 签到成功后的静默刷新（check_in.rs）复用同一份清单，避免两处合并语义漂移。
pub(super) fn apply_refresh_owned_fields(provider: &mut Provider, refreshed: Provider) {
    provider.quota = refreshed.quota;
    provider.identity.name = refreshed.identity.name;
    provider.identity.display_name = refreshed.identity.display_name;
    provider.identity.username = refreshed.identity.username;
    provider.identity.user_id = refreshed.identity.user_id;
    provider.identity.site_logo = refreshed.identity.site_logo;
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
) -> Result<Vec<Provider>, String> {
    const MAX_CONCURRENT_REFRESH: usize = 6;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REFRESH));
    let mut handles = Vec::new();
    for provider in providers
        .into_iter()
        .filter(|provider| provider.runtime.enabled && should_refresh(provider))
    {
        let settings = Arc::clone(&settings);
        let semaphore = Arc::clone(&semaphore);
        handles.push(tauri::async_runtime::spawn(async move {
            // 信号量只在本函数生命周期内使用、从不 close，acquire 不会失败。
            let _permit = semaphore.acquire().await.expect("refresh semaphore closed");
            NewApiAdapter
                .refresh_provider(settings.as_ref(), &provider)
                .await
        }));
    }

    let mut refreshed = Vec::with_capacity(handles.len());
    for handle in handles {
        refreshed.push(handle.await.map_err(|err| format!("刷新任务异常: {err}"))?);
    }

    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderStatus};

    fn provider(id: &str) -> Provider {
        Provider::from_input(ProviderInput::default(), id.to_string())
    }

    #[test]
    fn apply_refreshed_merges_only_refresh_owned_fields() {
        let mut stored = provider("p1");
        stored.identity.name = "用户改名".to_string();
        stored.auth.api_key = "sk-user-edited".to_string();
        stored.automation.check_in_time = "09:30".to_string();

        // 刷新结果基于旧快照：站点报了新名字和新配额，但 api_key/签到时间是旧值。
        let mut refreshed = provider("p1");
        refreshed.identity.name = "站点自报名".to_string();
        refreshed.auth.api_key = "sk-old-snapshot".to_string();
        refreshed.automation.check_in_time = String::new();
        refreshed.quota.available = 42.0;
        refreshed.runtime.status = ProviderStatus::Ok;
        refreshed.automation.last_synced_at = Some("1700000000".to_string());

        let mut data = AppData::default();
        data.providers.push(stored);
        apply_refreshed(&mut data, vec![refreshed]);

        let merged = &data.providers[0];
        // 刷新拥有的字段：跟随刷新结果。
        assert_eq!(merged.identity.name, "站点自报名");
        assert_eq!(merged.quota.available, 42.0);
        assert_eq!(
            merged.automation.last_synced_at,
            Some("1700000000".to_string())
        );
        // 用户拥有的字段：并发编辑不被覆盖。
        assert_eq!(merged.auth.api_key, "sk-user-edited");
        assert_eq!(merged.automation.check_in_time, "09:30");
    }

    #[test]
    fn apply_refreshed_skips_providers_removed_meanwhile() {
        let mut data = AppData::default();
        data.providers.push(provider("kept"));

        apply_refreshed(&mut data, vec![provider("removed")]);

        assert_eq!(data.providers.len(), 1);
        assert_eq!(data.providers[0].identity.id, "kept");
    }
}
