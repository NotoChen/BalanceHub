//! 阿里云 WAF / SCDN 的 JS 挑战求解。
//!
//! 挑战页会返回一段混淆 JS 和 `var arg1='<40 位十六进制>'`，浏览器执行后算出
//! `acw_sc__v2` 写进 cookie 再自动刷新。算法是静态的（固定置换表 + 固定异或密钥），
//! 因此可以直接在本地复刻，不需要浏览器、也不需要任何额外请求——命中的那个响应
//! 自身就带着求解所需的全部输入。

use super::{ShieldCredential, ShieldKind};

const XOR_KEY: &str = "3000176000856006061501533003690027800375";
const UNSBOX_TABLE: [usize; 40] = [
    0xf, 0x23, 0x1d, 0x18, 0x21, 0x10, 0x1, 0x26, 0xa, 0x9, 0x13, 0x1f, 0x28, 0x1b, 0x16, 0x17,
    0x19, 0xd, 0x6, 0xb, 0x27, 0x12, 0x14, 0x8, 0xe, 0x15, 0x20, 0x1a, 0x2, 0x1e, 0x7, 0x4, 0x11,
    0x5, 0x3, 0x1c, 0x22, 0x25, 0xc, 0x24,
];

/// 是否是阿里云挑战页。`arg1` 是这套盾的充分特征。
pub(crate) fn matches(body: &str) -> bool {
    extract_arg1(body).is_some()
}

/// 用命中响应求解。`set_cookies` 是该响应的 `Set-Cookie`（通常含 `acw_tc`），
/// 必须与算出的 `acw_sc__v2` 一起回带，缺一不可。
pub(crate) fn solve(body: &str, set_cookies: &[String]) -> Result<ShieldCredential, String> {
    let arg1 = extract_arg1(body).ok_or_else(|| "阿里云验证页缺少 arg1".to_string())?;
    let computed = compute_acw_cookie(&arg1)?;

    let mut sources = set_cookies
        .iter()
        .map(String::as_str)
        .filter_map(parse_set_cookie_pair)
        .collect::<Vec<_>>();
    sources.push(parse_cookie_pair(&computed).expect("computed cookie has a valid name"));

    Ok(ShieldCredential::from_pairs(
        ShieldKind::AliyunWaf,
        sources,
        // 阿里云盾不绑定 UA，保持调用方原有的 User-Agent。
        None,
    ))
}

/// 复刻挑战页 JS 的 `unsbox` + `hexXor`：
/// 先按固定表重排 arg1 的 40 个字符，再逐字节与固定密钥异或。
fn compute_acw_cookie(arg1: &str) -> Result<String, String> {
    if arg1.len() != 40 || !arg1.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("arg1 格式无效".to_string());
    }

    let arg1_chars = arg1.chars().collect::<Vec<_>>();
    // 置换表是 1-indexed，故减一。
    let unsboxed = UNSBOX_TABLE
        .iter()
        .map(|index| arg1_chars[index - 1])
        .collect::<String>();

    let mut out = String::with_capacity(40);
    for index in (0..40).step_by(2) {
        let a = u8::from_str_radix(&unsboxed[index..index + 2], 16)
            .map_err(|err| format!("arg1 解析失败: {err}"))?;
        let b = u8::from_str_radix(&XOR_KEY[index..index + 2], 16)
            .map_err(|err| format!("XOR key 解析失败: {err}"))?;
        out.push_str(&format!("{:02x}", a ^ b));
    }

    Ok(format!("acw_sc__v2={out}"))
}

fn extract_arg1(html: &str) -> Option<String> {
    let marker = "var arg1";
    let marker_index = html.find(marker)?;
    let after_marker = &html[marker_index + marker.len()..];
    let equals_index = after_marker.find('=')?;
    let after_equals = after_marker[equals_index + 1..].trim_start();
    let quote = after_equals.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let after_quote = &after_equals[quote.len_utf8()..];
    let end_index = after_quote.find(quote)?;
    let value = &after_quote[..end_index];
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(value.to_string())
    } else {
        None
    }
}

fn parse_set_cookie_pair(line: &str) -> Option<(String, String)> {
    let first_part = line.split(';').next()?.trim();
    let (name, value) = first_part.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || !super::cookie_name_allowed(ShieldKind::AliyunWaf, name) {
        None
    } else {
        Some((name.to_string(), value.trim().to_string()))
    }
}

fn parse_cookie_pair(value: &str) -> Option<(String, String)> {
    let (name, value) = value.split_once('=')?;
    Some((name.trim().to_string(), value.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{compute_acw_cookie, extract_arg1, matches, solve};

    const SAMPLE_ARG1: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn extracts_arg1_from_challenge_script() {
        let html = format!("hello <script>var arg1='{SAMPLE_ARG1}';</script>");
        assert_eq!(extract_arg1(&html).as_deref(), Some(SAMPLE_ARG1));
        assert!(matches(&html));
        assert!(!matches(r#"{"success":true}"#));
    }

    #[test]
    fn rejects_malformed_arg1() {
        assert!(extract_arg1("var arg1='tooshort';").is_none());
        assert!(compute_acw_cookie("nothex").is_err());
    }

    #[test]
    fn computes_acw_cookie_shape() {
        let cookie = compute_acw_cookie(SAMPLE_ARG1).unwrap();
        assert!(cookie.starts_with("acw_sc__v2="));
        assert_eq!(cookie.len(), "acw_sc__v2=".len() + 40);
    }

    #[test]
    fn solve_merges_response_cookies_with_computed_one() {
        let body = format!("<script>var arg1='{SAMPLE_ARG1}';</script>");
        let set_cookies = vec!["acw_tc=abc123; path=/; HttpOnly".to_string()];
        let credential = solve(&body, &set_cookies).unwrap();

        assert!(credential.cookie_header().contains("acw_tc=abc123"));
        assert!(credential.cookie_header().contains("acw_sc__v2="));
        // 阿里云盾不绑定 UA，不应覆盖调用方的 User-Agent。
        assert!(credential.user_agent.is_none());
    }
}
