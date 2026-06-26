use async_trait::async_trait;
use serde_json::json;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    post_json, required_webhook_url, NotificationAdapter, NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;

pub struct GenericAdapter;

#[async_trait]
impl NotificationAdapter for GenericAdapter {
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
            "source": "BalanceHub",
            "title": message.title,
            "markdown": message.markdown,
            "text": message.markdown,
        });

        match post_json(context, url, &body).await {
            Ok(_) => NotificationDeliveryResult::success(channel, "Webhook 通知已发送"),
            Err(err) => NotificationDeliveryResult::failure(channel, err),
        }
    }
}
