use crate::{
    models::AgentCliKind,
    network::ProxyEnvironment,
    services::agent_cli::{
        self,
        contracts::{EnvironmentPatch, TemporaryLaunchPlan},
    },
};
use std::path::Path;

/// Wrap the existing Agent plan with wsl.exe. Arguments remain distinct through
/// PowerShell, WSL and the login shell; no command is assembled by interpolation.
pub(crate) fn wrap(
    id: &str,
    kind: AgentCliKind,
    workdir: &Path,
    cli: &str,
    plan: &mut TemporaryLaunchPlan,
    proxy: &ProxyEnvironment,
) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("WSL 会话只能在 Windows 上启动".into());
    }
    let distro = super::require_running(id)?;
    let agent = distro
        .agents
        .iter()
        .find(|agent| agent.agent_kind == kind)
        .ok_or("WSL Agent 已不可用")?;
    if agent.executable.as_deref() != Some(cli) {
        return Err("WSL CLI 已改变，请重新检测".into());
    }
    let (name, guest_workdir) = super::split_host_path(workdir).ok_or("会话工作目录不属于 WSL")?;
    if !name.eq_ignore_ascii_case(&distro.name) {
        return Err("会话工作目录属于其它发行版".into());
    }
    let adapter = agent_cli::definition(kind)
        .sessions()
        .ok_or("Agent 不支持会话")?
        .history();
    let environment =
        (adapter.launch_environment)(Path::new(&agent.config_root)).map_err(str::to_owned)?;
    let proxy = proxy.for_wsl(
        distro.network_mode.as_deref() == Some("mirrored"),
        distro.host_gateway.as_deref(),
    )?;
    let mut args = vec![
        "--distribution".into(),
        distro.name,
        "--cd".into(),
        guest_workdir,
        "--exec".into(),
        "/bin/sh".into(),
        "-lc".into(),
        "exec \"$@\"".into(),
        "balancehub".into(),
        "env".into(),
    ];
    for patch in [&plan.environment, &environment] {
        for name in patch.removed_names() {
            args.extend(["-u".into(), name.into()]);
        }
    }
    for name in proxy.removed_names() {
        args.extend(["-u".into(), name.into()]);
    }
    args.push("--".into());
    for patch in [&plan.environment, &environment] {
        for (name, value) in patch.set_values() {
            args.push(format!("{name}={value}"));
        }
    }
    for (name, value) in proxy.variables() {
        args.push(format!("{name}={value}"));
    }
    args.push(cli.into());
    args.append(&mut plan.args);
    plan.args = args;
    plan.environment = EnvironmentPatch::default();
    // An absolute system path prevents a workspace executable named wsl.exe.
    let executable = super::probe::executable()?;
    Ok(executable.to_string_lossy().into_owned())
}
