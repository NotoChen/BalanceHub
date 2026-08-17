use serde::Serialize;
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

const DATA_FILE_NAME: &str = "data.json";
const BACKUP_FILE_NAME: &str = "data.json.bak";
const TMP_FILE_NAME: &str = "data.json.tmp";

pub(super) fn data_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("获取应用配置目录失败: {error}"))?;
    Ok(config_dir.join(DATA_FILE_NAME))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn replace_data_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    fs::rename(tmp_path, path).map_err(|error| format!("写入配置失败({}): {error}", path.display()))
}

#[cfg(target_os = "windows")]
pub(super) fn replace_data_file(tmp_path: &Path, path: &Path) -> Result<(), String> {
    let backup_path = backup_file_path(path);
    let had_target = path.exists();

    if had_target {
        fs::copy(path, &backup_path)
            .map_err(|error| format!("备份配置失败({}): {error}", backup_path.display()))?;
        fs::remove_file(path)
            .map_err(|error| format!("替换配置失败({}): {error}", path.display()))?;
    }

    match fs::rename(tmp_path, path) {
        Ok(()) => {
            if had_target {
                let _ = fs::remove_file(&backup_path);
            }
            Ok(())
        }
        Err(error) => {
            if had_target && !path.exists() && backup_path.exists() {
                let _ = fs::rename(&backup_path, path);
            }
            Err(format!("写入配置失败({}): {error}", path.display()))
        }
    }
}

pub(super) fn tmp_file_path(path: &Path) -> PathBuf {
    path.with_file_name(TMP_FILE_NAME)
}

pub(super) fn backup_file_path(path: &Path) -> PathBuf {
    path.with_file_name(BACKUP_FILE_NAME)
}

struct LimitedWriter<W> {
    inner: W,
    written: usize,
    limit: usize,
    exceeded: bool,
}

impl<W> LimitedWriter<W> {
    fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            exceeded: false,
        }
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.written.saturating_add(buffer.len()) > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "serialized data exceeds configured limit",
            ));
        }
        let written = self.inner.write(buffer)?;
        self.written = self.written.saturating_add(written);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub(super) fn write_json_file_limited<T: Serialize>(
    path: &Path,
    value: &T,
    max_bytes: usize,
    context: &str,
) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|error| format!("{context}失败({}): {error}", path.display()))?;
    let mut writer = LimitedWriter::new(BufWriter::new(file), max_bytes);
    if let Err(error) = serde_json::to_writer_pretty(&mut writer, value) {
        let exceeded = writer.exceeded;
        drop(writer);
        let _ = fs::remove_file(path);
        return Err(if exceeded {
            format!(
                "{context}失败({})：序列化结果超过 {} MiB 上限",
                path.display(),
                max_bytes / 1024 / 1024
            )
        } else {
            format!("{context}失败({}): {error}", path.display())
        });
    }
    writer.flush().map_err(|error| {
        let _ = fs::remove_file(path);
        format!("{context}失败({}): {error}", path.display())
    })?;
    drop(writer);
    Ok(())
}
