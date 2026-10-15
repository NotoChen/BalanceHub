//! Read model defaults from the selected Codex installation, without starting a
//! session or refreshing its account catalog over the network.
use super::{NativeSnapshot, Observed};
use crate::{
    models::{
        AgentModelCatalog, AgentModelCatalogContext, AgentSettingChoice, AgentSettingValueState,
    },
    network::EffectiveProxy,
    platform::{paths::compatible_path, process},
    services::agent_cli::{self, environment::mutation::GuardedFile},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const MODEL_KEYS: &[&str] = &[
    "slug",
    "id",
    "display_name",
    "visibility",
    "priority",
    "context_window",
    "max_context_window",
    "auto_compact_token_limit",
    "effective_context_window_percent",
    "default_reasoning_level",
    "supported_reasoning_levels",
    "multi_agent_version",
    "supports_reasoning_summary_parameter",
    "default_reasoning_summary",
    "support_verbosity",
    "default_verbosity",
    "service_tiers",
    "default_service_tier",
];

struct CachedCatalog {
    read_at: Instant,
    models: Option<Arc<Vec<Value>>>,
}

static BUNDLED: OnceLock<Mutex<BTreeMap<String, CachedCatalog>>> = OnceLock::new();
static FILES: OnceLock<Mutex<BTreeMap<String, CachedCatalog>>> = OnceLock::new();

pub(super) struct ModelInfo {
    entry: Value,
    source: String,
}

impl ModelInfo {
    pub(super) fn value(&self, key: &str) -> Option<&Value> {
        self.entry.get(key).filter(|value| !value.is_null())
    }

    pub(super) fn observed(&self, key: &str) -> Option<Observed> {
        self.value(key).map(|value| Observed {
            value: value.clone(),
            source: self.source.clone(),
            managed: false,
            state: AgentSettingValueState::Default,
        })
    }

    pub(super) fn slider_maximum(&self) -> Option<i64> {
        self.value("max_context_window")
            .or_else(|| self.value("context_window"))
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
    }
}

pub(super) fn read(snapshot: &NativeSnapshot, cli_path: &str, model: &str) -> Option<ModelInfo> {
    if model.is_empty() {
        return None;
    }
    catalogs(snapshot, cli_path)
        .into_iter()
        .find_map(|(models, source)| find(&models, model, source))
}

fn model_choices(models: &[Value], source: &str) -> Vec<AgentSettingChoice> {
    let mut choices = Vec::new();
    let mut entries = models.iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        entry
            .get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(i64::MAX)
    });
    for entry in entries {
        if entry.get("visibility").and_then(Value::as_str) == Some("hide") {
            continue;
        }
        let Some(id) = entry
            .get("slug")
            .or_else(|| entry.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        choices.push(AgentSettingChoice {
            value: id.into(),
            label: entry
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .into(),
            description: source.into(),
        });
    }
    choices
}

pub(in crate::services::agent_cli::settings) fn query(
    context: &AgentModelCatalogContext,
    proxy: &EffectiveProxy,
) -> Result<AgentModelCatalog, String> {
    use super::super::model_catalog;
    if let Some(path) = &context.catalog_path {
        let models = read_file(Path::new(path)).ok_or("配置指定的 Codex 模型目录暂不可读")?;
        return Ok(model_catalog::catalog(
            model_choices(&models, "指定模型目录"),
            "配置指定的 Codex 模型目录",
        ));
    }
    let cache_path = Path::new(&context.config_root).join("models_cache.json");
    if context.cli_path.is_empty() {
        let models =
            read_file(&cache_path).ok_or("当前环境没有可读取的 Codex 模型目录；仍可输入模型 ID")?;
        return Ok(model_catalog::catalog(
            model_choices(&models, "原生缓存"),
            "Codex 本地模型目录",
        ));
    }
    let mut command = model_catalog::command(context, proxy);
    command
        .args(["debug", "models"])
        .env("CODEX_HOME", &context.config_root);
    let queried = process::run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(10),
        MAX_CATALOG_BYTES,
    )
    .ok()
    .filter(|output| {
        !output.timed_out
            && !output.stdout_truncated
            && output.status.is_some_and(|status| status.success())
    })
    .and_then(|output| parse(output.stdout.as_bytes()));
    if let Some(models) = queried {
        return Ok(model_catalog::catalog(
            model_choices(&models, "CLI 模型目录"),
            "Codex CLI 模型目录",
        ));
    }
    let models = read_file(&cache_path)
        .or_else(|| bundled(&context.cli_path))
        .ok_or("Codex 模型目录刷新失败；仍可输入模型 ID")?;
    let mut result =
        model_catalog::catalog(model_choices(&models, "本地模型目录"), "Codex 本地模型目录");
    result.stale = true;
    result.notice = "暂时无法刷新 Codex 模型，正在显示本地目录".into();
    Ok(result)
}

type Catalogs = Vec<(Arc<Vec<Value>>, String)>;

fn catalogs(snapshot: &NativeSnapshot, cli_path: &str) -> Catalogs {
    if let Some(configured) = snapshot.get("/model_catalog_json") {
        return configured_catalog(snapshot, &configured)
            .into_iter()
            .collect();
    }
    let mut catalogs = Vec::new();
    let cache_path = snapshot.root.join("models_cache.json");
    if let Some(models) = read_file(&cache_path) {
        catalogs.push((models, source_path(&cache_path)));
    }
    if let Some(models) = bundled(cli_path) {
        catalogs.push((models, "所选 Codex CLI 的内置模型目录".into()));
    }
    catalogs
}

fn configured_catalog(
    snapshot: &NativeSnapshot,
    configured: &Observed,
) -> Option<(Arc<Vec<Value>>, String)> {
    let path = configured_path(snapshot, configured)?;
    read_file(&path).map(|models| (models, source_path(&path)))
}

pub(in crate::services::agent_cli::settings) fn configured_path(
    snapshot: &NativeSnapshot,
    configured: &Observed,
) -> Option<PathBuf> {
    let configured_path = configured.value.as_str()?;
    let path = PathBuf::from(configured_path);
    let guest = crate::services::wsl::split_host_path(&snapshot.root);
    let path = if let Some((distro, _)) = guest.filter(|_| configured_path.starts_with('/')) {
        crate::services::wsl::host_path(&distro, configured_path).ok()?
    } else if path.is_absolute() && snapshot.include_process_environment {
        path
    } else {
        Path::new(&configured.source)
            .parent()
            .unwrap_or(&snapshot.root)
            .join(path)
    };
    if !snapshot.include_process_environment
        && crate::services::wsl::split_host_path(&path).is_none()
    {
        return None;
    }
    // An explicit catalog replaces the native catalog. Do not fill a missing
    // model with unrelated account or bundled metadata.
    Some(path)
}

fn find(models: &[Value], model: &str, source: String) -> Option<ModelInfo> {
    models
        .iter()
        .find(|entry| {
            entry
                .get("slug")
                .or_else(|| entry.get("id"))
                .and_then(Value::as_str)
                == Some(model)
        })
        .map(|entry| ModelInfo {
            entry: entry.clone(),
            source,
        })
}

fn source_path(path: &Path) -> String {
    compatible_path(path).to_string_lossy().into_owned()
}

fn parse(bytes: &[u8]) -> Option<Vec<Value>> {
    let mut catalog: Value = serde_json::from_slice(bytes).ok()?;
    let models = catalog.get_mut("models")?.as_array_mut()?;
    for model in models.iter_mut() {
        if let Some(fields) = model.as_object_mut() {
            // Keep only settings metadata, never the bundled system prompts.
            fields.retain(|key, _| MODEL_KEYS.contains(&key.as_str()));
        }
    }
    Some(std::mem::take(models))
}

fn read_file(path: &Path) -> Option<Arc<Vec<Value>>> {
    let metadata = std::fs::metadata(path).ok()?;
    let key = format!(
        "{}:{:?}:{}:{:?}",
        source_path(&std::fs::canonicalize(path).ok()?),
        metadata.modified().ok(),
        metadata.len(),
        metadata.permissions()
    );
    let mut cache = FILES.get_or_init(Mutex::default).lock().ok()?;
    if let Some(cached) = cache.get(&key) {
        if cached.models.is_some() || cached.read_at.elapsed() < Duration::from_secs(60) {
            return cached.models.clone();
        }
    }
    let models = GuardedFile::capture_path(path.parent()?, path, MAX_CATALOG_BYTES)
        .ok()
        .and_then(|file| file.bytes().and_then(parse))
        .map(Arc::new);
    if cache.len() >= 24 {
        cache.clear();
    }
    cache.insert(
        key,
        CachedCatalog {
            read_at: Instant::now(),
            models: models.clone(),
        },
    );
    models
}

fn bundled(cli_path: &str) -> Option<Arc<Vec<Value>>> {
    let key = super::super::help::executable_key(cli_path).ok()?;
    let mut cache = BUNDLED.get_or_init(Mutex::default).lock().ok()?;
    if let Some(cached) = cache.get(&key) {
        let lifetime = if cached.models.is_some() { 300 } else { 60 };
        if cached.read_at.elapsed() < Duration::from_secs(lifetime) {
            return cached.models.clone();
        }
    }
    // Serialize this small offline inspection so rapid model changes share one
    // read. A transient failure is retried later; it never blocks default launch.
    let mut command = Command::new(compatible_path(Path::new(cli_path)).as_ref());
    command.args(["debug", "models", "--bundled"]);
    if let Some(path) = agent_cli::runtime_path_for(Path::new(cli_path)) {
        command.env("PATH", path);
    }
    let models = process::run_command_with_output_timeout(
        &mut command,
        Duration::from_secs(2),
        MAX_CATALOG_BYTES,
    )
    .ok()
    .filter(|output| {
        !output.timed_out
            && !output.stdout_truncated
            && output.status.is_some_and(|status| status.success())
    })
    .and_then(|output| parse(output.stdout.as_bytes()))
    .map(Arc::new);
    if cache.len() >= 24 {
        cache.clear();
    }
    cache.insert(
        key,
        CachedCatalog {
            read_at: Instant::now(),
            models: models.clone(),
        },
    );
    models
}
