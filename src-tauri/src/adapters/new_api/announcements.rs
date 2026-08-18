use crate::{
    adapters::announcements::{build_site_announcement, SiteAnnouncementDraft},
    models::{AppSettings, Provider, SiteAnnouncement},
};
use reqwest::header::{ACCEPT, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;
use std::time::Duration;

use super::{
    adapter::NewApiAdapter,
    http::{build_client, build_url, normalize_base_url, USER_AGENT_VALUE},
    response::{parse_success_data, send_text},
};

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
        let request = client
            .get(build_url(&base_url, "/api/notice")?)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ORIGIN, &base_url)
            .header(REFERER, format!("{base_url}/"))
            .timeout(Duration::from_secs(8));
        let (status, body) = send_text(&client, request, "读取 NewAPI 站点公告").await?;
        let data = parse_success_data(&status, body, "站点公告")?;
        let content = notice_content(&data).unwrap_or_default();
        let announcements = build_site_announcement(
            provider,
            SiteAnnouncementDraft {
                id: "notice",
                title: "",
                content: &content,
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: false,
            },
        )
        .into_iter()
        .collect();
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

#[cfg(test)]
mod tests {
    use super::notice_content;
    use serde_json::json;

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
}
