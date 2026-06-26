pub mod adapters;

use serde::Serialize;
use tauri::AppHandle;

use crate::models::{
    AppSettings, NotificationChannel, NotificationChannelKind, Provider, ProviderNotificationMode,
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

pub async fn send_configured_notification(
    app: &AppHandle,
    settings: &AppSettings,
    title: impl Into<String>,
    markdown: impl Into<String>,
    ignore_switch: bool,
) -> NotificationSendResult {
    if !ignore_switch && !settings.notification_enabled {
        return NotificationSendResult {
            sent_count: 0,
            results: Vec::new(),
        };
    }

    let message = NotificationMessage {
        title: title.into(),
        markdown: markdown.into(),
    };
    send_to_channels(app, &settings.notification_channels, message).await
}

pub async fn send_provider_notification(
    app: &AppHandle,
    settings: &AppSettings,
    provider: &Provider,
    title: impl Into<String>,
    markdown: impl Into<String>,
    ignore_switch: bool,
) -> NotificationSendResult {
    if !ignore_switch && !settings.notification_enabled {
        return NotificationSendResult {
            sent_count: 0,
            results: Vec::new(),
        };
    }
    if matches!(
        provider.notification.mode,
        ProviderNotificationMode::Disabled
    ) {
        return NotificationSendResult {
            sent_count: 0,
            results: Vec::new(),
        };
    }

    let selected_channels = selected_provider_channels(settings, provider);
    let message = NotificationMessage {
        title: title.into(),
        markdown: markdown.into(),
    };
    send_to_channels(app, &selected_channels, message).await
}

async fn send_to_channels(
    app: &AppHandle,
    channels: &[NotificationChannel],
    message: NotificationMessage,
) -> NotificationSendResult {
    let client = reqwest::Client::new();
    let context = NotificationContext {
        app,
        client: &client,
    };
    let mut results = Vec::new();

    for channel in channels.iter().filter(|channel| channel.enabled) {
        let adapter = adapter_for(channel.kind);
        results.push(adapter.send(&context, channel, &message).await);
    }

    NotificationSendResult {
        sent_count: results.iter().filter(|result| result.ok).count(),
        results,
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
