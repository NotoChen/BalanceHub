use crate::{
    adapters::protocol::ProtocolAdapter,
    limits,
    models::{
        check_in_message_indicates_disabled, provider_domain, Provider, ProviderBatchDetails,
        ProviderBatchOperation, ProviderBatchProgressEvent, ProviderBatchProgressItem,
        ProviderBatchStatus, ProviderCheckInRecord, ProviderCheckInRecordsResult,
        ProviderCheckInResult, ProviderQuotaDisplay, ProviderStatus, RefreshResult,
    },
    util::unix_millis as current_timestamp_millis,
};
use std::sync::Arc;
use tauri::{ipc::Channel, Manager};

use super::{
    find_provider, refresh::apply_refresh_owned_fields, MutationDecision, ProviderRequestContext,
    ProviderService,
};

impl<'a> ProviderService<'a> {
    pub async fn check_in_records(
        &self,
        id: String,
        month: String,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        match ProtocolAdapter
            .check_in_records(&data.settings, &provider, &month)
            .await
        {
            Ok(operation) => {
                self.persist_operation_credentials(&request_context, &operation.credentials)
                    .await?
                    .ok_or_else(|| "本地配置已变更，本次签到记录结果已忽略".to_string())?;
                Ok(operation.value)
            }
            Err(message) => {
                self.current_operation_provider(&request_context)
                    .await?
                    .ok_or_else(|| "本地配置已变更，本次签到记录结果已忽略".to_string())?;
                Ok(local_check_in_records_result(
                    &provider,
                    &month,
                    Some(message),
                ))
            }
        }
    }

    pub async fn check_in(&self, id: String) -> Result<ProviderCheckInResult, String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let _gate = state.check_in_gate.lock().await;
        self.check_in_inner(id).await
    }

    pub async fn mark_auto_check_in_failure(
        &self,
        provider: &Provider,
        message: String,
    ) -> Result<(), String> {
        let request_context = ProviderRequestContext::capture(provider);
        self.mutate_decided_async(move |data| {
            if let Some(provider) = data
                .providers
                .iter_mut()
                .find(|provider| request_context.matches(provider))
            {
                let changed = !matches!(provider.runtime.status, ProviderStatus::Error)
                    || provider.runtime.error_message.as_deref() != Some(message.as_str());
                if !changed {
                    return Ok(MutationDecision::unchanged(()));
                }
                provider.runtime.status = ProviderStatus::Error;
                provider.runtime.error_message = Some(message);
                return Ok(MutationDecision::changed(()));
            }
            Ok(MutationDecision::unchanged(()))
        })
        .await
    }

    async fn check_in_inner(&self, id: String) -> Result<ProviderCheckInResult, String> {
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let adapter = ProtocolAdapter;
        let operation = adapter.check_in(&data.settings, &provider).await?;
        let effective_provider = self
            .persist_operation_credentials(&request_context, &operation.credentials)
            .await?
            .ok_or_else(|| {
                "签到请求已完成，但本地配置已变更，本次结果未写入当前账号".to_string()
            })?;
        let mutation_context = ProviderRequestContext::capture(&effective_provider);
        let mut result = operation.value;
        let is_anyrouter = adapter.is_anyrouter(&effective_provider);
        let refreshed_provider = if result.ok {
            let refresh_outcome = adapter
                .refresh_provider(&data.settings, &effective_provider)
                .await;
            let mut refreshed = effective_provider.clone();
            refresh_outcome.apply_to(&mut refreshed);
            Some(refreshed)
        } else {
            None
        };

        if result.ok {
            let checked_in_at = current_timestamp_millis().to_string();
            let check_in_user =
                provider_domain::capabilities::check_in_user(&effective_provider, is_anyrouter);
            let quota_delta = refreshed_provider
                .as_ref()
                .and_then(|refreshed| check_in_quota_delta(&effective_provider, refreshed));
            result.quota_delta = quota_delta;
            let stored_checked_in_at = checked_in_at.clone();
            let stored_user = check_in_user.clone();
            let stored_record = local_check_in_record(
                &stored_checked_in_at,
                non_empty(&result.message, "签到成功"),
                quota_delta,
            );
            let refreshed_provider = refreshed_provider.filter(is_successful_quota_refresh);
            let provider_id = id.clone();
            let checked_in_at_for_mutation = stored_checked_in_at.clone();
            let check_in_user_for_mutation = stored_user.clone();
            let persisted = self
                .mutate_decided_async(move |data| {
                    if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                        stored.identity.id == provider_id && mutation_context.matches(stored)
                    }) {
                        if let Some(refreshed) = refreshed_provider {
                            let _ = apply_refresh_owned_fields(
                                stored_provider,
                                refreshed,
                                &mutation_context,
                            );
                        }
                        stored_provider.automation.last_checked_in_at =
                            Some(checked_in_at_for_mutation);
                        stored_provider.automation.last_check_in_user = check_in_user_for_mutation;
                        upsert_local_check_in_record(stored_provider, stored_record);
                        if stored_provider
                            .runtime
                            .error_message
                            .as_deref()
                            .is_some_and(is_auto_check_in_error)
                        {
                            stored_provider.runtime.error_message = None;
                            stored_provider.runtime.status =
                                if stored_provider.automation.last_synced_at.is_some() {
                                    ProviderStatus::Ok
                                } else {
                                    ProviderStatus::Warning
                                };
                        }
                        Ok(MutationDecision::changed(true))
                    } else {
                        Ok(MutationDecision::unchanged(false))
                    }
                })
                .await?;
            if persisted {
                result.last_checked_in_at = Some(checked_in_at);
                result.last_check_in_user = Some(check_in_user);
            } else {
                result.message = format!(
                    "{}；本地配置已变更，本次结果未写入当前账号",
                    non_empty(&result.message, "签到成功")
                );
            }
        } else if check_in_message_indicates_disabled(&result.message) {
            let probed_at = current_timestamp_millis().to_string();
            let provider_id = id.clone();
            let mutation_context_for_probe = mutation_context.clone();
            self.mutate_decided_async(move |data| {
                if let Some(stored_provider) = data.providers.iter_mut().find(|stored| {
                    stored.identity.id == provider_id && mutation_context_for_probe.matches(stored)
                }) {
                    stored_provider.capabilities.check_in_known = true;
                    stored_provider.capabilities.check_in_supported = false;
                    stored_provider.capabilities.check_in_auth_modes.clear();
                    stored_provider.capabilities.probed_at = Some(probed_at);
                    return Ok(MutationDecision::changed(()));
                }
                Ok(MutationDecision::unchanged(()))
            })
            .await?;
        }

        Ok(result)
    }

    /// 批量签到的唯一后端入口。目标清单、跳过原因和逐站结果均由 Rust 根据当前
    /// 存储状态计算，前端只负责订阅事件和展示，不再维护第二套签到筛选规则。
    pub async fn check_in_all_with_progress(
        &self,
        channel: Channel<ProviderBatchProgressEvent>,
    ) -> Result<RefreshResult, String> {
        const MAX_CONCURRENT_CHECK_IN: usize = 6;

        // 与全局刷新共用闸门，避免签到后的静默刷新和全量刷新同时写同一张卡片。
        let state = self.app.state::<crate::state::AppState>();
        let _refresh_gate = state.refresh_gate.lock().await;
        let _check_in_gate = state.check_in_gate.lock().await;
        let data = self.snapshot_async().await?;
        let mut progress_items = Vec::with_capacity(data.providers.len());
        let mut targets = Vec::new();
        for provider in &data.providers {
            let item = if !provider.runtime.enabled {
                ProviderBatchProgressItem::skipped(provider, "中转站已停用")
            } else {
                let is_anyrouter = ProtocolAdapter.is_anyrouter(provider);
                let supports =
                    provider_domain::capabilities::supports_check_in(provider, is_anyrouter);
                if !supports {
                    ProviderBatchProgressItem::skipped(provider, "当前协议或站点不支持签到")
                } else if provider_domain::capabilities::checked_in_today(provider, is_anyrouter) {
                    ProviderBatchProgressItem::skipped(provider, "今日已签到")
                } else {
                    targets.push(provider.clone());
                    ProviderBatchProgressItem::pending(provider)
                }
            };
            progress_items.push(item);
        }
        send_progress(
            &channel,
            ProviderBatchProgressEvent::Started {
                operation: ProviderBatchOperation::CheckIn,
                total: progress_items.len(),
                items: progress_items.clone(),
            },
        );

        let target_ids = targets
            .iter()
            .map(|provider| provider.identity.id.clone())
            .collect::<Vec<_>>();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CHECK_IN));
        let mut handles = Vec::with_capacity(targets.len());
        for provider in targets {
            let app = self.app.clone();
            let semaphore = Arc::clone(&semaphore);
            let progress = channel.clone();
            handles.push(tauri::async_runtime::spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .expect("check-in semaphore closed");
                send_progress(
                    &progress,
                    ProviderBatchProgressEvent::ProviderStarted {
                        operation: ProviderBatchOperation::CheckIn,
                        item: ProviderBatchProgressItem::new(
                            &provider,
                            ProviderBatchStatus::Running,
                            "正在签到",
                            None,
                        ),
                    },
                );

                let provider_id = provider.identity.id.clone();
                let service = ProviderService::new(&app);
                let (status, message, quota_delta) = match service.check_in_inner(provider_id).await
                {
                    Ok(result) => (
                        if result.ok {
                            ProviderBatchStatus::Success
                        } else {
                            ProviderBatchStatus::Failed
                        },
                        non_empty(
                            &result.message,
                            if result.ok {
                                "签到成功"
                            } else {
                                "签到失败"
                            },
                        )
                        .to_string(),
                        result.quota_delta,
                    ),
                    Err(error) => (ProviderBatchStatus::Failed, error, None),
                };
                let latest = service
                    .snapshot_async()
                    .await
                    .ok()
                    .and_then(|data| find_provider(&data, &provider.identity.id).ok());
                let display_provider = latest.as_ref().unwrap_or(&provider);
                let item = ProviderBatchProgressItem::new(
                    display_provider,
                    status,
                    message,
                    Some(ProviderBatchDetails::from_provider(
                        display_provider,
                        quota_delta,
                    )),
                );
                send_progress(
                    &progress,
                    ProviderBatchProgressEvent::ProviderFinished {
                        operation: ProviderBatchOperation::CheckIn,
                        item: item.clone(),
                    },
                );
                item
            }));
        }

        for handle in handles {
            let item = handle
                .await
                .map_err(|error| format!("签到任务异常: {error}"))?;
            if let Some(slot) = progress_items
                .iter_mut()
                .find(|slot| slot.provider_id == item.provider_id)
            {
                *slot = item;
            }
        }

        let updated_providers = self.providers_by_ids_async(&target_ids).await?;
        send_progress(
            &channel,
            ProviderBatchProgressEvent::Completed {
                operation: ProviderBatchOperation::CheckIn,
                summary: crate::models::ProviderBatchSummary::from_items(&progress_items),
            },
        );
        Ok(RefreshResult { updated_providers })
    }
}

fn send_progress(channel: &Channel<ProviderBatchProgressEvent>, event: ProviderBatchProgressEvent) {
    let _ = channel.send(event);
}

fn is_auto_check_in_error(message: &str) -> bool {
    message.starts_with("自动签到失败：") || message.starts_with("自动签到异常：")
}

fn local_check_in_records_result(
    provider: &Provider,
    month: &str,
    official_error: Option<String>,
) -> ProviderCheckInRecordsResult {
    let mut records = provider
        .automation
        .check_in_records
        .iter()
        .filter(|record| record.date.starts_with(month))
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.date.cmp(&right.date));
    records.dedup_by(|left, right| left.date == right.date);

    let message = match official_error {
        Some(error) if records.is_empty() => {
            format!("官方签到记录不可用，且本地暂无该月记录：{error}")
        }
        Some(error) => format!("官方签到记录不可用，已展示本地记录：{error}"),
        None if records.is_empty() => "本地暂无该月签到记录".to_string(),
        None => format!("已展示 {} 条本地签到记录", records.len()),
    };

    ProviderCheckInRecordsResult {
        provider_id: provider.identity.id.clone(),
        month: month.to_string(),
        records,
        quota_display: ProviderQuotaDisplay {
            quota_display_type: provider.quota.display_type.clone(),
            currency_symbol: provider.quota.currency_symbol.clone(),
        },
        message,
    }
}

fn local_check_in_record(
    checked_at: &str,
    message: &str,
    quota_delta: Option<f64>,
) -> ProviderCheckInRecord {
    ProviderCheckInRecord {
        date: local_date_from_timestamp(checked_at)
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string()),
        checked_at: Some(checked_at.to_string()),
        quota_delta,
        message: non_empty(message, "签到成功").to_string(),
    }
}

fn upsert_local_check_in_record(provider: &mut Provider, record: ProviderCheckInRecord) {
    let previous_quota_delta = provider
        .automation
        .check_in_records
        .iter()
        .find(|stored| stored.date == record.date)
        .and_then(|stored| stored.quota_delta);
    let mut record = record;
    if record.quota_delta.is_none() {
        record.quota_delta = previous_quota_delta;
    }

    provider
        .automation
        .check_in_records
        .retain(|stored| stored.date != record.date);
    provider.automation.check_in_records.push(record);
    provider
        .automation
        .check_in_records
        .sort_by(|left, right| left.date.cmp(&right.date));

    if provider.automation.check_in_records.len() > limits::MAX_CHECK_IN_RECORDS {
        let remove_count =
            provider.automation.check_in_records.len() - limits::MAX_CHECK_IN_RECORDS;
        provider.automation.check_in_records.drain(0..remove_count);
    }
}

fn check_in_quota_delta(before: &Provider, after: &Provider) -> Option<f64> {
    if !is_successful_quota_refresh(after) || before.quota.scope != after.quota.scope {
        return None;
    }

    let delta = after.quota.available - before.quota.available;
    if delta.is_finite() && delta > 0.000_001 {
        Some(delta)
    } else {
        None
    }
}

fn is_successful_quota_refresh(provider: &Provider) -> bool {
    !matches!(provider.runtime.status, ProviderStatus::Error)
}

fn local_date_from_timestamp(value: &str) -> Option<String> {
    let raw = value.trim();
    let timestamp = raw.parse::<i64>().ok()?;
    let seconds = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    chrono::DateTime::from_timestamp(seconds, 0).map(|date| {
        date.with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string()
    })
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderInput, ProviderQuotaScope};

    fn provider_with_available(available: f64) -> Provider {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider.quota.available = available;
        provider.quota.scope = ProviderQuotaScope::Account;
        provider.runtime.status = ProviderStatus::Ok;
        provider
    }

    #[test]
    fn check_in_quota_delta_uses_positive_available_difference() {
        let before = provider_with_available(10.0);
        let after = provider_with_available(15.5);

        assert_eq!(check_in_quota_delta(&before, &after), Some(5.5));
    }

    #[test]
    fn upsert_local_check_in_record_preserves_existing_quota_delta() {
        let mut provider = provider_with_available(10.0);
        let mut first = local_check_in_record("1782460800000", "签到成功", Some(5.0));
        first.date = "2026-06-26".to_string();
        upsert_local_check_in_record(&mut provider, first);

        let mut repeated = local_check_in_record("1782460900000", "今日已签到", None);
        repeated.date = "2026-06-26".to_string();
        upsert_local_check_in_record(&mut provider, repeated);

        assert_eq!(provider.automation.check_in_records.len(), 1);
        assert_eq!(
            provider.automation.check_in_records[0].quota_delta,
            Some(5.0)
        );
        assert_eq!(
            provider.automation.check_in_records[0].message,
            "今日已签到"
        );
    }
}
