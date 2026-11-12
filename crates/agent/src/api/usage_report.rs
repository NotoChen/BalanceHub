//! 本地会话用量对账命令。本机解码与站点账单并行读取：站点失败只影响
//! 对比区，本地结果照常返回；费用始终标注为估算，以站点账单为准。

use crate::{
    api::blocking,
    models::{AgentUsageReport, AgentUsageReportRequest, AgentUsageSite, AgentUsageSiteSummary},
    services::cli_sessions::workbench::{self, AgentSessionActor},
};
use balancehub_core::services::provider_service::{ProviderService, ProviderUsageSource};
use std::{collections::BTreeMap, sync::atomic::Ordering, time::Duration};

const SITE_TIMEOUT: Duration = Duration::from_secs(25);
const METADATA_TIMEOUT: Duration = Duration::from_secs(8);
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) async fn get_agent_usage_report(
    actor: String,
    request: AgentUsageReportRequest,
) -> Result<AgentUsageReport, String> {
    workbench::validate_usage_period(&request.period)?;
    crate::services::wsl::refresh_background();
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::USAGE_REPORT_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let period = request.period.clone();
    let local = tokio::time::timeout(
        workbench::USAGE_REPORT_TIMEOUT,
        blocking("汇总本机会话用量", {
            let actor = actor.clone();
            let period = period.clone();
            move || {
                workbench::collect_usage_report(
                    AgentSessionActor { client: &actor },
                    &period,
                    &budget,
                )
            }
        }),
    );
    let site = async {
        if !request.include_site_usage {
            return Ok(Vec::new());
        }
        match tokio::time::timeout(
            SITE_TIMEOUT,
            ProviderService::new().usage_summary_sources(&period, &[]),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err("站点账单读取超时".to_string()),
        }
    };
    let (local, site) = tokio::join!(local, site);
    let collected = match local {
        Ok(Ok(collected)) => collected,
        Ok(Err(error)) => {
            cancellation.store(true, Ordering::Release);
            return Err(error);
        }
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            return Err("本机用量统计超时，后台读取已取消".into());
        }
    };
    let model_ids = collected.model_ids();
    let mut pricing_error = None;
    if !model_ids.is_empty() {
        match tokio::time::timeout(
            METADATA_TIMEOUT,
            crate::api::fetch_model_metadata(model_ids),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => pricing_error = Some(format!("模型价格资料读取失败：{error}")),
            Err(_) => {
                pricing_error = Some("模型价格资料读取超时，费用估算可能不完整".to_string());
            }
        }
    }
    let data = crate::state::app_data();
    let catalog = crate::state::core_state().model_catalog().ok();
    let providers = data.providers;
    let mut report = tokio::time::timeout(
        FINALIZE_TIMEOUT,
        blocking("汇总本地用量", move || {
            Ok(workbench::finalize_usage_report(
                collected,
                &providers,
                catalog.as_deref(),
                pricing_error,
            ))
        }),
    )
    .await
    .map_err(|_| "本地用量汇总超时".to_string())??;
    cancellation.store(true, Ordering::Release);
    attach_site_usage(&mut report, site);
    Ok(report)
}

/// 把站点账单附到本地报告上。只保留本地确实归属过的站点；站点统计包含
/// 该账号的全部流量，本地只覆盖本机会话日志，两个数字不要求相等。
fn attach_site_usage(
    report: &mut AgentUsageReport,
    site: Result<Vec<ProviderUsageSource>, String>,
) {
    let sources = match site {
        Ok(sources) => sources,
        Err(error) => {
            report
                .diagnostics
                .push(format!("站点账单读取失败：{error}"));
            return;
        }
    };
    let mut local_tokens = BTreeMap::<String, u64>::new();
    for account in &report.accounts {
        let Some(provider_id) = account.provider_id.as_ref() else {
            continue;
        };
        let tokens = account
            .usage
            .tokens
            .input_tokens
            .unwrap_or_default()
            .saturating_add(account.usage.tokens.output_tokens.unwrap_or_default());
        let entry = local_tokens.entry(provider_id.clone()).or_default();
        *entry = entry.saturating_add(tokens);
    }
    for source in sources {
        // 本地没有归属到该站的会话时，对比区不铺开无关站点。
        let Some(local) = local_tokens.get(&source.provider_id).copied() else {
            continue;
        };
        let (error, usage) = match source.summary {
            Some(summary) => {
                let token_used = summary
                    .model_stats
                    .iter()
                    .map(|stat| stat.token_used.max(0) as u64)
                    .fold(0u64, u64::saturating_add);
                (
                    None,
                    Some(AgentUsageSiteSummary {
                        request_count: summary.request_count,
                        token_used,
                    }),
                )
            }
            None => (source.error, None),
        };
        report.sites.push(AgentUsageSite {
            provider_id: source.provider_id,
            provider_name: source.provider_name,
            local_tokens: local,
            error,
            usage,
        });
    }
    report.sites.sort_by(|left, right| {
        right
            .local_tokens
            .cmp(&left.local_tokens)
            .then_with(|| left.provider_name.cmp(&right.provider_name))
    });
}
