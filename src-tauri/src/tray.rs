use crate::models::{Provider, ProviderProtocol, ProviderQuotaScope};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

pub const MAIN_TRAY_ID: &str = "main-tray";

#[derive(Default)]
pub struct TrayAvailability(AtomicBool);

pub fn set_available(app: &AppHandle, available: bool) {
    app.state::<TrayAvailability>()
        .0
        .store(available, Ordering::Release);
}

pub fn is_available(app: &AppHandle) -> bool {
    app.state::<TrayAvailability>().0.load(Ordering::Acquire)
}

pub fn update_tooltip(app: &AppHandle, providers: &[Provider]) {
    let tooltip = build_tooltip(providers);
    apply_tooltip(app, tooltip);
}

/// 只做纯计算，不碰托盘、不发生阻塞，因此可以安全地在持有状态锁时调用。
fn build_tooltip(providers: &[Provider]) -> String {
    let active_providers = providers
        .iter()
        .filter(|provider| provider.runtime.enabled)
        .collect::<Vec<_>>();
    let balance_providers = active_providers
        .iter()
        .copied()
        .filter(|provider| {
            !matches!(
                provider.identity.protocol,
                ProviderProtocol::OAuth | ProviderProtocol::CliProxyApi
            )
        })
        .collect::<Vec<_>>();
    let has_unlimited = balance_providers
        .iter()
        .any(|provider| provider.quota.unlimited);
    let available = balance_providers
        .iter()
        .filter(|provider| !provider.quota.unlimited)
        .map(|provider| provider.quota.available)
        .sum::<f64>();
    let used = balance_providers
        .iter()
        .map(|provider| provider.quota.used)
        .sum::<f64>();

    let provider_lines = active_providers
        .iter()
        .map(|provider| provider_tooltip(provider))
        .collect::<Vec<_>>()
        .join("\n");
    let tooltip = if provider_lines.is_empty() {
        "BalanceHub · 暂无启用中转站".to_string()
    } else if balance_providers.is_empty() {
        format!(
            "BalanceHub · {} 个账号与网关\n{provider_lines}",
            active_providers.len()
        )
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

    tooltip
}

fn provider_tooltip(provider: &Provider) -> String {
    let name = full_provider_identity(provider);
    if provider.identity.protocol == ProviderProtocol::OAuth {
        let subscription = provider.capabilities.subscription.as_ref();
        let mut parts = vec![name];
        if let Some(oauth) = &provider.auth.oauth {
            parts.push(oauth.vendor.label().into());
            let plan = subscription
                .map(|value| value.plan.as_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(&oauth.plan);
            if !plan.is_empty() {
                parts.push(plan.into());
            }
        }
        if let Some(subscription) = subscription {
            parts.extend(subscription.windows.iter().take(2).filter_map(|window| {
                window
                    .used_percent
                    .filter(|value| value.is_finite())
                    .map(|percent| format!("{}已用 {percent:.2}%", window.label))
            }));
        }
        return parts.join(" · ");
    }
    if provider.identity.protocol == ProviderProtocol::CliProxyApi {
        return match &provider.capabilities.gateway {
            Some(gateway) => format!(
                "{name} · CPA · 可用授权 {} / {}",
                gateway
                    .accounts
                    .iter()
                    .filter(|account| account.available)
                    .count(),
                gateway.accounts.len()
            ),
            None => format!("{name} · CPA · 尚未同步"),
        };
    }
    let mut parts = vec![
        name,
        format!(
            "已用 {}",
            format_provider_quota(provider, provider.quota.used)
        ),
        format!("可用 {}", format_provider_available(provider)),
    ];
    // 订阅窗口才是这类账号的额度来源；托盘只补一句最紧的窗口，避免提示过长。
    if let Some(window) = provider.quota.tightest_window() {
        if let Some(percent) = window.percent() {
            parts.push(format!("{}已用 {percent:.0}%", window.label));
        }
    }
    parts.join(" · ")
}

/// `TrayIcon::set_tooltip` 必须在主线程执行。把它排入主线程队列后立即返回，
/// 既不会阻塞调度/异步工作线程，也不会在状态锁作用域内等待 UI 回执。
fn apply_tooltip(app: &AppHandle, tooltip: String) {
    if let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) {
        let _ = app.run_on_main_thread(move || {
            let _ = tray.set_tooltip(Some(tooltip));
        });
    }
}

pub fn refresh_from_state(app: &AppHandle) {
    // 先在锁内算好文案并立刻释放锁，再去操作托盘。
    //
    // 旧实现把读锁一直持有到托盘操作返回，主线程同步命令再等待写锁时会形成锁顺序
    // 反转。现在锁只覆盖纯文案计算，托盘更新也不会阻塞当前线程。
    let tooltip = {
        let state = app.state::<AppState>();
        let guard = state.data.read().unwrap_or_else(|err| err.into_inner());
        build_tooltip(&guard.providers)
    };
    apply_tooltip(app, tooltip);
}

pub fn show_main_window(app: &AppHandle) {
    // macOS 关窗后应用退成纯托盘形态（Accessory），重新打开时必须先切回
    // Regular 再 show/focus，否则窗口无法激活到前台。
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 托盘图标左键「切换」语义：窗口已经在前台就收起，否则唤到前台。
pub fn toggle_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        show_main_window(app);
        return;
    };
    let active = window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false);
    if active {
        hide_main_window(app);
    } else {
        show_main_window(app);
    }
}

pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    // 关窗即隐藏 Dock 图标，只留菜单栏托盘；Windows/Linux 隐藏窗口后
    // 任务栏本就没有按钮，无需处理。
    #[cfg(target_os = "macos")]
    if hide_dock_icon(app) {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

/// 关窗后是否退成纯菜单栏形态。默认开启，与「隐藏到托盘」的默认行为一致。
#[cfg(target_os = "macos")]
fn hide_dock_icon(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| {
            state
                .data
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .settings
                .window_hide_dock_icon
        })
        .unwrap_or(true)
}

fn full_provider_identity(provider: &Provider) -> String {
    let user_name = provider_user_name(provider);
    if user_name.trim().is_empty() {
        provider.display_label()
    } else {
        format!("{} · {}", provider.display_label(), user_name)
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

#[cfg(test)]
mod tests {
    use super::build_tooltip;
    use crate::models::{Provider, ProviderInput};

    fn provider(name: &str, enabled: bool, available: f64) -> Provider {
        let mut provider = Provider::from_input(
            ProviderInput {
                identity: crate::models::ProviderIdentityInput {
                    name: name.to_string(),
                    base_url: "https://example.com".to_string(),
                    ..crate::models::ProviderIdentityInput::default()
                },
                ..ProviderInput::default()
            },
            format!("provider-{name}"),
        );
        provider.runtime.enabled = enabled;
        provider.quota.available = available;
        provider
    }

    /// build_tooltip 必须是纯计算：它会在持有状态读锁时被调用，
    /// 任何阻塞式托盘操作都必须留在 apply_tooltip 里，否则会与主线程互等死锁。
    #[test]
    fn builds_tooltip_without_touching_the_tray() {
        let tooltip = build_tooltip(&[
            provider("启用站点", true, 10.0),
            provider("停用站点", false, 99.0),
        ]);

        assert!(tooltip.contains("启用站点"));
        // 未启用的中转站不计入
        assert!(!tooltip.contains("停用站点"));
    }

    #[test]
    fn reports_empty_state_when_no_provider_is_enabled() {
        assert_eq!(build_tooltip(&[]), "BalanceHub · 暂无启用中转站");
        assert_eq!(
            build_tooltip(&[provider("停用站点", false, 1.0)]),
            "BalanceHub · 暂无启用中转站"
        );
    }

    #[test]
    fn oauth_and_cpa_show_their_own_metrics_without_a_fabricated_balance() {
        use crate::models::{
            AccountQuotaWindow, AccountSubscription, CpaAccountSummary, CpaGatewaySnapshot,
            OAuthCredential, OAuthVendor, ProviderProtocol,
        };
        let mut oauth = provider("演示账号", true, 999.0);
        oauth.identity.protocol = ProviderProtocol::OAuth;
        oauth.auth.oauth = Some(OAuthCredential::new(OAuthVendor::Claude));
        oauth.capabilities.subscription = Some(AccountSubscription {
            plan: "Pro".into(),
            windows: vec![AccountQuotaWindow {
                label: "5 小时".into(),
                used_percent: Some(12.3456),
                ..AccountQuotaWindow::default()
            }],
            ..AccountSubscription::default()
        });
        let mut gateway = provider("演示网关", true, 777.0);
        gateway.identity.protocol = ProviderProtocol::CliProxyApi;
        gateway.capabilities.gateway = Some(CpaGatewaySnapshot {
            accounts: vec![CpaAccountSummary {
                available: true,
                ..CpaAccountSummary::default()
            }],
            ..CpaGatewaySnapshot::default()
        });
        let managed = build_tooltip(&[oauth.clone(), gateway.clone()]);
        assert!(managed.starts_with("BalanceHub · 2 个账号与网关"));
        assert!(managed.contains("Claude · Pro · 5 小时已用 12.35%"));
        assert!(managed.contains("CPA · 可用授权 1 / 1"));
        assert!(!managed.contains("$"));
        let mixed = build_tooltip(&[provider("中转站", true, 10.0), oauth, gateway]);
        assert!(mixed.lines().next().unwrap().ends_with("可用 $10.00"));
    }
}
