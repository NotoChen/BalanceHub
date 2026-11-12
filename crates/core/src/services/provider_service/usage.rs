use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{
        AuthMode, ProviderModelHealth, ProviderProtocol, ProviderRequestLogsQuery,
        ProviderRequestLogsResult, ProviderStatus, ProviderUsageModelStat, ProviderUsageOverview,
        ProviderUsageSourceError, ProviderUsageSummary,
    },
};

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

/// 一个站点的用量读取结果。调用方既可能是全站汇总，也可能是本地会话
/// 用量对账；两侧共享同一轮并发读取、凭据持久化和归并名处理。
#[derive(Debug, Clone)]
pub struct ProviderUsageSource {
    pub provider_id: String,
    pub provider_name: String,
    pub summary: Option<ProviderUsageSummary>,
    pub error: Option<String>,
}

/// 用量排行按卡片声明的归并名聚合：同款模型的多个上游 ID（以及汇总视图里
/// 不同中转站的同名模型）合成一行，避免排行被拆散。
fn merge_model_stats(
    stats: impl IntoIterator<Item = ProviderUsageModelStat>,
) -> Vec<ProviderUsageModelStat> {
    let mut merged: std::collections::BTreeMap<String, ProviderUsageModelStat> =
        std::collections::BTreeMap::new();
    for stat in stats {
        let entry =
            merged
                .entry(stat.model_name.clone())
                .or_insert_with(|| ProviderUsageModelStat {
                    model_name: stat.model_name.clone(),
                    used: 0.0,
                    request_count: 0,
                    token_used: 0,
                });
        entry.used += stat.used;
        entry.request_count = entry.request_count.saturating_add(stat.request_count);
        entry.token_used = entry.token_used.saturating_add(stat.token_used);
    }
    merged.into_values().collect()
}

/// 单站排行先把上游模型 ID 换成卡片声明的归并名，再交给合并逻辑。
fn apply_usage_aliases(provider: &crate::models::Provider, stats: &mut [ProviderUsageModelStat]) {
    for stat in stats {
        let model = stat.model_name.trim();
        stat.model_name = provider.model_config.usage_alias(model).to_string();
    }
}

/// 响应质量行和用量排行一样落在卡片声明的归并名上，否则同一模型会在
/// 「模型额度排行」和「响应质量」里显示成两个名字。
fn apply_health_aliases(provider: &crate::models::Provider, health: &mut [ProviderModelHealth]) {
    for row in health {
        let model = row.model_name.trim();
        row.model_name = provider.model_config.usage_alias(model).to_string();
    }
}

/// 响应质量来自各站点自己的采样，汇总结果没有站点维度：同名模型只保留最差
/// 的一次采样，避免多站同名行的列表键冲突，也避免把问题站点摊平在均值里。
fn merge_model_health(health: Vec<ProviderModelHealth>) -> Vec<ProviderModelHealth> {
    let mut merged: std::collections::BTreeMap<String, ProviderModelHealth> =
        std::collections::BTreeMap::new();
    for row in health {
        match merged.get(&row.model_name) {
            Some(current) => {
                let worse = match (current.success_rate, row.success_rate) {
                    // 从未采样换成有采样，有采样更有信息。
                    (None, Some(_)) => true,
                    (Some(before), Some(after)) => after < before,
                    _ => false,
                };
                if worse {
                    merged.insert(row.model_name.clone(), row);
                }
            }
            None => {
                merged.insert(row.model_name.clone(), row);
            }
        }
    }
    merged.into_values().collect()
}

/// 单个站点的排行按花费排序，与适配器返回时的口径一致。
fn sort_by_usage(stats: &mut [ProviderUsageModelStat]) {
    stats.sort_by(|left, right| {
        right
            .used
            .partial_cmp(&left.used)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.request_count.cmp(&left.request_count))
    });
}

/// 汇总视图是「全部中转站合计」，同名模型合并成一行后再排行。列表只留
/// 前 `MODEL_LIMIT` 项，模型总数单独记录，供界面显示真实覆盖范围。
fn finalize_overview(mut overview: ProviderUsageOverview) -> ProviderUsageOverview {
    overview.model_stats = merge_model_stats(overview.model_stats);
    overview.model_health = merge_model_health(overview.model_health);
    overview
        .model_stats
        .sort_by_key(|stat| std::cmp::Reverse(stat.request_count));
    overview.model_count = overview.model_stats.len();
    overview.model_stats.truncate(MODEL_LIMIT);
    // 失败率高的站点先列，和单个站点的响应质量面板同一顺序；未采样的
    // 模型没有成功率可排，放在最后。
    overview.model_health.sort_by(|left, right| {
        let rank = |rate: Option<f64>| rate.unwrap_or(f64::INFINITY);
        rank(left.success_rate)
            .partial_cmp(&rank(right.success_rate))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    overview.model_health.truncate(MODEL_LIMIT);
    overview
}

impl ProviderService {
    pub async fn usage_summary(
        &self,
        id: String,
        period: String,
    ) -> Result<ProviderUsageSummary, String> {
        let state = crate::host::core_state();
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
        let mut summary = operation.value;
        apply_usage_aliases(&provider, &mut summary.model_stats);
        apply_health_aliases(&provider, &mut summary.model_health);
        summary.model_stats = merge_model_stats(summary.model_stats);
        sort_by_usage(&mut summary.model_stats);
        Ok(summary)
    }

    /// 逐站读取用量。`provider_ids` 为空时读取全部能回答用量的站点；
    /// 非空时只读取这些站点，供本地用量对账按账号归属做对比。
    /// 一个站点慢或失败不该拖住其余站点，失败原因随结果返回。
    pub async fn usage_summary_sources(
        &self,
        period: &str,
        provider_ids: &[String],
    ) -> Result<Vec<ProviderUsageSource>, String> {
        let state = crate::host::core_state();
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
            .filter(|provider| {
                provider_ids.is_empty() || provider_ids.iter().any(|id| id == &provider.identity.id)
            })
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

        let mut sources = Vec::with_capacity(entries.len());
        for (provider, name, outcome) in entries {
            let mut source = ProviderUsageSource {
                provider_id: provider.identity.id.clone(),
                provider_name: name,
                summary: None,
                error: None,
            };
            match outcome {
                Ok(mut summary) => {
                    apply_usage_aliases(provider, &mut summary.model_stats);
                    apply_health_aliases(provider, &mut summary.model_health);
                    summary.model_stats = merge_model_stats(summary.model_stats);
                    sort_by_usage(&mut summary.model_stats);
                    source.summary = Some(summary);
                }
                Err(error) => source.error = Some(error),
            }
            sources.push(source);
        }
        Ok(sources)
    }

    /// 汇总视图（不带中转站）把能回答用量的站点合成一份结果。
    pub async fn usage_summary_overview(
        &self,
        period: &str,
    ) -> Result<ProviderUsageOverview, String> {
        let sources = self.usage_summary_sources(period, &[]).await?;
        let mut overview = ProviderUsageOverview {
            request_count: 0,
            model_stats: Vec::new(),
            model_count: 0,
            model_health: Vec::new(),
            errors: Vec::new(),
            window_minutes: 0,
        };
        for source in sources {
            let summary = match source.summary {
                Some(summary) => summary,
                None => {
                    overview.errors.push(ProviderUsageSourceError {
                        provider_id: source.provider_id,
                        provider_name: source.provider_name,
                        message: source.error.unwrap_or_else(|| "用量读取失败".to_string()),
                    });
                    continue;
                }
            };
            overview.request_count += summary.request_count;
            overview.window_minutes = overview.window_minutes.max(summary.window_minutes);
            overview.model_stats.extend(summary.model_stats);
            overview.model_health.extend(summary.model_health);
        }
        Ok(finalize_overview(overview))
    }

    pub async fn request_logs(
        &self,
        id: String,
        query: ProviderRequestLogsQuery,
    ) -> Result<ProviderRequestLogsResult, String> {
        let state = crate::host::core_state();
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
    use crate::models::{ProviderModelOverride, ProviderStatus, ProviderUsageModelStat};

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
    fn usage_rows_merge_under_the_declared_alias() {
        let mut provider = provider(
            "https://relay.example.com",
            ProviderProtocol::NewApi,
            AuthMode::Session,
            ProviderStatus::Ok,
        );
        provider.model_config.overrides.insert(
            "gpt-5.5-2026".into(),
            ProviderModelOverride {
                alias: "gpt-5.5".into(),
                ..Default::default()
            },
        );
        let mut stats = vec![
            ProviderUsageModelStat {
                model_name: "gpt-5.5-2026".into(),
                used: 3.0,
                request_count: 30,
                token_used: 300,
            },
            ProviderUsageModelStat {
                model_name: "gpt-5.5".into(),
                used: 2.0,
                request_count: 20,
                token_used: 200,
            },
            ProviderUsageModelStat {
                model_name: "o5".into(),
                used: 1.0,
                request_count: 10,
                token_used: 100,
            },
        ];

        apply_usage_aliases(&provider, &mut stats);
        let mut merged = merge_model_stats(stats);
        sort_by_usage(&mut merged);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].model_name, "gpt-5.5");
        assert_eq!(merged[0].used, 5.0);
        assert_eq!(merged[0].request_count, 50);
        assert_eq!(merged[0].token_used, 500);
        assert_eq!(merged[1].model_name, "o5");
    }

    #[test]
    fn overview_health_keeps_the_worst_sample_per_model() {
        let health = vec![
            ProviderModelHealth {
                model_name: "gpt-5.5".into(),
                success_rate: Some(0.9),
                avg_latency_ms: Some(100),
                avg_tps: None,
            },
            ProviderModelHealth {
                model_name: "gpt-5.5".into(),
                success_rate: Some(0.4),
                avg_latency_ms: Some(900),
                avg_tps: Some(12.0),
            },
            ProviderModelHealth {
                model_name: "o5".into(),
                success_rate: None,
                avg_latency_ms: None,
                avg_tps: None,
            },
            ProviderModelHealth {
                model_name: "o5".into(),
                success_rate: Some(0.8),
                avg_latency_ms: Some(200),
                avg_tps: None,
            },
        ];

        let merged = merge_model_health(health);
        assert_eq!(merged.len(), 2);
        let gpt = merged
            .iter()
            .find(|row| row.model_name == "gpt-5.5")
            .expect("gpt row");
        assert_eq!(gpt.success_rate, Some(0.4));
        assert_eq!(gpt.avg_latency_ms, Some(900));
        let o5 = merged
            .iter()
            .find(|row| row.model_name == "o5")
            .expect("o5 row");
        // 未采样行不会盖掉真实的成功率。
        assert_eq!(o5.success_rate, Some(0.8));
    }

    #[test]
    fn overview_model_count_is_kept_before_the_top_list_is_truncated() {
        let stats = (0..MODEL_LIMIT + 3)
            .map(|index| ProviderUsageModelStat {
                model_name: format!("model-{index}"),
                used: 0.0,
                request_count: index as i64,
                token_used: 0,
            })
            .collect::<Vec<_>>();
        let overview = finalize_overview(ProviderUsageOverview {
            request_count: 0,
            model_stats: stats,
            model_count: 0,
            model_health: Vec::new(),
            errors: Vec::new(),
            window_minutes: 0,
        });
        // 界面上的「涉及模型」来自 model_count，不能等于被截断的列表长度。
        assert_eq!(overview.model_count, MODEL_LIMIT + 3);
        assert_eq!(overview.model_stats.len(), MODEL_LIMIT);
        assert_eq!(
            overview.model_stats[0].request_count,
            (MODEL_LIMIT + 2) as i64
        );
    }

    #[test]
    fn health_rows_merge_under_the_declared_alias() {
        let mut provider = provider(
            "https://relay.example.com",
            ProviderProtocol::NewApi,
            AuthMode::Session,
            ProviderStatus::Ok,
        );
        provider.model_config.overrides.insert(
            "gpt-5.5-2026".into(),
            ProviderModelOverride {
                alias: "gpt-5.5".into(),
                ..Default::default()
            },
        );
        let mut health = vec![
            ProviderModelHealth {
                model_name: "gpt-5.5-2026".into(),
                success_rate: Some(99.0),
                avg_latency_ms: Some(100),
                avg_tps: None,
            },
            ProviderModelHealth {
                model_name: "gpt-5.5".into(),
                success_rate: Some(80.0),
                avg_latency_ms: Some(300),
                avg_tps: None,
            },
        ];
        apply_health_aliases(&provider, &mut health);
        let merged = merge_model_health(health);
        // 同一个模型的两个上游 ID 在响应质量里也只剩一行，且取更差的那次采样。
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].model_name, "gpt-5.5");
        assert_eq!(merged[0].success_rate, Some(80.0));
    }
}
