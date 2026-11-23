/// 系统时钟被回拨后，持久化的「上次同步 / 冷却结束 / 下次尝试」会落在
/// 未来。超过这个容差的未来时刻只可能是时钟问题，不应按真实进度等待。
pub(crate) const CLOCK_ROLLBACK_TOLERANCE_SECS: u64 = 10 * 60;

#[path = "provider_domain/auth.rs"]
pub mod auth;
#[path = "provider_domain/automation.rs"]
pub mod automation;
#[path = "provider_domain/capabilities.rs"]
pub mod capabilities;
#[path = "provider_domain/check_in.rs"]
pub mod check_in;
#[path = "provider_domain/endpoints.rs"]
pub mod endpoints;
#[path = "provider_domain/failure.rs"]
pub mod failure;
#[path = "provider_domain/liveness.rs"]
pub mod liveness;
#[path = "provider_domain/model_list.rs"]
pub mod model_list;
