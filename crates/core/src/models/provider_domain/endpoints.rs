//! 协议对站点发出的每个请求的默认约定。
//!
//! 这份目录同时服务两个目的：编辑器把它原样列出来，让人知道 BalanceHub 会
//! 请求什么；传输层用同一份路径匹配卡片里的按接口覆盖。路径、方法和固定参数
//! 只在这里维护，避免界面说明和实际请求各写一份。

use serde::{Deserialize, Serialize};

use crate::limits;
use crate::models::{ProviderHeader, ProviderProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEndpointGroup {
    Account,
    CheckIn,
    Announcements,
    ApiKeys,
    Usage,
    Models,
    Links,
}

impl ProviderEndpointGroup {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::CheckIn => "checkIn",
            Self::Announcements => "announcements",
            Self::ApiKeys => "apiKeys",
            Self::Usage => "usage",
            Self::Models => "models",
            Self::Links => "links",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Account => "账号与登录",
            Self::CheckIn => "签到",
            Self::Announcements => "公告",
            Self::ApiKeys => "API Key 管理",
            Self::Usage => "用量与额度",
            Self::Models => "模型",
            Self::Links => "外部链接",
        }
    }
}

/// 该接口会附加哪一类认证。`None` 是公开接口，不随卡片认证方式变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEndpointAuth {
    None,
    Account,
    ApiKey,
}

impl ProviderEndpointAuth {
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Account => "account",
            Self::ApiKey => "apiKey",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "不附加认证",
            Self::Account => "按当前认证方式附加 Cookie 或访问令牌",
            Self::ApiKey => "仅附加 API Key",
        }
    }
}

/// 按接口改写的请求约定。只描述「这个接口和默认写法有什么不同」，未填写的
/// 部分继续使用目录里的默认值。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderEndpointOverride {
    /// 目录里的接口 id。
    pub operation: String,
    /// 自定义路径；空表示沿用默认路径。可以继续使用 `{id}` 这类占位符。
    pub path: String,
    /// 查询参数的增量修改；值留空表示删除同名默认参数。
    pub query: Vec<ProviderEndpointQuery>,
    /// 只附加到该接口的请求头；同名时覆盖卡片级请求头。
    pub headers: Vec<ProviderHeader>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderEndpointQuery {
    pub name: String,
    pub value: String,
}

impl ProviderEndpointOverride {
    pub fn is_empty(&self) -> bool {
        self.path.trim().is_empty() && self.query.is_empty() && self.headers.is_empty()
    }
}

/// 一次请求在目录中的匹配结果：命中的接口，以及路径里占位符的实际值。
pub struct MatchedEndpoint {
    pub spec: &'static ProviderEndpointSpec,
    pub params: Vec<(&'static str, String)>,
}

/// 按方法和相对路径匹配接口。`match_query` 用来区分同一路径上的不同操作；
/// 命中固定参数更多的条目优先，主路径优先于备用路径。
pub fn match_request(
    protocol: ProviderProtocol,
    method: &str,
    path: &str,
    query: &[(String, String)],
) -> Option<MatchedEndpoint> {
    let mut best: Option<(usize, MatchedEndpoint)> = None;
    for spec in catalog(protocol) {
        let Some(spec_method) = spec.method else {
            continue;
        };
        if !spec_method.eq_ignore_ascii_case(method)
            || !spec.match_query.iter().all(|(name, value)| {
                query
                    .iter()
                    .any(|(key, actual)| key == name && actual == value)
            })
        {
            continue;
        }
        let primary = match_path(spec.path, path);
        let primary_matched = primary.is_some();
        let params = match primary {
            Some(params) => params,
            None => match spec.alt_paths.iter().find_map(|alt| match_path(alt, path)) {
                Some(params) => params,
                None => continue,
            },
        };
        let score = spec.match_query.len() * 4 + usize::from(primary_matched) * 2;
        if best.as_ref().is_none_or(|(current, _)| score > *current) {
            best = Some((score, MatchedEndpoint { spec, params }));
        }
    }
    best.map(|(_, matched)| matched)
}

/// 路径模板按 `/` 分段比较；`{name}` 匹配任意非空段并记录实际值。
fn match_path(template: &'static str, path: &str) -> Option<Vec<(&'static str, String)>> {
    let template = template.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    let template_segments = template.split('/');
    let path_segments = path.split('/').collect::<Vec<_>>();
    let mut params = Vec::new();
    for (index, segment) in template_segments.enumerate() {
        let actual = *path_segments.get(index)?;
        if let Some(name) = segment
            .strip_prefix('{')
            .and_then(|segment| segment.strip_suffix('}'))
        {
            if actual.is_empty() {
                return None;
            }
            params.push((name, actual.to_string()));
        } else if segment != actual {
            return None;
        }
    }
    (path_segments.len() == template.split('/').count()).then_some(params)
}

/// 把占位符替换成实际值；没有对应值的占位符保持原样，由校验阶段挡住。
pub fn render_path(template: &str, params: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (name, value) in params {
        rendered = rendered.replace(&format!("{{{name}}}"), value);
    }
    rendered
}

/// 把按接口声明的查询参数合并到 URL。值留空表示删除同名参数。
pub fn apply_query_override(
    url: &mut reqwest::Url,
    fields: &[ProviderEndpointQuery],
    params: &[(&str, String)],
) {
    if fields.is_empty() {
        return;
    }
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    for field in fields {
        let name = field.name.trim();
        if name.is_empty() {
            continue;
        }
        if field.value.is_empty() {
            pairs.retain(|(key, _)| key != name);
            continue;
        }
        let value = render_path(field.value.trim(), params);
        match pairs.iter_mut().find(|(key, _)| key == name) {
            Some((_, current)) => *current = value,
            None => pairs.push((name.to_string(), value)),
        }
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
}

/// 卡片按接口改写的路径模板；未改写时为空。
pub fn link_template<'a>(overrides: &'a [ProviderEndpointOverride], operation: &str) -> &'a str {
    overrides
        .iter()
        .find(|over| over.operation == operation)
        .map(|over| over.path.trim())
        .unwrap_or_default()
}

/// 目录里的默认值加上卡片覆盖，最终生成可复制的邀请链接。
pub fn provider_link(
    protocol: ProviderProtocol,
    overrides: &[ProviderEndpointOverride],
    operation: &str,
    base_url: &str,
    code: &str,
) -> Option<String> {
    let spec = spec(protocol, operation)?;
    let template = match link_template(overrides, operation) {
        "" => spec.path,
        custom => custom,
    };
    let template = template.replace("{code}", &encode_component(code));
    Some(format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if template.starts_with('/') {
            template
        } else {
            format!("/{template}")
        }
    ))
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn normalize_endpoint_overrides(
    protocol: ProviderProtocol,
    overrides: Vec<ProviderEndpointOverride>,
    normalize_headers: impl Fn(Vec<ProviderHeader>) -> Vec<ProviderHeader>,
) -> Vec<ProviderEndpointOverride> {
    let mut normalized: Vec<ProviderEndpointOverride> = Vec::new();
    for over in overrides {
        let operation = clip_text(over.operation.trim(), 64);
        if operation.is_empty() || spec(protocol, &operation).is_none() {
            continue;
        }
        let normalized_override = ProviderEndpointOverride {
            operation,
            path: clip_text(over.path.trim(), limits::MAX_ENDPOINT_PATH_CHARS),
            query: normalize_query(over.query),
            headers: normalize_headers(over.headers),
        };
        if normalized_override.is_empty() {
            continue;
        }
        match normalized
            .iter_mut()
            .find(|existing| existing.operation == normalized_override.operation)
        {
            Some(existing) => *existing = normalized_override,
            None => normalized.push(normalized_override),
        }
        if normalized.len() >= limits::MAX_ENDPOINT_OVERRIDES {
            break;
        }
    }
    normalized
}

fn normalize_query(fields: Vec<ProviderEndpointQuery>) -> Vec<ProviderEndpointQuery> {
    let mut normalized: Vec<ProviderEndpointQuery> = Vec::new();
    for field in fields {
        let name = clip_text(field.name.trim(), limits::MAX_HEADER_NAME_CHARS);
        if name.is_empty() {
            continue;
        }
        let value = clip_text(field.value.trim(), limits::MAX_HEADER_VALUE_CHARS);
        match normalized.iter_mut().find(|field| field.name == name) {
            // 同名参数以最后一次填写为准，位置沿用第一次出现的位置。
            Some(existing) => existing.value = value,
            None => normalized.push(ProviderEndpointQuery { name, value }),
        }
        if normalized.len() >= limits::MAX_PROVIDER_HEADERS {
            break;
        }
    }
    normalized
}

fn clip_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

pub fn validate_endpoint_overrides(
    protocol: ProviderProtocol,
    overrides: &[ProviderEndpointOverride],
) -> Result<(), String> {
    if overrides.len() > limits::MAX_ENDPOINT_OVERRIDES {
        return Err(format!(
            "接口覆盖数量超过上限（{} 个）",
            limits::MAX_ENDPOINT_OVERRIDES
        ));
    }
    for over in overrides {
        let spec = spec(protocol, &over.operation)
            .ok_or_else(|| format!("接口覆盖引用了当前协议不存在的接口：{}", over.operation))?;
        validate_override_path(spec, &over.path)?;
        for field in &over.query {
            let name = field.name.trim();
            if name.is_empty() {
                continue;
            }
            if name.chars().count() > limits::MAX_HEADER_NAME_CHARS {
                return Err(format!("接口参数名称过长：{name}"));
            }
            if field.value.chars().count() > limits::MAX_HEADER_VALUE_CHARS {
                return Err(format!("接口参数「{name}」的值过长"));
            }
            if name.chars().any(|character| character.is_control())
                || field.value.chars().any(|character| character.is_control())
            {
                return Err(format!("接口参数「{name}」包含无法发送的控制字符"));
            }
        }
        crate::models::validate_headers(&over.headers, "接口请求头")?;
    }
    Ok(())
}

fn validate_override_path(spec: &ProviderEndpointSpec, raw: &str) -> Result<(), String> {
    let path = raw.trim();
    if path.is_empty() {
        return Ok(());
    }
    if path.chars().count() > limits::MAX_ENDPOINT_PATH_CHARS {
        return Err(format!("接口「{}」的路径过长", spec.label));
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(format!("接口「{}」的路径必须以单个 / 开头", spec.label));
    }
    if path.contains("://") {
        return Err(format!("接口「{}」的路径只填写地址后半段", spec.label));
    }
    if path.chars().any(|character| character.is_control()) {
        return Err(format!("接口「{}」的路径包含控制字符", spec.label));
    }
    if spec.method.is_none() {
        if !path.contains("{code}") {
            return Err(format!(
                "接口「{}」的链接模板需要保留 {{code}} 占位符",
                spec.label
            ));
        }
    } else if path.contains('?') || path.contains('#') {
        return Err(format!(
            "接口「{}」的路径不要包含 ? 或 #，查询参数请填写在参数表里",
            spec.label
        ));
    }
    for placeholder in path_placeholders(path) {
        if !spec.placeholders.contains(&placeholder.as_str()) {
            return Err(format!(
                "接口「{}」不支持占位符 {{{placeholder}}}",
                spec.label
            ));
        }
    }
    Ok(())
}

fn path_placeholders(path: &str) -> Vec<String> {
    let mut placeholders = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start + 1..].find('}') else {
            break;
        };
        placeholders.push(rest[start + 1..start + 1 + end].to_string());
        rest = &rest[start + 1 + end + 1..];
    }
    placeholders
}

pub struct ProviderEndpointSpec {
    pub id: &'static str,
    pub group: ProviderEndpointGroup,
    pub label: &'static str,
    pub description: &'static str,
    /// `None` 表示不是 HTTP 请求，而是一个可直接复制的外部链接模板。
    pub method: Option<&'static str>,
    pub path: &'static str,
    /// 拼在路径后的固定查询参数，按原样发送。
    pub default_query: &'static str,
    /// 同一接口在另一种地址写法下的路径（例如网关模型列表）。
    pub alt_paths: &'static [&'static str],
    /// 路径里会被实际值替换的占位符。
    pub placeholders: &'static [&'static str],
    /// 只有带这些查询参数时才匹配该接口，用于区分同一路径上的不同操作。
    pub match_query: &'static [(&'static str, &'static str)],
    /// 适配器固定写入的请求头；认证头和卡片自定义请求头由界面另行说明。
    pub headers: &'static [(&'static str, &'static str)],
    pub auth: ProviderEndpointAuth,
    /// 请求体是 JSON 对象，可以追加或替换字段。
    pub json_body: bool,
}

const UA: (&str, &str) = ("User-Agent", "内置浏览器标识");
const JSON_BODY: (&str, &str) = ("Content-Type", "application/json");
const JSON_ACCEPT: (&str, &str) = ("Accept", "application/json, text/plain, */*");
const SUB2_ACCEPT: (&str, &str) = ("Accept", "application/json");
const ORIGIN: (&str, &str) = ("Origin", "<站点地址>");
const REFERER: (&str, &str) = ("Referer", "<站点地址>/");
const API_USER: (&str, &str) = ("new-api-user", "<用户 ID>");
const XHR: (&str, &str) = ("X-Requested-With", "XMLHttpRequest");

const PUBLIC_HEADERS: &[(&str, &str)] = &[UA, JSON_ACCEPT, ORIGIN, REFERER];
const LOGIN_HEADERS: &[(&str, &str)] = &[UA, JSON_BODY, JSON_ACCEPT, ORIGIN, REFERER];
const ACCOUNT_HEADERS: &[(&str, &str)] = &[UA, JSON_BODY, JSON_ACCEPT, ORIGIN, REFERER, API_USER];
const CHECKIN_HEADERS: &[(&str, &str)] = &[UA, JSON_BODY, JSON_ACCEPT, ORIGIN, REFERER, XHR];
const API_KEY_HEADERS: &[(&str, &str)] = &[UA, JSON_ACCEPT];
const SUB2_HEADERS: &[(&str, &str)] = &[SUB2_ACCEPT, JSON_BODY];

const NEW_API_ENDPOINTS: &[ProviderEndpointSpec] = &[
    ProviderEndpointSpec {
        id: "login",
        group: ProviderEndpointGroup::Account,
        label: "账号密码登录",
        description: "首次登录、改密后的重新登录，以及需要重新登录的签到流程。",
        method: Some("POST"),
        path: "/api/user/login",
        default_query: "turnstile=",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: LOGIN_HEADERS,
        auth: ProviderEndpointAuth::None,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "refresh_session",
        group: ProviderEndpointGroup::Account,
        label: "续期登录会话",
        description: "用续期 Cookie 换取新的登录会话，不解锁新的凭据范围。",
        method: Some("POST"),
        path: "/api/user/auth/refresh",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: &[
            UA,
            JSON_BODY,
            JSON_ACCEPT,
            ORIGIN,
            REFERER,
            ("Cookie", "new_api_refresh=<续期 Cookie>"),
        ],
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "profile",
        group: ProviderEndpointGroup::Account,
        label: "账号信息",
        description: "读取用户 ID、用户名和账号余额，也用于登录后核对账号。",
        method: Some("GET"),
        path: "/api/user/self",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "change_password",
        group: ProviderEndpointGroup::Account,
        label: "修改密码",
        description: "在应用内修改站点账号密码。",
        method: Some("PUT"),
        path: "/api/user/self",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "access_token",
        group: ProviderEndpointGroup::Account,
        label: "生成访问令牌",
        description: "账号接口令牌；生成后旧令牌会失效。",
        method: Some("GET"),
        path: "/api/user/token",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "invite_code",
        group: ProviderEndpointGroup::Account,
        label: "邀请码",
        description: "读取当前账号的邀请码，再拼成邀请链接。",
        method: Some("GET"),
        path: "/api/user/aff",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "groups",
        group: ProviderEndpointGroup::Account,
        label: "可用分组",
        description: "创建或编辑 API Key 时读取账号可用的分组。",
        method: Some("GET"),
        path: "/api/user/self/groups",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "check_in_status",
        group: ProviderEndpointGroup::CheckIn,
        label: "签到状态与记录",
        description: "读取本月签到状态和历史记录。",
        method: Some("GET"),
        path: "/api/user/checkin",
        default_query: "month={month}",
        alt_paths: &[],
        placeholders: &["month"],
        match_query: &[],
        headers: CHECKIN_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "check_in",
        group: ProviderEndpointGroup::CheckIn,
        label: "提交签到",
        description: "标准签到接口；需要 Turnstile 时会附加 turnstile 参数。",
        method: Some("POST"),
        path: "/api/user/checkin",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: CHECKIN_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "session_sign_in",
        group: ProviderEndpointGroup::CheckIn,
        label: "会话签到",
        description: "只使用 session Cookie 签到的站点，例如 AnyRouter。",
        method: Some("POST"),
        path: "/api/user/sign_in",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: CHECKIN_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "notice",
        group: ProviderEndpointGroup::Announcements,
        label: "站点公告",
        description: "公开公告接口，不需要登录。",
        method: Some("GET"),
        path: "/api/notice",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: PUBLIC_HEADERS,
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "site_status",
        group: ProviderEndpointGroup::Announcements,
        label: "站点状态",
        description: "公开站点信息、控制台公告开关和 Turnstile 配置都来自这里。",
        method: Some("GET"),
        path: "/api/status",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: PUBLIC_HEADERS,
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_list",
        group: ProviderEndpointGroup::ApiKeys,
        label: "Key 列表",
        description: "读取账号下最多 100 把 API Key 的元数据。",
        method: Some("GET"),
        path: "/api/token/",
        default_query: "p=0&size=100",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_create",
        group: ProviderEndpointGroup::ApiKeys,
        label: "创建 Key",
        description: "按表单创建站点 API Key，请求体是 Key 设置。",
        method: Some("POST"),
        path: "/api/token/",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_read",
        group: ProviderEndpointGroup::ApiKeys,
        label: "读取 Key 设置",
        description: "编辑前读取单把 Key 的最新配置。",
        method: Some("GET"),
        path: "/api/token/{id}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_update",
        group: ProviderEndpointGroup::ApiKeys,
        label: "更新 Key 设置",
        description: "提交 Key 的额度、分组、模型和 IP 限制。",
        method: Some("PUT"),
        path: "/api/token/",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_status",
        group: ProviderEndpointGroup::ApiKeys,
        label: "启用或停用 Key",
        description: "只提交 id 和 status 的轻量更新。",
        method: Some("PUT"),
        path: "/api/token/",
        default_query: "status_only=true",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[("status_only", "true")],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_delete",
        group: ProviderEndpointGroup::ApiKeys,
        label: "删除 Key",
        description: "删除站点上的 API Key。",
        method: Some("DELETE"),
        path: "/api/token/{id}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_reveal",
        group: ProviderEndpointGroup::ApiKeys,
        label: "读取完整 Key",
        description: "按 id 读取单把 Key 的完整值。",
        method: Some("POST"),
        path: "/api/token/{id}/key",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_batch_reveal",
        group: ProviderEndpointGroup::ApiKeys,
        label: "批量读取完整 Key",
        description: "一次读取多把 Key 的完整值。",
        method: Some("POST"),
        path: "/api/token/batch/keys",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_auto_groups",
        group: ProviderEndpointGroup::ApiKeys,
        label: "自动分组范围",
        description: "读取 auto 分组可跨组重试的范围。",
        method: Some("GET"),
        path: "/api/token/auto-groups",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "request_logs",
        group: ProviderEndpointGroup::Usage,
        label: "请求日志",
        description: "按时间范围分页读取请求日志。",
        method: Some("GET"),
        path: "/api/log/self",
        default_query: "start_timestamp={start}&end_timestamp={end}&p={page}&page_size={size}",
        alt_paths: &[],
        placeholders: &["start", "end", "page", "size"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "request_log_stats",
        group: ProviderEndpointGroup::Usage,
        label: "日志消耗统计",
        description: "按时间范围读取日志汇总，用于用量卡片。",
        method: Some("GET"),
        path: "/api/log/self/stat",
        default_query: "start_timestamp={start}&end_timestamp={end}",
        alt_paths: &[],
        placeholders: &["start", "end"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "usage_trend",
        group: ProviderEndpointGroup::Usage,
        label: "用量趋势",
        description: "读取账号的按日用量。",
        method: Some("GET"),
        path: "/api/data/self",
        default_query: "start_timestamp={start}&end_timestamp={end}",
        alt_paths: &[],
        placeholders: &["start", "end"],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "model_health",
        group: ProviderEndpointGroup::Usage,
        label: "响应质量",
        description: "读取站点近 24 小时的模型成功率与吞吐。",
        method: Some("GET"),
        path: "/api/perf-metrics/summary",
        default_query: "hours=24",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_quota",
        group: ProviderEndpointGroup::Usage,
        label: "Key 额度",
        description: "纯 API Key 认证下的密钥维度额度。",
        method: Some("GET"),
        path: "/api/usage/token/",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: API_KEY_HEADERS,
        auth: ProviderEndpointAuth::ApiKey,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "subscription_windows",
        group: ProviderEndpointGroup::Usage,
        label: "订阅窗口",
        description: "读取站点的套餐订阅进度；旧站点没有这个接口。",
        method: Some("GET"),
        path: "/api/subscription/self",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "account_models",
        group: ProviderEndpointGroup::Models,
        label: "账号模型列表",
        description: "账号认证下读取全部可用分组汇总的模型。",
        method: Some("GET"),
        path: "/api/user/models",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: ACCOUNT_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "gateway_models",
        group: ProviderEndpointGroup::Models,
        label: "网关模型列表",
        description: "按当前调用 Key 的分组读取模型；地址以 /v1 结尾时使用 /models。",
        method: Some("GET"),
        path: "/v1/models",
        default_query: "",
        alt_paths: &["/models"],
        placeholders: &[],
        match_query: &[],
        headers: API_KEY_HEADERS,
        auth: ProviderEndpointAuth::ApiKey,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "register_link",
        group: ProviderEndpointGroup::Links,
        label: "邀请链接",
        description: "把邀请码拼进注册页。不同站点的注册路径和参数名可能不同。",
        method: None,
        path: "/register?aff={code}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["code"],
        match_query: &[],
        headers: &[],
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
];

const SUB2_API_ENDPOINTS: &[ProviderEndpointSpec] = &[
    ProviderEndpointSpec {
        id: "site_settings",
        group: ProviderEndpointGroup::Announcements,
        label: "站点信息",
        description: "公开站点设置，用于识别 Sub2API 和页面标题。",
        method: Some("GET"),
        path: "/api/v1/settings/public",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: &[SUB2_ACCEPT],
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "profile",
        group: ProviderEndpointGroup::Account,
        label: "用户资料",
        description: "读取当前登录用户和余额。",
        method: Some("GET"),
        path: "/api/v1/user/profile",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "login",
        group: ProviderEndpointGroup::Account,
        label: "账号密码登录",
        description: "用邮箱和密码换取访问令牌与刷新令牌。",
        method: Some("POST"),
        path: "/api/v1/auth/login",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::None,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "refresh_session",
        group: ProviderEndpointGroup::Account,
        label: "刷新令牌",
        description: "用刷新令牌滚动获取新的访问令牌。",
        method: Some("POST"),
        path: "/api/v1/auth/refresh",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::None,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "change_password",
        group: ProviderEndpointGroup::Account,
        label: "修改密码",
        description: "在应用内修改站点账号密码。",
        method: Some("PUT"),
        path: "/api/v1/user/password",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "invite_code",
        group: ProviderEndpointGroup::Account,
        label: "邀请码",
        description: "读取当前账号的邀请码，再拼成邀请链接。",
        method: Some("GET"),
        path: "/api/v1/user/aff",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "announcements",
        group: ProviderEndpointGroup::Announcements,
        label: "公告列表",
        description: "读取站点公告。",
        method: Some("GET"),
        path: "/api/v1/announcements",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "announcement_read",
        group: ProviderEndpointGroup::Announcements,
        label: "标记公告已读",
        description: "把一条公告标记为已读。",
        method: Some("POST"),
        path: "/api/v1/announcements/{id}/read",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_list",
        group: ProviderEndpointGroup::ApiKeys,
        label: "Key 列表",
        description: "读取账号下的 API Key 列表。",
        method: Some("GET"),
        path: "/api/v1/keys",
        default_query: "page=1&page_size=100&sort_by=created_at&sort_order=desc",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_read",
        group: ProviderEndpointGroup::ApiKeys,
        label: "读取 Key 设置",
        description: "编辑前读取单把 Key 的最新配置。",
        method: Some("GET"),
        path: "/api/v1/keys/{id}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_create",
        group: ProviderEndpointGroup::ApiKeys,
        label: "创建 Key",
        description: "按表单创建站点 API Key。",
        method: Some("POST"),
        path: "/api/v1/keys",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_update",
        group: ProviderEndpointGroup::ApiKeys,
        label: "更新 Key 设置",
        description: "提交 Key 的额度、分组、IP 和周期上限。",
        method: Some("PUT"),
        path: "/api/v1/keys/{id}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: true,
    },
    ProviderEndpointSpec {
        id: "key_delete",
        group: ProviderEndpointGroup::ApiKeys,
        label: "删除 Key",
        description: "删除站点上的 API Key。",
        method: Some("DELETE"),
        path: "/api/v1/keys/{id}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["id"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "usage_trend",
        group: ProviderEndpointGroup::Usage,
        label: "用量趋势",
        description: "按日期范围读取用量趋势。",
        method: Some("GET"),
        path: "/api/v1/usage/dashboard/trend",
        default_query: "start_date={start}&end_date={end}",
        alt_paths: &[],
        placeholders: &["start", "end"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "usage_models",
        group: ProviderEndpointGroup::Usage,
        label: "模型用量",
        description: "按日期范围读取模型维度用量。",
        method: Some("GET"),
        path: "/api/v1/usage/dashboard/models",
        default_query: "start_date={start}&end_date={end}",
        alt_paths: &[],
        placeholders: &["start", "end"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "request_logs",
        group: ProviderEndpointGroup::Usage,
        label: "请求日志",
        description: "分页读取请求日志。",
        method: Some("GET"),
        path: "/api/v1/usage",
        default_query: "page={page}&page_size={size}&start_date={start}&end_date={end}",
        alt_paths: &[],
        placeholders: &["page", "size", "start", "end"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "request_log_stats",
        group: ProviderEndpointGroup::Usage,
        label: "日志消耗统计",
        description: "按日期范围读取日志汇总。",
        method: Some("GET"),
        path: "/api/v1/usage/stats",
        default_query: "start_date={start}&end_date={end}",
        alt_paths: &[],
        placeholders: &["start", "end"],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "dashboard_stats",
        group: ProviderEndpointGroup::Usage,
        label: "实时统计",
        description: "读取 RPM、TPM 和平均耗时。",
        method: Some("GET"),
        path: "/api/v1/usage/dashboard/stats",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "key_quota",
        group: ProviderEndpointGroup::Usage,
        label: "Key 额度",
        description: "订阅配额与周期限额来自这把 Key 的用量接口。",
        method: Some("GET"),
        path: "/v1/usage",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: &[UA, SUB2_ACCEPT],
        auth: ProviderEndpointAuth::ApiKey,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "subscription_windows",
        group: ProviderEndpointGroup::Usage,
        label: "订阅窗口",
        description: "读取订阅套餐的 5 小时、每日和每周进度。",
        method: Some("GET"),
        path: "/api/v1/subscriptions/progress",
        default_query: "",
        alt_paths: &[],
        placeholders: &[],
        match_query: &[],
        headers: SUB2_HEADERS,
        auth: ProviderEndpointAuth::Account,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "gateway_models",
        group: ProviderEndpointGroup::Models,
        label: "网关模型列表",
        description: "按当前调用 Key 读取模型；地址以 /v1 结尾时使用 /models。",
        method: Some("GET"),
        path: "/v1/models",
        default_query: "",
        alt_paths: &["/models"],
        placeholders: &[],
        match_query: &[],
        headers: &[UA, SUB2_ACCEPT],
        auth: ProviderEndpointAuth::ApiKey,
        json_body: false,
    },
    ProviderEndpointSpec {
        id: "register_link",
        group: ProviderEndpointGroup::Links,
        label: "邀请链接",
        description: "把邀请码拼进注册页。不同站点的注册路径和参数名可能不同。",
        method: None,
        path: "/register?aff_code={code}",
        default_query: "",
        alt_paths: &[],
        placeholders: &["code"],
        match_query: &[],
        headers: &[],
        auth: ProviderEndpointAuth::None,
        json_body: false,
    },
];

pub fn catalog(protocol: ProviderProtocol) -> &'static [ProviderEndpointSpec] {
    match protocol {
        ProviderProtocol::NewApi => NEW_API_ENDPOINTS,
        ProviderProtocol::Sub2Api => SUB2_API_ENDPOINTS,
        _ => &[],
    }
}

pub fn spec(protocol: ProviderProtocol, id: &str) -> Option<&'static ProviderEndpointSpec> {
    catalog(protocol).iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn endpoint_ids_are_unique_and_paths_are_absolute() {
        for protocol in [ProviderProtocol::NewApi, ProviderProtocol::Sub2Api] {
            let mut ids = HashSet::new();
            for spec in catalog(protocol) {
                assert!(
                    ids.insert(spec.id),
                    "{} 的接口 id 重复：{}",
                    protocol.key(),
                    spec.id
                );
                assert!(
                    spec.path.starts_with('/'),
                    "{} 的 {} 路径必须以 / 开头",
                    protocol.key(),
                    spec.id
                );
                for placeholder in spec.placeholders {
                    assert!(
                        spec.path.contains(&format!("{{{placeholder}}}"))
                            || spec.default_query.contains(&format!("{{{placeholder}}}")),
                        "{} 的 {} 声明了未使用的占位符 {placeholder}",
                        protocol.key(),
                        spec.id
                    );
                }
                if spec.method.is_none() {
                    assert_eq!(spec.group, ProviderEndpointGroup::Links);
                }
            }
        }
    }

    #[test]
    fn request_matching_distinguishes_methods_paths_and_fixed_queries() {
        let matched = |method: &str, path: &str, query: &[(&str, &str)]| {
            let query = query
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect::<Vec<_>>();
            match_request(ProviderProtocol::NewApi, method, path, &query)
                .map(|matched| matched.spec.id)
        };

        assert_eq!(
            matched("POST", "/api/user/login", &[("turnstile", "")]),
            Some("login")
        );
        assert_eq!(matched("GET", "/api/token/", &[]), Some("key_list"));
        assert_eq!(matched("POST", "/api/token/", &[]), Some("key_create"));
        assert_eq!(matched("PUT", "/api/token/", &[]), Some("key_update"));
        assert_eq!(
            matched("PUT", "/api/token/", &[("status_only", "true")]),
            Some("key_status")
        );
        assert_eq!(
            matched("POST", "/api/token/12/key", &[]),
            Some("key_reveal")
        );
        assert_eq!(matched("GET", "/api/status", &[]), Some("site_status"));
        assert_eq!(
            matched("GET", "/api/user/models", &[]),
            Some("account_models")
        );
        assert_eq!(matched("GET", "/v1/models", &[]), Some("gateway_models"));
        assert_eq!(matched("GET", "/models", &[]), Some("gateway_models"));
        assert_eq!(matched("GET", "/unrelated", &[]), None);

        let matched_sub2 = |method: &str, path: &str| {
            match_request(ProviderProtocol::Sub2Api, method, path, &[])
                .map(|matched| matched.spec.id)
        };
        // Sub2API 的目录路径包含 `/api/v1`，与相对站点根地址的实际请求一致。
        assert_eq!(matched_sub2("POST", "/api/v1/auth/login"), Some("login"));
        assert_eq!(matched_sub2("PUT", "/api/v1/keys/7"), Some("key_update"));
        assert_eq!(matched_sub2("GET", "/api/v1/keys/7"), Some("key_read"));
        assert_eq!(matched_sub2("GET", "/api/v1/usage"), Some("request_logs"));
        assert_eq!(matched_sub2("GET", "/v1/usage"), Some("key_quota"));
    }

    #[test]
    fn matched_placeholders_feed_path_and_query_overrides() {
        let matched = match_request(ProviderProtocol::NewApi, "DELETE", "/api/token/42/", &[])
            .expect("delete key should match");
        assert_eq!(matched.params, vec![("id", "42".to_string())]);
        assert_eq!(
            render_path("/api/v2/token/{id}", &matched.params),
            "/api/v2/token/42"
        );

        let mut url =
            reqwest::Url::parse("https://relay.example.com/api/token/?p=0&size=100").expect("url");
        apply_query_override(
            &mut url,
            &[
                ProviderEndpointQuery {
                    name: "size".into(),
                    value: "50".into(),
                },
                ProviderEndpointQuery {
                    name: "p".into(),
                    value: String::new(),
                },
                ProviderEndpointQuery {
                    name: "filter".into(),
                    value: "token-{id}".into(),
                },
            ],
            &matched.params,
        );
        let pairs = url.query_pairs().into_owned().collect::<Vec<_>>();
        assert_eq!(
            pairs,
            vec![
                ("size".to_string(), "50".to_string()),
                ("filter".to_string(), "token-42".to_string()),
            ]
        );
    }

    #[test]
    fn invite_link_template_is_overridable_and_encodes_the_code() {
        let default = provider_link(
            ProviderProtocol::NewApi,
            &[],
            "register_link",
            "https://relay.example.com/",
            "ABC",
        )
        .expect("default link");
        assert_eq!(default, "https://relay.example.com/register?aff=ABC");

        let overrides = vec![ProviderEndpointOverride {
            operation: "register_link".into(),
            path: "/signup?invite={code}".into(),
            ..Default::default()
        }];
        let custom = provider_link(
            ProviderProtocol::Sub2Api,
            &overrides,
            "register_link",
            "https://relay.example.com",
            "A/B",
        )
        .expect("custom link");
        assert_eq!(custom, "https://relay.example.com/signup?invite=A%2FB");
    }

    #[test]
    fn overrides_are_normalized_against_the_protocol_catalog() {
        let normalized = normalize_endpoint_overrides(
            ProviderProtocol::NewApi,
            vec![
                ProviderEndpointOverride {
                    operation: " check_in ".into(),
                    path: "  /api/user/sign_in  ".into(),
                    query: vec![ProviderEndpointQuery {
                        name: " turnstile ".into(),
                        value: " 1 ".into(),
                    }],
                    headers: Vec::new(),
                },
                ProviderEndpointOverride {
                    operation: "unknown".into(),
                    path: "/x".into(),
                    ..Default::default()
                },
                ProviderEndpointOverride {
                    operation: "key_list".into(),
                    ..Default::default()
                },
            ],
            |headers| headers,
        );
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].operation, "check_in");
        assert_eq!(normalized[0].path, "/api/user/sign_in");
        assert_eq!(normalized[0].query[0].name, "turnstile");
    }

    #[test]
    fn invalid_override_paths_are_rejected() {
        let validate = |path: &str| {
            validate_endpoint_overrides(
                ProviderProtocol::NewApi,
                &[ProviderEndpointOverride {
                    operation: "check_in".into(),
                    path: path.into(),
                    ..Default::default()
                }],
            )
        };
        assert!(validate("/api/user/sign_in").is_ok());
        assert!(validate("api/user/sign_in").is_err());
        assert!(validate("/api/user/sign_in?x=1").is_err());
        assert!(validate("/api/token/{id}").is_err());
        assert!(validate_endpoint_overrides(
            ProviderProtocol::NewApi,
            &[ProviderEndpointOverride {
                operation: "register_link".into(),
                path: "/register".into(),
                ..Default::default()
            }],
        )
        .is_err());
    }

    #[test]
    fn critical_operations_stay_in_the_catalog() {
        let new_api = catalog(ProviderProtocol::NewApi);
        for id in [
            "login",
            "profile",
            "check_in",
            "session_sign_in",
            "notice",
            "key_list",
            "key_update",
            "register_link",
        ] {
            assert!(
                new_api.iter().any(|spec| spec.id == id),
                "NewAPI 缺少接口 {id}"
            );
        }
        let sub2_api = catalog(ProviderProtocol::Sub2Api);
        for id in [
            "login",
            "refresh_session",
            "key_update",
            "subscription_windows",
            "register_link",
        ] {
            assert!(
                sub2_api.iter().any(|spec| spec.id == id),
                "Sub2API 缺少接口 {id}"
            );
        }
        assert!(catalog(ProviderProtocol::Api).is_empty());
        assert!(spec(ProviderProtocol::NewApi, "login").is_some());
        assert!(spec(ProviderProtocol::Sub2Api, "login").is_some());
        assert!(spec(ProviderProtocol::NewApi, "unknown").is_none());
    }
}
