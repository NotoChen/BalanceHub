use super::{
    already_checked_in,
    challenge::{parse_checked_in, verification_required, verification_result},
    executor::{Credentials, Executor},
    parse_check_in_response,
};
use crate::{
    adapters::new_api::http::build_url,
    models::{
        CheckInError, CheckInPhase, Provider, ProviderCheckInResult, ProviderCheckInVerification,
    },
    util::current_month,
};
use reqwest::{Method, Url};
use serde_json::Value;

pub(super) async fn run(
    executor: &mut Executor<'_>,
    provider: &Provider,
    mut needs_token: bool,
) -> Result<ProviderCheckInResult, CheckInError> {
    let status_url = build_url(
        &provider.identity.base_url,
        &format!("/api/user/checkin?month={}", current_month()),
    )?;
    for attempt in 0..3 {
        executor.phase(CheckInPhase::Checking);
        let before = match executor.read(provider, &status_url, true).await {
            Ok(response) => response,
            Err(error) if !executor.is_browser() && provider.automation.auto_shield => {
                let _ = error;
                return Ok(verification_result(ProviderCheckInVerification::Connection));
            }
            Err(error) => return Err(error),
        };
        if let Some(kind) = verification_required(before.status, &before.headers, &before.body) {
            if !executor
                .retry_verification(provider, kind, &status_url)
                .await?
            {
                return Ok(verification_result(kind));
            }
            continue;
        }
        if !executor.is_browser()
            && provider.automation.auto_shield
            && before.status.is_success()
            && serde_json::from_str::<Value>(&before.body).is_err()
        {
            return Ok(verification_result(ProviderCheckInVerification::Connection));
        }
        if parse_checked_in(before.status, &before.body)? {
            return Ok(already_checked_in());
        }
        let mut submit_url = build_url(&provider.identity.base_url, "/api/user/checkin")?;
        if needs_token {
            let Some(token) = executor.turnstile_token(provider).await? else {
                return Ok(verification_result(ProviderCheckInVerification::Turnstile));
            };
            submit_url
                .query_pairs_mut()
                .append_pair("turnstile", &token);
            // Human verification may take minutes; do not submit after another client won.
            let latest = executor.read(provider, &status_url, true).await?;
            if parse_checked_in(latest.status, &latest.body)? {
                return Ok(already_checked_in());
            }
        }
        executor.phase(CheckInPhase::Requesting);
        let response = executor
            .send(
                provider,
                &submit_url,
                Method::POST,
                Credentials::Account,
                Some(String::new()),
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                if confirmed(executor, provider, &status_url).await {
                    return Ok(already_checked_in());
                }
                return Err(CheckInError::Unconfirmed(
                    "签到提交后连接中断，结果尚未确认；已停止自动重试，请查看站点记录".into(),
                ));
            }
        };
        if let Some(kind) =
            verification_required(response.status, &response.headers, &response.body)
        {
            if !executor
                .retry_verification(provider, kind, &status_url)
                .await?
            {
                return Ok(verification_result(kind));
            }
            if attempt == 2 {
                return Err("完成验证后站点仍拒绝签到，已停止重试".into());
            }
            needs_token |= kind == ProviderCheckInVerification::Turnstile;
            continue;
        }
        let mut result = parse_check_in_response(response.status, &response.body);
        let unknown_response =
            response.status.is_success() && serde_json::from_str::<Value>(&response.body).is_err();
        if !result.ok && !unknown_response {
            return Ok(result);
        }
        if !confirmed(executor, provider, &status_url).await {
            return Err(CheckInError::Unconfirmed(
                "签到请求已返回，但回读未能确认今日记录；已停止重试，请查看站点记录".into(),
            ));
        }
        if unknown_response {
            result = already_checked_in();
        }
        result.message = format!("{}（已向站点确认）", result.message);
        return Ok(result);
    }
    Err("签到验证未完成，已停止重试".into())
}

async fn confirmed(executor: &mut Executor<'_>, provider: &Provider, status_url: &Url) -> bool {
    executor.phase(CheckInPhase::VerifyingResult);
    executor
        .read(provider, status_url, true)
        .await
        .is_ok_and(|response| parse_checked_in(response.status, &response.body) == Ok(true))
}
