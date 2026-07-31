use super::super::{replace_file, INSTANCE_COUNTER};
use serde_json::Value as JsonValue;
use std::{fs, path::Path, sync::atomic::Ordering};
use toml_edit::{value as toml_value, Document as TomlDocument};

pub(super) fn rewrite_codex_config(
    config: &str,
    auth: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(String, String), String> {
    let mut document = config
        .parse::<TomlDocument>()
        .map_err(|_| "Codex 配置文件格式无效".to_string())?;
    let provider_name = document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Codex 配置缺少 model_provider，无法只更新中转站地址".to_string())?;
    let providers = document
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| "Codex 配置缺少 model_providers".to_string())?;
    let selected = providers
        .get_mut(&provider_name)
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| format!("Codex 配置缺少当前 provider：{provider_name}"))?;
    selected.insert("base_url", toml_value(base_url.trim()));

    let mut auth = serde_json::from_str::<JsonValue>(auth)
        .map_err(|_| "Codex 认证文件格式无效".to_string())?;
    let auth = auth
        .as_object_mut()
        .ok_or_else(|| "Codex 认证文件格式无效".to_string())?;
    auth.insert(
        "OPENAI_API_KEY".to_string(),
        JsonValue::String(api_key.trim().to_string()),
    );
    let auth = serde_json::to_string_pretty(auth)
        .map_err(|err| format!("生成 Codex 认证配置失败: {err}"))?;

    Ok((document.to_string(), format!("{auth}\n")))
}

pub(super) fn rewrite_claude_config(
    settings: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut settings = serde_json::from_str::<JsonValue>(settings)
        .map_err(|_| "Claude Code 配置文件格式无效".to_string())?;
    let settings = settings
        .as_object_mut()
        .ok_or_else(|| "Claude Code 配置文件格式无效".to_string())?;
    let env = settings
        .entry("env".to_string())
        .or_insert_with(|| JsonValue::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Claude Code 配置中的 env 不是对象".to_string())?;
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        JsonValue::String(base_url.trim().to_string()),
    );

    let has_auth_token = env.contains_key("ANTHROPIC_AUTH_TOKEN");
    let has_api_key = env.contains_key("ANTHROPIC_API_KEY");
    if has_auth_token || !has_api_key {
        env.insert(
            "ANTHROPIC_AUTH_TOKEN".to_string(),
            JsonValue::String(api_key.trim().to_string()),
        );
    }
    if has_api_key {
        env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            JsonValue::String(api_key.trim().to_string()),
        );
    }

    serde_json::to_string_pretty(settings)
        .map(|text| format!("{text}\n"))
        .map_err(|err| format!("生成 Claude Code 配置失败: {err}"))
}

pub(super) fn write_config_text(path: &Path, text: &str, label: &str) -> Result<(), String> {
    let sequence = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("balancehub-{}-{sequence}.tmp", std::process::id()));
    fs::write(&tmp_path, text)
        .map_err(|err| format!("写入{label}临时文件失败({}): {err}", tmp_path.display()))?;
    if let Ok(metadata) = fs::metadata(path) {
        if let Err(err) = fs::set_permissions(&tmp_path, metadata.permissions()) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("保留{label}文件权限失败: {err}"));
        }
    }
    replace_file(&tmp_path, path).map_err(|err| format!("更新{label}失败: {err}"))
}

pub(super) fn parse_codex_config(config: &str, auth: &str) -> Result<Option<(String, String)>, ()> {
    let config = config.parse::<toml::Value>().map_err(|_| ())?;
    let auth = serde_json::from_str::<JsonValue>(auth).map_err(|_| ())?;
    let provider_name = config.get("model_provider").and_then(toml::Value::as_str);
    let base_url = provider_name
        .and_then(|name| config.get("model_providers")?.get(name))
        .and_then(|provider| provider.get("base_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = auth
        .get("OPENAI_API_KEY")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}

pub(super) fn parse_claude_config(settings: &str) -> Result<Option<(String, String)>, ()> {
    let settings = serde_json::from_str::<JsonValue>(settings).map_err(|_| ())?;
    let env = settings.get("env").and_then(JsonValue::as_object);
    let base_url = env
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = env
        .and_then(|env| {
            env.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
        })
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    Ok(match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => Some((base_url.to_string(), api_key.to_string())),
        _ => None,
    })
}
