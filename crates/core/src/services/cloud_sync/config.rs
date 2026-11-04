use super::{files, format, s3, transport::normalize_url};
use crate::models::{CloudSyncBackend, CloudSyncSettingsInput, CloudSyncSettingsView};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Settings {
    pub backend: CloudSyncBackend,
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub passphrase: String,
    /// WebDAV directory or S3 object prefix.
    pub remote_root: String,
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub device_name: String,
    pub auto_sync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backend: CloudSyncBackend::Webdav,
            server_url: String::new(),
            username: String::new(),
            password: String::new(),
            passphrase: String::new(),
            remote_root: "BalanceHub".to_owned(),
            s3_endpoint: String::new(),
            s3_region: "us-east-1".to_owned(),
            s3_bucket: String::new(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
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
            backend: self.backend,
            server_url: self.server_url.clone(),
            username: self.username.clone(),
            remote_root: self.remote_root.clone(),
            s3_endpoint: self.s3_endpoint.clone(),
            s3_region: self.s3_region.clone(),
            s3_bucket: self.s3_bucket.clone(),
            s3_access_key: self.s3_access_key.clone(),
            device_name: self.device_name.clone(),
            auto_sync: self.auto_sync,
            has_password: !self.password.is_empty(),
            has_secret_key: !self.s3_secret_key.is_empty(),
            has_passphrase: !self.passphrase.is_empty(),
        }
    }

    pub fn update(&self, input: CloudSyncSettingsInput) -> Result<Self, String> {
        let backend = input.backend;
        let server_url = input.server_url.trim().to_owned();
        let username = input.username.trim().to_owned();
        let s3_endpoint = input.s3_endpoint.trim().to_owned();
        let s3_bucket = input.s3_bucket.trim().to_owned();
        let s3_access_key = input.s3_access_key.trim().to_owned();
        let password_provided = input.password.is_some();
        let secret_provided = input.s3_secret_key.is_some();
        let next = Self {
            backend,
            server_url,
            username,
            password: input.password.unwrap_or_else(|| self.password.clone()),
            passphrase: input.passphrase.unwrap_or_else(|| self.passphrase.clone()),
            remote_root: input.remote_root.trim().trim_matches('/').to_owned(),
            s3_endpoint,
            s3_region: input.s3_region.trim().to_owned(),
            s3_bucket,
            s3_access_key,
            s3_secret_key: input
                .s3_secret_key
                .unwrap_or_else(|| self.s3_secret_key.clone()),
            device_name: input.device_name.trim().to_owned(),
            auto_sync: input.auto_sync,
        };
        // Switching backends keeps both credentials, but a changed endpoint,
        // account or bucket must be confirmed with its secret again.
        let (identity_changed, saved_secret, provided_secret) = match backend {
            CloudSyncBackend::Webdav => (
                next.server_url != self.server_url || next.username != self.username,
                &self.password,
                password_provided,
            ),
            CloudSyncBackend::S3 => (
                next.s3_endpoint != self.s3_endpoint
                    || next.s3_bucket != self.s3_bucket
                    || next.s3_access_key != self.s3_access_key,
                &self.s3_secret_key,
                secret_provided,
            ),
        };
        if identity_changed && !saved_secret.is_empty() && !provided_secret {
            return Err(match backend {
                CloudSyncBackend::Webdav => {
                    "更换服务地址或账号后，请重新输入 WebDAV 应用密码".to_owned()
                }
                CloudSyncBackend::S3 => {
                    "更换 S3 服务地址、存储桶或 Access Key 后，请重新输入 Secret Key".to_owned()
                }
            });
        }
        if next.device_name.is_empty() || next.device_name.chars().count() > 80 {
            return Err("请填写不超过 80 字的设备名称".to_owned());
        }
        if next.username.len() > 1024
            || next.password.len() > 8192
            || next.passphrase.len() > 8192
            || next.server_url.len() > 4096
            || next.remote_root.len() > 1024
            || next.s3_endpoint.len() > 4096
            || next.s3_region.len() > 128
            || next.s3_bucket.len() > 128
            || next.s3_access_key.len() > 1024
            || next.s3_secret_key.len() > 8192
        {
            return Err("同步配置输入过长".to_owned());
        }
        if next.remote_configured() {
            next.validate_remote()?;
        }
        if !next.passphrase.is_empty() && next.passphrase.chars().count() < 12 {
            return Err("同步密码至少需要 12 个字符；其他设备连接时使用同一个密码".to_owned());
        }
        if next.auto_sync {
            next.ready()?;
        }
        Ok(next)
    }

    fn remote_configured(&self) -> bool {
        match self.backend {
            CloudSyncBackend::Webdav => !self.server_url.is_empty(),
            CloudSyncBackend::S3 => {
                !self.s3_bucket.is_empty()
                    || !self.s3_endpoint.is_empty()
                    || !self.s3_access_key.is_empty()
            }
        }
    }

    pub fn validate_remote(&self) -> Result<(), String> {
        match self.backend {
            CloudSyncBackend::Webdav => {
                normalize_url(&self.server_url, &self.remote_root)?;
                Ok(())
            }
            CloudSyncBackend::S3 => s3::validate(
                &self.s3_endpoint,
                &self.s3_bucket,
                &self.remote_root,
                &self.s3_region,
                &self.s3_access_key,
            ),
        }
    }

    pub fn ready(&self) -> Result<(), String> {
        if !self.remote_configured() {
            return Err("请先填写云端存储连接信息".to_owned());
        }
        self.validate_remote()?;
        if self.passphrase.chars().count() < 12 {
            return Err("请先设置至少 12 个字符的同步密码".to_owned());
        }
        Ok(())
    }

    /// Credentials are not part of a replica's identity. Refreshing an app
    /// password must keep the common ancestor and offline deletion history.
    pub fn space_id(&self) -> Result<String, String> {
        Ok(format::digest(
            match self.backend {
                CloudSyncBackend::Webdav => {
                    format!(
                        "webdav\n{}\n{}",
                        normalize_url(&self.server_url, &self.remote_root)?,
                        self.username
                    )
                }
                CloudSyncBackend::S3 => {
                    let endpoint = s3::normalize_endpoint(&self.s3_endpoint, &self.s3_region)?;
                    format!(
                        "s3\n{}\n{}\n{}\n{}\n{}",
                        endpoint.as_str().trim_end_matches('/'),
                        self.s3_bucket,
                        s3::normalize_prefix(&self.remote_root)?,
                        self.s3_region,
                        self.s3_access_key
                    )
                }
            }
            .as_bytes(),
        ))
    }

    pub fn verification_id(&self) -> Result<String, String> {
        Ok(format::digest(
            format!(
                "{}\n{}",
                self.space_id()?,
                match self.backend {
                    CloudSyncBackend::Webdav => &self.password,
                    CloudSyncBackend::S3 => &self.s3_secret_key,
                }
            )
            .as_bytes(),
        ))
    }
}
