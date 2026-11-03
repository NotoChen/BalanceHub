mod commands;
mod desktop;
mod desktop_host;
mod services;
mod state;
mod tray;

// 网络协议、数据模型和调度核心已经拆到 `balancehub-core`。这里保留同名
// 模块别名，桌面代码可以继续使用 `crate::models::…` 这类路径，而不必
// 在每个文件里区分能力属于哪个 crate。
pub(crate) use balancehub_core::{contracts, limits, models, network, platform, storage};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some(exit_code) = balancehub_agent::hook_helper_exit_code() {
        std::process::exit(exit_code);
    }
    desktop::run();
}
