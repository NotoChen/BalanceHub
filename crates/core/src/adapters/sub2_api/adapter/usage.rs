use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        sub2_api::{
            auth::request_account_json,
            json::{integer_field, number_field, object_array, string_field},
            profile::quota_display,
            usage::{normalize_log, urlencoding, usage_dates, usage_window_minutes},
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
    pub async fn usage_summary(
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
                window_minutes: usage_window_minutes(period),
            },
        ))
    }

    pub async fn request_logs(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        query: ProviderRequestLogsQuery,
    ) -> Result<(Provider, ProviderRequestLogsResult), String> {
        let client = build_client(settings, provider).await?;
        let page = query.page + 1;
        // 统计接口必须带时间范围，且要和列表使用同一段窗口，否则“消耗”会和
        // 列表里的记录对不上。Sub2API 只按自然日筛选，时间戳在这里转换成
        // 本地日期；没有传范围时沿用 30 天。
        let (start_date, end_date) = query_dates(&query);
        let model = query.model.trim();
        let mut path = format!(
            "/usage?page={page}&page_size={}&start_date={start_date}&end_date={end_date}",
            query.page_size.max(1)
        );
        if !model.is_empty() {
            path.push_str("&model=");
            path.push_str(&urlencoding(model));
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
            fetch_request_log_stats(&client, &authenticated, model, &start_date, &end_date).await;
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

/// Sub2API 的日志接口只接受自然日范围。查询里带了完整时间戳时换算成本地
/// 日期，任一端缺失或无效则回退到默认 30 天，避免生成空日期参数。
fn query_dates(query: &ProviderRequestLogsQuery) -> (String, String) {
    let local_date = |timestamp: i64| {
        chrono::DateTime::from_timestamp(timestamp, 0).map(|datetime| {
            datetime
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
    };
    match (
        query.start_timestamp.and_then(local_date),
        query.end_timestamp.and_then(local_date),
    ) {
        (Some(start), Some(end)) => (start, end),
        _ => usage_dates("30d"),
    }
}

/// 统计是日志列表的补充：Sub2API 把它拆在 `/usage/stats`（按同一筛选汇总消耗）
/// 与 `/usage/dashboard/stats`（全站近 5 分钟 RPM / TPM）。任一接口失败只让对应
/// 数字变成 `None`（界面显示「—」），不影响已经读到的日志列表；每次调用返回的
/// provider 都要继续传下去，否则中途轮换的令牌会被丢掉。
async fn fetch_request_log_stats(
    client: &ProviderTransport,
    provider: &Provider,
    model: &str,
    start_date: &str,
    end_date: &str,
) -> (Provider, ProviderRequestLogStats) {
    let mut current = provider.clone();
    let mut stats_path = format!("/usage/stats?start_date={start_date}&end_date={end_date}");
    if !model.is_empty() {
        stats_path.push_str("&model=");
        stats_path.push_str(&urlencoding(model));
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
            Some(number_field(
                &data,
                &["total_actual_cost", "totalActualCost"],
            ))
        }
        Err(_) => None,
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
                Some(number_field(&data, &["rpm"])),
                Some(number_field(&data, &["tpm"])),
                (duration > 0.0).then_some(duration),
            )
        }
        Err(_) => (None, None, None),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::test_support::{serve, Reply};
    use crate::models::{AuthMode, ProviderProtocol};

    fn local_date(timestamp: i64) -> String {
        chrono::DateTime::from_timestamp(timestamp, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    }

    #[test]
    fn missing_timestamps_fall_back_to_the_default_window() {
        let query = ProviderRequestLogsQuery::default();
        assert_eq!(query_dates(&query), usage_dates("30d"));
        assert_eq!(
            query_dates(&ProviderRequestLogsQuery {
                start_timestamp: Some(1_767_225_600),
                end_timestamp: None,
                ..Default::default()
            }),
            usage_dates("30d")
        );
    }

    #[test]
    fn timestamps_are_converted_to_local_dates() {
        let start = 1_767_225_600;
        let end = 1_767_312_000;
        assert_eq!(
            query_dates(&ProviderRequestLogsQuery {
                start_timestamp: Some(start),
                end_timestamp: Some(end),
                ..Default::default()
            }),
            (local_date(start), local_date(end))
        );
    }

    #[tokio::test]
    async fn request_logs_send_the_model_and_share_the_date_window_with_stats() {
        let (mut provider, requests, server) = serve(vec![
            Reply(200, r#"{"code":0,"data":{"items":[],"total":0}}"#),
            Reply(200, r#"{"code":0,"data":{"total_actual_cost":0}}"#),
            Reply(200, r#"{"code":0,"data":{"rpm":0,"tpm":0}}"#),
        ]);
        provider.identity.protocol = ProviderProtocol::Sub2Api;
        provider.auth.mode = AuthMode::AccessToken;
        provider.auth.access_token = "fixture-jwt".to_string();
        let start = 1_767_225_600;
        let end = 1_767_312_000;
        Sub2ApiAdapter
            .request_logs(
                &AppSettings::default(),
                &provider,
                ProviderRequestLogsQuery {
                    model: " gpt-4o ".to_string(),
                    page: 1,
                    page_size: 20,
                    start_timestamp: Some(start),
                    end_timestamp: Some(end),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        server.join().unwrap();

        let expected = format!(
            "start_date={}&end_date={}",
            local_date(start),
            local_date(end)
        );
        let list = requests.recv().unwrap();
        assert!(list.starts_with("GET /relay/api/v1/usage?page=2&page_size=20&"));
        assert!(list.contains(&expected));
        assert!(list.contains("&model=gpt-4o"));
        let stats = requests.recv().unwrap();
        assert!(stats.starts_with("GET /relay/api/v1/usage/stats?"));
        assert!(stats.contains(&expected));
        assert!(stats.contains("&model=gpt-4o"));
    }
}
