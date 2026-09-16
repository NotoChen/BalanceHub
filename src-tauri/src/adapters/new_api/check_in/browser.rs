use super::{
    already_checked_in,
    challenge::{parse_checked_in, verification_required},
    parse_check_in_transport_response,
};
use crate::{
    adapters::{
        browser::BrowserSession,
        new_api::http::{auth_header_values, build_url, normalize_base_url},
        transport::TransportResponse,
    },
    models::{
        CheckInError, CheckInPhase, Provider, ProviderCheckInResult, ProviderCheckInVerification,
    },
    util::current_month,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) async fn check_in_with_browser(
    session: &mut BrowserSession,
    provider: &Provider,
    initial_verification: ProviderCheckInVerification,
    ensure_current: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<ProviderCheckInResult, CheckInError> {
    let base = normalize_base_url(&provider.identity.base_url);
    let status_url = build_url(
        &base,
        &format!("/api/user/checkin?month={}", current_month()),
    )?;
    let submit_url = build_url(&base, "/api/user/checkin")?;
    let public_url = build_url(&base, "/api/status")?;
    let mut headers = auth_header_values(provider)
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    headers.insert("content-type", "application/json".to_string());
    headers.insert("accept", "application/json".to_string());
    let headers = serde_json::to_value(headers).map_err(|_| "无法构造签到认证请求")?;
    let mut needs_turnstile = initial_verification == ProviderCheckInVerification::Turnstile;

    for attempt in 0..3 {
        let before = read(session, status_url.as_str(), &headers, ensure_current).await?;
        if parse_checked_in(before.status, &before.body)? {
            return Ok(already_checked_in());
        }
        let mut url = submit_url.clone();
        if needs_turnstile {
            let token = turnstile_token(session, public_url.as_str(), ensure_current).await?;
            url.query_pairs_mut().append_pair("turnstile", &token);
            // Human verification can take minutes; another client may have signed in.
            let latest = read(session, status_url.as_str(), &headers, ensure_current).await?;
            if parse_checked_in(latest.status, &latest.body)? {
                return Ok(already_checked_in());
            }
        }
        ensure_current()?;
        session.phase(CheckInPhase::Requesting);
        let response = session.fetch(url.as_str(), "POST", &headers).await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                session.phase(CheckInPhase::VerifyingResult);
                let confirmed = read(session, status_url.as_str(), &headers, ensure_current)
                    .await
                    .and_then(|response| {
                        parse_checked_in(response.status, &response.body).map_err(Into::into)
                    });
                if confirmed == Ok(true) {
                    let mut result = already_checked_in();
                    result.message = "签到已由站点确认".to_string();
                    return Ok(result);
                }
                return Err(CheckInError::Unconfirmed(
                    "签到提交后连接中断，结果尚未确认；已停止自动重试，请查看站点记录".to_string(),
                ));
            }
        };
        let mut result = parse_check_in_transport_response(&response);
        match result.verification_required {
            Some(ProviderCheckInVerification::Turnstile) if attempt < 2 => {
                needs_turnstile = true;
                continue;
            }
            Some(ProviderCheckInVerification::Cloudflare) if attempt < 2 => {
                session
                    .request("navigate", json!({ "path": status_url.as_str() }))
                    .await?;
                continue;
            }
            Some(_) => return Err("完成浏览器验证后，站点仍拒绝签到；已停止重试".into()),
            None if !result.ok => return Ok(result),
            None => {}
        }
        session.phase(CheckInPhase::VerifyingResult);
        let confirmed = read(session, status_url.as_str(), &headers, ensure_current)
            .await
            .and_then(|response| {
                parse_checked_in(response.status, &response.body).map_err(Into::into)
            });
        if confirmed != Ok(true) {
            return Err(CheckInError::Unconfirmed(
                "站点返回签到成功，但回读未能确认今日记录；已停止重试，请查看站点记录".to_string(),
            ));
        }
        result.message = format!("{}（已向站点确认）", result.message);
        return Ok(result);
    }
    Err("签到验证未完成".into())
}

pub(crate) async fn turnstile_token(
    session: &mut BrowserSession,
    public_url: &str,
    ensure_current: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<String, CheckInError> {
    let metadata = read(session, public_url, &json!({}), ensure_current).await?;
    if !metadata.status.is_success() {
        return Err(format!("读取验证配置失败：HTTP {}", metadata.status.as_u16()).into());
    }
    let metadata: Value =
        serde_json::from_str(&metadata.body).map_err(|_| "站点没有返回有效的验证配置")?;
    let site_key = metadata
        .pointer("/data/turnstile_site_key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .ok_or("站点要求 Turnstile，但没有提供验证配置")?;
    let verification = session
        .request("verify", json!({ "siteKey": site_key }))
        .await?;
    verification
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "未取得签到验证结果".into())
}

async fn read(
    session: &mut BrowserSession,
    url: &str,
    headers: &Value,
    ensure_current: &(dyn Fn() -> Result<(), String> + Send + Sync),
) -> Result<TransportResponse, CheckInError> {
    ensure_current()?;
    let response = session.fetch(url, "GET", headers).await?;
    if verification_required(response.status, &response.headers, &response.body)
        == Some(ProviderCheckInVerification::Cloudflare)
    {
        session.request("navigate", json!({ "path": url })).await?;
        ensure_current()?;
        return session.fetch(url, "GET", headers).await;
    }
    Ok(response)
}
