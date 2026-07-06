use async_trait::async_trait;
use serde_json::json;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    post_json, required_webhook_url, NotificationAdapter, NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;

pub struct SlackAdapter;

#[async_trait]
impl NotificationAdapter for SlackAdapter {
    async fn send(
        &self,
        context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult {
        let Ok(url) = required_webhook_url(channel) else {
            return NotificationDeliveryResult::failure(channel, "Webhook 地址为空");
        };
        let body = json!({
            "text": format!("*{}*\n{}", message.title, message.markdown),
            "mrkdwn": true,
        });

        match post_json(context, url, &body).await {
            Ok(text) if text.trim().is_empty() || text.trim() == "ok" => {
                NotificationDeliveryResult::success(channel, "Slack 通知已发送")
            }
            Ok(text) => {
                NotificationDeliveryResult::failure(channel, format!("Slack 返回失败：{text}"))
            }
            Err(err) => NotificationDeliveryResult::failure(channel, err),
        }
    }
}
