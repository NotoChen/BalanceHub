use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{
        AuthMode, ProviderProtocol, ProviderRequestLogsQuery, ProviderRequestLogsResult,
        ProviderStatus, ProviderUsageModelStat, ProviderUsageOverview, ProviderUsageSourceError,
        ProviderUsageSummary,
    },
};
use tauri::Manager;

use super::{find_provider, ProviderRequestContext, ProviderService};

/// 汇总视图只读取账号令牌能回答用量的站点。API Key 卡片只有 Key 自身额度，
/// 强制登录的站点状态也会是 Error，两者都先排除，避免把“本来就读不到”
/// 当作读取失败列给用户。
fn overview_target(provider: &crate::models::Provider) -> bool {
    matches!(
        provider.identity.protocol,
        ProviderProtocol::NewApi | ProviderProtocol::Sub2Api
    ) && !matches!(provider.runtime.status, ProviderStatus::Error)
        && !matches!(provider.auth.mode, AuthMode::ApiKey)
}

/// 汇总视图只展示排在前面的模型，排行铺满屏幕没有意义。
const MODEL_LIMIT: usize = 10;

impl<'a> ProviderService<'a> {
    pub async fn usage_summary(
        &self,
        id: String,
        period: String,
    ) -> Result<ProviderUsageSummary, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        // 用量趋势与请求日志都是站点级统计，API Key 卡片只有 Key 自身额度，
        // 与汇总视图同一判断，直接说明原因而不是先登录再报错。
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("用量趋势按站点统计，API Key 卡片请在卡片上查看 Key 额度".to_string());
        }
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let operation = ProtocolAdapter
            .usage_summary(&data.settings, &provider, &period)
            .await?;
        self.persist_operation_credentials(
            &ProviderRequestContext::capture(&provider),
            &operation.credentials,
        )
        .await?
        .ok_or_else(|| "本地配置已变更，本次用量结果已忽略".to_string())?;
        Ok(operation.value)
    }

    /// 汇总视图（不带中转站）把能回答用量的站点合成一份结果。逐个在后台并发读取：
    /// 一个站点慢或失败不该拖住其余站点，失败原因随汇总结果返回。
    pub async fn usage_summary_overview(
        &self,
        period: &str,
    ) -> Result<ProviderUsageOverview, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        // 只读取确实能回答用量的站点：协议支持之外，还要求卡片当前可用且认证方式
        // 不是 API Key（站点级用量要账号令牌）。这类站点本来就不会打开用量面板，
        // 列进汇总只会把“还没填写账号信息”误报成读取失败。
        // 每个站点读完后立即保存轮换后的凭据：站点级接口可能刷新会话或刷新令牌，
        // 全部读完再写会让并发读取共用旧凭据，下一次读取就被拒绝。
        let targets = data
            .providers
            .iter()
            .filter(|provider| overview_target(provider))
            .cloned()
            .collect::<Vec<_>>();
        let entries = futures_util::future::join_all(targets.iter().map(|provider| {
            let settings = &data.settings;
            async move {
                let name = provider.display_label();
                let outcome = match ProtocolAdapter
                    .usage_summary(settings, provider, period)
                    .await
                {
                    Ok(operation) => self
                        .persist_operation_credentials(
                            &ProviderRequestContext::capture(provider),
                            &operation.credentials,
                        )
                        .await
                        .map(|_| operation.value),
                    Err(error) => Err(error),
                };
                (provider, name, outcome)
            }
        }))
        .await;

        let mut overview = ProviderUsageOverview {
            request_count: 0,
            model_stats: Vec::new(),
            model_health: Vec::new(),
            errors: Vec::new(),
        };
        for (provider, name, outcome) in entries {
            let summary = match outcome {
                Ok(summary) => summary,
                Err(error) => {
                    overview.errors.push(ProviderUsageSourceError {
                        provider_id: provider.identity.id.clone(),
                        provider_name: name.clone(),
                        message: error,
                    });
                    continue;
                }
            };
            overview.request_count += summary.request_count;
            overview
                .model_stats
                .extend(
                    summary
                        .model_stats
                        .into_iter()
                        .map(|stat| ProviderUsageModelStat {
                            model_name: stat.model_name,
                            used: stat.used,
                            request_count: stat.request_count,
                            token_used: stat.token_used,
                        }),
                );
            overview.model_health.extend(summary.model_health);
        }
        overview
            .model_stats
            .sort_by_key(|stat| std::cmp::Reverse(stat.request_count));
        overview.model_stats.truncate(MODEL_LIMIT);
        // 失败率高的站点先列，和单个站点的响应质量面板同一顺序。
        overview.model_health.sort_by(|left, right| {
            right
                .success_rate
                .partial_cmp(&left.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        overview.model_health.truncate(MODEL_LIMIT);
        Ok(overview)
    }

    pub async fn request_logs(
        &self,
        id: String,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        // 与用量趋势同一判断：站点级日志需要账号令牌，Key 卡片的请求留在客户端，
        // 直接说明原因而不是先登录再报错。
        if matches!(provider.auth.mode, AuthMode::ApiKey) {
            return Err("请求日志按站点统计，API Key 卡片不提供请求日志".to_string());
        }
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
        let operation = ProtocolAdapter
            .request_logs(&data.settings, &provider, query)
            .await?;
        self.persist_operation_credentials(
            &ProviderRequestContext::capture(&provider),
            &operation.credentials,
        )
        .await?
        .ok_or_else(|| "本地配置已变更，本次请求日志结果已忽略".to_string())?;
        Ok(operation.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderIdentityInput, ProviderInput};
    use crate::models::{ProviderStatus, ProviderUsageModelStat};

    fn provider(
        base_url: &str,
        protocol: ProviderProtocol,
        mode: AuthMode,
        status: ProviderStatus,
    ) -> crate::models::Provider {
        let mut provider = crate::models::Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: base_url.to_string(),
                    protocol,
                    ..ProviderIdentityInput::default()
                },
                ..ProviderInput::default()
            },
            "p".to_string(),
        );
        provider.auth.mode = mode;
        provider.runtime.status = status;
        provider
    }

    #[test]
    fn overview_skips_key_cards_and_forced_login_sites() {
        let site = "https://relay.example.com";
        assert!(overview_target(&provider(
            site,
            ProviderProtocol::NewApi,
            AuthMode::Session,
            ProviderStatus::Ok
        )));
        assert!(overview_target(&provider(
            site,
            ProviderProtocol::Sub2Api,
            AuthMode::AccessToken,
            ProviderStatus::Warning
        )));
        // Key 卡片只有 Key 自身额度，站点级用量无从读起。
        assert!(!overview_target(&provider(
            site,
            ProviderProtocol::NewApi,
            AuthMode::ApiKey,
            ProviderStatus::Ok
        )));
        // 还没填写账号信息的站点本来就不该被计成读取失败。
        assert!(!overview_target(&provider(
            site,
            ProviderProtocol::Sub2Api,
            AuthMode::Session,
            ProviderStatus::Error
        )));
        // OAuth 账号与 CPA 网关不是中转站站点。
        assert!(!overview_target(&provider(
            site,
            ProviderProtocol::OAuth,
            AuthMode::ApiKey,
            ProviderStatus::Ok
        )));
    }

    #[test]
    fn overview_keeps_only_the_busiest_models() {
        let mut stats = (0..MODEL_LIMIT + 3)
            .map(|index| ProviderUsageModelStat {
                model_name: format!("model-{index}"),
                used: 0.0,
                request_count: index as i64,
                token_used: 0,
            })
            .collect::<Vec<_>>();
        stats.sort_by_key(|stat| std::cmp::Reverse(stat.request_count));
        stats.truncate(MODEL_LIMIT);
        assert_eq!(stats.len(), MODEL_LIMIT);
        assert_eq!(stats[0].request_count, (MODEL_LIMIT + 2) as i64);
    }
}
