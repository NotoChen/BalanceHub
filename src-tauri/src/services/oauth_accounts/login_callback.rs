use crate::{adapters::oauth::tokens, models::OAuthVendor};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

/// 回调端口取决于上游客户端的公开登记，不能自己随便挑。对同一个 `client_id` 实测：
/// `auth.openai.com` 只接受 1455 和 1457，其它端口（1456、43219 等）一律
/// `invalid_authorize_request`；Claude 的授权服务器接受任意 loopback 端口。
/// 因此 Codex 只能在登记的端口之间换，Claude 在首选端口被占用时可以改随机端口。
const CODEX_CALLBACK_PORTS: &[u16] = &[1455, 1457];
const CLAUDE_CALLBACK_PORT: u16 = 54545;

/// 授权回调端口的可用范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallbackPorts {
    /// 上游只登记了这几个端口；被占用时只能换本机地址族，不能换端口。
    Registered(&'static [u16]),
    /// 上游接受任意 loopback 端口：优先用登记的端口，被占用时改由系统分配。
    Flexible(u16),
}

impl CallbackPorts {
    /// 一次授权实际使用的首选端口。
    pub(super) fn preferred(self) -> u16 {
        match self {
            Self::Registered(ports) => ports.first().copied().unwrap_or(0),
            Self::Flexible(port) => port,
        }
    }

    /// 端口全被占用时告知用户的端口说明。
    pub(super) fn label(self) -> String {
        match self {
            Self::Registered(ports) => ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(" 和 "),
            Self::Flexible(port) => port.to_string(),
        }
    }
}

/// 需要浏览器回连的授权方式返回 `(路径, 端口范围)`；粘贴授权码或设备码的形态返回 `None`。
pub(super) fn loopback_spec(vendor: OAuthVendor) -> Option<(&'static str, CallbackPorts)> {
    match vendor {
        OAuthVendor::Codex => Some((
            "/auth/callback",
            CallbackPorts::Registered(CODEX_CALLBACK_PORTS),
        )),
        OAuthVendor::Claude => Some(("/callback", CallbackPorts::Flexible(CLAUDE_CALLBACK_PORT))),
        OAuthVendor::Gemini | OAuthVendor::Grok => None,
    }
}

pub(super) fn loopback_redirect(path: &str, port: u16) -> String {
    format!("http://localhost:{port}{path}")
}

/// Gemini CLI 的 headless 流程在页面上直接给出授权码，由用户粘贴回来。
pub(super) fn manual_redirect(vendor: OAuthVendor) -> String {
    match vendor {
        OAuthVendor::Gemini => "https://codeassist.google.com/authcode".to_string(),
        _ => String::new(),
    }
}

pub(super) fn authorization_url(
    vendor: OAuthVendor,
    state: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<String, String> {
    let endpoint = match vendor {
        OAuthVendor::Codex => "https://auth.openai.com/oauth/authorize",
        OAuthVendor::Claude => "https://claude.ai/oauth/authorize",
        OAuthVendor::Gemini => "https://accounts.google.com/o/oauth2/v2/auth",
        OAuthVendor::Grok => return Err("Grok 使用设备授权".into()),
    };
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = Url::parse(endpoint).map_err(|_| "授权地址无效")?;
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            ("response_type", "code"),
            ("client_id", tokens::client_id(vendor)),
            ("redirect_uri", redirect_uri),
            ("scope", tokens::scopes(vendor)),
            ("state", state),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        match vendor {
            OAuthVendor::Codex => {
                query.extend_pairs([
                    ("id_token_add_organizations", "true"),
                    ("codex_cli_simplified_flow", "true"),
                    ("prompt", "login"),
                ]);
            }
            OAuthVendor::Claude => {
                query.append_pair("code", "true");
            }
            OAuthVendor::Gemini => {
                query.extend_pairs([
                    ("access_type", "offline"),
                    ("prompt", "consent"),
                    ("include_granted_scopes", "true"),
                ]);
            }
            OAuthVendor::Grok => unreachable!(),
        }
    }
    Ok(url.into())
}

/// 回调地址由会话给出而不是按厂商写死：Claude 的端口可能因为被占用而改成随机端口。
pub(super) fn parse_code(
    input: &str,
    vendor: OAuthVendor,
    state: &str,
    redirect: &str,
) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 8192 || input.chars().any(char::is_control) {
        return Err("请粘贴本次授权返回的完整地址或授权码".into());
    }
    // A manually delivered Google code is bound to this session by PKCE.
    if vendor == OAuthVendor::Gemini
        && !input.contains("://")
        && !input.chars().any(char::is_whitespace)
    {
        return Ok(input.to_owned());
    }
    if vendor == OAuthVendor::Claude {
        if let Some((code, returned_state)) = input.split_once('#') {
            if !code.contains("://") && returned_state == state && !code.is_empty() {
                return Ok(code.to_owned());
            }
        }
    }
    let url = Url::parse(input).map_err(|_| "请粘贴浏览器地址栏中的完整回调地址")?;
    let expected = Url::parse(redirect).map_err(|_| "此授权不接受回调地址")?;
    if url.scheme() != expected.scheme()
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != expected.port_or_known_default()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("回调地址与本次授权不匹配".into());
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let one = |key: &str| -> Result<Option<String>, String> {
        let values: Vec<_> = pairs.iter().filter(|(k, _)| k == key).collect();
        if values.len() > 1 {
            return Err("回调地址包含重复参数".into());
        }
        Ok(values.first().map(|(_, v)| v.to_string()))
    };
    if one("state")?.as_deref() != Some(state) {
        return Err("授权状态不匹配，请使用当前窗口发起的授权".into());
    }
    if one("error")?.is_some() {
        return Err("授权未被允许，请重新发起授权".into());
    }
    one("code")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "回调地址缺少授权码".into())
}

pub(super) async fn receive(
    mut socket: TcpStream,
    vendor: OAuthVendor,
    state: &str,
    redirect: &str,
) -> Option<String> {
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut request = Vec::new();
        loop {
            let mut chunk = [0u8; 1024];
            let size = socket.read(&mut chunk).await.ok()?;
            if size == 0 || request.len() + size > 12 * 1024 {
                return None;
            }
            request.extend_from_slice(&chunk[..size]);
            if request.windows(4).any(|v| v == b"\r\n\r\n") {
                break;
            }
        }
        let request = std::str::from_utf8(&request).ok()?;
        let mut line = request.lines().next()?.split_whitespace();
        if line.next()? != "GET" {
            return None;
        }
        let path = line.next()?;
        if !path.starts_with('/') || path.starts_with("//") {
            return None;
        }
        let base = Url::parse(redirect).ok()?;
        let url = base.join(path).ok()?;
        parse_code(url.as_str(), vendor, state, redirect).ok()
    })
    .await
    .ok()
    .flatten();
    let (status, body) = if result.is_some() {
        ("200 OK", "<!doctype html><meta charset=utf-8><title>BalanceHub</title><p>已收到授权，请返回 BalanceHub 查看保存结果。可以关闭此页面。</p>")
    } else {
        ("400 Bad Request", "<!doctype html><meta charset=utf-8><title>BalanceHub</title><p>此回调不属于当前授权。请返回 BalanceHub 重新操作。</p>")
    };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        socket.write_all(response.as_bytes()),
    )
    .await;
    result
}
