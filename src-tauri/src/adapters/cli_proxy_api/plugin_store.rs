//! Browsing the gateway's plugin store and installing one entry.
//!
//! Installing writes a third-party binary into the gateway's plugin directory
//! and runs it there, so this module only forwards the entry the user named:
//! the gateway fetches the release, verifies it against its source and enables
//! the plugin itself. Nothing is downloaded or unpacked by the panel.
use super::{base_url, identifier, inventory, require_key, CpaInventory};
use crate::adapters::oauth::http;
use crate::models::*;
use crate::network;
use reqwest::Response;
use serde_json::{json, Value};

const CONTEXT: &str = "读取 CPA 插件商店";

/// v8 moved the store under the plugin group, while v0 kept the dash spelling
/// of a route that predates it. The two are otherwise the same endpoint.
fn store_root(version: &str) -> &'static str {
    if version == "v8" {
        "/v8/management/plugins/store"
    } else {
        "/v0/management/plugin-store"
    }
}

/// Lists what the gateway's configured store sources offer, together with what
/// it already holds. The gateway reaches the sources itself, so a failed source
/// is reported next to the entries the other sources still serve.
pub(crate) async fn store(
    settings: &AppSettings,
    provider: &Provider,
) -> Result<CpaPluginStore, String> {
    require_key(provider)?;
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let base = base_url(provider)?;
    let response = client
        .get(format!("{base}{}", store_root(&version)))
        .bearer_auth(&provider.auth.access_token)
        .send()
        .await
        .map_err(|_| "连接 CPA 管理接口失败")?;
    parse_store(&store_body(response, CONTEXT).await?)
}

/// Installs one store entry on the gateway. `source_id` travels with the id
/// because two sources may offer the same plugin under different releases, and
/// the gateway refuses to switch an installed plugin to another source.
pub(crate) async fn install(
    settings: &AppSettings,
    provider: &Provider,
    plugin_id: &str,
    source_id: &str,
    version: &str,
) -> Result<CpaStoreInstall, String> {
    require_key(provider)?;
    let id = identifier(plugin_id, "CPA 插件标识无效")?;
    let source = match source_id.trim() {
        "" => None,
        raw => Some(identifier(raw, "CPA 插件来源无效")?),
    };
    let release = match version.trim() {
        "" => None,
        raw => Some(valid_version(raw)?),
    };
    let client = http::client(settings, provider).await?;
    let CpaInventory { version, .. } = inventory(&client, provider).await?;
    let base = base_url(provider)?;
    let mut query: Vec<(&str, &str)> = Vec::new();
    if let Some(source) = source.as_deref() {
        query.push(("source", source));
    }
    if let Some(release) = release.as_deref() {
        query.push(("version", release));
    }
    let mut request = client
        .post(format!("{base}{}/{id}/install", store_root(&version)))
        .bearer_auth(&provider.auth.access_token);
    if !query.is_empty() {
        request = request.query(&query);
    }
    if let Some(release) = release.as_deref() {
        request = request.json(&json!({ "version": release }));
    }
    let value = store_body(
        request.send().await.map_err(|_| "连接 CPA 管理接口失败")?,
        "安装 CPA 插件",
    )
    .await?;
    if value.get("status").and_then(Value::as_str) != Some("installed") {
        return Err("CPA 未确认插件安装，请刷新插件列表核对".into());
    }
    Ok(CpaStoreInstall {
        id: http::text(&value, &["id"]),
        version: http::text(&value, &["version"]),
        restart_required: value
            .get("restart_required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// A plugin release reaches a query and a body, so only the characters a tag or
/// a semantic version is made of are accepted.
fn valid_version(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'));
    if !valid {
        return Err("CPA 插件版本号无效".into());
    }
    Ok(value.to_owned())
}

/// Reads the body before checking the status: the store's own reason names the
/// broken source or the refused release, which a status cannot.
async fn store_body(response: Response, context: &str) -> Result<Value, String> {
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
    Err(store_reason(&value).unwrap_or_else(|| http::http_error(context, status.as_u16())))
}

/// The refusals a store read or install can carry, in the panel's words. A
/// gateway-side failure keeps its detail, because it names the source or the
/// release the provider refused.
fn store_reason(value: &Value) -> Option<String> {
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let with_detail = |prefix: &str| {
        if message.is_empty() {
            prefix.to_owned()
        } else {
            format!("{prefix}：{message}")
        }
    };
    let code = value.get("error").and_then(Value::as_str)?;
    Some(match code {
        "plugin_not_found" => "插件商店里找不到该插件".into(),
        "plugin_store_source_not_found" => "该插件来源已不在网关的商店配置里".into(),
        "plugin_store_source_required" => "多个来源都有这个插件，请重新读取商店后再试".into(),
        "plugin_store_source_conflict" => "该插件已从另一个来源安装，需先卸载再切换来源".into(),
        "plugin_store_installed_source_unknown" => {
            "无法确认已安装插件的来源，请先在网关上卸载".into()
        }
        "plugin_update_requires_restart" => "该插件正在运行，重启网关后才能更新".into(),
        "plugin_store_rate_limited" => match value.get("retry_after").and_then(Value::as_u64) {
            Some(seconds) => format!("插件商店请求过于频繁，请 {seconds} 秒后重试"),
            None => "插件商店请求过于频繁，请稍后重试".into(),
        },
        "plugin_store_registry_failed" => with_detail("从插件商店拉取索引失败"),
        "plugin_store_source_invalid" => with_detail("网关的插件商店来源配置无效"),
        "plugin_install_failed" => with_detail("插件安装失败"),
        "plugin_manifest_invalid" => with_detail("插件清单无效"),
        "plugin_directory_invalid" | "plugin_discovery_failed" => with_detail("网关插件目录不可用"),
        "config_save_failed"
        | "config_update_failed"
        | "config_unavailable"
        | "plugin_manifest_failed" => with_detail("插件文件已写入，但网关未能启用它"),
        "invalid_request" => with_detail("插件安装请求无效"),
        _ => return None,
    })
}

fn parse_store(value: &Value) -> Result<CpaPluginStore, String> {
    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or("CPA 未返回插件商店列表")?;
    Ok(CpaPluginStore {
        enabled: value
            .get("plugins_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        plugins: plugins.iter().filter_map(store_plugin).collect(),
        source_errors: value
            .get("source_errors")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(store_source_error).collect())
            .unwrap_or_default(),
    })
}

/// A store entry without an id cannot be installed, and without a source the
/// gateway cannot know which release to fetch, so both are required. `store_id`
/// exists only to key the row when two sources offer the same plugin.
fn store_plugin(value: &Value) -> Option<CpaStorePlugin> {
    let id = http::text(value, &["id"]);
    if id.is_empty() {
        return None;
    }
    Some(CpaStorePlugin {
        id,
        source_id: http::text(value, &["source_id"]),
        source_name: http::text(value, &["source_name"]),
        name: http::text(value, &["name"]),
        description: http::text(value, &["description"]),
        author: http::text(value, &["author"]),
        version: http::text(value, &["version"]),
        repository: http::text(value, &["repository"]),
        tags: value
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        auth_required: value
            .get("auth_required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        auth_configured: value
            .get("auth_configured")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        installed: value
            .get("installed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        installed_version: http::text(value, &["installed_version"]),
        update_available: value
            .get("update_available")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn store_source_error(value: &Value) -> Option<CpaStoreSourceError> {
    let message = http::text(value, &["message"]);
    let source_id = http::text(value, &["source_id"]);
    if message.is_empty() && source_id.is_empty() {
        return None;
    }
    Some(CpaStoreSourceError {
        source_id,
        source_name: http::text(value, &["source_name"]),
        message,
    })
}
