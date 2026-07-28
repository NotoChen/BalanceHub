use crate::models::{
    AppSettings, AuthMode, Provider, ProviderProtocol, ProviderProtocolDetectionResult,
    ProviderSiteProbeResult,
};

use super::{api::ApiAdapter, new_api::NewApiAdapter, sub2_api::Sub2ApiAdapter};

pub(crate) struct ProtocolDetector;

impl ProtocolDetector {
    pub(crate) async fn detect(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderProtocolDetectionResult {
        let generic_api = async {
            if matches!(provider.auth.mode, AuthMode::ApiKey) {
                Some(ApiAdapter.probe_site(settings, provider).await)
            } else {
                None
            }
        };
        let (new_api, sub2_api, generic_api) = tokio::join!(
            NewApiAdapter.probe_site(settings, provider),
            Sub2ApiAdapter.probe_site(settings, provider),
            generic_api,
        );

        apply_api_key_fallback(
            resolve_detection(new_api.ok(), sub2_api.ok()),
            provider.auth.mode,
            generic_api.and_then(Result::ok),
        )
    }
}

fn apply_api_key_fallback(
    detected: ProviderProtocolDetectionResult,
    auth_mode: AuthMode,
    generic_api: Option<ProviderSiteProbeResult>,
) -> ProviderProtocolDetectionResult {
    if matches!(auth_mode, AuthMode::ApiKey)
        && detected.detected_protocol.is_none()
        && !detected.ambiguous
    {
        if let Some(site) = generic_api.as_ref().filter(|site| site.ok) {
            return ProviderProtocolDetectionResult {
                detected_protocol: Some(ProviderProtocol::Api),
                message: "未识别为 NewAPI/Sub2API，已通过模型接口识别为通用 API".to_string(),
                site: Some(site.clone()),
                ambiguous: false,
            };
        }
        let detail = generic_api
            .as_ref()
            .map(|site| site.message.clone())
            .filter(|message| !message.trim().is_empty())
            .map(|message| format!("；通用 API 验证失败：{message}"))
            .unwrap_or_default();
        return ProviderProtocolDetectionResult {
            detected_protocol: None,
            message: format!("无法识别中转站协议{detail}"),
            site: None,
            ambiguous: false,
        };
    }
    detected
}

fn resolve_detection(
    new_api: Option<ProviderSiteProbeResult>,
    sub2_api: Option<ProviderSiteProbeResult>,
) -> ProviderProtocolDetectionResult {
    let new_api = new_api.filter(|result| result.ok);
    let sub2_api = sub2_api.filter(|result| result.ok);

    match (new_api, sub2_api) {
        (Some(_), Some(_)) => ProviderProtocolDetectionResult {
            detected_protocol: None,
            message: "站点同时匹配 NewAPI 和 Sub2API，请手动选择协议".to_string(),
            site: None,
            ambiguous: true,
        },
        (Some(site), None) => ProviderProtocolDetectionResult {
            detected_protocol: Some(ProviderProtocol::NewApi),
            message: "已识别为 NewAPI".to_string(),
            site: Some(site),
            ambiguous: false,
        },
        (None, Some(site)) => ProviderProtocolDetectionResult {
            detected_protocol: Some(ProviderProtocol::Sub2Api),
            message: "已识别为 Sub2API".to_string(),
            site: Some(site),
            ambiguous: false,
        },
        (None, None) => ProviderProtocolDetectionResult {
            detected_protocol: None,
            message: "无法识别中转站协议，请手动选择".to_string(),
            site: None,
            ambiguous: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderQuotaDisplay;

    fn site(name: &str) -> ProviderSiteProbeResult {
        ProviderSiteProbeResult {
            ok: true,
            message: "ok".to_string(),
            system_name: Some(name.to_string()),
            logo: None,
            quota_display: ProviderQuotaDisplay::default(),
        }
    }

    #[test]
    fn resolves_a_single_protocol_match() {
        let detected = resolve_detection(Some(site("NewAPI")), None);
        assert_eq!(detected.detected_protocol, Some(ProviderProtocol::NewApi));
        assert!(!detected.ambiguous);

        let detected = resolve_detection(None, Some(site("Sub2API")));
        assert_eq!(detected.detected_protocol, Some(ProviderProtocol::Sub2Api));
        assert!(!detected.ambiguous);
    }

    #[test]
    fn reports_ambiguous_and_unknown_results() {
        let ambiguous = resolve_detection(Some(site("A")), Some(site("B")));
        assert!(ambiguous.detected_protocol.is_none());
        assert!(ambiguous.ambiguous);

        let unknown = resolve_detection(None, None);
        assert!(unknown.detected_protocol.is_none());
        assert!(!unknown.ambiguous);
    }

    #[test]
    fn unknown_api_key_sites_fall_back_to_generic_protocol() {
        let generic = apply_api_key_fallback(
            resolve_detection(None, None),
            AuthMode::ApiKey,
            Some(site("Generic API")),
        );
        assert_eq!(generic.detected_protocol, Some(ProviderProtocol::Api));
        assert!(!generic.ambiguous);

        let rejected = apply_api_key_fallback(
            resolve_detection(None, None),
            AuthMode::ApiKey,
            Some(ProviderSiteProbeResult {
                ok: false,
                message: "HTTP 401".to_string(),
                system_name: None,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            }),
        );
        assert!(rejected.detected_protocol.is_none());
        assert!(rejected.message.contains("HTTP 401"));

        let account =
            apply_api_key_fallback(resolve_detection(None, None), AuthMode::Password, None);
        assert!(account.detected_protocol.is_none());

        let ambiguous = apply_api_key_fallback(
            resolve_detection(Some(site("A")), Some(site("B"))),
            AuthMode::ApiKey,
            Some(site("Generic API")),
        );
        assert!(ambiguous.detected_protocol.is_none());
        assert!(ambiguous.ambiguous);
    }
}
