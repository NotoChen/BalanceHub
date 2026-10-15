//! Read the registered native configuration sources without enumerating assets.
//! Raw documents stay in Rust; the UI receives only the requested setting values.
use crate::{
    models::*,
    services::{
        agent_cli::{
            self,
            configuration::contracts::{
                AgentConfigurationSourceOutput, AgentConfigurationSourceSpec,
            },
            contracts::AgentSourceDiscoveryRequest,
            environment::{self, config_document, mutation::GuardedFile},
        },
        cli_paths,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct Layer {
    pub(super) precedence: u32,
    pub value: Value,
    pub path: PathBuf,
    pub scope: AgentAssetScope,
    pub role: String,
    pub requirements: bool,
}

#[derive(Clone)]
pub(crate) struct NativeSnapshot {
    pub(super) include_process_environment: bool,
    pub kind: AgentCliKind,
    pub trust: AgentTrustState,
    pub root: PathBuf,
    pub(super) workspace: Option<PathBuf>,
    pub(super) feature_defaults: BTreeMap<String, bool>,
    pub(super) feature_defaults_notice: String,
    pub(super) layers: Vec<Layer>,
    pub notices: Vec<String>,
    pub(super) read_failures: Vec<SourceReadFailure>,
}

#[derive(Clone)]
pub(super) struct SourceReadFailure {
    pub path: PathBuf,
    pub message: String,
    precedence: u32,
    role: String,
}

pub(super) struct Observed {
    pub value: Value,
    pub source: String,
    pub managed: bool,
    pub state: AgentSettingValueState,
}

impl Observed {
    pub(super) fn text(&self) -> String {
        display_value(&self.value)
    }
}

struct Output(Vec<AgentConfigurationSourceSpec>);
impl AgentConfigurationSourceOutput for Output {
    fn emit(&mut self, source: AgentConfigurationSourceSpec) -> ControlFlow<()> {
        let role = source.native.native_source_key.as_str();
        if source.profile.is_none()
            && (matches!(
                role,
                "config"
                    | "workspace-config"
                    | "system-config"
                    | "system-requirements"
                    | "settings"
                    | "workspace-settings"
                    | "workspace-local-settings"
                    | "workspace-root-local-settings"
                    | "managed-settings"
                    | "system-defaults"
                    | "system-settings"
                    | "hook-user-managed"
                    | "hook-user-requirements"
            ) || role.starts_with("ancestor-config:"))
        {
            self.0.push(source);
        }
        ControlFlow::Continue(())
    }
    fn diagnostic(&mut self, _: AgentConfigurationDiagnostic) {}
}

pub(super) fn read(request: &TemporaryCliSettingsRequest) -> Result<NativeSnapshot, String> {
    let _read_scope = agent_cli::cache::ReadScope::enter();
    let home = cli_paths::user_home().ok_or("无法定位 Agent 用户目录")?;
    let workspace = environment::normalize_optional_workspace(
        (!request.workdir.trim().is_empty()).then(|| Path::new(&request.workdir)),
    )?;
    // None intentionally avoids installation probing. The selected executable
    // is separately inspected by the bounded, cached --help reader.
    let contexts = environment::configuration_contexts(
        &home,
        workspace.as_deref(),
        workspace.as_deref(),
        request.cli_kind,
        None,
    );
    let context = contexts.contexts.first().ok_or("无法定位 Agent 配置来源")?;
    read_context(
        &home,
        context,
        workspace.as_deref(),
        &contexts.installations,
    )
}

pub(super) fn read_context(
    home: &Path,
    context: &AgentConfigurationContext,
    workspace: Option<&Path>,
    installations: &[AgentInstallation],
) -> Result<NativeSnapshot, String> {
    let mut result = NativeSnapshot {
        kind: context.agent_kind,
        trust: context.trust_context,
        include_process_environment: !context.environment_id.starts_with("wsl:"),
        root: PathBuf::from(&context.config_root),
        workspace: workspace.map(Path::to_path_buf),
        feature_defaults: BTreeMap::new(),
        feature_defaults_notice: String::new(),
        layers: Vec::new(),
        notices: Vec::new(),
        read_failures: Vec::new(),
    };
    if !result.include_process_environment {
        result.notices.push("当前值来自已读取的 WSL 配置文件；发行版内的环境变量和运行时管理设置由 Agent 启动时确认。".into());
    }
    let mut output = Output(Vec::new());
    (agent_cli::definition(context.agent_kind)
        .configuration()
        .discover)(
        AgentSourceDiscoveryRequest {
            context,
            home,
            workspace,
            installations,
        },
        &mut output,
    );
    // Ancestor configs precede the selected directory's config at equal scope.
    output.0.sort_by_key(|spec| {
        (
            spec.native.precedence,
            spec.native.path.components().count(),
        )
    });
    let mut seen = BTreeSet::new();
    for spec in output.0 {
        if !result.include_process_environment
            && crate::services::wsl::split_host_path(&spec.native.path).is_none()
        {
            continue;
        }
        if !seen.insert(spec.native.path.clone()) {
            continue;
        }
        if matches!(
            spec.native.scope,
            AgentAssetScope::Workspace | AgentAssetScope::Local
        ) && context.trust_context != AgentTrustState::Trusted
        {
            if spec.native.path.is_file() {
                result.notices.push("所选目录尚未确认原生信任；当前值按已加载的用户与管理配置显示，项目设置由 Agent 首次启动时确认。".into());
            }
            continue;
        }
        if !environment::configuration_source_allowed(
            home,
            context,
            workspace,
            &spec.native,
            installations,
        ) {
            result.record_read_failure(&spec, "配置来源的读取权限无法确认");
            continue;
        }
        let file = match GuardedFile::capture_path(
            &spec.native.allowed_root,
            &spec.native.path,
            1024 * 1024,
        ) {
            Ok(file) => file,
            Err(error) => {
                result.record_read_failure(&spec, &format!("读取失败：{error}"));
                continue;
            }
        };
        let Some(bytes) = file.bytes() else {
            continue;
        };
        let format = match spec.format {
            AgentConfigurationFormat::Toml => config_document::ConfigDocumentFormat::Toml,
            AgentConfigurationFormat::Jsonc => config_document::ConfigDocumentFormat::Jsonc,
            _ => config_document::ConfigDocumentFormat::Json,
        };
        let Some(value) = config_document::parse(bytes, format).filter(Value::is_object) else {
            result.record_read_failure(&spec, "格式错误，无法解析配置内容");
            continue;
        };
        let role = spec.native.native_source_key;
        result.layers.push(Layer {
            precedence: spec.native.precedence,
            value,
            path: spec.native.path,
            scope: spec.native.scope,
            requirements: role.contains("requirements"),
            role,
        });
    }
    result.notices.sort();
    result.notices.dedup();
    Ok(result)
}

impl NativeSnapshot {
    pub(super) fn feature_default(&self, key: &str) -> Option<Observed> {
        self.feature_defaults.get(key).map(|value| Observed {
            value: Value::Bool(*value),
            source: "当前 Codex CLI 默认开关 · features list".into(),
            managed: false,
            state: AgentSettingValueState::Default,
        })
    }
    fn record_read_failure(&mut self, spec: &AgentConfigurationSourceSpec, reason: &str) {
        let message = format!(
            "{}（{}）：{reason}",
            spec.native.label,
            crate::platform::paths::compatible_path(&spec.native.path).display()
        );
        self.notices.push(message.clone());
        self.read_failures.push(SourceReadFailure {
            path: spec.native.path.clone(),
            precedence: spec.native.precedence,
            role: spec.native.native_source_key.clone(),
            message,
        });
    }

    /// A lower-priority unreadable file cannot replace a value from a higher
    /// readable layer. Environment and invocation overrides are also explicit.
    pub(super) fn read_problem(&self, source: &str, section: &str) -> Option<String> {
        if source.starts_with("环境变量 ") || matches!(source, "本次启动模型映射" | "本次启动选择")
        {
            return None;
        }
        let precedence = self
            .layers
            .iter()
            .filter(|layer| {
                crate::platform::paths::compatible_path(&layer.path).to_string_lossy() == source
            })
            .map(|layer| layer.precedence)
            .max();
        let messages: Vec<_> = self
            .read_failures
            .iter()
            .filter(|failure| {
                precedence.is_none_or(|value| failure.precedence >= value)
                    && !(self.kind == AgentCliKind::Grok
                        && failure.role == "workspace-config"
                        && section != "permissions")
            })
            .map(|failure| failure.message.as_str())
            .collect();
        (!messages.is_empty()).then(|| messages.join("\n"))
    }

    pub(super) fn get(&self, pointer: &str) -> Option<Observed> {
        self.layers
            .iter()
            .rev()
            .filter(|layer| !layer.requirements)
            .find_map(|layer| self.value_from_layer(layer, pointer))
    }

    pub(super) fn value_from_layer(&self, layer: &Layer, pointer: &str) -> Option<Observed> {
        // Grok's project config deliberately contributes only these sections.
        if layer.role == "workspace-config"
            && self.kind == AgentCliKind::Grok
            && !["/mcp_servers", "/plugins", "/permission"]
                .iter()
                .any(|prefix| pointer.starts_with(prefix))
        {
            return None;
        }
        layer.value.pointer(pointer).map(|value| Observed {
            value: value.clone(),
            source: crate::platform::paths::compatible_path(&layer.path)
                .to_string_lossy()
                .into_owned(),
            managed: layer.role != "system-defaults"
                && matches!(
                    layer.scope,
                    AgentAssetScope::Managed | AgentAssetScope::System
                ),
            state: AgentSettingValueState::Configured,
        })
    }

    pub(super) fn env(&self, name: &str, pointer: Option<&str>) -> Option<Observed> {
        if let Some(value) = pointer.and_then(|pointer| {
            self.layers
                .iter()
                .rev()
                .filter(|layer| layer.role == "launch-model-mappings")
                .find_map(|layer| self.value_from_layer(layer, pointer))
        }) {
            return Some(value);
        }
        // Native environment keys are passed only from the explicit descriptors.
        self.include_process_environment
            .then(|| std::env::var(name).ok())
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .map(|value| Observed {
                value: Value::String(value),
                source: format!("环境变量 {name}"),
                managed: false,
                state: AgentSettingValueState::Configured,
            })
            .or_else(|| pointer.and_then(|pointer| self.get(pointer)))
    }

    pub(super) fn arrays(&self, pointer: &str) -> Vec<Value> {
        let mut values = Vec::new();
        for layer in self.layers.iter().filter(|layer| !layer.requirements) {
            if let Some(items) = layer.value.pointer(pointer).and_then(Value::as_array) {
                for item in items {
                    if !values.contains(item) {
                        values.push(item.clone());
                    }
                }
            }
        }
        values
    }

    pub(super) fn requirement(&self, pointer: &str) -> Option<&Value> {
        self.layers
            .iter()
            .rev()
            .filter(|layer| layer.requirements)
            .find_map(|layer| layer.value.pointer(pointer))
    }
}

pub(super) fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .map(display_value)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => value.to_string(),
    }
}
