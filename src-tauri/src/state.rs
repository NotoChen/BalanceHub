use balancehub_core::state::CoreState;
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::models::AppData;

/// 前端一旦观察到刷新、签到、测活或 Agent 任务，就把标记同步到这里。
/// 关闭窗口或点菜单退出时先看这一位，避免在用户不知情的情况下中断工作。
#[derive(Default)]
pub struct ExitGuardState(AtomicBool);

impl ExitGuardState {
    pub fn set_running(&self, running: bool) {
        self.0.store(running, Ordering::Release);
    }

    pub fn running(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// 桌面进程的应用状态。
///
/// 配置数据、刷新闸门和版本号属于 `balancehub-core`；Agent 服务由
/// `balancehub-agent` 持有进程级注册表，不再占用宿主状态。
/// `Deref` 让现有 `state.data`、`state.next_revision()` 之类的写法继续成立。
pub struct AppState {
    pub core: Arc<CoreState>,
}

impl Deref for AppState {
    type Target = CoreState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl AppState {
    pub fn new(data: AppData) -> Self {
        Self::with_load_error(data, None)
    }

    pub fn with_load_error(data: AppData, load_error: Option<String>) -> Self {
        Self {
            core: Arc::new(CoreState::with_load_error(data, load_error)),
        }
    }
}
