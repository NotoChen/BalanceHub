pub mod adapters;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Timelike;
use futures_util::future::join_all;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{
    limits,
    models::{
        AppSettings, NotificationChannel, NotificationChannelKind, NotificationEvent, Provider,
        ProviderNotificationMode,
    },
    network,
};

use self::adapters::{adapter_for, NotificationContext, NotificationMessage};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSendResult {
    pub sent_count: usize,
    pub results: Vec<NotificationDeliveryResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDeliveryResult {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_kind: NotificationChannelKind,
    pub ok: bool,
    pub message: String,
}

impl NotificationDeliveryResult {
    pub fn success(channel: &NotificationChannel, message: impl Into<String>) -> Self {
        Self {
            channel_id: channel.id.clone(),
            channel_name: channel.name.clone(),
            channel_kind: channel.kind,
            ok: true,
            message: message.into(),
        }
    }

    pub fn failure(channel: &NotificationChannel, message: impl Into<String>) -> Self {
        Self {
            channel_id: channel.id.clone(),
            channel_name: channel.name.clone(),
            channel_kind: channel.kind,
            ok: false,
            message: message.into(),
        }
    }
}

/// 同类事件的合并窗口。键是「中转站 + 事件类别」，窗口内只放行一条：
/// 一个站点连续失败时，用户需要一次提醒，不是每个 tick 一条。
#[derive(Default)]
pub struct NotificationThrottle {
    last_sent: Mutex<HashMap<String, Instant>>,
}

impl NotificationThrottle {
    fn allow(&self, key: &str, window: Duration) -> bool {
        if window.is_zero() {
            return true;
        }
        let now = Instant::now();
        let mut guard = self.last_sent.lock().unwrap_or_else(|err| err.into_inner());
        // 每秒都可能进来，顺手清掉早已过期的键，避免长期运行后无界增长。
        guard.retain(|_, sent_at| now.duration_since(*sent_at) < window);
        if guard.contains_key(key) {
            return false;
        }
        guard.insert(key.to_string(), now);
        true
    }
}

pub async fn send_configured_notification(
    app: &AppHandle,
    settings: &AppSettings,
    title: impl Into<String>,
    markdown: impl Into<String>,
    ignore_switch: bool,
) -> NotificationSendResult {
    if !ignore_switch && !settings.notification_enabled {
        return NotificationSendResult::empty();
    }

    let message = NotificationMessage::new(title, markdown);
    send_to_channels(app, settings, &settings.notification_channels, message).await
}

pub async fn send_provider_notification(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    event: NotificationEvent,
    title: impl Into<String>,
    markdown: impl Into<String>,
    ignore_switch: bool,
) -> NotificationSendResult {
    if !ignore_switch {
        if !settings.notification_enabled || !settings.notify_enabled_for(event) {
            return NotificationSendResult::empty();
        }
        if quiet_hours_active(settings, chrono::Local::now().time()) {
            return NotificationSendResult::empty();
        }
        if !allow_through_throttle(app, settings, provider, event) {
            return NotificationSendResult::empty();
        }
    }
    if matches!(
        provider.notification.mode,
        ProviderNotificationMode::Disabled
    ) {
        return NotificationSendResult::empty();
    }

    let selected_channels = selected_provider_channels(settings, provider);
    let message = NotificationMessage::new(title, markdown);
    send_to_channels(app, settings, &selected_channels, message).await
}

impl NotificationSendResult {
    fn empty() -> Self {
        Self {
            sent_count: 0,
            results: Vec::new(),
        }
    }
}

fn allow_through_throttle(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    event: NotificationEvent,
) -> bool {
    if matches!(event, NotificationEvent::Manual) {
        return true;
    }
    let window = Duration::from_secs(settings.notify_merge_window_secs);
    if window.is_zero() {
        return true;
    }
    let Some(throttle) = app.try_state::<NotificationThrottle>() else {
        return true;
    };
    let key = format!("{}:{event:?}", provider.identity.id);
    throttle.allow(&key, window)
}

/// 静默时段按本地时间判定；起点晚于终点时视为跨午夜区间。
/// 起止相同时视为没有静默时段，而不是「全天静默」。
pub fn quiet_hours_active(settings: &AppSettings, now: chrono::NaiveTime) -> bool {
    if !settings.notify_quiet_hours {
        return false;
    }
    let Some(start) = clock_minutes(&settings.notify_quiet_start) else {
        return false;
    };
    let Some(end) = clock_minutes(&settings.notify_quiet_end) else {
        return false;
    };
    if start == end {
        return false;
    }
    let current = now.hour() * 60 + now.minute();
    if start < end {
        current >= start && current < end
    } else {
        current >= start || current < end
    }
}

fn clock_minutes(value: &str) -> Option<u32> {
    let (hour, minute) = value.trim().split_once(':')?;
    let (hour, minute) = (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?);
    (hour <= 23 && minute <= 59).then_some(hour * 60 + minute)
}

async fn send_to_channels(
    app: &AppHandle,
    settings: &AppSettings,
    channels: &[NotificationChannel],
    message: NotificationMessage,
) -> NotificationSendResult {
    let client = match network::build_webhook_client(settings).await {
        Ok(client) => client,
        Err(err) => {
            let results = channels
                .iter()
                .filter(|channel| channel.enabled)
                .map(|channel| {
                    NotificationDeliveryResult::failure(
                        channel,
                        format!("初始化通知客户端失败：{err}"),
                    )
                })
                .collect::<Vec<_>>();

            return NotificationSendResult {
                sent_count: 0,
                results,
            };
        }
    };
    let context = NotificationContext {
        app,
        client: &client,
    };
    let retries = settings.notify_retry_count;
    let results = join_all(
        channels
            .iter()
            .filter(|channel| channel.enabled)
            .take(limits::MAX_NOTIFICATION_CHANNELS)
            .map(|channel| send_with_retry(&context, channel, &message, retries)),
    )
    .await;

    NotificationSendResult {
        sent_count: results.iter().filter(|result| result.ok).count(),
        results,
    }
}

/// Webhook 投递失败大多是瞬时网络问题，按设置重试几次；每次间隔线性递增。
async fn send_with_retry(
    context: &NotificationContext<'_>,
    channel: &NotificationChannel,
    message: &NotificationMessage,
    retries: u64,
) -> NotificationDeliveryResult {
    let adapter = adapter_for(channel.kind);
    let mut attempt = 0;
    loop {
        let result = adapter.send(context, channel, message).await;
        if result.ok || attempt >= retries {
            return result;
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(300 * attempt)).await;
    }
}

fn selected_provider_channels(
    settings: &AppSettings,
    provider: &Provider,
) -> Vec<NotificationChannel> {
    if !matches!(provider.notification.mode, ProviderNotificationMode::Custom) {
        return settings.notification_channels.clone();
    }
    settings
        .notification_channels
        .iter()
        .filter(|channel| provider.notification.channel_ids.contains(&channel.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::quiet_hours_active;
    use crate::models::{AppSettings, NotificationEvent};

    fn at(hour: u32, minute: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(hour, minute, 0).expect("valid time")
    }

    #[test]
    fn quiet_hours_wrap_across_midnight() {
        let settings = AppSettings {
            notify_quiet_hours: true,
            notify_quiet_start: "22:00".into(),
            notify_quiet_end: "08:00".into(),
            ..AppSettings::default()
        };
        assert!(quiet_hours_active(&settings, at(23, 30)));
        assert!(quiet_hours_active(&settings, at(7, 59)));
        assert!(!quiet_hours_active(&settings, at(8, 0)));
        assert!(!quiet_hours_active(&settings, at(12, 0)));
    }

    #[test]
    fn quiet_hours_inside_one_day_and_identical_edges() {
        let mut settings = AppSettings {
            notify_quiet_hours: true,
            notify_quiet_start: "12:00".into(),
            notify_quiet_end: "13:30".into(),
            ..AppSettings::default()
        };
        assert!(quiet_hours_active(&settings, at(12, 30)));
        assert!(!quiet_hours_active(&settings, at(13, 30)));
        // 起止相同 = 没有静默时段，而不是全天静默。
        settings.notify_quiet_end = "12:00".into();
        assert!(!quiet_hours_active(&settings, at(12, 0)));
        // 开关关闭时任何时间都放行。
        settings.notify_quiet_hours = false;
        assert!(!quiet_hours_active(&settings, at(12, 30)));
    }

    #[test]
    fn each_event_has_its_own_switch() {
        let settings = AppSettings {
            notify_refresh_failure: false,
            ..AppSettings::default()
        };
        assert!(!settings.notify_enabled_for(NotificationEvent::RefreshFailure));
        assert!(settings.notify_enabled_for(NotificationEvent::LivenessFailure));
        assert!(settings.notify_enabled_for(NotificationEvent::CheckIn));
        assert!(settings.notify_enabled_for(NotificationEvent::Manual));
    }
}
