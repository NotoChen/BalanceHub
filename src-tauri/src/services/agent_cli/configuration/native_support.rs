//! Schema-neutral helpers for Agent-owned configuration manifests.
//! Native file locations and formats are supplied by each registered adapter.
use super::contracts::*;
use crate::{
    models::*,
    services::agent_cli::{
        contracts::{
            AgentAssetSnapshot, AgentAssetSourceSpec, AgentDiagnosticEmission,
            AgentDiagnosticOutput, AgentOutputStop, AgentSourceDiscoveryRequest,
            InitialSourceOutput,
        },
        environment::{
            self,
            config_document::{self, ConfigDocumentFormat},
            verified_path::inspect_verified_path,
            SourceInput,
        },
    },
};
use serde_json::Value;
use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SOURCE_LIMIT: usize = 128;

pub(crate) fn collect_native_sources(
    discover: for<'a> fn(AgentSourceDiscoveryRequest<'a>, &mut dyn InitialSourceOutput),
    request: AgentSourceDiscoveryRequest<'_>,
    roles: &[&str],
) -> Vec<AgentAssetSourceSpec> {
    struct Collector<'a> {
        roles: &'a [&'a str],
        sources: Vec<AgentAssetSourceSpec>,
        deadline: Instant,
    }
    impl AgentDiagnosticOutput for Collector<'_> {
        fn has_regular_capacity(&self) -> bool {
            self.sources.len() < SOURCE_LIMIT && Instant::now() < self.deadline
        }
        fn emit_diagnostic(&mut self, _value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            AgentDiagnosticEmission::Accepted
        }
    }
    impl InitialSourceOutput for Collector<'_> {
        fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
            if !self.has_regular_capacity() {
                return ControlFlow::Break(AgentOutputStop::SourceLimit);
            }
            if self.roles.contains(&source.native_source_key.as_str()) {
                self.sources.push(source);
            }
            ControlFlow::Continue(())
        }
    }
    // snapshot_initial deliberately retains its default: discovery gets no raw
    // filesystem reader and cannot perform eager package discovery here.
    let mut output = Collector {
        roles,
        sources: Vec::new(),
        deadline: Instant::now() + Duration::from_secs(2),
    };
    discover(request, &mut output);
    output.sources
}

pub(crate) fn describe(
    native: AgentAssetSourceSpec,
    format: AgentConfigurationFormat,
    may_create: bool,
    load_rule: &str,
    reload_hint: &str,
) -> AgentConfigurationSourceSpec {
    AgentConfigurationSourceSpec {
        native,
        format,
        may_create,
        profile: None,
        load_rule: load_rule.to_owned(),
        reload_hint: reload_hint.to_owned(),
        version_requirement: None,
        initial_text: None,
    }
}

pub(in crate::services::agent_cli) fn document(
    input: SourceInput<'_>,
    format: AgentConfigurationFormat,
    may_create: bool,
    load_rule: &str,
) -> AgentConfigurationSourceSpec {
    describe(
        environment::source(input),
        format,
        may_create,
        load_rule,
        "保存后，请在新会话或原生重载后确认加载结果",
    )
}

pub(crate) fn instruction(
    root: &Path,
    path: PathBuf,
    key: &str,
    label: &str,
    scope: AgentAssetScope,
    may_create: bool,
    load_rule: &str,
) -> AgentConfigurationSourceSpec {
    document(
        SourceInput {
            origin: AgentAssetInstallationOrigin::LocalFiles,
            native_source_key: key,
            label,
            path,
            allowed_root: root,
            scope,
            precedence: if scope == AgentAssetScope::User {
                10
            } else {
                20
            },
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[],
        },
        AgentConfigurationFormat::Markdown,
        may_create,
        load_rule,
    )
}

pub(crate) fn credential_cache(
    root: &Path,
    path: PathBuf,
    key: &str,
    label: &str,
) -> AgentConfigurationSourceSpec {
    let native = environment::source(SourceInput {
        origin: AgentAssetInstallationOrigin::LocalFiles,
        native_source_key: key,
        label,
        path,
        allowed_root: root,
        scope: AgentAssetScope::User,
        precedence: 0,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: &[],
    });
    describe(
        native,
        AgentConfigurationFormat::Json,
        false,
        "原生 CLI 使用的认证文件，登录或退出时也可能更新此文件",
        "认证缓存由原生登录或退出流程维护",
    )
}

/// One directory only, no followed links and no recursive project/home scan.
pub(crate) fn regular_files(root: &Path, directory: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(guard) =
        inspect_verified_path(&[root], root, directory, AgentAssetSourceKind::Directory)
    else {
        return Vec::new();
    };
    let Ok(entries) = guard.read_entries() else {
        return Vec::new();
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut paths = Vec::new();
    for entry in entries.take(SOURCE_LIMIT) {
        if Instant::now() >= deadline {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if entry.is_symlink || entry.source_kind != AgentAssetSourceKind::File {
            continue;
        }
        if entry
            .name
            .to_str()
            .is_some_and(|name| name.ends_with(suffix))
        {
            paths.push(directory.join(entry.name));
        }
    }
    if guard.revalidate().is_err() {
        return Vec::new();
    }
    paths.sort();
    paths
}

/// Native configuration values are read only to discover explicit instruction
/// filenames. This never grants authority to an arbitrary path in that value.
pub(crate) fn configured_filenames(
    root: &Path,
    path: &Path,
    format: ConfigDocumentFormat,
    keys: &[&str],
) -> Vec<String> {
    let Ok(guard) = inspect_verified_path(&[root], root, path, AgentAssetSourceKind::File) else {
        return Vec::new();
    };
    let Ok(bytes) = guard.read_file_bounded(1024 * 1024) else {
        return Vec::new();
    };
    let Some(value) = config_document::parse(&bytes, format) else {
        return Vec::new();
    };
    if guard.revalidate().is_err() {
        return Vec::new();
    }
    let value = keys.iter().try_fold(&value, |value, key| value.get(*key));
    let values: Vec<&str> = match value {
        Some(Value::String(name)) => vec![name],
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .take(16)
        .filter(|name| {
            !name.is_empty()
                && *name != "."
                && *name != ".."
                && !name.contains(['/', '\\', '\0'])
                && name.len() <= 128
        })
        .map(str::to_owned)
        .collect()
}

/// Asset inventory reads resource-bearing sources only. Configuration-only
/// files (including authentication caches) are discovered by the configuration
/// service on demand, not parsed as asset policy inputs.
pub(crate) fn discover_inventory_sources(
    resources: for<'a> fn(AgentSourceDiscoveryRequest<'a>, &mut dyn InitialSourceOutput),
    discover: for<'a> fn(AgentSourceDiscoveryRequest<'a>, &mut dyn AgentConfigurationSourceOutput),
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    struct SourceOutput<'a> {
        output: &'a mut dyn InitialSourceOutput,
        stop: Option<AgentOutputStop>,
    }
    impl AgentDiagnosticOutput for SourceOutput<'_> {
        fn has_regular_capacity(&self) -> bool {
            self.stop.is_none() && self.output.has_regular_capacity()
        }

        fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            self.output.emit_diagnostic(value)
        }
    }
    impl InitialSourceOutput for SourceOutput<'_> {
        fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
            if let Some(stop) = self.stop {
                return ControlFlow::Break(stop);
            }
            let result = self.output.emit_initial(source);
            if let ControlFlow::Break(stop) = result {
                self.stop = Some(stop);
            }
            result
        }

        fn snapshot_initial(
            &mut self,
            source: AgentAssetSourceSpec,
        ) -> ControlFlow<AgentOutputStop, Option<AgentAssetSnapshot>> {
            if let Some(stop) = self.stop {
                return ControlFlow::Break(stop);
            }
            let result = self.output.snapshot_initial(source);
            if let ControlFlow::Break(stop) = &result {
                self.stop = Some(*stop);
            }
            result
        }
    }
    struct Sink<'a>(&'a mut dyn InitialSourceOutput);
    impl AgentConfigurationSourceOutput for Sink<'_> {
        fn emit(&mut self, source: AgentConfigurationSourceSpec) -> ControlFlow<()> {
            if source.native.categories.is_empty() {
                return ControlFlow::Continue(());
            }
            if !self.0.has_regular_capacity() {
                return ControlFlow::Break(());
            }
            self.0.emit_initial(source.native).map_break(|_| ())
        }
        fn diagnostic(&mut self, diagnostic: AgentConfigurationDiagnostic) {
            if diagnostic.code == "configurationAncestorLimit" {
                self.0.emit_diagnostic(AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::FirstLevelEntries,
                    accepted: 32,
                    observed_at_least: 33,
                });
            }
            // Format/scope advice belongs to the configuration surface. Native
            // inventory still reports its own snapshot/access diagnostics.
        }
    }
    let mut output = SourceOutput { output, stop: None };
    resources(request, &mut output);
    if output.stop.is_some() {
        return;
    }
    discover(request, &mut Sink(&mut output));
}

pub(crate) fn ancestor_chain(
    workspace: &Path,
    stop: Option<&Path>,
    output: &mut dyn AgentConfigurationSourceOutput,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for ancestor in workspace.ancestors().take(33) {
        if paths.len() == 32 {
            output.diagnostic(diagnostic(
                "configurationAncestorLimit",
                "祖先目录超过本次 32 层读取限制，来源列表不完整",
                AgentConfigurationDiagnosticSeverity::Warning,
            ));
            break;
        }
        paths.push(ancestor.to_path_buf());
        if stop == Some(ancestor) {
            break;
        }
    }
    paths.reverse();
    paths
}

pub(crate) fn repository_chain(
    workspace: &Path,
    output: &mut dyn AgentConfigurationSourceOutput,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for ancestor in workspace.ancestors().take(33) {
        if paths.len() == 32 {
            output.diagnostic(diagnostic(
                "configurationAncestorLimit",
                "项目根查找超过 32 层限制，暂只列当前目录来源",
                AgentConfigurationDiagnosticSeverity::Warning,
            ));
            return vec![workspace.to_owned()];
        }
        paths.push(ancestor.to_path_buf());
        if std::fs::symlink_metadata(ancestor.join(".git"))
            .is_ok_and(|metadata| !metadata.file_type().is_symlink())
        {
            paths.reverse();
            return paths;
        }
    }
    output.diagnostic(diagnostic(
        "configurationProjectRootUnknown",
        "未定位到 Git 项目根；只列当前目录，原生自定义项目根规则尚未核对",
        AgentConfigurationDiagnosticSeverity::Info,
    ));
    vec![workspace.to_owned()]
}

pub(crate) fn diagnostic(
    code: &str,
    message: &str,
    severity: AgentConfigurationDiagnosticSeverity,
) -> AgentConfigurationDiagnostic {
    AgentConfigurationDiagnostic {
        code: code.to_owned(),
        message: message.to_owned(),
        severity,
        source_id: None,
        line: None,
        column: None,
    }
}

pub(crate) fn parse(
    text: &str,
    format: AgentConfigurationFormat,
) -> Result<Value, AgentConfigurationError> {
    let format = match format {
        AgentConfigurationFormat::Json => ConfigDocumentFormat::Json,
        AgentConfigurationFormat::Jsonc => ConfigDocumentFormat::Jsonc,
        AgentConfigurationFormat::Toml => ConfigDocumentFormat::Toml,
        AgentConfigurationFormat::Dotenv => {
            return super::codec::parse_env(text).map_err(|_| {
                error(
                    AgentConfigurationErrorKind::InvalidSyntax,
                    ".env 只能包含静态单行赋值与注释，不能有重复键或未闭合引号",
                )
            });
        }
        AgentConfigurationFormat::Markdown => {
            return if text.contains('\0') {
                Err(error(
                    AgentConfigurationErrorKind::InvalidSyntax,
                    "文本包含不支持的空字符",
                ))
            } else {
                Ok(Value::Null)
            };
        }
    };
    config_document::parse(text.as_bytes(), format).ok_or_else(|| {
        error(
            AgentConfigurationErrorKind::InvalidSyntax,
            "配置格式无效，请检查语法、重复字段和文件格式",
        )
    })
}

pub(crate) fn validate(
    request: ConfigurationValidationRequest<'_>,
) -> Result<ConfigurationValidation, AgentConfigurationError> {
    parse(request.after, request.source.format)?;
    Ok(ConfigurationValidation {
        diagnostics: Vec::new(),
        reload_hints: vec![request.source.reload_hint.clone()],
    })
}

pub(crate) fn error(kind: AgentConfigurationErrorKind, message: &str) -> AgentConfigurationError {
    AgentConfigurationError {
        kind,
        message: message.to_owned(),
        diagnostics: vec![diagnostic(
            "nativeConfigurationRejected",
            message,
            AgentConfigurationDiagnosticSeverity::Error,
        )],
    }
}
