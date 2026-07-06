use crate::models::{AppSettings, Provider, ProviderProxyMode, ProxyMode};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct EffectiveProxy {
    pub mode: ProxyMode,
    pub url: String,
}

impl EffectiveProxy {
    pub fn none() -> Self {
        Self {
            mode: ProxyMode::NoProxy,
            url: String::new(),
        }
    }
}

pub fn resolve_proxy(settings: &AppSettings, provider: &Provider) -> EffectiveProxy {
    match provider.proxy.mode {
        ProviderProxyMode::Inherit => resolve_global_proxy(settings),
        ProviderProxyMode::System => resolve_system_proxy(),
        ProviderProxyMode::NoProxy => EffectiveProxy::none(),
        ProviderProxyMode::Custom => EffectiveProxy {
            mode: ProxyMode::Custom,
            url: provider.proxy.url.clone(),
        },
    }
}

pub fn resolve_global_proxy(settings: &AppSettings) -> EffectiveProxy {
    match settings.proxy_mode {
        ProxyMode::System => resolve_system_proxy(),
        ProxyMode::NoProxy => EffectiveProxy::none(),
        ProxyMode::Custom => EffectiveProxy {
            mode: ProxyMode::Custom,
            url: settings.proxy_url.clone(),
        },
    }
}

pub fn resolve_system_proxy() -> EffectiveProxy {
    // 缓存系统代理探测结果 10 秒：默认代理模式是「跟随系统」，
    // 否则每次构建 HTTP 客户端都会 spawn 一次 scutil 子进程。
    static CACHE: OnceLock<Mutex<Option<(Instant, EffectiveProxy)>>> = OnceLock::new();
    const TTL: Duration = Duration::from_secs(10);
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    if let Ok(guard) = cache.lock() {
        if let Some((resolved_at, proxy)) = guard.as_ref() {
            if resolved_at.elapsed() < TTL {
                return proxy.clone();
            }
        }
    }

    let resolved = system_proxy_url()
        .map(|url| EffectiveProxy {
            mode: ProxyMode::Custom,
            url,
        })
        .unwrap_or_else(|| EffectiveProxy {
            mode: ProxyMode::System,
            url: String::new(),
        });

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), resolved.clone()));
    }
    resolved
}

pub fn apply_proxy_env(command: &mut Command, proxy: &EffectiveProxy) {
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(key);
    }
    command.env("NO_PROXY", "127.0.0.1,localhost");
    command.env("no_proxy", "127.0.0.1,localhost");

    if matches!(proxy.mode, ProxyMode::NoProxy) {
        return;
    }
    let url = proxy.url.trim();
    if url.is_empty() {
        return;
    }
    command.env("HTTP_PROXY", url);
    command.env("HTTPS_PROXY", url);
    command.env("ALL_PROXY", url);
    command.env("http_proxy", url);
    command.env("https_proxy", url);
    command.env("all_proxy", url);
}

#[cfg(target_os = "macos")]
fn system_proxy_url() -> Option<String> {
    let output = Command::new("scutil")
        .arg("--proxy")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let https_enabled = proxy_enabled(&text, "HTTPSEnable");
    let http_enabled = proxy_enabled(&text, "HTTPEnable");
    let socks_enabled = proxy_enabled(&text, "SOCKSEnable");

    if https_enabled {
        if let Some(url) = proxy_url(&text, "HTTPSProxy", "HTTPSPort", "http") {
            return Some(url);
        }
    }
    if http_enabled {
        if let Some(url) = proxy_url(&text, "HTTPProxy", "HTTPPort", "http") {
            return Some(url);
        }
    }
    if socks_enabled {
        if let Some(url) = proxy_url(&text, "SOCKSProxy", "SOCKSPort", "socks5") {
            return Some(url);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn system_proxy_url() -> Option<String> {
    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyEnable",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return env_proxy_url();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if !text.contains("0x1") {
        return env_proxy_url();
    }

    let output = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return env_proxy_url();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_windows_proxy_server(&text).or_else(env_proxy_url)
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn system_proxy_url() -> Option<String> {
    gnome_proxy_url().or_else(env_proxy_url)
}

#[cfg(not(target_os = "macos"))]
fn env_proxy_url() -> Option<String> {
    std::env::var("HTTPS_PROXY")
        .ok()
        .or_else(|| std::env::var("HTTP_PROXY").ok())
        .or_else(|| std::env::var("ALL_PROXY").ok())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn gnome_proxy_url() -> Option<String> {
    let mode = gsettings_string("org.gnome.system.proxy", "mode")?;
    if mode != "manual" {
        return None;
    }

    gnome_proxy_for("https", "http")
        .or_else(|| gnome_proxy_for("http", "http"))
        .or_else(|| gnome_proxy_for("socks", "socks5"))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn gnome_proxy_for(name: &str, scheme: &str) -> Option<String> {
    let schema = format!("org.gnome.system.proxy.{name}");
    let host = gsettings_string(&schema, "host")?;
    let port = gsettings_string(&schema, "port")?;
    if host.trim().is_empty() || port.trim().is_empty() || port.trim() == "0" {
        return None;
    }
    Some(format!("{scheme}://{}:{}", host.trim(), port.trim()))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn gsettings_string(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_gsettings_value(&String::from_utf8_lossy(
        &output.stdout,
    )))
    .filter(|value| !value.trim().is_empty())
}

#[cfg(any(all(not(target_os = "macos"), not(target_os = "windows")), test))]
fn parse_gsettings_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string()
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_proxy_server(text: &str) -> Option<String> {
    let line = text.lines().find(|line| line.contains("ProxyServer"))?;
    let value = line
        .split_once("REG_SZ")
        .map(|(_, value)| value.trim())
        .or_else(|| line.split_whitespace().last())?;
    if value.is_empty() {
        return None;
    }

    let https = windows_proxy_rule(value, "https");
    let http = windows_proxy_rule(value, "http");
    let socks = windows_proxy_rule(value, "socks");
    let (scheme, selected) = https
        .map(|proxy| ("http", proxy))
        .or_else(|| http.map(|proxy| ("http", proxy)))
        .or_else(|| socks.map(|proxy| ("socks5", proxy)))
        .unwrap_or(("http", value));

    if selected.contains("://") {
        Some(selected.to_string())
    } else {
        Some(format!("{scheme}://{selected}"))
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_proxy_rule<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    value.split(';').find_map(|part| {
        let (name, proxy) = part.trim().split_once('=')?;
        (name.trim().eq_ignore_ascii_case(key))
            .then_some(proxy.trim())
            .filter(|proxy| !proxy.is_empty())
    })
}

#[cfg(target_os = "macos")]
fn proxy_enabled(text: &str, key: &str) -> bool {
    find_proxy_value(text, key).is_some_and(|value| value.trim() == "1")
}

#[cfg(target_os = "macos")]
fn proxy_url(text: &str, host_key: &str, port_key: &str, scheme: &str) -> Option<String> {
    let host = find_proxy_value(text, host_key)?;
    let port = find_proxy_value(text, port_key)?;
    if host.trim().is_empty() || port.trim().is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}:{}", host.trim(), port.trim()))
}

#[cfg(target_os = "macos")]
fn find_proxy_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_proxy_server_single_value() {
        let text = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    ProxyServer    REG_SZ    127.0.0.1:7890
"#;

        assert_eq!(
            parse_windows_proxy_server(text),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn parses_windows_proxy_server_prefers_https_then_http() {
        let text = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    ProxyServer    REG_SZ    http=127.0.0.1:7890;https=127.0.0.1:7891;socks=127.0.0.1:7892
"#;

        assert_eq!(
            parse_windows_proxy_server(text),
            Some("http://127.0.0.1:7891".to_string())
        );
    }

    #[test]
    fn parses_windows_proxy_server_supports_socks_only() {
        let text = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    ProxyServer    REG_SZ    socks=127.0.0.1:7892
"#;

        assert_eq!(
            parse_windows_proxy_server(text),
            Some("socks5://127.0.0.1:7892".to_string())
        );
    }

    #[test]
    fn parses_windows_proxy_server_preserves_explicit_scheme() {
        let text = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    ProxyServer    REG_SZ    SOCKS=socks5://127.0.0.1:7892
"#;

        assert_eq!(
            parse_windows_proxy_server(text),
            Some("socks5://127.0.0.1:7892".to_string())
        );
    }

    #[test]
    fn parses_gsettings_quoted_and_plain_values() {
        assert_eq!(parse_gsettings_value("'manual'\n"), "manual");
        assert_eq!(parse_gsettings_value("7890\n"), "7890");
    }
}
