#[cfg(any(target_os = "windows", test))]
use super::SystemProxyConfig;
#[cfg(target_os = "windows")]
use crate::{limits, platform::process::run_command_with_output_timeout};
#[cfg(target_os = "windows")]
use std::{process::Command, time::Duration};

#[cfg(target_os = "windows")]
const INTERNET_SETTINGS_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[cfg(target_os = "windows")]
pub(super) fn system_proxy_config() -> SystemProxyConfig {
    let mut command = Command::new("reg");
    command.args(["query", INTERNET_SETTINGS_KEY]);
    let Ok(output) = run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(3),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    ) else {
        return SystemProxyConfig::default();
    };
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return SystemProxyConfig::default();
    }
    parse_registry_proxy(&output.stdout)
}

fn parse_registry_proxy(text: &str) -> SystemProxyConfig {
    let has_pac = registry_string_value(text, "AutoConfigURL").is_some_and(|url| !url.is_empty());
    let auto_detect = registry_dword_enabled(text, "AutoDetect");
    if has_pac || auto_detect || !registry_dword_enabled(text, "ProxyEnable") {
        // PAC/WPAD 无法转换成一组静态环境变量；保持 reqwest/CLI 的环境继承语义。
        return SystemProxyConfig::default();
    }

    let Some(server) = registry_string_value(text, "ProxyServer") else {
        return SystemProxyConfig::default();
    };
    let mut config = parse_proxy_server_value(server);
    if config.http_url.is_empty() && config.https_url.is_empty() && config.all_url.is_empty() {
        return SystemProxyConfig::default();
    }
    config.no_proxy = registry_string_value(text, "ProxyOverride")
        .unwrap_or_default()
        .to_string();
    config.inherit_environment = false;
    config
}

fn parse_proxy_server_value(value: &str) -> SystemProxyConfig {
    let value = value.trim();
    if value.is_empty() {
        return SystemProxyConfig::default();
    }

    if !value.contains('=') {
        let url = with_proxy_scheme(value, "http");
        return SystemProxyConfig {
            http_url: url.clone(),
            https_url: url,
            all_url: String::new(),
            no_proxy: String::new(),
            inherit_environment: false,
        };
    }

    SystemProxyConfig {
        http_url: proxy_rule(value, "http")
            .map(|proxy| with_proxy_scheme(proxy, "http"))
            .unwrap_or_default(),
        https_url: proxy_rule(value, "https")
            .map(|proxy| with_proxy_scheme(proxy, "http"))
            .unwrap_or_default(),
        all_url: proxy_rule(value, "socks")
            .or_else(|| proxy_rule(value, "socks5"))
            .map(|proxy| with_proxy_scheme(proxy, "socks5h"))
            .unwrap_or_default(),
        no_proxy: String::new(),
        inherit_environment: false,
    }
}

fn with_proxy_scheme(value: &str, scheme: &str) -> String {
    if value.contains("://") {
        value.to_string()
    } else {
        format!("{scheme}://{value}")
    }
}

fn registry_string_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let line = text.lines().find(|line| line_contains_value(line, name))?;
    let (_, value) = line.split_once("REG_SZ")?;
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn registry_dword_enabled(text: &str, name: &str) -> bool {
    let Some(line) = text.lines().find(|line| line_contains_value(line, name)) else {
        return false;
    };
    line.split_once("REG_DWORD")
        .map(|(_, value)| value.trim())
        .is_some_and(|value| value.eq_ignore_ascii_case("0x1"))
}

fn line_contains_value(line: &str, name: &str) -> bool {
    line.split_whitespace().next() == Some(name)
}

fn proxy_rule<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    value.split(';').find_map(|part| {
        let (name, proxy) = part.trim().split_once('=')?;
        (name.trim().eq_ignore_ascii_case(key))
            .then_some(proxy.trim())
            .filter(|proxy| !proxy.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_proxy_for_http_and_https() {
        let config = parse_registry_proxy(
            "    ProxyEnable    REG_DWORD    0x1\n    ProxyServer    REG_SZ    127.0.0.1:7890\n",
        );
        assert_eq!(config.http_url, "http://127.0.0.1:7890");
        assert_eq!(config.https_url, "http://127.0.0.1:7890");
        assert!(config.all_url.is_empty());
        assert!(!config.inherit_environment);
    }

    #[test]
    fn preserves_protocol_specific_and_socks_proxy_rules() {
        let config = parse_registry_proxy(
            "    ProxyEnable    REG_DWORD    0x1\n    ProxyServer    REG_SZ    http=127.0.0.1:7890;https=127.0.0.1:7891;socks=127.0.0.1:7892\n    ProxyOverride    REG_SZ    localhost;*.internal;<local>\n",
        );
        assert_eq!(config.http_url, "http://127.0.0.1:7890");
        assert_eq!(config.https_url, "http://127.0.0.1:7891");
        assert_eq!(config.all_url, "socks5h://127.0.0.1:7892");
        assert_eq!(config.no_proxy, "localhost;*.internal;<local>");
    }

    #[test]
    fn pac_and_wpad_are_not_misreported_as_static_proxies() {
        for text in [
            "    AutoConfigURL    REG_SZ    https://proxy.example/proxy.pac\n    ProxyEnable    REG_DWORD    0x1\n    ProxyServer    REG_SZ    127.0.0.1:7890\n",
            "    AutoDetect    REG_DWORD    0x1\n    ProxyEnable    REG_DWORD    0x1\n    ProxyServer    REG_SZ    127.0.0.1:7890\n",
        ] {
            assert_eq!(parse_registry_proxy(text), SystemProxyConfig::default());
        }
    }
}
