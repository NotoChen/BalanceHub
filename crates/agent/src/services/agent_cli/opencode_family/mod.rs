//! Shared configuration surface for OpenCode-shaped CLIs (OpenCode forks that
//! keep `provider.<id>` in a single JSON/JSONC file and select `model` as
//! `<provider>/<model>`). OpenCode itself keeps its own richer adapter for
//! `OPENCODE_CONFIG` and project-level precedence.

pub(super) mod sessions;

use super::config_support::{
    cli_config_target, config_error, latest_modified_at, match_provider_key, normalize_endpoint,
    oauth_snapshot, parse_jsonc_document, read_stable_optional, relay_snapshot,
    rewrite_json_document, CliConfigTarget,
};
use super::configuration::contracts::ProviderConfigurationCandidate;
use super::contracts::{ParsedLivenessOutput, ParsedTokenUsage};
use super::integration::validate_native_value;
use super::liveness_support::{add_optional, extract_u64, token_sum};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::cli_paths::user_home,
};
use serde_json::{Map, Value as JsonValue};
use std::path::{Path, PathBuf};

const PROVIDER_ID: &str = "balancehub";

pub(super) struct FamilySpec {
    pub(super) label: &'static str,
    pub(super) dir_for_home: fn(&Path) -> PathBuf,
    pub(super) file_in_dir: fn(&Path) -> PathBuf,
    pub(super) configuration_label: &'static str,
    pub(super) configuration_note: &'static str,
}

pub(super) fn config_dir_for(spec: &FamilySpec, home: &Path) -> PathBuf {
    (spec.dir_for_home)(home)
}

pub(super) fn config_path_for(spec: &FamilySpec, root: &Path) -> PathBuf {
    (spec.file_in_dir)(root)
}

fn config_file(spec: &FamilySpec) -> Option<PathBuf> {
    Some(config_path_for(spec, &config_dir_for(spec, &user_home()?)))
}

pub(super) fn snapshot(
    spec: &FamilySpec,
    cli_kind: AgentCliKind,
    providers: &[Provider],
) -> CliConfigSnapshot {
    let Some(path) = config_file(spec) else {
        return config_error(cli_kind, "无法定位用户目录");
    };
    let file = match read_stable_optional(&path) {
        Ok(value) => value,
        Err(_) => return config_error(cli_kind, &format!("读取 {} 配置失败", spec.label)),
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
    let path = config_file(spec).ok_or_else(|| "无法定位用户目录".to_string())?;
    let before = read_stable_optional(&path)?.map(|file| file.text);
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
            let after = merge_config(
                spec,
                before.as_deref().unwrap_or_default(),
                &provider.identity.name,
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
        CliConfigTarget::OAuth { .. } => Err(format!("{} 不支持 OAuth 账号切换", spec.label)),
    }
}

fn parse_config(text: &str) -> Result<Option<(String, String)>, ()> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = parse_jsonc_document(text).map_err(|_| ())?;
    let Some((provider_id, _)) = value
        .get("model")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|model| model.split_once('/'))
    else {
        return Ok(None);
    };
    if provider_id != PROVIDER_ID {
        return Ok(None);
    }
    let options = value
        .get("provider")
        .and_then(|providers| providers.get(provider_id))
        .and_then(|provider| provider.get("options"))
        .and_then(JsonValue::as_object);
    let base_url = options
        .and_then(|options| options.get("baseURL"))
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let api_key = options
        .and_then(|options| options.get("apiKey"))
        .and_then(JsonValue::as_str)
        .and_then(literal_or_environment);
    let Some(base_url) = base_url else {
        return Ok(None);
    };
    if normalize_endpoint(base_url).is_none() {
        return Err(());
    }
    Ok(api_key.map(|api_key| (base_url.to_string(), api_key)))
}

/// The fork expands `{env:NAME}` in option strings, as OpenCode does.
fn literal_or_environment(value: &str) -> Option<String> {
    let value = value.trim();
    let name = value
        .strip_prefix("{env:")
        .and_then(|value| value.strip_suffix('}'))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let resolved = match name {
        Some(name) => std::env::var(name).ok(),
        None => Some(value.to_string()),
    }?;
    let resolved = resolved.trim();
    if resolved.is_empty() || resolved.chars().any(char::is_control) {
        return None;
    }
    Some(resolved.to_string())
}

pub(super) fn merge_config(
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
            .ok_or_else(|| format!("{} 配置根节点不是对象", spec.label))?
    };
    {
        let providers = object_member(&mut root, "provider", spec)?;
        let entry = providers
            .entry(PROVIDER_ID.to_string())
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| format!("{} 配置中的 provider.balancehub 必须是对象", spec.label))?;
        entry.insert(
            "npm".into(),
            JsonValue::String("@ai-sdk/openai-compatible".into()),
        );
        let display_name = provider_name.trim();
        entry.insert(
            "name".into(),
            JsonValue::String(if display_name.is_empty() {
                "BalanceHub".into()
            } else {
                format!("BalanceHub · {display_name}")
            }),
        );
        let options = object_member(entry, "options", spec)?;
        options.insert("baseURL".into(), JsonValue::String(base_url.to_string()));
        options.insert("apiKey".into(), JsonValue::String(api_key.to_string()));
        let models = object_member(entry, "models", spec)?;
        models
            .entry(model.to_string())
            .or_insert_with(|| serde_json::json!({ "name": model }));
    }
    root.insert(
        "model".into(),
        JsonValue::String(format!("{PROVIDER_ID}/{model}")),
    );
    let desired = JsonValue::Object(root);
    if source.trim().is_empty() {
        serde_json::to_string_pretty(&desired)
            .map_err(|err| format!("生成 {} 配置失败: {err}", spec.label))
    } else {
        rewrite_json_document(source, &desired, true)
            .map_err(|err| format!("生成 {} 配置失败: {err}", spec.label))
    }
}

fn object_member<'a>(
    root: &'a mut Map<String, JsonValue>,
    key: &str,
    spec: &FamilySpec,
) -> Result<&'a mut Map<String, JsonValue>, String> {
    root.entry(key.to_string())
        .or_insert_with(|| JsonValue::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("{} 配置中的 {key} 必须是对象", spec.label))
}

/// The `run --format json` event stream shared by OpenCode-shaped CLIs:
/// assistant text arrives on `text` events and per-step token usage on
/// `step_finish`; transport failures surface as `error` events.
pub(super) fn parse_run_events(
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
        match event.get("type").and_then(JsonValue::as_str) {
            Some("text") => {
                if let Some(chunk) = event
                    .get("part")
                    .and_then(|part| part.get("text"))
                    .and_then(JsonValue::as_str)
                {
                    text.push_str(chunk);
                }
            }
            Some("step_finish") => {
                if let Some(tokens) = event.get("part").and_then(|part| part.get("tokens")) {
                    add_usage(&mut usage, tokens);
                }
                if usage.total_cost_usd.is_none() {
                    usage.total_cost_usd = event
                        .get("part")
                        .and_then(|part| part.get("cost"))
                        .and_then(JsonValue::as_f64)
                        .filter(|cost| *cost > 0.0);
                }
            }
            Some("error") => {
                error = event
                    .get("error")
                    .and_then(|error| {
                        error
                            .get("data")
                            .and_then(|data| data.get("message"))
                            .and_then(JsonValue::as_str)
                            .or_else(|| error.get("message").and_then(JsonValue::as_str))
                    })
                    .map(str::to_owned)
                    .or_else(|| Some(format!("{label} 测活执行失败")));
            }
            _ => {}
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

fn add_usage(usage: &mut ParsedTokenUsage, tokens: &JsonValue) {
    add_optional(&mut usage.input_tokens, extract_u64(tokens, "input"));
    add_optional(
        &mut usage.cached_input_tokens,
        tokens
            .get("cache")
            .and_then(|cache| extract_u64(cache, "read")),
    );
    add_optional(&mut usage.output_tokens, extract_u64(tokens, "output"));
    add_optional(
        &mut usage.reasoning_output_tokens,
        extract_u64(tokens, "reasoning"),
    );
    add_optional(&mut usage.total_tokens, extract_u64(tokens, "total"));
}
