//! 签到命令。服务端没有可显示的签到窗口，窗口操作会明确报错。

use serde_json::Value;

use super::{arg, to_value};
use crate::{models::CheckInSource, services::check_in_tasks};

pub const COMMANDS: &[&str] = &[
    "check_in_provider",
    "check_in_all_providers",
    "list_check_in_tasks",
    "cancel_check_in_task",
    "resume_check_in_task",
    "show_check_in_window",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "check_in_provider" => {
            let id: String = arg(args, "id")?;
            to_value(check_in_tasks::enqueue(id, CheckInSource::Manual, None, 0)?)
        }
        "check_in_all_providers" => to_value(check_in_tasks::enqueue_all()?),
        "list_check_in_tasks" => to_value(check_in_tasks::list()),
        "cancel_check_in_task" => {
            let run_id: String = arg(args, "runId")?;
            check_in_tasks::cancel(&run_id)?;
            Ok(Value::Null)
        }
        "resume_check_in_task" => {
            let run_id: String = arg(args, "runId")?;
            to_value(check_in_tasks::resume(&run_id)?)
        }
        "show_check_in_window" => {
            let run_id: String = arg(args, "runId")?;
            check_in_tasks::show_window(&run_id).await?;
            Ok(Value::Null)
        }
        _ => Err(format!("未实现的签到命令：{command}")),
    }
}
