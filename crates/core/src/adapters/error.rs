//! 适配器错误的统一形态：文案用于展示，分类用于自动重试决策。
//!
//! 只有真正拿到 HTTP 状态码或上游明确拒绝的路径才构造带分类的错误；
//! 其它错误一律按瞬时失败处理，避免把展示文案当作业务信号。

use reqwest::header::HeaderValue;
use reqwest::StatusCode;

use crate::models::provider_domain::failure::ProviderFailureKind;

#[derive(Debug)]
pub struct AdapterError {
    pub message: String,
    pub failure: ProviderFailureKind,
}

impl AdapterError {
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            failure: ProviderFailureKind::Transient,
        }
    }

    pub fn credential(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            failure: ProviderFailureKind::Credential,
        }
    }

    /// 按响应状态分类：401/403 视为凭据被拒，429 尊重 Retry-After，其余瞬时。
    pub fn from_status(
        message: impl Into<String>,
        status: StatusCode,
        retry_after: Option<&HeaderValue>,
    ) -> Self {
        let failure = match status.as_u16() {
            401 | 403 => ProviderFailureKind::Credential,
            429 => ProviderFailureKind::RateLimited {
                retry_after_secs: retry_after
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok()),
            },
            _ => ProviderFailureKind::Transient,
        };
        Self {
            message: message.into(),
            failure,
        }
    }
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<AdapterError> for String {
    fn from(error: AdapterError) -> Self {
        error.message
    }
}

/// 适配器里大量 `?` 仍以字符串收口（超时、客户端构建、URL 解析）；这些
/// 都不是凭据问题，统一按瞬时失败处理。
impl From<String> for AdapterError {
    fn from(message: String) -> Self {
        Self::transient(message)
    }
}
