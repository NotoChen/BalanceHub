use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::request_account_json,
            json::{integer_field, number_field, object_array, string_field},
            profile::quota_display,
            usage::{normalize_log, urlencoding, usage_dates},
        },
        transport::build_client,
    },
    models::{
        AppSettings, Provider, ProviderRequestLogStats, ProviderRequestLogsQuery,
        ProviderRequestLogsResult, ProviderUsageModelStat, ProviderUsagePoint,
        ProviderUsageSummary,
    },
};
use reqwest::Method;

impl Sub2ApiAdapter {
    pub(crate) async fn usage_summary(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        period: &str,
    ) -> Result<(Provider, ProviderUsageSummary), String> {
        let client = build_client(settings, provider).await?;
        let (start, end) = usage_dates(period);
        let query = format!("?start_date={start}&end_date={end}");
        let (authenticated, trend) = request_account_json(
            &client,
            provider,
            Method::GET,
            &format!("/usage/dashboard/trend{query}"),
            None,
            "读取用量趋势",
        )
        .await?;
        let (authenticated, models) = request_account_json(
            &client,
            &authenticated,
            Method::GET,
            &format!("/usage/dashboard/models{query}"),
            None,
            "读取模型用量",
        )
        .await?;

        let points = object_array(&trend, "trend")
            .into_iter()
            .map(|item| ProviderUsagePoint {
                date: string_field(&item, &["date"]).unwrap_or_default(),
                used: number_field(&item, &["actual_cost", "cost"]),
                request_count: integer_field(&item, &["requests"]),
                token_used: integer_field(&item, &["total_tokens"]),
            })
            .collect::<Vec<_>>();
        let model_stats = object_array(&models, "models")
            .into_iter()
            .map(|item| ProviderUsageModelStat {
                model_name: string_field(&item, &["model"])
                    .unwrap_or_else(|| "未知模型".to_string()),
                used: number_field(&item, &["actual_cost", "cost"]),
                request_count: integer_field(&item, &["requests"]),
                token_used: integer_field(&item, &["total_tokens"]),
            })
            .collect::<Vec<_>>();
        Ok((
            authenticated,
            ProviderUsageSummary {
                provider_id: provider.identity.id.clone(),
                provider_name: provider.display_label(),
                quota_display: quota_display(provider),
                points,
                model_stats,
                // Sub2API only exposes aggregate daily trend and aggregate model
                // statistics. Do not fabricate per-model daily zero points.
                model_points: Vec::new(),
            },
        ))
    }

    pub(crate) async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<(Provider, ProviderRequestLogsResult), String> {
        let client = build_client(settings, provider).await?;
        let page = query.page + 1;
        let mut path = format!("/usage?page={page}&page_size={}", query.page_size.max(1));
        if !query.keyword.trim().is_empty() {
            path.push_str("&model=");
            path.push_str(&urlencoding(query.keyword.trim()));
        }
        let (authenticated, data) =
            request_account_json(&client, provider, Method::GET, &path, None, "读取请求日志")
                .await?;
        let logs = object_array(&data, "items")
            .into_iter()
            .map(normalize_log)
            .collect::<Vec<_>>();
        let total = data.get("total").map(|_| integer_field(&data, &["total"]));
        Ok((
            authenticated,
            ProviderRequestLogsResult {
                provider_id: provider.identity.id.clone(),
                provider_name: provider.display_label(),
                page: query.page,
                page_size: query.page_size,
                total,
                quota_display: quota_display(provider),
                stats: ProviderRequestLogStats::default(),
                logs,
                message: "请求日志已加载".to_string(),
            },
        ))
    }
}
