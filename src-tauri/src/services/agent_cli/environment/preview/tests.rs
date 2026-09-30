use super::*;
use crate::{
    models::{AgentAssetCategory, AgentAssetScope, AgentAssetSourceKind, AgentCliKind},
    services::agent_cli::{
        contracts::{AgentAssetLogicalOrigin, AgentAssetSourceSpec},
        definition,
    },
};
use std::path::PathBuf;

fn policy(kind: AgentCliKind, native_source_key: &str) -> AgentSourcePreviewPolicy {
    let source = AgentAssetSourceSpec {
        hook_definition_source: true,
        verified_physical_path: None,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: native_source_key.to_string(),
        label: "测试 schema".to_string(),
        scope: AgentAssetScope::User,
        path: PathBuf::from("/display-only/settings.json"),
        allowed_root: PathBuf::from("/display-only"),
        precedence: 10,
        writable: false,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![AgentAssetCategory::Mcp],
        allowed_logical_origins: vec![AgentAssetLogicalOrigin {
            scope: AgentAssetScope::User,
            precedence: 10,
        }],
    };
    definition(kind).environment().source_preview(&source)
}

#[test]
fn json_policy_keeps_safe_launcher_arguments_and_hides_credentials_urls_and_tainted_values() {
    let value = serde_json::json!({
        "mcpServers": {
            "safe": {
                "command":"node",
                "args":["server.js","--port","3000","--api-key","key-value","--token=token-value","--unknown=unknown-value","unknown-positional"],
                "url":"https://example.invalid/mcp?transport=stdio",
                "env":{"VISIBLE_NAME":"key-value","EXTRA":"env-value"},
                "headers":{"X-Custom":"header-value"},
                "enabled":true
            },
            "tainted": {
                "command":"command-secret",
                "args":["positional-secret"],
                "env":{"COPY":"command-secret"},
                "url":"https://example.invalid/%65%6e%76%2d%76%61%6c%75%65"
            },
            "unsafe": {
                "command":"sh",
                "args":["-c","echo command-line-secret"],
                "url":"https://private-user:password-value@example.invalid/mcp"
            },
            "query": {
                "url":"https://example.invalid/mcp?api_key=query-value"
            }
        },
        "customCredential":"custom-value"
    });
    let result = render_preview(
        policy(AgentCliKind::ClaudeCode, "workspace-mcp"),
        &serde_json::to_vec(&value).unwrap(),
    );
    assert!(result.metadata_reason.is_none());
    assert!(result.redacted);
    let output = result.content.unwrap();
    for safe in [
        "node",
        "server.js",
        "--port",
        "3000",
        "transport=stdio",
        "VISIBLE_NAME",
        "X-Custom",
    ] {
        assert!(output.contains(safe), "safe schema field missing: {safe}");
    }
    for secret in [
        "key-value",
        "token-value",
        "unknown-value",
        "unknown-positional",
        "env-value",
        "header-value",
        "command-secret",
        "positional-secret",
        "command-line-secret",
        "password-value",
        "query-value",
        "custom-value",
    ] {
        assert!(
            !output.contains(secret),
            "credential escaped schema preview"
        );
    }
    assert!(!output.contains("%65%6e%76"));
}

#[test]
fn toml_policies_mask_credential_containers_without_blanket_hiding_mcp_parameters() {
    let bytes = br#"
[mcp_servers.fixture]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "--cwd", "/safe/workspace", "--password=toml-secret"]
enabled = true
url = "https://example.invalid/mcp"
[mcp_servers.fixture.env]
PUBLIC_NAME = "toml-env-secret"
[mcp_servers.fixture.http_headers]
X-Custom = "toml-header-secret"
[unknown]
futureCredential = "future-secret"
"#;
    for kind in [AgentCliKind::Codex, AgentCliKind::Grok] {
        let result = render_preview(policy(kind, "config"), bytes);
        let output = result.content.expect("native TOML policy");
        for safe in [
            "npx",
            "server-filesystem",
            "/safe/workspace",
            "example.invalid",
            "PUBLIC_NAME",
            "X-Custom",
        ] {
            assert!(output.contains(safe), "missing safe field for {kind:?}");
        }
        for secret in [
            "toml-secret",
            "toml-env-secret",
            "toml-header-secret",
            "future-secret",
        ] {
            assert!(!output.contains(secret));
        }
        assert!(output.parse::<toml::Value>().is_ok());
    }
}

#[test]
fn unknown_argv_grammar_never_exposes_free_text_or_an_unknown_option_value_as_a_package() {
    let value = serde_json::json!({"mcpServers":{
        "unknown":{"command":"custom-server","args":["arbitrary-secret","--secret","another-secret"]},
        "npx":{"command":"npx","args":["--unknown-option","package-shaped-secret","another-package-secret"]},
        "shell":{"command":"node --api-key raw-command-secret","args":["script.js","--port","1000"]}
    }});
    let result = render_preview(
        policy(AgentCliKind::ClaudeCode, "workspace-mcp"),
        &serde_json::to_vec(&value).unwrap(),
    );
    let output = result.content.unwrap();
    for secret in [
        "arbitrary-secret",
        "another-secret",
        "package-shaped-secret",
        "another-package-secret",
        "raw-command-secret",
    ] {
        assert!(!output.contains(secret));
    }
}

#[test]
fn native_source_selection_does_not_guess_schema_from_display_filename() {
    for kind in [
        AgentCliKind::Codex,
        AgentCliKind::ClaudeCode,
        AgentCliKind::Gemini,
        AgentCliKind::Grok,
    ] {
        let result = render_preview(
            policy(kind, "unrecognized-source"),
            br#"{"apiKey":"must-stay-private"}"#,
        );
        assert_eq!(
            result.metadata_reason,
            Some(AgentPreviewMetadataReason::UnsupportedSchema)
        );
        assert!(result.content.is_none());
        let auth = render_preview(policy(kind, "auth"), b"free credential text");
        assert!(auth.content.is_none());
    }
}

#[test]
fn malformed_duplicate_invalid_utf8_and_plain_documents_fail_to_metadata() {
    let json_policy = policy(AgentCliKind::ClaudeCode, "workspace-mcp");
    for bytes in [
        br#"{"mcpServers":{"entry":{"command":"node","command":"duplicate-secret"}}}"#.as_slice(),
        br#"{"a":1,"a":"duplicate-secret"}"#.as_slice(),
        br#"{"unfinished":"private-secret""#.as_slice(),
        b"free text private-secret".as_slice(),
        b"\xff\xfeprivate-secret".as_slice(),
        b"[]".as_slice(),
    ] {
        let result = render_preview(json_policy, bytes);
        assert_eq!(
            result.metadata_reason,
            Some(AgentPreviewMetadataReason::InvalidDocument)
        );
        assert!(result.content.is_none());
    }
    let toml = render_preview(
        policy(AgentCliKind::Codex, "config"),
        b"key='first'\nkey='duplicate-secret'",
    );
    assert_eq!(
        toml.metadata_reason,
        Some(AgentPreviewMetadataReason::InvalidDocument)
    );
}

#[test]
fn complete_document_is_redacted_before_ui_truncation() {
    let mut servers = serde_json::Map::new();
    for index in 0..1200 {
        servers.insert(
            format!("{index:04}-{}", "display-name".repeat(8)),
            serde_json::json!({
                "command": if index == 0 { "secret-at-document-end" } else { "node" },
                "args": ["server.js", "--port", "3000"],
                "enabled": true
            }),
        );
    }
    let value = serde_json::json!({
        "mcpServers":servers,
        "zzPrivate":{"token":"secret-at-document-end"}
    });
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() > MAX_PREVIEW_BYTES);
    let result = render_preview(policy(AgentCliKind::ClaudeCode, "workspace-mcp"), &bytes);
    assert!(result.truncated);
    let content = result.content.unwrap();
    assert!(content.len() <= MAX_PREVIEW_BYTES);
    assert!(!content.contains("secret-at-document-end"));
    assert!(content.contains(REDACTED));
}

#[test]
fn account_and_enablement_policies_reveal_only_their_native_schema_fields() {
    let account = render_preview(
        policy(AgentCliKind::ClaudeCode, "account"),
        br#"{
        "oauthAccount":{"emailAddress":"account-secret","accessToken":"oauth-secret"},
        "mcpServers":{"fixture":{"command":"node","args":["server.js","--port","3000"]}},
        "projects":{"/workspace":{"mcpServers":{"local":{"command":"node","args":["local.js"]}}}}
    }"#,
    )
    .content
    .unwrap();
    assert!(account.contains("3000"));
    assert!(account.contains("local.js"));
    assert!(!account.contains("account-secret"));
    assert!(!account.contains("oauth-secret"));
    let enablement = render_preview(
        policy(AgentCliKind::Gemini, "mcp-enablement"),
        br#"{
        "fixture":{"enabled":true,"unknownCredential":"enablement-secret"}
    }"#,
    )
    .content
    .unwrap();
    assert!(enablement.contains("true"));
    assert!(!enablement.contains("enablement-secret"));
}
