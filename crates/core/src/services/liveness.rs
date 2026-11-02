//! 测活的纯规则与宿主执行器接口。
//!
//! 提示词、间隔和模型选择只依赖配置数据，放在核心；真正启动本地 Agent CLI
//! 并读取响应属于桌面能力，由桌面宿主实现 [`LivenessExecutor`] 后装入。
//! 没有执行器的宿主（例如服务端）不会自动测活。

pub mod prompt;

pub use prompt::{effective_cli_kind, effective_interval, preview_prompts};

use crate::models::{AppSettings, LivenessRecord, Provider};
use std::sync::{Arc, OnceLock};

pub trait LivenessExecutor: Send + Sync + 'static {
    fn run(
        &self,
        settings: &AppSettings,
        provider: &Provider,
        prompt: Option<String>,
        automatic: bool,
    ) -> Result<LivenessRecord, String>;

    fn waits_for_update(&self, settings: &AppSettings, provider: &Provider) -> bool;
}

static EXECUTOR: OnceLock<Arc<dyn LivenessExecutor>> = OnceLock::new();

pub fn install_executor(executor: Arc<dyn LivenessExecutor>) -> Result<(), String> {
    EXECUTOR
        .set(executor)
        .map_err(|_| "测活执行器已经安装".to_owned())
}

pub fn executor() -> Option<&'static Arc<dyn LivenessExecutor>> {
    EXECUTOR.get()
}

pub fn waits_for_agent_update(settings: &AppSettings, provider: &Provider) -> bool {
    EXECUTOR
        .get()
        .is_some_and(|executor| executor.waits_for_update(settings, provider))
}
