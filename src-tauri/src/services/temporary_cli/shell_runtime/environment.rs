use std::collections::BTreeMap;

#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(in crate::services::temporary_cli) struct ShellEnvironmentSnapshot {
    pub(in crate::services::temporary_cli) variables: BTreeMap<String, String>,
    pub(in crate::services::temporary_cli) aliases: BTreeMap<String, String>,
    pub(in crate::services::temporary_cli) functions: BTreeMap<String, String>,
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(in crate::services::temporary_cli) fn capture_shell_environment() -> BTreeMap<String, String> {
    windows::capture().variables
}

#[cfg(not(target_os = "windows"))]
pub(in crate::services::temporary_cli) fn capture_shell_environment() -> BTreeMap<String, String> {
    // Unix 临时脚本会在目标终端中启动用户的登录交互 shell，导出变量随父 shell
    // 继承到 /bin/sh 和最终 CLI，无需在 Rust 侧再次复制一份环境。
    BTreeMap::new()
}

#[cfg(target_os = "windows")]
pub(in crate::services::temporary_cli) fn capture_shell_snapshot() -> ShellEnvironmentSnapshot {
    windows::capture()
}

pub(in crate::services::temporary_cli) fn insert_environment(
    target: &mut BTreeMap<String, String>,
    name: &str,
    value: &str,
) {
    #[cfg(any(target_os = "windows", test))]
    if let Some(existing) = target
        .keys()
        .find(|existing| existing.eq_ignore_ascii_case(name))
        .cloned()
    {
        target.remove(&existing);
    }
    target.insert(name.to_string(), value.to_string());
}
