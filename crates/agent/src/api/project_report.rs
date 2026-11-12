//! 项目视图命令。元数据与用量候选在同一次来源扫描中收集；启用消耗统计时
//! 再单独解码候选正文，复用用量对账的计价链路。用量失败只降级为元数据，
//! 不影响项目列表。

use crate::{
    api::{blocking, ProgressSink},
    models::{
        AgentProjectProgress, AgentProjectProgressPhase, AgentProjectReport,
        AgentProjectReportRequest,
    },
    services::{
        agent_cli::contracts::SessionReadBudget,
        cli_sessions::workbench::{self, AgentSessionActor},
    },
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

const METADATA_TIMEOUT: Duration = Duration::from_secs(20);
const USAGE_TIMEOUT: Duration = Duration::from_secs(40);
const METADATA_FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const METADATA_BYTES: u64 = 256 * 1024 * 1024;
const USAGE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) async fn get_agent_projects(
    actor: String,
    request: AgentProjectReportRequest,
    progress: Option<Arc<dyn ProgressSink>>,
) -> Result<AgentProjectReport, String> {
    workbench::validate_usage_period(&request.period)?;
    crate::services::wsl::refresh_background();
    let include_usage = request.include_usage;
    let budget = workbench::begin_request(
        AgentSessionActor { client: &actor },
        &request.consumer_id,
        request.request_id,
        workbench::PROJECT_REPORT_TIMEOUT,
    )?;
    let cancellation = budget.cancelled.clone();
    let period = request.period.clone();
    let reporter: Option<workbench::ProjectProgressReporter> = progress.map(|sink| {
        let reporter: workbench::ProjectProgressReporter =
            Arc::new(move |event: AgentProjectProgress| {
                if let Ok(value) = serde_json::to_value(event) {
                    let _ = sink.send(value);
                }
            });
        reporter
    });
    let scan_reporter = reporter.clone();
    let metadata_budget =
        SessionReadBudget::new(budget.deadline, cancellation.clone(), METADATA_BYTES);
    let collected = tokio::time::timeout(
        METADATA_TIMEOUT,
        blocking("汇总 Agent 项目", {
            let actor = actor.clone();
            let period = period.clone();
            move || {
                workbench::collect_project_report(
                    AgentSessionActor { client: &actor },
                    &period,
                    &metadata_budget,
                    include_usage,
                    scan_reporter,
                )
            }
        }),
    )
    .await;
    let mut collected = match collected {
        Ok(Ok(collected)) => collected,
        Ok(Err(error)) => {
            cancellation.store(true, Ordering::Release);
            return Err(error);
        }
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            return Err("项目扫描超时，后台读取已取消".into());
        }
    };
    let usage_candidates = collected.take_usage_candidates();

    let mut usage = None;
    let mut usage_error = None;
    if include_usage {
        if let Some(candidates) = usage_candidates {
            let usage_budget = SessionReadBudget::new(
                Instant::now() + USAGE_TIMEOUT,
                cancellation.clone(),
                USAGE_BYTES,
            );
            let decode_reporter = reporter.clone();
            let collected_usage = tokio::time::timeout(
                USAGE_TIMEOUT,
                blocking("解码项目用量", {
                    let actor = actor.clone();
                    move || {
                        candidates.decode(
                            AgentSessionActor { client: &actor },
                            &usage_budget,
                            decode_reporter,
                        )
                    }
                }),
            )
            .await;
            match collected_usage {
                Ok(Ok(collected_usage)) => {
                    let model_ids = collected_usage.model_ids();
                    let mut pricing_error = None;
                    if !model_ids.is_empty() {
                        match tokio::time::timeout(
                            METADATA_FETCH_TIMEOUT,
                            crate::api::fetch_model_metadata(model_ids),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {}
                            Ok(Err(error)) => {
                                pricing_error = Some(format!("模型价格资料读取失败：{error}"))
                            }
                            Err(_) => {
                                pricing_error =
                                    Some("模型价格资料读取超时，费用估算可能不完整".to_string())
                            }
                        }
                    }
                    let data = crate::state::app_data();
                    let catalog = crate::state::core_state().model_catalog().ok();
                    let providers = data.providers;
                    let finalized = tokio::time::timeout(
                        FINALIZE_TIMEOUT,
                        blocking("汇总项目用量", move || {
                            Ok(workbench::finalize_usage_report(
                                collected_usage,
                                &providers,
                                catalog.as_deref(),
                                pricing_error,
                            ))
                        }),
                    )
                    .await;
                    match finalized {
                        Ok(Ok(report)) => usage = Some(report),
                        Ok(Err(error)) => usage_error = Some(format!("用量汇总失败：{error}")),
                        Err(_) => usage_error = Some("用量汇总超时，仅显示会话元数据".into()),
                    }
                }
                Ok(Err(error)) => usage_error = Some(format!("用量统计失败：{error}")),
                Err(_) => usage_error = Some("用量统计超时，仅显示会话元数据".into()),
            }
        }
    }

    if let Some(reporter) = &reporter {
        reporter(AgentProjectProgress {
            phase: AgentProjectProgressPhase::Finalizing,
            completed: 0,
            total: 1,
            message: "汇总项目与用量".into(),
        });
    }

    let data = crate::state::app_data();
    let providers = data.providers;
    let report = tokio::time::timeout(
        FINALIZE_TIMEOUT,
        blocking("汇总 Agent 项目", move || {
            Ok(workbench::finalize_project_report(
                collected,
                &providers,
                usage,
                usage_error,
            ))
        }),
    )
    .await
    .map_err(|_| "项目汇总超时".to_string())??;
    cancellation.store(true, Ordering::Release);
    Ok(report)
}
