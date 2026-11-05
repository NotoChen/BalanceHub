//! 宿主能力：核心服务只依赖这组接口，不依赖任何窗口框架。
//!
//! 桌面宿主把 Tauri 的数据目录、事件和系统集成能力接进来；无界面宿主
//! 只提供数据目录与可选的 HTTP 事件通道。

use crate::{models::AppData, services::cloud_sync::library::CloudLibrary, state::CoreState};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

pub trait Host: Send + Sync + 'static {
    /// 宿主类型：桌面把本机能力都接上，服务端只提供数据与事件。
    fn kind(&self) -> &'static str;

    /// 进程内唯一的配置状态。
    fn core_state(&self) -> Arc<CoreState>;

    /// 应用数据目录：测活隔离 HOME、浏览器运行时、登录档案都以此为基准。
    fn data_dir(&self) -> Result<PathBuf, String>;
    /// 应用配置目录：`data.json` 与本地扩展配置都在这里。
    fn config_dir(&self) -> Result<PathBuf, String>;
    /// 可再生的缓存目录。
    fn cache_dir(&self) -> Result<PathBuf, String>;

    /// 向界面广播事件；没有界面的宿主可以忽略。
    fn emit(&self, event: &str, payload: serde_json::Value);
    /// 发送系统通知；不支持通知的宿主返回错误。
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;
    /// 用系统默认浏览器打开链接。
    fn open_url(&self, url: &str) -> Result<(), String>;

    /// 当前进程是否有可交互的桌面会话；由宿主按运行环境判断。
    fn has_desktop_session(&self) -> bool {
        true
    }

    /// 配置数据落盘后的宿主扩展点（例如云同步）。
    /// `changed` 由宿主自行判断，核心不重复实现同步规则。
    fn app_data_changed(&self, previous: &AppData, next: &AppData);

    /// 共享库（Agent 资产）的同步扩展点；没有本机资产库的宿主返回 `None`。
    fn cloud_library(&self) -> Option<Arc<dyn CloudLibrary>> {
        None
    }

    /// 托盘提示随中转站状态变化；没有托盘概念的宿主不需要实现。
    fn refresh_tray(&self) {}
}

/// 把任意可序列化负载转成事件值后广播。
pub fn emit<S: Serialize>(event: &str, payload: S) {
    current().emit(
        event,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    );
}

pub fn core_state() -> Arc<CoreState> {
    current().core_state()
}

pub fn data_dir() -> Result<PathBuf, String> {
    current().data_dir()
}

pub fn config_dir() -> Result<PathBuf, String> {
    current().config_dir()
}

pub fn cache_dir() -> Result<PathBuf, String> {
    current().cache_dir()
}

pub fn notify(title: &str, body: &str) -> Result<(), String> {
    current().notify(title, body)
}

pub fn open_url(url: &str) -> Result<(), String> {
    current().open_url(url)
}

pub fn app_data_changed(previous: &AppData, next: &AppData) {
    current().app_data_changed(previous, next);
}

pub fn cloud_library() -> Option<Arc<dyn CloudLibrary>> {
    current().cloud_library()
}

pub fn refresh_tray() {
    current().refresh_tray();
}

/// 无界面宿主（服务端）没有可显示的签到窗口，浏览器以无头模式运行。
pub fn is_headless() -> bool {
    SHARED.get().is_some_and(|host| host.kind() == "server")
}

/// 当前进程是否有可交互的桌面会话。
///
/// 与「是否无界面宿主」不同：服务端部署在有桌面环境的机器上时，终端启动和
/// 系统文件打开同样可用。由宿主按运行环境判断，界面据此决定是否展示入口。
pub fn has_desktop_session() -> bool {
    SHARED.get().is_none_or(|host| host.has_desktop_session())
}

static SHARED: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// 进程级宿主。核心服务在启动后通过它取得数据目录、事件和系统集成能力，
/// 因此不再把宿主句柄一路传进每个函数。
pub fn current() -> &'static dyn Host {
    SHARED.get().map(|host| &**host).expect("宿主尚未安装")
}

/// 进程启动时注册唯一宿主，便于没有显式接收宿主参数的纯函数取用。
pub fn install(host: Arc<dyn Host>) -> Result<(), String> {
    SHARED.set(host).map_err(|_| "宿主已经注册".to_owned())
}

/// 测试宿主：核心测试需要数据目录与事件接口，但不涉及窗口。
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Once;

    pub struct TestHost {
        root: PathBuf,
        core: OnceLock<Arc<CoreState>>,
    }

    static INSTALL: Once = Once::new();

    impl TestHost {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("balancehub-core-test-{}", std::process::id()));
            let _ = std::fs::create_dir_all(root.join("config"));
            let _ = std::fs::create_dir_all(root.join("cache"));
            Self {
                root,
                core: OnceLock::new(),
            }
        }
    }

    impl Host for TestHost {
        fn kind(&self) -> &'static str {
            "test"
        }

        fn core_state(&self) -> Arc<CoreState> {
            Arc::clone(
                self.core
                    .get_or_init(|| Arc::new(CoreState::new(AppData::default()))),
            )
        }

        fn data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.join("data"))
        }

        fn config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.join("config"))
        }

        fn cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.join("cache"))
        }

        fn emit(&self, _event: &str, _payload: serde_json::Value) {}

        fn notify(&self, _title: &str, _body: &str) -> Result<(), String> {
            Ok(())
        }

        fn open_url(&self, _url: &str) -> Result<(), String> {
            Err("测试宿主不能打开浏览器".to_owned())
        }

        fn app_data_changed(&self, _previous: &AppData, _next: &AppData) {}
    }

    /// 安装一次进程级测试宿主；多个测试共用同一份临时目录。
    pub fn install() {
        INSTALL.call_once(|| {
            let host = Arc::new(TestHost::new());
            let _ = super::install(host.clone());
        });
    }
}
