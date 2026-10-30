mod grok;
mod grok_billing;
pub(crate) mod http;
pub(crate) mod tokens;
pub(super) mod usage;

#[cfg(test)]
mod oauth_tests;

use crate::{
    adapters::{protocol::contracts::*, transport::ProviderTransport},
    models::*,
};
use async_trait::async_trait;

pub(crate) struct OAuthAdapter;

/// How many readings one quota window keeps. The cards draw a short trend and
/// the saved document must not grow with every scheduled refresh.
const QUOTA_HISTORY_SAMPLES: usize = 24;

/// A change smaller than this is below what a percent meter can show, so a
/// refresh that read the same value does not add another point.
const QUOTA_HISTORY_EPSILON: f64 = 0.05;

/// Carry the previous readings forward and append this refresh's value.
///
/// Providers serve only the current quota, so a trend can only come from what
/// BalanceHub itself observed. Windows are matched by ID: a window the vendor
/// reports under a new ID starts a fresh series instead of inheriting another
/// limit's history.
fn record_quota_history(
    next: &mut AccountSubscription,
    previous: Option<&AccountSubscription>,
    at: i64,
) {
    let Some(previous) = previous else { return };
    for window in &mut next.windows {
        // A refresh can omit the percentage while still reporting the window;
        // the series survives so the chart does not lose its history.
        let prior = previous.windows.iter().find(|prior| prior.id == window.id);
        if let Some(prior) = prior {
            window.history.clone_from(&prior.history);
        }
        let Some(percent) = window.used_percent.filter(|value| value.is_finite()) else {
            continue;
        };
        let unchanged = prior.is_some_and(|prior| {
            prior
                .history
                .last()
                .is_some_and(|sample| (sample.used_percent - percent).abs() < QUOTA_HISTORY_EPSILON)
        });
        if unchanged {
            continue;
        }
        window.history.push(AccountQuotaSample {
            at,
            used_percent: percent,
        });
        if window.history.len() > QUOTA_HISTORY_SAMPLES {
            let excess = window.history.len() - QUOTA_HISTORY_SAMPLES;
            window.history.drain(..excess);
        }
    }
}

#[async_trait]
impl CredentialCapability for OAuthAdapter {
    async fn complete_credentials(
        &self,
        _settings: &AppSettings,
        _input: ProviderInput,
        _provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        Err("请通过账号卡片的重新授权或导入授权文件更新凭据".into())
    }
}

#[async_trait]
impl ConnectionCapability for OAuthAdapter {
    async fn fetch_available_models(
        &self,
        client: &ProviderTransport,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderModelList>, String> {
        let models = usage::models(client.non_replaying_client(), provider).await?;
        Ok(ProviderOperationOutcome::unchanged(ProviderModelList::new(
            models,
            ProviderModelScope::Account,
        )))
    }
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        let client = http::client(settings, provider).await?;
        usage::subscription(&client, provider).await?;
        Ok(ProviderOperationOutcome::unchanged(
            ProviderConnectionTestResult {
                ok: true,
                message: "账号已连接".into(),
                available: None,
                used: None,
                quota_display: ProviderQuotaDisplay::default(),
                steps: Vec::new(),
            },
        ))
    }
    async fn probe_site(
        &self,
        _settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        Ok(ProviderSiteProbeResult {
            ok: provider.auth.oauth.is_some(),
            message: "OAuth 账号".into(),
            system_name: Some(provider.identity.name.clone()),
            logo: None,
            quota_display: ProviderQuotaDisplay::default(),
        })
    }
    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        let mut next = provider.clone();
        next.quota.known = false;
        next.quota.total_known = false;
        let outcome = async {
            let client = http::client(settings, provider).await?;
            // Profile reads do not rotate credentials. Apply their metadata with
            // the same revision-checked outcome as quotas and model observations.
            let identity_result = tokens::load_identity(&client, &mut next.auth).await;
            if let Some(identity) = &next.auth.oauth {
                next.identity.username.clone_from(&identity.email);
                next.identity.display_name.clone_from(&identity.email);
            }
            let (subscription, models) = if provider
                .auth
                .oauth
                .as_ref()
                .is_some_and(|o| o.vendor == OAuthVendor::Gemini)
            {
                let subscription = usage::subscription(&client, provider).await;
                let models = subscription
                    .as_ref()
                    .map(|s| {
                        let mut models: Vec<_> =
                            s.windows.iter().filter_map(|w| w.model.clone()).collect();
                        models.sort();
                        models.dedup();
                        models
                    })
                    .map_err(Clone::clone);
                (subscription, models)
            } else {
                tokio::join!(
                    usage::subscription(&client, provider),
                    usage::models(&client, provider)
                )
            };
            match models {
                Ok(models) => next
                    .capabilities
                    .set_available_models(ProviderModelList::new(
                        models,
                        ProviderModelScope::Account,
                    )),
                Err(error) => next.capabilities.available_models_state.error = Some(error),
            }
            let mut snapshot = subscription?;
            if let Some(identity) = &mut next.auth.oauth {
                if !snapshot.project_id.is_empty() {
                    identity.project_id.clone_from(&snapshot.project_id);
                }
                if !snapshot.plan.is_empty() {
                    identity.plan.clone_from(&snapshot.plan);
                } else {
                    snapshot.plan.clone_from(&identity.plan);
                }
                snapshot
                    .organization
                    .clone_from(&identity.organization_name);
            }
            if let Err(error) = identity_result {
                snapshot.error = Some(match snapshot.error {
                    Some(usage_error) => format!("{usage_error}；{error}"),
                    None => error,
                });
            }
            record_quota_history(
                &mut snapshot,
                provider.capabilities.subscription.as_ref(),
                crate::util::unix_secs() as i64,
            );
            snapshot.fill_missing_percentages();
            snapshot.sort_windows_by_pressure();
            next.capabilities.subscription = Some(snapshot);
            Ok::<(), String>(())
        }
        .await;
        next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
        match outcome {
            Ok(()) => {
                next.runtime.error_message = next
                    .capabilities
                    .subscription
                    .as_ref()
                    .and_then(|s| s.error.clone());
                next.runtime.status = if next.runtime.error_message.is_some() {
                    ProviderStatus::Warning
                } else {
                    ProviderStatus::Ok
                };
            }
            Err(message) => {
                let snapshot = next
                    .capabilities
                    .subscription
                    .get_or_insert_with(AccountSubscription::default);
                snapshot.error = Some(message.clone());
                next.runtime.status = ProviderStatus::Warning;
                next.runtime.error_message = Some(message);
            }
        }
        ProviderOperationOutcome::refreshed(provider, next)
    }
}

#[async_trait]
impl CapabilityProbe for OAuthAdapter {
    async fn probe_capabilities(
        &self,
        _settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        let mut capabilities = provider.capabilities.clone();
        capabilities.check_in_known = true;
        capabilities.api_key_management_known = true;
        capabilities.invitation_known = true;
        Ok(ProviderOperationOutcome::unchanged((
            capabilities,
            "账号授权已识别".into(),
            None,
        )))
    }
}
