use balancehub_core::state::CoreState;
use std::{ops::Deref, sync::Arc};

use crate::models::AppData;

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
