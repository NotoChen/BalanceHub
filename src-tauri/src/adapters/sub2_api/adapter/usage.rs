use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::request_account_json,
            json::{integer_field, number_field, object_array, string_field},
            profile::quota_display,
            usage::{normalize_log, urlencoding, usage_dates},
        },
        transport::{build_client, ProviderTransport},
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
                request_count: points.iter().map(|point| point.request_count).sum(),
                points,
                model_stats,
                // Sub2API only exposes aggregate daily trend and aggregate model
                // statistics. Do not fabricate per-model daily zero points.
                model_points: Vec::new(),
                model_health: Vec::new(),
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
        // 统计接口必须带时间范围，且要和列表使用同一段窗口，否则“消耗”会和
        // 列表里的记录对不上；30 天与 NewAPI 的请求日志窗口一致。
        let (start_date, end_date) = usage_dates("30d");
        let keyword = query.keyword.trim();
        let mut path = format!(
            "/usage?page={page}&page_size={}&start_date={start_date}&end_date={end_date}",
            query.page_size.max(1)
        );
        if !keyword.is_empty() {
            path.push_str("&model=");
            path.push_str(&urlencoding(keyword));
        }
        let (authenticated, data) =
            request_account_json(&client, provider, Method::GET, &path, None, "读取请求日志")
                .await?;
        let logs = object_array(&data, "items")
            .into_iter()
            .map(normalize_log)
            .collect::<Vec<_>>();
        let total = data.get("total").map(|_| integer_field(&data, &["total"]));
        let (authenticated, stats) =
            fetch_request_log_stats(&client, &authenticated, keyword, &start_date, &end_date).await;
        Ok((
            authenticated,
            ProviderRequestLogsResult {
                provider_id: provider.identity.id.clone(),
                provider_name: provider.display_label(),
                page: query.page,
                page_size: query.page_size,
                total,
                quota_display: quota_display(provider),
                stats,
                logs,
                message: "请求日志已加载".to_string(),
            },
        ))
    }
}

/// 统计是日志列表的补充：Sub2API 把它拆在 `/usage/stats`（按同一筛选汇总消耗）
/// 与 `/usage/dashboard/stats`（近 5 分钟 RPM / TPM）。任一接口失败只让对应数字
/// 归零，不影响已经读到的日志列表；每次调用返回的 provider 都要继续传下去，
/// 否则中途轮换的令牌会被丢掉。
async fn fetch_request_log_stats(
    client: &ProviderTransport,
    provider: &Provider,
    keyword: &str,
    start_date: &str,
    end_date: &str,
) -> (Provider, ProviderRequestLogStats) {
    let mut current = provider.clone();
    let mut stats_path = format!("/usage/stats?start_date={start_date}&end_date={end_date}");
    if !keyword.is_empty() {
        stats_path.push_str("&model=");
        stats_path.push_str(&urlencoding(keyword));
    }
    let quota = match request_account_json(
        client,
        &current,
        Method::GET,
        &stats_path,
        None,
        "读取日志消耗",
    )
    .await
    {
        Ok((next, data)) => {
            current = next;
            number_field(&data, &["total_actual_cost", "totalActualCost"])
        }
        Err(_) => 0.0,
    };
    let (rpm, tpm, avg_duration_ms) = match request_account_json(
        client,
        &current,
        Method::GET,
        "/usage/dashboard/stats",
        None,
        "读取日志指标",
    )
    .await
    {
        Ok((next, data)) => {
            current = next;
            let duration =
                number_field(&data, &["average_duration_ms", "averageDurationMs"]).max(0.0);
            (
                number_field(&data, &["rpm"]),
                number_field(&data, &["tpm"]),
                (duration > 0.0).then_some(duration),
            )
        }
        Err(_) => (0.0, 0.0, None),
    };
    (
        current,
        ProviderRequestLogStats {
            quota,
            rpm,
            tpm,
            avg_duration_ms,
        },
    )
}
