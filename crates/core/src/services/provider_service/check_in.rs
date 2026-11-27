use crate::services::check_in_tasks::{CheckInContext, HTTP_SLOTS};
use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{
        check_in_message_indicates_disabled, provider_domain, AppSettings, CheckInError,
        CheckInPhase, Provider, ProviderCheckInRecord, ProviderCheckInRecordsResult,
        ProviderCheckInResult, ProviderQuotaDisplay, ProviderStatus,
    },
    util::unix_millis as current_timestamp_millis,
};
use std::time::Duration;

use super::{
    find_provider, refresh::apply_refresh_owned_fields, MutationDecision, ProviderRequestContext,
    ProviderService,
};

impl ProviderService {
    pub async fn check_in_records(
        &self,
        id: String,
        month: String,
    ) -> Result<ProviderCheckInRecordsResult, String> {
        let state = crate::host::core_state();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        let provider = self
            .prepare_operation_provider(&data.settings, &provider)
            .await?;
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

    pub async fn check_in_attempt(
        &self,
        id: String,
        task: &CheckInContext,
    ) -> Result<ProviderCheckInResult, CheckInError> {
        task.phase(CheckInPhase::Checking);
        let data = self.snapshot_async().await?;
        let provider = find_provider(&data, &id)?;
        provider_domain::check_in::validate_credentials(&provider)?;
        let browser_account = if provider_domain::check_in::effective_method(&provider)
            == crate::models::ProviderCheckInMethod::FreshLogin
        {
            match provider_domain::check_in::fresh_login_route(&provider)? {
                provider_domain::check_in::FreshLoginRoute::BrowserAccount(id) => Some(id),
                provider_domain::check_in::FreshLoginRoute::Password => None,
            }
        } else {
            None
        };
        let (mut effective_provider, mut result) = if let Some(account_id) = browser_account {
            let authenticated = crate::services::provider_browser_login::check_in(
                &data.settings,
                &provider,
                account_id,
                task,
            )
            .await?;
            (authenticated, ProviderCheckInResult::confirmed_login())
        } else {
            match self
                .request_http_check_in(&data.settings, &provider, task)
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    // 标准签到带着已保存的会话提交；会话过期时站点只回一行
                    // 「未登录」。绑定了登录账号的卡就地重新登录再重试一次，
                    // 用户不必自己再走一遍「登录并导入」。重试成功仍走普通
                    // 签到收尾：真实业务动作是重试的这次签到，额度差照常计算。
                    let Some(account_id) =
                        relogin_account_after_credential_failure(&provider, &error)
                    else {
                        return Err(error);
                    };
                    let authenticated = crate::services::provider_browser_login::check_in(
                        &data.settings,
                        &provider,
                        account_id,
                        task,
                    )
                    .await?;
                    self.request_http_check_in(&data.settings, &authenticated, task)
                        .await?
                }
            }
        };
        task.authenticated(&effective_provider);
        let mut browser_assisted =
            browser_account.is_some() || result.verification_required.is_some();
        if let Some(verification) = result.verification_required {
            let (provider, browser_result) = self
                .complete_browser_check_in(
                    &data.settings,
                    effective_provider,
                    crate::models::CheckInVerificationRequest {
                        kind: verification,
                        requires_login: result.verification_requires_login,
                    },
                    task,
                )
                .await?;
            effective_provider = provider;
            result = browser_result;
            browser_assisted = true;
        }
        // 浏览器挑战通过后站点仍可能回 401（会话在挑战期间过期，或
        // AnyRouter 一类站点只接受新的登录会话）。绑定了登录账号的卡可以
        // 再自动登录一次，并让浏览器重新执行签到；不再让用户自己重走流程。
        if let Some(error) = check_in_result_credential_failure(&result) {
            if let Some(account_id) =
                relogin_account_after_credential_failure(&effective_provider, &error)
            {
                let authenticated = crate::services::provider_browser_login::check_in(
                    &data.settings,
                    &effective_provider,
                    account_id,
                    task,
                )
                .await?;
                let (provider, browser_result) = self
                    .complete_browser_check_in(
                        &data.settings,
                        authenticated,
                        crate::models::CheckInVerificationRequest {
                            kind: crate::models::ProviderCheckInVerification::Connection,
                            requires_login: false,
                        },
                        task,
                    )
                    .await?;
                effective_provider = provider;
                result = browser_result;
                browser_assisted = true;
            }
        }
        let refresh_id = browser_assisted.then(|| id.clone());
        let result = self
            .finish_check_in(
                id,
                &data.settings,
                effective_provider,
                result,
                browser_assisted,
                task,
            )
            .await?;
        if let Some(id) = refresh_id.filter(|_| result.ok) {
            self.refresh_after_browser_check_in(id);
        }
        Ok(result)
    }

    /// Browser-assisted check-ins already confirmed the business result, so the
    /// task must not wait for another full refresh. The card still needs the
    /// renewed session's quota and status, though; refresh once in the
    /// background after the result is saved.
    fn refresh_after_browser_check_in(&self, id: String) {
        crate::runtime::spawn(async move {
            let _ = ProviderService::new()
                .refresh_by_ids(vec![id.clone()])
                .await;
            // refresh_by_ids 不广播事件，而签到终态事件在刷新完成之前就已
            // 发出；不在这里补一次，卡片会一直显示签到前的余额和状态。
            crate::app_events::emit_providers_changed(std::slice::from_ref(&id));
        });
    }

    async fn request_http_check_in(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        task: &CheckInContext,
    ) -> Result<(Provider, ProviderCheckInResult), CheckInError> {
        let _http_slot = HTTP_SLOTS.acquire().await.map_err(|_| "签到队列不可用")?;
        let state = crate::host::core_state();
        let _network_gate = state.refresh_gate.lock().await;
        let provider = self
            .current_operation_provider(&ProviderRequestContext::capture(provider))
            .await?
            .filter(|provider| provider.runtime.enabled)
            .ok_or("账号配置已变更，已停止本次签到")?;
        // Login itself is the business action for these sites; do not perform
        // an extra preflight password login that would already check in.
        let provider = if provider_domain::check_in::effective_method(&provider)
            == crate::models::ProviderCheckInMethod::FreshLogin
            || (provider.auth.mode == crate::models::AuthMode::Password
                && provider.auth.new_api_session.is_none())
        {
            provider
        } else {
            self.prepare_operation_provider(settings, &provider).await?
        };
        let request_context = ProviderRequestContext::capture(&provider);
        task.authenticated(&provider);
        let adapter = ProtocolAdapter;
        task.phase(CheckInPhase::Requesting);
        let operation = tokio::time::timeout(
            Duration::from_secs(120),
            adapter.check_in(settings, &provider),
        )
        .await
        .map_err(|_| {
            CheckInError::Unconfirmed("签到请求超时；结果可能已提交，请查看站点记录".to_string())
        })??;
        let effective_provider = self
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
        // 会话签到/标准签到会把 401 作为“失败结果”返回，而不是错误。
        // 统一收口成错误，让上面的绑定账号重登与重试逻辑也能处理这类响应。
        if let Some(error) = check_in_result_credential_failure(&operation.value) {
            return Err(error);
        }
        Ok((effective_provider, operation.value))
    }

    async fn complete_browser_check_in(
        &self,
        settings: &AppSettings,
        provider: Provider,
        verification: crate::models::CheckInVerificationRequest,
        task: &CheckInContext,
    ) -> Result<(Provider, ProviderCheckInResult), CheckInError> {
        let browser_operation =
            crate::services::browser_check_in::run(settings, &provider, verification, task).await?;
        let provider = self
            .persist_operation_credentials(
                &ProviderRequestContext::capture(&provider),
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
        task.authenticated(&provider);
        Ok((provider, browser_operation.value))
    }

    async fn finish_check_in(
        &self,
        id: String,
        settings: &AppSettings,
        effective_provider: Provider,
        mut result: ProviderCheckInResult,
        browser_assisted: bool,
        task: &CheckInContext,
    ) -> Result<ProviderCheckInResult, CheckInError> {
        if result.unconfirmed {
            return Err(CheckInError::Unconfirmed(result.message));
        }
        let mutation_context = ProviderRequestContext::capture(&effective_provider);
        // Browser requests already confirmed the result. A full HTTP refresh
        // would immediately re-enter the same wall and create unrelated traffic.
        let refreshed_provider = if result.ok && !browser_assisted {
            let state = crate::host::core_state();
            let _network_gate = state.refresh_gate.lock().await;
            let refresh_outcome = ProtocolAdapter
                .refresh_provider(settings, &effective_provider)
                .await;
            let refresh_succeeded = refresh_outcome.failure.is_none();
            let mut refreshed = effective_provider.clone();
            refresh_outcome.apply_to(&mut refreshed);
            if refresh_succeeded {
                // 刷新观测会带上签到前的冷却/连续失败快照。签到已经成功，
                // 继续把这些字段复制回去会让卡片显示“冷却中”，自动刷新也
                // 被已经恢复的旧失败挡住。
                provider_domain::failure::record_attempt(&mut refreshed, None, 0, 0);
            }
            Some(refreshed)
        } else {
            None
        };

        if result.ok {
            task.phase(CheckInPhase::Saving);
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
                        stored.identity.id == provider_id
                            && mutation_context.matches_check_in(stored)
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
                        upsert_local_check_in_record(
                            stored_provider,
                            stored_record,
                            data.settings.check_in_record_limit as usize,
                        );
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
                    stored.identity.id == provider_id
                        && mutation_context_for_probe.matches_check_in(stored)
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

fn upsert_local_check_in_record(
    provider: &mut Provider,
    record: ProviderCheckInRecord,
    record_limit: usize,
) {
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

    if provider.automation.check_in_records.len() > record_limit {
        let remove_count = provider.automation.check_in_records.len() - record_limit;
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

/// 会话/标准签到把认证失败放在 `ProviderCheckInResult.message` 里。只有
/// 明确是凭据问题才转成错误交给重登链路；权限不足、验证未完成等结果保持
/// 原样，避免用重新登录掩盖站点侧的规则问题。
fn check_in_result_credential_failure(result: &ProviderCheckInResult) -> Option<CheckInError> {
    if result.ok || result.unconfirmed || result.verification_required.is_some() {
        return None;
    }
    provider_domain::failure::looks_like_credential_rejection(&result.message)
        .then(|| CheckInError::Failed(result.message.clone()))
}

/// 标准签到被站点明确拒绝凭据时，是否可以用绑定的浏览器账号就地重新登录。
/// 这里只判断业务条件；浏览器运行时就绪与否由调用方检查，避免把登录窗口
/// 开在一个根本起不来的环境里。
fn credential_failure_relogin_account<'a>(
    provider: &'a Provider,
    error: &CheckInError,
) -> Option<&'a str> {
    let CheckInError::Failed(message) = error else {
        return None;
    };
    if !provider_domain::failure::looks_like_credential_rejection(message) {
        return None;
    }
    match provider_domain::check_in::fresh_login_route(provider).ok()? {
        provider_domain::check_in::FreshLoginRoute::BrowserAccount(account_id) => Some(account_id),
        provider_domain::check_in::FreshLoginRoute::Password => None,
    }
}

fn relogin_account_after_credential_failure<'a>(
    provider: &'a Provider,
    error: &CheckInError,
) -> Option<&'a str> {
    credential_failure_relogin_account(provider, error).filter(|_| {
        crate::services::browser_runtime::status(false).is_ok_and(|status| status.ready)
    })
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

    fn provider_with_browser_binding() -> Provider {
        let mut provider =
            Provider::from_input(ProviderInput::default(), "provider-test".to_string());
        provider.auth.api_user = "42".to_string();
        provider.auth.browser_binding = Some(crate::models::BrowserLoginBinding {
            account_id: Some("linux-do-1".to_string()),
            platform: crate::models::LoginPlatform::LinuxDo,
            mechanism: crate::models::BrowserLoginMechanism::Oauth,
            imported_at: 0,
        });
        provider
    }

    #[test]
    fn rejected_credentials_on_a_bound_card_fall_back_to_browser_relogin() {
        let provider = provider_with_browser_binding();
        assert_eq!(
            credential_failure_relogin_account(
                &provider,
                &CheckInError::Failed("未登录".to_string())
            ),
            Some("linux-do-1")
        );
        // 网络故障不触发登录窗口，保持原有失败信息交给自动重试。
        assert_eq!(
            credential_failure_relogin_account(
                &provider,
                &CheckInError::Failed("HTTP 503: maintenance".to_string())
            ),
            None
        );
        // 结果可能已提交时不能重新登录：那会把一次未确认的签到变成两次。
        assert_eq!(
            credential_failure_relogin_account(
                &provider,
                &CheckInError::Unconfirmed("签到请求超时".to_string())
            ),
            None
        );
        // 没有绑定浏览器账号的卡只能由用户手动补全凭据。
        let mut unbound = provider_with_browser_binding();
        unbound.auth.browser_binding = None;
        assert_eq!(
            credential_failure_relogin_account(
                &unbound,
                &CheckInError::Failed("未登录".to_string())
            ),
            None
        );
    }

    fn failed_check_in(message: &str) -> ProviderCheckInResult {
        ProviderCheckInResult {
            ok: false,
            message: message.to_string(),
            verification_required: None,
            verification_requires_login: false,
            unconfirmed: false,
            last_checked_in_at: None,
            last_check_in_user: None,
            quota_delta: None,
        }
    }

    #[test]
    fn auth_failure_results_trigger_relogin_but_permission_errors_do_not() {
        assert!(matches!(
            check_in_result_credential_failure(&failed_check_in(
                "认证无效(401): session expired"
            )),
            Some(CheckInError::Failed(message)) if message.contains("401")
        ));
        assert!(matches!(
            check_in_result_credential_failure(&failed_check_in(
                "Unauthorized, invalid access token"
            )),
            Some(CheckInError::Failed(_))
        ));
        // 每日奖励签到要求当天新会话；重新登录后重试即可。
        assert!(matches!(
            check_in_result_credential_failure(&failed_check_in(
                "Check-in requires a session created today. Please sign in again and retry."
            )),
            Some(CheckInError::Failed(_))
        ));
        // 账号登录有效但站点不允许签到；重新登录无法解决，不能开登录窗口。
        assert!(check_in_result_credential_failure(&failed_check_in(
            "Unauthorized, insufficient privileges"
        ))
        .is_none());
        // 需要浏览器验证的结果不能当成凭据失败。
        let mut verification = failed_check_in("认证无效(401)");
        verification.verification_required =
            Some(crate::models::ProviderCheckInVerification::Connection);
        assert!(check_in_result_credential_failure(&verification).is_none());
        let mut unconfirmed = failed_check_in("认证无效(401)");
        unconfirmed.unconfirmed = true;
        assert!(check_in_result_credential_failure(&unconfirmed).is_none());
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
        upsert_local_check_in_record(&mut provider, first, 730);

        let mut repeated = local_check_in_record("1782460900000", "今日已签到", None);
        repeated.date = "2026-06-26".to_string();
        upsert_local_check_in_record(&mut provider, repeated, 730);

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
