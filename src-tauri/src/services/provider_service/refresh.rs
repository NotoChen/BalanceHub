use crate::{
    adapters::newapi::NewApiAdapter,
    models::{AppData, AppSettings, Provider, RefreshResult},
};
use std::{collections::HashSet, sync::Arc};

use super::ProviderService;

impl<'a> ProviderService<'a> {
    pub async fn refresh_all(&self) -> Result<RefreshResult, String> {
        let data = self.snapshot();
        let settings = Arc::new(data.settings);
        let refreshed = refresh_providers_concurrently(settings, data.providers, |_| true).await?;
        let providers = self.mutate(|data| {
            apply_refreshed(data, refreshed);
            data.providers.clone()
        })?;
        Ok(RefreshResult { providers })
    }

    pub async fn refresh_by_ids(&self, ids: Vec<String>) -> Result<RefreshResult, String> {
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

/// 把刷新结果按 id 合并回最新状态（只更新已存在的中转站，
/// 不会重新插入期间被删除的、也不会覆盖期间新增的）。
pub(super) fn apply_refreshed(data: &mut AppData, refreshed: Vec<Provider>) {
    for next in refreshed {
        if let Some(slot) = data
            .providers
            .iter_mut()
            .find(|provider| provider.identity.id == next.identity.id)
        {
            *slot = next;
        }
    }
}

/// 并发刷新中转站：启用且满足条件的并发拉取，其余原样返回；结果按原顺序返回。
pub(super) async fn refresh_providers_concurrently(
    settings: Arc<AppSettings>,
    providers: Vec<Provider>,
    should_refresh: impl Fn(&Provider) -> bool,
) -> Result<Vec<Provider>, String> {
    const MAX_CONCURRENT_REFRESH: usize = 6;

    let provider_count = providers.len();
    let mut refreshed = Vec::with_capacity(provider_count);
    let mut providers = providers.into_iter();
    loop {
        let batch = providers
            .by_ref()
            .take(MAX_CONCURRENT_REFRESH)
            .collect::<Vec<_>>();
        if batch.is_empty() {
            break;
        }

        let mut handles = Vec::with_capacity(batch.len());
        for provider in batch {
            let refresh = provider.runtime.enabled && should_refresh(&provider);
            let settings = Arc::clone(&settings);
            handles.push(tauri::async_runtime::spawn(async move {
                if refresh {
                    NewApiAdapter
                        .refresh_provider(settings.as_ref(), &provider)
                        .await
                } else {
                    provider
                }
            }));
        }

        for handle in handles {
            refreshed.push(handle.await.map_err(|err| format!("刷新任务异常: {err}"))?);
        }
    }

    Ok(refreshed)
}
