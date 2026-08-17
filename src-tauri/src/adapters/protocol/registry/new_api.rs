use super::super::definition::{
    auth_field, ProtocolDetectionRole, ProviderAccessTokenAssistantFlow, ProviderAuthFieldSchema,
    ProviderCredentialAssistantDefinition, ProviderProtocolAuthSchema, ProviderProtocolDefinition,
    ProviderProtocolOperationMethods,
};
use crate::{
    adapters::new_api::{provider_is_anyrouter, NewApiAdapter},
    models::{AuthMode, ProviderProtocol},
};

static ADAPTER: NewApiAdapter = NewApiAdapter;

const PASSWORD_FIELDS: &[ProviderAuthFieldSchema] = &[
    auth_field(
        "loginUsername",
        "账号",
        "用户名或邮箱",
        false,
        false,
        false,
        true,
    ),
    auth_field(
        "loginPassword",
        "密码",
        "NewAPI 登录密码",
        true,
        false,
        false,
        true,
    ),
    auth_field(
        "apiUser",
        "登录后用户 ID",
        "登录后自动读取",
        false,
        true,
        true,
        false,
    ),
];

const SESSION_FIELDS: &[ProviderAuthFieldSchema] = &[
    auth_field(
        "sessionCookie",
        "会话 Cookie",
        "session=xxx 或直接粘贴 Cookie 值",
        true,
        true,
        false,
        true,
    ),
    auth_field(
        "apiUser",
        "API User ID",
        "自动解析，也可手动填写",
        false,
        false,
        false,
        true,
    ),
];

const ACCESS_TOKEN_FIELDS: &[ProviderAuthFieldSchema] = &[
    auth_field(
        "accessToken",
        "访问令牌",
        "粘贴访问令牌",
        true,
        false,
        false,
        true,
    ),
    auth_field(
        "apiUser",
        "API User ID",
        "输入用户 ID",
        false,
        false,
        false,
        true,
    ),
];

const API_KEY_FIELDS: &[ProviderAuthFieldSchema] = &[auth_field(
    "apiKey",
    "API Key",
    "粘贴 API Key（可不含 sk-）",
    true,
    true,
    false,
    true,
)];

pub(super) const DEFINITION: ProviderProtocolDefinition = ProviderProtocolDefinition {
    kind: ProviderProtocol::NewApi,
    label: "NewAPI",
    description: "兼容 NewAPI / AnyRouter 协议",
    detection_role: ProtocolDetectionRole::Primary,
    default_auth_mode: AuthMode::Password,
    auth_schemas: &[
        ProviderProtocolAuthSchema::new(
            AuthMode::Password,
            "账号密码",
            "登录并建立会话",
            "保存后首次同步会登录站点并缓存会话；开启 2FA 或验证码的站点请改用 Cookie。",
            &["loginUsername", "loginPassword"],
            &["sessionCookie", "apiUser", "accessToken", "apiKey"],
            PASSWORD_FIELDS,
        ),
        ProviderProtocolAuthSchema::new(
            AuthMode::Session,
            "Cookie",
            "已有浏览器会话",
            "",
            &["sessionCookie"],
            &["apiUser", "accessToken", "apiKey"],
            SESSION_FIELDS,
        ),
        ProviderProtocolAuthSchema::new(
            AuthMode::AccessToken,
            "访问令牌",
            "账号接口令牌",
            "",
            &["accessToken", "apiUser"],
            &["apiKey"],
            ACCESS_TOKEN_FIELDS,
        ),
        ProviderProtocolAuthSchema::new(
            AuthMode::ApiKey,
            "API Key",
            "仅密钥额度",
            "",
            &["apiKey"],
            &[],
            API_KEY_FIELDS,
        ),
    ],
    operation_methods: ProviderProtocolOperationMethods {
        check_in: Some("GET /api/user/checkin?month=YYYY-MM"),
        api_keys: Some("GET /api/token/"),
        invitation: Some("GET /api/user/aff"),
        models: "GET OpenAI 兼容 /models",
    },
    credential_assistant: ProviderCredentialAssistantDefinition {
        enabled: true,
        access_token_flow: ProviderAccessTokenAssistantFlow::SessionGeneration,
        api_key_required_fields: &["apiUser"],
        api_key_required_any_fields: &["sessionCookie", "accessToken"],
    },
    credentials: &ADAPTER,
    connection: &ADAPTER,
    access_token: Some(&ADAPTER),
    api_keys: Some(&ADAPTER),
    usage: Some(&ADAPTER),
    account: Some(&ADAPTER),
    capability_probe: &ADAPTER,
    check_in: Some(&ADAPTER),
    is_anyrouter: provider_is_anyrouter,
};
