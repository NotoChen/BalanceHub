use super::super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, openai_base_url, plain_secret, provider_display_name, read_stable_optional,
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
use yaml_edit::{Mapping, Sequence};

const PROVIDER_ID: &str = "balancehub";
const PI_ROW: &str = "llm-pi-ai";
const PI_PLUGIN: &str = "@deepseek-ai/dsh-llm-pi-ai";
pub(super) const API_KEY_ENV: &str = "BALANCEHUB_DSH_API_KEY";
const DEFAULT_MODEL_ROW: &str = "agent-default-model";
const PATCH_CONTEXT: &str = "dsh 补丁配置";
const SETTINGS_CONTEXT: &str = "dsh 设置";

/// dsh keeps its home at `$DSH_HOME`, else `~/.dsh`.
pub(super) fn data_dir_for_home(home: &Path) -> PathBuf {
    configured_path("DSH_HOME")
        .map(|path| expand_tilde(&path.to_string_lossy(), home))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".dsh"))
}

pub(super) fn data_dir() -> Option<PathBuf> {
    Some(data_dir_for_home(&user_home()?))
}

/// The patch list dsh reads: the active profile's, else the desktop app's or
/// the web profile's, else the web profile a first start will make.
pub(super) fn patch_path_for_home(home: &Path) -> PathBuf {
    patch_path_for_root(&data_dir_for_home(home))
}

pub(super) fn patch_path_for_root(root: &Path) -> PathBuf {
    let profile = |name: &str| root.join("profiles").join(name).join("cordis.patch.yml");
    if let Some(name) = std::fs::read_to_string(root.join("active_profile"))
        .ok()
        .map(|value| value.trim_start_matches('\u{feff}').trim().to_string())
        .filter(|name| valid_profile(name))
    {
        let path = profile(&name);
        if path.is_file() {
            return path;
        }
    }
    let desktop = profile("desktop");
    let web = profile("web");
    if web.is_file() {
        return web;
    }
    // A dsh that booted a profile other than web still reads that profile's
    // list; web stays first when both exist, as dsh's own profile order does.
    if let Some(name) = std::fs::read_dir(root.join("profiles"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| valid_profile(name) && profile(name).is_file())
        .min()
    {
        return profile(&name);
    }
    if desktop.is_file() {
        return desktop;
    }
    web
}

fn valid_profile(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && name.len() <= 128
}

pub(super) fn settings_path_for_home(home: &Path) -> PathBuf {
    settings_path_for_root(&data_dir_for_home(home))
}

pub(super) fn settings_path_for_root(root: &Path) -> PathBuf {
    root.join("settings.yaml")
}

/// The provider/model dsh currently starts a new session with, read from the
/// saved pick or the active profile patch. A launch overlay must repeat the
/// provider because a patch row replaces the whole config object.
pub(super) fn active_default_model() -> Option<(String, String)> {
    let root = data_dir()?;
    let patch = std::fs::read_to_string(patch_path_for_root(&root)).ok()?;
    let items = yaml_source::value(&patch).ok()?;
    let items = items.as_array()?;
    let saved = read_stable_optional(&settings_path_for_root(&root))
        .ok()
        .flatten()
        .and_then(|file| yaml_source::value(&file.text).ok())
        .and_then(|root| root.get(DEFAULT_MODEL_ROW).cloned());
    let config = saved.or_else(|| last_config(items, DEFAULT_MODEL_ROW).cloned())?;
    let provider = config
        .get("provider")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let model = config
        .get("model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some((provider.to_string(), model.to_string()))
}

pub(super) fn temporary_patch(model: &str, base_url: &str) -> Result<String, String> {
    serde_yaml_ng::to_string(&serde_json::json!([
        {
            "id": PI_ROW,
            "name": PI_PLUGIN,
            "config": {
                "providers": {
                    PROVIDER_ID: {
                        "displayName": "BalanceHub",
                        "apiKeyEnv": API_KEY_ENV,
                        "api": "openai-completions",
                        "baseURL": openai_base_url(base_url),
                        "models": [{
                            "id": model,
                            "name": model,
                        }],
                    },
                },
            },
        },
        {
            "id": DEFAULT_MODEL_ROW,
            "config": {
                "provider": PROVIDER_ID,
                "model": model,
            },
        },
    ]))
    .map_err(|err| format!("生成 dsh 临时启动配置失败: {err}"))
}

pub(super) fn model_patch(provider: &str, model: &str) -> Result<String, String> {
    serde_yaml_ng::to_string(&serde_json::json!([
        {
            "id": DEFAULT_MODEL_ROW,
            "config": {
                "provider": provider,
                "model": model,
            },
        },
    ]))
    .map_err(|err| format!("生成 dsh 模型覆盖配置失败: {err}"))
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    let Some(home) = user_home() else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let patch_path = patch_path_for_home(&home);
    let settings_path = settings_path_for_home(&home);
    let patch = match read_stable_optional(&patch_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 dsh 补丁配置失败"),
    };
    let settings = match read_stable_optional(&settings_path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, "读取 dsh 设置失败"),
    };
    let modified_at = latest_modified_at([patch.as_ref(), settings.as_ref()]);
    let Some(patch) = patch else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(
        &patch.text,
        settings.as_ref().map(|file| file.text.as_str()),
    ) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, "dsh 配置文件格式无效")
        },
    }
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    let patch_path = patch_path_for_home(&home);
    let settings_path = settings_path_for_home(&home);
    let patch_before = read_stable_optional(&patch_path)?.map(|file| file.text);
    let settings_before = read_stable_optional(&settings_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err("请先在中转站编辑窗口填写 CLI 首选模型，再切换到 dsh".into());
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let name = provider.identity.name.as_str();
            let patch_after = merge_patch(
                patch_before.as_deref().unwrap_or_default(),
                name,
                model,
                &base_url,
                &api_key,
            )?;
            let mut candidates = vec![ProviderConfigurationCandidate {
                path: patch_path,
                before: patch_before,
                after: patch_after,
            }];
            if let Some(settings) = settings_before.as_deref() {
                let after = merge_settings(settings, name, model, &base_url, &api_key)?;
                if after != settings {
                    candidates.push(ProviderConfigurationCandidate {
                        path: settings_path,
                        before: Some(settings.to_string()),
                        after,
                    });
                }
            }
            Ok(candidates)
        }
        CliConfigTarget::OAuth { .. } => Err("dsh 不支持 OAuth 账号切换".into()),
    }
}

fn parse_config(patch: &str, settings: Option<&str>) -> Result<Option<(String, String)>, ()> {
    if patch.trim().is_empty() {
        return Ok(None);
    }
    let items = yaml_source::value(patch).map_err(|_| ())?;
    let items = items.as_array().ok_or(())?;
    let settings = settings
        .filter(|text| !text.trim().is_empty())
        .map(|text| yaml_source::value(text).map_err(|_| ()))
        .transpose()?;
    // A pick saved in settings.yaml overrides the patch entry; dsh restores it
    // on start and the CLI follows it.
    let saved = settings
        .as_ref()
        .and_then(|root| root.get(DEFAULT_MODEL_ROW))
        .filter(|entry| {
            entry
                .get("model")
                .and_then(JsonValue::as_str)
                .is_some_and(|model| !model.trim().is_empty())
        });
    let (provider, model) = match saved {
        Some(entry) => (
            entry.get("provider").and_then(JsonValue::as_str),
            entry.get("model").and_then(JsonValue::as_str),
        ),
        None => last_config(items, DEFAULT_MODEL_ROW).map_or((None, None), |config| {
            (
                config.get("provider").and_then(JsonValue::as_str),
                config.get("model").and_then(JsonValue::as_str),
            )
        }),
    };
    if model.map(str::trim).filter(|value| !value.is_empty()).is_none() {
        return Ok(None);
    }
    if provider.map(str::trim) != Some(PROVIDER_ID) {
        return Ok(None);
    }
    let route = last_config(items, PI_ROW)
        .and_then(|config| config.get("providers"))
        .and_then(|providers| providers.get(PROVIDER_ID))
        .or_else(|| {
            settings
                .as_ref()
                .and_then(|root| root.pointer("/llm-pi-ai/providers/balancehub"))
        });
    let Some(route) = route else {
        return Ok(None);
    };
    let base_url = route
        .get("baseURL")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    let api_key = route
        .get("apiKey")
        .and_then(JsonValue::as_str)
        .and_then(plain_secret);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

/// The last patch entry with this id that supplies a whole config. A later
/// metadata-only entry leaves the config before it in force.
fn last_config<'a>(items: &'a [JsonValue], id: &str) -> Option<&'a JsonValue> {
    items
        .iter()
        .rev()
        .filter(|item| item.get("id").and_then(JsonValue::as_str) == Some(id))
        .find_map(|item| item.get("config"))
}

fn merge_patch(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = openai_base_url(base_url);
    yaml_source::edit_sequence(source, PATCH_CONTEXT, |items| {
        let route = entry_with_config(items, PI_ROW)?;
        let config = yaml_source::mapping_member(&route, "config", PATCH_CONTEXT)?;
        let providers = yaml_source::mapping_member(&config, "providers", PATCH_CONTEXT)?;
        let provider = yaml_source::mapping_member(&providers, PROVIDER_ID, PATCH_CONTEXT)?;
        set_route(
            &provider,
            provider_name,
            model,
            &base_url,
            api_key,
            PATCH_CONTEXT,
        )?;

        let selection = entry_with_config(items, DEFAULT_MODEL_ROW)?;
        let config = yaml_source::mapping_member(&selection, "config", PATCH_CONTEXT)?;
        config.set("provider", PROVIDER_ID);
        config.set("model", model);
        config.remove("reasoningEffort");
        Ok(())
    })
}

fn merge_settings(
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let base_url = openai_base_url(base_url);
    yaml_source::edit(source, SETTINGS_CONTEXT, |root| {
        let pi = yaml_source::mapping_member(root, "llm-pi-ai", SETTINGS_CONTEXT)?;
        let providers = yaml_source::mapping_member(&pi, "providers", SETTINGS_CONTEXT)?;
        let provider = yaml_source::mapping_member(&providers, PROVIDER_ID, SETTINGS_CONTEXT)?;
        set_route(
            &provider,
            provider_name,
            model,
            &base_url,
            api_key,
            SETTINGS_CONTEXT,
        )?;
        // Only follow a pick this file already records: writing one into a
        // settings file that does not have it would fight the patch entry.
        if let Some(selection) = yaml_source::mapping_at(root, DEFAULT_MODEL_ROW) {
            selection.set("provider", PROVIDER_ID);
            selection.set("model", model);
        }
        Ok(())
    })
}

fn set_route(
    provider: &Mapping,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
    context: &str,
) -> Result<(), String> {
    if yaml_source::nonempty_text_at(provider, "displayName").is_none() {
        provider.set("displayName", provider_display_name(provider_name));
    }
    provider.set("api", "openai-completions");
    provider.set("baseURL", base_url);
    provider.set("apiKey", api_key);
    let models = yaml_source::sequence_member(provider, "models", context)?;
    let present = (0..models.len()).any(|index| {
        models.get(index).is_some_and(|node| {
            node.as_mapping()
                .and_then(|entry| yaml_source::text_at(entry, "id"))
                .as_deref()
                == Some(model)
        })
    });
    if !present {
        let entry = Mapping::new();
        entry.set("id", model);
        entry.set("name", model);
        models.push(entry);
    }
    Ok(())
}

fn entry_with_config(items: &Sequence, id: &str) -> Result<Mapping, String> {
    let found = (0..items.len()).rev().find(|index| {
        items.get(*index).is_some_and(|node| {
            node.as_mapping().is_some_and(|entry| {
                yaml_source::text_at(entry, "id").as_deref() == Some(id)
                    && yaml_source::mapping_at(entry, "config").is_some()
            })
        })
    });
    let index = match found {
        Some(index) => index,
        None => {
            let entry = Mapping::new();
            entry.set("id", id);
            entry.set("config", Mapping::new());
            if id == PI_ROW {
                entry.set("name", PI_PLUGIN);
            }
            items.push(entry);
            items.len() - 1
        }
    };
    items
        .get(index)
        .and_then(|node| node.as_mapping().cloned())
        .ok_or_else(|| format!("{PATCH_CONTEXT}中的 {id} 条目必须是映射"))
}
