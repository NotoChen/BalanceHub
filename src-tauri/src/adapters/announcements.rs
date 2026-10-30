use crate::{
    limits,
    models::{Provider, SiteAnnouncement, SiteAnnouncementSource},
};
use reqwest::Url;
use sha2::{Digest, Sha256};

pub(crate) struct SiteAnnouncementDraft<'a> {
    pub id: &'a str,
    pub source: SiteAnnouncementSource,
    pub title: &'a str,
    pub content: &'a str,
    /// 协议给出的公告级别；只有 NewAPI 控制台公告带语义，其余协议留空。
    pub level: &'a str,
    /// 公告正文之外的附加说明，NewAPI 控制台公告单独展示这一段。
    pub extra: &'a str,
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
    let content = bounded_content(draft.content, limits::MAX_ANNOUNCEMENT_CONTENT_CHARS);
    if title.is_empty() && content.is_empty() {
        return None;
    }

    let title_is_derived = title.is_empty();
    let title = if title_is_derived {
        derived_title(&content)
    } else {
        title
    };
    let level = bounded(draft.level, 32).to_ascii_lowercase();
    let extra = bounded(draft.extra, limits::MAX_ANNOUNCEMENT_EXTRA_CHARS);
    let generated_id = digest_hex(format!("{title}\n{content}").as_bytes());
    let id = if draft.id.trim().is_empty() {
        generated_id.chars().take(24).collect()
    } else {
        bounded(draft.id, 240)
    };
    let protocol = serde_json::to_string(&provider.identity.protocol)
        .unwrap_or_else(|_| "unknown".to_string());
    let site = normalized_source(&provider.identity.base_url);
    // BalanceHub 的公告入口按站点汇总。同一协议、同一站点返回的同一条公告，
    // 即使通过不同本地账号读取，也只保留一个本地指纹和一份已读状态。
    let fingerprint = digest_hex(
        format!(
            "{protocol}\n{site}\n{}\n{id}\n{title}\n{content}\n{level}\n{extra}",
            draft.source.key()
        )
        .as_bytes(),
    );

    Some(SiteAnnouncement {
        id,
        fingerprint,
        provider_id: provider.identity.id.clone(),
        provider_name: provider.display_label(),
        provider_protocol: provider.identity.protocol,
        source: draft.source,
        title,
        title_is_derived,
        content,
        level,
        extra,
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

/// 公告正文可能内嵌 data URI 图片（本站有人直接贴 base64 海报），长度以十万
/// 字符计。超限时按标签边界收口：宁可丢掉最后一张图，也不要留下半截
/// `<img src="…` 把后面的正文一起吞掉。
fn bounded_content(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let Some((limit, _)) = trimmed.char_indices().nth(max_chars) else {
        return trimmed.to_string();
    };
    let cut = trimmed[..limit].rfind('>').map_or(limit, |index| index + 1);
    trimmed[..cut].trim_end().to_string()
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
    use crate::models::{
        Provider, ProviderIdentityInput, ProviderInput, ProviderProtocol, SiteAnnouncementSource,
    };

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
                source: SiteAnnouncementSource::Console,
                title: "",
                content: "# 维护通知\n今晚维护",
                level: "warning",
                extra: "维护期间暂停充值",
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
                source: SiteAnnouncementSource::Console,
                title: "",
                content: "# 维护通知\n维护时间变更",
                level: "",
                extra: "",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: false,
            },
        )
        .unwrap();

        assert_eq!(first.title, "维护通知");
        // 标题是从正文第一行提取的，界面不能再渲染一次，否则和正文重复。
        assert!(first.title_is_derived);
        assert_eq!(first.level, "warning");
        assert_eq!(first.extra, "维护期间暂停充值");
        assert_ne!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn announcement_fingerprint_changes_when_only_extra_changes() {
        let provider = provider("provider-1", ProviderProtocol::NewApi);
        let draft = |extra: &'static str| SiteAnnouncementDraft {
            id: "7",
            source: SiteAnnouncementSource::Console,
            title: "维护通知",
            content: "今晚维护",
            level: "warning",
            extra,
            published_at: None,
            updated_at: None,
            read_at: None,
            can_mark_read: false,
        };
        let first = build_site_announcement(&provider, draft("22:00 开始")).unwrap();
        let second = build_site_announcement(&provider, draft("23:00 开始")).unwrap();

        assert_eq!(first.content, second.content);
        assert_ne!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn empty_announcement_is_filtered() {
        let provider = provider("provider-1", ProviderProtocol::NewApi);
        assert!(build_site_announcement(
            &provider,
            SiteAnnouncementDraft {
                id: "",
                source: SiteAnnouncementSource::Console,
                title: "",
                content: "   ",
                level: "",
                extra: "",
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
                source: SiteAnnouncementSource::Console,
                title: "仅标题公告",
                content: "",
                level: "",
                extra: "",
                published_at: None,
                updated_at: None,
                read_at: None,
                can_mark_read: true,
            },
        )
        .unwrap();

        assert_eq!(announcement.title, "仅标题公告");
        // 协议自己给了标题，界面照常单独渲染。
        assert!(!announcement.title_is_derived);
        assert!(announcement.content.is_empty());
    }

    #[test]
    fn announcement_fingerprint_is_site_scoped_for_all_supported_protocols() {
        let draft = || SiteAnnouncementDraft {
            id: "7",
            source: SiteAnnouncementSource::Console,
            title: "维护通知",
            content: "今晚维护",
            level: "",
            extra: "",
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

    #[test]
    fn same_content_from_two_new_api_sources_keeps_two_fingerprints() {
        let provider = provider("provider-1", ProviderProtocol::NewApi);
        let draft = |source| SiteAnnouncementDraft {
            id: "7",
            source,
            title: "维护通知",
            content: "今晚维护",
            level: "",
            extra: "",
            published_at: None,
            updated_at: None,
            read_at: None,
            can_mark_read: false,
        };
        let notice =
            build_site_announcement(&provider, draft(SiteAnnouncementSource::SiteNotice)).unwrap();
        let console =
            build_site_announcement(&provider, draft(SiteAnnouncementSource::Console)).unwrap();

        assert_eq!(notice.source, SiteAnnouncementSource::SiteNotice);
        assert_eq!(console.source, SiteAnnouncementSource::Console);
        assert_ne!(notice.fingerprint, console.fingerprint);
    }

    #[test]
    fn oversized_content_is_cut_at_a_tag_boundary() {
        let image = format!(
            "<p>海报</p><img src=\"data:image/png;base64,{}\"><p>图片之后仍然保留</p>",
            "A".repeat(120)
        );
        let cut = bounded_content(&image, 90);

        // 不留下半截 <img src="…：宁可丢掉最后一张图，也不把后续正文吞进属性值。
        assert_eq!(cut, "<p>海报</p>");
        assert_eq!(bounded_content(&image, 10_000), image);
        // 没有标签时按字符数截断，不能截断多字节字符。
        assert_eq!(bounded_content("维护通知继续", 2), "维护");
    }
}
