use crate::models::ProviderProtocol;

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
