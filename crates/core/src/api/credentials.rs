//! 站点凭据详情：读取、清除与验证。
//!
//! 凭据保存在核心数据里，读取和网络验证都不依赖桌面窗口，浏览器版同样可用。

use serde_json::Value;

use super::{arg, blocking, to_value};
use crate::services::provider_service::{CredentialKind, ProviderService};

pub const COMMANDS: &[&str] = &[
    "get_provider_credentials",
    "read_provider_credential",
    "clear_provider_credential",
    "validate_provider_credentials",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    let service = ProviderService::new();
    match command {
        "get_provider_credentials" => {
            let id: String = arg(args, "id")?;
            let details = blocking(move || service.credential_details(&id)).await?;
            to_value(details)
        }
        "read_provider_credential" => {
            let id: String = arg(args, "id")?;
            let kind: CredentialKind = arg(args, "kind")?;
            let revision: u64 = arg(args, "revision")?;
            let content =
                blocking(move || ProviderService::new().read_credential(&id, kind, revision))
                    .await?;
            to_value(content)
        }
        "clear_provider_credential" => {
            let id: String = arg(args, "id")?;
            let kind: CredentialKind = arg(args, "kind")?;
            let revision: u64 = arg(args, "revision")?;
            service.clear_credential(id, kind, revision).await?;
            Ok(Value::Null)
        }
        "validate_provider_credentials" => {
            let id: String = arg(args, "id")?;
            to_value(service.validate_credentials(id).await?)
        }
        _ => Err(format!("未实现的凭据命令：{command}")),
    }
}
