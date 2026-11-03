//! Model names belong to the installed CLI, not to an App release. These
//! bounded queries are separate from configuration parsing and asset scans.
mod claude;
mod gemini;
mod grok;

use crate::{
    models::{
        AgentCliKind, AgentModelCatalog, AgentModelCatalogContext, AgentSettingChoice, AppSettings,
    },
    network::{self, EffectiveProxy},
    platform::{paths::compatible_path, process},
    services::agent_cli::{self, cache},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const FRESH_FOR: Duration = Duration::from_secs(300);
const RETRY_AFTER: Duration = Duration::from_secs(60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: usize = 2 * 1024 * 1024;

#[derive(Default)]
struct Entry {
    fingerprint: String,
    checked_at: Option<Instant>,
    catalog: Option<AgentModelCatalog>,
}

type Entries = BTreeMap<String, Arc<Mutex<Entry>>>;
static CATALOGS: OnceLock<Mutex<Entries>> = OnceLock::new();

pub(crate) fn read(
    context: &AgentModelCatalogContext,
    settings: &AppSettings,
    refresh: bool,
) -> Result<AgentModelCatalog, String> {
    if context.config_root.is_empty()
        || !Path::new(&context.config_root).is_absolute()
        || context.cli_path.contains('\0')
    {
        return Err("模型目录的运行环境无效".into());
    }
    let proxy = network::resolve_global_proxy(settings);
    let key = serde_json::to_vec(context).map_err(|_| "无法识别模型目录")?;
    let key = cache::key(&key);
    let mut entries = CATALOGS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "模型目录缓存不可用")?;
    if entries.len() >= 32 && !entries.contains_key(&key) {
        // A pending reader retains its Arc and completes normally.
        entries.retain(|_, entry| Arc::strong_count(entry) > 1);
    }
    let entry = entries.entry(key).or_default().clone();
    drop(entries);
    let requested_at = Instant::now();
    let mut entry = entry.lock().map_err(|_| "模型目录缓存不可用")?;
    let current_fingerprint = fingerprint(context, &proxy);
    if entry.fingerprint == current_fingerprint {
        if let (Some(catalog), Some(checked_at)) = (&entry.catalog, entry.checked_at) {
            let lifetime = if catalog.stale {
                RETRY_AFTER
            } else {
                FRESH_FOR
            };
            // Concurrent refresh clicks share the query that just completed.
            if checked_at >= requested_at || (!refresh && checked_at.elapsed() < lifetime) {
                return Ok(catalog.clone());
            }
        }
    } else {
        // An account, configuration root or installation change must not reuse
        // a previous environment's model choices after a failed refresh.
        entry.catalog = None;
    }
    let queried = match context.cli_kind {
        AgentCliKind::Codex => super::codex::query_model_catalog(context, &proxy),
        _ if context.cli_path.is_empty() => {
            Err("当前环境未提供可查询的 CLI，可使用已有配置或输入模型 ID".into())
        }
        AgentCliKind::ClaudeCode => claude::query(context, &proxy),
        AgentCliKind::Gemini => gemini::query(context, &proxy),
        AgentCliKind::Grok => grok::query(context, &proxy),
    };
    let catalog = match queried {
        Ok(mut catalog) => {
            normalize(&mut catalog.choices);
            catalog
        }
        Err(notice) => {
            let mut catalog = entry.catalog.clone().unwrap_or(AgentModelCatalog {
                choices: Vec::new(),
                source: String::new(),
                notice: String::new(),
                stale: true,
                refreshed_at: None,
            });
            catalog.notice = if catalog.choices.is_empty() {
                notice
            } else {
                format!("{notice}；已保留上次读取的模型")
            };
            catalog.stale = true;
            catalog
        }
    };
    // Native metadata commands may update their own model cache. Observe after
    // the read as well, otherwise every open would invalidate our own result.
    entry.fingerprint = fingerprint(context, &proxy);
    entry.checked_at = Some(Instant::now());
    entry.catalog = Some(catalog.clone());
    Ok(catalog)
}

fn fingerprint(context: &AgentModelCatalogContext, proxy: &EffectiveProxy) -> String {
    let mut digest = Sha256::new();
    cache::observe_environment(&mut digest, context.cli_kind);
    cache::observe_executable(&mut digest, Path::new(&context.cli_path));
    let root = Path::new(&context.config_root);
    for name in [
        "config.toml",
        "settings.json",
        "auth.json",
        ".credentials.json",
        "credentials.json",
        "models_cache.json",
    ] {
        cache::observe_path(&mut digest, &root.join(name));
    }
    if context.cli_kind == AgentCliKind::ClaudeCode {
        cache::observe_path(&mut digest, &root.with_extension("json"));
    }
    if let Some(path) = &context.catalog_path {
        cache::observe_path(&mut digest, Path::new(path));
    }
    if let Some(parent) = Path::new(&context.cli_path).parent() {
        let package = parent.join("node_modules").join(
            agent_cli::definition(context.cli_kind)
                .environment()
                .package_name(),
        );
        cache::observe_path(&mut digest, &package.join("package.json"));
        cache::observe_path(&mut digest, &package);
    }
    digest.update(proxy.fingerprint());
    format!("{:x}", digest.finalize())
}

pub(super) fn command(context: &AgentModelCatalogContext, proxy: &EffectiveProxy) -> Command {
    let path = Path::new(&context.cli_path);
    let mut command = Command::new(compatible_path(path).as_ref());
    if let Some(path) = agent_cli::runtime_path_for(path) {
        command.env("PATH", path);
    }
    if Path::new(&context.config_root).is_dir() {
        command.current_dir(compatible_path(Path::new(&context.config_root)).as_ref());
    }
    network::apply_proxy_env(&mut command, proxy);
    command.env("NO_COLOR", "1");
    command
}

pub(super) fn output(command: &mut Command) -> Result<String, String> {
    let output = process::run_command_with_output_timeout(command, QUERY_TIMEOUT, MAX_OUTPUT)
        .map_err(|_| "无法运行所选 CLI 的模型查询")?;
    if output.timed_out {
        return Err("模型查询超时，可稍后刷新".into());
    }
    if output.stdout_truncated {
        return Err("CLI 模型目录过大，未使用不完整结果".into());
    }
    if !output.status.is_some_and(|status| status.success()) {
        // Native stderr can contain account details or credentials. Keep only
        // fixed diagnostics in IPC; no process output is logged or persisted.
        return Err("CLI 未能返回模型目录，请检查原生 CLI 的登录和网络状态".into());
    }
    Ok(output.stdout)
}

pub(in crate::services::agent_cli::settings) fn catalog(
    choices: Vec<AgentSettingChoice>,
    source: &str,
) -> AgentModelCatalog {
    AgentModelCatalog {
        choices,
        source: source.into(),
        notice: String::new(),
        stale: false,
        refreshed_at: Some(chrono::Utc::now().timestamp_millis()),
    }
}

pub(super) fn normalize(choices: &mut Vec<AgentSettingChoice>) {
    let mut seen = BTreeSet::new();
    choices.retain_mut(|choice| {
        choice.value = choice.value.trim().to_owned();
        if choice.value.is_empty()
            || choice.value.len() > 512
            || choice.value.chars().any(char::is_control)
            || !seen.insert(choice.value.clone())
        {
            return false;
        }
        if choice.label.is_empty() {
            choice.label = choice.value.clone();
        }
        choice.label = choice
            .label
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .collect();
        choice.description = choice
            .description
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        true
    });
}
