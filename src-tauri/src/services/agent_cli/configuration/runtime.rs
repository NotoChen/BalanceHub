use crate::{
    models::*,
    services::{agent_cli::environment, wsl},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) struct ConfigurationRuntime {
    pub home: PathBuf,
    pub workspace: Option<PathBuf>,
    pub contexts: environment::ConfigurationContexts,
}

fn failure(message: String) -> AgentConfigurationError {
    super::native_support::error(AgentConfigurationErrorKind::RootChanged, &message)
}

pub(super) fn resolve(
    home: &Path,
    request: &AgentConfigurationListRequest,
    settings: Option<&AppSettings>,
) -> Result<ConfigurationRuntime, AgentConfigurationError> {
    match &request.runtime_scope {
        AgentRuntimeScope::Native => {
            let workspace = environment::normalize_optional_workspace(
                request.workspace.as_deref().map(Path::new),
            )
            .map_err(failure)?;
            let contexts = environment::configuration_contexts(
                home,
                workspace.as_deref(),
                request.workspace.as_deref().map(Path::new),
                request.agent_kind,
                settings,
            );
            Ok(ConfigurationRuntime {
                home: home.to_owned(),
                workspace,
                contexts,
            })
        }
        AgentRuntimeScope::Wsl { distro_id } => {
            let distro = wsl::require_running(distro_id).map_err(failure)?;
            let home = wsl::host_path(
                &distro.name,
                distro
                    .home
                    .as_deref()
                    .ok_or_else(|| failure("WSL 用户目录不可用".into()))?,
            )
            .map_err(failure)?;
            let agent = distro
                .agents
                .iter()
                .find(|agent| agent.agent_kind == request.agent_kind)
                .ok_or_else(|| failure("WSL Agent 环境不可用".into()))?;
            let root = wsl::host_path(&distro.name, &agent.config_root).map_err(failure)?;
            let workspace = request
                .workspace
                .as_deref()
                .map(|path| {
                    if let Some((name, guest)) = wsl::split_host_path(Path::new(path)) {
                        if !name.eq_ignore_ascii_case(&distro.name) {
                            return Err("工作目录属于其它 WSL 发行版".to_owned());
                        }
                        wsl::host_path(&name, &guest)
                    } else {
                        wsl::host_path(&distro.name, path)
                    }
                })
                .transpose()
                .map_err(failure)?;
            let mut environment = environment::native_environment();
            environment.id = format!("wsl:{}", distro.id);
            environment.kind = AgentEnvironmentKind::Wsl;
            environment.guest_platform = Some("linux".into());
            environment.display_name = format!("WSL · {}", distro.name);
            let root = root.to_string_lossy().into_owned();
            let workspace_id = workspace.as_ref().map(|path| {
                environment::stable_id("wsl-workspace", &[&environment.id, &path.to_string_lossy()])
            });
            let context = AgentConfigurationContext {
                id: environment::stable_id(
                    "wsl-configuration-v1",
                    &[
                        &environment.id,
                        request.agent_kind.key(),
                        &root,
                        workspace_id.as_deref().unwrap_or(""),
                    ],
                ),
                environment_id: environment.id.clone(),
                agent_kind: request.agent_kind,
                config_root: root,
                profile: "default".into(),
                workspace_id,
                trust_context: AgentTrustState::Unknown,
                parser_version: 1,
                schema_facts: BTreeMap::new(),
                compatible_installation_ids: Vec::new(),
            };
            Ok(ConfigurationRuntime {
                home,
                workspace,
                contexts: environment::ConfigurationContexts {
                    environment,
                    contexts: vec![context],
                    installations: Vec::new(),
                },
            })
        }
    }
}
