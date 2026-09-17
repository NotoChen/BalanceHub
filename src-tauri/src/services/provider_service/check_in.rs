use crate::services::check_in_tasks::{CheckInContext, HTTP_SLOTS};
use crate::{
    adapters::protocol::ProtocolAdapter,
    limits,
    models::{
        check_in_message_indicates_disabled, provider_domain, CheckInError, CheckInPhase, Provider,
        ProviderCheckInRecord, ProviderCheckInRecordsResult, ProviderCheckInResult,
        ProviderQuotaDisplay, ProviderStatus,
    },
    util::unix_millis as current_timestamp_millis,
};
use std::time::Duration;
use tauri::Manager;

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

    pub(crate) async fn check_in_attempt(
        &self,
        id: String,
        task: &CheckInContext,
    ) -> Result<ProviderCheckInResult, CheckInError> {
        let http_slot = HTTP_SLOTS.acquire().await.map_err(|_| "签到队列不可用")?;
        task.phase(self.app, CheckInPhase::Checking);
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let adapter = ProtocolAdapter;
        let state = self.app.state::<crate::state::AppState>();
        let network_gate = state.refresh_gate.lock().await;
        self.current_operation_provider(&request_context)
            .await?
            .ok_or("账号配置已变更，已停止本次签到")?;
        task.phase(self.app, CheckInPhase::Requesting);
        let operation = tokio::time::timeout(
            Duration::from_secs(120),
            adapter.check_in(&data.settings, &provider),
        )
        .await
        .map_err(|_| {
            CheckInError::Unconfirmed("签到请求超时；结果可能已提交，请查看站点记录".to_string())
        })??;
        let mut effective_provider = self
            .persist_operation_credentials(&request_context, &operation.credentials)
            .await
            .map_err(|error| {
                CheckInError::Unconfirmed(format!(
                    "签到请求已返回，但本地会话保存失败：{error}；已停止自动重试"
                ))
            })?
            .ok_or_else(|| {
                CheckInError::Unconfirmed(
                    "签到请求已完成，但本地配置已变更，本次结果未写入当前账号".to_string(),
                )
            })?;
        drop(network_gate);
        drop(http_slot);
        task.authenticated(&effective_provider);
        let mut result = operation.value;
        let browser_assisted = result.verification_required.is_some();
        if let Some(verification) = result.verification_required {
            let verification = crate::models::CheckInVerificationRequest {
                kind: verification,
                requires_login: result.verification_requires_login,
            };
            let browser_operation = crate::services::browser_check_in::run(
                self.app,
                &data.settings,
                &effective_provider,
                verification,
                task,
            )
            .await?;
            effective_provider = self
                .persist_operation_credentials(
                    &ProviderRequestContext::capture(&effective_provider),
                    &browser_operation.credentials,
                )
                .await
                .map_err(|error| {
                    CheckInError::Unconfirmed(format!(
                        "站点操作已完成，但本地会话保存失败：{error}；已停止自动重试"
                    ))
                })?
                .ok_or_else(|| {
                    CheckInError::Unconfirmed("站点操作已完成，但账号配置已变更".to_string())
                })?;
            task.authenticated(&effective_provider);
            result = browser_operation.value;
        }
        if result.unconfirmed {
            return Err(CheckInError::Unconfirmed(result.message));
        }
        let mutation_context = ProviderRequestContext::capture(&effective_provider);
        // Browser requests already confirmed the result. A full HTTP refresh
        // would immediately re-enter the same wall and create unrelated traffic.
        let refreshed_provider = if result.ok && !browser_assisted {
            let _network_gate = state.refresh_gate.lock().await;
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
            task.phase(self.app, CheckInPhase::Saving);
            let checked_in_at = current_timestamp_millis().to_string();
            let check_in_user = provider_domain::capabilities::check_in_user(&effective_provider);
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
                .await
                .map_err(|error| {
                    CheckInError::Unconfirmed(format!(
                        "站点已确认签到，但本地记录保存失败：{error}；请勿重复提交"
                    ))
                })?;
            if persisted {
                result.last_checked_in_at = Some(checked_in_at);
                result.last_check_in_user = Some(check_in_user);
            } else {
                return Err(CheckInError::Unconfirmed(
                    "站点已确认签到，但本地配置已变更，记录未写入当前账号".to_string(),
                ));
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
