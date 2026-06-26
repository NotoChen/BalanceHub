mod dingtalk;
mod feishu;
mod generic;
mod slack;
mod system;
mod wecom;

use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;
use tauri::AppHandle;

use crate::models::{NotificationChannel, NotificationChannelKind};
use crate::services::notifications::NotificationDeliveryResult;

use self::{
    dingtalk::DingTalkAdapter, feishu::FeishuAdapter, generic::GenericAdapter, slack::SlackAdapter,
    system::SystemAdapter, wecom::WeComAdapter,
};

static SYSTEM_ADAPTER: SystemAdapter = SystemAdapter;
static DINGTALK_ADAPTER: DingTalkAdapter = DingTalkAdapter;
static WECOM_ADAPTER: WeComAdapter = WeComAdapter;
static FEISHU_ADAPTER: FeishuAdapter = FeishuAdapter;
static SLACK_ADAPTER: SlackAdapter = SlackAdapter;
static GENERIC_ADAPTER: GenericAdapter = GenericAdapter;

pub struct NotificationContext<'a> {
    pub app: &'a AppHandle,
    pub client: &'a reqwest::Client,
}

pub struct NotificationMessage {
    pub title: String,
    pub markdown: String,
}

#[async_trait]
pub trait NotificationAdapter: Sync {
    async fn send(
        &self,
        context: &NotificationContext<'_>,
        channel: &NotificationChannel,
        message: &NotificationMessage,
    ) -> NotificationDeliveryResult;
}

pub fn adapter_for(kind: NotificationChannelKind) -> &'static dyn NotificationAdapter {
    match kind {
        NotificationChannelKind::System => &SYSTEM_ADAPTER,
        NotificationChannelKind::DingTalk => &DINGTALK_ADAPTER,
        NotificationChannelKind::WeCom => &WECOM_ADAPTER,
        NotificationChannelKind::Feishu => &FEISHU_ADAPTER,
        NotificationChannelKind::Slack => &SLACK_ADAPTER,
        NotificationChannelKind::Generic => &GENERIC_ADAPTER,
    }
}

pub(super) async fn post_json(
    context: &NotificationContext<'_>,
    url: &str,
    body: &Value,
) -> Result<String, String> {
    let response = context
        .client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|err| format!("发送失败：{err}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}：{}", status.as_u16(), text));
    }
    Ok(text)
}

pub(super) fn required_webhook_url(channel: &NotificationChannel) -> Result<&str, String> {
    let url = channel.url.trim();
    if url.is_empty() {
        Err("Webhook 地址为空".to_string())
    } else {
        Ok(url)
    }
}

pub(super) fn append_query(url: &str, pairs: &[(&str, String)]) -> Result<String, String> {
    let mut parsed = Url::parse(url).map_err(|err| format!("Webhook 地址无效：{err}"))?;
    {
        let mut query = parsed.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    }
    Ok(parsed.to_string())
}

pub(super) fn json_code_success(text: &str, code_fields: &[&str]) -> bool {
    if text.trim().is_empty() {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return true;
    };
    code_fields.iter().any(|field| {
        value
            .get(*field)
            .and_then(Value::as_i64)
            .is_some_and(|code| code == 0)
    })
}

pub(super) fn json_message(text: &str, fields: &[&str]) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return text.trim().to_string();
    };
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
        .unwrap_or(text.trim())
        .to_string()
}
