//! CPA management credentials and inference API keys have separate privileges.
//! Management requests never substitute the inference key or consume usage queues.
use super::{oauth::http, protocol::contracts::*, transport::ProviderTransport};
use crate::{models::*, network};
use async_trait::async_trait;
use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::{Client, Method, Response};
use serde_json::{json, Value};

pub(crate) struct CliProxyApiAdapter;

/// Signing in to a provider on the gateway itself, which keeps the account on
/// the gateway host instead of in this app.
pub(crate) mod login;

/// Reading a credential's quota through the gateway's quota provider.
pub(crate) mod quota;

/// Browsing the gateway's plugin store and installing one entry.
pub(crate) mod plugin_store;

#[cfg(test)]
mod oauth_tests;

pub(crate) fn base_url(provider: &Provider) -> Result<String, String> {
    crate::models::normalize_cpa_base_url(&provider.identity.base_url)
}

fn root(version: &str) -> &'static str {
    if version == "v8" {
        "/v8/management/credentials"
    } else {
        "/v0/management/auth-files"
    }
}

/// Route prefix shared by the management endpoints that are not part of the
/// credential group, such as cooldown resets and the error-log listing.
fn management_root(version: &str) -> &'static str {
    if version == "v8" {
        "/v8/management"
    } else {
        "/v0/management"
    }
}

/// Request-error logs live under the observability group in v8 and under the
/// v0 root; both serve one file as a text attachment.
fn error_log_root(version: &str) -> &'static str {
    if version == "v8" {
        "/v8/management/observability/logs/errors"
    } else {
        "/v0/management/request-error-logs"
    }
}
fn require_key(provider: &Provider) -> Result<(), String> {
    if provider.identity.protocol != ProviderProtocol::CliProxyApi {
        return Err("所选卡片不是 CPA 网关".into());
    }
    if provider.auth.access_token.trim().is_empty() {
        return Err("请填写 CPA 管理密钥".into());
    }
    Ok(())
}

/// Credential indices and plugin ids reach a URL path, so both are held to the
/// identifier shape the gateway itself accepts. `message` names the value that
/// failed, because the panel shows it next to the control that produced it.
pub(super) fn identifier(raw: &str, message: &str) -> Result<String, String> {
    let value = raw.trim();
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !valid {
        return Err(message.into());
    }
    Ok(value.to_owned())
}

async fn read_response(response: reqwest::Response, context: &str) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(http::http_error(context, status.as_u16()));
    }
    let text = network::read_http_text(response, context)
        .await
        .map_err(|_| format!("{context}响应读取失败"))?;
    serde_json::from_str(&text).map_err(|_| format!("{context}响应格式无效"))
}

/// v8 hands out an entity tag with every config read and refuses a write that
/// does not carry it, so the tag has to survive alongside the parsed value.
async fn read_response_with_etag(
    response: reqwest::Response,
    context: &str,
) -> Result<(Value, Option<String>), String> {
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    Ok((read_response(response, context).await?, etag))
}

/// Both management generations expose the same configuration document; only
/// the route prefix differs.
fn config_root(version: &str) -> &'static str {
    if version == "v8" {
        "/v8/management/config"
    } else {
        "/v0/management/config"
    }
}

/// Reads the gateway-wide scheduling settings. The configuration document
/// carries credentials as well, so only these five values leave this function.
/// Which management generation answers, and the configuration tag of the
/// revision it answered with. v8 refuses a config write that does not carry
/// this tag, so a concurrent edit fails instead of being overwritten.
struct ManagementSession {
    version: &'static str,
    etag: Option<String>,
}

async fn management_session(
    client: &Client,
    provider: &Provider,
) -> Result<ManagementSession, String> {
    require_key(provider)?;
    let base = base_url(provider)?;
    let response = client
        .get(format!("{base}{}", config_root("v8")))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if matches!(response.status().as_u16(), 404 | 405) {
        return Ok(ManagementSession {
            version: "v0",
            etag: None,
        });
    }
    let (_, etag) = read_response_with_etag(response, "读取 CPA 网关设置").await?;
    Ok(ManagementSession {
        version: "v8",
        etag,
    })
}

pub(crate) async fn gateway_settings(
    client: &Client,
    provider: &Provider,
) -> Result<(CpaGatewaySettings, String), String> {
    require_key(provider)?;
    let base = base_url(provider)?;
    let session = management_session(client, provider).await?;
    let (value, _) = read_response_with_etag(
        client
            .get(format!("{base}{}", config_root(session.version)))
            .bearer_auth(&provider.auth.access_token)
            .send()
            .await
            .map_err(|_| "连接 CPA 管理接口失败")?,
        "读取 CPA 网关设置",
    )
    .await?;
    Ok((
        parse_gateway_settings(&value, session.version)?,
        session.version.into(),
    ))
}

/// v8 stores the scheduling values in its nested layout, while v0 serializes
/// the same settings with the retry counts and proxy at the document root.
/// Both put the strategy under `routing`, and both keep the quota-exceeded
/// switches under `quota-exceeded`.
fn parse_gateway_settings(value: &Value, version: &str) -> Result<CpaGatewaySettings, String> {
    if !value.is_object() {
        return Err("CPA 网关配置格式无效，请检查管理接口地址".into());
    }
    let retry = if version == "v8" {
        value.pointer("/routing/retry")
    } else {
        Some(value)
    };
    let number = |key: &str| retry.and_then(|node| node.get(key)).and_then(Value::as_i64);
    let proxy = if version == "v8" {
        value.pointer("/requests/proxy-url")
    } else {
        value.get("proxy-url")
    };
    // The log and usage switches live under their own sections in v8 and at the
    // root of the legacy document, so each value names both spellings.
    let field = |v8: &str, v0: &str| {
        let node = if version == "v8" {
            value.pointer(v8)
        } else {
            value.get(v0)
        };
        node.cloned()
    };
    let boolean = |v8: &str, v0: &str| field(v8, v0).and_then(|node| node.as_bool());
    let integer = |v8: &str, v0: &str| field(v8, v0).and_then(|node| node.as_i64());
    Ok(CpaGatewaySettings {
        routing_strategy: value
            .pointer("/routing/strategy")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        proxy_url: proxy.and_then(Value::as_str).unwrap_or_default().to_owned(),
        request_retry: number("request-retry"),
        max_retry_credentials: number("max-retry-credentials"),
        max_retry_interval: number("max-retry-interval"),
        switch_project: value
            .pointer("/quota-exceeded/switch-project")
            .and_then(Value::as_bool),
        switch_preview_model: value
            .pointer("/quota-exceeded/switch-preview-model")
            .and_then(Value::as_bool),
        request_log: boolean("/observability/logs/request-log", "request-log"),
        logs_max_total_size_mb: integer(
            "/observability/logs/logs-max-total-size-mb",
            "logs-max-total-size-mb",
        ),
        error_logs_max_files: integer(
            "/observability/logs/error-logs-max-files",
            "error-logs-max-files",
        ),
        usage_statistics_enabled: boolean(
            "/observability/usage/usage-statistics-enabled",
            "usage-statistics-enabled",
        ),
        debug: boolean("/observability/logs/debug", "debug"),
        force_model_prefix: boolean("/routing/force-model-prefix", "force-model-prefix"),
        // The WebSocket switch belongs to the AI Studio provider, which v8 nests
        // two levels deeper than the other values here.
        ws_auth: boolean("/oauth/providers/aistudio/ws-auth", "ws-auth"),
    })
}

/// One writable scheduling value: the v0 management path that owns it and the
/// nested path it takes in the v8 configuration tree.
struct GatewaySettingWrite<'a> {
    legacy: &'a str,
    v8_path: String,
    value: Value,
}

/// Applies only the fields the panel changed. v0 owns one endpoint per value;
/// v8 edits the config tree, whose every write is guarded by the tag of the
/// revision it was read from, so a concurrent edit fails instead of being
/// overwritten. The tag returned by each write is reused for the next field,
/// which keeps a multi-field save to one extra read.
pub(crate) async fn update_gateway_settings(
    client: &Client,
    provider: &Provider,
    update: &CpaGatewaySettingsUpdate,
) -> Result<(), String> {
    require_key(provider)?;
    let mut writes = Vec::new();
    if let Some(strategy) = &update.routing_strategy {
        if !matches!(
            strategy.as_str(),
            "round-robin" | "weighted-round-robin" | "fill-first"
        ) {
            return Err("CPA 调度策略无效".into());
        }
        writes.push(GatewaySettingWrite {
            legacy: "routing/strategy",
            v8_path: "routing/strategy".into(),
            value: json!(strategy),
        });
    }
    if let Some(value) = &update.proxy_url {
        writes.push(GatewaySettingWrite {
            legacy: "proxy-url",
            v8_path: "requests/proxy-url".into(),
            value: json!(value),
        });
    }
    for (key, value) in [
        ("request-retry", update.request_retry),
        ("max-retry-credentials", update.max_retry_credentials),
        ("max-retry-interval", update.max_retry_interval),
    ] {
        if let Some(value) = value {
            writes.push(GatewaySettingWrite {
                legacy: key,
                v8_path: format!("routing/retry/{key}"),
                value: json!(value),
            });
        }
    }
    // The gateway clamps a negative size or file count instead of rejecting it,
    // which would silently save a value the user did not type.
    for (key, value, v8_path) in [
        (
            "logs-max-total-size-mb",
            update.logs_max_total_size_mb,
            "observability/logs/logs-max-total-size-mb",
        ),
        (
            "error-logs-max-files",
            update.error_logs_max_files,
            "observability/logs/error-logs-max-files",
        ),
    ] {
        if let Some(value) = value {
            if value < 0 {
                return Err("CPA 日志容量与文件数不能为负数".into());
            }
            writes.push(GatewaySettingWrite {
                legacy: key,
                v8_path: v8_path.into(),
                value: json!(value),
            });
        }
    }
    for (value, legacy, v8_path) in [
        (
            update.switch_project,
            "quota-exceeded/switch-project",
            "quota-exceeded/switch-project",
        ),
        (
            update.switch_preview_model,
            "quota-exceeded/switch-preview-model",
            "quota-exceeded/switch-preview-model",
        ),
        (
            update.request_log,
            "request-log",
            "observability/logs/request-log",
        ),
        (
            update.usage_statistics_enabled,
            "usage-statistics-enabled",
            "observability/usage/usage-statistics-enabled",
        ),
        (update.debug, "debug", "observability/logs/debug"),
        (
            update.force_model_prefix,
            "force-model-prefix",
            "routing/force-model-prefix",
        ),
        (
            update.ws_auth,
            "ws-auth",
            "oauth/providers/aistudio/ws-auth",
        ),
    ] {
        if let Some(value) = value {
            writes.push(GatewaySettingWrite {
                legacy,
                v8_path: v8_path.into(),
                value: json!(value),
            });
        }
    }
    if writes.is_empty() {
        return Ok(());
    }
    let base = base_url(provider)?;
    let probe = client
        .get(format!("{base}{}", config_root("v8")))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if matches!(probe.status().as_u16(), 404 | 405) {
        for write in &writes {
            let value = http::json(
                client
                    .put(format!("{base}/v0/management/{}", write.legacy))
                    .bearer_auth(&provider.auth.access_token)
                    .json(&json!({ "value": write.value })),
                "更新 CPA 网关设置",
            )
            .await?;
            if value.get("status").and_then(Value::as_str) != Some("ok") {
                return Err("CPA 未确认网关设置变更，请刷新后核对".into());
            }
        }
        return Ok(());
    }
    let (_, mut etag) = read_response_with_etag(probe, "读取 CPA 网关设置").await?;
    for write in &writes {
        if etag.is_none() {
            let (_, current) = read_response_with_etag(
                client
                    .get(format!("{base}{}", config_root("v8")))
                    .bearer_auth(&provider.auth.access_token)
                    .send()
                    .await
                    .map_err(|_| "连接 CPA 管理接口失败")?,
                "读取 CPA 网关设置",
            )
            .await?;
            etag = current;
        }
        let mut request = client
            .put(format!("{base}{}/{}", config_root("v8"), write.v8_path))
            .bearer_auth(&provider.auth.access_token)
            .json(&write.value);
        if let Some(tag) = &etag {
            request = request.header(reqwest::header::IF_MATCH, tag);
        }
        let response = request.send().await.map_err(|_| "连接 CPA 管理接口失败")?;
        if response.status().as_u16() == 412 {
            return Err("网关配置已被其他会话修改，请刷新后重试".into());
        }
        etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let value = read_response(response, "更新 CPA 网关设置").await?;
        if value.get("status").and_then(Value::as_str) != Some("ok") {
            return Err("CPA 未确认网关设置变更，请刷新后核对".into());
        }
    }
    Ok(())
}

/// Reads the gateway's own configuration document verbatim. Both management
/// generations serve it as YAML: v8 normalizes the layout on the way out and
/// hands back the tag a save has to carry, while v0 returns the file as written.
pub(crate) async fn gateway_config(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<CpaGatewayConfig, String> {
    require_key(provider)?;
    let client = http::client(settings, provider).await?;
    let base = base_url(provider)?;
    let v8 = client
        .get(format!("{base}/v8/management/config.yaml"))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    let (response, api_version) = if v8.status().as_u16() == 404 {
        let legacy = client
            .get(format!("{base}/v0/management/config.yaml"))
            .bearer_auth(&provider.auth.access_token)
            .send()
            .await
            .map_err(|_| "连接 CPA 管理接口失败")?;
        (legacy, "v0")
    } else {
        (v8, "v8")
    };
    let status = response.status();
    if status.as_u16() == 404 {
        return Err("网关没有提供 config.yaml，请确认管理接口已启用且配置文件存在".into());
    }
    if !status.is_success() {
        return Err(http::http_error("读取 CPA 网关配置", status.as_u16()));
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_default();
    let content = network::read_http_text(response, "读取 CPA 网关配置")
        .await
        .map_err(|_| "读取 CPA 网关配置响应读取失败")?;
    Ok(CpaGatewayConfig {
        content,
        etag,
        api_version: api_version.to_owned(),
    })
}

/// Replaces the whole configuration document. The gateway validates the text
/// before it writes the file, so a refused save leaves the old configuration
/// intact; its own message says what it refused, which the panel cannot guess.
pub(crate) async fn update_gateway_config(
    settings: &AppSettings,
    provider: &Provider,
    content: &str,
    etag: &str,
    api_version: &str,
) -> Result<(), String> {
    require_key(provider)?;
    // An empty document is not a harmless no-op: v0 accepts it as a valid
    // configuration and would replace the file with nothing.
    if content.trim().is_empty() {
        return Err("网关配置不能为空".into());
    }
    let client = http::client(settings, provider).await?;
    let base = base_url(provider)?;
    let path = if api_version == "v8" {
        "/v8/management/config.yaml"
    } else {
        "/v0/management/config.yaml"
    };
    let mut request = client
        .put(format!("{base}{path}"))
        .bearer_auth(&provider.auth.access_token)
        .header(reqwest::header::CONTENT_TYPE, "application/yaml")
        .body(content.to_owned());
    if api_version == "v8" && !etag.is_empty() {
        request = request.header(reqwest::header::IF_MATCH, etag);
    }
    let response = request.send().await.map_err(|_| "连接 CPA 管理接口失败")?;
    let status = response.status();
    let text = network::read_http_text(response, "保存 CPA 网关配置")
        .await
        .map_err(|_| "保存 CPA 网关配置响应读取失败")?;
    if status.is_success() {
        return Ok(());
    }
    if status.as_u16() == 412 {
        return Err("网关配置已被其他会话修改，请重新读取后再保存".into());
    }
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    let kind = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let detail = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let reason = |prefix: &str| {
        if detail.is_empty() {
            prefix.to_owned()
        } else {
            format!("{prefix}：{detail}")
        }
    };
    Err(match kind {
        "invalid_yaml" => reason("配置不是有效的 YAML"),
        "invalid_body" => reason("配置内容无法解析为 YAML"),
        "config_must_be_object" => "配置根节点必须是一个对象".into(),
        "invalid_config" => reason("配置未通过网关校验"),
        "read_only_field" => reason("配置里的该字段由网关自己维护，不能修改"),
        "write_failed" | "reload_failed" => reason("网关写入配置失败"),
        "not_found" => "网关没有找到 config.yaml".into(),
        _ => http::http_error("保存 CPA 网关配置", status.as_u16()),
    })
}

/// Keys downstream clients present to the gateway. v8 stores them in its
/// configuration tree under `access.api-keys` and answers 404 while the list is
/// absent; v0 has a dedicated endpoint that returns it under `api-keys`.
pub(crate) async fn client_keys(
    client: &Client,
    provider: &Provider,
) -> Result<CpaClientKeys, String> {
    let session = management_session(client, provider).await?;
    read_client_keys(client, provider, &session).await
}

fn key_list_path(version: &str) -> String {
    if version == "v8" {
        format!("{}/access/api-keys", config_root("v8"))
    } else {
        "/v0/management/api-keys".into()
    }
}

async fn read_client_keys(
    client: &Client,
    provider: &Provider,
    session: &ManagementSession,
) -> Result<CpaClientKeys, String> {
    let response = client
        .get(format!(
            "{}{}",
            base_url(provider)?,
            key_list_path(session.version)
        ))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    // v8 answers 404 while `access.api-keys` has never been written; an absent
    // list is an empty gateway, not a failed read.
    if response.status().as_u16() == 404 {
        return Ok(CpaClientKeys::default());
    }
    let value = read_response(response, "读取 CPA 客户端密钥").await?;
    Ok(parse_client_keys(&value, session.version))
}

fn parse_client_keys(value: &Value, version: &str) -> CpaClientKeys {
    let list = if version == "v8" {
        value.as_array()
    } else {
        value.get("api-keys").and_then(Value::as_array)
    };
    CpaClientKeys {
        keys: list
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())
            .collect(),
    }
}

/// Applies one edit to the client-key list. CPA has no per-key write in either
/// generation, so every change replaces the list; the edit runs between a read
/// and the write that carries the same v8 revision tag, which keeps a
/// concurrent change on the gateway from being silently overwritten.
pub(crate) async fn update_client_keys(
    client: &Client,
    provider: &Provider,
    edit: impl FnOnce(&mut Vec<String>) -> Result<(), String>,
) -> Result<CpaClientKeys, String> {
    let session = management_session(client, provider).await?;
    let mut list = read_client_keys(client, provider, &session).await?;
    edit(&mut list.keys)?;
    let mut request = client
        .put(format!(
            "{}{}",
            base_url(provider)?,
            key_list_path(session.version)
        ))
        .bearer_auth(&provider.auth.access_token)
        .json(&Value::Array(
            list.keys.iter().map(|key| json!(key)).collect(),
        ));
    if let Some(tag) = &session.etag {
        request = request.header(reqwest::header::IF_MATCH, tag);
    }
    let response = request.send().await.map_err(|_| "连接 CPA 管理接口失败")?;
    if response.status().as_u16() == 412 {
        return Err("网关配置已被其他会话修改，请刷新后重试".into());
    }
    let value = read_response(response, "更新 CPA 客户端密钥").await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认客户端密钥变更，请刷新后核对".into());
    }
    Ok(list)
}

/// What one credential listing returned besides the credentials themselves:
/// which management generation answered, and the gateway build it came from.
pub(crate) struct CpaInventory {
    pub version: String,
    /// CPA stamps its own version on every management response, so the card can
    /// name the gateway build without a second request.
    pub gateway_version: String,
    pub accounts: Vec<CpaAccountSummary>,
}

pub(crate) async fn inventory(
    client: &Client,
    provider: &Provider,
) -> Result<CpaInventory, String> {
    require_key(provider)?;
    let base = base_url(provider)?;
    let response = client
        .get(format!("{base}{}", root("v8")))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    let (version, response) = if matches!(response.status().as_u16(), 404 | 405) {
        (
            "v0",
            client
                .get(format!("{base}{}", root("v0")))
                .bearer_auth(&provider.auth.access_token)
                .send()
                .await
                .map_err(|_| "连接 CPA 管理接口失败")?,
        )
    } else {
        ("v8", response)
    };
    let gateway_version = header(response.headers(), "x-cpa-version");
    let value = read_response(response, "读取 CPA 授权列表").await?;
    Ok(CpaInventory {
        version: version.into(),
        gateway_version,
        accounts: parse_inventory(&value)?,
    })
}

/// CPA's management responses carry its build in a header; an empty string
/// means the gateway did not report one, which the card simply omits.
fn header(headers: &reqwest::header::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn parse_inventory(value: &Value) -> Result<Vec<CpaAccountSummary>, String> {
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .ok_or("CPA 未返回授权列表，请检查管理接口地址")?;
    if value.get("has_more").and_then(Value::as_bool) == Some(true) {
        return Err("CPA 返回了不完整的授权列表，请检查网关分页设置".into());
    }
    Ok(files
        .iter()
        .map(|file| {
            let name = http::text(file, &["name", "id"]);
            let status = http::text(file, &["status"]);
            let provider = http::text(file, &["provider", "type"]);
            let email = http::text(file, &["email"]);
            let disabled = file
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || status == "disabled";
            let unavailable = file
                .get("unavailable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status_label = if disabled {
                "已停用"
            } else if unavailable {
                "暂不可用"
            } else {
                match status.as_str() {
                    "active" => "可用",
                    "pending" => "待验证",
                    "refreshing" => "续期中",
                    "error" => "连接异常",
                    _ => "状态未知",
                }
            };
            let can_download = validate_name(&name).is_ok()
                && name.to_ascii_lowercase().ends_with(".json")
                && file.get("runtime_only").and_then(Value::as_bool) != Some(true)
                && file.get("source").and_then(Value::as_str) != Some("memory");
            let last_refresh = cpa_time(file, "last_refresh");
            let quota = file.get("quota");
            CpaAccountSummary {
                auth_index: http::text(file, &["auth_index"]),
                models: cpa_strings(file.get("models")),
                success: cpa_number(file, "success").and_then(|v| u64::try_from(v).ok()),
                failed: cpa_number(file, "failed").and_then(|v| u64::try_from(v).ok()),
                priority: cpa_number(file, "priority"),
                weight: cpa_number(file, "weight"),
                note: http::text(file, &["note"]),
                excluded_models: cpa_strings(file.get("excluded_models")),
                request_retry: cpa_number(file, "request_retry"),
                last_refresh,
                next_retry_after: cpa_time(file, "next_retry_after"),
                quota_exceeded: quota
                    .and_then(|quota| quota.get("exceeded"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                quota_reason: quota
                    .and_then(|quota| quota.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                quota_next_recover_at: quota.and_then(|quota| cpa_time(quota, "next_recover_at")),
                backoff_level: quota.and_then(|quota| cpa_number(quota, "backoff_level")),
                supports_quota: file
                    .get("supports_quota")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                quota_plugin_id: http::text(file, &["quota_plugin_id"]),
                recent_requests: cpa_request_buckets(file.get("recent_requests")),
                name,
                provider,
                email,
                available: status == "active" && !disabled && !unavailable,
                status,
                status_label: status_label.into(),
                disabled,
                unavailable,
                can_download,
                download_unavailable_reason: (!can_download)
                    .then(|| "CPA 未提供此凭据的授权文件，请在 CPA 中管理".into()),
                // Remote messages may contain request material. Only a short status is kept.
            }
        })
        .collect())
}

/// Remote `models` and `excluded_models` reach CPA as either a list or a
/// comma-separated string, depending on the entry point that wrote them.
fn cpa_number(value: &Value, key: &str) -> Option<i64> {
    match value.get(key)? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// CPA writes its timestamps as RFC 3339 text; a numeric epoch is accepted for
/// gateway builds that serialize the raw value.
fn cpa_time(value: &Value, key: &str) -> Option<i64> {
    match value.get(key)? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => chrono::DateTime::parse_from_rfc3339(text.trim())
            .ok()
            .map(|at| at.timestamp()),
        _ => None,
    }
}

fn cpa_request_buckets(value: Option<&Value>) -> Vec<CpaRequestBucket> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|bucket| CpaRequestBucket {
                    time: bucket
                        .get("time")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    success: cpa_number(bucket, "success")
                        .and_then(|v| u64::try_from(v).ok())
                        .unwrap_or(0),
                    failed: cpa_number(bucket, "failed")
                        .and_then(|v| u64::try_from(v).ok())
                        .unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cpa_strings(value: Option<&Value>) -> Vec<String> {
    let items: Vec<&str> = match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        Some(Value::String(text)) => text.split(',').collect(),
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn models(
    client: &Client,
    provider: &Provider,
    version: &str,
    accounts: &[CpaAccountSummary],
) -> Result<ProviderModelList, String> {
    let base = base_url(provider)?;
    if !provider.auth.api_key.trim().is_empty() {
        let value = http::json(
            client
                .get(format!("{base}/v1/models"))
                .bearer_auth(&provider.auth.api_key),
            "读取 CPA 调用模型",
        )
        .await?;
        return Ok(ProviderModelList::new(
            super::oauth::usage::parse_models(&value)?,
            ProviderModelScope::ApiKey,
        ));
    }
    let names: Vec<String> = accounts
        .iter()
        .filter(|a| !a.disabled)
        .map(|a| a.name.clone())
        .collect();
    let batches: Vec<Vec<String>> = stream::iter(names.into_iter().map(|name| {
        let request = client
            .get(format!("{base}{}/models", root(version)))
            .bearer_auth(&provider.auth.access_token)
            .query(&[("name", name)]);
        async move {
            super::oauth::usage::parse_models(&http::json(request, "读取 CPA 授权模型").await?)
        }
    }))
    .buffer_unordered(6)
    .try_collect()
    .await?;
    let mut models: Vec<_> = batches.into_iter().flatten().collect();
    models.sort();
    models.dedup();
    Ok(ProviderModelList::new(models, ProviderModelScope::Account))
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 255
        || name.contains(['/', '\\', '\0', '\r', '\n'])
        || name == "."
        || name == ".."
    {
        return Err("CPA 授权文件名无效".into());
    }
    Ok(())
}

pub(crate) async fn download_selected(
    settings: &AppSettings,
    provider: &Provider,
    mut names: Vec<String>,
) -> Result<Vec<OAuthImportFile>, String> {
    names.sort();
    names.dedup();
    for name in &names {
        validate_name(name)?;
    }
    let client = http::client(settings, provider).await?;
    let CpaInventory {
        version, accounts, ..
    } = inventory(&client, provider).await?;
    if names
        .iter()
        .any(|name| !accounts.iter().any(|a| a.name == *name))
    {
        return Err("CPA 授权文件已变化，请刷新后重试".into());
    }
    if names
        .iter()
        .any(|name| accounts.iter().any(|a| a.name == *name && !a.can_download))
    {
        return Err("所选凭据没有可下载的 CPA 授权文件，请刷新后重新选择".into());
    }
    let url = format!("{}{}/download", base_url(provider)?, root(&version));
    let files = stream::iter(names.into_iter().map(|name| {
        let request = client
            .get(&url)
            .bearer_auth(&provider.auth.access_token)
            .query(&[("name", &name)]);
        async move {
            let value = http::json(request, "读取 CPA 授权文件").await?;
            Ok::<_, String>(OAuthImportFile {
                name,
                content: serde_json::to_string(&value).map_err(|_| "授权文件格式无效")?,
            })
        }
    }))
    .buffer_unordered(4);
    let (files, _) = files
        .try_fold(
            (Vec::new(), 0usize),
            |(mut files, total), file| async move {
                let total = total.saturating_add(file.content.len());
                if total > 8 * 1024 * 1024 {
                    return Err("所选授权超过 8 MiB，请分批导入".into());
                }
                files.push(file);
                Ok((files, total))
            },
        )
        .await?;
    Ok(files)
}

pub(crate) async fn upload(
    settings: &AppSettings,
    provider: &Provider,
    file: &OAuthExportFile,
) -> Result<(), String> {
    validate_name(&file.name)?;
    let client = http::client(settings, provider).await?;
    let CpaInventory {
        version, accounts, ..
    } = inventory(&client, provider).await?;
    if accounts.iter().any(|a| a.name == file.name) {
        return Err("CPA 已有同名授权文件，请在 CPA 中核对；未覆盖远端文件".into());
    }
    let value = http::json(
        client
            .post(format!("{}{}", base_url(provider)?, root(&version)))
            .bearer_auth(&provider.auth.access_token)
            .query(&[("name", &file.name)])
            .header("Content-Type", "application/json")
            .body(file.content.clone()),
        "上传 CPA 授权文件",
    )
    .await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认导入成功，请刷新远端列表核对，勿重复提交".into());
    }
    Ok(())
}

/// A mutation always re-reads the inventory under one client: the version and
/// the exact account are checked before anything is written.
async fn session(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<(Client, String, Vec<CpaAccountSummary>), String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory {
        version, accounts, ..
    } = inventory(&client, provider).await?;
    Ok((client, version, accounts))
}

fn verified<'a>(
    accounts: &'a [CpaAccountSummary],
    name: &str,
    auth_index: &str,
) -> Result<&'a CpaAccountSummary, String> {
    validate_name(name)?;
    // v0 does not expose an auth index, so an empty index matches by name only.
    accounts
        .iter()
        .find(|a| a.name == name && (auth_index.trim().is_empty() || a.auth_index == auth_index))
        .ok_or_else(|| "CPA 授权文件已变化，请刷新后重试".to_string())
}

async fn patch(
    client: &Client,
    provider: &Provider,
    version: &str,
    kind: &str,
    body: &Value,
) -> Result<(), String> {
    let value = http::json(
        client
            .request(
                Method::PATCH,
                format!("{}{}/{kind}", base_url(provider)?, root(version)),
            )
            .bearer_auth(&provider.auth.access_token)
            .json(body),
        "更新 CPA 授权",
    )
    .await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认变更，请刷新远端列表核对".into());
    }
    Ok(())
}

pub(crate) async fn set_disabled(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
    auth_index: &str,
    disabled: bool,
) -> Result<(), String> {
    let (client, version, accounts) = session(settings, provider).await?;
    verified(&accounts, name, auth_index)?;
    patch(
        &client,
        provider,
        &version,
        "status",
        &json!({"name":name,"auth_index":auth_index,"disabled":disabled}),
    )
    .await
}

/// Patches scheduling fields through `credentials/fields`. Omitted fields keep
/// their remote value; an empty note or empty list clears that field.
pub(crate) async fn patch_fields(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
    auth_index: &str,
    fields: &CpaCredentialFields,
) -> Result<(), String> {
    let (client, version, accounts) = session(settings, provider).await?;
    verified(&accounts, name, auth_index)?;
    let mut body = serde_json::Map::new();
    body.insert("name".into(), name.into());
    if !auth_index.trim().is_empty() {
        body.insert("auth_index".into(), auth_index.into());
    }
    if let Some(priority) = fields.priority {
        body.insert("priority".into(), priority.into());
    }
    if let Some(weight) = fields.weight {
        body.insert("weight".into(), weight.into());
    }
    if let Some(retry) = fields.request_retry {
        // Null removes the per-account override; zero is not a valid retry count.
        body.insert(
            "request_retry".into(),
            if retry <= 0 {
                Value::Null
            } else {
                retry.into()
            },
        );
    }
    if let Some(note) = &fields.note {
        body.insert("note".into(), Value::String(note.trim().into()));
    }
    if let Some(models) = &fields.excluded_models {
        body.insert("excluded_models".into(), json!(models));
    }
    if body.len() == 1 {
        return Err("没有需要更新的字段".into());
    }
    patch(&client, provider, &version, "fields", &Value::Object(body)).await
}

/// Asks CPA to rotate the stored token itself. Success only means the request
/// was accepted, so the caller re-reads the inventory instead of trusting this.
pub(crate) async fn refresh_remote(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
    auth_index: &str,
) -> Result<(), String> {
    let (client, version, accounts) = session(settings, provider).await?;
    verified(&accounts, name, auth_index)?;
    let mut body = json!({"name":name});
    if !auth_index.trim().is_empty() {
        body["auth_index"] = auth_index.into();
    }
    let value = http::json(
        client
            .post(format!("{}{}/refresh", base_url(provider)?, root(&version)))
            .bearer_auth(&provider.auth.access_token)
            .json(&body),
        "请求 CPA 续期授权",
    )
    .await?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("CPA 未确认续期请求，请刷新远端列表核对".into());
    }
    Ok(())
}

/// Deletes exactly the account the user confirmed. `auth_index` guards against
/// a file replaced under the same name since the panel was rendered.
pub(crate) async fn delete(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
    auth_index: &str,
) -> Result<(), String> {
    let (client, version, accounts) = session(settings, provider).await?;
    verified(&accounts, name, auth_index)?;
    let value = http::json(
        client
            .request(
                Method::DELETE,
                format!("{}{}", base_url(provider)?, root(&version)),
            )
            .bearer_auth(&provider.auth.access_token)
            .query(&[("name", name)]),
        "移除 CPA 授权",
    )
    .await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认移除，请刷新远端列表核对".into());
    }
    Ok(())
}

/// Clears CPA's quota and cooldown routing state for one credential. The
/// caller re-reads the inventory afterwards: the response only reports which
/// models became usable again.
pub(crate) async fn reset_cooldown(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
    auth_index: &str,
) -> Result<(), String> {
    // Both the v8 and the v0 route address the credential by its index, so a
    // gateway that does not expose one cannot be reset by this action.
    if auth_index.trim().is_empty() {
        return Err("CPA 未提供该授权的索引，请在 CPA 面板中重置冷却".into());
    }
    let (client, version, accounts) = session(settings, provider).await?;
    verified(&accounts, name, auth_index)?;
    let path = if version == "v8" {
        "/routing/cooldown/reset"
    } else {
        "/reset-quota"
    };
    let value = http::json(
        client
            .post(format!(
                "{}{}{path}",
                base_url(provider)?,
                management_root(&version)
            ))
            .bearer_auth(&provider.auth.access_token)
            .json(&json!({"auth_index": auth_index})),
        "重置 CPA 授权冷却",
    )
    .await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认重置冷却，请刷新远端列表核对".into());
    }
    Ok(())
}

/// Asks CPA to refresh every stored credential at once. Like a single
/// credential request, success only means the gateway accepted the work.
pub(crate) async fn refresh_all(settings: &AppSettings, provider: &Provider) -> Result<(), String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let value = http::json(
        client
            .post(format!("{}{}/refresh", base_url(provider)?, root(&version)))
            .bearer_auth(&provider.auth.access_token)
            .json(&json!({"all": true})),
        "请求 CPA 续期全部授权",
    )
    .await?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("CPA 未确认全部续期请求，请刷新远端列表核对".into());
    }
    Ok(())
}

/// Lists the plugins the gateway reports. CPA exposes both known plugin files
/// and configured instances here; per-plugin writes use the v0 management
/// routes, because the v8 configuration tree has no equivalent.
pub(crate) async fn plugins(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<CpaPluginList, String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let value = http::json(
        client
            .get(format!(
                "{}{}/plugins",
                base_url(provider)?,
                management_root(&version)
            ))
            .bearer_auth(&provider.auth.access_token),
        "读取 CPA 插件列表",
    )
    .await?;
    parse_plugins(&value)
}

fn parse_plugins(value: &Value) -> Result<CpaPluginList, String> {
    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or("CPA 未返回插件列表")?;
    Ok(CpaPluginList {
        enabled: value
            .get("plugins_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        plugins: plugins
            .iter()
            .map(|item| {
                let id = http::text(item, &["id"]);
                let metadata = item.get("metadata");
                let metadata_text = |key: &str| {
                    metadata
                        .and_then(|meta| meta.get(key))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                };
                CpaPlugin {
                    name: metadata_text("name").unwrap_or_else(|| id.clone()),
                    version: metadata_text("version").unwrap_or_default(),
                    enabled: item
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    effective_enabled: item
                        .get("effective_enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    configured: item
                        .get("configured")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    registered: item
                        .get("registered")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    supports_quota: item
                        .get("supports_quota")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    oauth_provider: http::text(item, &["oauth_provider"]),
                    id,
                }
            })
            .collect(),
    })
}

/// Asks the gateway which release it would consider current. CPA itself calls
/// GitHub for this, so the answer depends on the gateway host's network and can
/// be unavailable even when the connection works. The comparison is done here
/// so the panel does not have to parse version strings.
pub(crate) async fn latest_release(
    client: &Client,
    provider: &Provider,
    current: &str,
) -> Result<CpaVersionCheck, String> {
    require_key(provider)?;
    let base = base_url(provider)?;
    let mut response = client
        .get(format!("{base}/v8/management/server/latest-version"))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if matches!(response.status().as_u16(), 404 | 405) {
        response = client
            .get(format!("{base}/v0/management/latest-version"))
            .bearer_auth(&provider.auth.access_token)
            .send()
            .await
            .map_err(|_| "连接 CPA 管理接口失败")?;
    }
    let value = read_response(response, "检查 CPA 新版本").await?;
    let latest = value
        .get("latest-version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or("CPA 未返回最新版本")?
        .to_owned();
    Ok(CpaVersionCheck {
        update_available: compare_versions(&latest, current),
        current: current.to_owned(),
        latest,
    })
}

/// Dotted numeric comparison that tolerates the `v` prefix and a build suffix
/// such as `-12-gabc1234`. Anything it cannot read yields no verdict rather
/// than a guess.
fn compare_versions(latest: &str, current: &str) -> Option<bool> {
    let parse = |text: &str| -> Option<Vec<u64>> {
        let core = text
            .trim()
            .trim_start_matches(['v', 'V'])
            .split(['-', '+'])
            .next()?;
        if core.is_empty() {
            return None;
        }
        core.split('.').map(|part| part.parse().ok()).collect()
    };
    let latest = parse(latest)?;
    let current = parse(current)?;
    let width = latest.len().max(current.len());
    let pad = |value: &[u64]| -> Vec<u64> {
        let mut out = value.to_vec();
        out.resize(width, 0);
        out
    };
    Some(pad(&latest) > pad(&current))
}

/// Per-plugin writes exist only on the v0 routes; the v8 configuration tree
/// lists plugins but offers no way to enable or configure one. The id reaches
/// the path directly, so it is checked against the gateway's own rule.
fn plugin_path(id: &str, suffix: &str) -> Result<String, String> {
    Ok(format!(
        "/v0/management/plugins/{}{suffix}",
        identifier(id, "CPA 插件标识无效")?
    ))
}

fn plugin_url(provider: &Provider, id: &str, suffix: &str) -> Result<String, String> {
    Ok(format!(
        "{}{}",
        base_url(provider)?,
        plugin_path(id, suffix)?
    ))
}

fn confirmed(value: &Value, message: &str) -> Result<(), String> {
    if value.get("status").and_then(Value::as_str) == Some("ok") {
        Ok(())
    } else {
        Err(message.into())
    }
}

/// Reads the JSON object CPA stores under `plugins.configs.<id>`. A registered
/// or discovered plugin without a saved configuration answers with `{}`, which
/// the panel shows as an empty draft rather than as an error.
pub(crate) async fn plugin_config(
    client: &Client,
    provider: &Provider,
    id: &str,
) -> Result<CpaPluginConfig, String> {
    require_key(provider)?;
    let response = client
        .get(plugin_url(provider, id, "/config")?)
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if response.status().as_u16() == 404 {
        return Err("网关上找不到该插件".into());
    }
    let value = read_response(response, "读取 CPA 插件配置").await?;
    if !value.is_object() {
        return Err("CPA 插件配置格式无效".into());
    }
    Ok(CpaPluginConfig {
        content: serde_json::to_string_pretty(&value).map_err(|_| "CPA 插件配置格式无效")?,
    })
}

/// Flips the per-plugin switch only; the gateway-wide plugin system stays as
/// it is, so a plugin enabled here still waits for that switch to be on.
pub(crate) async fn set_plugin_enabled(
    client: &Client,
    provider: &Provider,
    id: &str,
    enabled: bool,
) -> Result<(), String> {
    require_key(provider)?;
    let value = http::json(
        client
            .patch(plugin_url(provider, id, "/enabled")?)
            .bearer_auth(&provider.auth.access_token)
            .json(&json!({ "enabled": enabled })),
        "更新 CPA 插件状态",
    )
    .await?;
    confirmed(&value, "CPA 未确认插件状态变更，请刷新后核对")
}

/// Removes a plugin from the gateway. The plugin file and its saved
/// configuration go together, and a plugin that is still loaded cannot be
/// deleted until the gateway restarts.
pub(crate) async fn remove_plugin(
    client: &Client,
    provider: &Provider,
    id: &str,
) -> Result<CpaPluginRemoval, String> {
    require_key(provider)?;
    let response = client
        .delete(plugin_url(provider, id, "")?)
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    let status = response.status();
    let text = network::read_http_text(response, "移除 CPA 插件")
        .await
        .map_err(|_| "移除 CPA 插件响应读取失败")?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(removal_reason(&value)
            .unwrap_or_else(|| http::http_error("移除 CPA 插件", status.as_u16())));
    }
    if value.get("status").and_then(Value::as_str) != Some("deleted") {
        return Err("CPA 未确认插件移除，请刷新插件列表核对".into());
    }
    Ok(CpaPluginRemoval {
        file_deleted: value
            .get("file_deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        configured_removed: value
            .get("configured_removed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// The refusals a plugin removal can carry, in the panel's words. A failed
/// save keeps its detail because it tells the user the file is gone but the
/// gateway configuration still names the plugin.
fn removal_reason(value: &Value) -> Option<String> {
    let detail = value
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    Some(match value.get("error").and_then(Value::as_str)? {
        "plugin_not_found" => "网关已没有该插件".into(),
        "plugin_delete_requires_restart" => "该插件正在运行，重启网关后才能移除".into(),
        "plugin_delete_failed" => {
            if detail.is_empty() {
                "移除插件文件失败".into()
            } else {
                format!("移除插件文件失败：{detail}")
            }
        }
        "config_save_failed" => {
            if detail.is_empty() {
                "插件已删除，但网关未能保存配置".into()
            } else {
                format!("插件已删除，但网关未能保存配置：{detail}")
            }
        }
        "plugin_directory_invalid" | "plugin_discovery_failed" => {
            if detail.is_empty() {
                "网关插件目录不可用".into()
            } else {
                format!("网关插件目录不可用：{detail}")
            }
        }
        _ => return None,
    })
}

/// Replaces the plugin's saved configuration with the edited object. CPA
/// reloads the gateway from the new file after this write.
pub(crate) async fn update_plugin_config(
    client: &Client,
    provider: &Provider,
    id: &str,
    content: &str,
) -> Result<CpaPluginConfig, String> {
    require_key(provider)?;
    let object: Value = serde_json::from_str(content).map_err(|_| "插件配置不是有效的 JSON")?;
    if !object.is_object() {
        return Err("插件配置必须是 JSON 对象".into());
    }
    let value = http::json(
        client
            .put(plugin_url(provider, id, "/config")?)
            .bearer_auth(&provider.auth.access_token)
            .json(&object),
        "保存 CPA 插件配置",
    )
    .await?;
    confirmed(&value, "CPA 未确认插件配置变更，请刷新后核对")?;
    Ok(CpaPluginConfig {
        content: serde_json::to_string_pretty(&object).map_err(|_| "插件配置不是有效的 JSON")?,
    })
}

/// Static model definitions for one OAuth channel: the models the gateway can
/// serve there. The panel offers them as choices instead of asking the user to
/// type an ID into an exclusion rule. Both management generations mount the v0
/// route, so one request is enough; v8 adds a mirror under its routing group.
pub(crate) async fn model_definitions(
    client: &Client,
    provider: &Provider,
    channel: &str,
) -> Result<Vec<CpaModelDefinition>, String> {
    require_key(provider)?;
    let channel = identifier(channel, "CPA 模型通道无效")?;
    let response = client
        .get(format!(
            "{}/v0/management/model-definitions/{channel}",
            base_url(provider)?
        ))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    let status = response.status();
    let text = network::read_http_text(response, "读取 CPA 通道模型")
        .await
        .map_err(|_| "读取 CPA 通道模型响应读取失败")?;
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        // A 400 here names a channel the gateway does not know, which is a
        // fixable choice rather than a broken connection.
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(match error {
            "unknown channel" => format!("CPA 不认识通道「{channel}」，请确认通道名后再读"),
            "channel is required" => "请先选择模型通道".into(),
            _ => http::http_error("读取 CPA 通道模型", status.as_u16()),
        });
    }
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or("CPA 通道模型格式无效")?;
    Ok(models.iter().filter_map(model_definition).collect())
}

/// The catalog entry a channel lists. A definition without an ID cannot be
/// named in a rule, so it is dropped rather than shown as a blank choice.
fn model_definition(value: &Value) -> Option<CpaModelDefinition> {
    let id = http::text(value, &["id", "name"]);
    if id.is_empty() {
        return None;
    }
    Some(CpaModelDefinition {
        id,
        display_name: http::text(value, &["display_name", "displayName"]),
        model_type: http::text(value, &["type"]),
        owned_by: http::text(value, &["owned_by", "ownedBy"]),
        context_length: value
            .get("context_length")
            .and_then(Value::as_i64)
            .filter(|length| *length > 0),
    })
}

/// Gateway-wide model rules. CPA serves them from the v0 value routes (the v8
/// tree only projects the same keys), and answers `null` for a map that has
/// never been written. Excluded models hide an upstream model from clients;
/// aliases rename one, or publish a copy beside it when `fork` is set.
pub(crate) async fn model_rules(
    client: &Client,
    provider: &Provider,
) -> Result<CpaModelRules, String> {
    require_key(provider)?;
    Ok(CpaModelRules {
        excluded_models: rule_map(
            client,
            provider,
            "oauth-excluded-models",
            "读取 CPA 模型排除",
        )
        .await?,
        model_aliases: rule_map(client, provider, "oauth-model-alias", "读取 CPA 模型别名").await?,
    })
}

async fn rule_map(
    client: &Client,
    provider: &Provider,
    route: &str,
    action: &str,
) -> Result<String, String> {
    let value = http::json(
        client
            .get(format!("{}/v0/management/{route}", base_url(provider)?))
            .bearer_auth(&provider.auth.access_token),
        action,
    )
    .await?;
    rule_map_text(route, &value)
}

/// A missing wrapper key is an error, not an empty map: saving the `{}` that
/// would stand in for it would erase rules the gateway still holds.
fn rule_map_text(route: &str, value: &Value) -> Result<String, String> {
    match value.get(route) {
        None => Err(format!("CPA 未返回 {route}")),
        Some(Value::Null) => Ok("{}".into()),
        Some(Value::Object(map)) => {
            serde_json::to_string_pretty(map).map_err(|_| "CPA 返回的模型规则格式无效".into())
        }
        Some(_) => Err("CPA 返回的模型规则格式无效".into()),
    }
}

/// Both maps are channel → list. Checking the shape here names the offending
/// channel, which the gateway's bare HTTP 400 does not; it does not reject what
/// CPA would keep, so normalization still happens on the gateway's side.
fn check_rule_document(kind: CpaModelRuleKind, object: &Value) -> Result<(), String> {
    let entries = object.as_object().ok_or("模型规则必须是 JSON 对象")?;
    for (channel, items) in entries {
        let items = items
            .as_array()
            .ok_or_else(|| format!("通道 {channel} 的值必须是数组"))?;
        if kind == CpaModelRuleKind::ExcludedModels {
            if items.iter().any(|item| !item.is_string()) {
                return Err(format!("通道 {channel} 的排除模型必须是字符串"));
            }
            continue;
        }
        for item in items {
            let field = |name: &str| -> bool {
                item.get(name)
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
            };
            if !field("name") || !field("alias") {
                return Err(format!("通道 {channel} 的每个别名都需要 name 和 alias"));
            }
        }
    }
    Ok(())
}

/// Replaces one rule map. CPA normalizes what it stores — providers are
/// lowercased, empty lists and unset-only aliases are dropped — so the
/// confirmed rules are read back instead of echoing the draft.
pub(crate) async fn update_model_rules(
    client: &Client,
    provider: &Provider,
    kind: CpaModelRuleKind,
    content: &str,
) -> Result<CpaModelRules, String> {
    require_key(provider)?;
    let object: Value = serde_json::from_str(content).map_err(|_| "模型规则不是有效的 JSON")?;
    check_rule_document(kind, &object)?;
    let (route, action) = match kind {
        CpaModelRuleKind::ExcludedModels => ("oauth-excluded-models", "保存 CPA 模型排除"),
        CpaModelRuleKind::ModelAliases => ("oauth-model-alias", "保存 CPA 模型别名"),
    };
    let value = http::json(
        client
            .put(format!("{}/v0/management/{route}", base_url(provider)?))
            .bearer_auth(&provider.auth.access_token)
            .json(&object),
        action,
    )
    .await?;
    confirmed(&value, "CPA 未确认模型规则变更，请刷新后核对")?;
    model_rules(client, provider).await
}

/// Tail of the gateway's own log file. CPA answers 400 when file logging is
/// off, which the UI shows as an unavailable state rather than a failure.
///
/// `cursor` asks for what was written since the previous read, so following a
/// live gateway costs one small answer per poll instead of the whole tail. The
/// gateway reports when it rotated the file and could not honour the cursor,
/// and the caller then replaces its buffer with the returned lines.
pub(crate) async fn request_logs(
    settings: &AppSettings,
    provider: &Provider,
    limit: u32,
    cursor: &str,
) -> Result<CpaLogTail, String> {
    let cursor = log_cursor(cursor)?;
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let path = if version == "v8" {
        "/v8/management/observability/logs"
    } else {
        "/v0/management/logs"
    };
    let limit = limit.clamp(1, 500).to_string();
    let mut query = vec![("limit", limit.as_str())];
    if let Some(cursor) = cursor.as_deref() {
        query.push(("cursor", cursor));
    }
    let response = client
        .get(format!("{}{path}", base_url(provider)?))
        .query(&query)
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    log_response(response, "读取 CPA 请求日志")
        .await
        .and_then(|value| parse_request_logs(&value))
}

/// A follow cursor is an opaque base64url token the gateway handed out, so only
/// its alphabet and length are checked before it reaches a query string.
fn log_cursor(raw: &str) -> Result<Option<String>, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 4096
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
    {
        return Err("CPA 日志游标无效".into());
    }
    Ok(Some(value.to_owned()))
}

/// Reads a log answer. The gateway explains a disabled log in the body while
/// answering 400, and a rotated cursor arrives as data rather than an error.
async fn log_response(response: Response, context: &str) -> Result<Value, String> {
    let status = response.status();
    let text = network::read_http_text(response, context)
        .await
        .map_err(|_| format!("{context}响应读取失败"))?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(value);
    }
    if matches!(status.as_u16(), 401 | 403) {
        return Err(http::http_error(context, status.as_u16()));
    }
    // Only the disabled log gets its own words: that state is fixable from the
    // panel, so it must not read like a broken connection.
    if value.get("error").and_then(Value::as_str) == Some("logging to file disabled") {
        return Err("CPA 未开启日志写入文件，请求日志不可用".into());
    }
    Err(http::http_error(context, status.as_u16()))
}

fn parse_request_logs(value: &Value) -> Result<CpaLogTail, String> {
    let lines = value
        .get("lines")
        .and_then(Value::as_array)
        .ok_or("CPA 未返回请求日志")?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    Ok(CpaLogTail {
        lines,
        line_count: value
            .get("line-count")
            .or_else(|| value.get("line_count"))
            .and_then(Value::as_u64),
        latest_at: value
            .get("latest-timestamp")
            .or_else(|| value.get("latest_timestamp"))
            .and_then(Value::as_i64),
        next_cursor: http::text(value, &["next-cursor", "next_cursor"]),
        cursor_reset: value
            .get("cursor-reset")
            .or_else(|| value.get("cursor_reset"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Request counts for the API-key credentials the gateway holds in its own
/// configuration. They are not auth files, so the credential list cannot show
/// them; CPA reports one bucket per `base_url|api_key`, grouped by provider.
pub(crate) async fn api_key_usage(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<Vec<CpaApiKeyUsage>, String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let path = if version == "v8" {
        "/v8/management/observability/usage/api-keys"
    } else {
        "/v0/management/api-key-usage"
    };
    let value = http::json(
        client
            .get(format!("{}{path}", base_url(provider)?))
            .bearer_auth(&provider.auth.access_token),
        "读取 CPA 调用用量",
    )
    .await?;
    let providers = value.as_object().ok_or("CPA 调用用量格式无效")?;
    let mut usage = Vec::new();
    for (name, entries) in providers {
        let Some(entries) = entries.as_object() else {
            continue;
        };
        for (composite, entry) in entries {
            // The base URL is optional: an inline key without one is keyed as
            // `|key`, so the separator is the split, not the first character.
            let (base_url, key) = composite.split_once('|').unwrap_or(("", composite));
            usage.push(CpaApiKeyUsage {
                provider: name.clone(),
                base_url: base_url.to_owned(),
                key: key.to_owned(),
                success: cpa_number(entry, "success")
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or(0),
                failed: cpa_number(entry, "failed")
                    .and_then(|value| u64::try_from(value).ok())
                    .unwrap_or(0),
                recent_requests: cpa_request_buckets(entry.get("recent_requests")),
            });
        }
    }
    // Busiest first, then a stable order so re-reading does not reshuffle rows
    // whose counts are equal.
    usage.sort_by(|left, right| {
        (right.success + right.failed)
            .cmp(&(left.success + left.failed))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.key.cmp(&right.key))
    });
    Ok(usage)
}

/// One request's own log file. CPA names it after the last eight characters of
/// the request ID and serves the newest match as an attachment, so the body is
/// text rather than JSON. The file only exists while request logging is on,
/// which is why a miss explains that setting rather than reading as a failure.
pub(crate) async fn request_log(
    settings: &AppSettings,
    provider: &Provider,
    request_id: &str,
) -> Result<CpaRequestLog, String> {
    let request_id = validate_request_id(request_id)?;
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let path = if version == "v8" {
        format!("/v8/management/observability/logs/requests/{request_id}")
    } else {
        format!("/v0/management/request-log-by-id/{request_id}")
    };
    let response = client
        .get(format!("{}{path}", base_url(provider)?))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "读取 CPA 单次请求日志失败")?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Err(
            "CPA 未找到这次请求的日志文件；可能未开启「单独记录请求日志」，或日志已被清理".into(),
        );
    }
    if !status.is_success() {
        return Err(http::http_error("读取 CPA 单次请求日志", status.as_u16()));
    }
    let content = network::read_http_text(response, "读取 CPA 单次请求日志")
        .await
        .map_err(|_| "读取 CPA 单次请求日志失败".to_string())?;
    Ok(CpaRequestLog {
        request_id,
        content,
    })
}

/// The gateway matches the ID against log-file names, so only the characters an
/// ID can contain are accepted before it reaches the path.
fn validate_request_id(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    // The log line carries the shortened ID; the gateway accepts either and
    // narrows it itself, so the check is about shape rather than a UUID format.
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("CPA 请求 ID 无效".into());
    }
    Ok(value.to_owned())
}

/// Clears the shared log. CPA truncates the file it is writing and removes the
/// rotated ones, and refuses the whole call while file logging is off.
pub(crate) async fn clear_logs(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<CpaLogClear, String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let path = if version == "v8" {
        "/v8/management/observability/logs"
    } else {
        "/v0/management/logs"
    };
    let response = client
        .delete(format!("{}{path}", base_url(provider)?))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    let status = response.status();
    let text = network::read_http_text(response, "清理 CPA 请求日志")
        .await
        .map_err(|_| "清理 CPA 请求日志响应读取失败".to_string())?;
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        if value.get("error").and_then(Value::as_str) == Some("logging to file disabled") {
            return Err("CPA 未开启日志写入文件，没有可清理的请求日志".into());
        }
        return Err(http::http_error("清理 CPA 请求日志", status.as_u16()));
    }
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Err("CPA 未确认日志清理结果，请刷新后核对".into());
    }
    Ok(CpaLogClear {
        removed: value.get("removed").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// v8 keeps the switch in its configuration tree, where a missing leaf makes
/// the path answer 404; v0 has a route of its own. Both mean the gateway does
/// not write a log until the switch is on.
fn log_setting_path(version: &str) -> String {
    if version == "v8" {
        format!("{}/observability/logs/logging-to-file", config_root("v8"))
    } else {
        "/v0/management/logging-to-file".into()
    }
}

pub(crate) async fn log_settings(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<CpaLogSettings, String> {
    let client = http::client(settings, provider).await?;
    let session = management_session(&client, provider).await?;
    let response = client
        .get(format!(
            "{}{}",
            base_url(provider)?,
            log_setting_path(session.version)
        ))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    if response.status().as_u16() == 404 {
        return Ok(CpaLogSettings::default());
    }
    let value = read_response(response, "读取 CPA 日志设置").await?;
    let enabled = if session.version == "v8" {
        value.as_bool()
    } else {
        value.get("logging-to-file").and_then(Value::as_bool)
    };
    Ok(CpaLogSettings {
        enabled: enabled.unwrap_or(false),
    })
}

/// Turns the gateway's file log on or off. v0 owns one endpoint for the value;
/// v8 edits the config tree, whose write is guarded by the tag of the revision
/// it was read from so a concurrent edit fails instead of being overwritten.
pub(crate) async fn set_log_settings(
    settings: &AppSettings,
    provider: &Provider,
    enabled: bool,
) -> Result<(), String> {
    let client = http::client(settings, provider).await?;
    let session = management_session(&client, provider).await?;
    let base = base_url(provider)?;
    if session.version == "v0" {
        let value = http::json(
            client
                .put(format!("{base}/v0/management/logging-to-file"))
                .bearer_auth(&provider.auth.access_token)
                .json(&json!({ "value": enabled })),
            "更新 CPA 日志设置",
        )
        .await?;
        if value.get("status").and_then(Value::as_str) != Some("ok") {
            return Err("CPA 未确认日志设置变更，请刷新后核对".into());
        }
        return Ok(());
    }
    let mut request = client
        .put(format!(
            "{base}{}/observability/logs/logging-to-file",
            config_root("v8")
        ))
        .bearer_auth(&provider.auth.access_token)
        .json(&json!(enabled));
    if let Some(tag) = &session.etag {
        request = request.header(reqwest::header::IF_MATCH, tag);
    }
    let response = request.send().await.map_err(|_| "连接 CPA 管理接口失败")?;
    if response.status().as_u16() == 412 {
        return Err("网关配置已被其他会话修改，请刷新后重试".into());
    }
    let value = read_response(response, "更新 CPA 日志设置").await?;
    if value.get("status").and_then(Value::as_str) != Some("ok") {
        return Err("CPA 未确认日志设置变更，请刷新后核对".into());
    }
    Ok(())
}

/// Lists the request-error logs CPA has written on the gateway host. The list
/// is metadata only; the file itself is read on demand.
pub(crate) async fn error_logs(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<Vec<CpaErrorLog>, String> {
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let value = http::json(
        client
            .get(format!(
                "{}{}",
                base_url(provider)?,
                error_log_root(&version)
            ))
            .bearer_auth(&provider.auth.access_token),
        "读取 CPA 错误日志列表",
    )
    .await?;
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .ok_or("CPA 未返回错误日志列表")?;
    files
        .iter()
        .map(|file| {
            let name = http::text(file, &["name"]);
            validate_error_log_name(&name)?;
            Ok(CpaErrorLog {
                size: cpa_number(file, "size")
                    .and_then(|size| u64::try_from(size).ok())
                    .unwrap_or(0),
                modified_at: cpa_time(file, "modified"),
                name,
            })
        })
        .collect()
}

/// Reads one request-error log verbatim. CPA serves it as an attachment, so the
/// body is text rather than JSON.
pub(crate) async fn fetch_error_log(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
) -> Result<CpaErrorLogContent, String> {
    validate_error_log_name(name)?;
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let response = client
        .get(format!(
            "{}{}/{}",
            base_url(provider)?,
            error_log_root(&version),
            name
        ))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "读取 CPA 错误日志失败")?;
    let status = response.status();
    if !status.is_success() {
        return Err(http::http_error("读取 CPA 错误日志", status.as_u16()));
    }
    let content = network::read_http_text(response, "读取 CPA 错误日志")
        .await
        .map_err(|_| "读取 CPA 错误日志失败".to_string())?;
    Ok(CpaErrorLogContent {
        name: name.to_owned(),
        content,
    })
}

/// CPA only serves files it named itself, so the same shape is required here
/// before the name is placed in a request path.
fn validate_error_log_name(name: &str) -> Result<(), String> {
    if name.len() > 255
        || !name.starts_with("error-")
        || !name.ends_with(".log")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err("CPA 错误日志文件名无效".into());
    }
    Ok(())
}

/// Reads one remote authorization file verbatim. CPA serves an attachment, so
/// the body is read as text and only checked for JSON syntax.
pub(crate) async fn fetch_file(
    settings: &AppSettings,
    provider: &Provider,
    name: &str,
) -> Result<CpaCredentialFile, String> {
    let (client, version, accounts) = session(settings, provider).await?;
    let account = verified(&accounts, name, "")?;
    if !account.can_download {
        return Err(account
            .download_unavailable_reason
            .clone()
            .unwrap_or_else(|| "CPA 未提供此凭据的授权文件".into()));
    }
    let response = client
        .get(format!(
            "{}{}/download",
            base_url(provider)?,
            root(&version)
        ))
        .bearer_auth(&provider.auth.access_token)
        .query(&[("name", name)])
        .send()
        .await
        .map_err(|_| "读取 CPA 授权文件失败")?;
    let status = response.status();
    if !status.is_success() {
        return Err(http::http_error("读取 CPA 授权文件", status.as_u16()));
    }
    let content = network::read_http_text(response, "读取 CPA 授权文件")
        .await
        .map_err(|_| "读取 CPA 授权文件失败".to_string())?;
    serde_json::from_str::<Value>(&content)
        .map_err(|_| "CPA 授权文件内容不是有效 JSON".to_string())?;
    Ok(CpaCredentialFile {
        name: name.into(),
        content,
        editable: false,
        reason: Some(
            "CPA 管理接口只提供读取授权文件，修改请通过调度字段或 CPA 自身完成；替换整份授权会中断该账号的续期。"
                .into(),
        ),
    })
}

#[async_trait]
impl CredentialCapability for CliProxyApiAdapter {
    async fn complete_credentials(
        &self,
        _settings: &AppSettings,
        input: ProviderInput,
        _provider_id: String,
    ) -> Result<ProviderCredentialCompletionResult, String> {
        if input.auth.access_token.trim().is_empty() {
            return Err("请填写 CPA 管理密钥".into());
        }
        Ok(ProviderCredentialCompletionResult {
            input,
            changed_fields: Vec::new(),
            steps: Vec::new(),
            api_key_options: Vec::new(),
        })
    }
}

#[async_trait]
impl ConnectionCapability for CliProxyApiAdapter {
    async fn fetch_available_models(
        &self,
        client: &ProviderTransport,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderModelList>, String> {
        let client = client.non_replaying_client();
        let CpaInventory {
            version, accounts, ..
        } = inventory(client, provider).await?;
        Ok(ProviderOperationOutcome::unchanged(
            models(client, provider, &version, &accounts).await?,
        ))
    }
    async fn test_connection(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<ProviderConnectionTestResult>, String> {
        let client = http::client(settings, provider).await?;
        let CpaInventory { accounts, .. } = inventory(&client, provider).await?;
        Ok(ProviderOperationOutcome::unchanged(
            ProviderConnectionTestResult {
                ok: true,
                message: format!("CPA 已连接，共 {} 份授权", accounts.len()),
                available: None,
                used: None,
                quota_display: ProviderQuotaDisplay::default(),
                steps: Vec::new(),
            },
        ))
    }
    async fn probe_site(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderSiteProbeResult, String> {
        self.test_connection(settings, provider).await?;
        Ok(ProviderSiteProbeResult {
            ok: true,
            message: "CPA 管理接口已连接".into(),
            system_name: Some(provider.identity.name.clone()),
            logo: None,
            quota_display: ProviderQuotaDisplay::default(),
        })
    }
    async fn refresh_provider(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderOperationOutcome<()> {
        let mut next = provider.clone();
        next.quota.known = false;
        next.quota.total_known = false;
        let result = async {
            let client = http::client(settings, provider).await?;
            let CpaInventory {
                version,
                gateway_version,
                accounts,
            } = inventory(&client, provider).await?;
            match models(&client, provider, &version, &accounts).await {
                Ok(models) => next.capabilities.set_available_models(models),
                Err(error) => next.capabilities.available_models_state.error = Some(error),
            }
            // The management API exposes per-credential counters only: there is
            // no lifetime token total, and the usage queue is consume-only.
            let success_total = accounts
                .iter()
                .filter_map(|a| a.success)
                .fold(None, |total: Option<u64>, value| {
                    Some(total.unwrap_or(0).saturating_add(value))
                });
            let failed_total = accounts
                .iter()
                .filter_map(|a| a.failed)
                .fold(None, |total: Option<u64>, value| {
                    Some(total.unwrap_or(0).saturating_add(value))
                });
            let snapshot = CpaGatewaySnapshot {
                api_version: version,
                gateway_version,
                total_requests: (success_total.is_some() || failed_total.is_some()).then(|| {
                    success_total
                        .unwrap_or(0)
                        .saturating_add(failed_total.unwrap_or(0))
                }),
                failed_requests: failed_total,
                accounts,
                updated_at: Some(chrono::Utc::now().timestamp()),
            };
            next.capabilities.gateway = Some(snapshot);
            Ok::<(), String>(())
        }
        .await;
        next.automation.last_synced_at = Some(crate::util::unix_secs().to_string());
        match result {
            Ok(()) => {
                next.runtime.status = ProviderStatus::Ok;
                next.runtime.error_message = None;
            }
            Err(error) => {
                next.runtime.status = ProviderStatus::Error;
                next.runtime.error_message = Some(error);
            }
        }
        ProviderOperationOutcome::refreshed(provider, next)
    }
}
#[async_trait]
impl CapabilityProbe for CliProxyApiAdapter {
    async fn probe_capabilities(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> Result<ProviderOperationOutcome<(ProviderCapabilities, String, Option<String>)>, String>
    {
        self.test_connection(settings, provider).await?;
        let mut capabilities = provider.capabilities.clone();
        capabilities.check_in_known = true;
        capabilities.api_key_management_known = true;
        capabilities.invitation_known = true;
        Ok(ProviderOperationOutcome::unchanged((
            capabilities,
            "CPA 管理接口已连接".into(),
            None,
        )))
    }
}
