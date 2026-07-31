use super::SystemProxyConfig;
use crate::{limits, platform::process::run_command_with_output_timeout};
use std::{process::Command, time::Duration};

pub(super) fn system_proxy_config() -> SystemProxyConfig {
    if gsettings_string("org.gnome.system.proxy", "mode").as_deref() != Some("manual") {
        // GNOME auto 模式、非 GNOME 桌面和不可用的 gsettings 都无法静态解析。
        return SystemProxyConfig::default();
    }

    let config = SystemProxyConfig {
        http_url: gnome_proxy_for("http", "http").unwrap_or_default(),
        https_url: gnome_proxy_for("https", "http").unwrap_or_default(),
        all_url: gnome_proxy_for("socks", "socks5h").unwrap_or_default(),
        no_proxy: gsettings_string("org.gnome.system.proxy", "ignore-hosts")
            .map(|value| parse_ignore_hosts(&value))
            .unwrap_or_default(),
        inherit_environment: false,
    };
    if config.http_url.is_empty() && config.https_url.is_empty() && config.all_url.is_empty() {
        SystemProxyConfig::default()
    } else {
        config
    }
}

fn gnome_proxy_for(name: &str, scheme: &str) -> Option<String> {
    let schema = format!("org.gnome.system.proxy.{name}");
    let host = gsettings_string(&schema, "host")?;
    let port = gsettings_string(&schema, "port")?;
    if host.trim().is_empty() || port.trim().is_empty() || port.trim() == "0" {
        return None;
    }
    Some(format!("{scheme}://{}:{}", host.trim(), port.trim()))
}

fn gsettings_string(schema: &str, key: &str) -> Option<String> {
    let mut command = Command::new("gsettings");
    command.args(["get", schema, key]);
    let output = run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(3),
        limits::MAX_SYSTEM_COMMAND_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || !output.status.is_some_and(|status| status.success()) {
        return None;
    }
    Some(parse_gsettings_value(&output.stdout)).filter(|value| !value.trim().is_empty())
}

fn parse_gsettings_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string()
}

fn parse_ignore_hosts(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|entry| entry.trim().trim_matches('\'').trim_matches('"'))
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gsettings_values() {
        assert_eq!(parse_gsettings_value("'manual'\n"), "manual");
        assert_eq!(parse_gsettings_value("7890\n"), "7890");
        assert_eq!(
            parse_ignore_hosts("['localhost', '*.internal', '127.0.0.0/8']"),
            "localhost,*.internal,127.0.0.0/8"
        );
    }
}
