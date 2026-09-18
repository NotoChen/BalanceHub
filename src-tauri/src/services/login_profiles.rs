//! Local browser profile storage. Paths are derived solely from validated IDs.
use crate::{
    models::{valid_login_account_id, LoginPlatform},
    util::read_text_file_limited,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub(crate) fn directory(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_login_account_id(id) {
        return Err("登录账号标识无效".into());
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位登录数据目录")?
        .join("login-browser")
        .join(id))
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
}

impl ProfileCookie {
    pub(crate) fn id(&self) -> String {
        serde_json::json!([self.domain, self.path, self.name]).to_string()
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdentitySnapshot {
    #[serde(default)]
    pub platform: LoginPlatform,
    pub identity: Option<String>,
    pub observed_at: Option<i64>,
}

pub(crate) fn cookies(app: &AppHandle, id: &str) -> Result<Vec<ProfileCookie>, String> {
    let path = directory(app, id)?.join("identity-cookies.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = read_text_file_limited(&path, 4 * 1024 * 1024, "读取账号 Cookie")?;
    serde_json::from_str(&text).map_err(|_| "本地账号 Cookie 文件格式无效".into())
}

pub(crate) fn identity(app: &AppHandle, id: &str) -> Result<IdentitySnapshot, String> {
    let path = directory(app, id)?.join("identity-state.json");
    if !path.exists() {
        return Ok(IdentitySnapshot::default());
    }
    let text = read_text_file_limited(&path, 64 * 1024, "读取账号身份")?;
    serde_json::from_str(&text).map_err(|_| "本地账号身份文件格式无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_ids_cannot_escape_the_login_directory() {
        for value in ["", "..", "../profile", "a/b", "a\\b", "/tmp", "a.b"] {
            assert!(!valid_login_account_id(value));
        }
        assert!(valid_login_account_id("account-1-2"));
        assert!(valid_login_account_id("profile"));
    }
}
