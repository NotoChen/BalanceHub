use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::{ProviderApiKeyOption, ProviderProtocol};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderApiKeyQuotaInput {
    pub unlimited: bool,
    pub amount: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProviderApiKeyExpiration {
    #[default]
    Never,
    At {
        timestamp: i64,
    },
    AfterDays {
        days: u32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderApiKeySpendingLimits {
    pub five_hours: f64,
    pub one_day: f64,
    pub seven_days: f64,
}

/// Only changed fields are sent on update. None never means "clear".
/// Explicit empty arrays/strings and Expiration::Never perform clearing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ProviderApiKeyPatch {
    pub name: Option<String>,
    pub group: Option<String>,
    pub quota: Option<ProviderApiKeyQuotaInput>,
    pub expiration: Option<ProviderApiKeyExpiration>,
    pub allow_ips: Option<Vec<String>>,
    pub deny_ips: Option<Vec<String>>,
    pub model_limits: Option<Vec<String>>,
    pub model_limits_enabled: Option<bool>,
    pub cross_group_retry: Option<bool>,
    pub auto_groups: Option<Vec<String>>,
    pub spending_limits: Option<ProviderApiKeySpendingLimits>,
    pub enabled: Option<bool>,
    pub custom_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeySettings {
    pub name: String,
    pub group: String,
    pub quota: ProviderApiKeyQuotaInput,
    pub expiration: ProviderApiKeyExpiration,
    pub allow_ips: Vec<String>,
    pub deny_ips: Vec<String>,
    pub model_limits: Vec<String>,
    pub model_limits_enabled: bool,
    pub cross_group_retry: bool,
    pub auto_groups: Vec<String>,
    pub spending_limits: ProviderApiKeySpendingLimits,
    pub enabled: bool,
}

impl ProviderApiKeySettings {
    pub(crate) fn from_option(option: &ProviderApiKeyOption, protocol: ProviderProtocol) -> Self {
        Self {
            name: option.name.clone(),
            group: if protocol == ProviderProtocol::Sub2Api {
                option.group_id.clone()
            } else {
                option.group.clone()
            },
            quota: ProviderApiKeyQuotaInput {
                unlimited: option.unlimited_quota,
                amount: if protocol == ProviderProtocol::Sub2Api {
                    option.total_quota
                } else {
                    option.remain_quota
                },
            },
            expiration: option
                .expired_time
                .filter(|value| *value > 0)
                .map(|timestamp| ProviderApiKeyExpiration::At {
                    timestamp: if protocol == ProviderProtocol::NewApi {
                        timestamp * 1000
                    } else {
                        timestamp
                    },
                })
                .unwrap_or_default(),
            allow_ips: option.allow_ips.clone(),
            deny_ips: option.deny_ips.clone(),
            model_limits: option.model_limits.clone(),
            model_limits_enabled: option.model_limits_enabled,
            cross_group_retry: option.cross_group_retry,
            auto_groups: option.auto_groups.clone(),
            spending_limits: option.spending_limits.clone(),
            enabled: matches!(option.status.as_str(), "1" | "enabled" | "active"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyGroup {
    pub value: String,
    pub label: String,
    pub description: String,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyAutoGroups {
    pub groups: Vec<String>,
    pub max_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiKeyEditorContext {
    pub credential_revision: u64,
    pub settings: ProviderApiKeySettings,
    pub groups: Vec<ProviderApiKeyGroup>,
    pub group_clearable: bool,
    pub default_group_label: String,
    pub quota_label: String,
    pub quota_unit: String,
    pub quota_minimum: f64,
    pub expiration_in_days: bool,
    pub supports_ip_blacklist: bool,
    pub supports_model_limits: bool,
    pub supports_cross_group_retry: bool,
    pub automatic_group: Option<String>,
    pub supports_spending_limits: bool,
    pub supports_custom_key: bool,
    pub auto_groups: Option<ProviderApiKeyAutoGroups>,
    pub model_options: Vec<String>,
    pub model_options_error: Option<String>,
}

impl ProviderApiKeyPatch {
    pub(crate) fn validate(
        &self,
        protocol: ProviderProtocol,
        creating: bool,
    ) -> Result<(), String> {
        if creating && self.name.as_ref().is_none_or(|name| name.trim().is_empty()) {
            return Err("请填写站点 Key 名称".into());
        }
        if let Some(name) = &self.name {
            if name.trim().is_empty()
                || (protocol == ProviderProtocol::NewApi && name.trim().len() > 50)
            {
                return Err("Key 名称不能为空；NewAPI 名称最多 50 字节".into());
            }
        }
        if let Some(quota) = &self.quota {
            validate_amount(quota.amount)?;
            if protocol == ProviderProtocol::Sub2Api && !quota.unlimited && quota.amount == 0.0 {
                return Err("Sub2API 的限额必须大于 0；不限额请打开无限额度".into());
            }
        }
        if let Some(limits) = &self.spending_limits {
            for amount in [limits.five_hours, limits.one_day, limits.seven_days] {
                validate_amount(amount)?;
            }
        }
        for list in [&self.allow_ips, &self.deny_ips].into_iter().flatten() {
            for pattern in list {
                validate_ip_pattern(pattern)?;
            }
        }
        if let Some(expiration) = &self.expiration {
            match expiration {
                ProviderApiKeyExpiration::At { timestamp }
                    if *timestamp <= crate::util::unix_millis() as i64 =>
                {
                    return Err("到期时间必须晚于当前时间".into())
                }
                ProviderApiKeyExpiration::AfterDays { days } if *days == 0 || *days > 36500 => {
                    return Err("有效天数应为 1 至 36500 天".into())
                }
                _ => {}
            }
            if matches!(expiration, ProviderApiKeyExpiration::AfterDays { .. })
                != (protocol == ProviderProtocol::Sub2Api && creating)
                && !matches!(expiration, ProviderApiKeyExpiration::Never)
            {
                return Err("当前操作不支持这种有效期设置".into());
            }
        }
        if protocol == ProviderProtocol::NewApi
            && (self.deny_ips.is_some()
                || self.spending_limits.is_some()
                || self.custom_key.is_some())
        {
            return Err("NewAPI 不支持 IP 黑名单、周期消费上限或自定义 Key".into());
        }
        if protocol == ProviderProtocol::Sub2Api
            && (self.model_limits.is_some()
                || self.model_limits_enabled.is_some()
                || self.cross_group_retry.is_some()
                || self.auto_groups.is_some())
        {
            return Err("Sub2API 不支持 Key 级模型限制或跨分组重试".into());
        }
        if let Some(key) = &self.custom_key {
            if !creating {
                return Err("已创建的 Key 不能修改密钥值".into());
            }
            if !key.is_empty()
                && (key.len() < 16
                    || key.len() > 128
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
            {
                return Err("自定义 Key 需 16 至 128 个字母、数字、下划线或连字符".into());
            }
        }
        if creating && self.enabled == Some(false) {
            return Err("请先创建 Key，再按需停用".into());
        }
        Ok(())
    }
}

fn validate_amount(amount: f64) -> Result<(), String> {
    if !amount.is_finite() || amount < 0.0 {
        Err("额度必须是非负有限数值".into())
    } else {
        Ok(())
    }
}

fn validate_ip_pattern(pattern: &str) -> Result<(), String> {
    let (address, prefix) = pattern
        .trim()
        .split_once('/')
        .map_or((pattern.trim(), None), |(address, prefix)| {
            (address, Some(prefix))
        });
    let address = address
        .parse::<IpAddr>()
        .map_err(|_| "IP 限制应填写有效的 IPv4、IPv6 或 CIDR 网段".to_string())?;
    if let Some(prefix) = prefix {
        let bits = prefix
            .parse::<u8>()
            .map_err(|_| "CIDR 网段前缀无效".to_string())?;
        if bits > if address.is_ipv4() { 32 } else { 128 } {
            return Err("CIDR 网段前缀超出地址范围".into());
        }
    }
    Ok(())
}

pub(crate) fn remote_api_key_id(id: &str) -> Result<i64, String> {
    id.trim()
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| "站点 Key ID 无效，请重新同步".into())
}
