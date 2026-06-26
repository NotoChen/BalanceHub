use crate::models::{Provider, ProviderQuotaScope};
use crate::state::AppState;
use tauri::{AppHandle, Manager};

pub const MAIN_TRAY_ID: &str = "main-tray";

pub fn update_tooltip(app: &AppHandle, providers: &[Provider]) {
    let active_providers = providers
        .iter()
        .filter(|provider| provider.runtime.enabled)
        .collect::<Vec<_>>();
    let has_unlimited = active_providers
        .iter()
        .any(|provider| provider.quota.unlimited);
    let available = active_providers
        .iter()
        .filter(|provider| !provider.quota.unlimited)
        .map(|provider| provider.quota.available)
        .sum::<f64>();
    let used = active_providers
        .iter()
        .map(|provider| provider.quota.used)
        .sum::<f64>();

    let provider_lines = active_providers
        .iter()
        .map(|provider| {
            format!(
                "{} · 已用 {} · 可用 {}",
                full_provider_identity(provider),
                format_provider_quota(provider, provider.quota.used),
                format_provider_available(provider)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tooltip = if provider_lines.is_empty() {
        "BalanceHub · 暂无启用中转站".to_string()
    } else {
        format!(
            "BalanceHub · 已用 {} · 可用 {}\n{}",
            format_usd_full(used),
            if has_unlimited {
                "∞".to_string()
            } else {
                format_usd_full(available)
            },
            provider_lines
        )
    };

    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

pub fn refresh_from_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    let guard = state.data.read().unwrap_or_else(|err| err.into_inner());
    update_tooltip(app, &guard.providers);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn full_provider_identity(provider: &Provider) -> String {
    let user_name = provider_user_name(provider);
    if user_name.trim().is_empty() {
        provider.identity.name.clone()
    } else {
        format!("{} · {}", provider.identity.name, user_name)
    }
}

fn provider_user_name(provider: &Provider) -> String {
    [
        provider.identity.display_name.as_str(),
        provider.identity.username.as_str(),
        provider.identity.user_id.as_str(),
        provider.auth.api_user.as_str(),
    ]
    .iter()
    .map(|value| value.trim())
    .find(|value| !value.is_empty())
    .unwrap_or("")
    .to_string()
}

fn format_provider_available(provider: &Provider) -> String {
    if provider.quota.unlimited {
        if provider.quota.scope == ProviderQuotaScope::Token {
            "∞（令牌额度）".to_string()
        } else {
            "∞".to_string()
        }
    } else {
        format_provider_quota(provider, provider.quota.available)
    }
}

fn format_provider_quota(provider: &Provider, value: f64) -> String {
    let symbol = provider.quota.currency_symbol.trim();
    if provider.quota.display_type.eq_ignore_ascii_case("tokens") || symbol.is_empty() {
        return format_number_full(value);
    }
    format_number_with_symbol(value, symbol)
}

fn format_usd_full(value: f64) -> String {
    format_number_with_symbol(value, "$")
}

fn format_number_with_symbol(value: f64, symbol: &str) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let rounded = format!("{:.2}", value.abs());
    let Some((integer, fractional)) = rounded.split_once('.') else {
        return format!("{sign}{symbol}{rounded}");
    };
    let mut grouped = String::new();
    for (index, ch) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let grouped = grouped.chars().rev().collect::<String>();
    format!("{sign}{symbol}{grouped}.{fractional}")
}

fn format_number_full(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let rounded = format!("{:.0}", value.abs());
    let mut grouped = String::new();
    for (index, ch) in rounded.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let grouped = grouped.chars().rev().collect::<String>();
    format!("{sign}{grouped}")
}
