use super::contracts::{
    AccessTokenCapability, AccountCapability, AccountProfileCapability,
    AccountRedemptionCapability, AnnouncementCapability, ApiKeyManagementCapability,
    CapabilityProbe, CheckInCapability, ConnectionCapability, CredentialCapability,
    UsageCapability,
};
use crate::models::{AuthMode, Provider, ProviderProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolDetectionRole {
    Explicit,
    Primary,
    ApiKeyFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderProtocolCapabilities {
    pub access_token: bool,
    pub api_key_management: bool,
    pub usage: bool,
    pub account: bool,
    pub check_in: bool,
    pub announcements: bool,
    /// 该协议允许填写的备用地址数量；0 表示请求不经过中转站传输层，备用地
    /// 址填了也不会生效。数量与存储层共用 `limits` 里的同一常量。
    pub max_backup_urls: usize,
    /// 额外请求头会附加到该协议的每个请求。
    pub request_headers: bool,
    /// API Key 的发送方式由该协议的请求构造读取。
    pub api_key_scheme: bool,
    /// 该协议会按卡片声明的余额接口读取额度。
    pub quota_spec: bool,
    /// 该协议允许声明的订阅窗口数量；0 表示不读取余额接口。
    pub max_quota_windows: usize,
    /// 该协议会读取卡片声明的模型列表地址。
    pub models_url: bool,
    /// 该协议允许按接口改写的数量；0 表示没有可改写的请求目录。
    pub max_endpoint_overrides: usize,
    /// 请求日志实际支持的筛选维度。界面只显示这里声明的控件，避免把协议
    /// 不认识的字段发出去后仍显示“已筛选”。
    pub request_log_filters: &'static [crate::models::ProviderRequestLogFilter],
    /// 该协议允许在应用内修改账号资料。
    pub account_profile: bool,
    /// 该协议提供余额兑换码入口。
    pub account_redemption: bool,
}

pub struct ProviderProtocolAuthSchema {
    pub mode: AuthMode,
    pub label: &'static str,
    pub description: &'static str,
    pub note: &'static str,
    pub required_fields: &'static [&'static str],
    pub required_any_fields: &'static [&'static str],
    pub optional_fields: &'static [&'static str],
    pub fields: &'static [ProviderAuthFieldSchema],
}

impl ProviderProtocolAuthSchema {
    pub const fn new(
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

    pub const fn with_required_any_fields(mut self, fields: &'static [&'static str]) -> Self {
        self.required_any_fields = fields;
        self
    }
}

/// Field metadata shared with the webview. The protocol adapter owns the
/// semantics; the frontend only renders the declared field and forwards edits.
#[derive(Debug, Clone, Copy)]
pub struct ProviderAuthFieldSchema {
    pub field: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub secret: bool,
    pub wide: bool,
    pub readonly: bool,
    pub show_when_empty: bool,
}

pub const fn auth_field(
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
pub struct ProviderProtocolDefinition {
    pub kind: ProviderProtocol,
    pub label: &'static str,
    pub description: &'static str,
    pub detection_role: ProtocolDetectionRole,
    pub default_auth_mode: AuthMode,
    pub auth_schemas: &'static [ProviderProtocolAuthSchema],
    pub operation_methods: ProviderProtocolOperationMethods,
    pub credential_assistant: ProviderCredentialAssistantDefinition,
    pub credentials: &'static dyn CredentialCapability,
    pub connection: &'static dyn ConnectionCapability,
    pub access_token: Option<&'static dyn AccessTokenCapability>,
    pub api_keys: Option<&'static dyn ApiKeyManagementCapability>,
    pub usage: Option<&'static dyn UsageCapability>,
    pub account: Option<&'static dyn AccountCapability>,
    pub account_profile: Option<&'static dyn AccountProfileCapability>,
    pub account_redemption: Option<&'static dyn AccountRedemptionCapability>,
    pub capability_probe: &'static dyn CapabilityProbe,
    pub check_in: Option<&'static dyn CheckInCapability>,
    pub announcements: Option<&'static dyn AnnouncementCapability>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderProtocolOperationMethods {
    pub check_in: Option<&'static str>,
    pub api_keys: Option<&'static str>,
    pub invitation: Option<&'static str>,
    pub models: &'static str,
    pub announcements: Option<&'static str>,
    pub pricing: Option<&'static str>,
    pub usage: Option<&'static str>,
    pub request_logs: Option<&'static str>,
    pub subscription: Option<&'static str>,
    pub groups: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub enum ProviderAccessTokenAssistantFlow {
    None,
    CredentialCompletion,
    SessionGeneration,
}

impl ProviderAccessTokenAssistantFlow {
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::CredentialCompletion => "credentialCompletion",
            Self::SessionGeneration => "sessionGeneration",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderCredentialAssistantDefinition {
    pub enabled: bool,
    pub access_token_flow: ProviderAccessTokenAssistantFlow,
    pub access_token_skip_fields: &'static [&'static str],
    pub api_key_required_fields: &'static [&'static str],
    pub api_key_required_any_fields: &'static [&'static str],
}

impl ProviderProtocolDefinition {
    pub fn capabilities(&self) -> ProviderProtocolCapabilities {
        ProviderProtocolCapabilities {
            access_token: self.access_token.is_some(),
            api_key_management: self.api_keys.is_some(),
            usage: self.usage.is_some(),
            account: self.account.is_some(),
            account_profile: self.account_profile.is_some(),
            account_redemption: self.account_redemption.is_some(),
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
            request_log_filters: match self.kind {
                ProviderProtocol::NewApi => &[
                    crate::models::ProviderRequestLogFilter::Model,
                    crate::models::ProviderRequestLogFilter::TokenName,
                    crate::models::ProviderRequestLogFilter::Group,
                    crate::models::ProviderRequestLogFilter::LogType,
                    crate::models::ProviderRequestLogFilter::TimeRange,
                    crate::models::ProviderRequestLogFilter::RequestId,
                ],
                ProviderProtocol::Sub2Api => &[
                    crate::models::ProviderRequestLogFilter::Model,
                    crate::models::ProviderRequestLogFilter::TimeRange,
                ],
                _ => &[],
            },
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

    pub fn connection(&self) -> &'static dyn ConnectionCapability {
        self.connection
    }

    pub fn detection_enabled(&self, provider: &Provider) -> bool {
        match self.detection_role {
            ProtocolDetectionRole::Explicit => provider.identity.protocol == self.kind,
            ProtocolDetectionRole::Primary => true,
            ProtocolDetectionRole::ApiKeyFallback => {
                matches!(provider.auth.mode, AuthMode::ApiKey)
            }
        }
    }

    pub fn unsupported(&self, operation: &str) -> String {
        format!("{} 不支持{operation}", self.label)
    }
}
