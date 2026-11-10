//! Pi and Prime Agent read the same `settings.json` / `models.json` shapes.
//! This module keeps one parser and one writer for both, with the agent folder
//! and the optional schema hints left to each caller.
use super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, parse_jsonc_document, read_stable_optional, relay_snapshot,
    rewrite_json_document, CliConfigTarget,
};
use super::configuration::contracts::ProviderConfigurationCandidate;
use super::contracts::{ParsedLivenessOutput, ParsedTokenUsage};
use super::liveness_support::{extract_f64, extract_u64, token_sum};
use crate::models::{AgentCliKind, CliConfigSnapshot, Provider};
use crate::services::agent_cli::integration::validate_native_value;
use crate::services::cli_paths::user_home;
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

pub(super) const PROVIDER_ID: &str = "balancehub";

pub(crate) mod sessions;

pub(super) struct FamilySpec {
    pub label: &'static str,
    pub dir_for_home: fn(&Path) -> PathBuf,
    pub settings_schema: Option<&'static str>,
    pub models_schema: Option<&'static str>,
}

pub(super) fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub(super) fn models_path(root: &Path) -> PathBuf {
    root.join("models.json")
}

/// Reads one of the family's JSONC files when it exists; a missing or
/// unparsable file is simply absent for callers that only need a hint.
pub(super) fn read_optional_json(path: &Path) -> Option<JsonValue> {
    let file = read_stable_optional(path).ok().flatten()?;
    parse_jsonc_document(&file.text).ok()
}

pub(super) fn snapshot(
    spec: &FamilySpec,
    cli_kind: AgentCliKind,
    providers: &[Provider],
) -> CliConfigSnapshot {
    let Some(root) = user_home().map(|home| (spec.dir_for_home)(&home)) else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let settings = match read_stable_optional(&settings_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, &format!("读取 {} 设置失败", spec.label)),
    };
    let models = match read_stable_optional(&models_path(&root)) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, &format!("读取 {} 模型配置失败", spec.label)),
    };
    let modified_at = latest_modified_at([settings.as_ref(), models.as_ref()]);
    let Some(settings) = settings else {
        return oauth_snapshot(cli_kind, None, modified_at);
    };
    match parse_config(
        &settings.text,
        models.as_ref().map(|file| file.text.as_str()),
    ) {
        Ok(Some((base_url, api_key))) => relay_snapshot(
            cli_kind,
            match_provider_key(providers, cli_kind, &base_url, &api_key),
            modified_at,
        ),
        Ok(None) => oauth_snapshot(cli_kind, None, modified_at),
        Err(()) => CliConfigSnapshot {
            modified_at,
            ..config_error(cli_kind, &format!("{} 配置文件格式无效", spec.label))
        },
    }
}

pub(super) fn candidates(
    spec: &FamilySpec,
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    let root = user_home()
        .map(|home| (spec.dir_for_home)(&home))
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let settings_path = settings_path(&root);
    let models_path = models_path(&root);
    let settings = read_stable_optional(&settings_path)?.map(|file| file.text);
    let models = read_stable_optional(&models_path)?.map(|file| file.text);
    match cli_config_target(provider, cli_kind, api_key_local_id)? {
        CliConfigTarget::ApiKey { base_url, api_key } => {
            let model = provider.cli.preferred_model.trim();
            if model.is_empty() {
                return Err(format!(
                    "请先在中转站编辑窗口填写 CLI 首选模型，再切换到 {}",
                    spec.label
                ));
            }
            validate_native_value(model, "模型 ID")?;
            validate_native_value(&api_key, "API Key")?;
            let next_settings =
                merge_settings(spec, settings.as_deref().unwrap_or_default(), model)?;
            let next_models = merge_models(
                spec,
                models.as_deref().unwrap_or_default(),
                &provider.identity.name,
                model,
                &base_url,
                &api_key,
            )?;
            Ok(vec![
                ProviderConfigurationCandidate {
                    path: settings_path,
                    before: settings,
                    after: next_settings,
                },
                ProviderConfigurationCandidate {
                    path: models_path,
                    before: models,
                    after: next_models,
                },
            ])
        }
        CliConfigTarget::OAuth { .. } => Err(format!("{} 不支持 OAuth 账号切换", spec.label)),
    }
}

fn parse_config(
    settings_text: &str,
    models_text: Option<&str>,
) -> Result<Option<(String, String)>, ()> {
    if settings_text.trim().is_empty() {
        return Ok(None);
    }
    let settings = parse_jsonc_document(settings_text).map_err(|_| ())?;
    let settings = settings.as_object().ok_or(())?;
    let provider = settings
        .get("defaultProvider")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if provider != Some(PROVIDER_ID) {
        return Ok(None);
    }
    let Some(models_text) = models_text.filter(|text| !text.trim().is_empty()) else {
        return Ok(None);
    };
    let models = parse_jsonc_document(models_text).map_err(|_| ())?;
    models.as_object().ok_or(())?;
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
        .and_then(literal_or_environment);
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

/// Pi resolves `$NAME` and `${NAME}` in `models.json` at request time. Resolve
/// it for the active-config match without ever returning the value to the UI.
/// A `!command` value would execute an arbitrary program, so it is not read
/// here and the card simply reports no matched key.
fn literal_or_environment(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('!') {
        return None;
    }
    let resolved = if let Some(rest) = value.strip_prefix("${") {
        std::env::var(rest.strip_suffix('}')?).ok()?
    } else if let Some(rest) = value.strip_prefix("$$") {
        format!("${rest}")
    } else if let Some(rest) = value.strip_prefix("$!") {
        format!("!{rest}")
    } else if let Some(name) = value.strip_prefix('$') {
        if name.is_empty() {
            return None;
        }
        std::env::var(name).ok()?
    } else {
        value.to_string()
    };
    let resolved = resolved.trim();
    if resolved.is_empty() || resolved.chars().any(char::is_control) {
        return None;
    }
    Some(resolved.to_string())
}

fn merge_settings(spec: &FamilySpec, source: &str, model: &str) -> Result<String, String> {
    let mut root = if source.trim().is_empty() {
        Map::new()
    } else {
        parse_jsonc_document(source)?
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{} 设置文件根节点不是对象", spec.label))?
    };
    if source.trim().is_empty() {
        if let Some(schema) = spec.settings_schema {
            root.insert("$schema".into(), JsonValue::String(schema.into()));
        }
    }
    root.insert(
        "defaultProvider".into(),
        JsonValue::String(PROVIDER_ID.into()),
    );
    root.insert("defaultModel".into(), JsonValue::String(model.to_string()));
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 {} 设置失败: {err}", spec.label))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 {} 设置失败: {err}", spec.label))
    }
}

fn merge_models(
    spec: &FamilySpec,
    source: &str,
    provider_name: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    let mut root = if source.trim().is_empty() {
        Map::new()
    } else {
        parse_jsonc_document(source)?
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{} 模型配置根节点不是对象", spec.label))?
    };
    if source.trim().is_empty() {
        if let Some(schema) = spec.models_schema {
            root.insert("$schema".into(), JsonValue::String(schema.into()));
        }
    }
    {
        let providers = object_member(spec, &mut root, "providers")?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| {
                format!(
                    "{} 模型配置中的 providers.balancehub 必须是对象",
                    spec.label
                )
            })?;
        let display_name = provider_name.trim();
        entry.insert(
            "name".into(),
            JsonValue::String(if display_name.is_empty() {
                "BalanceHub".into()
            } else {
                format!("BalanceHub · {display_name}")
            }),
        );
        entry.insert("baseUrl".into(), JsonValue::String(base_url.to_string()));
        entry.insert("api".into(), JsonValue::String("openai-completions".into()));
        entry.insert("apiKey".into(), JsonValue::String(api_key.to_string()));
        let models = entry
            .entry("models".to_string())
            .or_insert_with(|| JsonValue::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("{} 模型配置中的 models 必须是数组", spec.label))?;
        match models
            .iter_mut()
            .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(model))
        {
            Some(entry) => {
                let entry = entry
                    .as_object_mut()
                    .ok_or_else(|| format!("{} 模型条目必须是对象", spec.label))?;
                let named = entry
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .is_some_and(|name| !name.is_empty());
                if !named {
                    entry.insert("name".into(), JsonValue::String(model.to_string()));
                }
            }
            None => models.push(serde_json::json!({ "id": model, "name": model })),
        }
    }
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 {} 模型配置失败: {err}", spec.label))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 {} 模型配置失败: {err}", spec.label))
    }
}

fn object_member<'a>(
    spec: &FamilySpec,
    root: &'a mut Map<String, JsonValue>,
    key: &str,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("{} 模型配置中的 {key} 必须是对象", spec.label))
}

/// The `--mode json` event stream shared by Pi-shaped CLIs: the committed
/// assistant `message_end` carries the answer, token usage, and the stop
/// reason. A transport failure arrives as a `message_end` with
/// `stopReason: error` plus `errorMessage`.
pub(super) fn parse_message_end_events(
    label: &str,
    response_output: &str,
    stdout: &str,
) -> ParsedLivenessOutput {
    let source = if response_output.trim().is_empty() {
        stdout
    } else {
        response_output
    };
    let mut text = String::new();
    let mut usage = ParsedTokenUsage::default();
    let mut error = None;
    for line in source.lines() {
        let Ok(event) = serde_json::from_str::<JsonValue>(line.trim()) else {
            continue;
        };
        if event.get("type").and_then(JsonValue::as_str) != Some("message_end") {
            continue;
        }
        let Some(message) = event.get("message") else {
            continue;
        };
        if message.get("role").and_then(JsonValue::as_str) != Some("assistant") {
            continue;
        }
        text = message
            .get("content")
            .and_then(JsonValue::as_array)
            .map(|content| {
                content
                    .iter()
                    .filter_map(|block| {
                        (block.get("type").and_then(JsonValue::as_str) == Some("text"))
                            .then(|| block.get("text").and_then(JsonValue::as_str))
                            .flatten()
                    })
                    .collect::<String>()
            })
            .unwrap_or_default();
        if let Some(value) = message.get("usage") {
            usage = ParsedTokenUsage {
                input_tokens: extract_u64(value, "input"),
                cached_input_tokens: extract_u64(value, "cacheRead"),
                output_tokens: extract_u64(value, "output"),
                reasoning_output_tokens: extract_u64(value, "reasoning"),
                total_tokens: extract_u64(value, "totalTokens"),
                total_cost_usd: value
                    .get("cost")
                    .and_then(|cost| extract_f64(cost, "total"))
                    .filter(|cost| *cost > 0.0),
            };
        }
        if let Some(reason) = message.get("stopReason").and_then(JsonValue::as_str) {
            if matches!(reason, "error" | "aborted") {
                error = Some(format!("{label} 对话未完成: {reason}"));
            }
        }
        if let Some(message) = message.get("errorMessage").and_then(JsonValue::as_str) {
            if !message.trim().is_empty() {
                error = Some(message.to_string());
            }
        }
    }
    if usage.total_tokens.is_none() {
        usage.total_tokens = token_sum(&[
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            usage.reasoning_output_tokens,
        ]);
    }
    ParsedLivenessOutput {
        response: text.trim().chars().take(240).collect(),
        error,
        usage,
    }
}
