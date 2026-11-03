use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct LaunchDirectory {
    directory: PathBuf,
}

pub(crate) fn register_launch_directory(
    status_path: &Path,
    directory: &Path,
) -> Result<(), String> {
    let instance_dir = status_path.parent().ok_or("运行记录目录不存在")?;
    let directory =
        validated_directory(directory).ok_or("启动配置目录不属于 BalanceHub 临时目录")?;
    super::write_json_atomic(
        &instance_dir.join("launch-cleanup.json"),
        &LaunchDirectory { directory },
    )
}

pub(super) fn cleanup(instance_dir: &Path) {
    let marker = instance_dir.join("launch-cleanup.json");
    let Ok(record) = super::read_json::<LaunchDirectory>(&marker) else {
        return;
    };
    if !record.directory.exists() {
        let _ = fs::remove_file(marker);
        return;
    }
    let Some(directory) = validated_directory(&record.directory) else {
        return;
    };
    if fs::remove_dir_all(directory).is_ok() {
        let _ = fs::remove_file(marker);
    }
}

fn validated_directory(path: &Path) -> Option<PathBuf> {
    if fs::symlink_metadata(path).ok()?.file_type().is_symlink() {
        return None;
    }
    if !path
        .file_name()?
        .to_str()?
        .starts_with("balancehub-temporary-cli-")
    {
        return None;
    }
    let path = fs::canonicalize(path).ok()?;
    if !path
        .file_name()?
        .to_str()?
        .starts_with("balancehub-temporary-cli-")
    {
        return None;
    }
    let temporary = fs::canonicalize(std::env::temp_dir()).ok()?;
    (path.parent() == Some(temporary.as_path())).then_some(path)
}
