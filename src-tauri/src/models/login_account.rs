use serde::{Deserialize, Serialize};

pub(crate) fn valid_login_account_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginPlatform {
    LinuxDo,
    Github,
    Other,
    #[default]
    Unknown,
}

impl LoginPlatform {
    pub fn label(self) -> &'static str {
        match self {
            Self::LinuxDo => "Linux DO",
            Self::Github => "GitHub",
            Self::Other => "站点账号 / 其他平台",
            Self::Unknown => "平台尚未确认",
        }
    }

    pub fn account_url(self, authorizations: bool) -> Option<&'static str> {
        match (self, authorizations) {
            (Self::LinuxDo, true) => Some("https://connect.linux.do/"),
            (Self::LinuxDo, false) => Some("https://linux.do/"),
            (Self::Github, true) => Some("https://github.com/settings/applications"),
            (Self::Github, false) => Some("https://github.com/login"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserLoginMechanism {
    Oauth,
    Password,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLoginBinding {
    pub account_id: Option<String>,
    pub platform: LoginPlatform,
    pub mechanism: BrowserLoginMechanism,
    pub imported_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAccount {
    pub id: String,
    pub name: String,
    pub platform: LoginPlatform,
    pub identity: Option<String>,
    pub created_at: i64,
    pub last_opened_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub identity_observed_at: Option<i64>,
    pub generation: u64,
}

impl LoginAccount {
    pub fn new(id: String, name: String, platform: LoginPlatform, now: i64) -> Self {
        Self {
            id,
            name,
            platform,
            identity: None,
            created_at: now,
            last_opened_at: None,
            last_used_at: None,
            identity_observed_at: None,
            generation: 0,
        }
    }
}
