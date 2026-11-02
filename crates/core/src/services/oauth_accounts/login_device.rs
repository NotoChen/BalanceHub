use crate::{
    adapters::oauth::{http, tokens},
    models::{OAuthCredential, OAuthVendor, ProviderAuth, ProviderInput},
};
use reqwest::{Client, Url};
use serde_json::Value;

pub(super) struct Device {
    pub endpoint: String,
    pub code: String,
    pub user_code: String,
    pub url: String,
    pub interval: u64,
    pub expires: i64,
}
pub(super) enum Poll {
    Pending,
    SlowDown,
    Ready(Box<ProviderAuth>),
}

pub(super) async fn begin(client: &Client) -> Result<Device, String> {
    let (endpoint, token_endpoint) = tokens::discover_grok(client).await?;
    let data = http::json(
        client.post(endpoint).form(&[
            ("client_id", tokens::GROK_CLIENT_ID),
            ("scope", tokens::GROK_SCOPE),
        ]),
        "申请 Grok 设备授权",
    )
    .await?;
    let code = http::text(&data, &["device_code"]);
    let user_code = http::text(&data, &["user_code"]);
    let raw_url = http::text(&data, &["verification_uri_complete", "verification_uri"]);
    let url = Url::parse(&raw_url).map_err(|_| "Grok 验证地址无效")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !url
            .host_str()
            .is_some_and(|h| h == "x.ai" || h.ends_with(".x.ai"))
    {
        return Err("Grok 验证地址不属于官方服务".into());
    }
    if code.is_empty() || code.len() > 8192 || user_code.is_empty() || user_code.len() > 128 {
        return Err("Grok 未返回有效设备授权码".into());
    }
    let expires = data
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .ok_or("Grok 未返回授权有效期")?
        .min(1800);
    let interval = data
        .get("interval")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 300);
    Ok(Device {
        endpoint: token_endpoint,
        code,
        user_code,
        url: url.into(),
        interval,
        expires: chrono::Utc::now().timestamp() + expires,
    })
}

pub(super) async fn poll(client: &Client, device: &Device) -> Result<Poll, String> {
    let mut response = client
        .post(&device.endpoint)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device.code.as_str()),
            ("client_id", tokens::GROK_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|_| "查询 Grok 授权结果失败，请重新发起授权")?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "读取 Grok 授权结果失败")?
    {
        if bytes.len() + chunk.len() > 128 * 1024 {
            return Err("Grok 授权响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Grok 授权响应格式无效")?;
    match data.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(Poll::Pending),
        Some("slow_down") => return Ok(Poll::SlowDown),
        Some("expired_token") => return Err("Grok 授权码已过期，请重新发起授权".into()),
        Some("access_denied") => return Err("Grok 授权未被允许".into()),
        Some(_) => return Err("Grok 拒绝了本次授权，请重新发起授权".into()),
        None => {}
    }
    if !status.is_success() {
        return Err(http::http_error("Grok 授权", status.as_u16()));
    }
    let mut auth = ProviderInput::default().auth;
    auth.oauth = Some(OAuthCredential::new(OAuthVendor::Grok));
    Ok(Poll::Ready(Box::new(tokens::apply_response(auth, &data)?)))
}
