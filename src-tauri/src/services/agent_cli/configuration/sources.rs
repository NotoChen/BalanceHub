//! Native discovery projected into the shared opaque access publication.
use super::{
    contracts::*, errors::diagnostic, service::at_after, ConfigurationService, MAX_DOCUMENT_BYTES,
    MAX_PRIVATE_BYTES,
};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        contracts::AgentSourceDiscoveryRequest,
        environment::{
            self, access_registry::AgentConfigurationAccessAnchor, mutation::GuardedFile,
        },
    },
};
use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) struct ConfigurationSourceAuthority {
    pub(crate) source: AgentConfigurationSource,
    pub(crate) context: AgentConfigurationContext,
    pub(crate) spec: AgentConfigurationSourceSpec,
    pub(crate) file: GuardedFile,
}

struct SourceOutput {
    specs: Vec<AgentConfigurationSourceSpec>,
    diagnostics: Vec<AgentConfigurationDiagnostic>,
    deadline: Instant,
}
impl AgentConfigurationSourceOutput for SourceOutput {
    fn emit(&mut self, source: AgentConfigurationSourceSpec) -> ControlFlow<()> {
        if self.specs.len() >= 256 || Instant::now() >= self.deadline {
            return ControlFlow::Break(());
        }
        self.specs.push(source);
        ControlFlow::Continue(())
    }
    fn diagnostic(&mut self, diagnostic: AgentConfigurationDiagnostic) {
        if self.diagnostics.len() < 128 {
            self.diagnostics.push(diagnostic);
        }
    }
}

impl ConfigurationService {
    pub(crate) fn list_sources(
        &self,
        actor: &str,
        request: AgentConfigurationListRequest,
        settings: Option<&AppSettings>,
    ) -> Result<AgentConfigurationSnapshot, AgentConfigurationError> {
        let workspace =
            environment::normalize_optional_workspace(request.workspace.as_deref().map(Path::new))
                .map_err(|_| {
                    AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidRequest)
                })?;
        let ticket = self.access.begin_configuration_publish(
            actor,
            request.agent_kind,
            workspace.as_deref(),
        )?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let contexts = self.contexts(
            workspace.as_deref(),
            request.workspace.as_deref().map(Path::new),
            request.agent_kind,
            settings,
        );
        let adapter = agent_cli::definition(request.agent_kind).configuration();
        let mut sources = BTreeMap::new();
        let mut authorities = Vec::new();
        let mut diagnostics = Vec::new();
        let mut captured_bytes = 0usize;
        for context in contexts.contexts {
            let mut output = SourceOutput {
                specs: Vec::new(),
                diagnostics: Vec::new(),
                deadline,
            };
            (adapter.discover)(
                AgentSourceDiscoveryRequest {
                    context: &context,
                    home: &self.home,
                    workspace: workspace.as_deref(),
                    installations: &contexts.installations,
                },
                &mut output,
            );
            diagnostics.extend(output.diagnostics);
            for spec in output.specs {
                if Instant::now() >= deadline {
                    return Err(AgentConfigurationError::new(
                        AgentConfigurationErrorKind::Timeout,
                    ));
                }
                let source_id = environment::source_stable_id(&context, &spec.native.path);
                if sources.contains_key(&source_id) {
                    continue;
                }
                let mut source =
                    public_source(&context, &spec, source_id.clone(), workspace.as_deref());
                let allowed = environment::configuration_source_allowed(
                    &self.home,
                    &context,
                    workspace.as_deref(),
                    &spec.native,
                    &contexts.installations,
                );
                let captured = allowed
                    .then(|| {
                        GuardedFile::capture_path(
                            &spec.native.allowed_root,
                            &spec.native.path,
                            MAX_DOCUMENT_BYTES,
                        )
                    })
                    .and_then(Result::ok);
                if let Some(mut file) = captured {
                    file.source_id = source_id.clone();
                    let bytes = file.bytes().map_or(0, <[u8]>::len);
                    if captured_bytes.saturating_add(bytes) <= MAX_PRIVATE_BYTES {
                        captured_bytes += bytes;
                        source.revision = file.revision();
                        source.revision.observed_at = at_after(Duration::ZERO);
                        let missing = source.revision.is_missing;
                        let writable = file_permissions_allow_write(&spec.native.path, missing);
                        source.actions = actions(
                            !missing,
                            writable && !missing,
                            writable && missing && spec.may_create,
                        );
                        authorities.push(Arc::new(ConfigurationSourceAuthority {
                            source: source.clone(),
                            context: context.clone(),
                            spec,
                            file,
                        }));
                    } else {
                        source.diagnostics.push(diagnostic(
                            "configurationReadBudget",
                            "来源总量超出安全读取限制，请缩小所选工作区",
                            AgentConfigurationDiagnosticSeverity::Warning,
                        ));
                    }
                } else {
                    source.diagnostics.push(diagnostic(
                        "configurationSourceUnavailable",
                        "来源不存在、路径包含链接或无法安全读取；未授予写入权限",
                        AgentConfigurationDiagnosticSeverity::Warning,
                    ));
                }
                sources.insert(source_id, source);
            }
        }
        if sources.len() >= 256 {
            diagnostics.push(diagnostic(
                "configurationSourceLimit",
                "来源枚举已达到本次读取限制",
                AgentConfigurationDiagnosticSeverity::Warning,
            ));
        }
        let mut snapshot = AgentConfigurationSnapshot {
            revision: String::new(),
            agent_kind: request.agent_kind,
            environment_id: contexts.environment.id,
            workspace: workspace.map(|path| path.to_string_lossy().into_owned()),
            sources: sources.into_values().collect(),
            diagnostics,
        };
        snapshot.revision = environment::stable_id(
            "configuration-snapshot",
            &snapshot
                .sources
                .iter()
                .map(|source| source.revision.identity.as_str())
                .collect::<Vec<_>>(),
        );
        self.access
            .publish_configuration(ticket, &mut snapshot, authorities)?;
        self.reconcile_receipts(actor, &snapshot);
        Ok(snapshot)
    }

    fn contexts(
        &self,
        workspace: Option<&Path>,
        lexical: Option<&Path>,
        kind: AgentCliKind,
        settings: Option<&AppSettings>,
    ) -> environment::ConfigurationContexts {
        environment::configuration_contexts(&self.home, workspace, lexical, kind, settings)
    }

    pub(super) fn source(
        &self,
        actor: &str,
        request: &AgentConfigurationSourceRequest,
    ) -> Result<Arc<AgentConfigurationAccessAnchor>, AgentConfigurationError> {
        let anchor = self.access.resolve_configuration(actor, request)?;
        if anchor.authority.source.revision.identity != request.expected_revision {
            return Err(super::errors::conflict());
        }
        anchor
            .authority
            .file
            .revalidate()
            .map_err(|_| super::errors::conflict())?;
        // Re-resolve native roots/context before reads and planning, not just apply.
        self.revalidate_context(&anchor.authority)?;
        Ok(anchor)
    }

    pub(super) fn revalidate_context(
        &self,
        source: &ConfigurationSourceAuthority,
    ) -> Result<(), AgentConfigurationError> {
        let workspace = source.source.workspace.as_deref().map(Path::new);
        let current = self.contexts(workspace, workspace, source.source.agent_kind, None);
        if !current.contexts.iter().any(|context| {
            context.id == source.context.id
                && context.config_root == source.context.config_root
                && context.parser_version == source.context.parser_version
        }) {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::RootChanged,
            ));
        }
        Ok(())
    }
}

pub(super) fn public_source(
    context: &AgentConfigurationContext,
    spec: &AgentConfigurationSourceSpec,
    id: String,
    workspace: Option<&Path>,
) -> AgentConfigurationSource {
    AgentConfigurationSource {
        source_id: id,
        context_id: context.id.clone(),
        environment_id: context.environment_id.clone(),
        agent_kind: context.agent_kind,
        workspace: workspace.map(|path| path.to_string_lossy().into_owned()),
        native_role: spec.native.native_source_key.clone(),
        scope: spec.native.scope,
        profile: spec.profile.clone(),
        label: spec.native.label.clone(),
        path: spec.native.path.to_string_lossy().into_owned(),
        format: spec.format,
        revision: AgentAssetRevision::default(),
        access: AgentAssetAccess::default(),
        actions: actions(false, false, false),
        effect: AgentConfigurationEffect {
            kind: AgentConfigurationEffectKind::Unknown,
            message: spec.load_rule.clone(),
            related_source_ids: Vec::new(),
            evidence: spec.version_requirement.iter().cloned().collect(),
        },
        reload_hint: spec.reload_hint.clone(),
        diagnostics: Vec::new(),
    }
}
pub(super) fn actions(read: bool, edit: bool, create: bool) -> Vec<AgentConfigurationAction> {
    use AgentConfigurationActionKind as A;
    [
        (A::Read, read),
        (A::Edit, edit),
        (A::Create, create),
        (A::Open, read),
        (A::Reveal, read),
    ]
    .into_iter()
    .map(|(action, available)| AgentConfigurationAction {
        action,
        available,
        reason: (!available).then(|| {
            match action {
                A::Edit if read => "当前用户没有文件或所在目录的写入权限",
                A::Create => "此位置不能新建配置文件",
                _ => "文件不存在或当前无法读取",
            }
            .to_owned()
        }),
        risks: if available && matches!(action, A::Open | A::Reveal) {
            vec![
                AgentAssetAccessRisk::ExternalPathnameRace,
                AgentAssetAccessRisk::RawSensitiveContent,
            ]
        } else {
            Vec::new()
        },
    })
    .collect()
}
pub(super) fn file_permissions_allow_write(path: &Path, missing: bool) -> bool {
    let mut parent = path;
    if missing {
        while !parent.exists() {
            let Some(ancestor) = parent.parent() else {
                return false;
            };
            parent = ancestor;
        }
    } else {
        if !path_is_writable(path, false) {
            return false;
        }
        let Some(directory) = path.parent() else {
            return false;
        };
        parent = directory;
    }
    path_is_writable(parent, true)
}

#[cfg(unix)]
fn path_is_writable(path: &Path, directory: bool) -> bool {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mode = libc::W_OK | if directory { libc::X_OK } else { 0 };
    // access only queries permissions for the current process user.
    unsafe { libc::access(path.as_ptr(), mode) == 0 }
}

#[cfg(not(unix))]
fn path_is_writable(path: &Path, _directory: bool) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() || !metadata.permissions().readonly())
}

pub(super) fn require_action(
    source: &AgentConfigurationSource,
    action: AgentConfigurationActionKind,
) -> Result<(), AgentConfigurationError> {
    if source
        .actions
        .iter()
        .any(|item| item.action == action && item.available)
    {
        Ok(())
    } else {
        Err(AgentConfigurationError::new(
            AgentConfigurationErrorKind::ReadOnly,
        ))
    }
}
