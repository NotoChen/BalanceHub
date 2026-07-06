use async_trait::async_trait;
use serde_json::json;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    json_code_success, json_message, post_json, required_webhook_url, NotificationAdapter,
    NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;

pub struct WeComAdapter;

#[async_trait]
impl NotificationAdapter for WeComAdapter {
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
            "msgtype": "markdown",
            "markdown": {
                "content": message.markdown,
            },
        });

        match post_json(context, url, &body).await {
            Ok(text) if json_code_success(&text, &["errcode"]) => {
                NotificationDeliveryResult::success(channel, "企业微信通知已发送")
            }
            Ok(text) => NotificationDeliveryResult::failure(
                channel,
                format!(
                    "企业微信返回失败：{}",
                    json_message(&text, &["errmsg", "message"])
                ),
            ),
            Err(err) => NotificationDeliveryResult::failure(channel, err),
        }
    }
}
