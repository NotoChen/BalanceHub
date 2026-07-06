use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    json_code_success, json_message, post_json, required_webhook_url, NotificationAdapter,
    NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;
use crate::util::unix_secs;

type HmacSha256 = Hmac<Sha256>;

pub struct FeishuAdapter;

#[async_trait]
impl NotificationAdapter for FeishuAdapter {
    async fn send(
        &self,
        context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult {
        let Ok(url) = required_webhook_url(channel) else {
            return NotificationDeliveryResult::failure(channel, "Webhook 地址为空");
        };
        let body = match request_body(channel, message) {
            Ok(body) => body,
            Err(err) => return NotificationDeliveryResult::failure(channel, err),
        };

        match post_json(context, url, &body).await {
            Ok(text) if json_code_success(&text, &["code", "StatusCode"]) => {
                NotificationDeliveryResult::success(channel, "飞书通知已发送")
            }
            Ok(text) => NotificationDeliveryResult::failure(
                channel,
                format!(
                    "飞书返回失败：{}",
                    json_message(&text, &["msg", "message", "StatusMessage"])
                ),
            ),
            Err(err) => NotificationDeliveryResult::failure(channel, err),
        }
    }
}

fn request_body(
    channel: &NotificationChannel,
    message: &NotificationMessage,
) -> Result<Value, String> {
    let mut body = json!({
        "msg_type": "interactive",
        "card": {
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": message.title,
                },
            },
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": message.markdown,
                    },
                ],
            },
        },
    });

    let secret = channel.secret.trim();
    if !secret.is_empty() {
        let timestamp = unix_secs().to_string();
        let sign = sign_feishu(&timestamp, secret)?;
        body["timestamp"] = Value::String(timestamp);
        body["sign"] = Value::String(sign);
    }

    Ok(body)
}

fn sign_feishu(timestamp: &str, secret: &str) -> Result<String, String> {
    let key = format!("{timestamp}\n{secret}");
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|err| format!("飞书签名密钥无效：{err}"))?;
    mac.update(b"");
    Ok(general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}
