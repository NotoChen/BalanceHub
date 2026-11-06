use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const DATA_FILE_NAME: &str = "data.json";
const BACKUP_FILE_NAME: &str = "data.json.bak";
const TMP_FILE_NAME: &str = "data.json.tmp";
const LOCK_FILE_NAME: &str = "data.lock";

pub(super) fn data_file_path() -> Result<PathBuf, String> {
    let config_dir =
        crate::host::config_dir().map_err(|error| format!("获取应用配置目录失败: {error}"))?;
    Ok(config_dir.join(DATA_FILE_NAME))
}

/// 同一份配置目录只允许一个进程写入。桌面版与服务端指向同一目录时，两边
/// 各自持有完整快照，后写的一方会覆盖另一方的修改；这里用操作系统文件锁
/// 在启动阶段直接拦下第二个进程。进程崩溃时内核会释放锁，残留的锁文件不
/// 会阻塞下次启动。
pub(super) fn lock_data_directory(path: &Path) -> Result<(), String> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, File>>> = OnceLock::new();
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)
        .map_err(|error| format!("创建配置目录失败({}): {error}", directory.display()))?;
    let lock_path = directory.join(LOCK_FILE_NAME);
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if locks.contains_key(&lock_path) {
        return Ok(());
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("打开配置锁失败({}): {error}", lock_path.display()))?;
    match try_lock_exclusive(&file) {
        Ok(true) => {
            locks.insert(lock_path, file);
            Ok(())
        }
        Ok(false) => Err(format!(
            "配置目录正在被另一个 BalanceHub 进程使用({})；请先退出桌面版或另一个服务端实例",
            directory.display()
        )),
        Err(error) => Err(format!(
            "锁定配置目录失败({}): {error}",
            lock_path.display()
        )),
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File) -> std::io::Result<bool> {
    use std::os::fd::AsFd;
    match rustix::fs::flock(
        file.as_fd(),
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    ) {
        Ok(()) => Ok(true),
        Err(error)
            if error == rustix::io::Errno::WOULDBLOCK || error == rustix::io::Errno::AGAIN =>
        {
            Ok(false)
        }
        Err(error) => Err(std::io::Error::from(error)),
    }
}

#[cfg(windows)]
fn try_lock_exclusive(file: &File) -> std::io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::ERROR_LOCK_VIOLATION,
        Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY},
        System::IO::OVERLAPPED,
    };
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let locked = unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if locked != 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(not(any(unix, windows)))]
fn try_lock_exclusive(_file: &File) -> std::io::Result<bool> {
    Ok(true)
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
