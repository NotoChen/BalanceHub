//! Guarded directory creation shared by library and native configuration writes.
use crate::{
    models::AgentAssetSourceKind,
    services::agent_cli::environment::verified_path::inspect_verified_path,
};
use std::path::{Component, Path};

#[cfg(unix)]
pub(crate) fn ensure_directory(root: &Path, path: &Path) -> Result<(), String> {
    ensure_directory_before(root, path, || Ok(()))
}

#[cfg(unix)]
pub(crate) fn ensure_directory_before(
    root: &Path,
    path: &Path,
    before_create: impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    ensure_directory_observed(root, path, before_create, || {})
}

#[cfg(unix)]
pub(crate) fn ensure_directory_observed(
    root: &Path,
    path: &Path,
    before_create: impl Fn() -> Result<(), String>,
    after_create: impl Fn(),
) -> Result<(), String> {
    use rustix::fs::{mkdirat, Mode};
    let relative = path.strip_prefix(root).map_err(|_| "目录越界")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("目录包含无效组件".to_owned());
        };
        let parent =
            inspect_verified_path(&[root], root, &current, AgentAssetSourceKind::Directory)
                .map_err(|_| "父目录不可安全访问")?;
        let next = current.join(name);
        match std::fs::symlink_metadata(&next) {
            Ok(_) => {
                inspect_verified_path(&[root], root, &next, AgentAssetSourceKind::Directory)
                    .map_err(|_| "目标目录不可安全访问")?;
                current = next;
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("目标目录状态无法确定".to_owned()),
        }
        before_create()?;
        match mkdirat(parent.source_handle(), name, Mode::from_raw_mode(0o700)) {
            Ok(()) => {
                after_create();
                parent
                    .source_handle()
                    .sync_all()
                    .map_err(|_| "目录创建后同步失败")?;
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(_) => return Err("无法创建资产目录".to_owned()),
        }
        parent
            .revalidate_identity()
            .map_err(|_| "父目录在创建期间变化")?;
        current.push(name);
        inspect_verified_path(&[root], root, &current, AgentAssetSourceKind::Directory)
            .map_err(|_| "目标目录不可安全访问")?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn ensure_directory(root: &Path, path: &Path) -> Result<(), String> {
    ensure_directory_before(root, path, || Ok(()))
}

#[cfg(windows)]
pub(crate) fn ensure_directory_before(
    root: &Path,
    path: &Path,
    before_create: impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    ensure_directory_observed(root, path, before_create, || {})
}

#[cfg(windows)]
pub(crate) fn ensure_directory_observed(
    root: &Path,
    path: &Path,
    before_create: impl Fn() -> Result<(), String>,
    after_create: impl Fn(),
) -> Result<(), String> {
    let relative = path.strip_prefix(root).map_err(|_| "目录越界")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("目录包含无效组件".to_owned());
        };
        // Windows verified guards retain all ancestors without delete sharing.
        // Neither the parent nor its ancestors can be renamed during creation.
        let parent =
            inspect_verified_path(&[root], root, &current, AgentAssetSourceKind::Directory)
                .map_err(|_| "父目录不可安全访问")?;
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                before_create()?;
                match std::fs::create_dir(&current) {
                    Ok(()) => after_create(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err("无法创建资产目录".to_owned()),
                }
            }
            Err(_) => return Err("目标目录状态无法确定".to_owned()),
        }
        parent
            .revalidate_identity()
            .map_err(|_| "父目录在创建期间变化")?;
        inspect_verified_path(&[root], root, &current, AgentAssetSourceKind::Directory)
            .map_err(|_| "目标目录不可安全访问")?;
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn ensure_directory(root: &Path, path: &Path) -> Result<(), String> {
    ensure_directory_before(root, path, || Ok(()))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn ensure_directory_before(
    root: &Path,
    path: &Path,
    _before_create: impl Fn() -> Result<(), String>,
) -> Result<(), String> {
    if path == root {
        Ok(())
    } else {
        Err("此平台的原子目录写入尚未验证".to_owned())
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn ensure_directory_observed(
    root: &Path,
    path: &Path,
    before_create: impl Fn() -> Result<(), String>,
    _after_create: impl Fn(),
) -> Result<(), String> {
    ensure_directory_before(root, path, before_create)
}
