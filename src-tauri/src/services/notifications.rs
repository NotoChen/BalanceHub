pub mod adapters;

use futures_util::future::join_all;
use serde::Serialize;
use tauri::AppHandle;

use crate::{
    limits,
    models::{
        AppSettings, NotificationChannel, NotificationChannelKind, Provider,
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

    let message = NotificationMessage::new(title, markdown);
    send_to_channels(app, settings, &settings.notification_channels, message).await
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
    let message = NotificationMessage::new(title, markdown);
    send_to_channels(app, settings, &selected_channels, message).await
}

async fn send_to_channels(
    app: &AppHandle,
    settings: &AppSettings,
    channels: &[NotificationChannel],
    message: NotificationMessage,
) -> NotificationSendResult {
    let client = match network::build_webhook_client(settings) {
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
    let results = join_all(
        channels
            .iter()
            .filter(|channel| channel.enabled)
            .take(limits::MAX_NOTIFICATION_CHANNELS)
            .map(|channel| adapter_for(channel.kind).send(&context, channel, &message)),
    )
    .await;

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
