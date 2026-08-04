//! Provider shield detection, solving, and bounded credential storage.
//!
//! This module deliberately does not own business authentication. Shield
//! cookies are allow-listed and scoped to a provider, origin, proxy route, and
//! shield kind so they can never replace a user's session cookie.

pub(crate) mod aliyun;
pub(crate) mod cloudflare;
mod store;

use crate::util::unix_secs;
use reqwest::{header::HeaderMap, Url};
use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) use cloudflare::{init, ChallengeMode, WEBVIEW_USER_AGENT};

const SHIELD_KINDS: [ShieldKind; 2] = [ShieldKind::AliyunWaf, ShieldKind::Cloudflare];

/// Shield types are detected from the response and dispatched to their solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShieldKind {
    AliyunWaf,
    Cloudflare,
}

impl ShieldKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::AliyunWaf => "阿里云 WAF 验证",
            Self::Cloudflare => "Cloudflare 人机验证",
        }
    }

    pub(crate) fn may_need_interaction(self) -> bool {
        matches!(self, Self::Cloudflare)
    }
}

/// Context shared by all requests belonging to one provider operation.
/// `webview_proxy_error` is kept as a capability error: normal HTTP requests
/// remain usable even when Cloudflare cannot safely share their proxy route.
#[derive(Debug, Clone)]
pub(crate) struct ShieldContext {
    pub provider_id: String,
    pub proxy_fingerprint: String,
    pub webview_proxy_url: Option<Url>,
    pub webview_proxy_error: Option<String>,
}

impl ShieldContext {
    pub(crate) fn new(
        provider_id: impl Into<String>,
        proxy_fingerprint: String,
        webview_proxy: Result<Option<Url>, String>,
    ) -> Self {
        let (webview_proxy_url, webview_proxy_error) = match webview_proxy {
            Ok(url) => (url, None),
            Err(error) => (None, Some(error)),
        };
        Self {
            provider_id: provider_id.into(),
            proxy_fingerprint,
            webview_proxy_url,
            webview_proxy_error,
        }
    }

    fn cache_key(&self, url: &Url, kind: ShieldKind) -> CacheKey {
        CacheKey {
            provider_id: self.provider_id.clone(),
            origin: origin_key(url),
            proxy_fingerprint: self.proxy_fingerprint.clone(),
            kind,
        }
    }
}

/// A shield credential contains only cookies known to belong to that shield.
#[derive(Debug, Clone)]
pub(crate) struct ShieldCredential {
    pub cookies: BTreeMap<String, String>,
    pub user_agent: Option<String>,
    pub acquired_at: u64,
}

impl ShieldCredential {
    pub(crate) fn from_pairs(
        kind: ShieldKind,
        pairs: impl IntoIterator<Item = (String, String)>,
        user_agent: Option<String>,
    ) -> Self {
        let cookies = pairs
            .into_iter()
            .filter(|(name, value)| !value.trim().is_empty() && cookie_name_allowed(kind, name))
            .collect();
        Self {
            cookies,
            user_agent,
            acquired_at: unix_secs(),
        }
    }

    pub(crate) fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn same_material(&self, other: &Self) -> bool {
        self.cookies == other.cookies && self.user_agent == other.user_agent
    }
}

/// Context captured from a response that hit a shield.
pub(crate) struct ShieldHit {
    pub kind: ShieldKind,
    pub url: Url,
    pub body: String,
    pub set_cookies: Vec<String>,
}

/// Ephemeral UI state. It is kept in memory only and is never written to the
/// installed application's provider configuration.
#[derive(Debug, Clone)]
pub(crate) struct ChallengeState {
    pub provider_id: String,
    pub kind: ShieldKind,
    pub url: String,
    pub recorded_at: u64,
}

pub(crate) fn detect(headers: &HeaderMap, body: &str) -> Option<ShieldKind> {
    if cloudflare::matches(headers, body) {
        return Some(ShieldKind::Cloudflare);
    }
    if aliyun::matches(body) {
        return Some(ShieldKind::AliyunWaf);
    }
    None
}

pub(crate) fn hit_from_response(
    kind: ShieldKind,
    url: &Url,
    headers: &HeaderMap,
    body: &str,
) -> ShieldHit {
    ShieldHit {
        kind,
        url: url.clone(),
        body: body.to_string(),
        set_cookies: headers
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect(),
    }
}

/// Solve a response-triggered challenge once, coalescing concurrent requests.
pub(crate) async fn solve(
    context: &ShieldContext,
    hit: &ShieldHit,
    mode: ChallengeMode,
) -> Result<ShieldCredential, String> {
    let key = context.cache_key(&hit.url, hit.kind);
    let lock = store::lock_for(&key);
    let _guard = lock.lock().await;
    if let Some(credential) = store::cached(&key) {
        return Ok(credential);
    }

    if hit.kind == ShieldKind::Cloudflare {
        mark_challenge(context, hit.kind, &hit.url);
    }
    let credential = match hit.kind {
        ShieldKind::AliyunWaf => aliyun::solve(&hit.body, &hit.set_cookies)?,
        ShieldKind::Cloudflare => {
            let proxy_error = context.webview_proxy_error.as_deref();
            if let Some(error) = proxy_error {
                return Err(error.to_string());
            }
            cloudflare::solve(&hit.url, mode, context.webview_proxy_url.as_ref()).await?
        }
    };
    store::store(key, credential.clone());
    Ok(credential)
}

/// Solve the currently recorded interactive challenge for a provider.
pub(crate) async fn solve_interactively(
    context: &ShieldContext,
    state: &ChallengeState,
) -> Result<ShieldCredential, String> {
    if !state.kind.may_need_interaction() {
        return Err(format!(
            "{}无需人工验证，命中时会自动通过",
            state.kind.label()
        ));
    }
    if let Some(error) = context.webview_proxy_error.as_deref() {
        return Err(error.to_string());
    }

    let url = Url::parse(&state.url).map_err(|error| format!("挑战地址无效: {error}"))?;
    let key = context.cache_key(&url, state.kind);
    let lock = store::lock_for(&key);
    let _guard = lock.lock().await;
    if let Some(credential) = store::cached(&key) {
        clear_challenge(&context.provider_id);
        return Ok(credential);
    }
    let credential = cloudflare::solve(
        &url,
        ChallengeMode::Interactive,
        context.webview_proxy_url.as_ref(),
    )
    .await?;
    store::store(key, credential.clone());
    clear_challenge(&context.provider_id);
    Ok(credential)
}

pub(crate) fn cached_credentials(
    context: &ShieldContext,
    url: &Url,
) -> Vec<(ShieldKind, ShieldCredential)> {
    SHIELD_KINDS
        .into_iter()
        .filter_map(|kind| {
            store::cached(&context.cache_key(url, kind)).map(|credential| (kind, credential))
        })
        .collect()
}

pub(crate) fn invalidate_if_matches(
    context: &ShieldContext,
    url: &Url,
    kind: ShieldKind,
    credential: &ShieldCredential,
) {
    store::invalidate_if_matches(&context.cache_key(url, kind), credential);
}

pub(crate) fn mark_challenge(context: &ShieldContext, kind: ShieldKind, url: &Url) {
    if store::mark_challenge(ChallengeState {
        provider_id: context.provider_id.clone(),
        kind,
        url: url.to_string(),
        recorded_at: unix_secs(),
    }) {
        cloudflare::notify_provider_views_changed();
    }
}

pub(crate) fn challenge_for(provider_id: &str) -> Option<ChallengeState> {
    store::challenge_for(provider_id)
}

pub(crate) fn clear_challenge(provider_id: &str) {
    if store::clear_challenge(provider_id) {
        cloudflare::notify_provider_views_changed();
    }
}

pub(crate) fn cookie_name_allowed(kind: ShieldKind, name: &str) -> bool {
    let name = name.trim();
    match kind {
        ShieldKind::AliyunWaf => matches!(name, "acw_tc" | "acw_sc__v2"),
        ShieldKind::Cloudflare => {
            name == "cf_clearance" || name == "__cf_bm" || name.starts_with("cf_chl_")
        }
    }
}

pub(crate) fn ttl_secs(kind: ShieldKind) -> u64 {
    match kind {
        ShieldKind::AliyunWaf => 10 * 60,
        ShieldKind::Cloudflare => 20 * 60,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CacheKey {
    provider_id: String,
    origin: String,
    proxy_fingerprint: String,
    kind: ShieldKind,
}

fn origin_key(url: &Url) -> String {
    url.origin().ascii_serialization()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).unwrap());
        headers
    }

    fn context() -> ShieldContext {
        ShieldContext::new("provider-1", "proxy-a".to_string(), Ok(None))
    }

    #[test]
    fn detects_header_only_cloudflare_challenge() {
        assert_eq!(
            detect(&headers_with("cf-mitigated", "challenge"), ""),
            Some(ShieldKind::Cloudflare)
        );
    }

    #[test]
    fn keeps_only_known_shield_cookies() {
        let credential = ShieldCredential::from_pairs(
            ShieldKind::Cloudflare,
            [
                ("cf_clearance".to_string(), "abc".to_string()),
                ("session".to_string(), "must-not-enter".to_string()),
            ],
            Some("UA".to_string()),
        );
        assert_eq!(credential.cookie_header(), "cf_clearance=abc");
    }

    #[test]
    fn cache_key_separates_provider_origin_and_proxy() {
        let url = Url::parse("https://example.test/api/check").unwrap();
        let first = context().cache_key(&url, ShieldKind::Cloudflare);
        let second = ShieldContext::new("provider-2", "proxy-a".to_string(), Ok(None))
            .cache_key(&url, ShieldKind::Cloudflare);
        let third = ShieldContext::new("provider-1", "proxy-b".to_string(), Ok(None))
            .cache_key(&url, ShieldKind::Cloudflare);
        assert_ne!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn cache_key_normalizes_default_origin_ports() {
        let implicit = Url::parse("https://example.test/api").unwrap();
        let explicit = Url::parse("https://example.test:443/api").unwrap();
        assert_eq!(
            context().cache_key(&implicit, ShieldKind::Cloudflare),
            context().cache_key(&explicit, ShieldKind::Cloudflare)
        );
    }

    #[test]
    fn stale_response_cannot_invalidate_a_replaced_credential() {
        let context = ShieldContext::new(
            "conditional-invalidation-provider",
            "proxy-a".to_string(),
            Ok(None),
        );
        let url = Url::parse("https://conditional.example.test/api").unwrap();
        let key = context.cache_key(&url, ShieldKind::Cloudflare);
        let stale = ShieldCredential::from_pairs(
            ShieldKind::Cloudflare,
            [("cf_clearance".to_string(), "stale".to_string())],
            Some("UA".to_string()),
        );
        let fresh = ShieldCredential::from_pairs(
            ShieldKind::Cloudflare,
            [("cf_clearance".to_string(), "fresh".to_string())],
            Some("UA".to_string()),
        );

        store::store(key.clone(), fresh.clone());
        store::invalidate_if_matches(&key, &stale);
        assert_eq!(
            store::cached(&key).unwrap().cookie_header(),
            "cf_clearance=fresh"
        );

        store::invalidate_if_matches(&key, &fresh);
        assert!(store::cached(&key).is_none());
    }

    #[test]
    fn challenge_state_is_scoped_to_provider() {
        let first = context();
        mark_challenge(
            &first,
            ShieldKind::Cloudflare,
            &Url::parse("https://example.test/api").unwrap(),
        );
        assert!(challenge_for("provider-1").is_some());
        assert!(challenge_for("provider-2").is_none());
        clear_challenge("provider-1");
    }
}
