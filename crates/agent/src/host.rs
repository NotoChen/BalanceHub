//! 宿主扩展点：打开/定位本机文件。
//!
//! 桌面把这两件事交给系统外壳，无界面后端通常没有可用的文件管理器；
//! 未安装时调用会得到明确的「当前宿主不支持」错误，而不是静默失败。

use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

pub trait FileOpener: Send + Sync + 'static {
    fn open_path(&self, path: &Path) -> Result<(), String>;
    fn reveal(&self, path: &Path) -> Result<(), String>;
}

static OPENER: OnceLock<Arc<dyn FileOpener>> = OnceLock::new();

pub fn install_file_opener(opener: Arc<dyn FileOpener>) -> Result<(), String> {
    OPENER
        .set(opener)
        .map_err(|_| "文件打开器已经安装".to_owned())
}

pub(crate) fn opener() -> Option<&'static Arc<dyn FileOpener>> {
    OPENER.get()
}

/// 当前进程是否有可交互的桌面会话：决定终端启动、系统文件打开是否可用。
pub(crate) fn desktop_session() -> bool {
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        return true;
    }
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// 用系统默认程序打开一个本机文件；桌面宿主的实现会先做路径兼容处理。
pub(crate) fn open_path(path: &Path) -> Result<(), String> {
    let opener = opener().ok_or_else(|| "当前宿主不支持打开本机文件".to_owned())?;
    opener.open_path(crate::platform::paths::compatible_path(path).as_ref())
}

/// 在文件管理器中定位一个本机文件。
pub(crate) fn reveal_path(path: &Path) -> Result<(), String> {
    let opener = opener().ok_or_else(|| "当前宿主不支持定位本机文件".to_owned())?;
    opener.reveal(crate::platform::paths::compatible_path(path).as_ref())
}
