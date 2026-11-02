use async_trait::async_trait;

use crate::models::NotificationChannel;
use crate::services::notifications::adapters::{
    NotificationAdapter, NotificationContext, NotificationMessage,
};
use crate::services::notifications::NotificationDeliveryResult;

pub struct SystemAdapter;

#[async_trait]
impl NotificationAdapter for SystemAdapter {
    async fn send(
        &self,
        _context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult {
        match crate::host::notify(&message.title, &message.text) {
            Ok(_) => NotificationDeliveryResult::success(channel, "已触发系统通知"),
            Err(err) => {
                NotificationDeliveryResult::failure(channel, format!("系统通知失败：{err}"))
            }
        }
    }
}
