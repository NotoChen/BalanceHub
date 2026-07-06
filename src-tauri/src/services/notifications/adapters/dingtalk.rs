use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    append_query, json_code_success, json_message, post_json, required_webhook_url,
    NotificationAdapter, NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;
use crate::util::unix_millis;

type HmacSha256 = Hmac<Sha256>;

pub struct DingTalkAdapter;

#[async_trait]
impl NotificationAdapter for DingTalkAdapter {
    async fn send(
        &self,
        context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult {
        let Ok(raw_url) = required_webhook_url(channel) else {
            return NotificationDeliveryResult::failure(channel, "Webhook 地址为空");
        };
        let url = match signed_url(raw_url, channel.secret.trim()) {
            Ok(url) => url,
            Err(err) => return NotificationDeliveryResult::failure(channel, err),
        };
        let body = json!({
            "msgtype": "markdown",
            "markdown": {
                "title": message.title,
                "text": message.markdown,
            },
        });

        match post_json(context, &url, &body).await {
            Ok(text) if json_code_success(&text, &["errcode"]) => {
                NotificationDeliveryResult::success(channel, "钉钉通知已发送")
            }
            Ok(text) => NotificationDeliveryResult::failure(
                channel,
                format!(
                    "钉钉返回失败：{}",
                    json_message(&text, &["errmsg", "message"])
                ),
            ),
            Err(err) => NotificationDeliveryResult::failure(channel, err),
        }
    }
}

fn signed_url(url: &str, secret: &str) -> Result<String, String> {
    if secret.is_empty() {
        return Ok(url.to_string());
    }
    let timestamp = unix_millis().to_string();
    let payload = format!("{timestamp}\n{secret}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|err| format!("钉钉签名密钥无效：{err}"))?;
    mac.update(payload.as_bytes());
    let sign = general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    append_query(url, &[("timestamp", timestamp), ("sign", sign)])
}
