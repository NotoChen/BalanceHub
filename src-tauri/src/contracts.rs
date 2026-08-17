use serde::Serialize;

use crate::{
    adapters::protocol::ProtocolAdapter,
    models::{
        provider_domain, AppData, AppSettings, Provider, ProviderCapabilityProbeResult,
        ProviderModelSyncResult, ProviderSaveResult, RefreshResult, TemporaryCliPreference,
        Workspace,
    },
};

/// Tauri IPC only view of a provider. The persisted `Provider` remains free of
/// presentation-only derived state; actions are recalculated whenever Rust
/// returns a provider to the webview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: Provider,
    pub actions: ProviderActions,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderActions {
    pub account_management: bool,
    pub check_in: bool,
    pub checked_in_today: bool,
    pub api_key_management: bool,
    pub invitation: bool,
}

impl From<Provider> for ProviderView {
    fn from(provider: Provider) -> Self {
        let is_anyrouter = ProtocolAdapter.is_anyrouter(&provider);
        let actions = ProviderActions {
            account_management: provider_domain::capabilities::supports_account_management(
                &provider,
            ),
            check_in: provider_domain::capabilities::supports_check_in(&provider, is_anyrouter),
            checked_in_today: provider_domain::capabilities::checked_in_today(
                &provider,
                is_anyrouter,
            ),
            api_key_management: provider_domain::capabilities::supports_api_key_management(
                &provider,
            ),
            invitation: provider_domain::capabilities::supports_invitation(&provider),
        };
        Self { provider, actions }
    }
}

pub fn provider_views(providers: Vec<Provider>) -> Vec<ProviderView> {
    providers.into_iter().map(ProviderView::from).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSaveResultView {
    pub providers: Vec<ProviderView>,
    pub saved: bool,
    pub saved_provider_id: Option<String>,
    pub conflict: Option<crate::models::ProviderSaveConflict>,
}

impl From<ProviderSaveResult> for ProviderSaveResultView {
    fn from(result: ProviderSaveResult) -> Self {
        Self {
            providers: provider_views(result.providers),
            saved: result.saved,
            saved_provider_id: result.saved_provider_id,
            conflict: result.conflict,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataView {
    pub schema_version: u32,
    pub providers: Vec<ProviderView>,
    pub settings: AppSettings,
    pub workspaces: Vec<Workspace>,
    pub temporary_cli_preferences: Vec<TemporaryCliPreference>,
}

impl From<AppData> for AppDataView {
    fn from(data: AppData) -> Self {
        Self {
            schema_version: data.schema_version,
            providers: provider_views(data.providers),
            settings: data.settings,
            workspaces: data.workspaces,
            temporary_cli_preferences: data.temporary_cli_preferences,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResultView {
    pub providers: Vec<ProviderView>,
}

impl From<RefreshResult> for RefreshResultView {
    fn from(result: RefreshResult) -> Self {
        Self {
            providers: provider_views(result.providers),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityProbeResultView {
    pub providers: Vec<ProviderView>,
    pub provider: ProviderView,
    pub message: String,
}

impl From<ProviderCapabilityProbeResult> for ProviderCapabilityProbeResultView {
    fn from(result: ProviderCapabilityProbeResult) -> Self {
        Self {
            providers: provider_views(result.providers),
            provider: ProviderView::from(result.provider),
            message: result.message,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSyncResultView {
    pub providers: Vec<ProviderView>,
    pub provider: ProviderView,
    pub models: Vec<String>,
    pub message: String,
}

impl From<ProviderModelSyncResult> for ProviderModelSyncResultView {
    fn from(result: ProviderModelSyncResult) -> Self {
        Self {
            providers: provider_views(result.providers),
            provider: ProviderView::from(result.provider),
            models: result.models,
            message: result.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AuthMode, ProviderInput, ProviderProtocol};

    #[test]
    fn provider_view_adds_rust_owned_actions_without_changing_persisted_model() {
        let mut input = ProviderInput::default();
        input.identity.protocol = ProviderProtocol::Api;
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "sk-test".to_string();
        let view = ProviderView::from(Provider::from_input(input, "provider-1".to_string()));

        assert!(!view.actions.account_management);
        assert!(!view.actions.check_in);
        assert!(!view.actions.api_key_management);
        assert!(!view.actions.invitation);

        let value = serde_json::to_value(view).expect("provider view should serialize");
        assert_eq!(value["identity"]["id"], "provider-1");
        assert_eq!(value["actions"]["checkIn"], false);

        let provider = serde_json::from_value::<Provider>(value)
            .expect("Provider should ignore IPC-only actions on commands sent back to Rust");
        assert_eq!(provider.identity.id, "provider-1");
    }
}
