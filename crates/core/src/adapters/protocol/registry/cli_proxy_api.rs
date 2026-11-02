use super::super::definition::{
    auth_field, ProtocolDetectionRole, ProviderAccessTokenAssistantFlow, ProviderAuthFieldSchema,
    ProviderCredentialAssistantDefinition, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
    ProviderProtocolOperationMethods,
};
use crate::{
    adapters::cli_proxy_api::CliProxyApiAdapter,
    models::{AuthMode, ProviderProtocol},
};

static ADAPTER: CliProxyApiAdapter = CliProxyApiAdapter;

const API_KEY_FIELDS: &[ProviderAuthFieldSchema] = &[auth_field("accessToken", "管理密钥", "CPA 管理接口密钥", true, true, false, true), auth_field("apiKey", "调用 API Key", "可选，用于临时 CLI 和模型调用", true, true, false, true)];

pub(super) const DEFINITION: ProviderProtocolDefinition = ProviderProtocolDefinition {
    kind: ProviderProtocol::CliProxyApi,
    label: "CPA",
    description: "连接 CLIProxyAPI 网关，管理授权与模型",
    detection_role: ProtocolDetectionRole::Explicit,
    default_auth_mode: AuthMode::AccessToken,
    auth_schemas: &[ProviderProtocolAuthSchema::new(
        AuthMode::AccessToken,
        "CPA",
        "连接 CLIProxyAPI 网关，管理授权与模型",
        "",
        &["accessToken"],
        &["apiKey"],
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
        access_token_skip_fields: &[],
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
};
