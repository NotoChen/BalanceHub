use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, plain_secret, read_stable_optional, relay_snapshot, yaml_source, CliConfigTarget,
};
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::integration::validate_native_value;
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::{configured_path, user_home},
};
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};

pub(super) const PROVIDER_ID: &str = "balancehub";
const API: &str = "openai-completions";

/// omp's root: `~/.omp`, or `~/$PI_CONFIG_DIR`.
fn root_for_home(home: &Path) -> PathBuf {
    configured_path("PI_CONFIG_DIR")
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                home.join(path)
            }
        })
        .unwrap_or_else(|| home.join(".omp"))
}

/// omp's agent folder: a profile's when `OMP_PROFILE`/`PI_PROFILE` names one,
/// else `$PI_CODING_AGENT_DIR`, else `~/.omp/agent`.
pub(super) fn agent_dir_for_home(home: &Path) -> PathBuf {
    let root = root_for_home(home);
    let profile = configured_path("OMP_PROFILE")
        .or_else(|| configured_path("PI_PROFILE"))
        .map(|path| path.to_string_lossy().trim().to_string())
        .filter(|name| valid_profile(name));
    if let Some(name) = profile {
        return root.join("profiles").join(name).join("agent");
    }
    if let Some(dir) = configured_path("PI_CODING_AGENT_DIR").filter(|path| path.is_absolute()) {
        return dir;
    }
    root.join("agent")
}

pub(super) fn config_dir() -> Option<PathBuf> {
    user_home().map(|home| agent_dir_for_home(&home))
}

/// omp's native session directory. Isolating `PI_CODING_AGENT_DIR` for a
/// temporary launch would otherwise move new sessions into the launch folder;
/// the explicit `--session-dir` keeps them in the user's real location. Scan
/// and launch share the same rules, so a relative override is not re-based on
/// the project workdir.
pub(super) fn native_session_dir() -> Option<String> {
    let root = config_dir()?;
    Some(
        super::super::pi_family::sessions::session_directory(&root, &super::sessions::SPEC)
            .to_string_lossy()
            .into_owned(),
    )
}

/// The name omp's pi-utils accepts: lowercase letters, digits, `.`, `_` and
/// `-`; `default` is not a profile, and neither is a Windows device name.
fn valid_profile(name: &str) -> bool {
    !name.is_empty()
        && name != "default"
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && !name.ends_with('.')
        && !matches!(
            name.split('.').next().unwrap_or_default(),
            "con" | "prn" | "aux" | "nul" | "com0" | "com1" | "com2" | "com3" | "com4" | "com5"
                | "com6" | "com7" | "com8" | "com9" | "lpt0" | "lpt1" | "lpt2" | "lpt3" | "lpt4"
                | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9"
        )
}

pub(super) fn config_path(root: &Path) -> PathBuf {
    pick(root, "config")
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    pick(root, "models")
}

/// omp reads the `.yml` and falls back to the `.yaml` only when the `.yml`
/// does not exist.
fn pick(root: &Path, name: &str) -> PathBuf {
    let yml = root.join(format!("{name}.yml"));
    if !yml.exists() && root.join(format!("{name}.yaml")).exists() {
        root.join(format!("{name}.yaml"))
    } else {
        yml
    }
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let root = agent_dir_for_home(&home);
    let config = match read_stable_optional(&config_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 omp 配置失败"),
    };
    let models = match read_stable_optional(&models_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 omp 模型配置失败"),
    };
    let modified_at = latest_modified_at([config.as_ref(), models.as_ref()]);
    let (Some(config), Some(models)) = (config, models) else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(&config.text, &models.text) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "omp 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = agent_dir_for_home(&home);
    let config_path = config_path(&root);
    let models_path = models_path(&root);
    let config_before = read_stable_optional(&config_path)?.map(|file| file.text);
    let models_before = read_stable_optional(&models_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 omp".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let config_after = merge_config(
                config_before.as_deref().unwrap_or_default(),
                model,
            )?;
            let models_after = merge_models(
                models_before.as_deref().unwrap_or_default(),
                provider.identity.name.as_str(),
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: config_path,
                    before: config_before,
                    after: config_after,
                },
                ProviderConfigurationCandidate {
                    path: models_path,
                    before: models_before,
                    after: models_after,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err("omp 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(config: &str, models: &str) -> Result<Option<(String, String)>, ()> {
    if config.trim().is_empty() || models.trim().is_empty() {
        return Ok(None);
    }
    let config = yaml_source::value(config).map_err(|_| ())?;
    let selected = config
        .pointer("/modelRoles/default")
        .and_then(JsonValue::as_str)
        .map(str::trim);
    let Some(selected) = selected.and_then(|value| value.strip_prefix(&format!("{PROVIDER_ID}/"))) else {
        return Ok(None);
    };
    if selected.is_empty() {
        return Ok(None);
    }
    let models = yaml_source::value(models).map_err(|_| ())?;
    let entry = models
        .pointer("/providers/balancehub")
        .and_then(JsonValue::as_object);
    let base_url = entry
        .and_then(|entry| entry.get("baseUrl"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = entry
        .and_then(|entry| entry.get("apiKey"))
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

/// Standalone config files for a temporary process; the native agent folder
/// is never read or written.
pub(super) fn temporary_config(model: &str) -> Result<String, String> {
    merge_config("", model)
}

pub(super) fn temporary_models(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    merge_models("", "BalanceHub", model, base_url, api_key)
}

fn merge_config(source: &str, model: &str) -> Result<String, String> {
    yaml_source::edit(source, "omp 配置", |root| {
        let roles = yaml_source::mapping_member(root, "modelRoles", "omp 配置")?;
        roles.set("default", format!("{PROVIDER_ID}/{model}"));
        Ok(())
    })
}

fn merge_models(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = super::super::config_support::openai_base_url(base_url);
    yaml_source::edit(source, "omp 模型配置", |root| {
        let providers = yaml_source::mapping_member(root, "providers", "omp 模型配置")?;
        let entry = yaml_source::mapping_member(&providers, PROVIDER_ID, "omp 模型配置")?;
        if yaml_source::nonempty_text_at(&entry, "name").is_none()
            && yaml_source::nonempty_text_at(&entry, "displayName").is_none()
        {
            entry.set(
                "displayName",
                super::super::config_support::provider_display_name(provider_name),
            );
        }
        entry.set("baseUrl", base_url.as_str());
        entry.set("api", API);
        entry.remove("auth");
        entry.set("apiKey", api_key);
        let models = yaml_source::sequence_member(&entry, "models", "omp 模型配置")?;
        let present = (0..models.len()).any(|index| {
            models.get(index).as_ref().is_some_and(|node| {
                node.as_mapping()
                    .and_then(|entry| yaml_source::text_at(entry, "id"))
                    .as_deref()
                    == Some(model)
            })
        });
        if !present {
            let model_entry = yaml_edit::Mapping::new();
            model_entry.set("id", model);
            model_entry.set("name", model);
            models.push(model_entry);
        }
        Ok(())
    })
}
