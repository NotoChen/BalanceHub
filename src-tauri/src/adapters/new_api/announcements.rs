use crate::{
    adapters::announcements::{build_site_announcement, SiteAnnouncementDraft},
    limits,
    models::{AppSettings, Provider, SiteAnnouncement, SiteAnnouncementSource},
};
use reqwest::header::{ACCEPT, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;
use std::time::Duration;

use super::{
    adapter::NewApiAdapter,
    http::{build_client, build_url, normalize_base_url, ProviderTransport, USER_AGENT_VALUE},
    response::{parse_success_data, send_text},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

impl NewApiAdapter {
    pub(crate) async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Vec<SiteAnnouncement>), String> {
        let base_url = normalize_base_url(&provider.identity.base_url);
        if base_url.is_empty() {
            return Err("缺少中转站地址".to_string());
        }
        let client = build_client(settings, provider).await?;
        // NewAPI 把公告拆在两个公开接口里：/api/notice 是站点总公告，
        // /api/status 的 announcements 是控制台公告面板的列表。只读前者会漏掉
        // 站点实际发布的大部分内容，尤其是只在控制台发布公告的站点。
        let notice = fetch_notice(&client, &base_url).await;
        let console = fetch_console_announcements(&client, &base_url).await;

        let mut announcements = Vec::new();
        let mut failures = Vec::new();
        match notice {
            Ok(content) => announcements.extend(notice_announcement(provider, &content)),
            Err(message) => failures.push(format!("站点公告接口：{message}")),
        }
        match console {
            Ok(items) => announcements.extend(
                items
                    .into_iter()
                    .take(limits::MAX_ANNOUNCEMENTS_PER_SOURCE)
                    .filter_map(|item| console_announcement(provider, &item)),
            ),
            Err(message) => failures.push(format!("控制台公告接口：{message}")),
        }
        // 旧版本或裁剪过的 NewAPI 没有控制台公告接口，这时总公告仍然可用；
        // 只有两个接口都读不到内容时才把整个公告源判为失败。
        if announcements.is_empty() && !failures.is_empty() {
            return Err(failures.join("；"));
        }
        Ok((provider.clone(), announcements))
    }

    pub(crate) async fn mark_announcement_read(
        &self,
        _settings: &AppSettings,
        _provider: &Provider,
        _announcement_id: &str,
    ) -> Result<(Provider, ()), String> {
        Err("NewAPI 公告接口不支持远程标记已读".to_string())
    }
}

async fn fetch_notice(client: &ProviderTransport, base_url: &str) -> Result<String, String> {
    let request = announcement_request(client, base_url, "/api/notice")?;
    let (status, body) = send_text(client, request, "读取 NewAPI 站点公告").await?;
    let data = parse_success_data(&status, body, "站点公告")?;
    Ok(notice_content(&data).unwrap_or_default())
}

async fn fetch_console_announcements(
    client: &ProviderTransport,
    base_url: &str,
) -> Result<Vec<Value>, String> {
    let request = announcement_request(client, base_url, "/api/status")?;
    let (status, body) = send_text(client, request, "读取 NewAPI 控制台公告").await?;
    let data = parse_success_data(&status, body, "控制台公告")?;
    Ok(console_announcements(&data))
}

fn announcement_request(
    client: &ProviderTransport,
    base_url: &str,
    path: &str,
) -> Result<reqwest::RequestBuilder, String> {
    Ok(client
        .get(build_url(base_url, path)?)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(ACCEPT, "application/json, text/plain, */*")
        .header(ORIGIN, base_url)
        .header(REFERER, format!("{base_url}/"))
        .timeout(REQUEST_TIMEOUT))
}

fn notice_announcement(provider: &Provider, content: &str) -> Option<SiteAnnouncement> {
    build_site_announcement(
        provider,
        SiteAnnouncementDraft {
            id: "notice",
            source: SiteAnnouncementSource::SiteNotice,
            title: "",
            content,
            level: "",
            extra: "",
            published_at: None,
            updated_at: None,
            read_at: None,
            can_mark_read: false,
        },
    )
}

/// `/api/status` 的 data 里，announcements 只在控制台公告面板启用时出现。
fn console_announcements(data: &Value) -> Vec<Value> {
    if !data
        .get("announcements_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        return Vec::new();
    }
    match data.get("announcements") {
        Some(Value::Array(items)) => items.clone(),
        // 控制台设置本身以 JSON 字符串保存；被裁剪过的版本可能直接透传该字符串。
        Some(Value::String(raw)) => serde_json::from_str::<Vec<Value>>(raw).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn console_announcement(provider: &Provider, item: &Value) -> Option<SiteAnnouncement> {
    let content = text_field(item, &["content", "message"])?;
    let title = text_field(item, &["title", "name"]).unwrap_or_default();
    let extra =
        text_field(item, &["extra", "additional_info", "additionalInfo"]).unwrap_or_default();
    let level = text_field(item, &["type", "level"]).unwrap_or_default();
    let published_at = text_field(
        item,
        &[
            "publishDate",
            "publish_date",
            "published_at",
            "publishedAt",
            "created_at",
            "createdAt",
        ],
    );
    let updated_at = text_field(item, &["updated_at", "updatedAt"]);
    let id = scalar_id(item.get("id"));
    let scoped_id = if id.is_empty() {
        String::new()
    } else {
        format!("console-{id}")
    };
    build_site_announcement(
        provider,
        SiteAnnouncementDraft {
            id: &scoped_id,
            source: SiteAnnouncementSource::Console,
            title: &title,
            content: &content,
            level: &level,
            extra: &extra,
            published_at,
            updated_at,
            read_at: None,
            can_mark_read: false,
        },
    )
}

fn notice_content(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            ["notice", "content", "message"]
                .iter()
                .find_map(|field| value.get(*field).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn text_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn scalar_id(value: Option<&Value>) -> String {
    match value {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{console_announcement, console_announcements, notice_announcement, notice_content};
    use crate::models::{
        Provider, ProviderIdentityInput, ProviderInput, ProviderProtocol, SiteAnnouncementSource,
    };
    use serde_json::json;

    fn provider() -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: "https://relay.example.com".to_string(),
                    protocol: ProviderProtocol::NewApi,
                    ..ProviderIdentityInput::default()
                },
                ..ProviderInput::default()
            },
            "provider-1".to_string(),
        )
    }

    #[test]
    fn notice_accepts_official_string_and_named_object_variants() {
        assert_eq!(
            notice_content(&json!("维护通知")).as_deref(),
            Some("维护通知")
        );
        assert_eq!(
            notice_content(&json!({"content": "升级完成"})).as_deref(),
            Some("升级完成")
        );
        assert!(notice_content(&json!("   ")).is_none());
    }

    #[test]
    fn console_announcements_read_the_array_and_respect_the_enabled_switch() {
        let enabled = json!({
            "announcements_enabled": true,
            "announcements": [{"id": 1, "content": "第一条"}, {"id": 2, "content": "第二条"}],
        });
        assert_eq!(console_announcements(&enabled).len(), 2);

        let disabled = json!({
            "announcements_enabled": false,
            "announcements": [{"id": 1, "content": "第一条"}],
        });
        assert!(console_announcements(&disabled).is_empty());

        // 控制台设置以 JSON 字符串保存，旧版本可能原样透传。
        let as_string = json!({
            "announcements_enabled": true,
            "announcements": r#"[{"id": 3, "content": "字符串形态"}]"#,
        });
        assert_eq!(console_announcements(&as_string).len(), 1);
        assert!(console_announcements(&json!({"announcements_enabled": true})).is_empty());
    }

    #[test]
    fn console_announcement_keeps_level_extra_and_publish_date() {
        let announcement = console_announcement(
            &provider(),
            &json!({
                "id": 7,
                "content": "今晚 22:00 维护",
                "publishDate": "2026-10-20T10:00:00Z",
                "type": "warning",
                "extra": "维护期间暂停充值",
            }),
        )
        .unwrap();

        assert_eq!(announcement.id, "console-7");
        assert_eq!(announcement.title, "今晚 22:00 维护");
        assert_eq!(announcement.content, "今晚 22:00 维护");
        assert_eq!(announcement.level, "warning");
        assert_eq!(announcement.extra, "维护期间暂停充值");
        assert_eq!(
            announcement.published_at.as_deref(),
            Some("2026-10-20T10:00:00Z")
        );
        assert!(!announcement.can_mark_read);
    }

    #[test]
    fn console_announcement_requires_content() {
        assert!(
            console_announcement(&provider(), &json!({"id": 8, "extra": "只有附加说明"})).is_none()
        );
    }

    #[test]
    fn site_notice_and_console_announcements_keep_separate_sources() {
        let notice = notice_announcement(&provider(), "# 站点公告\n长期有效").unwrap();
        let console =
            console_announcement(&provider(), &json!({"id": 7, "content": "今晚维护"})).unwrap();

        assert_eq!(notice.source, SiteAnnouncementSource::SiteNotice);
        assert_eq!(console.source, SiteAnnouncementSource::Console);
        // 站点总公告是单条长期内容，控制台公告是多条列表；两者绝不能共用指纹。
        assert_ne!(notice.fingerprint, console.fingerprint);
    }
}
