//! Provider shield detection and bounded credential storage.
//!
//! The only shield handled here is the deterministic Aliyun WAF challenge used
//! by AnyRouter. It is solved from the challenge response itself; no browser
//! window, WebView state, or manual verification is involved.

pub(crate) mod aliyun;
mod store;

use crate::util::unix_secs;
use reqwest::{header::HeaderMap, Url};
use serde::Serialize;
use std::collections::BTreeMap;

const SHIELD_KINDS: [ShieldKind; 1] = [ShieldKind::AliyunWaf];

/// Shield types are detected from the response and dispatched to their solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ShieldKind {
    AliyunWaf,
}

impl ShieldKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::AliyunWaf => "阿里云 WAF 验证",
        }
    }
}

/// Context shared by all requests belonging to one provider operation.
#[derive(Debug, Clone)]
pub(crate) struct ShieldContext {
    pub provider_id: String,
    pub proxy_fingerprint: String,
}

impl ShieldContext {
    pub(crate) fn new(provider_id: impl Into<String>, proxy_fingerprint: String) -> Self {
        Self {
            provider_id: provider_id.into(),
            proxy_fingerprint,
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
    pub acquired_at: u64,
}

impl ShieldCredential {
    pub(crate) fn from_pairs(
        kind: ShieldKind,
        pairs: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let cookies = pairs
            .into_iter()
            .filter(|(name, value)| !value.trim().is_empty() && cookie_name_allowed(kind, name))
            .collect();
        Self {
            cookies,
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
        self.cookies == other.cookies
    }
}

/// Context captured from a response that hit a shield.
pub(crate) struct ShieldHit {
    pub kind: ShieldKind,
    pub url: Url,
    pub body: String,
    pub set_cookies: Vec<String>,
}

pub(crate) fn detect(_headers: &HeaderMap, body: &str) -> Option<ShieldKind> {
    aliyun::matches(body).then_some(ShieldKind::AliyunWaf)
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

/// Solve a response-triggered WAF challenge once, coalescing concurrent
/// requests for the same provider, origin, and proxy route.
pub(crate) async fn solve(
    context: &ShieldContext,
    hit: &ShieldHit,
) -> Result<ShieldCredential, String> {
    let key = context.cache_key(&hit.url, hit.kind);
    let lock = store::lock_for(&key);
    if let Ok(_guard) = lock.try_lock() {
        if let Some(credential) = store::cached(&key) {
            return Ok(credential);
        }
        let credential = aliyun::solve(&hit.body, &hit.set_cookies)?;
        store::store(key, credential.clone());
        return Ok(credential);
    }

    let _guard = lock.lock().await;
    store::cached(&key).ok_or_else(|| {
        format!(
            "{}验证正在由另一个请求处理，但未获取有效凭证，请稍后重试",
            hit.kind.label()
        )
    })
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

pub(crate) fn cookie_name_allowed(kind: ShieldKind, name: &str) -> bool {
    let name = name.trim();
    match kind {
        ShieldKind::AliyunWaf => matches!(name, "acw_tc" | "acw_sc__v2"),
    }
}

pub(crate) fn ttl_secs(kind: ShieldKind) -> u64 {
    match kind {
        ShieldKind::AliyunWaf => 10 * 60,
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
    use reqwest::header::HeaderMap;

    fn context() -> ShieldContext {
        ShieldContext::new("provider-1", "proxy-a".to_string())
    }

    const SAMPLE_ARG1: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn detects_aliyun_challenge_from_body() {
        let body = format!("<script>var arg1='{SAMPLE_ARG1}';</script>");
        assert_eq!(
            detect(&HeaderMap::new(), &body),
            Some(ShieldKind::AliyunWaf)
        );
        assert_eq!(detect(&HeaderMap::new(), ""), None);
    }

    #[test]
    fn keeps_only_known_shield_cookies() {
        let credential = ShieldCredential::from_pairs(
            ShieldKind::AliyunWaf,
            [
                ("acw_tc".to_string(), "abc".to_string()),
                ("session".to_string(), "must-not-enter".to_string()),
            ],
        );
        assert_eq!(credential.cookie_header(), "acw_tc=abc");
    }

    #[test]
    fn cache_key_separates_provider_origin_and_proxy() {
        let url = Url::parse("https://example.test/api/check").unwrap();
        let first = context().cache_key(&url, ShieldKind::AliyunWaf);
        let second = ShieldContext::new("provider-2", "proxy-a".to_string())
            .cache_key(&url, ShieldKind::AliyunWaf);
        let third = ShieldContext::new("provider-1", "proxy-b".to_string())
            .cache_key(&url, ShieldKind::AliyunWaf);
        assert_ne!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn cache_key_normalizes_default_origin_ports() {
        let implicit = Url::parse("https://example.test/api").unwrap();
        let explicit = Url::parse("https://example.test:443/api").unwrap();
        assert_eq!(
            context().cache_key(&implicit, ShieldKind::AliyunWaf),
            context().cache_key(&explicit, ShieldKind::AliyunWaf)
        );
    }

    #[test]
    fn stale_response_cannot_invalidate_a_replaced_credential() {
        let context =
            ShieldContext::new("conditional-invalidation-provider", "proxy-a".to_string());
        let url = Url::parse("https://conditional.example.test/api").unwrap();
        let key = context.cache_key(&url, ShieldKind::AliyunWaf);
        let stale = ShieldCredential::from_pairs(
            ShieldKind::AliyunWaf,
            [("acw_sc__v2".to_string(), "stale".to_string())],
        );
        let fresh = ShieldCredential::from_pairs(
            ShieldKind::AliyunWaf,
            [("acw_sc__v2".to_string(), "fresh".to_string())],
        );

        store::store(key.clone(), fresh.clone());
        store::invalidate_if_matches(&key, &stale);
        assert_eq!(
            store::cached(&key).unwrap().cookie_header(),
            "acw_sc__v2=fresh"
        );

        store::invalidate_if_matches(&key, &fresh);
        assert!(store::cached(&key).is_none());
    }
}
