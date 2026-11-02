use super::contracts::{
    AccessTokenCapability, AccountCapability, AnnouncementCapability, ApiKeyManagementCapability,
    CapabilityProbe, CheckInCapability, ConnectionCapability, CredentialCapability,
    UsageCapability,
};
use crate::models::{AuthMode, Provider, ProviderProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolDetectionRole {
    Explicit,
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
    /// 该协议允许填写的备用地址数量；0 表示请求不经过中转站传输层，备用地
    /// 址填了也不会生效。数量与存储层共用 `limits` 里的同一常量。
    pub(crate) max_backup_urls: usize,
    /// 额外请求头会附加到该协议的每个请求。
    pub(crate) request_headers: bool,
    /// API Key 的发送方式由该协议的请求构造读取。
    pub(crate) api_key_scheme: bool,
    /// 该协议会按卡片声明的余额接口读取额度。
    pub(crate) quota_spec: bool,
    /// 该协议允许声明的订阅窗口数量；0 表示不读取余额接口。
    pub(crate) max_quota_windows: usize,
    /// 该协议会读取卡片声明的模型列表地址。
    pub(crate) models_url: bool,
    /// 该协议允许按接口改写的数量；0 表示没有可改写的请求目录。
    pub(crate) max_endpoint_overrides: usize,
}

pub(crate) struct ProviderProtocolAuthSchema {
    pub(crate) mode: AuthMode,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) note: &'static str,
    pub(crate) required_fields: &'static [&'static str],
    pub(crate) required_any_fields: &'static [&'static str],
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
            required_any_fields: &[],
            optional_fields,
            fields,
        }
    }

    pub(crate) const fn with_required_any_fields(
        mut self,
        fields: &'static [&'static str],
    ) -> Self {
        self.required_any_fields = fields;
        self
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
    pub(crate) access_token_skip_fields: &'static [&'static str],
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
            // 回退发生在 `ProviderTransport`：中转站请求都以填写的主地址为前缀。
            // CPA 管理接口和 OAuth 厂商接口直接使用各自的客户端，备用地址对它们
            // 不会生效，编辑器也就不提供这一项。
            max_backup_urls: if matches!(
                self.kind,
                ProviderProtocol::OAuth | ProviderProtocol::CliProxyApi
            ) {
                0
            } else {
                crate::limits::MAX_BACKUP_URLS_PER_PROVIDER
            },
            // OAuth 账号和 CPA 网关直接使用各自的客户端，不经过
            // `ProviderTransport` 的请求头附加层。
            request_headers: matches!(
                self.kind,
                ProviderProtocol::NewApi | ProviderProtocol::Sub2Api | ProviderProtocol::Api
            ),
            // API Key 发送方式只有通用 OpenAI 兼容协议和余额接口会读取；
            // NewAPI / Sub2API 的凭据形态由协议自己决定。
            api_key_scheme: self.kind == ProviderProtocol::Api,
            quota_spec: self.kind == ProviderProtocol::Api,
            // 窗口数量与存储层共用同一常量，编辑器不再自己写死一个上限。
            max_quota_windows: if self.kind == ProviderProtocol::Api {
                crate::limits::MAX_QUOTA_WINDOWS
            } else {
                0
            },
            models_url: self.kind == ProviderProtocol::Api,
            // 请求目录与传输层的匹配共用 `provider_domain::endpoints`；其他
            // 协议有自己的客户端，不经过按接口改写这一层。
            max_endpoint_overrides: if matches!(
                self.kind,
                ProviderProtocol::NewApi | ProviderProtocol::Sub2Api
            ) {
                crate::limits::MAX_ENDPOINT_OVERRIDES
            } else {
                0
            },
        }
    }

    pub(crate) fn connection(&self) -> &'static dyn ConnectionCapability {
        self.connection
    }

    pub(crate) fn detection_enabled(&self, provider: &Provider) -> bool {
        match self.detection_role {
            ProtocolDetectionRole::Explicit => provider.identity.protocol == self.kind,
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
