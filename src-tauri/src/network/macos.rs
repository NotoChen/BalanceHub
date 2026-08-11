use super::SystemProxyConfig;
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{process::Command, time::Duration};

const SCUTIL_PATH: &str = "/usr/sbin/scutil";
const NETWORKSETUP_PATH: &str = "/usr/sbin/networksetup";
const SYSTEM_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn system_proxy_config() -> SystemProxyConfig {
    if let Some(scutil_text) = command_stdout(SCUTIL_PATH, &["--proxy"]) {
        if automatic_proxy_enabled(&scutil_text) {
            // PAC/WPAD 依赖按目标 URL 计算规则，不能安全压成静态代理地址。
            return SystemProxyConfig::default();
        }

        let config = parse_scutil_proxy(&scutil_text);
        if has_static_proxy(&config) {
            return config;
        }
    }

    // `scutil --proxy` 只反映 SystemConfiguration 的全局代理。在 macOS 上，
    // networksetup 还可能保存每个网络服务自己的代理（例如 Wi-Fi），而 Finder
    // 启动的 App 又没有 shell 环境变量可供 reqwest 继承，因此必须读取这个回退。
    networksetup_proxy_config().unwrap_or_default()
}

fn parse_scutil_proxy(text: &str) -> SystemProxyConfig {
    if automatic_proxy_enabled(text) {
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

fn automatic_proxy_enabled(text: &str) -> bool {
    proxy_enabled(text, "ProxyAutoConfigEnable") || proxy_enabled(text, "ProxyAutoDiscoveryEnable")
}

fn has_static_proxy(config: &SystemProxyConfig) -> bool {
    !config.http_url.is_empty() || !config.https_url.is_empty() || !config.all_url.is_empty()
}

fn networksetup_proxy_config() -> Option<SystemProxyConfig> {
    let services = command_stdout(NETWORKSETUP_PATH, &["-listallnetworkservices"])?;
    network_service_names(&services)
        .into_iter()
        .filter_map(|service| networksetup_proxy_for_service(&service))
        .next()
}

fn networksetup_proxy_for_service(service: &str) -> Option<SystemProxyConfig> {
    let http_url = networksetup_proxy_url("-getwebproxy", service, "http").unwrap_or_default();
    let https_url = networksetup_proxy_url("-getsecurewebproxy", service, "http");
    let all_url = networksetup_proxy_url("-getsocksfirewallproxy", service, "socks5h");

    let config = SystemProxyConfig {
        http_url,
        https_url: https_url.unwrap_or_default(),
        all_url: all_url.unwrap_or_default(),
        // networksetup does not expose the bypass list in these commands. Keep
        // the loopback defaults and let reqwest bypass local app endpoints.
        no_proxy: String::new(),
        inherit_environment: false,
    };
    has_static_proxy(&config).then_some(config)
}

fn networksetup_proxy_url(command_name: &str, service: &str, scheme: &str) -> Option<String> {
    let output = command_stdout(NETWORKSETUP_PATH, &[command_name, service])?;
    parse_networksetup_proxy(&output, scheme)
}

fn parse_networksetup_proxy(text: &str, scheme: &str) -> Option<String> {
    if !find_proxy_value(text, "Enabled").is_some_and(|value| value.eq_ignore_ascii_case("yes")) {
        return None;
    }
    let host = find_proxy_value(text, "Server")?;
    let port = find_proxy_value(text, "Port")?;
    if host.is_empty() || port.is_empty() || port == "0" {
        return None;
    }
    Some(format_proxy_url(scheme, &host, &port))
}

fn format_proxy_url(scheme: &str, host: &str, port: &str) -> String {
    let host = host.trim();
    let host = if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{scheme}://{host}:{port}")
}

fn network_service_names(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('*'))
        .map(ToOwned::to_owned)
        .collect()
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    let output = run_command_with_output_timeout(
        &mut command,
        SYSTEM_COMMAND_TIMEOUT,
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return None;
    }
    Some(output.stdout)
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
    Some(format_proxy_url(scheme, host.trim(), port.trim()))
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
    fn parses_networksetup_http_proxy_output() {
        assert_eq!(
            parse_networksetup_proxy(
                "Enabled: Yes\nServer: 127.0.0.1\nPort: 6152\nAuthenticated Proxy Enabled: 0\n",
                "http"
            ),
            Some("http://127.0.0.1:6152".to_string())
        );
    }

    #[test]
    fn ignores_disabled_or_empty_networksetup_proxy() {
        for text in [
            "Enabled: No\nServer: 127.0.0.1\nPort: 6152\n",
            "Enabled: Yes\nServer: 127.0.0.1\nPort: 0\n",
        ] {
            assert_eq!(parse_networksetup_proxy(text, "http"), None);
        }
    }

    #[test]
    fn parses_network_service_names_and_skips_disabled_services() {
        assert_eq!(
            network_service_names(
                "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*USB 10/100/1000 LAN\nVPN\n"
            ),
            vec!["Wi-Fi".to_string(), "VPN".to_string()]
        );
    }

    #[test]
    fn formats_ipv6_proxy_hosts() {
        assert_eq!(
            format_proxy_url("socks5h", "::1", "1080"),
            "socks5h://[::1]:1080"
        );
    }

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
