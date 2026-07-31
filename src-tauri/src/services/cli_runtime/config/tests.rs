use super::*;
use crate::models::{AuthMode, ProviderInput, ProviderProtocol};

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
fn parses_selected_codex_provider_and_auth_file() {
    let parsed = parse_codex_config(
        r#"
model_provider = "relay"

[model_providers.relay]
base_url = "https://relay.example.com/v1"
"#,
        r#"{"OPENAI_API_KEY":"sk-test"}"#,
    )
    .expect("config should parse")
    .expect("config should be complete");

    assert_eq!(parsed.0, "https://relay.example.com/v1");
    assert_eq!(parsed.1, "sk-test");
}

#[test]
fn parses_claude_settings_env() {
    let parsed = parse_claude_config(
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://relay.example.com","ANTHROPIC_AUTH_TOKEN":"sk-test"}}"#,
        )
        .expect("settings should parse")
        .expect("settings should be complete");

    assert_eq!(parsed.0, "https://relay.example.com");
    assert_eq!(parsed.1, "sk-test");
}

#[test]
fn codex_switch_only_updates_selected_provider_url_and_api_key() {
    let config = r#"model_provider = "relay"
model = "gpt-test"

[model_providers.relay]
name = "Relay"
base_url = "https://old.example.com/v1"
wire_api = "responses"

[mcp_servers.local]
command = "node"
"#;
    let auth = r#"{"OPENAI_API_KEY":"sk-old","tokens":{"access":"keep"}}"#;

    let (config, auth) =
        rewrite_codex_config(config, auth, "https://new.example.com/v1", "sk-new").unwrap();

    assert!(config.contains("base_url = \"https://new.example.com/v1\""));
    assert!(config.contains("model = \"gpt-test\""));
    assert!(config.contains("[mcp_servers.local]"));
    let auth = serde_json::from_str::<JsonValue>(&auth).unwrap();
    assert_eq!(auth["OPENAI_API_KEY"], "sk-new");
    assert_eq!(auth["tokens"]["access"], "keep");
}

#[test]
fn claude_switch_preserves_other_settings_and_updates_existing_key_fields() {
    let settings = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://old.example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-old",
    "ANTHROPIC_API_KEY": "sk-old-api",
    "KEEP_ME": "yes"
  },
  "permissions": { "defaultMode": "bypassPermissions" }
}"#;

    let settings = rewrite_claude_config(settings, "https://new.example.com", "sk-new").unwrap();
    let settings = serde_json::from_str::<JsonValue>(&settings).unwrap();

    assert_eq!(
        settings["env"]["ANTHROPIC_BASE_URL"],
        "https://new.example.com"
    );
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-new");
    assert_eq!(settings["env"]["ANTHROPIC_API_KEY"], "sk-new");
    assert_eq!(settings["env"]["KEEP_ME"], "yes");
    assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
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
            LivenessCliKind::Codex,
            "https://relay.example.com/v1/",
            "sk-test",
        ),
        Some("provider-test".to_string())
    );
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            LivenessCliKind::ClaudeCode,
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

    let (_, key) = cli_target(&provider, LivenessCliKind::Codex).unwrap();
    assert_eq!(key, "gsk_custom-key");
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            LivenessCliKind::Codex,
            "https://generic.example.com/v1",
            "gsk_custom-key",
        ),
        Some("generic-provider".to_string())
    );
    assert_eq!(
        match_provider(
            std::slice::from_ref(&provider),
            LivenessCliKind::Codex,
            "https://generic.example.com/v1",
            "sk-gsk_custom-key",
        ),
        None
    );
}
