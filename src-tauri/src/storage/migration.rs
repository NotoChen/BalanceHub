use crate::{
    limits,
    models::{AppData, CURRENT_SCHEMA_VERSION},
    util::read_text_file_limited,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const DATA_FILE_NAME: &str = "data.json";

/// 迁移前是否备份原文件：主配置/恢复候选会被迁移结果覆盖，需要备份；
/// 导入的外部文件不会被改写，备份只会在来源目录留垃圾。
#[derive(Clone, Copy, PartialEq)]
pub(super) enum BackupBeforeMigrate {
    Yes,
    No,
}

/// 读取并按需迁移一个配置文件。返回 `(数据, 是否发生了迁移)`，迁移过的数据
/// 由调用方决定何时落盘。
pub(super) fn read_app_data_file(
    path: &Path,
    backup_mode: BackupBeforeMigrate,
) -> Result<(AppData, bool), String> {
    let text = read_text_file_limited(path, limits::MAX_APP_DATA_FILE_BYTES, "读取配置")?;
    let stored_version = stored_schema_version(&text)
        .map_err(|error| format!("解析配置失败({}): {error}", path.display()))?;

    if stored_version == CURRENT_SCHEMA_VERSION {
        let data = serde_json::from_str::<AppData>(&text)
            .map_err(|error| format!("解析配置失败({}): {error}", path.display()))?;
        return Ok((data, false));
    }
    if stored_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "配置结构版本过新：当前应用只支持 schemaVersion {CURRENT_SCHEMA_VERSION}，检测到 {stored_version}。请升级应用后再使用该配置。"
        ));
    }

    let backup = if backup_mode == BackupBeforeMigrate::Yes {
        backup_legacy_file(path, stored_version, &text)
    } else {
        None
    };
    let backup_hint = backup
        .as_ref()
        .map(|backup_path| format!("，原文件已备份至 {}", backup_path.display()))
        .unwrap_or_default();
    let data = migrate_app_data(&text, stored_version).map_err(|error| {
        format!(
            "配置从 schemaVersion {stored_version} 迁移到 {CURRENT_SCHEMA_VERSION} 失败：{error}{backup_hint}"
        )
    })?;
    Ok((data, true))
}

fn stored_schema_version(text: &str) -> Result<u32, String> {
    let value =
        serde_json::from_str::<serde_json::Value>(text).map_err(|error| error.to_string())?;
    Ok(value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32)
}

pub(super) fn migrate_app_data(text: &str, stored_version: u32) -> Result<AppData, String> {
    let mut value =
        serde_json::from_str::<serde_json::Value>(text).map_err(|error| error.to_string())?;
    for version in stored_version..CURRENT_SCHEMA_VERSION {
        migrate_step(version, &mut value)?;
    }
    value["schemaVersion"] = serde_json::Value::from(CURRENT_SCHEMA_VERSION);
    serde_json::from_value::<AppData>(value).map_err(|error| error.to_string())
}

fn migrate_step(version: u32, data: &mut serde_json::Value) -> Result<(), String> {
    match version {
        1 | 2 => Ok(()),
        3 => {
            data["workspaces"] = serde_json::Value::Array(Vec::new());
            Ok(())
        }
        4 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let auth = provider
                        .as_object_mut()
                        .map(|object| {
                            object
                                .entry("auth")
                                .or_insert_with(|| serde_json::json!({}))
                        })
                        .and_then(serde_json::Value::as_object_mut);
                    if let Some(auth) = auth {
                        auth.entry("loginUsername")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                        auth.entry("loginPassword")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                    }
                }
            }
            Ok(())
        }
        5 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let auth = provider
                        .as_object_mut()
                        .map(|object| {
                            object
                                .entry("auth")
                                .or_insert_with(|| serde_json::json!({}))
                        })
                        .and_then(serde_json::Value::as_object_mut);
                    if let Some(auth) = auth {
                        auth.entry("apiKeyTokenId")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                        auth.entry("apiKeyOptions")
                            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                    }
                }
            }
            Ok(())
        }
        6 => {
            if let Some(settings) = data
                .get_mut("settings")
                .and_then(serde_json::Value::as_object_mut)
            {
                let removed_terminal_mode = settings
                    .get("temporaryCliTerminalKind")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| matches!(kind, "auto" | "systemDefault" | "custom"));
                if removed_terminal_mode {
                    settings.insert(
                        "temporaryCliTerminalKind".to_string(),
                        serde_json::to_value(crate::models::TemporaryCliTerminalKind::default())
                            .map_err(|error| format!("生成默认终端配置失败: {error}"))?,
                    );
                }
                settings.remove("temporaryCliTerminalCommand");
            }
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(|value| value.as_array_mut())
            {
                for provider in providers {
                    let Some(object) = provider.as_object_mut() else {
                        continue;
                    };
                    if let Some(identity) = object
                        .get_mut("identity")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        identity
                            .entry("protocol")
                            .or_insert_with(|| serde_json::Value::String("newApi".to_string()));
                    }
                    if let Some(auth) = object
                        .get_mut("auth")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        let source = if auth.get("mode").and_then(serde_json::Value::as_str)
                            == Some("password")
                        {
                            "password"
                        } else {
                            "manual"
                        };
                        auth.entry("source")
                            .or_insert_with(|| serde_json::Value::String(source.to_string()));
                    }
                }
            }
            Ok(())
        }
        7 => {
            if let Some(settings) = data
                .get_mut("settings")
                .and_then(serde_json::Value::as_object_mut)
            {
                let mut paths = settings
                    .remove("agentCliPaths")
                    .and_then(|value| value.as_object().cloned())
                    .unwrap_or_default();
                for (legacy_key, agent_key) in
                    [("codexCliPath", "codex"), ("claudeCliPath", "claudeCode")]
                {
                    if let Some(path) = settings
                        .remove(legacy_key)
                        .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
                        .filter(|path| !path.is_empty())
                    {
                        paths
                            .entry(agent_key.to_string())
                            .or_insert_with(|| serde_json::Value::String(path));
                    }
                }
                settings.insert(
                    "agentCliPaths".to_string(),
                    serde_json::Value::Object(paths),
                );
            }
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(serde_json::Value::as_array_mut)
            {
                for provider in providers {
                    let Some(liveness) = provider
                        .get_mut("liveness")
                        .and_then(serde_json::Value::as_object_mut)
                    else {
                        continue;
                    };
                    let mut base_urls = liveness
                        .remove("agentBaseUrls")
                        .and_then(|value| value.as_object().cloned())
                        .unwrap_or_default();
                    for (legacy_key, agent_key) in [
                        ("openaiBaseUrl", "codex"),
                        ("anthropicBaseUrl", "claudeCode"),
                    ] {
                        if let Some(url) = liveness
                            .remove(legacy_key)
                            .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
                            .filter(|url| !url.is_empty())
                        {
                            base_urls
                                .entry(agent_key.to_string())
                                .or_insert_with(|| serde_json::Value::String(url));
                        }
                    }
                    liveness.insert(
                        "agentBaseUrls".to_string(),
                        serde_json::Value::Object(base_urls),
                    );
                }
            }
            Ok(())
        }
        8 => {
            if let Some(settings) = data
                .get_mut("settings")
                .and_then(serde_json::Value::as_object_mut)
            {
                settings
                    .entry("sessionIndexEnabled")
                    .or_insert_with(|| serde_json::Value::Bool(true));
                settings
                    .entry("sessionIndexDirectory")
                    .or_insert_with(|| serde_json::Value::String(String::new()));
                settings.entry("sessionIndexMaxSizeMiB").or_insert_with(|| {
                    serde_json::Value::from(crate::models::default_session_index_max_size_mib())
                });
            }
            Ok(())
        }
        9 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(serde_json::Value::as_array_mut)
            {
                for provider in providers {
                    let Some(options) = provider
                        .get_mut("auth")
                        .and_then(|auth| auth.get_mut("apiKeyOptions"))
                        .and_then(serde_json::Value::as_array_mut)
                    else {
                        continue;
                    };
                    for option in options {
                        let Some(option) = option.as_object_mut() else {
                            continue;
                        };
                        option
                            .entry("localId")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                        option
                            .entry("localName")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                    }
                }
            }
            if let Some(preferences) = data
                .get_mut("temporaryCliPreferences")
                .and_then(serde_json::Value::as_array_mut)
            {
                for preference in preferences {
                    let Some(preference) = preference.as_object_mut() else {
                        continue;
                    };
                    let legacy = preference
                        .remove("apiKeyTokenId")
                        .unwrap_or_else(|| serde_json::Value::String(String::new()));
                    preference.entry("apiKeyLocalId").or_insert(legacy);
                }
            }
            Ok(())
        }
        10 => {
            if let Some(providers) = data
                .get_mut("providers")
                .and_then(serde_json::Value::as_array_mut)
            {
                for provider in providers {
                    let Some(object) = provider.as_object_mut() else {
                        continue;
                    };
                    object
                        .entry("identity")
                        .or_insert_with(|| serde_json::json!({}));
                    if let Some(identity) = object
                        .get_mut("identity")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        identity
                            .entry("remark")
                            .or_insert_with(|| serde_json::Value::String(String::new()));
                    }
                }
            }
            Ok(())
        }
        other => Err(format!(
            "没有从 schemaVersion {other} 出发的迁移路径，请重新初始化配置或导入新版配置"
        )),
    }
}

pub(super) fn backup_legacy_file(path: &Path, version: u32, text: &str) -> Option<PathBuf> {
    let backup_path = path.with_file_name(format!("{DATA_FILE_NAME}.v{version}.bak"));
    if backup_path.exists() {
        return Some(backup_path);
    }
    fs::write(&backup_path, text).ok()?;
    Some(backup_path)
}
