use serde::Serialize;

use crate::{
    adapters::protocol::{self, ProtocolAdapter},
    models::{
        provider_domain, AppData, AppDataTransferResult, AppSettings, AuthMode, Provider,
        ProviderCapabilityProbeResult, ProviderModelSyncResult, ProviderProtocol,
        ProviderSaveResult, RefreshResult, TemporaryCliPreference, Workspace,
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
    pub revision: u64,
    pub display_label: String,
    pub protocol_label: &'static str,
    pub protocol_description: &'static str,
    pub auth_mode_label: &'static str,
    pub auth_mode_description: &'static str,
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
    pub refresh_models_only: bool,
}

impl From<Provider> for ProviderView {
    fn from(provider: Provider) -> Self {
        let revision = provider.revision;
        let display_label = provider.display_label();
        let protocol_definition = protocol::definition(provider.identity.protocol);
        let auth_schema = protocol_definition
            .auth_schemas
            .iter()
            .find(|schema| schema.mode == provider.auth.mode);
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
            refresh_models_only: !protocol_definition.capabilities().account,
        };
        Self {
            provider,
            revision,
            display_label,
            protocol_label: protocol_definition.label,
            protocol_description: protocol_definition.description,
            auth_mode_label: auth_schema.map_or("认证凭据", |schema| schema.label),
            auth_mode_description: auth_schema.map_or("", |schema| schema.description),
            actions,
        }
    }
}

pub fn provider_views(providers: Vec<Provider>) -> Vec<ProviderView> {
    providers.into_iter().map(ProviderView::from).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthModeDescriptorView {
    pub mode: AuthMode,
    pub label: &'static str,
    pub description: &'static str,
    pub note: &'static str,
    pub required_fields: Vec<&'static str>,
    pub optional_fields: Vec<&'static str>,
    pub fields: Vec<ProviderAuthFieldDescriptorView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthFieldDescriptorView {
    pub field: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub secret: bool,
    pub wide: bool,
    pub readonly: bool,
    pub show_when_empty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProtocolCapabilitiesView {
    pub access_token: bool,
    pub api_key_management: bool,
    pub usage: bool,
    pub account: bool,
    pub check_in: bool,
    pub announcements: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProtocolOperationMethodsView {
    pub check_in: Option<&'static str>,
    pub api_keys: Option<&'static str>,
    pub invitation: Option<&'static str>,
    pub models: &'static str,
    pub announcements: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialAssistantDescriptorView {
    pub enabled: bool,
    pub access_token_flow: &'static str,
    pub api_key_required_fields: Vec<&'static str>,
    pub api_key_required_any_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProtocolDescriptorView {
    pub kind: ProviderProtocol,
    pub label: &'static str,
    pub description: &'static str,
    pub default_auth_mode: AuthMode,
    pub auth_modes: Vec<ProviderAuthModeDescriptorView>,
    pub capabilities: ProviderProtocolCapabilitiesView,
    pub operation_methods: ProviderProtocolOperationMethodsView,
    pub credential_assistant: ProviderCredentialAssistantDescriptorView,
}

pub fn provider_protocol_views() -> Vec<ProviderProtocolDescriptorView> {
    protocol::definitions()
        .iter()
        .map(|definition| {
            let capabilities = definition.capabilities();
            ProviderProtocolDescriptorView {
                kind: definition.kind,
                label: definition.label,
                description: definition.description,
                default_auth_mode: definition.default_auth_mode,
                auth_modes: definition.auth_schemas.iter().map(auth_mode_view).collect(),
                capabilities: ProviderProtocolCapabilitiesView {
                    access_token: capabilities.access_token,
                    api_key_management: capabilities.api_key_management,
                    usage: capabilities.usage,
                    account: capabilities.account,
                    check_in: capabilities.check_in,
                    announcements: capabilities.announcements,
                },
                operation_methods: ProviderProtocolOperationMethodsView {
                    check_in: definition.operation_methods.check_in,
                    api_keys: definition.operation_methods.api_keys,
                    invitation: definition.operation_methods.invitation,
                    models: definition.operation_methods.models,
                    announcements: definition.operation_methods.announcements,
                },
                credential_assistant: ProviderCredentialAssistantDescriptorView {
                    enabled: definition.credential_assistant.enabled,
                    access_token_flow: definition.credential_assistant.access_token_flow.key(),
                    api_key_required_fields: definition
                        .credential_assistant
                        .api_key_required_fields
                        .to_vec(),
                    api_key_required_any_fields: definition
                        .credential_assistant
                        .api_key_required_any_fields
                        .to_vec(),
                },
            }
        })
        .collect()
}

fn auth_mode_view(schema: &protocol::ProviderProtocolAuthSchema) -> ProviderAuthModeDescriptorView {
    ProviderAuthModeDescriptorView {
        mode: schema.mode,
        label: schema.label,
        description: schema.description,
        note: schema.note,
        required_fields: schema.required_fields.to_vec(),
        optional_fields: schema.optional_fields.to_vec(),
        fields: schema
            .fields
            .iter()
            .map(|field| ProviderAuthFieldDescriptorView {
                field: field.field,
                label: field.label,
                placeholder: field.placeholder,
                secret: field.secret,
                wide: field.wide,
                readonly: field.readonly,
                show_when_empty: field.show_when_empty,
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSaveResultView {
    pub saved: bool,
    pub provider: Option<ProviderView>,
    pub conflict: Option<crate::models::ProviderSaveConflict>,
}

impl From<ProviderSaveResult> for ProviderSaveResultView {
    fn from(result: ProviderSaveResult) -> Self {
        Self {
            saved: result.saved,
            provider: result.provider.map(ProviderView::from),
            conflict: result.conflict,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataView {
    pub revision: u64,
    pub schema_version: u32,
    pub providers: Vec<ProviderView>,
    pub provider_protocols: Vec<ProviderProtocolDescriptorView>,
    pub settings: AppSettings,
    pub workspaces: Vec<Workspace>,
    pub temporary_cli_preferences: Vec<TemporaryCliPreference>,
}

impl From<AppData> for AppDataView {
    fn from(data: AppData) -> Self {
        Self {
            revision: data.revision,
            schema_version: data.schema_version,
            providers: provider_views(data.providers),
            provider_protocols: provider_protocol_views(),
            settings: data.settings,
            workspaces: data.workspaces,
            temporary_cli_preferences: data.temporary_cli_preferences,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDataImportResultView {
    pub data: AppDataView,
    pub transfer: AppDataTransferResult,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResultView {
    pub updated_providers: Vec<ProviderView>,
}

impl From<RefreshResult> for RefreshResultView {
    fn from(result: RefreshResult) -> Self {
        Self {
            updated_providers: provider_views(result.updated_providers),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilityProbeResultView {
    pub provider: ProviderView,
    pub message: String,
}

impl From<ProviderCapabilityProbeResult> for ProviderCapabilityProbeResultView {
    fn from(result: ProviderCapabilityProbeResult) -> Self {
        Self {
            provider: ProviderView::from(result.provider),
            message: result.message,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSyncResultView {
    pub provider: ProviderView,
    pub models: Vec<String>,
    pub message: String,
}

impl From<ProviderModelSyncResult> for ProviderModelSyncResultView {
    fn from(result: ProviderModelSyncResult) -> Self {
        Self {
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
        input.identity.name = "Relay Site".to_string();
        input.auth.mode = AuthMode::ApiKey;
        input.auth.api_key = "sk-test".to_string();
        let view = ProviderView::from(Provider::from_input(input, "provider-1".to_string()));

        assert!(!view.actions.account_management);
        assert!(!view.actions.check_in);
        assert!(!view.actions.api_key_management);
        assert!(!view.actions.invitation);
        assert!(view.actions.refresh_models_only);
        assert_eq!(view.protocol_label, "通用 API Key");
        assert_eq!(view.auth_mode_label, "API Key");
        assert_eq!(view.display_label, "Relay Site");

        let value = serde_json::to_value(view).expect("provider view should serialize");
        assert_eq!(value["identity"]["id"], "provider-1");
        assert_eq!(value["displayLabel"], "Relay Site");
        assert_eq!(value["actions"]["checkIn"], false);

        let provider = serde_json::from_value::<Provider>(value)
            .expect("Provider should ignore IPC-only actions on commands sent back to Rust");
        assert_eq!(provider.identity.id, "provider-1");
    }

    #[test]
    fn provider_view_uses_one_display_label_rule_for_account_and_api_key_cards() {
        let mut account_input = ProviderInput::default();
        account_input.identity.name = "Relay Site".to_string();
        account_input.identity.remark = "Claude 主用".to_string();
        let account = ProviderView::from(Provider::from_input(
            account_input,
            "provider-account".to_string(),
        ));
        assert_eq!(account.display_label, "Relay Site · Claude 主用");

        let mut api_key_input = ProviderInput::default();
        api_key_input.identity.name = "Relay Site".to_string();
        api_key_input.identity.remark = "Codex 备用".to_string();
        api_key_input.identity.protocol = ProviderProtocol::Api;
        api_key_input.auth.mode = AuthMode::ApiKey;
        let api_key = ProviderView::from(Provider::from_input(
            api_key_input,
            "provider-api-key".to_string(),
        ));
        assert_eq!(api_key.display_label, "Codex 备用");
    }

    #[test]
    fn protocol_views_expose_auth_fields_and_assistant_rules_from_rust() {
        let views = provider_protocol_views();
        let new_api = views
            .iter()
            .find(|view| view.kind == ProviderProtocol::NewApi)
            .expect("NewAPI descriptor");
        let access_token = new_api
            .auth_modes
            .iter()
            .find(|mode| mode.mode == AuthMode::AccessToken)
            .expect("NewAPI access token schema");

        assert_eq!(access_token.required_fields, ["accessToken", "apiUser"]);
        assert!(access_token.note.is_empty());
        assert_eq!(
            access_token
                .fields
                .iter()
                .map(|field| field.field)
                .collect::<Vec<_>>(),
            ["accessToken", "apiUser"]
        );
        assert_eq!(
            new_api.credential_assistant.access_token_flow,
            "sessionGeneration"
        );
        assert_eq!(
            new_api.credential_assistant.api_key_required_fields,
            ["apiUser"]
        );
        assert_eq!(new_api.operation_methods.api_keys, Some("GET /api/token/"));
        let password = new_api
            .auth_modes
            .iter()
            .find(|mode| mode.mode == AuthMode::Password)
            .expect("NewAPI password schema");
        assert!(password.note.contains("2FA"));

        let generic = views
            .iter()
            .find(|view| view.kind == ProviderProtocol::Api)
            .expect("generic API descriptor");
        assert!(!generic.credential_assistant.enabled);
        assert!(generic.operation_methods.api_keys.is_none());
    }

    #[test]
    fn app_data_ipc_serializes_the_complete_protocol_registry() {
        let value = serde_json::to_value(AppDataView::from(AppData::default()))
            .expect("app data IPC view should serialize");
        let protocols = value["providerProtocols"]
            .as_array()
            .expect("providerProtocols should be an array");

        assert_eq!(protocols.len(), ProviderProtocol::ALL.len());
        assert!(protocols.iter().all(|protocol| {
            protocol["kind"].is_string()
                && protocol["label"].is_string()
                && protocol["defaultAuthMode"].is_string()
                && protocol["authModes"].is_array()
                && protocol["capabilities"].is_object()
                && protocol["operationMethods"]["models"].is_string()
                && protocol["credentialAssistant"]["accessTokenFlow"].is_string()
        }));

        let new_api = protocols
            .iter()
            .find(|protocol| protocol["kind"] == "newApi")
            .expect("serialized NewAPI descriptor");
        let password = new_api["authModes"]
            .as_array()
            .and_then(|modes| modes.iter().find(|mode| mode["mode"] == "password"))
            .expect("serialized password schema");
        assert!(password["note"]
            .as_str()
            .is_some_and(|note| note.contains("2FA")));
        assert_eq!(password["fields"][0]["field"], "loginUsername");
        assert_eq!(password["fields"][0]["showWhenEmpty"], true);
    }
}
