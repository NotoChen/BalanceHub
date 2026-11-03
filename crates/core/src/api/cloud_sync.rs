//! WebDAV 配置同步命令。桌面与服务端共用同一个同步服务实例。

use serde_json::Value;

use super::{arg, to_value};
use crate::{
    models::{CloudSyncResolution, CloudSyncSettingsInput},
    services::cloud_sync::{service, Action},
};

pub const COMMANDS: &[&str] = &[
    "cloud_sync_state",
    "cloud_sync_save",
    "cloud_sync_start",
    "cloud_sync_test",
    "cloud_sync_confirm",
    "cloud_sync_restore",
    "cloud_sync_cancel",
    "cloud_sync_compare",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    let service = service()?;
    match command {
        "cloud_sync_state" => to_value(service.snapshot()),
        "cloud_sync_save" => {
            let input: CloudSyncSettingsInput = arg(args, "input")?;
            to_value(service.save(input)?)
        }
        "cloud_sync_start" => to_value(service.start(Action::Sync, false)?),
        "cloud_sync_test" => to_value(service.start(Action::Test, false)?),
        "cloud_sync_confirm" => {
            let review_id: String = arg(args, "reviewId")?;
            let resolutions: Vec<CloudSyncResolution> = arg(args, "resolutions")?;
            to_value(service.start(
                Action::Confirm {
                    review_id,
                    resolutions,
                },
                false,
            )?)
        }
        "cloud_sync_restore" => to_value(service.start(Action::Restore, false)?),
        "cloud_sync_cancel" => {
            let task_id: String = arg(args, "taskId")?;
            to_value(service.cancel(&task_id)?)
        }
        "cloud_sync_compare" => {
            let review_id: String = arg(args, "reviewId")?;
            let key: String = arg(args, "key")?;
            to_value(service.compare(&review_id, &key)?)
        }
        _ => Err(format!("未实现的同步命令：{command}")),
    }
}
