use super::{files, format, transport::normalize_url};
use crate::models::{CloudSyncSettingsInput, CloudSyncSettingsView};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Settings {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub passphrase: String,
    pub remote_root: String,
    pub device_name: String,
    pub auto_sync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            username: String::new(),
            password: String::new(),
            passphrase: String::new(),
            remote_root: "BalanceHub".to_owned(),
            device_name: "这台电脑".to_owned(),
            auto_sync: false,
        }
    }
}

impl Settings {
    pub fn load(root: &Path) -> Result<Self, String> {
        Ok(files::read_json(&root.join("settings.json"))?.unwrap_or_default())
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        files::write_json(&root.join("settings.json"), self)
    }

    pub fn view(&self) -> CloudSyncSettingsView {
        CloudSyncSettingsView {
            server_url: self.server_url.clone(),
            username: self.username.clone(),
            remote_root: self.remote_root.clone(),
            device_name: self.device_name.clone(),
            auto_sync: self.auto_sync,
            has_password: !self.password.is_empty(),
            has_passphrase: !self.passphrase.is_empty(),
        }
    }

    pub fn update(&self, input: CloudSyncSettingsInput) -> Result<Self, String> {
        let server_url = input.server_url.trim().to_owned();
        let username = input.username.trim().to_owned();
        if (server_url != self.server_url || username != self.username)
            && !self.password.is_empty()
            && input.password.is_none()
        {
            return Err("更换服务地址或账号后，请重新输入 WebDAV 应用密码".to_owned());
        }
        let next = Self {
            server_url,
            username,
            password: input.password.unwrap_or_else(|| self.password.clone()),
            passphrase: input.passphrase.unwrap_or_else(|| self.passphrase.clone()),
            remote_root: input.remote_root.trim().trim_matches('/').to_owned(),
            device_name: input.device_name.trim().to_owned(),
            auto_sync: input.auto_sync,
        };
        if next.device_name.is_empty() || next.device_name.chars().count() > 80 {
            return Err("请填写不超过 80 字的设备名称".to_owned());
        }
        if next.username.len() > 1024
            || next.password.len() > 8192
            || next.passphrase.len() > 8192
            || next.server_url.len() > 4096
            || next.remote_root.len() > 1024
        {
            return Err("同步配置输入过长".to_owned());
        }
        if !next.server_url.is_empty() {
            normalize_url(&next.server_url, &next.remote_root)?;
        }
        if !next.passphrase.is_empty() && next.passphrase.chars().count() < 12 {
            return Err("同步密码至少需要 12 个字符；其他设备连接时使用同一个密码".to_owned());
        }
        if next.auto_sync {
            next.ready()?;
        }
        Ok(next)
    }

    pub fn ready(&self) -> Result<(), String> {
        normalize_url(&self.server_url, &self.remote_root)?;
        if self.passphrase.chars().count() < 12 {
            return Err("请先设置至少 12 个字符的同步密码".to_owned());
        }
        Ok(())
    }

    /// Credentials are not part of a replica's identity. Refreshing an app
    /// password must keep the common ancestor and offline deletion history.
    pub fn space_id(&self) -> Result<String, String> {
        let url = normalize_url(&self.server_url, &self.remote_root)?;
        Ok(format::digest(
            format!("{url}\n{}", self.username).as_bytes(),
        ))
    }

    pub fn verification_id(&self) -> Result<String, String> {
        Ok(format::digest(
            format!("{}\n{}", self.space_id()?, self.password).as_bytes(),
        ))
    }
}
