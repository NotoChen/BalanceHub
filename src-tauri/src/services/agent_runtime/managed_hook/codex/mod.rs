//! Codex user-scope Hook adapter.
//!
//! The adapter is split by responsibility so inspection, planning, mutation,
//! ownership persistence and command formatting remain independently auditable.

use std::{
    env,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

mod apply;
mod format;
mod helper;
mod inspect;
pub(crate) mod ownership;
mod plan;

pub(super) const HELPER_VERSION: &str = "codex-hook-v1";
pub(crate) const MANIFEST_SCHEMA_VERSION: u16 = 1;
pub(super) const HOOK_EVENTS: [&str; 5] = [
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

#[derive(Debug, Clone)]
pub struct CodexHookService {
    pub(super) config_path: PathBuf,
    pub(super) manifest_path: PathBuf,
    pub(super) helper_path: PathBuf,
    pub(super) spool_root: PathBuf,
}

#[derive(Debug, Clone)]
pub(super) struct FoundResource {
    pub(super) event_name: String,
    pub(super) structural_identity: String,
    pub(super) fingerprint: String,
}

#[derive(Debug, Clone)]
pub(super) struct HookDefinition {
    pub(super) event_name: &'static str,
    pub(super) identity: String,
    pub(super) handler: serde_json::Value,
}

pub(crate) static MUTATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

impl CodexHookService {
    pub fn from_app(app: &tauri::AppHandle) -> Result<Self, String> {
        use tauri::Manager;
        let home = ownership::home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法定位 BalanceHub 数据目录: {error}"))?;
        let managed_dir = app_data.join("agent-hooks").join("codex");
        Ok(Self::new(
            home.join(".codex").join("hooks.json"),
            managed_dir.join("ownership.json"),
            env::current_exe()
                .map_err(|error| format!("无法定位 BalanceHub 可执行文件: {error}"))?,
            app_data,
        ))
    }

    pub fn new(
        config_path: PathBuf,
        manifest_path: PathBuf,
        helper_path: PathBuf,
        spool_root: PathBuf,
    ) -> Self {
        Self {
            config_path,
            manifest_path,
            helper_path,
            spool_root,
        }
    }
}

pub fn helper_from_process_args() -> Option<i32> {
    helper::run()
}

#[cfg(test)]
#[path = "../tests.rs"]
mod tests;
