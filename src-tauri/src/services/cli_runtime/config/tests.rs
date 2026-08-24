use super::*;
use crate::models::{AuthMode, ProviderInput, ProviderProtocol};
use crate::services::agent_cli::config_support::{
    cli_target_for_key, match_provider_key, normalize_endpoint, CliConfigProviderMatch,
};

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
    let mut provider = relay_provider();
    provider
        .add_named_api_key("sk-second", "备用 Key")
        .expect("second key should be added");
    let second_local_id = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key == "sk-second")
        .expect("second key option")
        .local_id
        .clone();
    let default_local_id = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key == "sk-test")
        .expect("default key option")
        .local_id
        .clone();

    assert_eq!(
        match_provider_key(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://relay.example.com/v1/",
            "sk-test",
        ),
        Some(CliConfigProviderMatch {
            provider_id: "provider-test".to_string(),
            api_key_local_id: default_local_id,
        })
    );
    assert_eq!(
        match_provider_key(
            std::slice::from_ref(&provider),
            AgentCliKind::ClaudeCode,
            "https://relay.example.com",
            "sk-other",
        ),
        None
    );
    assert_eq!(
        match_provider_key(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://relay.example.com/v1",
            "sk-second",
        ),
        Some(CliConfigProviderMatch {
            provider_id: "provider-test".to_string(),
            api_key_local_id: second_local_id,
        })
    );
}

#[test]
fn cli_target_for_key_selects_a_non_default_key_by_stable_local_id() {
    let mut provider = relay_provider();
    provider
        .add_named_api_key("sk-second", "备用 Key")
        .expect("second key should be added");
    let second = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key == "sk-second")
        .expect("second key option")
        .clone();

    let target = cli_target_for_key(&provider, AgentCliKind::Codex, &second.local_id)
        .expect("second key should be selected");
    assert_eq!(target.api_key, "sk-second");
    assert_eq!(target.api_key_local_id, second.local_id);
    assert_eq!(target.api_key_label, "备用 Key");
}

#[test]
fn cli_target_for_key_rejects_missing_or_unreadable_selected_keys() {
    let mut provider = relay_provider();
    provider
        .add_named_api_key("sk-second", "备用 Key")
        .expect("second key should be added");
    let second_local_id = provider
        .auth
        .api_key_options
        .iter()
        .find(|option| option.key == "sk-second")
        .expect("second key option")
        .local_id
        .clone();

    let missing = cli_target_for_key(&provider, AgentCliKind::Codex, "does-not-exist")
        .expect_err("unknown key should fail");
    assert_eq!(missing, "所选 API Key 已不存在，请重新选择");

    let option = provider
        .auth
        .api_key_options
        .iter_mut()
        .find(|option| option.local_id == second_local_id)
        .expect("second key option");
    option.key = "sk-****".to_string();
    option.key_available = false;
    let unreadable = cli_target_for_key(&provider, AgentCliKind::Codex, &second_local_id)
        .expect_err("masked key should fail");
    assert_eq!(unreadable, "所选 API Key 未读取到完整值，无法切换 CLI 配置");
}

#[test]
fn generic_provider_keeps_an_arbitrary_api_key_for_cli_config() {
    let mut input = ProviderInput::default();
    input.identity.base_url = "https://generic.example.com".to_string();
    input.identity.protocol = ProviderProtocol::Api;
    input.auth.mode = AuthMode::ApiKey;
    input.auth.api_key = "gsk_custom-key".to_string();
    let provider = Provider::from_input(input, "generic-provider".to_string());

    let target = cli_target_for_key(&provider, AgentCliKind::Codex, "").unwrap();
    assert_eq!(target.api_key, "gsk_custom-key");
    assert_eq!(
        match_provider_key(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://generic.example.com/v1",
            "gsk_custom-key",
        ),
        Some(CliConfigProviderMatch {
            provider_id: "generic-provider".to_string(),
            api_key_local_id: provider.auth.api_key_options[0].local_id.clone(),
        })
    );
    assert_eq!(
        match_provider_key(
            std::slice::from_ref(&provider),
            AgentCliKind::Codex,
            "https://generic.example.com/v1",
            "sk-gsk_custom-key",
        ),
        None
    );
}
