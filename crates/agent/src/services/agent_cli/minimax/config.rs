use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, plain_secret, read_stable_optional, relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::{expand_tilde, validate_native_value};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

const PROVIDER: &str = "custom_provider:balancehub";
const ENTRY: &str = "balancehub";
const USER_AGENT: &str = "minimax-code";

/// MiniMax Code keeps `config.yaml` in `$MINIMAX_DATA_DIR`, else `~/.minimax`.
pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    configured_path("MINIMAX_DATA_DIR")
        .map(|path| expand_tilde(&path.to_string_lossy(), home))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".minimax"))
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    root.join("config.yaml")
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let path = config_path(&data_dir_for_home(&home));
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 MiniMax Code 配置失败"),
    };
    let modified_at = latest_modified_at([file.as_ref()]);
    let Some(file) = file else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&file.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "MiniMax Code 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let path = config_path(&data_dir_for_home(&home));
    let before = read_stable_optional(&path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 MiniMax Code".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let after = merge_config(
                before.as_deref().unwrap_or_default(),
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
        CliConfigTarget::OAuth { .. } => Err("MiniMax Code 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let root = yaml_source::value(text).map_err(|_| ())?;
    let default = root
        .get("defaultModel")
        .and_then(JsonValue::as_str)
        .map(str::trim);
    let Some(default) = default.and_then(|value| value.strip_prefix(&format!("{PROVIDER}/"))) else {
        return Ok(None);
    };
    if default.is_empty() {
        return Ok(None);
    }
    let options = root
        .pointer("/custom_provider/balancehub/options")
        .and_then(JsonValue::as_object);
    let base_url = options
        .and_then(|options| options.get("baseURL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = options
        .and_then(|options| options.get("apiKey"))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

pub(super) fn merge_config(
    source: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    // MiniMax Code splits defaultModel at the first slash and looks the model
    // up under a key with each slash spelled `~` (its flat id).
    let flat = model.replace('/', "~");
    yaml_source::edit(source, "MiniMax Code 配置", |root| {
        root.set("defaultModel", format!("{PROVIDER}/{flat}"));
        root.remove("defaultModelVariant");
        let custom = yaml_source::mapping_member(root, "custom_provider", "MiniMax Code 配置")?;
        let entry = yaml_source::mapping_member(&custom, ENTRY, "MiniMax Code 配置")?;
        if yaml_source::nonempty_text_at(&entry, "name").is_none() {
            entry.set("name", "BalanceHub");
        }
        entry.set("kind", "custom");
        entry.set("enabled", true);
        entry.set("api", "anthropic-messages");
        let options = yaml_source::mapping_member(&entry, "options", "MiniMax Code 配置")?;
        options.set("apiKey", api_key);
        options.set("baseURL", base_url);
        options.set("authMode", "api-key");
        let headers = yaml_source::mapping_member(&options, "headers", "MiniMax Code 配置")?;
        headers.set("User-Agent", USER_AGENT);
        let models = yaml_source::mapping_member(&entry, "models", "MiniMax Code 配置")?;
        let model_entry = if models.contains_key(&flat) {
            yaml_source::mapping_member(&models, &flat, "MiniMax Code 配置")?
        } else {
            models.set(&flat, yaml_edit::Mapping::new());
            yaml_source::mapping_member(&models, &flat, "MiniMax Code 配置")?
        };
        if yaml_source::nonempty_text_at(&model_entry, "name").is_none() {
            model_entry.set("name", model);
        }
        Ok(())
    })
}
