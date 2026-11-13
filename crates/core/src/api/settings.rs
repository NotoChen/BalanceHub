//! 应用设置、配置读写、模型资料与通知命令。

use serde_json::{json, Value};

use super::{arg, optional, to_value};
use crate::{
    contracts::{AppDataImportResultView, AppDataView},
    models::{AppSettings, ModelMetadataSnapshot},
    services::{notifications, provider_service::ProviderService},
};

pub const COMMANDS: &[&str] = &[
    "load_app_data",
    "save_settings",
    "get_model_metadata",
    "send_app_notification",
    "host_platform",
    "get_host_capabilities",
    "preview_liveness_prompts",
    "export_app_data_json",
    "import_app_data_json",
];

pub async fn handle(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "load_app_data" => {
            let data = ProviderService::new().load_app_data()?;
            to_value(AppDataView::from(data))
        }
        "save_settings" => {
            let settings: AppSettings = arg(args, "settings")?;
            let expected: AppSettings = arg(args, "expected")?;
            let saved = crate::api::blocking(move || {
                ProviderService::new().save_settings(settings, expected)
            })
            .await?;
            crate::host::refresh_tray();
            to_value(saved)
        }
        "get_model_metadata" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let refresh: bool = optional(args, "refresh")?.unwrap_or(false);
            let provider_id: Option<String> = optional(args, "providerId")?;
            if ids.len() > 2000 || ids.iter().any(|id| id.len() > 512) {
                return Err("模型资料查询数量或名称超出范围".to_owned());
            }
            let state = crate::host::core_state();
            let (model_config, site_pricing, settings) = {
                let data = state.data.read().unwrap_or_else(|error| error.into_inner());
                let provider = provider_id.and_then(|id| {
                    data.providers
                        .iter()
                        .find(|provider| provider.identity.id == id)
                });
                let model_config = provider.map(|provider| provider.model_config.clone());
                let site_pricing = provider.map(|provider| provider.capabilities.pricing.clone());
                (model_config, site_pricing, data.settings.clone())
            };
            let service = state.model_catalog()?;
            let snapshot: ModelMetadataSnapshot =
                service.snapshot_for(&ids, model_config.as_ref(), site_pricing.as_ref())?;
            if refresh || snapshot.updated_at.is_none() {
                let _ = service.refresh(&settings, refresh).await;
                let current =
                    service.snapshot_for(&ids, model_config.as_ref(), site_pricing.as_ref())?;
                if current.updated_at != snapshot.updated_at {
                    crate::host::emit("model-metadata-updated", json!(null));
                }
                return to_value(current);
            }
            if snapshot.stale {
                let updated_at = snapshot.updated_at.clone();
                let service_config = model_config.clone();
                let service_pricing = site_pricing.clone();
                let service = std::sync::Arc::clone(&service);
                crate::runtime::spawn(async move {
                    let _ = service.refresh(&settings, false).await;
                    if service
                        .snapshot_for(&[], service_config.as_ref(), service_pricing.as_ref())
                        .is_ok_and(|current| current.updated_at != updated_at)
                    {
                        crate::host::emit("model-metadata-updated", json!(null));
                    }
                });
            }
            to_value(snapshot)
        }
        "send_app_notification" => {
            let settings: AppSettings = arg(args, "settings")?;
            // 前端传入的是卡片上的完整 Provider 快照，直接按其结构反序列化。
            let provider: Option<crate::models::Provider> = optional(args, "provider")?;
            let title: String = arg(args, "title")?;
            let markdown: String = arg(args, "markdown")?;
            let ignore_switch: bool = optional(args, "ignoreSwitch")?.unwrap_or(false);
            let result = match provider {
                Some(provider) => {
                    notifications::send_provider_notification(
                        &settings,
                        &provider,
                        crate::models::NotificationEvent::Manual,
                        title,
                        markdown,
                        ignore_switch,
                    )
                    .await
                }
                None => {
                    notifications::send_configured_notification(
                        &settings,
                        title,
                        markdown,
                        ignore_switch,
                    )
                    .await
                }
            };
            to_value(result)
        }
        "host_platform" => Ok(json!(std::env::consts::OS)),
        "preview_liveness_prompts" => {
            let settings: AppSettings = arg(args, "settings")?;
            let count: usize = arg(args, "count")?;
            to_value(crate::services::liveness::preview_prompts(&settings, count))
        }
        "export_app_data_json" => to_value(ProviderService::new().export_app_data_json()?),
        "import_app_data_json" => {
            let text: String = arg(args, "text")?;
            let (data, transfer) =
                crate::api::blocking(move || ProviderService::new().import_app_data_json(text))
                    .await?;
            crate::host::refresh_tray();
            to_value(AppDataImportResultView {
                data: AppDataView::from(data),
                transfer,
            })
        }
        "get_host_capabilities" => Ok(json!({
            "host": crate::host::current().kind(),
            "version": env!("CARGO_PKG_VERSION"),
            "commands": visible_commands(crate::host::has_desktop_session()),
        })),
        _ => Err(format!("未实现的应用命令：{command}")),
    }
}

/// 站点浏览器登录需要可交互的浏览器窗口；无显示环境里命令仍然存在，但入口
/// 应当隐藏，避免用户点开一个必然失败的任务。
fn visible_commands(desktop_session: bool) -> Vec<&'static str> {
    super::commands()
        .into_iter()
        .filter(|command| {
            desktop_session
                || !matches!(
                    *command,
                    "start_provider_browser_login"
                        | "list_provider_browser_logins"
                        | "cancel_provider_browser_login"
                        | "show_provider_login_window"
                        | "open_login_account"
                )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::visible_commands;

    #[test]
    fn browser_login_is_hidden_without_a_desktop_session() {
        let headless = visible_commands(false);
        assert!(!headless.contains(&"start_provider_browser_login"));
        assert!(!headless.contains(&"open_login_account"));
        assert!(headless.contains(&"list_login_accounts"));
        assert!(visible_commands(true).contains(&"start_provider_browser_login"));
    }
}
