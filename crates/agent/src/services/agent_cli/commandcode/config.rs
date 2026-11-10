use super::super::config_support::{
    cli_config_target, config_error, json_object_member, latest_modified_at, match_provider_key,
    merge_json_config, normalize_endpoint, oauth_snapshot, openai_base_url, parse_jsonc_document,
    plain_secret, provider_display_name, read_stable_optional, relay_snapshot, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::user_home,
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

/// Command Code keeps every native file in `~/.commandcode`; the home
/// directory is its only root, so an isolated launch moves `HOME` instead of
/// pointing the CLI at another directory.
pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    home.join(".commandcode")
}

pub(super) fn config_root() -> Option<PathBuf> {
    Some(config_dir_for_home(&user_home()?))
}

pub(super) fn user_config_path(root: &Path) -> PathBuf {
    root.join("config.json")
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub(super) fn providers_path(root: &Path) -> PathBuf {
    root.join("providers.json")
}

pub(super) fn credentials_path(root: &Path) -> PathBuf {
    root.join("credentials.json")
}

pub(super) fn mcp_path(root: &Path) -> PathBuf {
    root.join("mcp.json")
}

pub(super) fn optional_config_text(root: &Path, name: &str) -> Option<String> {
    read_stable_optional(&root.join(name))
        .ok()
        .flatten()
        .map(|file| file.text)
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(root) = config_root() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let user = match read_stable_optional(&user_config_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Command Code 用户配置失败"),
    };
    let settings = match read_stable_optional(&settings_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Command Code 设置失败"),
    };
    let provider_config = match read_stable_optional(&providers_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Command Code Provider 配置失败"),
    };
    let credentials = match read_stable_optional(&credentials_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 Command Code 凭据失败"),
    };
    let modified_at = latest_modified_at([
        user.as_ref(),
        settings.as_ref(),
        provider_config.as_ref(),
        credentials.as_ref(),
    ]);
    if user.is_none() && settings.is_none() {
        return oauth_snapshot(cli_kind, None, modified_at);
    }
    match parse_config(
        user.as_ref().map(|file| file.text.as_str()),
        settings.as_ref().map(|file| file.text.as_str()),
        provider_config
            .as_ref()
            .map(|file| file.text.as_str()),
        credentials.as_ref().map(|file| file.text.as_str()),
    ) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "Command Code 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = config_root().ok_or_else(|| "无法定位用户目录".to_string())?;
    let paths = Paths::new(&root);
    let before = paths
        .all()
        .into_iter()
        .map(|(_, path)| read_stable_optional(path).map(|file| file.map(|file| file.text)))
        .collect::<Result<Vec<_>, _>>()?;
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err(
                    "请先在中转站编辑窗口填写 CLI 首选模型，再切换到 Command Code".into(),
                );
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let documents = provider_documents(
                model,
                &provider.identity.name,
                &base_url,
                &api_key,
                before[0].as_deref(),
                before[1].as_deref(),
                before[2].as_deref(),
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: paths.providers.clone(),
                    before: before[0].clone(),
                    after: documents.providers,
                },
                ProviderConfigurationCandidate {
                    path: paths.credentials.clone(),
                    before: before[1].clone(),
                    after: documents.credentials,
                },
                ProviderConfigurationCandidate {
                    path: paths.user_config.clone(),
                    before: before[2].clone(),
                    after: documents.user_config,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("Command Code 不支持 OAuth 账号切换".into()),
    }
}

struct Paths {
    providers: PathBuf,
    credentials: PathBuf,
    user_config: PathBuf,
}

impl Paths {
    fn new(root: &Path) -> Self {
        Self {
            providers: providers_path(root),
            credentials: credentials_path(root),
            user_config: user_config_path(root),
        }
    }

    fn all(&self) -> [(&'static str, &Path); 3] {
        [
            ("providers", &self.providers),
            ("credentials", &self.credentials),
            ("user-config", &self.user_config),
        ]
    }
}

pub(super) struct ProviderDocuments {
    pub(super) providers: String,
    pub(super) credentials: String,
    pub(super) user_config: String,
}

/// The three native documents that select BalanceHub as a BYOK provider.
/// Callers pass the current text so an edit preserves comments and unrelated
/// entries; temporary launches pass empty strings.
#[allow(clippy::too_many_arguments)]
pub(super) fn provider_documents(
    model: &str,
    provider_name: &str,
    base_url: &str,
    api_key: &str,
    providers_source: Option<&str>,
    credentials_source: Option<&str>,
    user_config_source: Option<&str>,
) -> Result<ProviderDocuments, String> {
    Ok(ProviderDocuments {
        providers: merge_providers(
            providers_source.unwrap_or_default(),
            provider_name,
            model,
            &openai_base_url(base_url),
        )?,
        credentials: merge_credentials(credentials_source.unwrap_or_default(), api_key)?,
        user_config: merge_user_config(user_config_source.unwrap_or_default(), model)?,
    })
}

fn parse_config(
    user: Option<&str>,
    settings: Option<&str>,
    providers: Option<&str>,
    credentials: Option<&str>,
) -> Result<Option<(String, String)>, ()> {
    let user = parse_optional(user)?;
    let settings = parse_optional(settings)?;
    let active = active_model(user.as_ref(), settings.as_ref());
    let Some((provider, model)) = active else {
        return Ok(None);
    };
    if provider != PROVIDER_ID || model.is_empty() {
        return Ok(None);
    }
    let Some(providers_text) = providers.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let providers = parse_jsonc_document(providers_text).map_err(|_| ())?;
    let entry = providers
        .get("provider")
        .or_else(|| providers.get("providers"))
        .and_then(JsonValue::as_object)
        .and_then(|entries| entries.get(PROVIDER_ID))
        .and_then(JsonValue::as_object);
    let base_url = entry
        .and_then(|entry| entry.get("baseURL").or_else(|| entry.get("baseUrl")))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let Some(credentials_text) = credentials.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let credentials = parse_jsonc_document(credentials_text).map_err(|_| ())?;
    let api_key = credentials
        .get(PROVIDER_ID)
        .and_then(JsonValue::as_object)
        .filter(|entry| entry.get("kind").and_then(JsonValue::as_str) == Some("api"))
        .and_then(|entry| entry.get("token"))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

fn parse_optional(text: Option<&str>) -> Result<Option<JsonValue>, ()> {
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    parse_jsonc_document(text)
        .map(Some)
        .map_err(|_| ())
}

/// The settings layer is newer than the native config file and wins when both
/// define a model, matching how the CLI merges its own configuration.
fn active_model(
    user: Option<&JsonValue>,
    settings: Option<&JsonValue>,
) -> Option<(String, String)> {
    let settings_model = settings.and_then(text_field("model"));
    let model = settings_model.or_else(|| user.and_then(text_field("model")))?;
    let settings_provider = settings.and_then(text_field("modelProvider"));
    let provider = settings_provider.or_else(|| user.and_then(text_field("modelProvider")));
    match model.split_once('/') {
        Some((prefix, rest)) if !prefix.is_empty() && !rest.is_empty() => {
            Some((prefix.to_string(), rest.to_string()))
        }
        _ => provider
            .filter(|provider| !provider.is_empty())
            .map(|provider| (provider.to_string(), model.to_string())),
    }
}

fn text_field(key: &'static str) -> impl Fn(&JsonValue) -> Option<String> + 'static {
    move |value: &JsonValue| {
        value
            .get(key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}

fn merge_providers(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
) -> Result<String, String> {
    merge_json_config(source, "Command Code Provider 配置", |root| {
        let key = if root.get("provider").is_some() {
            "provider"
        } else if root.get("providers").is_some() {
            "providers"
        } else {
            "provider"
        };
        let providers = json_object_member(root, key, "Command Code Provider 配置")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "Command Code 的 Provider 条目必须是对象".to_string())?;
        if entry
            .get("name")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .is_none_or(|name| name.is_empty())
        {
            entry.insert(
                "name".into(),
                JsonValue::String(provider_display_name(provider_name)),
            );
        }
        entry.insert(
            "api".into(),
            JsonValue::String("openai-completions".into()),
        );
        entry.insert("baseURL".into(), JsonValue::String(base_url.into()));
        let models = json_object_member(entry, "models", "Command Code Provider 配置")?;
        models.insert(
            model.to_string(),
            serde_json::json!({ "name": model }),
        );
        Ok(())
    })
}

fn merge_credentials(source: &str, api_key: &str) -> Result<String, String> {
    merge_json_config(source, "Command Code 凭据", |root| {
        root.insert(
            PROVIDER_ID.into(),
            serde_json::json!({ "kind": "api", "token": api_key }),
        );
        Ok(())
    })
}

fn merge_user_config(source: &str, model: &str) -> Result<String, String> {
    merge_json_config(source, "Command Code 用户配置", |root| {
        root.insert(
            "model".into(),
            JsonValue::String(format!("{PROVIDER_ID}/{model}")),
        );
        root.insert(
            "modelProvider".into(),
            JsonValue::String(PROVIDER_ID.into()),
        );
        Ok(())
    })
}
