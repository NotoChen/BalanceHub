use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

/// Private files share an atomic, synced replacement path on every platform.
pub(super) fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("同步存储目录无效")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建同步存储目录")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法创建同步临时文件")?;
    temporary.write_all(bytes).map_err(|_| "同步数据写入失败")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "同步数据落盘失败")?;
    temporary
        .persist(path)
        .map_err(|_| "同步数据原子替换失败")?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "同步目录落盘失败")?;
    Ok(())
}

pub(super) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "同步数据编码失败")?;
    write_bytes(path, &bytes)
}

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::File::open(path) {
        Ok(file) => {
            const LIMIT: u64 = 512 * 1024 * 1024;
            let mut bytes = Vec::new();
            file.take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "无法读取本机同步状态")?;
            if bytes.len() as u64 > LIMIT {
                return Err("本机同步文件异常过大，已保留原文件".to_owned());
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "本机同步状态损坏，已保留原文件")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取本机同步状态"),
    }
    .map_err(str::to_owned)
}
