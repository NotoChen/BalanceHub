//! Agent CLI registry and behavior boundary.
//!
//! The registry owns metadata, discovery and optional capability adapters. Generic orchestration
//! never matches concrete Agent kinds; a new built-in Agent adds one registry entry and its own
//! adapter modules without editing launch, liveness, session or config orchestration.

pub(crate) mod cache;
pub(crate) mod catalog;
pub(crate) mod config_support;
pub(crate) mod configuration;
pub(crate) mod contracts;
mod discovery;
pub(crate) mod environment;
pub(crate) mod integration;
pub(crate) mod lifecycle;
mod liveness_support;
pub(crate) mod maintenance;
pub(crate) mod mcp_check;
mod opencode_family;
pub(crate) mod overview;
mod pi_family;
pub(crate) mod settings;

use crate::models::{
    AgentCliCapabilities, AgentCliDescriptor, AgentCliKind, AppSettings, CliEnvironmentProbeResult,
    CliToolProbeResult, Provider,
};
use contracts::{
    DefaultConfigAdapter, EndpointAdapter, EnvironmentAdapter, LivenessAdapter, SessionAdapter,
    TemporaryLaunchAdapter,
};
use discovery::paths::{AgentHomeCandidateScanRequest, AgentHomeCandidateScanResult};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) struct AgentCliDefinition {
    pub kind: AgentCliKind,
    pub label: &'static str,
    pub executable: &'static str,
    pub session_name_hint: &'static str,
    pub additional_env_keys: &'static [&'static str],
    pub(in crate::services::agent_cli) home_scan:
        fn(AgentHomeCandidateScanRequest) -> AgentHomeCandidateScanResult,
    pub invalid_path_reason: Option<fn(&Path) -> Option<&'static str>>,
    pub require_version_substring: Option<&'static str>,
    endpoint: EndpointAdapter,
    temporary_launch: Option<TemporaryLaunchAdapter>,
    sessions: Option<SessionAdapter>,
    liveness: Option<LivenessAdapter>,
    default_config: Option<DefaultConfigAdapter>,
    environment: EnvironmentAdapter,
    configuration: configuration::contracts::AgentConfigurationAdapter,
}

impl AgentCliDefinition {
    pub(crate) fn capabilities(&self) -> AgentCliCapabilities {
        AgentCliCapabilities {
            temporary_launch: self.temporary_launch.is_some(),
            model_selection: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_model_selection()),
            session_history: self.sessions.is_some(),
            session_search: self
                .sessions
                .is_some_and(|adapter| adapter.supports_search()),
            session_detail: self
                .sessions
                .is_some_and(|adapter| adapter.supports_detail()),
            session_resume: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_session_resume()),
            session_name: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_session_name()),
            liveness: self.liveness.is_some(),
            default_config: self.default_config.is_some(),
            asset_inventory: self.environment.asset_inventory(),
            hook_inventory: self.environment.hook_adapter().is_some(),
            runtime_observation: self.temporary_launch.is_some()
                || self.environment.hook_adapter().is_some(),
        }
    }

    pub(crate) fn temporary_launch(&self) -> Option<&TemporaryLaunchAdapter> {
        self.temporary_launch.as_ref()
    }

    pub(crate) fn sessions(&self) -> Option<&SessionAdapter> {
        self.sessions.as_ref()
    }

    pub(crate) fn liveness(&self) -> Option<&LivenessAdapter> {
        self.liveness.as_ref()
    }

    pub(crate) fn default_config(&self) -> Option<&DefaultConfigAdapter> {
        self.default_config.as_ref()
    }

    pub(crate) fn configuration(&self) -> &configuration::contracts::AgentConfigurationAdapter {
        &self.configuration
    }

    pub(crate) fn environment(&self) -> &EnvironmentAdapter {
        &self.environment
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AgentCliExecutable {
    pub(crate) path: String,
    pub(crate) version: String,
}

macro_rules! register_agent_clis {
    (
        $(
            $variant:ident => { key: $key:literal, module: $module:ident }
        ),+
        $(,)?
    ) => {
        $(mod $module;)+
        mod native_agent_kinds {
            $(pub(super) mod $module {
                pub(in crate::services::agent_cli) const AGENT_KIND: crate::models::AgentCliKind =
                    crate::models::AgentCliKind::$variant;
            })+
        }
        const DEFINITIONS: &[AgentCliDefinition] = &[
            $($module::definition(native_agent_kinds::$module::AGENT_KIND)),+
        ];
        #[cfg(test)]
        const AGENT_MODULE_NAMES: &[&str] = &[$(stringify!($module)),+];
    };
}

// 枚举身份、模块声明和注册表由同一份目录生成。
crate::agent_cli_catalog::for_each_agent_cli!(register_agent_clis);

pub(crate) use droid::hook_config_dir_for_home as droid_hook_config_dir_for_home;
pub(crate) use gemini::merge_temporary_system_settings as merge_gemini_temporary_settings;
pub(crate) use kimi::hook_config_dir_for_home as kimi_hook_config_dir_for_home;

pub(crate) fn definitions() -> &'static [AgentCliDefinition] {
    debug_assert!(AgentCliKind::ALL.iter().all(|kind| {
        DEFINITIONS
            .iter()
            .filter(|definition| definition.kind == *kind)
            .count()
            == 1
    }));
    DEFINITIONS
}

pub(crate) fn definition(kind: AgentCliKind) -> &'static AgentCliDefinition {
    DEFINITIONS
        .iter()
        .find(|definition| definition.kind == kind)
        .expect("every AgentCliKind must have a registered definition")
}

fn descriptor(definition: &AgentCliDefinition) -> AgentCliDescriptor {
    AgentCliDescriptor {
        kind: definition.kind,
        label: definition.label.to_string(),
        executable: definition.executable.to_string(),
        session_name_hint: definition.session_name_hint.to_string(),
        capabilities: definition.capabilities(),
    }
}

/// 公共运行面板使用的 Agent 目录。顺序与注册表保持一致，新增 Agent
/// 只需要加入目录定义，不需要再维护一份汇总顺序。
pub(crate) fn descriptors() -> Vec<AgentCliDescriptor> {
    DEFINITIONS.iter().map(descriptor).collect()
}

pub(crate) fn find(
    settings: &AppSettings,
    kind: AgentCliKind,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    find_with_preferred(kind, settings.agent_cli_path(kind), include_shell)
}

/// 校验调用方已经选定的 CLI 入口。临时 CLI 预览/启动不应在每次调用时
/// 重新遍历版本管理器、PATH 和登录 Shell 的全部候选，否则 GUI 会被探测
/// 链路拖住；候选发现仍统一由 `find` 负责。
pub(crate) fn find_at_path(kind: AgentCliKind, path: &str) -> Result<AgentCliExecutable, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(format!("{} CLI 路径为空", definition(kind).label));
    }
    discovery::find_cli_at_path(std::path::Path::new(path), definition(kind))
}

pub(in crate::services::agent_cli) fn discover_installations(
    preferred_path: &str,
    definition: &AgentCliDefinition,
    include_shell: bool,
    run: &mut environment::run::AgentInventoryRun,
) -> Vec<crate::models::AgentInstallation> {
    discovery::discover_installations(preferred_path, definition, include_shell, run)
}

fn find_with_preferred(
    kind: AgentCliKind,
    preferred_path: &str,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    discovery::find_cli(preferred_path, definition(kind), include_shell)
}

pub(crate) fn probe_all(settings: &AppSettings, include_shell: bool) -> CliEnvironmentProbeResult {
    let tools = std::thread::scope(|scope| {
        let handles = DEFINITIONS
            .iter()
            .map(|definition| {
                let preferred_path = settings.agent_cli_path(definition.kind).to_string();
                scope.spawn(move || {
                    let result = discovery::find_cli(&preferred_path, definition, include_shell);
                    probe_result(definition, result)
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .zip(DEFINITIONS.iter())
            .map(|(handle, definition)| {
                handle.join().unwrap_or_else(|_| {
                    probe_result(
                        definition,
                        Err(format!("{} CLI 自动检测异常", definition.label)),
                    )
                })
            })
            .collect()
    });

    CliEnvironmentProbeResult { tools }
}

pub(crate) fn initialize_discovery_cache(root: std::path::PathBuf) {
    environment::releases::initialize(root.clone());
    discovery::initialize_cache(root);
}

pub(crate) fn runtime_path_for(cli_path: &Path) -> Option<std::ffi::OsString> {
    discovery::runtime_path_for(cli_path)
}

pub(crate) fn provider_base_url(kind: AgentCliKind, provider: &Provider) -> String {
    normalize_base_url(kind, provider_raw_base_url(kind, provider))
}

pub fn normalize_base_url(kind: AgentCliKind, base_url: &str) -> String {
    definition(kind).endpoint.normalize_base_url(base_url)
}

pub fn provider_raw_base_url(kind: AgentCliKind, provider: &Provider) -> &str {
    let override_url = provider
        .liveness
        .agent_base_urls
        .get(&kind)
        .map(String::as_str)
        .unwrap_or_default()
        .trim();
    if override_url.is_empty() {
        provider.identity.base_url.trim()
    } else {
        override_url
    }
}

fn probe_result(
    definition: &AgentCliDefinition,
    result: Result<AgentCliExecutable, String>,
) -> CliToolProbeResult {
    match result {
        Ok(result) => CliToolProbeResult {
            descriptor: descriptor(definition),
            available: true,
            path: result.path,
            version: result.version,
            message: String::new(),
        },
        Err(message) => CliToolProbeResult {
            descriptor: descriptor(definition),
            available: false,
            path: String::new(),
            version: String::new(),
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_exactly_one_definition() {
        assert_eq!(definitions().len(), AgentCliKind::ALL.len());
        let mut registered = std::collections::BTreeSet::new();
        for registered_definition in definitions() {
            assert!(
                registered.insert(registered_definition.kind),
                "duplicate Agent CLI registration: {}",
                registered_definition.kind.key()
            );
        }
        for &kind in AgentCliKind::ALL {
            assert!(registered.contains(&kind));
            assert_eq!(definition(kind).kind, kind);
        }
    }

    #[test]
    fn agent_modules_receive_their_kind_from_the_catalog() {
        fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(directory)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", directory.display()));
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_rust_sources(&path, sources);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    sources.push(path);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/agent_cli");
        for module in AGENT_MODULE_NAMES {
            let directory = root.join(module);
            let mut sources = Vec::new();
            collect_rust_sources(&directory, &mut sources);
            assert!(!sources.is_empty(), "missing Agent CLI module: {module}");
            for path in sources {
                if path.components().any(|component| {
                    component.as_os_str() == "tests.rs" || component.as_os_str() == "tests"
                }) {
                    continue;
                }
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
                let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
                assert!(
                    !production.contains("AgentCliKind::"),
                    "{} must receive its Agent kind from the catalog instead of hard-coding it",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn asset_mechanisms_receive_their_kind_from_the_catalog() {
        let mut record_count = 0;
        for registered in definitions() {
            let records = registered.environment().mechanism_records(registered.kind);
            record_count += records.len();
            assert!(records
                .iter()
                .all(|record| record.agent_kind == registered.kind));
        }
        assert!(record_count > 0);
    }

    #[test]
    fn balancehub_cli_path_environment_key_is_not_duplicated_per_agent() {
        for registered_definition in definitions() {
            assert!(registered_definition
                .additional_env_keys
                .iter()
                .all(|key| !key.starts_with("BALANCEHUB_")));
        }
    }

    #[test]
    fn capability_matrix_matches_snapshot() {
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&descriptors())
                .expect("capability matrix should serialize")
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/services/agent_cli/capabilities.snapshot.json");
        if std::env::var_os("UPDATE_AGENT_CAPABILITIES").is_some() {
            std::fs::write(&path, &rendered).expect("capability snapshot should be writable");
            return;
        }
        let expected = std::fs::read_to_string(&path).expect("capability snapshot should exist");
        assert_eq!(
            rendered, expected,
            "Agent 能力矩阵已变化；确认变更后运行 UPDATE_AGENT_CAPABILITIES=1 \
             npm run cargo -- test -p balancehub-agent --lib capability_matrix_matches_snapshot \
             更新快照，再运行 npm run docs:capabilities 更新文档"
        );
    }

    #[test]
    fn capabilities_are_derived_from_registered_adapters() {
        for registered_definition in definitions() {
            let capabilities = registered_definition.capabilities();
            assert_eq!(
                capabilities.temporary_launch,
                registered_definition.temporary_launch.is_some()
            );
            assert_eq!(
                capabilities.model_selection,
                registered_definition
                    .temporary_launch
                    .is_some_and(|adapter| adapter.supports_model_selection())
            );
            assert_eq!(
                capabilities.session_history,
                registered_definition.sessions.is_some()
            );
            assert_eq!(
                capabilities.session_search,
                registered_definition
                    .sessions
                    .is_some_and(|adapter| adapter.supports_search())
            );
            assert_eq!(
                capabilities.session_detail,
                registered_definition
                    .sessions
                    .is_some_and(|adapter| adapter.supports_detail())
            );
            assert_eq!(
                capabilities.session_resume,
                registered_definition
                    .temporary_launch
                    .is_some_and(|adapter| adapter.supports_session_resume())
            );
            assert_eq!(
                capabilities.session_name,
                registered_definition
                    .temporary_launch
                    .is_some_and(|adapter| adapter.supports_session_name())
            );
            assert_eq!(
                capabilities.liveness,
                registered_definition.liveness.is_some()
            );
            assert_eq!(
                capabilities.default_config,
                registered_definition.default_config.is_some()
            );
            assert_eq!(
                capabilities.asset_inventory,
                registered_definition.environment.asset_inventory()
            );
            assert_eq!(
                capabilities.hook_inventory,
                registered_definition.environment.hook_adapter().is_some()
            );
            assert_eq!(
                capabilities.runtime_observation,
                registered_definition.temporary_launch.is_some()
                    || registered_definition.environment.hook_adapter().is_some()
            );
        }

        assert!(!definition(AgentCliKind::Codex).capabilities().session_name);
        assert!(
            definition(AgentCliKind::ClaudeCode)
                .capabilities()
                .session_name
        );
        // Launch-capable Agents publish a temporary launch adapter; the
        // configuration-only tier deliberately leaves it empty.
        assert!(
            definition(AgentCliKind::OpenCode)
                .capabilities()
                .temporary_launch
        );
        let jcode = definition(AgentCliKind::Jcode).capabilities();
        assert!(jcode.temporary_launch);
        assert!(jcode.session_resume);
        assert!(jcode.liveness);
        assert!(
            definition(AgentCliKind::Gemini)
                .capabilities()
                .default_config
        );
        assert!(!definition(AgentCliKind::Grok).capabilities().session_name);
        assert!(
            definition(AgentCliKind::Grok)
                .capabilities()
                .session_history
        );
        assert!(definition(AgentCliKind::Grok).capabilities().liveness);
        assert!(definition(AgentCliKind::Grok).capabilities().default_config);
        let hermes = definition(AgentCliKind::Hermes).capabilities();
        assert!(hermes.temporary_launch);
        assert!(hermes.session_resume);
        assert!(hermes.liveness);
        assert!(definition(AgentCliKind::Dsh).capabilities().liveness);
        assert!(definition(AgentCliKind::QwenCode).capabilities().liveness);
        assert!(definition(AgentCliKind::OpenCode).capabilities().liveness);
        let pi = definition(AgentCliKind::Pi).capabilities();
        assert!(pi.temporary_launch);
        assert!(pi.session_resume);
        assert!(pi.session_name);
        assert!(pi.liveness);
        let crush = definition(AgentCliKind::Crush).capabilities();
        assert!(crush.temporary_launch);
        assert!(crush.session_resume);
        assert!(crush.liveness);
        let cline = definition(AgentCliKind::Cline).capabilities();
        assert!(cline.temporary_launch);
        assert!(cline.session_resume);
        assert!(cline.liveness);
        assert!(definition(AgentCliKind::CodeBuddy).capabilities().liveness);
        let goose = definition(AgentCliKind::Goose).capabilities();
        assert!(goose.temporary_launch);
        assert!(goose.session_resume);
        assert!(goose.session_name);
        assert!(goose.liveness);
        assert!(definition(AgentCliKind::OpenClaw).capabilities().liveness);
        let droid = definition(AgentCliKind::Droid).capabilities();
        assert!(droid.temporary_launch);
        assert!(droid.session_resume);
        assert!(droid.liveness);
        let copilot = definition(AgentCliKind::Copilot).capabilities();
        assert!(copilot.temporary_launch);
        assert!(copilot.session_resume);
        assert!(copilot.session_name);
        assert!(copilot.liveness);
        assert!(definition(AgentCliKind::KimiCode).capabilities().liveness);
        assert!(definition(AgentCliKind::MiMoCode).capabilities().liveness);
        assert!(
            definition(AgentCliKind::MiniMaxCode)
                .capabilities()
                .liveness
        );
        let step = definition(AgentCliKind::StepCode).capabilities();
        assert!(step.temporary_launch);
        assert!(step.session_resume);
        assert!(step.liveness);
        let prime = definition(AgentCliKind::PrimeAgent).capabilities();
        assert!(prime.temporary_launch);
        assert!(prime.session_resume);
        assert!(prime.liveness);
        let morph = definition(AgentCliKind::Morph).capabilities();
        assert!(morph.temporary_launch);
        assert!(morph.liveness);
        assert!(definition(AgentCliKind::MothX).capabilities().liveness);
        let omp = definition(AgentCliKind::Omp).capabilities();
        assert!(omp.temporary_launch);
        assert!(omp.session_resume);
        assert!(!omp.session_name);
        assert!(omp.liveness);
        let omo = definition(AgentCliKind::Omo).capabilities();
        assert!(omo.temporary_launch);
        assert!(omo.session_resume);
        assert!(omo.session_name);
        assert!(omo.liveness);
        let atomcode = definition(AgentCliKind::AtomCode).capabilities();
        assert!(atomcode.temporary_launch);
        assert!(atomcode.session_resume);
        assert!(atomcode.liveness);
        let deveco = definition(AgentCliKind::DevEco).capabilities();
        assert!(deveco.temporary_launch);
        assert!(deveco.session_resume);
        assert!(deveco.liveness);
        let mimo = definition(AgentCliKind::MiMoCode).capabilities();
        assert!(mimo.temporary_launch);
        assert!(mimo.session_resume);
        assert!(mimo.liveness);
        let kimi = definition(AgentCliKind::KimiCode).capabilities();
        assert!(kimi.temporary_launch);
        assert!(kimi.session_resume);
        assert!(kimi.liveness);
        let dsh = definition(AgentCliKind::Dsh).capabilities();
        assert!(dsh.temporary_launch);
        assert!(dsh.session_resume);
        assert!(dsh.liveness);
        let codebuddy = definition(AgentCliKind::CodeBuddy).capabilities();
        assert!(codebuddy.temporary_launch);
        assert!(codebuddy.session_resume);
        assert!(codebuddy.liveness);
        let openclaw = definition(AgentCliKind::OpenClaw).capabilities();
        assert!(openclaw.temporary_launch);
        assert!(openclaw.session_resume);
        assert!(openclaw.liveness);
    }

    #[test]
    fn generic_orchestration_does_not_branch_on_builtin_agents() {
        let sources = [
            ("temporary_cli.rs", include_str!("temporary_cli.rs")),
            (
                "temporary_cli/shell_runtime/script.rs",
                include_str!("temporary_cli/shell_runtime/script.rs"),
            ),
            ("liveness.rs", include_str!("liveness.rs")),
            ("cli_sessions/mod.rs", include_str!("cli_sessions/mod.rs")),
            (
                "cli_runtime/config.rs",
                include_str!("cli_runtime/config.rs"),
            ),
        ];
        for (path, source) in sources {
            assert!(
                !source.contains("AgentCliKind::"),
                "{path} must dispatch through the Agent CLI registry instead of naming a concrete Agent"
            );
        }
    }

    #[test]
    fn probe_contract_flattens_registry_metadata_for_the_frontend() {
        let result = probe_result(
            definition(AgentCliKind::ClaudeCode),
            Err("not installed".to_string()),
        );
        let value = serde_json::to_value(result).expect("probe result should serialize");

        assert_eq!(value["kind"], "claudeCode");
        assert_eq!(value["label"], "Claude Code");
        assert_eq!(value["executable"], "claude");
        assert_eq!(value["sessionNameHint"], "");
        assert_eq!(value["capabilities"]["sessionName"], true);
        assert_eq!(value["available"], false);
    }

    #[test]
    fn api_dialects_drive_provider_endpoint_normalization() {
        let mut provider = Provider::from_input(
            crate::models::ProviderInput::default(),
            "provider-test".to_string(),
        );
        provider.identity.base_url = "https://relay.example.com/root/".to_string();

        assert_eq!(
            provider_base_url(AgentCliKind::Codex, &provider),
            "https://relay.example.com/root/v1"
        );
        assert_eq!(
            provider_base_url(AgentCliKind::ClaudeCode, &provider),
            "https://relay.example.com/root"
        );
        assert_eq!(
            provider_base_url(AgentCliKind::Gemini, &provider),
            "https://relay.example.com/root"
        );
        assert_eq!(
            provider_base_url(AgentCliKind::Grok, &provider),
            "https://relay.example.com/root/v1"
        );

        provider.liveness.agent_base_urls.insert(
            AgentCliKind::Gemini,
            "https://gemini-relay.example.com/gateway/v1beta/".to_string(),
        );
        assert_eq!(
            provider_base_url(AgentCliKind::Gemini, &provider),
            "https://gemini-relay.example.com/gateway"
        );

        provider.liveness.agent_base_urls.insert(
            AgentCliKind::Grok,
            "https://grok-relay.example.com/gateway/v1/".to_string(),
        );
        assert_eq!(
            provider_base_url(AgentCliKind::Grok, &provider),
            "https://grok-relay.example.com/gateway/v1"
        );
    }
}
