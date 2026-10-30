//! Short-lived reservations close the gap between an App launch and process discovery.
//! External processes are observed, never stopped. Process arguments stay in memory.

use crate::{
    models::{
        AgentCliKind, AgentLifecycleChannel, AgentLifecycleTarget, AgentRuntimeScope,
        TemporaryCliInstanceStatus,
    },
    services::{agent_cli, cli_runtime},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone)]
pub(super) struct UpdateScope {
    pub(super) key: String,
    kind: AgentCliKind,
    roots: Vec<PathBuf>,
    manager_root: Option<PathBuf>,
}

impl UpdateScope {
    pub(super) fn from_target(target: &AgentLifecycleTarget) -> Option<Self> {
        let key = target.update_key.clone()?;
        let directory = PathBuf::from(target.directory.as_ref()?);
        let mut roots = Vec::new();
        let mut manager_root = None;
        match target.channel {
            AgentLifecycleChannel::Npm => {
                roots.push(super::npm::package_root(&directory, target.agent_kind));
                manager_root = Some(directory);
            }
            AgentLifecycleChannel::HomebrewFormula | AgentLifecycleChannel::HomebrewCask => {
                let canonical = Path::new(
                    &target
                        .installation
                        .executable_identity
                        .as_ref()?
                        .canonical_path,
                );
                let store = canonical.ancestors().find(|path| {
                    path.file_name()
                        .is_some_and(|name| name == "Cellar" || name == "Caskroom")
                })?;
                roots.push(
                    store.join(
                        canonical
                            .strip_prefix(store)
                            .ok()?
                            .components()
                            .next()?
                            .as_os_str(),
                    ),
                );
                manager_root = Some(directory);
            }
            AgentLifecycleChannel::VendorNative => roots.push(directory),
            AgentLifecycleChannel::Unverified => return None,
        }
        if let Some(path) = &target.installation.executable_path {
            roots.push(PathBuf::from(path));
        }
        if let Some(identity) = &target.installation.executable_identity {
            roots.push(PathBuf::from(&identity.canonical_path));
        }
        Some(Self {
            key,
            kind: target.agent_kind,
            roots,
            manager_root,
        })
    }

    fn matches(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| within(path, root))
    }

    /// A successful snapshot must contain this App. Missing Agent/Node command details
    /// are treated as uncertain occupancy instead of proof that an install is idle.
    pub(super) fn check_processes(&self) -> Result<(), String> {
        if cli_runtime::runtime_instances().iter().any(|instance| {
            instance.cli_kind == self.kind
                && instance.status == TemporaryCliInstanceStatus::Starting
                && instance
                    .native_session
                    .as_ref()
                    .is_none_or(|session| session.runtime_scope == AgentRuntimeScope::Native)
        }) {
            return Err("等待正在启动的 Agent 会话完成启动".into());
        }
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
        let current = sysinfo::Pid::from_u32(std::process::id());
        if system.process(current).is_none() {
            return Err("无法读取本机进程状态，自动更新等待下次核对".into());
        }
        let executable = agent_cli::definition(self.kind).executable;
        for (pid, process) in system.processes() {
            if *pid == current {
                continue;
            }
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            let name = name.strip_suffix(".exe").unwrap_or(&name);
            if process.exe().is_some_and(|path| self.matches(path))
                || process.cmd().iter().any(|arg| self.matches(Path::new(arg)))
            {
                return Err(format!(
                    "等待使用此安装的 Agent 退出（进程 {}）",
                    pid.as_u32()
                ));
            }
            if name == executable && process.exe().is_none() {
                return Err(format!(
                    "进程 {} 的安装位置暂时无法核实，等待其退出后更新",
                    pid.as_u32()
                ));
            }
            if matches!(name, "node" | "bun" | "deno") && process.cmd().is_empty() {
                return Err(format!(
                    "无法确认进程 {} 是否正在使用此安装，等待下次核对",
                    pid.as_u32()
                ));
            }
            if let Some(root) = &self.manager_root {
                let args = process.cmd();
                let changing = args.iter().any(|arg| {
                    matches!(
                        arg.to_str(),
                        Some(
                            "install" | "upgrade" | "update" | "reinstall" | "uninstall" | "remove"
                        )
                    )
                });
                let manager = args.iter().any(|arg| {
                    let path = Path::new(arg);
                    within(path, root)
                        && path.file_name().is_some_and(|name| {
                            name == "npm-cli.js" || name == "brew" || name == "brew.rb"
                        })
                });
                if changing && manager {
                    return Err(format!(
                        "等待此安装目录的包管理器完成（进程 {}）",
                        pid.as_u32()
                    ));
                }
            }
        }
        Ok(())
    }
}

fn within(path: &Path, root: &Path) -> bool {
    let path = crate::platform::paths::compatible_path(path);
    let root = crate::platform::paths::compatible_path(root);
    if cfg!(windows) {
        let path = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let root = root
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase();
        path == root || path.starts_with(&format!("{root}\\"))
    } else {
        path.starts_with(root.as_ref())
    }
}

#[derive(Default)]
struct Reservations {
    sequence: u64,
    uses: BTreeMap<u64, PathBuf>,
    updates: BTreeMap<u64, UpdateScope>,
}

fn reservations() -> &'static Mutex<Reservations> {
    static STATE: OnceLock<Mutex<Reservations>> = OnceLock::new();
    STATE.get_or_init(Mutex::default)
}

pub(crate) struct ExecutionLease {
    id: u64,
}

pub(crate) fn is_updating_kind(kind: AgentCliKind) -> bool {
    reservations().lock().map_or(true, |state| {
        state.updates.values().any(|scope| scope.kind == kind)
    })
}
pub(super) struct UpdateReservation {
    id: u64,
}

pub(crate) fn acquire(executable: &Path) -> Result<ExecutionLease, String> {
    let path = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_path_buf());
    let mut state = reservations()
        .lock()
        .map_err(|_| "Agent 运行状态暂时不可用")?;
    if state
        .updates
        .values()
        .any(|scope| scope.matches(&path) || scope.matches(executable))
    {
        return Err("此 Agent 正在自动更新，请在任务中心查看进度，完成后即可启动".into());
    }
    state.sequence = state.sequence.wrapping_add(1);
    let id = state.sequence;
    state.uses.insert(id, path);
    Ok(ExecutionLease { id })
}

pub(super) fn reserve(scope: &UpdateScope) -> Result<UpdateReservation, String> {
    let mut state = reservations()
        .lock()
        .map_err(|_| "Agent 运行状态暂时不可用")?;
    if state.uses.values().any(|path| scope.matches(path)) {
        return Err("等待此安装的启动或测活任务结束".into());
    }
    if state.updates.values().any(|current| {
        current.key == scope.key || current.roots.iter().any(|path| scope.matches(path))
    }) {
        return Err("等待此安装的其他更新任务完成".into());
    }
    state.sequence = state.sequence.wrapping_add(1);
    let id = state.sequence;
    state.updates.insert(id, scope.clone());
    Ok(UpdateReservation { id })
}

impl Drop for ExecutionLease {
    fn drop(&mut self) {
        reservations()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .uses
            .remove(&self.id);
    }
}
impl Drop for UpdateReservation {
    fn drop(&mut self) {
        reservations()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .updates
            .remove(&self.id);
    }
}
