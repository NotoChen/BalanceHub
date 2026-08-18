use super::super::definition::{
    auth_field, ProtocolDetectionRole, ProviderAccessTokenAssistantFlow, ProviderAuthFieldSchema,
    ProviderCredentialAssistantDefinition, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
    ProviderProtocolOperationMethods,
};
use crate::{
    adapters::api::ApiAdapter,
    models::{AuthMode, ProviderProtocol},
};

static ADAPTER: ApiAdapter = ApiAdapter;

const API_KEY_FIELDS: &[ProviderAuthFieldSchema] = &[auth_field(
    "apiKey",
    "API Key",
    "粘贴完整 API Key",
    true,
    true,
    false,
    true,
)];

pub(super) const DEFINITION: ProviderProtocolDefinition = ProviderProtocolDefinition {
    kind: ProviderProtocol::Api,
    label: "通用 API Key",
    description: "未知站点的 OpenAI 兼容模型接口，仅支持 API Key",
    detection_role: ProtocolDetectionRole::ApiKeyFallback,
    default_auth_mode: AuthMode::ApiKey,
    auth_schemas: &[ProviderProtocolAuthSchema::new(
        AuthMode::ApiKey,
        "API Key",
        "仅使用 API Key 调用模型接口",
        "",
        &["apiKey"],
        &[],
        API_KEY_FIELDS,
    )],
    operation_methods: ProviderProtocolOperationMethods {
        check_in: None,
        api_keys: None,
        invitation: None,
        models: "OpenAI 兼容模型接口",
        announcements: None,
    },
    credential_assistant: ProviderCredentialAssistantDefinition {
        enabled: false,
        access_token_flow: ProviderAccessTokenAssistantFlow::None,
        api_key_required_fields: &[],
        api_key_required_any_fields: &[],
    },
    credentials: &ADAPTER,
    connection: &ADAPTER,
    access_token: None,
    api_keys: None,
    usage: None,
    account: None,
    capability_probe: &ADAPTER,
    check_in: None,
    announcements: None,
    is_anyrouter: |_| false,
};
