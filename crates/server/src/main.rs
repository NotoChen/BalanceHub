//! BalanceHub 无界面后端。
//!
//! 与桌面版共用 `balancehub-core`，不编译任何 Agent、终端或 Tauri 代码。
//! 数据目录通过环境变量指定，默认落在用户目录下的 `.balancehub`。

mod api;
mod host;
mod logging;

use balancehub_core::{host as core_host, state::CoreState, storage};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();
    let data_dir = env_dir("BALANCEHUB_DATA_DIR", |root| root.clone());
    let config_dir = env_dir("BALANCEHUB_CONFIG_DIR", |root| root.join("config"));
    let cache_dir = env_dir("BALANCEHUB_CACHE_DIR", |root| root.join("cache"));
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&cache_dir)?;

    let server_host = Arc::new(host::ServerHost::new(
        data_dir,
        config_dir,
        cache_dir.clone(),
    ));
    core_host::install(server_host.clone())?;
    balancehub_core::runtime::install(tokio::runtime::Handle::current())?;

    let data = storage::load_app_data()?;
    let core = Arc::new(CoreState::new(data));
    server_host.attach_core(core);
    // 常驻进程的核心价值：没有界面也会按设置自动刷新、签到和测活。
    balancehub_core::services::scheduler::start();

    let addr: SocketAddr = std::env::var("BALANCEHUB_SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse()
        .map_err(|_| "BALANCEHUB_SERVER_ADDR 不是有效的监听地址")?;
    let web_dir = std::env::var_os("BALANCEHUB_WEB_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            // 发布包把前端放在可执行文件旁边；开发时退回仓库的 dist。
            let beside = std::env::current_exe().ok()?.parent()?.join("web");
            if beside.is_dir() {
                return Some(beside);
            }
            let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist");
            repository.is_dir().then_some(repository)
        });
    api::serve(addr, server_host.clone(), web_dir).await?;
    Ok(())
}

fn env_dir(variable: &str, fallback: impl FnOnce(&PathBuf) -> PathBuf) -> PathBuf {
    let root = std::env::var("BALANCEHUB_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default();
            PathBuf::from(home).join(".balancehub")
        });
    std::env::var(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|_| fallback(&root))
}
