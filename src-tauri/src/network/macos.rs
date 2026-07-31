use super::SystemProxyConfig;
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{process::Command, time::Duration};

pub(super) fn system_proxy_config() -> SystemProxyConfig {
    let mut command = Command::new("scutil");
    command.arg("--proxy");
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
    parse_scutil_proxy(&output.stdout)
}

fn parse_scutil_proxy(text: &str) -> SystemProxyConfig {
    if proxy_enabled(text, "ProxyAutoConfigEnable")
        || proxy_enabled(text, "ProxyAutoDiscoveryEnable")
    {
        // PAC 与自动发现依赖按目标 URL 计算规则，不能安全压成静态代理地址。
        return SystemProxyConfig::default();
    }

    let http_url = proxy_enabled(text, "HTTPEnable")
        .then(|| proxy_url(text, "HTTPProxy", "HTTPPort", "http"))
        .flatten()
        .unwrap_or_default();
    let https_url = proxy_enabled(text, "HTTPSEnable")
        .then(|| proxy_url(text, "HTTPSProxy", "HTTPSPort", "http"))
        .flatten()
        .unwrap_or_default();
    let all_url = proxy_enabled(text, "SOCKSEnable")
        .then(|| proxy_url(text, "SOCKSProxy", "SOCKSPort", "socks5h"))
        .flatten()
        .unwrap_or_default();

    if http_url.is_empty() && https_url.is_empty() && all_url.is_empty() {
        return SystemProxyConfig::default();
    }

    SystemProxyConfig {
        http_url,
        https_url,
        all_url,
        no_proxy: exception_list(text).join(","),
        inherit_environment: false,
    }
}

fn proxy_enabled(text: &str, key: &str) -> bool {
    find_proxy_value(text, key).is_some_and(|value| value.trim() == "1")
}

fn proxy_url(text: &str, host_key: &str, port_key: &str, scheme: &str) -> Option<String> {
    let host = find_proxy_value(text, host_key)?;
    let port = find_proxy_value(text, port_key)?;
    if host.trim().is_empty() || port.trim().is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}:{}", host.trim(), port.trim()))
}

fn find_proxy_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

fn exception_list(text: &str) -> Vec<String> {
    let mut inside = false;
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ExceptionsList") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed == "}" {
            break;
        }
        if let Some((index, value)) = trimmed.split_once(':') {
            if index.trim().parse::<usize>().is_ok() {
                let value = value.trim();
                if !value.is_empty() {
                    entries.push(value.to_string());
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protocol_specific_scutil_proxies_and_exceptions() {
        let config = parse_scutil_proxy(
            "HTTPEnable : 1\nHTTPProxy : 127.0.0.1\nHTTPPort : 7890\nHTTPSEnable : 1\nHTTPSProxy : 127.0.0.1\nHTTPSPort : 7891\nSOCKSEnable : 1\nSOCKSProxy : 127.0.0.1\nSOCKSPort : 7892\nExceptionsList : <array> {\n  0 : *.local\n  1 : example.com\n}\n",
        );
        assert_eq!(config.http_url, "http://127.0.0.1:7890");
        assert_eq!(config.https_url, "http://127.0.0.1:7891");
        assert_eq!(config.all_url, "socks5h://127.0.0.1:7892");
        assert_eq!(config.no_proxy, "*.local,example.com");
        assert!(!config.inherit_environment);
    }

    #[test]
    fn automatic_proxy_is_left_to_the_runtime_environment() {
        let config = parse_scutil_proxy(
            "ProxyAutoConfigEnable : 1\nProxyAutoConfigURLString : https://proxy.example/proxy.pac\nHTTPEnable : 1\nHTTPProxy : 127.0.0.1\nHTTPPort : 7890\n",
        );

        assert_eq!(config, SystemProxyConfig::default());
    }
}
