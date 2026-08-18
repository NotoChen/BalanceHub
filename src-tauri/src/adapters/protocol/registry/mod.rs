use super::definition::ProviderProtocolDefinition;
use crate::models::ProviderProtocol;

macro_rules! register_protocol_definitions {
    (
        $default_variant:ident => { key: $default_key:literal, module: $default_module:ident }
        $(, $variant:ident => { key: $key:literal, module: $module:ident })*
        $(,)?
    ) => {
        mod $default_module;
        $(mod $module;)*

        const DEFINITIONS: &[ProviderProtocolDefinition] = &[
            $default_module::DEFINITION,
            $($module::DEFINITION,)*
        ];
    };
}

crate::provider_protocol_catalog::for_each_provider_protocol!(register_protocol_definitions);

pub(crate) fn definitions() -> &'static [ProviderProtocolDefinition] {
    debug_assert!(ProviderProtocol::ALL.iter().all(|kind| {
        DEFINITIONS
            .iter()
            .filter(|definition| definition.kind == *kind)
            .count()
            == 1
    }));
    DEFINITIONS
}

pub(crate) fn definition(kind: ProviderProtocol) -> &'static ProviderProtocolDefinition {
    DEFINITIONS
        .iter()
        .find(|definition| definition.kind == kind)
        .expect("every ProviderProtocol must have a registered definition")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::protocol::definition::ProtocolDetectionRole,
        models::{AuthMode, ProviderInput},
    };
    use std::collections::HashSet;

    #[test]
    fn every_protocol_has_one_registered_definition() {
        let kinds = definitions()
            .iter()
            .map(|definition| definition.kind)
            .collect::<HashSet<_>>();
        assert_eq!(kinds.len(), ProviderProtocol::ALL.len());
        assert!(ProviderProtocol::ALL
            .iter()
            .all(|kind| kinds.contains(kind)));
    }

    #[test]
    fn unsupported_protocol_features_are_absent_instead_of_error_adapters() {
        let generic = definition(ProviderProtocol::Api).capabilities();
        assert!(!generic.access_token);
        assert!(!generic.api_key_management);
        assert!(!generic.usage);
        assert!(!generic.account);
        assert!(!generic.check_in);
        assert!(!generic.announcements);

        let sub2 = definition(ProviderProtocol::Sub2Api).capabilities();
        assert!(!sub2.check_in);
        assert!(sub2.announcements);

        let new_api = definition(ProviderProtocol::NewApi).capabilities();
        assert!(new_api.access_token);
        assert!(new_api.api_key_management);
        assert!(new_api.usage);
        assert!(new_api.account);
        assert!(new_api.check_in);
        assert!(new_api.announcements);
    }

    #[test]
    fn protocol_auth_schemas_are_complete_and_unambiguous() {
        for definition in definitions() {
            let modes = definition
                .auth_schemas
                .iter()
                .map(|schema| schema.mode)
                .collect::<Vec<_>>();
            assert_eq!(
                modes
                    .iter()
                    .filter(
                        |mode| modes.iter().filter(|candidate| *candidate == *mode).count() == 1
                    )
                    .count(),
                modes.len(),
                "{} 存在重复认证模式",
                definition.label
            );
            assert!(
                modes.contains(&definition.default_auth_mode),
                "{} 的默认认证模式没有对应 Schema",
                definition.label
            );

            for schema in definition.auth_schemas {
                let fields = schema
                    .fields
                    .iter()
                    .map(|field| field.field)
                    .collect::<HashSet<_>>();
                assert_eq!(
                    fields.len(),
                    schema.fields.len(),
                    "{} / {} 存在重复字段",
                    definition.label,
                    schema.label
                );
                assert!(
                    schema
                        .required_fields
                        .iter()
                        .all(|field| fields.contains(field)),
                    "{} / {} 的必填字段没有渲染定义",
                    definition.label,
                    schema.label
                );
            }
        }
    }

    #[test]
    fn detection_roles_match_their_authentication_boundary() {
        let mut generic_input = ProviderInput::default();
        generic_input.identity.protocol = ProviderProtocol::Api;
        let mut generic = crate::models::Provider::from_input(generic_input, "generic".to_string());
        // Persisted input is normalized to API Key; force an invalid runtime state to
        // assert the detector still fails closed if that invariant is ever bypassed.
        generic.auth.mode = AuthMode::Password;
        assert_eq!(
            definition(ProviderProtocol::Api).detection_role,
            ProtocolDetectionRole::ApiKeyFallback
        );
        assert!(!definition(ProviderProtocol::Api).detection_enabled(&generic));

        let new_api =
            crate::models::Provider::from_input(ProviderInput::default(), "new-api".to_string());
        assert_eq!(
            definition(ProviderProtocol::NewApi).detection_role,
            ProtocolDetectionRole::Primary
        );
        assert!(definition(ProviderProtocol::NewApi).detection_enabled(&new_api));
    }

    #[test]
    fn descriptor_metadata_matches_registered_capabilities() {
        for definition in definitions() {
            let capabilities = definition.capabilities();
            assert_eq!(
                definition.operation_methods.check_in.is_some(),
                capabilities.check_in,
                "{} 的签到说明与能力注册不一致",
                definition.label
            );
            assert_eq!(
                definition.operation_methods.api_keys.is_some(),
                capabilities.api_key_management,
                "{} 的 API Key 说明与能力注册不一致",
                definition.label
            );
            assert_eq!(
                definition.operation_methods.invitation.is_some(),
                capabilities.account,
                "{} 的账号说明与能力注册不一致",
                definition.label
            );
            assert_eq!(
                definition.operation_methods.announcements.is_some(),
                capabilities.announcements,
                "{} 的公告说明与能力注册不一致",
                definition.label
            );
            assert!(
                !definition.operation_methods.models.trim().is_empty(),
                "{} 缺少模型刷新说明",
                definition.label
            );
            let has_access_token_flow = !matches!(
                definition.credential_assistant.access_token_flow,
                crate::adapters::protocol::definition::ProviderAccessTokenAssistantFlow::None
            );
            assert!(
                !has_access_token_flow || definition.credential_assistant.enabled,
                "{} 注册了凭据流程但未启用凭据助手",
                definition.label
            );
            if !definition.credential_assistant.enabled {
                assert!(
                    definition
                        .credential_assistant
                        .api_key_required_fields
                        .is_empty()
                        && definition
                            .credential_assistant
                            .api_key_required_any_fields
                            .is_empty(),
                    "{} 禁用凭据助手后仍声明了创建条件",
                    definition.label
                );
            }
        }
    }
}
