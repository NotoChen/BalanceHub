use super::super::definition::{
    auth_field, ProtocolDetectionRole, ProviderAccessTokenAssistantFlow, ProviderAuthFieldSchema,
    ProviderCredentialAssistantDefinition, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
    ProviderProtocolOperationMethods,
};
use crate::{
    adapters::sub2_api::Sub2ApiAdapter,
    models::{AuthMode, ProviderProtocol},
};

static ADAPTER: Sub2ApiAdapter = Sub2ApiAdapter;

const PASSWORD_FIELDS: &[ProviderAuthFieldSchema] = &[
    auth_field("loginUsername", "账号", "邮箱", false, false, false, true),
    auth_field(
        "loginPassword",
        "密码",
        "Sub2API 登录密码",
        true,
        false,
        false,
        true,
    ),
];

const ACCESS_TOKEN_FIELDS: &[ProviderAuthFieldSchema] = &[
    auth_field(
        "accessToken",
        "Access Token",
        "粘贴 Access Token (JWT)",
        true,
        false,
        false,
        true,
    ),
    auth_field(
        "refreshToken",
        "Refresh Token（选填）",
        "填了可在过期前自动续期；留空则过期后需重新获取",
        true,
        false,
        false,
        true,
    ),
];

const API_KEY_FIELDS: &[ProviderAuthFieldSchema] = &[auth_field(
    "apiKey",
    "API Key",
    "粘贴完整 API Key（前缀以站点为准）",
    true,
    true,
    false,
    true,
)];

pub(super) const DEFINITION: ProviderProtocolDefinition = ProviderProtocolDefinition {
    kind: ProviderProtocol::Sub2Api,
    label: "Sub2API",
    description: "JWT 账号与 OpenAI 兼容网关",
    detection_role: ProtocolDetectionRole::Primary,
    default_auth_mode: AuthMode::Password,
    auth_schemas: &[
        ProviderProtocolAuthSchema::new(
            AuthMode::Password,
            "账号密码",
            "登录并缓存 Access / Refresh Token",
            "保存后首次同步会登录站点并缓存访问令牌；启用 2FA 时请先在站点完成登录后粘贴令牌。",
            &["loginUsername", "loginPassword"],
            &["accessToken", "refreshToken", "apiKey"],
            PASSWORD_FIELDS,
        ),
        ProviderProtocolAuthSchema::new(
            AuthMode::AccessToken,
            "Access Token",
            "使用 Access / Refresh Token 调用网关",
            "",
            &["accessToken"],
            &["refreshToken", "apiKey"],
            ACCESS_TOKEN_FIELDS,
        ),
        ProviderProtocolAuthSchema::new(
            AuthMode::ApiKey,
            "API Key",
            "使用网关 API Key 调用模型",
            "",
            &["apiKey"],
            &[],
            API_KEY_FIELDS,
        ),
    ],
    operation_methods: ProviderProtocolOperationMethods {
        check_in: None,
        api_keys: Some("Sub2API 密钥列表接口"),
        invitation: Some("Sub2API 邀请信息接口"),
        models: "GET OpenAI 兼容 /models",
    },
    credential_assistant: ProviderCredentialAssistantDefinition {
        enabled: true,
        access_token_flow: ProviderAccessTokenAssistantFlow::CredentialCompletion,
        api_key_required_fields: &[],
        api_key_required_any_fields: &[],
    },
    credentials: &ADAPTER,
    connection: &ADAPTER,
    access_token: Some(&ADAPTER),
    api_keys: Some(&ADAPTER),
    usage: Some(&ADAPTER),
    account: Some(&ADAPTER),
    capability_probe: &ADAPTER,
    check_in: None,
    is_anyrouter: |_| false,
};
