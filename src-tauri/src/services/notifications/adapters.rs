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
    pub text: String,
}

impl NotificationMessage {
    pub fn new(title: impl Into<String>, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        Self {
            title: title.into(),
            text: markdown_to_plain_text(&markdown),
            markdown,
        }
    }
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

fn markdown_to_plain_text(markdown: &str) -> String {
    let mut text = String::with_capacity(markdown.len());
    let mut chars = markdown.chars().peekable();
    let mut line_start = true;
    let mut pending_space = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {}
            '\n' => {
                trim_trailing_spaces(&mut text);
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                line_start = true;
                pending_space = false;
            }
            '*' | '_' | '`' => {}
            '#' if line_start => {
                while chars.peek().is_some_and(|next| *next == '#') {
                    chars.next();
                }
                if chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
            }
            '>' if line_start => {
                if chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
            }
            '-' if line_start && chars.peek().is_some_and(|next| next.is_whitespace()) => {
                chars.next();
                text.push_str("- ");
                line_start = false;
            }
            '[' => {
                let label = take_until(&mut chars, ']');
                if chars.peek().is_some_and(|next| *next == '(') {
                    chars.next();
                    let _ = take_until(&mut chars, ')');
                }
                push_text(&mut text, &label, &mut pending_space);
                line_start = false;
            }
            ch if ch.is_whitespace() => {
                pending_space = !line_start;
            }
            ch => {
                if pending_space && !text.ends_with(['\n', ' ']) {
                    text.push(' ');
                }
                pending_space = false;
                text.push(ch);
                line_start = false;
            }
        }
    }

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_text(text: &mut String, value: &str, pending_space: &mut bool) {
    for ch in value.chars() {
        if ch.is_whitespace() {
            *pending_space = true;
        } else {
            if *pending_space && !text.ends_with(['\n', ' ']) {
                text.push(' ');
            }
            *pending_space = false;
            text.push(ch);
        }
    }
}

fn take_until<I>(chars: &mut std::iter::Peekable<I>, end: char) -> String
where
    I: Iterator<Item = char>,
{
    let mut value = String::new();
    for ch in chars.by_ref() {
        if ch == end {
            break;
        }
        value.push(ch);
    }
    value
}

fn trim_trailing_spaces(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_message_keeps_markdown_and_adds_plain_text() {
        let message = NotificationMessage::new(
            "自动签到成功",
            "**中转站**：AnyRouter\n\n**结果**：获得 `$1.23`",
        );

        assert_eq!(
            message.markdown,
            "**中转站**：AnyRouter\n\n**结果**：获得 `$1.23`"
        );
        assert_eq!(message.text, "中转站：AnyRouter\n结果：获得 $1.23");
    }

    #[test]
    fn markdown_to_plain_text_handles_links_and_headings() {
        assert_eq!(
            markdown_to_plain_text("# 标题\n[详情](https://example.com)\n> **状态**：正常"),
            "标题\n详情\n状态：正常",
        );
    }
}
