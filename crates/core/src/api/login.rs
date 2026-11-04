//! 登录账号与站点浏览器登录命令。
//!
//! 登录窗口由浏览器组件执行，桌面与服务端共用；带桌面会话的服务端同样能
//! 打开登录窗口并导入账号，无显示环境只保留账号管理。

use serde_json::Value;

use super::{arg, blocking, to_value};
use crate::{
    models::{LoginPlatform, ProviderInput},
    services::{
        provider_browser_login::{self, ProviderBrowserLoginTask},
        provider_service::ProviderService,
    },
};

pub const COMMANDS: &[&str] = &[
    "start_provider_browser_login",
    "list_provider_browser_logins",
    "cancel_provider_browser_login",
    "show_provider_login_window",
    "list_login_accounts",
    "create_login_account",
    "update_login_account",
    "remove_login_account",
    "open_login_account",
    "list_login_account_cookies",
    "read_login_account_cookie",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "start_provider_browser_login" => {
            let input: ProviderInput = arg(args, "input")?;
            let login_account_id: String = arg(args, "loginAccountId")?;
            let task =
                blocking(move || provider_browser_login::start(input, login_account_id)).await?;
            to_value(task)
        }
        "list_provider_browser_logins" => {
            let tasks: Vec<ProviderBrowserLoginTask> =
                blocking(provider_browser_login::list).await?;
            to_value(tasks)
        }
        "cancel_provider_browser_login" => {
            let run_id: String = arg(args, "runId")?;
            blocking(move || provider_browser_login::cancel(&run_id)).await?;
            Ok(Value::Null)
        }
        "show_provider_login_window" => {
            let run_id: String = arg(args, "runId")?;
            provider_browser_login::show_window(&run_id).await?;
            Ok(Value::Null)
        }
        "list_login_accounts" => {
            let accounts = blocking(|| ProviderService::new().list_login_accounts()).await?;
            to_value(accounts)
        }
        "create_login_account" => {
            let name: String = arg(args, "name")?;
            let platform: LoginPlatform = arg(args, "platform")?;
            let account =
                blocking(move || ProviderService::new().create_login_account(name, platform))
                    .await?;
            to_value(account)
        }
        "update_login_account" => {
            let id: String = arg(args, "id")?;
            let name: String = arg(args, "name")?;
            let platform: LoginPlatform = arg(args, "platform")?;
            blocking(move || ProviderService::new().update_login_account(id, name, platform))
                .await?;
            Ok(Value::Null)
        }
        "remove_login_account" => {
            let id: String = arg(args, "id")?;
            let remove_entry: bool = arg(args, "removeEntry")?;
            blocking(move || ProviderService::new().remove_login_account(id, remove_entry)).await?;
            Ok(Value::Null)
        }
        "open_login_account" => {
            let id: String = arg(args, "id")?;
            let authorizations: bool = arg(args, "authorizations")?;
            let task =
                blocking(move || provider_browser_login::start_account(id, authorizations)).await?;
            to_value(task)
        }
        "list_login_account_cookies" => {
            let id: String = arg(args, "id")?;
            let cookies =
                blocking(move || ProviderService::new().login_account_cookies(&id)).await?;
            to_value(cookies)
        }
        "read_login_account_cookie" => {
            let id: String = arg(args, "id")?;
            let cookie_id: String = arg(args, "cookieId")?;
            let value =
                blocking(move || ProviderService::new().read_login_cookie(&id, &cookie_id)).await?;
            to_value(value)
        }
        _ => Err(format!("当前宿主不支持命令：{command}")),
    }
}
