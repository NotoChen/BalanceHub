use crate::{
    adapters::newapi::NewApiAdapter,
    models::{
        check_in_message_indicates_disabled, provider_domain, Provider, ProviderCheckInRecord,
        ProviderCheckInRecordsResult, ProviderCheckInResult, ProviderQuotaDisplay, ProviderStatus,
    },
    providers::{
        anyrouter::anyrouter_message_indicates_already_checked_in,
        newapi_http::provider_is_anyrouter,
    },
    util::unix_millis as current_timestamp_millis,
};

use super::{
    find_provider,
    refresh::{apply_refresh_owned_fields, RefreshAuthSnapshot},
    ProviderService,
};

impl<'a> ProviderService<'a> {
    pub async fn check_in_records(
        &self,
        id: String,
        month: String,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        match NewApiAdapter
            .check_in_records(&data.settings, &provider, &month)
            .await
        {
            Ok(result) => Ok(result),
            Err(message) => Ok(local_check_in_records_result(
                &provider,
                &month,
                Some(message),
            )),
        }
    }

    pub async fn check_in(&self, id: String) -> Result<ProviderCheckInResult, String> {
        let data = self.snapshot();
        let provider = find_provider(&data, &id)?;
        let refresh_auth_snapshot = RefreshAuthSnapshot::capture(&provider);
        let adapter = NewApiAdapter;
        let mut result = adapter.check_in(&data.settings, &provider).await?;
        let is_anyrouter = provider_is_anyrouter(&provider);
        let refreshed_provider = if result.ok {
            Some(adapter.refresh_provider(&data.settings, &provider).await)
        } else {
            None
        };

        if result.ok {
            let checked_in_at = current_timestamp_millis().to_string();
            let check_in_user =
                provider_domain::capabilities::check_in_user(&provider, is_anyrouter);
            let quota_delta = refreshed_provider
                .as_ref()
                .and_then(|refreshed| check_in_quota_delta(&provider, refreshed));
            let checked_date = local_date_from_timestamp(&checked_in_at)
                .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

            if is_anyrouter
                && quota_delta.is_none()
                && !has_rewarded_local_check_in(&provider, &checked_date)
                && !anyrouter_message_indicates_already_checked_in(&result.message)
            {
                self.mutate(|data| {
                    if let Some(stored_provider) = data
                        .providers
                        .iter_mut()
                        .find(|stored| stored.identity.id == id)
                    {
                        clear_unrewarded_local_check_in(stored_provider, &checked_date);
                    }
                })?;
                result.ok = false;
                result.message = anyrouter_no_reward_message(&result.message);
                return Ok(result);
            }

            let stored_checked_in_at = checked_in_at.clone();
            let stored_user = check_in_user.clone();
            let stored_record = local_check_in_record(
                &stored_checked_in_at,
                non_empty(&result.message, "签到成功"),
                quota_delta,
            );
            let refreshed_provider = refreshed_provider.filter(is_successful_quota_refresh);
            self.mutate(|data| {
                if let Some(stored_provider) = data
                    .providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == id)
                {
                    if let Some(refreshed) = refreshed_provider {
                        apply_refresh_owned_fields(
                            stored_provider,
                            refreshed,
                            &refresh_auth_snapshot,
                        );
                    }
                    stored_provider.automation.last_checked_in_at = Some(stored_checked_in_at);
                    stored_provider.automation.last_check_in_user = stored_user;
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
                }
            })?;
            result.last_checked_in_at = Some(checked_in_at);
            result.last_check_in_user = Some(check_in_user);
        } else if check_in_message_indicates_disabled(&result.message) {
            let probed_at = current_timestamp_millis().to_string();
            self.mutate(|data| {
                if let Some(stored_provider) = data
                    .providers
                    .iter_mut()
                    .find(|stored| stored.identity.id == id)
                {
                    stored_provider.capabilities.check_in_known = true;
                    stored_provider.capabilities.check_in_supported = false;
                    stored_provider.capabilities.check_in_auth_modes.clear();
                    stored_provider.capabilities.probed_at = Some(probed_at);
                }
            })?;
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

    if provider.automation.check_in_records.len() > 730 {
        let remove_count = provider.automation.check_in_records.len() - 730;
        provider.automation.check_in_records.drain(0..remove_count);
    }
}

fn clear_unrewarded_local_check_in(provider: &mut Provider, date: &str) {
    if has_rewarded_local_check_in(provider, date) {
        return;
    }

    provider
        .automation
        .check_in_records
        .retain(|record| record.date != date);
    if provider
        .automation
        .last_checked_in_at
        .as_deref()
        .and_then(local_date_from_timestamp)
        .as_deref()
        == Some(date)
    {
        provider.automation.last_checked_in_at = None;
        provider.automation.last_check_in_user.clear();
    }
}

fn has_rewarded_local_check_in(provider: &Provider, date: &str) -> bool {
    provider
        .automation
        .check_in_records
        .iter()
        .any(|record| record.date == date && record.quota_delta.is_some())
}

fn anyrouter_no_reward_message(site_message: &str) -> String {
    let site_message = site_message.trim();
    let suffix = if site_message.is_empty() {
        String::new()
    } else {
        format!("站点返回：{site_message}")
    };
    non_empty(
        &format!(
            "AnyRouter 本次签到未获得余额增量，未写入签到记录。通常 08:00 前签到不会发放余额，请 08:00 后重试。{suffix}"
        ),
        "AnyRouter 本次签到未获得余额增量，请 08:00 后重试。",
    )
    .to_string()
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

    #[test]
    fn clear_unrewarded_local_check_in_removes_empty_record_and_last_check_in() {
        let mut provider = provider_with_available(10.0);
        let mut record = local_check_in_record("1782460800000", "签到成功", None);
        record.date = "2026-06-26".to_string();
        upsert_local_check_in_record(&mut provider, record);
        provider.automation.last_checked_in_at = Some("1782460800000".to_string());
        provider.automation.last_check_in_user = "provider-test".to_string();

        clear_unrewarded_local_check_in(&mut provider, "2026-06-26");

        assert!(provider.automation.check_in_records.is_empty());
        assert_eq!(provider.automation.last_checked_in_at, None);
        assert!(provider.automation.last_check_in_user.is_empty());
    }

    #[test]
    fn clear_unrewarded_local_check_in_preserves_rewarded_record() {
        let mut provider = provider_with_available(10.0);
        let mut record = local_check_in_record("1782460800000", "签到成功", Some(5.0));
        record.date = "2026-06-26".to_string();
        upsert_local_check_in_record(&mut provider, record);
        provider.automation.last_checked_in_at = Some("1782460800000".to_string());
        provider.automation.last_check_in_user = "provider-test".to_string();

        clear_unrewarded_local_check_in(&mut provider, "2026-06-26");

        assert_eq!(provider.automation.check_in_records.len(), 1);
        assert_eq!(
            provider.automation.check_in_records[0].quota_delta,
            Some(5.0)
        );
        assert_eq!(
            provider.automation.last_checked_in_at,
            Some("1782460800000".to_string())
        );
    }
}
