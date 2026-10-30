use super::super::definition::{
    ProtocolDetectionRole, ProviderAccessTokenAssistantFlow, ProviderAuthFieldSchema,
    ProviderCredentialAssistantDefinition, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
    ProviderProtocolOperationMethods,
};
use crate::{
    adapters::oauth::OAuthAdapter,
    models::{AuthMode, ProviderProtocol},
};

static ADAPTER: OAuthAdapter = OAuthAdapter;

const API_KEY_FIELDS: &[ProviderAuthFieldSchema] = &[];

pub(super) const DEFINITION: ProviderProtocolDefinition = ProviderProtocolDefinition {
    kind: ProviderProtocol::OAuth,
    label: "OAuth 账号",
    description: "管理官方账号授权、套餐与用量窗口",
    detection_role: ProtocolDetectionRole::Explicit,
    default_auth_mode: AuthMode::AccessToken,
    auth_schemas: &[ProviderProtocolAuthSchema::new(
        AuthMode::AccessToken,
        "OAuth 账号",
        "管理官方账号授权、套餐与用量窗口",
        "",
        &[],
        &[],
        API_KEY_FIELDS,
    )],
    operation_methods: ProviderProtocolOperationMethods {
        check_in: None,
        api_keys: None,
        invitation: None,
        models: "账号模型目录",
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
