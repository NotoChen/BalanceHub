//! 核心异步运行时。
//!
//! 核心服务不直接依赖窗口框架的异步运行时。宿主在启动时装入自己的
//! Tokio 句柄（桌面宿主用 Tauri 的全局运行时，服务端用当前运行时），
//! 之后所有后台任务都从这里派发。

use std::{future::Future, sync::OnceLock};
use tokio::runtime::Handle;

static HANDLE: OnceLock<Handle> = OnceLock::new();

pub fn install(handle: Handle) -> Result<(), String> {
    HANDLE
        .set(handle)
        .map_err(|_| "核心异步运行时已经安装".to_owned())
}

fn handle() -> Option<&'static Handle> {
    HANDLE.get()
}

/// 派发一个后台任务。未安装句柄时退回当前 Tokio 上下文，测试和
/// 已经位于运行时内的调用方仍然可用。
pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    match handle() {
        Some(handle) => handle.spawn(future),
        None => tokio::spawn(future),
    }
}

pub fn spawn_blocking<F, R>(task: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    match handle() {
        Some(handle) => handle.spawn_blocking(task),
        None => tokio::task::spawn_blocking(task),
    }
}
