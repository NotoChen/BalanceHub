use super::super::config_support::{
    cli_config_target, config_error, env_value, latest_modified_at, match_provider_key,
    normalize_endpoint, oauth_snapshot, plain_secret, provider_display_name, read_stable_optional,
    relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::{expand_tilde, validate_native_value};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";
const API_MODE: &str = "chat_completions";

/// `$HERMES_HOME`, else `%LOCALAPPDATA%\hermes` on Windows and `~/.hermes`
/// elsewhere. A configured `~` is the user's home, as Hermes expands it.
pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    configured_path("HERMES_HOME")
        .map(|path| expand_tilde(&path.to_string_lossy(), home))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| default_data_dir(home))
}

fn default_data_dir(home: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = configured_path("LOCALAPPDATA") {
            return app_data.join("hermes");
        }
    }
    home.join(".hermes")
}

/// The state root of the profile `hermes` starts in.
pub(super) fn active_data_dir() -> Option<PathBuf> {
    Some(active_dir_for_home(&user_home()?))
}

/// The profile `hermes` starts in: a `HERMES_HOME` that already names a
/// profile, else the one `active_profile` makes sticky, else the root.
pub(super) fn active_dir_for_home(home: &Path) -> PathBuf {
    let root = data_dir_for_home(home);
    if root
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "profiles")
    {
        return root;
    }
    let Some(name) = std::fs::read_to_string(root.join("active_profile"))
        .ok()
        .map(|value| value.trim_start_matches('\u{feff}').trim().to_string())
        .filter(|value| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && !value.contains(['/', '\\', '\0'])
        })
    else {
        return root;
    };
    let profile = root.join("profiles").join(name);
    if profile.is_dir() {
        profile
    } else {
        root
    }
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join("config.yaml")
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let path = config_path(&active_dir_for_home(&home));
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Hermes Agent 配置失败"),
    };
    let modified_at = latest_modified_at([file.as_ref()]);
    let Some(file) = file else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&file.text, &path) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Hermes Agent 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let path = config_path(&active_dir_for_home(&home));
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Hermes Agent".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_config(
                before.as_deref().unwrap_or_default(),
                provider.identity.name.as_str(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![ProviderConfigurationCandidate {
                path,
                before,
                after,
            }])
        }
        CliConfigTarget::OAuth { .. } => Err("Hermes Agent 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str, path: &Path) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root = yaml_source::value(text).map_err(|_| ())?;
    let provider = root
        .pointer("/model/provider")
        .and_then(JsonValue::as_str)
        .map(str::trim);
    let model = root
        .pointer("/model/default")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if provider != Some(PROVIDER_ID) || model.is_none() {
        return Ok(None);
    }
    let entry = root.pointer("/providers/balancehub").and_then(JsonValue::as_object);
    let base_url = entry
        .and_then(|entry| entry.get("base_url"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let Some(api_key) = entry
        .and_then(|entry| entry.get("api_key"))
        .and_then(JsonValue::as_str)
        .and_then(|value| resolve_key(value, path))
    else {
        return Ok(None);
    };
    Ok(Some((base_url.to_string(), api_key)))
}

/// Hermes expands `${VAR}` and `${env:VAR}` from the `.env` beside the config
/// and then from its environment. Another secret source is left as it is and
/// cannot be compared with a relay key.
fn resolve_key(value: &str, config: &Path) -> Option<String> {
    let env_file = config.parent()?.join(".env");
    let env_source = read_stable_optional(&env_file)
        .ok()
        .flatten()
        .map(|file| file.text)
        .unwrap_or_default();
    if !value.contains("${") {
        return plain_secret(value);
    }
    let mut resolved = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        resolved.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find('}')?;
        let body = after[..end].trim();
        let name = body.strip_prefix("env:").unwrap_or(body).trim();
        if name.is_empty() || name.contains(':') {
            return None;
        }
        let found = env_value(&env_source, name).or_else(|| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })?;
        resolved.push_str(found.trim());
        rest = &after[end + 1..];
    }
    resolved.push_str(rest);
    plain_secret(&resolved)
}

fn merge_config(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = super::super::config_support::openai_base_url(base_url);
    yaml_source::edit(source, "Hermes Agent 配置", |root| {
        let model_section = yaml_source::mapping_member(root, "model", "Hermes Agent 配置")?;
        model_section.set("provider", PROVIDER_ID);
        model_section.set("default", model.to_string());

        let providers = yaml_source::mapping_member(root, "providers", "Hermes Agent 配置")?;
        let entry = yaml_source::mapping_member(&providers, PROVIDER_ID, "Hermes Agent 配置")?;
        if yaml_source::nonempty_text_at(&entry, "name").is_none() {
            entry.set("name", provider_display_name(provider_name));
        }
        entry.set("base_url", base_url.as_str());
        entry.set("api_key", api_key);
        entry.set("api_mode", API_MODE);
        if yaml_source::mapping_at(&entry, "extra_headers").is_none() {
            let headers = yaml_source::mapping_member(&entry, "extra_headers", "Hermes Agent 配置")?;
            headers.set("User-Agent", "hermes-agent");
        }
        let models = yaml_source::sequence_member(&entry, "models", "Hermes Agent 配置")?;
        let present = (0..models.len())
            .filter_map(|index| models.get(index))
            .filter_map(|node| yaml_source::scalar_text(&node))
            .any(|value| value.trim() == model);
        if !present {
            models.push(model.to_string());
        }
        Ok(())
    })
}
