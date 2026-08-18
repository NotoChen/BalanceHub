use super::contracts::{
    AccessTokenCapability, AccountCapability, AnnouncementCapability, ApiKeyManagementCapability,
    CapabilityProbe, CheckInCapability, ConnectionCapability, CredentialCapability,
    UsageCapability,
};
use crate::models::{AuthMode, Provider, ProviderProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolDetectionRole {
    Primary,
    ApiKeyFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderProtocolCapabilities {
    pub(crate) access_token: bool,
    pub(crate) api_key_management: bool,
    pub(crate) usage: bool,
    pub(crate) account: bool,
    pub(crate) check_in: bool,
    pub(crate) announcements: bool,
}

pub(crate) type DialectChecker = fn(&Provider) -> bool;

pub(crate) struct ProviderProtocolAuthSchema {
    pub(crate) mode: AuthMode,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) note: &'static str,
    pub(crate) required_fields: &'static [&'static str],
    pub(crate) optional_fields: &'static [&'static str],
    pub(crate) fields: &'static [ProviderAuthFieldSchema],
}

impl ProviderProtocolAuthSchema {
    pub(crate) const fn new(
        mode: AuthMode,
        label: &'static str,
        description: &'static str,
        note: &'static str,
        required_fields: &'static [&'static str],
        optional_fields: &'static [&'static str],
        fields: &'static [ProviderAuthFieldSchema],
    ) -> Self {
        Self {
            mode,
            label,
            description,
            note,
            required_fields,
            optional_fields,
            fields,
        }
    }
}

/// Field metadata shared with the webview. The protocol adapter owns the
/// semantics; the frontend only renders the declared field and forwards edits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderAuthFieldSchema {
    pub(crate) field: &'static str,
    pub(crate) label: &'static str,
    pub(crate) placeholder: &'static str,
    pub(crate) secret: bool,
    pub(crate) wide: bool,
    pub(crate) readonly: bool,
    pub(crate) show_when_empty: bool,
}

pub(crate) const fn auth_field(
    field: &'static str,
    label: &'static str,
    placeholder: &'static str,
    secret: bool,
    wide: bool,
    readonly: bool,
    show_when_empty: bool,
) -> ProviderAuthFieldSchema {
    ProviderAuthFieldSchema {
        field,
        label,
        placeholder,
        secret,
        wide,
        readonly,
        show_when_empty,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderProtocolDefinition {
    pub(crate) kind: ProviderProtocol,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) detection_role: ProtocolDetectionRole,
    pub(crate) default_auth_mode: AuthMode,
    pub(crate) auth_schemas: &'static [ProviderProtocolAuthSchema],
    pub(crate) operation_methods: ProviderProtocolOperationMethods,
    pub(crate) credential_assistant: ProviderCredentialAssistantDefinition,
    pub(crate) credentials: &'static dyn CredentialCapability,
    pub(crate) connection: &'static dyn ConnectionCapability,
    pub(crate) access_token: Option<&'static dyn AccessTokenCapability>,
    pub(crate) api_keys: Option<&'static dyn ApiKeyManagementCapability>,
    pub(crate) usage: Option<&'static dyn UsageCapability>,
    pub(crate) account: Option<&'static dyn AccountCapability>,
    pub(crate) capability_probe: &'static dyn CapabilityProbe,
    pub(crate) check_in: Option<&'static dyn CheckInCapability>,
    pub(crate) announcements: Option<&'static dyn AnnouncementCapability>,
    pub(crate) is_anyrouter: DialectChecker,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderProtocolOperationMethods {
    pub(crate) check_in: Option<&'static str>,
    pub(crate) api_keys: Option<&'static str>,
    pub(crate) invitation: Option<&'static str>,
    pub(crate) models: &'static str,
    pub(crate) announcements: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProviderAccessTokenAssistantFlow {
    None,
    CredentialCompletion,
    SessionGeneration,
}

impl ProviderAccessTokenAssistantFlow {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CredentialCompletion => "credentialCompletion",
            Self::SessionGeneration => "sessionGeneration",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderCredentialAssistantDefinition {
    pub(crate) enabled: bool,
    pub(crate) access_token_flow: ProviderAccessTokenAssistantFlow,
    pub(crate) api_key_required_fields: &'static [&'static str],
    pub(crate) api_key_required_any_fields: &'static [&'static str],
}

impl ProviderProtocolDefinition {
    pub(crate) fn capabilities(&self) -> ProviderProtocolCapabilities {
        ProviderProtocolCapabilities {
            access_token: self.access_token.is_some(),
            api_key_management: self.api_keys.is_some(),
            usage: self.usage.is_some(),
            account: self.account.is_some(),
            check_in: self.check_in.is_some(),
            announcements: self.announcements.is_some(),
        }
    }

    pub(crate) fn connection(&self) -> &'static dyn ConnectionCapability {
        self.connection
    }

    pub(crate) fn detection_enabled(&self, provider: &Provider) -> bool {
        match self.detection_role {
            ProtocolDetectionRole::Primary => true,
            ProtocolDetectionRole::ApiKeyFallback => {
                matches!(provider.auth.mode, AuthMode::ApiKey)
            }
        }
    }

    pub(crate) fn unsupported(&self, operation: &str) -> String {
        format!("{} 不支持{operation}", self.label)
    }
}
