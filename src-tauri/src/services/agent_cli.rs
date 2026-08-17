//! Agent CLI registry and behavior boundary.
//!
//! The registry owns metadata, discovery and optional capability adapters. Generic orchestration
//! never matches concrete Agent kinds; a new built-in Agent adds one registry entry and its own
//! adapter modules without editing launch, liveness, session or config orchestration.

pub(crate) mod config_support;
pub(crate) mod contracts;
mod discovery;
mod liveness_support;

use crate::models::{
    AgentCliCapabilities, AgentCliDescriptor, AgentCliKind, AppSettings, CliEnvironmentProbeResult,
    CliToolProbeResult, Provider,
};
use contracts::{
    DefaultConfigAdapter, EndpointAdapter, LivenessAdapter, SessionAdapter, TemporaryLaunchAdapter,
};
use std::path::{Path, PathBuf};

pub(crate) struct AgentCliDefinition {
    pub kind: AgentCliKind,
    pub label: &'static str,
    pub executable: &'static str,
    pub session_name_hint: &'static str,
    pub additional_env_keys: &'static [&'static str],
    pub home_candidates: fn(&Path) -> Vec<PathBuf>,
    pub invalid_path_reason: Option<fn(&Path) -> Option<&'static str>>,
    pub require_version_substring: Option<&'static str>,
    endpoint: EndpointAdapter,
    temporary_launch: Option<TemporaryLaunchAdapter>,
    sessions: Option<SessionAdapter>,
    liveness: Option<LivenessAdapter>,
    default_config: Option<DefaultConfigAdapter>,
}

impl AgentCliDefinition {
    pub(crate) fn capabilities(&self) -> AgentCliCapabilities {
        AgentCliCapabilities {
            temporary_launch: self.temporary_launch.is_some(),
            model_selection: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_model_selection()),
            session_history: self.sessions.is_some(),
            session_resume: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_session_resume()),
            session_name: self
                .temporary_launch
                .is_some_and(|adapter| adapter.supports_session_name()),
            liveness: self.liveness.is_some(),
            default_config: self.default_config.is_some(),
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
}

#[derive(Debug, Clone)]
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
        const DEFINITIONS: &[AgentCliDefinition] = &[
            $($module::definition(AgentCliKind::$variant)),+
        ];
        #[cfg(test)]
        const AGENT_MODULE_NAMES: &[&str] = &[$(stringify!($module)),+];
    };
}

// 枚举身份、模块声明和注册表由同一份目录生成。
crate::agent_cli_catalog::for_each_agent_cli!(register_agent_clis);

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

pub(crate) fn find(
    settings: &AppSettings,
    kind: AgentCliKind,
    include_shell: bool,
) -> Result<AgentCliExecutable, String> {
    find_with_preferred(kind, settings.agent_cli_path(kind), include_shell)
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

pub(crate) fn runtime_path_for(cli_path: &Path) -> Option<std::ffi::OsString> {
    discovery::runtime_path_for(cli_path)
}

pub(crate) fn provider_base_url(kind: AgentCliKind, provider: &Provider) -> String {
    definition(kind)
        .endpoint
        .normalize_base_url(provider_raw_base_url(kind, provider))
}

pub(crate) fn provider_raw_base_url(kind: AgentCliKind, provider: &Provider) -> &str {
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
    fn balancehub_cli_path_environment_key_is_not_duplicated_per_agent() {
        for registered_definition in definitions() {
            assert!(registered_definition
                .additional_env_keys
                .iter()
                .all(|key| !key.starts_with("BALANCEHUB_")));
        }
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
        }

        assert!(!definition(AgentCliKind::Codex).capabilities().session_name);
        assert!(
            definition(AgentCliKind::ClaudeCode)
                .capabilities()
                .session_name
        );
        assert!(definitions()
            .iter()
            .all(|definition| definition.capabilities().temporary_launch));
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
