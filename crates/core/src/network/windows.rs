use super::SystemProxyConfig;

#[cfg(windows)]
pub(super) fn system_proxy_config() -> SystemProxyConfig {
    use crate::platform::windows_registry::RegistryKey;
    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;
    let Some(key) = RegistryKey::open(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        0,
    ) else {
        return SystemProxyConfig::default();
    };
    proxy_from_values(ProxyValues {
        enabled: key.dword("ProxyEnable") == Some(1),
        auto_detect: key.dword("AutoDetect") == Some(1),
        auto_config_url: key.string("AutoConfigURL").unwrap_or_default(),
        server: key.string("ProxyServer").unwrap_or_default(),
        bypass: key.string("ProxyOverride").unwrap_or_default(),
    })
}

#[derive(Default)]
struct ProxyValues {
    enabled: bool,
    auto_detect: bool,
    auto_config_url: String,
    server: String,
    bypass: String,
}

fn proxy_from_values(values: ProxyValues) -> SystemProxyConfig {
    if !values.auto_config_url.is_empty() || values.auto_detect || !values.enabled {
        // PAC/WPAD cannot be reduced to static proxy variables.
        return SystemProxyConfig::default();
    }
    let mut config = parse_proxy_server_value(&values.server);
    if config.http_url.is_empty() && config.https_url.is_empty() && config.all_url.is_empty() {
        return SystemProxyConfig::default();
    }
    config.no_proxy = values.bypass;
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
        let config = proxy_from_values(ProxyValues {
            enabled: true,
            server: "127.0.0.1:7890".into(),
            ..Default::default()
        });
        assert_eq!(config.http_url, "http://127.0.0.1:7890");
        assert_eq!(config.https_url, "http://127.0.0.1:7890");
        assert!(config.all_url.is_empty());
        assert!(!config.inherit_environment);
    }

    #[test]
    fn preserves_protocol_specific_and_socks_proxy_rules() {
        let config = proxy_from_values(ProxyValues {
            enabled: true,
            server: "http=127.0.0.1:7890;https=127.0.0.1:7891;socks=127.0.0.1:7892".into(),
            bypass: "localhost;*.internal;<local>".into(),
            ..Default::default()
        });
        assert_eq!(config.http_url, "http://127.0.0.1:7890");
        assert_eq!(config.https_url, "http://127.0.0.1:7891");
        assert_eq!(config.all_url, "socks5h://127.0.0.1:7892");
        assert_eq!(config.no_proxy, "localhost;*.internal;<local>");
    }

    #[test]
    fn pac_wpad_and_disabled_proxy_keep_environment_inheritance() {
        for values in [
            ProxyValues {
                enabled: true,
                auto_config_url: "https://proxy.example/proxy.pac".into(),
                server: "127.0.0.1:7890".into(),
                ..Default::default()
            },
            ProxyValues {
                enabled: true,
                auto_detect: true,
                server: "127.0.0.1:7890".into(),
                ..Default::default()
            },
            ProxyValues {
                enabled: false,
                server: "127.0.0.1:7890".into(),
                ..Default::default()
            },
        ] {
            assert_eq!(proxy_from_values(values), SystemProxyConfig::default());
        }
    }

    #[test]
    fn unicode_proxy_values_remain_intact() {
        let config = proxy_from_values(ProxyValues {
            enabled: true,
            server: "代理.example:7890".into(),
            bypass: "localhost;*.内部.example".into(),
            ..Default::default()
        });
        assert_eq!(config.http_url, "http://代理.example:7890");
        assert_eq!(config.no_proxy, "localhost;*.内部.example");
    }
}
