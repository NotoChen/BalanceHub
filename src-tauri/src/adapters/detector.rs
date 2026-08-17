use crate::models::{
    AppSettings, Provider, ProviderProtocolDetectionResult, ProviderSiteProbeResult,
};
use futures_util::future::join_all;

use super::protocol::{definitions, ProtocolDetectionRole, ProviderProtocolDefinition};

pub(crate) struct ProtocolDetector;

struct DetectionProbe {
    definition: &'static ProviderProtocolDefinition,
    result: Option<ProviderSiteProbeResult>,
}

impl ProtocolDetector {
    pub(crate) async fn detect(
        &self,
        settings: &AppSettings,
        provider: &Provider,
    ) -> ProviderProtocolDetectionResult {
        let probes = join_all(
            definitions()
                .iter()
                .filter(|definition| definition.detection_enabled(provider))
                .map(|definition| {
                    let mut candidate = provider.clone();
                    candidate.identity.protocol = definition.kind;
                    async move {
                        DetectionProbe {
                            definition,
                            result: definition
                                .connection()
                                .probe_site(settings, &candidate)
                                .await
                                .ok(),
                        }
                    }
                }),
        )
        .await;

        resolve_detection(probes)
    }
}

fn resolve_detection(probes: Vec<DetectionProbe>) -> ProviderProtocolDetectionResult {
    let primary_matches = successful_matches(&probes, ProtocolDetectionRole::Primary);
    match primary_matches.as_slice() {
        [(definition, site)] => ProviderProtocolDetectionResult {
            detected_protocol: Some(definition.kind),
            message: format!("已识别为 {}", definition.label),
            site: Some((*site).clone()),
            ambiguous: false,
        },
        [] => resolve_fallback(&probes),
        matches => ProviderProtocolDetectionResult {
            detected_protocol: None,
            message: format!(
                "站点同时匹配 {}，请手动选择协议",
                matches
                    .iter()
                    .map(|(definition, _)| definition.label)
                    .collect::<Vec<_>>()
                    .join(" 和 ")
            ),
            site: None,
            ambiguous: true,
        },
    }
}

fn resolve_fallback(probes: &[DetectionProbe]) -> ProviderProtocolDetectionResult {
    let fallback_matches = successful_matches(probes, ProtocolDetectionRole::ApiKeyFallback);
    match fallback_matches.as_slice() {
        [(definition, site)] => ProviderProtocolDetectionResult {
            detected_protocol: Some(definition.kind),
            message: format!(
                "未识别为账号协议，已通过模型接口识别为 {}",
                definition.label
            ),
            site: Some((*site).clone()),
            ambiguous: false,
        },
        [] => {
            let fallback_error = probes
                .iter()
                .filter(|probe| {
                    probe.definition.detection_role == ProtocolDetectionRole::ApiKeyFallback
                })
                .filter_map(|probe| {
                    probe
                        .result
                        .as_ref()
                        .filter(|result| !result.ok)
                        .map(|result| (probe.definition.label, result.message.trim()))
                })
                .find(|(_, message)| !message.is_empty())
                .map(|(label, message)| format!("；{label} 验证失败：{message}"))
                .unwrap_or_default();
            ProviderProtocolDetectionResult {
                detected_protocol: None,
                message: if fallback_error.is_empty() {
                    "无法识别中转站协议，请手动选择".to_string()
                } else {
                    format!("无法识别中转站协议{fallback_error}")
                },
                site: None,
                ambiguous: false,
            }
        }
        matches => ProviderProtocolDetectionResult {
            detected_protocol: None,
            message: format!(
                "模型接口同时匹配 {}，请手动选择协议",
                matches
                    .iter()
                    .map(|(definition, _)| definition.label)
                    .collect::<Vec<_>>()
                    .join(" 和 ")
            ),
            site: None,
            ambiguous: true,
        },
    }
}

fn successful_matches(
    probes: &[DetectionProbe],
    role: ProtocolDetectionRole,
) -> Vec<(
    &'static ProviderProtocolDefinition,
    &ProviderSiteProbeResult,
)> {
    probes
        .iter()
        .filter(|probe| probe.definition.detection_role == role)
        .filter_map(|probe| {
            probe
                .result
                .as_ref()
                .filter(|result| result.ok)
                .map(|site| (probe.definition, site))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::protocol::definition,
        models::{ProviderProtocol, ProviderQuotaDisplay},
    };

    fn site(name: &str) -> ProviderSiteProbeResult {
        ProviderSiteProbeResult {
            ok: true,
            message: "ok".to_string(),
            system_name: Some(name.to_string()),
            logo: None,
            quota_display: ProviderQuotaDisplay::default(),
        }
    }

    fn probe(protocol: ProviderProtocol, result: ProviderSiteProbeResult) -> DetectionProbe {
        DetectionProbe {
            definition: definition(protocol),
            result: Some(result),
        }
    }

    #[test]
    fn resolves_a_single_protocol_match() {
        let detected = resolve_detection(vec![probe(ProviderProtocol::NewApi, site("NewAPI"))]);
        assert_eq!(detected.detected_protocol, Some(ProviderProtocol::NewApi));
        assert!(!detected.ambiguous);
    }

    #[test]
    fn reports_ambiguous_primary_matches() {
        let ambiguous = resolve_detection(vec![
            probe(ProviderProtocol::NewApi, site("A")),
            probe(ProviderProtocol::Sub2Api, site("B")),
            probe(ProviderProtocol::Api, site("Generic API")),
        ]);
        assert!(ambiguous.detected_protocol.is_none());
        assert!(ambiguous.ambiguous);
        assert!(ambiguous.message.contains("NewAPI 和 Sub2API"));
    }

    #[test]
    fn unknown_api_key_sites_fall_back_to_generic_protocol() {
        let generic = resolve_detection(vec![probe(ProviderProtocol::Api, site("Generic API"))]);
        assert_eq!(generic.detected_protocol, Some(ProviderProtocol::Api));
        assert!(!generic.ambiguous);

        let rejected = resolve_detection(vec![probe(
            ProviderProtocol::Api,
            ProviderSiteProbeResult {
                ok: false,
                message: "HTTP 401".to_string(),
                system_name: None,
                logo: None,
                quota_display: ProviderQuotaDisplay::default(),
            },
        )]);
        assert!(rejected.detected_protocol.is_none());
        assert!(rejected.message.contains("HTTP 401"));
    }
}
