//! 浏览器组件命令：安装、选择、探测与卸载签到用的本地浏览器。
//!
//! 桌面用可见窗口过盾；服务端没有窗口，用同一套组件以无头模式运行。

use serde_json::Value;

use super::{arg, to_value};
use crate::services::browser_runtime::{self, BrowserSelection};

pub const COMMANDS: &[&str] = &[
    "get_browser_runtime_status",
    "install_browser_runtime",
    "cancel_browser_runtime_install",
    "select_browser_runtime_browser",
    "inspect_browser_runtime_browser",
    "uninstall_browser_runtime",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "get_browser_runtime_status" => {
            let force: bool = super::optional(args, "force")?.unwrap_or(false);
            let status = crate::api::blocking(move || browser_runtime::status(force)).await?;
            to_value(status)
        }
        "install_browser_runtime" => {
            let selection: BrowserSelection = arg(args, "selection")?;
            let repair: bool = super::optional(args, "repair")?.unwrap_or(false);
            to_value(browser_runtime::start_install(selection, repair)?)
        }
        "cancel_browser_runtime_install" => {
            browser_runtime::cancel_install()?;
            Ok(Value::Null)
        }
        "select_browser_runtime_browser" => {
            let selection: BrowserSelection = arg(args, "selection")?;
            to_value(browser_runtime::select_browser(selection)?)
        }
        "inspect_browser_runtime_browser" => {
            let path: String = arg(args, "path")?;
            let browser = crate::api::blocking(move || {
                browser_runtime::inspect_browser(std::path::Path::new(&path))
            })
            .await?;
            to_value(browser)
        }
        "uninstall_browser_runtime" => {
            let status = crate::api::blocking(browser_runtime::uninstall).await?;
            to_value(status)
        }
        _ => Err(format!("未实现的浏览器组件命令：{command}")),
    }
}
