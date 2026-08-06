use crate::{
    limits,
    models::{AuthMode, ProviderProtocol},
};
use serde::Serialize;

use super::{Provider, ProviderInput};

pub fn check_in_message_indicates_disabled(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    let compact = normalized
        .replace([' ', '_', '-', '/', '\\'], "")
        .replace('，', ",")
        .replace('。', ".");
    let mentions_check_in = compact.contains("签到")
        || compact.contains("checkin")
        || compact.contains("signin")
        || compact.contains("signing");
    if !mentions_check_in {
        return false;
    }

    [
        "未开启",
        "未启用",
        "未开放",
        "不支持",
        "不可用",
        "已关闭",
        "关闭",
        "禁用",
        "disable",
        "disabled",
        "notenabled",
        "unsupported",
        "notsupported",
        "unavailable",
        "notavailable",
        "closed",
    ]
    .iter()
    .any(|keyword| compact.contains(keyword))
}

pub fn normalize_api_key(raw: &str) -> String {
    let text = normalize_api_key_literal(raw);
    if text.is_empty() {
        return String::new();
    }

    let has_key_prefix = text
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sk-"));

    if has_key_prefix || text.contains('*') {
        text
    } else {
        format!("sk-{text}")
    }
}

pub fn normalize_api_key_for_protocol(raw: &str, protocol: ProviderProtocol) -> String {
    match protocol {
        // NewAPI 的 token 接口常返回不带 sk- 的值，客户端调用网关时需要补齐。
        ProviderProtocol::NewApi => normalize_api_key(raw),
        // Sub2API 的 Key 前缀由服务端配置决定，且允许无 sk- 的自定义 Key；通用
        // OpenAI 兼容接口同样不能假设前缀格式，因此两者都保留完整原值。
        ProviderProtocol::Sub2Api | ProviderProtocol::Api => normalize_api_key_literal(raw),
    }
}

pub fn normalize_api_key_literal(raw: &str) -> String {
    let mut text = raw.trim();
    // 用 get(..7) 安全取前缀：若第 7 字节不在字符边界返回 None，避免直接按字节切片 panic。
    if text
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        text = text[7..].trim();
    }

    text.to_string()
}

/// Build the identity used to reject an exact duplicate provider configuration.
///
/// The endpoint alone is intentionally not enough: one site may legitimately
/// have several accounts or API keys. The credential identity is compared only
/// inside Rust and is never returned to the UI.
fn provider_duplicate_key(provider: &Provider) -> String {
    provider_duplicate_key_parts(
        &provider.identity.base_url,
        provider.identity.protocol,
        &provider.auth,
    )
}

fn provider_input_duplicate_key(input: &ProviderInput) -> String {
    provider_duplicate_key_parts(
        &input.identity.base_url,
        input.identity.protocol,
        &input.auth,
    )
}

/// Rust-owned duplicate categories returned by the save command. The key
/// material itself is never sent back to the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ProviderDuplicateKind {
    #[serde(rename = "sameAccount")]
    Account,
    #[serde(rename = "sameApiKey")]
    ApiKey,
    #[serde(rename = "sameUrlDifferentApiKey")]
    UrlDifferentApiKey,
}

/// Classify a saved provider against a new or edited input. Account protocols
/// prefer the stable user ID and only fall back to the credential identity
/// when a site has not returned a user ID yet. API-key inputs are key-scoped:
/// an identical key is a duplicate, while another key on the same endpoint can
/// be merged into the existing provider card after user confirmation.
pub(crate) fn provider_duplicate_kind(
    provider: &Provider,
    input: &ProviderInput,
) -> Option<ProviderDuplicateKind> {
    if provider.identity.protocol != input.identity.protocol {
        return None;
    }

    let input_mode = effective_auth_mode(input.auth.mode, input.identity.protocol);
    let provider_mode = effective_auth_mode(provider.auth.mode, provider.identity.protocol);
    if matches!(input_mode, AuthMode::ApiKey) {
        if normalize_provider_endpoint(&provider.identity.base_url)
            != normalize_provider_endpoint(&input.identity.base_url)
        {
            return None;
        }

        let input_key =
            normalize_api_key_for_protocol(&input.auth.api_key, input.identity.protocol);
        let provider_key =
            normalize_api_key_for_protocol(&provider.auth.api_key, provider.identity.protocol);
        if input_key.is_empty() {
            return None;
        }
        if input_key == provider_key
            || provider.auth.api_key_options.iter().any(|option| {
                normalize_api_key_for_protocol(&option.key, provider.identity.protocol) == input_key
            })
        {
            return Some(ProviderDuplicateKind::ApiKey);
        }
        return Some(ProviderDuplicateKind::UrlDifferentApiKey);
    }

    if !matches!(provider_mode, AuthMode::ApiKey)
        && normalize_provider_domain(&provider.identity.base_url)
            == normalize_provider_domain(&input.identity.base_url)
    {
        let input_user = provider_input_user_id(input);
        let provider_user = provider_user_id(provider);
        if input_user
            .as_deref()
            .zip(provider_user.as_deref())
            .is_some_and(|(left, right)| left == right)
        {
            return Some(ProviderDuplicateKind::Account);
        }

        // A newly entered account may not have completed the first profile
        // request yet. Keep the prior credential fallback for that case only.
        if input_user.is_none()
            && provider_user.is_none()
            && provider_duplicate_key(provider) == provider_input_duplicate_key(input)
        {
            return Some(ProviderDuplicateKind::Account);
        }
    }

    None
}

fn provider_input_user_id(input: &ProviderInput) -> Option<String> {
    if matches!(
        effective_auth_mode(input.auth.mode, input.identity.protocol),
        AuthMode::ApiKey
    ) {
        return None;
    }
    let value = first_non_empty(&[&input.identity.user_id, &input.auth.api_user]);
    (!value.is_empty()).then_some(value)
}

fn provider_user_id(provider: &Provider) -> Option<String> {
    if matches!(
        effective_auth_mode(provider.auth.mode, provider.identity.protocol),
        AuthMode::ApiKey
    ) {
        return None;
    }
    let value = first_non_empty(&[&provider.identity.user_id, &provider.auth.api_user]);
    (!value.is_empty()).then_some(value)
}

fn provider_duplicate_key_parts(
    base_url: &str,
    protocol: ProviderProtocol,
    auth: &super::ProviderAuth,
) -> String {
    let mode = effective_auth_mode(auth.mode, protocol);
    let credential = match mode {
        AuthMode::ApiKey => normalize_api_key_for_protocol(&auth.api_key, protocol),
        AuthMode::Password => password_identity(auth),
        AuthMode::AccessToken => auth.access_token.trim().to_string(),
        AuthMode::Session => auth.session_cookie.trim().to_string(),
    };
    format!(
        "{}|{}|{}|{}",
        normalize_provider_endpoint(base_url),
        protocol_key(protocol),
        auth_mode_key(mode),
        credential
    )
}

fn effective_auth_mode(mode: AuthMode, protocol: ProviderProtocol) -> AuthMode {
    if matches!(protocol, ProviderProtocol::Api) {
        AuthMode::ApiKey
    } else if matches!(protocol, ProviderProtocol::Sub2Api) && matches!(mode, AuthMode::Session) {
        AuthMode::Password
    } else {
        mode
    }
}

fn password_identity(auth: &super::ProviderAuth) -> String {
    let account = first_non_empty(&[&auth.login_username, &auth.api_user]);
    if account.is_empty() {
        // Password-only imports have no stable account name. Keep the fallback
        // exact because passwords are case-sensitive credentials.
        auth.login_password.trim().to_string()
    } else {
        // Login names are treated case-insensitively by the supported account
        // protocols, while the password itself is never used in this branch.
        account.to_ascii_lowercase()
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn protocol_key(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::NewApi => "newApi",
        ProviderProtocol::Sub2Api => "sub2Api",
        ProviderProtocol::Api => "api",
    }
}

fn auth_mode_key(mode: AuthMode) -> &'static str {
    match mode {
        AuthMode::ApiKey => "apiKey",
        AuthMode::AccessToken => "accessToken",
        AuthMode::Session => "session",
        AuthMode::Password => "password",
    }
}

fn normalize_provider_endpoint(value: &str) -> String {
    let value = value.trim();
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return value.trim_end_matches('/').to_ascii_lowercase();
    };

    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    if matches!(url.port_or_known_default(), Some(80 | 443))
        && ((url.scheme() == "http" && url.port() == Some(80))
            || (url.scheme() == "https" && url.port() == Some(443)))
    {
        let _ = url.set_port(None);
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    url.as_str().trim_end_matches('/').to_string()
}

fn normalize_provider_domain(value: &str) -> String {
    let value = value.trim();
    let Ok(url) = reqwest::Url::parse(value) else {
        return value
            .trim_end_matches('/')
            .split('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    };

    let Some(host) = url.host_str() else {
        return String::new();
    };
    let port = url.port_or_known_default().map(|port| {
        if (url.scheme() == "http" && port == 80) || (url.scheme() == "https" && port == 443) {
            String::new()
        } else {
            format!(":{port}")
        }
    });
    format!("{}{}", host.to_ascii_lowercase(), port.unwrap_or_default())
}

pub fn normalize_invite_link(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() || text.contains("/register?aff=") {
        return text.to_string();
    }

    let Some((base, code)) = text.split_once("?aff=") else {
        return text.to_string();
    };
    let base = base.trim_end_matches('/');
    if base.is_empty() || code.trim().is_empty() {
        return text.to_string();
    }

    format!("{base}/register?aff={}", code.trim())
}

pub(super) fn string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized.truncate(limits::MAX_NOTIFICATION_CHANNELS);
    normalized
}

/// 规范化备用地址时保留用户填写顺序。备用地址的顺序是维护信息，不能像通知渠道一样排序。
pub(super) fn backup_url_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().trim_end_matches('/').to_string();
        if value.is_empty() || normalized.iter().any(|item| item == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= limits::MAX_BACKUP_URLS_PER_PROVIDER {
            break;
        }
    }
    normalized
}

pub(super) fn provider_name_from_input(name: &str, base_url: &str) -> String {
    let trimmed_name = name.trim();
    if !trimmed_name.is_empty() {
        return trimmed_name.to_string();
    }

    let trimmed_url = base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .trim();

    if trimmed_url.is_empty() {
        "未命名中转站".to_string()
    } else {
        trimmed_url.to_string()
    }
}

pub(super) fn session_value(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }

    for part in text.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("session") {
            return value.trim().to_string();
        }
    }

    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_protocol_preserves_non_newapi_key_formats() {
        assert_eq!(
            normalize_api_key_for_protocol("  gsk_custom-key  ", ProviderProtocol::Api),
            "gsk_custom-key"
        );
        assert_eq!(
            normalize_api_key_for_protocol("Bearer key-without-sk-prefix", ProviderProtocol::Api,),
            "key-without-sk-prefix"
        );
    }

    #[test]
    fn duplicate_key_normalizes_endpoint_and_newapi_key_prefix() {
        let first = ProviderInput {
            identity: super::super::ProviderIdentityInput {
                base_url: "HTTPS://Relay.Example.com:443/v1/".to_string(),
                ..Default::default()
            },
            auth: super::super::ProviderAuth {
                mode: AuthMode::ApiKey,
                api_key: "plain-key".to_string(),
                ..ProviderInput::default().auth
            },
            ..ProviderInput::default()
        };
        let mut second = first.clone();
        second.identity.base_url = "https://relay.example.com/v1".to_string();
        second.auth.api_key = "sk-plain-key".to_string();

        assert_eq!(
            provider_input_duplicate_key(&first),
            provider_input_duplicate_key(&second)
        );
    }

    #[test]
    fn duplicate_key_allows_two_accounts_on_one_site() {
        let mut first = ProviderInput::default();
        first.identity.base_url = "https://relay.example.com".to_string();
        first.auth.login_username = "alice@example.com".to_string();
        let mut second = first.clone();
        second.auth.login_username = "bob@example.com".to_string();

        assert_ne!(
            provider_input_duplicate_key(&first),
            provider_input_duplicate_key(&second)
        );
    }

    #[test]
    fn password_identity_uses_case_insensitive_account_but_case_sensitive_fallback() {
        let mut first = ProviderInput::default();
        first.identity.base_url = "https://relay.example.com".to_string();
        first.auth.login_username = "Alice@example.com".to_string();
        first.auth.login_password = "Secret".to_string();
        let mut second = first.clone();
        second.auth.login_username = "alice@EXAMPLE.com".to_string();
        second.auth.login_password = "Different".to_string();
        assert_eq!(
            provider_input_duplicate_key(&first),
            provider_input_duplicate_key(&second)
        );

        let mut password_only = ProviderInput::default();
        password_only.identity.base_url = "https://relay.example.com".to_string();
        password_only.auth.login_password = "Secret".to_string();
        let mut different_case = password_only.clone();
        different_case.auth.login_password = "secret".to_string();
        assert_ne!(
            provider_input_duplicate_key(&password_only),
            provider_input_duplicate_key(&different_case)
        );
    }

    #[test]
    fn duplicate_key_uses_the_protocol_effective_auth_mode() {
        let mut input = ProviderInput::default();
        input.identity.base_url = "https://relay.example.com".to_string();
        input.identity.protocol = ProviderProtocol::Api;
        input.auth.mode = AuthMode::Password;
        input.auth.api_key = "gsk-key".to_string();
        let provider = Provider::from_input(input.clone(), "provider-test".to_string());
        assert_eq!(provider.auth.mode, AuthMode::ApiKey);
        assert_eq!(
            provider_input_duplicate_key(&input),
            provider_duplicate_key(&provider)
        );

        input.identity.protocol = ProviderProtocol::Sub2Api;
        input.auth.mode = AuthMode::Session;
        input.auth.login_username = "alice@example.com".to_string();
        input.auth.session_cookie = "session-cookie".to_string();
        let provider = Provider::from_input(input.clone(), "provider-test".to_string());
        assert_eq!(provider.auth.mode, AuthMode::Password);
        assert_eq!(
            provider_input_duplicate_key(&input),
            provider_duplicate_key(&provider)
        );
    }

    #[test]
    fn duplicate_kind_prefers_same_domain_and_user_id() {
        let mut saved_input = ProviderInput::default();
        saved_input.identity.base_url = "https://Relay.Example.com/v1".to_string();
        saved_input.identity.user_id = "42".to_string();
        saved_input.auth.login_username = "alice@example.com".to_string();
        saved_input.auth.login_password = "old-password".to_string();
        let saved = Provider::from_input(saved_input, "provider-saved".to_string());

        let mut new_input = ProviderInput::default();
        new_input.identity.base_url = "https://relay.example.com/another-path".to_string();
        new_input.identity.user_id = "42".to_string();
        new_input.auth.login_username = "different-login".to_string();
        new_input.auth.login_password = "different-password".to_string();

        assert_eq!(
            provider_duplicate_kind(&saved, &new_input),
            Some(ProviderDuplicateKind::Account)
        );
    }

    #[test]
    fn api_key_duplicate_kind_distinguishes_same_key_and_new_key_on_same_url() {
        let mut saved_input = ProviderInput::default();
        saved_input.identity.protocol = ProviderProtocol::Api;
        saved_input.identity.base_url = "https://relay.example.com/v1/".to_string();
        saved_input.auth.mode = AuthMode::ApiKey;
        saved_input.auth.api_key = "key-a".to_string();
        let saved = Provider::from_input(saved_input, "provider-saved".to_string());

        let mut same_key = ProviderInput::default();
        same_key.identity.protocol = ProviderProtocol::Api;
        same_key.identity.base_url = "https://RELAY.EXAMPLE.COM/v1".to_string();
        same_key.auth.mode = AuthMode::ApiKey;
        same_key.auth.api_key = "key-a".to_string();
        assert_eq!(
            provider_duplicate_kind(&saved, &same_key),
            Some(ProviderDuplicateKind::ApiKey)
        );

        let mut new_key = same_key.clone();
        new_key.auth.api_key = "key-b".to_string();
        assert_eq!(
            provider_duplicate_kind(&saved, &new_key),
            Some(ProviderDuplicateKind::UrlDifferentApiKey)
        );
    }

    #[test]
    fn newapi_keeps_key_prefix_normalization() {
        assert_eq!(
            normalize_api_key_for_protocol("plain-key", ProviderProtocol::NewApi),
            "sk-plain-key"
        );
    }

    #[test]
    fn sub2api_preserves_server_defined_key_prefixes() {
        assert_eq!(
            normalize_api_key_for_protocol("plain-key", ProviderProtocol::Sub2Api),
            "plain-key"
        );
        assert_eq!(
            normalize_api_key_for_protocol("custom-sub2-key", ProviderProtocol::Sub2Api),
            "custom-sub2-key"
        );
    }
}
