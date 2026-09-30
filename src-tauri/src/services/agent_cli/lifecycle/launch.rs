//! Report the same executable selection used by the launcher, including aliases.
use crate::{models::*, services::agent_cli};
use std::path::Path;

pub(crate) fn annotate(operation: &mut AgentLifecycleOperation, settings: &AppSettings) {
    if operation.outcome.is_none() {
        return;
    }
    let resolved = agent_cli::discovery::find_cli(
        settings.agent_cli_path(operation.agent_kind),
        agent_cli::definition(operation.agent_kind),
        false,
    );
    annotate_resolved(operation, resolved);
}

fn annotate_resolved(
    operation: &mut AgentLifecycleOperation,
    resolved: Result<agent_cli::AgentCliExecutable, String>,
) {
    operation.next_launch = Some(match resolved {
        Ok(executable) => {
            let selected = Path::new(&executable.path).canonicalize().ok();
            let upgraded = operation
                .verified_executable_path
                .as_deref()
                .and_then(|path| Path::new(path).canonicalize().ok());
            let uses_upgraded_installation = selected.is_some() && selected == upgraded;
            AgentLifecycleNextLaunch {
                message: if operation.verified_executable_path.is_none() {
                    "已重新检测下次启动路径；本次升级结果仍需核对"
                } else if uses_upgraded_installation {
                    "下次启动将使用此安装；已运行的会话保持原版本"
                } else {
                    "下次启动将使用另一处安装；可选择采用本次升级的安装"
                }
                .to_owned(),
                executable_path: Some(executable.path),
                version: Some(executable.version),
                uses_upgraded_installation,
            }
        }
        Err(_) => AgentLifecycleNextLaunch {
            executable_path: None,
            version: None,
            uses_upgraded_installation: false,
            message: "暂时无法确认下次启动路径，请重新检测安装".to_owned(),
        },
    });
}

pub(crate) fn annotate_batch(operations: &mut [AgentLifecycleOperation], settings: &AppSettings) {
    let mut resolved = std::collections::BTreeMap::new();
    for operation in operations
        .iter_mut()
        .filter(|operation| operation.outcome.is_some())
    {
        let executable = resolved.entry(operation.agent_kind).or_insert_with(|| {
            agent_cli::discovery::find_cli(
                settings.agent_cli_path(operation.agent_kind),
                agent_cli::definition(operation.agent_kind),
                false,
            )
        });
        annotate_resolved(operation, executable.clone());
    }
}
