pub(crate) mod imports;
pub(crate) mod interchange;
pub(crate) mod login;
mod login_callback;
mod login_device;
pub(crate) mod native;

#[cfg(test)]
mod oauth_tests;

use std::{
    collections::HashMap,
    fs,
    io::Write,
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub(crate) fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "无法生成安全随机数")?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}

/// The owner ID is deliberately outside AppData/WebDAV, so a synchronized
/// credential does not start rotating independently on another computer.
pub(crate) fn device_id(app: &tauri::AppHandle) -> Result<String, String> {
    static ID: OnceLock<Result<String, String>> = OnceLock::new();
    ID.get_or_init(|| {
        let path = crate::desktop::data_dir(app)
            .map_err(|_| "账号数据目录不可用")?
            .join("oauth-device-id");
        if !path.exists() {
            let id = random_id()?;
            fs::create_dir_all(path.parent().ok_or("账号数据目录不可用")?)
                .map_err(|_| "无法创建账号数据目录")?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(id.as_bytes())
                        .and_then(|_| file.sync_all())
                        .map_err(|_| "保存本机授权标识失败")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("保存本机授权标识失败".into()),
            }
        }
        let id = crate::util::read_text_file_limited(&path, 128, "读取本机授权标识")?;
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("本机授权标识损坏，已停止自动续期".into());
        }
        Ok(id)
    })
    .clone()
}

pub(crate) fn account_lock(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    type Locks = Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>;
    static LOCKS: OnceLock<Locks> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(id.into(), Arc::downgrade(&lock));
    lock
}
