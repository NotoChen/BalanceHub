use super::Sub2ApiAdapter;
use crate::{
    adapters::{
        announcements::{build_site_announcement, SiteAnnouncementDraft},
        sub2_api::{
            auth::request_account_json,
            json::{array_items, string_field},
            usage::urlencoding,
        },
        transport::build_client,
    },
    limits,
    models::{AppSettings, Provider, SiteAnnouncement},
};
use reqwest::Method;

impl Sub2ApiAdapter {
    pub(crate) async fn list_announcements(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<(Provider, Vec<SiteAnnouncement>), String> {
        let client = build_client(settings, provider).await?;
        let (authenticated, data) = request_account_json(
            &client,
            provider,
            Method::GET,
            "/announcements",
            None,
            "读取 Sub2API 站点公告",
        )
        .await?;
        let announcements = array_items(&data)
            .into_iter()
            .take(limits::MAX_ANNOUNCEMENTS_PER_SOURCE)
            .filter_map(|item| {
                let id = string_field(&item, &["id", "announcement_id", "announcementId"])
                    .unwrap_or_default();
                let title = string_field(&item, &["title", "name"]).unwrap_or_default();
                let content =
                    string_field(&item, &["content", "message", "body"]).unwrap_or_default();
                build_site_announcement(
                    &authenticated,
                    SiteAnnouncementDraft {
                        id: &id,
                        title: &title,
                        content: &content,
                        published_at: string_field(
                            &item,
                            &["published_at", "publishedAt", "created_at", "createdAt"],
                        ),
                        updated_at: string_field(&item, &["updated_at", "updatedAt"]),
                        read_at: string_field(&item, &["read_at", "readAt"]),
                        can_mark_read: valid_announcement_id(&id),
                    },
                )
            })
            .collect();
        Ok((authenticated, announcements))
    }

    pub(crate) async fn mark_announcement_read(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        announcement_id: &str,
    ) -> Result<(Provider, ()), String> {
        let announcement_id = announcement_id.trim();
        if !valid_announcement_id(announcement_id) {
            return Err("Sub2API 公告 ID 必须是正整数".to_string());
        }
        let client = build_client(settings, provider).await?;
        let (authenticated, _) = request_account_json(
            &client,
            provider,
            Method::POST,
            &format!("/announcements/{}/read", urlencoding(announcement_id)),
            None,
            "标记 Sub2API 公告已读",
        )
        .await?;
        Ok((authenticated, ()))
    }
}

fn valid_announcement_id(value: &str) -> bool {
    value.trim().parse::<u64>().is_ok_and(|id| id > 0)
}

#[cfg(test)]
mod tests {
    use super::valid_announcement_id;

    #[test]
    fn official_announcement_ids_must_be_positive_integers() {
        assert!(valid_announcement_id("7"));
        assert!(!valid_announcement_id("0"));
        assert!(!valid_announcement_id("notice-7"));
    }
}
