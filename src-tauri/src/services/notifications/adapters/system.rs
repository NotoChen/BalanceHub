use async_trait::async_trait;
use tauri_plugin_notification::NotificationExt;

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
        context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult {
        match context
            .app
            .notification()
            .builder()
            .title(&message.title)
            .body(&message.markdown)
            .show()
        {
            Ok(_) => NotificationDeliveryResult::success(channel, "已触发系统通知"),
            Err(err) => {
                NotificationDeliveryResult::failure(channel, format!("系统通知失败：{err}"))
            }
        }
    }
}
