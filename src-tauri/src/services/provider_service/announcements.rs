use crate::{
    adapters::{announcements::normalized_source, protocol, protocol::ProtocolAdapter},
    models::{
        AppSettings, Provider, ProviderProtocol, SiteAnnouncement, SiteAnnouncementSourceError,
        SiteAnnouncementsSnapshot,
    },
    util::unix_millis,
};
use futures_util::{stream, StreamExt};
use std::collections::{HashMap, HashSet};
use tauri::Manager;

use super::{ProviderRequestContext, ProviderService};

const ANNOUNCEMENT_SOURCE_CONCURRENCY: usize = 4;

struct AnnouncementSourceGroup {
    providers: Vec<Provider>,
}

struct AnnouncementSourceResult {
    source: Provider,
    announcements: Vec<SiteAnnouncement>,
    error: Option<String>,
}

impl<'a> ProviderService<'a> {
    pub async fn site_announcements(&self) -> Result<SiteAnnouncementsSnapshot, String> {
        let data = self.snapshot_async().await?;
        let settings = data.settings.clone();
        let mut results = stream::iter(
            announcement_source_groups(&data.providers)
                .into_iter()
                .enumerate(),
        )
        .map(|(index, group)| {
            let settings = settings.clone();
            async move { (index, self.load_announcement_source(&settings, group).await) }
        })
        .buffer_unordered(ANNOUNCEMENT_SOURCE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        results.sort_by_key(|(index, _)| *index);

        let mut announcements = Vec::new();
        let mut errors = Vec::new();
        for (_, result) in results {
            announcements.extend(result.announcements);
            if let Some(message) = result.error {
                let provider_name = result.source.display_label();
                errors.push(SiteAnnouncementSourceError {
                    provider_id: result.source.identity.id,
                    provider_name,
                    provider_protocol: result.source.identity.protocol,
                    message,
                });
            }
        }

        let mut fingerprints = HashSet::new();
        announcements.retain(|item| fingerprints.insert(item.fingerprint.clone()));
        Ok(SiteAnnouncementsSnapshot {
            fetched_at: unix_millis().to_string(),
            announcements,
            errors,
        })
    }

    async fn load_announcement_source(
        &self,
        settings: &AppSettings,
        group: AnnouncementSourceGroup,
    ) -> AnnouncementSourceResult {
        let mut providers = group.providers;
        providers.sort_by_key(announcement_provider_priority);
        let source = providers
            .first()
            .cloned()
            .expect("announcement source groups must not be empty");

        // NewAPI 公告是公开只读接口。Sub2API 的公告位于 JWT 保护路由之后，
        // 且访问令牌可能滚动刷新；保留认证闸门，避免同一进程中的旋转令牌竞态。
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = if matches!(source.identity.protocol, ProviderProtocol::Sub2Api) {
            Some(state.refresh_gate.lock().await)
        } else {
            None
        };

        let mut attempt_errors = Vec::new();
        for provider in providers {
            let request_context = ProviderRequestContext::capture(&provider);
            match ProtocolAdapter
                .list_announcements(settings, &provider)
                .await
            {
                Ok(operation) => {
                    match self
                        .persist_operation_credentials(&request_context, &operation.credentials)
                        .await
                    {
                        Ok(Some(_)) => {
                            return AnnouncementSourceResult {
                                source,
                                announcements: operation.value,
                                error: None,
                            };
                        }
                        Ok(None) => {
                            attempt_errors.push("本地配置已变更，公告结果已忽略".to_string())
                        }
                        Err(error) => attempt_errors.push(format!("凭据写回失败：{error}")),
                    }
                }
                Err(message) => attempt_errors.push(message),
            }
        }

        AnnouncementSourceResult {
            source,
            announcements: Vec::new(),
            error: Some(compact_errors(attempt_errors)),
        }
    }

    pub async fn mark_site_announcement_read(
        &self,
        provider_id: String,
        announcement_id: String,
    ) -> Result<(), String> {
        let state = self.app.state::<crate::state::AppState>();
        let _network_gate = state.refresh_gate.lock().await;
        let data = self.snapshot_async().await?;
        let provider = super::find_provider(&data, &provider_id)?;
        let request_context = ProviderRequestContext::capture(&provider);
        let operation = ProtocolAdapter
            .mark_announcement_read(&data.settings, &provider, &announcement_id)
            .await?;
        self.persist_operation_credentials(&request_context, &operation.credentials)
            .await?
            .ok_or_else(|| "本地配置已变更，公告已读状态未同步".to_string())?;
        Ok(())
    }
}

fn announcement_source_groups(providers: &[Provider]) -> Vec<AnnouncementSourceGroup> {
    let mut groups = Vec::<AnnouncementSourceGroup>::new();
    let mut indexes = HashMap::<String, usize>::new();
    for provider in providers.iter().filter(|provider| provider.runtime.enabled) {
        if protocol::definition(provider.identity.protocol)
            .announcements
            .is_none()
        {
            continue;
        }
        let key = announcement_source_key(provider);
        if let Some(index) = indexes.get(&key).copied() {
            groups[index].providers.push(provider.clone());
        } else {
            indexes.insert(key, groups.len());
            groups.push(AnnouncementSourceGroup {
                providers: vec![provider.clone()],
            });
        }
    }
    groups
}

fn announcement_source_key(provider: &Provider) -> String {
    match provider.identity.protocol {
        // 公告入口按「协议 + 站点 origin」分组。Sub2API 仍需从一个可用账号
        // 读取受保护接口，但同站点的多个账号只呈现一份公告入口。
        ProviderProtocol::NewApi => {
            format!("newApi:{}", normalized_source(&provider.identity.base_url))
        }
        ProviderProtocol::Sub2Api => {
            format!("sub2Api:{}", normalized_source(&provider.identity.base_url))
        }
        ProviderProtocol::Api => format!("api:{}", normalized_source(&provider.identity.base_url)),
    }
}

fn announcement_provider_priority(provider: &Provider) -> u8 {
    match provider.identity.protocol {
        ProviderProtocol::Sub2Api => {
            if !provider.auth.access_token.trim().is_empty() {
                0
            } else if !provider.auth.refresh_token.trim().is_empty() {
                1
            } else if !provider.auth.login_username.trim().is_empty() {
                2
            } else {
                3
            }
        }
        _ => 0,
    }
}

fn compact_errors(errors: Vec<String>) -> String {
    if errors.is_empty() {
        return "站点公告加载失败".to_string();
    }
    let mut unique = HashSet::new();
    errors
        .into_iter()
        .filter(|message| unique.insert(message.clone()))
        .take(3)
        .collect::<Vec<_>>()
        .join("；")
        .chars()
        .take(600)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderIdentityInput, ProviderInput};

    fn provider(id: &str, base_url: &str, protocol: ProviderProtocol) -> Provider {
        Provider::from_input(
            ProviderInput {
                identity: ProviderIdentityInput {
                    base_url: base_url.to_string(),
                    protocol,
                    ..ProviderIdentityInput::default()
                },
                ..ProviderInput::default()
            },
            id.to_string(),
        )
    }

    #[test]
    fn same_protocol_and_site_share_one_source_with_account_fallbacks() {
        let providers = vec![
            provider("a", "https://relay.example.com/", ProviderProtocol::NewApi),
            provider(
                "b",
                "https://relay.example.com/api",
                ProviderProtocol::NewApi,
            ),
            provider("c", "https://relay.example.com", ProviderProtocol::Sub2Api),
            provider(
                "d",
                "https://relay.example.com/api/v1",
                ProviderProtocol::Sub2Api,
            ),
        ];
        let groups = announcement_source_groups(&providers);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].providers.len(), 2);
        assert_eq!(groups[1].providers.len(), 2);
        assert_eq!(
            announcement_source_key(&groups[0].providers[0]),
            announcement_source_key(&groups[0].providers[1])
        );
        assert_eq!(
            announcement_source_key(&groups[1].providers[0]),
            announcement_source_key(&groups[1].providers[1])
        );
    }
}
