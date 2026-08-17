use super::*;
use crate::models::{AuthMode, ProviderInput, ProviderProtocol};
use crate::services::agent_cli::config_support::{cli_target, match_provider, normalize_endpoint};

fn relay_provider() -> Provider {
    Provider::from_input(
        ProviderInput {
            identity: crate::models::ProviderIdentityInput {
                name: "Relay".to_string(),
                base_url: "https://relay.example.com".to_string(),
                ..crate::models::ProviderIdentityInput::default()
            },
            auth: crate::models::ProviderAuth {
                mode: AuthMode::ApiKey,
                api_key: "sk-test".to_string(),
                ..ProviderInput::default().auth
            },
            ..ProviderInput::default()
        },
        "provider-test".to_string(),
    )
}

#[test]
fn endpoint_normalization_ignores_trailing_slashes_and_url_case_rules() {
    assert_eq!(
        normalize_endpoint("HTTPS://Relay.Example.COM/v1/"),
        normalize_endpoint("https://relay.example.com/v1")
    );
}

#[test]
fn provider_match_requires_the_effective_url_and_api_key() {
    let provider = relay_provider();

    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://relay.example.com/v1/",
            "sk-test",
        ),
        Some("provider-test".to_string())
    );
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            AgentCliKind::ClaudeCode,
            "https://relay.example.com",
            "sk-other",
        ),
        None
    );
}

#[test]
fn generic_provider_keeps_an_arbitrary_api_key_for_cli_config() {
    let mut input = ProviderInput::default();
    input.identity.base_url = "https://generic.example.com".to_string();
    input.identity.protocol = ProviderProtocol::Api;
    input.auth.mode = AuthMode::ApiKey;
    input.auth.api_key = "gsk_custom-key".to_string();
    let provider = Provider::from_input(input, "generic-provider".to_string());

    let (_, key) = cli_target(&provider, AgentCliKind::Codex).unwrap();
    assert_eq!(key, "gsk_custom-key");
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://generic.example.com/v1",
            "gsk_custom-key",
        ),
        Some("generic-provider".to_string())
    );
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://generic.example.com/v1",
            "sk-gsk_custom-key",
        ),
        None
    );
}
