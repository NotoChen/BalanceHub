use super::{
    filesystem::{changed, owned_writable_directory, FileStamp},
    native::{
        validate_vendor_upgrade, vendor_installation, Mechanism, NativeExecution, UPGRADE_TIMEOUT,
    },
    npm::{self, NpmRuntime},
    service::{internal, now_string},
};
use crate::{
    models::*,
    services::agent_cli::{
        self,
        environment::{
            self,
            mutation::{execution::CliApplyEvidence, GuardedDirectory},
            releases::{self, ReleaseSource},
        },
    },
};
use base64::Engine as _;
use futures_util::{future::BoxFuture, FutureExt};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(super) struct LifecycleContext {
    pub(super) home: PathBuf,
    pub(super) settings: AppSettings,
}

pub(super) struct PreparedLifecycle {
    pub(super) public: AgentLifecyclePlan,
    pub(super) lock_keys: Vec<String>,
    pub(super) execution: Arc<dyn LifecycleExecution>,
}

pub(super) trait LifecycleExecution: Send + Sync {
    fn revalidate(&self) -> Result<(), AgentLifecycleError>;
    fn run(&self, commit: &mut dyn FnMut() -> Result<(), AgentAssetMutationError>) -> RunEvidence;
    fn verify(&self) -> Verification;
    fn diagnostics(&self) -> Option<AgentLifecycleDiagnostics> {
        None
    }
}

pub(super) struct RunEvidence {
    pub(super) kind: CliApplyEvidence,
    pub(super) output_truncated: bool,
}

#[derive(Default, Clone)]
pub(super) struct Verification {
    pub(super) version: Option<String>,
    pub(super) executable_path: Option<String>,
    pub(super) changed: bool,
    pub(super) unchanged: bool,
}

pub(super) trait LifecyclePlanner: Send + Sync {
    fn catalog(
        &self,
        context: LifecycleContext,
        request: AgentLifecycleCatalogRequest,
    ) -> BoxFuture<'static, Result<AgentLifecycleCatalog, AgentLifecycleError>>;
    fn plan(
        &self,
        context: LifecycleContext,
        request: AgentLifecyclePlanRequest,
    ) -> BoxFuture<'static, Result<PreparedLifecycle, AgentLifecycleError>>;
}

pub(super) struct NativePlanner;

#[derive(Clone)]
struct InspectedTarget {
    public: AgentLifecycleTarget,
    mechanism: Option<Mechanism>,
    release_source: Option<ReleaseSource>,
    lock_keys: Vec<String>,
    affected_installations: Vec<String>,
}

impl LifecyclePlanner for NativePlanner {
    fn catalog(
        &self,
        context: LifecycleContext,
        request: AgentLifecycleCatalogRequest,
    ) -> BoxFuture<'static, Result<AgentLifecycleCatalog, AgentLifecycleError>> {
        async move {
            let mut targets = blocking({
                let context = context.clone();
                move || inspect_targets(&context)
            })
            .await?;
            targets.retain(|target| {
                request
                    .agent_kind
                    .is_none_or(|kind| target.public.agent_kind == kind)
            });
            let targets = futures_util::future::join_all(targets.into_iter().map(|mut target| {
                let context = context.clone();
                async move {
                    if let Some(source) = &target.release_source {
                        target.public.version = releases::check(
                            &context.settings,
                            source,
                            target.public.installation.installed_version.as_deref(),
                            request.version_refresh,
                        )
                        .await;
                        apply_version_actions(&mut target.public);
                    }
                    target.public
                }
            }))
            .await;
            let next_check_at = targets
                .iter()
                .filter_map(|target| target.version.next_check_at.as_ref())
                .filter_map(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .min()
                .map(|value| value.to_rfc3339());
            Ok(AgentLifecycleCatalog {
                targets,
                refreshed_at: now_string(),
                next_check_at,
            })
        }
        .boxed()
    }

    fn plan(
        &self,
        context: LifecycleContext,
        request: AgentLifecyclePlanRequest,
    ) -> BoxFuture<'static, Result<PreparedLifecycle, AgentLifecycleError>> {
        async move {
            let target = inspect_requested(&context, &request).await?;
            let source = target.release_source.as_ref().ok_or_else(version_failed)?;
            let checked = releases::check(
                &context.settings,
                source,
                target.public.installation.installed_version.as_deref(),
                AgentLifecycleVersionRefresh::IfStale,
            )
            .await;
            if checked.state == AgentLifecycleVersionState::CheckFailed {
                return Err(version_failed());
            }
            let version = checked.latest_version.ok_or_else(version_failed)?;
            prepare_version(context, request, target, version).await
        }
        .boxed()
    }
}

async fn inspect_requested(
    context: &LifecycleContext,
    request: &AgentLifecyclePlanRequest,
) -> Result<InspectedTarget, AgentLifecycleError> {
    let context = context.clone();
    let request = request.clone();
    blocking(move || requested_target(inspect_targets(&context)?, &request)).await
}

async fn prepare_version(
    context: LifecycleContext,
    request: AgentLifecyclePlanRequest,
    target: InspectedTarget,
    version: String,
) -> Result<PreparedLifecycle, AgentLifecycleError> {
    let previous = target
        .public
        .installation
        .installed_version
        .as_deref()
        .and_then(npm::normalized_version);
    if previous
        .as_ref()
        .is_some_and(|previous| !is_upgrade(previous, &version))
    {
        return Err(unavailable(AgentLifecycleUnavailableReason::AlreadyCurrent));
    }
    if let Some(Mechanism::Npm { runtime, .. }) = &target.mechanism {
        validate_npm_release(&context, target.public.agent_kind, runtime, &version).await?;
    }
    blocking(move || {
        // A network lookup must not authorize an installation that changed
        // while the version request was pending.
        let refreshed = requested_target(inspect_targets(&context)?, &request)?;
        prepare(context, refreshed, request.action, version)
    })
    .await
}

fn requested_target(
    targets: Vec<InspectedTarget>,
    request: &AgentLifecyclePlanRequest,
) -> Result<InspectedTarget, AgentLifecycleError> {
    let target = targets
        .into_iter()
        .find(|target| {
            target.public.id == request.target_id && target.public.agent_kind == request.agent_kind
        })
        .ok_or_else(changed)?;
    if target.public.evidence_revision != request.expected_evidence_revision {
        return Err(changed());
    }
    let action = target
        .public
        .actions
        .iter()
        .find(|action| action.kind == request.action)
        .ok_or_else(|| AgentLifecycleError::new(AgentLifecycleErrorKind::ActionMismatch))?;
    if !action.available {
        return Err(unavailable(
            action
                .reason
                .unwrap_or(AgentLifecycleUnavailableReason::ProvenanceUnverified),
        ));
    }
    Ok(target)
}

pub(super) fn installation_facts(
    context: &LifecycleContext,
) -> Result<Vec<AgentLifecycleTarget>, AgentLifecycleError> {
    inspect_targets(context)
        .map(|targets| targets.into_iter().map(|target| target.public).collect())
}

fn inspect_targets(
    context: &LifecycleContext,
) -> Result<Vec<InspectedTarget>, AgentLifecycleError> {
    if !context.home.is_absolute() {
        return Err(AgentLifecycleError::new(
            AgentLifecycleErrorKind::InvalidRequest,
        ));
    }
    let discovered = environment::installation_contexts(&context.home, &context.settings)
        .map_err(|_| internal())?;
    let installations = discovered.installations;
    let contexts = discovered.contexts;
    let current_installations: BTreeSet<_> = agent_cli::definitions()
        .iter()
        .filter_map(|definition| {
            agent_cli::discovery::current_installation_id(
                context.settings.agent_cli_path(definition.kind),
                definition,
                &installations,
            )
        })
        .collect();
    let mut targets = Vec::new();
    for installation in &installations {
        let definition = agent_cli::definition(installation.agent_kind);
        let mut channel = AgentLifecycleChannel::Unverified;
        let mut directory = None;
        let mechanism = if installation.availability != AgentInstallationAvailability::Available {
            Err(unavailable(
                AgentLifecycleUnavailableReason::InstallationUnavailable,
            ))
        } else if let Some(brew) =
            super::homebrew::HomebrewInstallation::inspect(installation).transpose()
        {
            match brew {
                Ok(brew) => {
                    channel = if brew.cask {
                        AgentLifecycleChannel::HomebrewCask
                    } else {
                        AgentLifecycleChannel::HomebrewFormula
                    };
                    directory = Some(brew.prefix.clone());
                    Ok(Mechanism::Homebrew(Box::new(brew)))
                }
                Err(error) => Err(error),
            }
        } else if installation.distribution == AgentCliDistribution::Npm {
            let owner = installation
                .executable_identity
                .as_ref()
                .zip(installation.installed_version.as_deref())
                .and_then(|(identity, version)| {
                    agent_cli::discovery::npm_package_owner(
                        std::path::Path::new(&identity.canonical_path),
                        definition.environment().package_name(),
                        version,
                    )
                });
            if let Some(owner) = owner {
                if let Some(prefix) = owner.global_prefix() {
                    channel = AgentLifecycleChannel::Npm;
                    directory = Some(prefix.clone());
                    (|| {
                        if definition.kind == AgentCliKind::Grok {
                            return Err(unavailable(
                                AgentLifecycleUnavailableReason::UnsupportedChannel,
                            ));
                        }
                        if !owned_writable_directory(&prefix) {
                            return Err(unavailable(
                                AgentLifecycleUnavailableReason::PermissionRequired,
                            ));
                        }
                        let runtime = NpmRuntime::discover(context, &prefix)?;
                        let (stamp, _) = FileStamp::read(&owner.manifest, 64 * 1024)?;
                        Ok(Mechanism::Npm {
                            prefix,
                            runtime: Box::new(runtime),
                            package: Box::new(stamp),
                        })
                    })()
                } else {
                    Err(unavailable(
                        AgentLifecycleUnavailableReason::UnsupportedChannel,
                    ))
                }
            } else {
                Err(unavailable(
                    AgentLifecycleUnavailableReason::ProvenanceUnverified,
                ))
            }
        } else {
            let native = vendor_installation(context, installation);
            if let Ok(native) = &native {
                channel = AgentLifecycleChannel::VendorNative;
                directory = Some(native.directory().to_path_buf());
            }
            native
        };
        let mut target = target_record(
            definition,
            installation.clone(),
            channel,
            directory,
            mechanism,
        );
        target.public.is_current = current_installations.contains(&installation.id);
        add_lock_domains(&mut target, &installations, &contexts);
        targets.push(target);
    }
    Ok(targets)
}

fn target_record(
    definition: &agent_cli::AgentCliDefinition,
    installation: AgentInstallation,
    channel: AgentLifecycleChannel,
    directory: Option<PathBuf>,
    mechanism: Result<Mechanism, AgentLifecycleError>,
) -> InspectedTarget {
    // Release provenance is read-only and remains useful when an upgrade is unavailable.
    let release_source = if let Ok(Mechanism::Homebrew(brew)) = &mechanism {
        Some(ReleaseSource::Homebrew {
            package: brew.package.clone(),
            cask: brew.cask,
        })
    } else if channel == AgentLifecycleChannel::Npm {
        Some(ReleaseSource::Npm(
            definition.environment().package_name().to_owned(),
        ))
    } else {
        mechanism
            .as_ref()
            .ok()
            .and_then(|mechanism| mechanism.release_url(definition.kind))
            .map(ReleaseSource::Vendor)
    };
    let release_track = match &mechanism {
        Ok(Mechanism::Vendor { track, .. }) => Some(track.clone()),
        _ if channel == AgentLifecycleChannel::Npm => Some("latest".to_owned()),
        _ => None,
    };
    let mut reason = mechanism.as_ref().err().and_then(|error| error.reason);
    let mut mechanism = mechanism.ok();
    let directory_guard = directory
        .as_deref()
        .map(GuardedDirectory::capture_path)
        .transpose();
    if directory_guard.is_err() {
        reason = Some(AgentLifecycleUnavailableReason::DirectoryConflict);
        mechanism = None;
    }
    let source = release_source
        .as_ref()
        .map_or(AgentLifecycleVersionSource::Unknown, ReleaseSource::kind);
    let channel_label = match channel {
        AgentLifecycleChannel::Npm => "npm 全局安装",
        AgentLifecycleChannel::HomebrewFormula => "Homebrew Formula",
        AgentLifecycleChannel::HomebrewCask => "Homebrew Cask",
        AgentLifecycleChannel::VendorNative => "官方原生安装",
        AgentLifecycleChannel::Unverified => "安装渠道尚未验证",
    };
    let channel_label = if definition.kind == AgentCliKind::ClaudeCode
        && source == AgentLifecycleVersionSource::NpmRegistry
    {
        format!("{channel_label}（官方已弃用 npm 渠道）")
    } else {
        channel_label.to_owned()
    };
    let directory_text = directory
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let id = environment::stable_id(
        "lifecycle-target",
        &[
            definition.kind.key(),
            &installation.id,
            directory_text.as_deref().unwrap_or_default(),
        ],
    );
    let fingerprint = serde_json::json!({
        "installation": (&installation.id, &installation.executable_identity, &installation.executable_revision, &installation.installed_version, installation.distribution),
        "directory": directory_guard.ok().flatten().map(|guard| guard.signature()),
        "mechanism": mechanism.as_ref().map(Mechanism::signature),
        "channel": channel,
    }).to_string();
    let evidence_revision = environment::stable_id("lifecycle-evidence", &[&fingerprint]);
    let available = mechanism.is_some() && reason.is_none();
    let reason = if available {
        None
    } else {
        Some(reason.unwrap_or(AgentLifecycleUnavailableReason::ProvenanceUnverified))
    };
    InspectedTarget {
        public: AgentLifecycleTarget {
            id,
            agent_kind: definition.kind,
            label: definition.label.to_owned(),
            installation,
            is_current: false,
            channel,
            channel_label,
            release_track,
            directory: directory_text,
            evidence_revision,
            version: AgentLifecycleVersion {
                state: if source == AgentLifecycleVersionSource::Unknown {
                    AgentLifecycleVersionState::Unsupported
                } else {
                    AgentLifecycleVersionState::NotChecked
                },
                source,
                latest_version: None,
                checked_at: None,
                last_success_at: None,
                next_check_at: None,
                stale: false,
                message: (source == AgentLifecycleVersionSource::Unknown)
                    .then(|| "尚未确认该安装的版本来源，请查看官方安装说明".to_owned()),
            },
            actions: vec![AgentLifecycleAction {
                kind: AgentLifecycleActionKind::Upgrade,
                available,
                reason,
                reason_message: reason.map(reason_message).map(str::to_owned),
            }],
        },
        mechanism,
        release_source,
        lock_keys: Vec::new(),
        affected_installations: Vec::new(),
    }
}

fn add_lock_domains(
    target: &mut InspectedTarget,
    installations: &[AgentInstallation],
    contexts: &[AgentConfigurationContext],
) {
    if let Some(mechanism) = &target.mechanism {
        target.lock_keys.push(format!(
            "lifecycle-directory:{}",
            mechanism.directory().display()
        ));
        let selected_id = &target.public.installation.id;
        for installation in installations {
            let shares_prefix = matches!(mechanism, Mechanism::Npm { .. } | Mechanism::Homebrew(_))
                && installation
                    .executable_identity
                    .as_ref()
                    .is_some_and(|identity| {
                        std::path::Path::new(&identity.canonical_path)
                            .starts_with(mechanism.directory())
                    });
            if selected_id == &installation.id || shares_prefix {
                target.affected_installations.push(installation.id.clone());
                target
                    .lock_keys
                    .push(format!("installation:{}", installation.id));
                for context in contexts {
                    if context
                        .compatible_installation_ids
                        .contains(&installation.id)
                    {
                        target
                            .lock_keys
                            .push(format!("config:{}", context.config_root));
                    }
                }
            }
        }
    }
    target.lock_keys.sort();
    target.lock_keys.dedup();
    target.affected_installations.sort();
    target.affected_installations.dedup();
}

fn prepare(
    context: LifecycleContext,
    target: InspectedTarget,
    action: AgentLifecycleActionKind,
    version: String,
) -> Result<PreparedLifecycle, AgentLifecycleError> {
    let mechanism = target
        .mechanism
        .ok_or_else(|| unavailable(AgentLifecycleUnavailableReason::ProvenanceUnverified))?;
    if matches!(mechanism, Mechanism::Vendor { .. }) {
        validate_vendor_upgrade(&context, &target.public.installation, &mechanism)?;
    }
    let execution = NativeExecution::prepare(context, &target.public, mechanism, &version)?;
    let command_preview = execution.command_preview();
    let from_version = target
        .public
        .installation
        .installed_version
        .as_deref()
        .and_then(npm::normalized_version);
    let directory = target.public.directory.clone().ok_or_else(changed)?;
    let mut changes = vec![
        format!(
            "{} 当前 {}，检查时可用版本 {}",
            target.public.label,
            from_version.as_deref().unwrap_or("版本未读取"),
            version
        ),
        format!("沿用安装渠道：{}", target.public.channel_label),
        format!("写入目录：{directory}"),
        "完成后核对实际版本，并按现有启动设置显示下次使用的安装".to_owned(),
    ];
    if matches!(
        target.public.channel,
        AgentLifecycleChannel::HomebrewFormula | AgentLifecycleChannel::HomebrewCask
    ) {
        changes.push("Homebrew 按包配方升级，可能安装或更新必要依赖；升级时允许刷新 Homebrew 元数据，已关闭自动清理和额外依赖方升级".to_owned());
    }
    if target.affected_installations.len() > 1 {
        changes
            .push("同一安装目录内的关联安装会共同串行，避免多个升级任务同时修改该目录".to_owned());
    }
    Ok(PreparedLifecycle {
        public: AgentLifecyclePlan {
            plan_token: String::new(), agent_kind: target.public.agent_kind, target_id: target.public.id,
            installation_id: target.public.installation.id, action,
            channel: target.public.channel, channel_label: target.public.channel_label, directory,
            from_version, to_version: version,
            mechanism_id: match target.public.channel {
                AgentLifecycleChannel::Npm => "official-npm-latest-prefix-v2",
                AgentLifecycleChannel::HomebrewFormula | AgentLifecycleChannel::HomebrewCask => "homebrew-owned-package-v1",
                AgentLifecycleChannel::VendorNative if target.public.agent_kind == AgentCliKind::ClaudeCode => "claude-native-update-v2",
                AgentLifecycleChannel::VendorNative => "grok-native-update-v2",
                AgentLifecycleChannel::Unverified => return Err(changed()),
            }.to_owned(),
            changes, command_preview, affected_installation_ids: target.affected_installations,
            confirmation_message: "确认后使用原安装渠道的更新命令。上方版本是检查时的可用版本，最终以更新器的渠道与版本策略为准；完成后核对实际版本。运行中的 Agent 会话继续使用原进程。".to_owned(),
            cancellation_boundary: "安装程序启动前可以取消；启动后在后台等待结果。安装命令最多运行 5 分钟，之后另行核对版本，不自动重试或回滚。".to_owned(),
            timeout_seconds: UPGRADE_TIMEOUT.as_secs(), expires_at: String::new(),
        },
        lock_keys: target.lock_keys,
        execution: Arc::new(execution),
    })
}

async fn validate_npm_release(
    context: &LifecycleContext,
    kind: AgentCliKind,
    runtime: &NpmRuntime,
    version: &str,
) -> Result<(), AgentLifecycleError> {
    let package = agent_cli::definition(kind).environment().package_name();
    let client = crate::network::build_provider_client_with_proxy(
        crate::network::resolve_global_proxy(&context.settings),
    )
    .map_err(|_| version_failed())?;
    let url = format!(
        "{}/{}/{}",
        npm::NPM_REGISTRY,
        package.replace('/', "%2F"),
        version
    );
    let bytes = releases::fetch_bounded(&client, &url, 512 * 1024)
        .await
        .map_err(|_| version_failed())?;
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| version_failed())?;
    validate_npm_release_document(&document, package, runtime, version)
}

fn validate_npm_release_document(
    document: &serde_json::Value,
    package: &str,
    runtime: &NpmRuntime,
    version: &str,
) -> Result<(), AgentLifecycleError> {
    if document.get("name").and_then(|value| value.as_str()) != Some(package)
        || document.get("version").and_then(|value| value.as_str()) != Some(version)
        || document
            .pointer("/dist/integrity")
            .and_then(|value| value.as_str())
            .and_then(|value| value.strip_prefix("sha512-"))
            .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
            .is_none_or(|digest| digest.len() != 64)
    {
        return Err(version_failed());
    }
    if let Some(requirement) = document.pointer("/engines/node") {
        let node = semver::Version::parse(&runtime.node_version).map_err(|_| version_failed())?;
        let compatible = requirement.as_str().is_some_and(|range| {
            range.split("||").any(|part| {
                let intersection = part.split_ascii_whitespace().collect::<Vec<_>>().join(", ");
                semver::VersionReq::parse(&intersection)
                    .is_ok_and(|requirement| requirement.matches(&node))
            })
        });
        if !compatible {
            return Err(unavailable(
                AgentLifecycleUnavailableReason::RuntimeUnavailable,
            ));
        }
    }
    Ok(())
}

fn apply_version_actions(target: &mut AgentLifecycleTarget) {
    let state = target.version.state;
    if matches!(
        state,
        AgentLifecycleVersionState::UpToDate | AgentLifecycleVersionState::AheadOfLatest
    ) {
        for action in &mut target.actions {
            if action.kind == AgentLifecycleActionKind::Upgrade && action.available {
                action.available = false;
                action.reason = Some(AgentLifecycleUnavailableReason::AlreadyCurrent);
                action.reason_message = Some(
                    reason_message(AgentLifecycleUnavailableReason::AlreadyCurrent).to_owned(),
                );
            }
        }
    }
}

fn is_upgrade(installed: &str, latest: &str) -> bool {
    environment::version_state(Some(installed), Some(latest))
        == AgentLifecycleVersionState::UpdateAvailable
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, AgentLifecycleError> + Send + 'static,
) -> Result<T, AgentLifecycleError> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|_| internal())?
}

fn version_failed() -> AgentLifecycleError {
    AgentLifecycleError::new(AgentLifecycleErrorKind::VersionCheckFailed)
}

pub(super) fn unavailable(reason: AgentLifecycleUnavailableReason) -> AgentLifecycleError {
    AgentLifecycleError {
        kind: AgentLifecycleErrorKind::ActionUnavailable,
        message: reason_message(reason).to_owned(),
        reason: Some(reason),
    }
}

fn reason_message(reason: AgentLifecycleUnavailableReason) -> &'static str {
    match reason {
        AgentLifecycleUnavailableReason::RuntimeUnavailable => {
            "需要可用且满足官方版本要求的 Node.js 与 npm，应用不会自动安装运行时"
        }
        AgentLifecycleUnavailableReason::UnsupportedChannel => {
            "BalanceHub 尚未验证此安装渠道的指定版本升级能力，请使用原安装方式更新"
        }
        AgentLifecycleUnavailableReason::UnsupportedPlatform => {
            "当前平台暂不支持在此升级，请按官方说明使用原安装方式更新"
        }
        AgentLifecycleUnavailableReason::InstallationUnavailable => {
            "选中的安装当前不可执行，请先检查路径"
        }
        AgentLifecycleUnavailableReason::ProvenanceUnverified => {
            "未能确认安装来源，请按官方说明使用原安装方式更新"
        }
        AgentLifecycleUnavailableReason::DirectoryConflict => {
            "安装目录或路径证据已变化，请重新检测后再升级"
        }
        AgentLifecycleUnavailableReason::PermissionRequired => {
            "安装目录不属于当前用户或不可写，请使用原安装方式更新"
        }
        AgentLifecycleUnavailableReason::VersionUnavailable => "无法确认该渠道的目标版本",
        AgentLifecycleUnavailableReason::AlreadyCurrent => {
            "当前安装已达到或高于该渠道最新版本，无需升级"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{installation, write_package, write_program, Fixture};
    use super::*;

    #[test]
    fn npm_plan_locks_all_installations_and_configurations_sharing_its_prefix() {
        let fixture = Fixture::new();
        let prefix = fixture.prefix(AgentCliKind::Codex);
        let codex = installation(
            AgentCliKind::Codex,
            &write_package(&prefix, AgentCliKind::Codex, "1.0.0"),
            "1.0.0",
        );
        let grok = installation(
            AgentCliKind::Grok,
            &write_package(&prefix, AgentCliKind::Grok, "1.0.0"),
            "1.0.0",
        );
        let other = installation(
            AgentCliKind::Gemini,
            &write_package(
                &fixture.prefix(AgentCliKind::Gemini),
                AgentCliKind::Gemini,
                "1.0.0",
            ),
            "1.0.0",
        );
        let mut inventory =
            environment::build_inventory_for_test(&fixture.home, None, &[]).unwrap();
        inventory.installations = vec![codex.clone(), grok.clone(), other.clone()];
        let config = fixture
            .home
            .join("fixture-config")
            .to_string_lossy()
            .into_owned();
        inventory.contexts = vec![AgentConfigurationContext {
            id: "fixture-context".to_owned(),
            environment_id: "fixture-environment".to_owned(),
            agent_kind: AgentCliKind::Codex,
            config_root: config.clone(),
            profile: "default".to_owned(),
            workspace_id: None,
            trust_context: AgentTrustState::Unknown,
            parser_version: 1,
            schema_facts: Default::default(),
            compatible_installation_ids: vec![codex.id.clone(), grok.id.clone()],
        }];
        let mut target = target_record(
            agent_cli::definition(AgentCliKind::Codex),
            codex.clone(),
            AgentLifecycleChannel::Npm,
            Some(prefix.clone()),
            Ok(Mechanism::Npm {
                prefix: prefix.clone(),
                runtime: Box::new(fixture.runtime()),
                package: Box::new(
                    npm::installed_package(&prefix, AgentCliKind::Codex)
                        .unwrap()
                        .1,
                ),
            }),
        );
        add_lock_domains(&mut target, &inventory.installations, &inventory.contexts);
        assert_eq!(
            target.affected_installations,
            [codex.id.clone(), grok.id.clone()]
        );
        assert!(target
            .lock_keys
            .contains(&format!("installation:{}", codex.id)));
        assert!(target
            .lock_keys
            .contains(&format!("installation:{}", grok.id)));
        assert!(!target
            .lock_keys
            .contains(&format!("installation:{}", other.id)));
        assert_eq!(
            target
                .lock_keys
                .iter()
                .filter(|key| *key == &format!("config:{config}"))
                .count(),
            1
        );
        assert!(target
            .lock_keys
            .contains(&format!("lifecycle-directory:{}", prefix.display())));
    }

    #[test]
    fn release_metadata_requires_the_exact_official_package_version_integrity_and_runtime() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode([7_u8; 64])
        );
        let valid = serde_json::json!({
            "name": "@openai/codex", "version": "2.0.0", "dist": { "integrity": integrity },
            "engines": { "node": ">=20.19.0 <21.0.0 || >=22.12.0" },
        });
        assert!(validate_npm_release_document(&valid, "@openai/codex", &runtime, "2.0.0").is_ok());
        for pointer in ["/name", "/version", "/dist/integrity", "/engines/node"] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() =
                serde_json::Value::String("invalid-fixture-value".to_owned());
            assert!(
                validate_npm_release_document(&changed, "@openai/codex", &runtime, "2.0.0")
                    .is_err(),
                "{pointer}"
            );
        }
        let mut incompatible = valid;
        incompatible["engines"]["node"] = serde_json::json!(">=99.0.0");
        assert_eq!(
            validate_npm_release_document(&incompatible, "@openai/codex", &runtime, "2.0.0")
                .unwrap_err()
                .reason,
            Some(AgentLifecycleUnavailableReason::RuntimeUnavailable)
        );
    }

    #[test]
    fn a_native_channel_without_a_proven_release_endpoint_stays_unknown() {
        let fixture = Fixture::new();
        let mechanism = Mechanism::Vendor {
            directory: fixture.home.clone(),
            launcher: fixture.home.join("unproven-codex"),
            track: "stable".to_owned(),
            receipts: Vec::new(),
            absent_receipts: Vec::new(),
        };
        let launcher = fixture.home.join("unproven-codex");
        write_program(&launcher, b"\x7fELFfixture-codex-never-executed");
        let target = target_record(
            agent_cli::definition(AgentCliKind::Codex),
            installation(AgentCliKind::Codex, &launcher, "1.0.0"),
            AgentLifecycleChannel::VendorNative,
            Some(fixture.home.clone()),
            Ok(mechanism),
        );
        assert_eq!(
            target.public.version.source,
            AgentLifecycleVersionSource::Unknown
        );
        assert_eq!(
            target.public.version.state,
            AgentLifecycleVersionState::Unsupported
        );
        assert!(target.release_source.is_none());
        assert!(target.public.version.latest_version.is_none());
    }

    #[test]
    fn up_to_date_and_ahead_versions_disable_only_an_available_upgrade() {
        let fixture = Fixture::new();
        let kind = AgentCliKind::Codex;
        let prefix = fixture.prefix(kind);
        let executable = write_package(&prefix, kind, "2.0.0");
        let current = installation(kind, &executable, "2.0.0");
        let base = super::super::tests::target(kind, &prefix, current);
        for (latest, expected, enabled) in [
            ("2.0.0", AgentLifecycleVersionState::UpToDate, false),
            ("1.0.0", AgentLifecycleVersionState::AheadOfLatest, false),
            ("3.0.0", AgentLifecycleVersionState::UpdateAvailable, true),
        ] {
            let mut target = base.clone();
            target.version.state = environment::version_state(
                target.installation.installed_version.as_deref(),
                Some(latest),
            );
            apply_version_actions(&mut target);
            assert_eq!(target.version.state, expected);
            assert_eq!(target.actions[0].available, enabled);
            assert_eq!(
                target.version.source,
                AgentLifecycleVersionSource::NpmRegistry
            );
        }
    }
}
