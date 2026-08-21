use crate::{
    limits,
    models::{Provider, SiteAnnouncement},
};
use reqwest::Url;
use sha2::{Digest, Sha256};

pub(crate) struct SiteAnnouncementDraft<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub published_at: Option<String>,
    pub updated_at: Option<String>,
    pub read_at: Option<String>,
    pub can_mark_read: bool,
}

pub(crate) fn build_site_announcement(
    provider: &Provider,
    draft: SiteAnnouncementDraft<'_>,
) -> Option<SiteAnnouncement> {
    let title = bounded(draft.title, limits::MAX_ANNOUNCEMENT_TITLE_CHARS);
    let content = bounded(draft.content, limits::MAX_ANNOUNCEMENT_CONTENT_CHARS);
    if title.is_empty() && content.is_empty() {
        return None;
    }

    let title = if title.is_empty() {
        derived_title(&content)
    } else {
        title
    };
    let generated_id = digest_hex(format!("{title}\n{content}").as_bytes());
    let id = if draft.id.trim().is_empty() {
        generated_id.chars().take(24).collect()
    } else {
        bounded(draft.id, 240)
    };
    let protocol = serde_json::to_string(&provider.identity.protocol)
        .unwrap_or_else(|_| "unknown".to_string());
    let source = normalized_source(&provider.identity.base_url);
    // BalanceHub 的公告入口按站点汇总。同一协议、同一站点返回的同一条公告，
    // 即使通过不同本地账号读取，也只保留一个本地指纹和一份已读状态。
    let fingerprint =
        digest_hex(format!("{protocol}\n{source}\n{id}\n{title}\n{content}").as_bytes());

    Some(SiteAnnouncement {
        id,
        fingerprint,
        provider_id: provider.identity.id.clone(),
        provider_name: provider.display_label(),
        provider_protocol: provider.identity.protocol,
        title,
        content,
        published_at: draft.published_at,
        updated_at: draft.updated_at,
        read_at: draft.read_at,
        can_mark_read: draft.can_mark_read,
    })
}

pub(crate) fn normalized_source(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    let Ok(mut url) = Url::parse(trimmed) else {
        return trimmed.to_ascii_lowercase();
    };
    url.set_query(None);
    url.set_fragment(None);
    // 公告属于站点而不是某个账号或某条 API 路径，统一归一化到 origin。
    url.set_path("/");
    url.to_string().trim_end_matches('/').to_string()
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn derived_title(content: &str) -> String {
    let line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("站点公告")
        .trim_start_matches(['#', '*', '-', '>'])
        .trim();
    let title = bounded(line, limits::MAX_ANNOUNCEMENT_TITLE_CHARS);
    if title.is_empty() {
        "站点公告".to_string()
    } else {
        title
    }
}

fn digest_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Provider, ProviderIdentityInput, ProviderInput, ProviderProtocol};

    fn provider(id: &str, protocol: ProviderProtocol) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: "https://relay.example.com".to_string(),
                    protocol,
                    ..ProviderIdentityInput::default()
                },
                ..ProviderInput::default()
            },
            id.to_string(),
        )
    }

    #[test]
    fn announcement_fingerprint_changes_when_content_changes() {
        let provider = provider("provider-1", ProviderProtocol::NewApi);
        let first = build_site_announcement(
            &provider,
            SiteAnnouncementDraft {
                id: "7",
                title: "",
                content: "# 维护通知\n今晚维护",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: false,
            },
        )
        .unwrap();
        let second = build_site_announcement(
            &provider,
            SiteAnnouncementDraft {
                id: "7",
                title: "",
                content: "# 维护通知\n维护时间变更",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: false,
            },
        )
        .unwrap();

        assert_eq!(first.title, "维护通知");
        assert_ne!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn empty_announcement_is_filtered() {
        let provider = provider("provider-1", ProviderProtocol::NewApi);
        assert!(build_site_announcement(
            &provider,
            SiteAnnouncementDraft {
                id: "",
                title: "",
                content: "   ",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: false,
            },
        )
        .is_none());
    }

    #[test]
    fn title_only_announcement_is_preserved() {
        let provider = provider("provider-1", ProviderProtocol::Sub2Api);
        let announcement = build_site_announcement(
            &provider,
            SiteAnnouncementDraft {
                id: "9",
                title: "仅标题公告",
                content: "",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: true,
            },
        )
        .unwrap();

        assert_eq!(announcement.title, "仅标题公告");
        assert!(announcement.content.is_empty());
    }

    #[test]
    fn announcement_fingerprint_is_site_scoped_for_all_supported_protocols() {
        let draft = || SiteAnnouncementDraft {
            id: "7",
            title: "维护通知",
            content: "今晚维护",
            published_at: None,
            updated_at: None,
            read_at: None,
            can_mark_read: true,
        };
        let sub2_first =
            build_site_announcement(&provider("sub2-a", ProviderProtocol::Sub2Api), draft())
                .unwrap();
        let sub2_second =
            build_site_announcement(&provider("sub2-b", ProviderProtocol::Sub2Api), draft())
                .unwrap();
        let new_api_first =
            build_site_announcement(&provider("new-api-a", ProviderProtocol::NewApi), draft())
                .unwrap();
        let new_api_second =
            build_site_announcement(&provider("new-api-b", ProviderProtocol::NewApi), draft())
                .unwrap();

        assert_eq!(sub2_first.fingerprint, sub2_second.fingerprint);
        assert_eq!(new_api_first.fingerprint, new_api_second.fingerprint);
    }

    #[test]
    fn normalized_source_uses_site_origin_instead_of_api_path() {
        assert_eq!(
            normalized_source("https://relay.example.com/api/v1/?from=test#top"),
            "https://relay.example.com"
        );
    }
}
