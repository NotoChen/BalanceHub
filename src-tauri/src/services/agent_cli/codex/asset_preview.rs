use crate::services::agent_cli::{
    contracts::AgentAssetSourceSpec,
    environment::preview::{
        AgentArgvPolicy, AgentArgvValueKind as V, AgentArgvValueRule, AgentPreviewFormat,
        AgentPreviewPathSegment::{AnyIndex as I, AnyKey as A, Key as K},
        AgentPreviewPolicy, AgentPreviewRule as R, AgentPreviewScalarKind as S,
        AgentSourcePreviewPolicy,
    },
};

const ARGV: AgentArgvPolicy = AgentArgvPolicy {
    command_field: "command",
    launchers: &[
        "node",
        "node.exe",
        "python",
        "python3",
        "python.exe",
        "python3.exe",
        "npx",
        "npx.cmd",
        "uvx",
        "uvx.exe",
    ],
    boolean_flags: &[
        "-y",
        "--yes",
        "--stdio",
        "--verbose",
        "--quiet",
        "--debug",
        "--help",
        "--version",
    ],
    value_flags: &[
        AgentArgvValueRule {
            flag: "--port",
            kind: V::Integer,
        },
        AgentArgvValueRule {
            flag: "--timeout",
            kind: V::Integer,
        },
        AgentArgvValueRule {
            flag: "--host",
            kind: V::Host,
        },
        AgentArgvValueRule {
            flag: "--cwd",
            kind: V::Path,
        },
        AgentArgvValueRule {
            flag: "--url",
            kind: V::Url,
        },
        AgentArgvValueRule {
            flag: "--transport",
            kind: V::Enum(&["stdio", "http", "sse"]),
        },
        AgentArgvValueRule {
            flag: "--log-level",
            kind: V::Enum(&["trace", "debug", "info", "warn", "error"]),
        },
    ],
    credential_flags: &[
        "--api-key",
        "--apikey",
        "--apiKey",
        "--key",
        "--token",
        "--access-token",
        "--bearer-token",
        "--password",
        "--cookie",
        "--authorization",
        "--header",
        "-H",
    ],
};

static CONFIG: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "codex.config",
    schema_version: 1,
    policy_id: "codex.config.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Toml,
    rules: &[
        R::url(&[K("model_providers"), A, K("base_url")], &[]),
        R::scalar(&[K("model_providers"), A, K("name")], S::Identifier),
        R::scalar(&[K("model_providers"), A, K("wire_api")], S::Identifier),
        R::scalar(&[K("model_providers"), A, K("env_key")], S::Identifier),
        R::scalar(
            &[K("model_providers"), A, K("requires_openai_auth")],
            S::Boolean,
        ),
        R::scalar(&[K("model_reasoning_effort")], S::Identifier),
        R::scalar(&[K("model_reasoning_summary")], S::Identifier),
        R::scalar(&[K("model_verbosity")], S::Identifier),
        R::scalar(
            &[K("cli_auth_credentials_store")],
            S::Enum(&["file", "keyring", "auto", "ephemeral"]),
        ),
        R::scalar(&[K("project_doc_fallback_filenames"), I], S::Path),
        R::scalar(&[K("project_doc_max_bytes")], S::Number),
        R::scalar(&[K("features"), A], S::Boolean),
        R::scalar(&[K("tui"), K("theme")], S::Identifier),
        R::scalar(&[K("mcp_servers"), A, K("command")], S::Executable),
        R::argv(&[K("mcp_servers"), A, K("args")], &ARGV),
        R::scalar(&[K("mcp_servers"), A, K("cwd")], S::Path),
        R::url(&[K("mcp_servers"), A, K("url")], &["transport"]),
        R::scalar(&[K("mcp_servers"), A, K("enabled")], S::Boolean),
        R::scalar(&[K("mcp_servers"), A, K("disabled")], S::Boolean),
        R::scalar(
            &[K("mcp_servers"), A, K("type")],
            S::Enum(&["stdio", "http", "sse", "websocket"]),
        ),
        R::scalar(
            &[K("mcp_servers"), A, K("transport")],
            S::Enum(&["stdio", "http", "sse", "websocket"]),
        ),
        R::scalar(&[K("mcp_servers"), A, K("timeout")], S::Number),
        R::scalar(&[K("mcp_servers"), A, K("startup_timeout_sec")], S::Number),
        R::hide_values(&[K("mcp_servers"), A, K("env")]),
        R::hide_values(&[K("mcp_servers"), A, K("headers")]),
        R::hide_values(&[K("mcp_servers"), A, K("http_headers")]),
        R::hide_values(&[K("mcp_servers"), A, K("env_http_headers")]),
        R::hide_values(&[K("mcp_servers"), A, K("auth")]),
        R::hide_values(&[K("mcp_servers"), A, K("oauth")]),
        R::hide(&[K("mcp_servers"), A, K("api_key")]),
        R::hide(&[K("mcp_servers"), A, K("apiKey")]),
        R::hide(&[K("mcp_servers"), A, K("token")]),
        R::hide(&[K("mcp_servers"), A, K("bearer_token")]),
        R::hide(&[K("mcp_servers"), A, K("password")]),
        R::hide(&[K("mcp_servers"), A, K("cookie")]),
        R::scalar(&[K("model")], S::Identifier),
        R::scalar(&[K("model_provider")], S::Identifier),
        R::scalar(
            &[K("sandbox_mode")],
            S::Enum(&["read-only", "workspace-write", "danger-full-access"]),
        ),
        R::scalar(
            &[K("approval_policy")],
            S::Enum(&["untrusted", "on-failure", "on-request", "never"]),
        ),
        R::scalar(&[K("plugins"), A, K("enabled")], S::Boolean),
        R::scalar(
            &[K("plugins"), A, K("mcp_servers"), A, K("enabled")],
            S::Boolean,
        ),
        R::scalar(&[K("skills"), K("config"), I, K("enabled")], S::Boolean),
        R::scalar(&[K("skills"), K("config"), I, K("path")], S::Path),
        R::scalar(&[K("skills"), K("config"), I, K("name")], S::Identifier),
        R::scalar(&[K("tui"), K("status_line"), I], S::Identifier),
        R::hide_values(&[K("env")]),
        R::hide(&[K("auth")]),
        R::hide(&[K("api_key")]),
    ],
};

static PLUGIN_MANIFEST: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "codex.plugin-manifest",
    schema_version: 1,
    policy_id: "codex.plugin-manifest.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Json,
    rules: &[
        R::scalar(&[K("name")], S::Identifier),
        R::scalar(&[K("version")], S::Identifier),
        R::scalar(&[K("skills")], S::Path),
        R::scalar(&[K("skills"), I], S::Path),
        R::scalar(&[K("mcpServers")], S::Path),
    ],
};

// Native legacy manifests permit both a bare server map and mcpServers.
// Agent Plugins uses the wrapped map. Unknown values remain redacted by default.
static PLUGIN_MCP: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "codex.plugin-mcp",
    schema_version: 1,
    policy_id: "codex.plugin-mcp.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Json,
    rules: &[
        R::scalar(&[K("mcpServers"), A, K("command")], S::Executable),
        R::argv(&[K("mcpServers"), A, K("args")], &ARGV),
        R::scalar(&[K("mcpServers"), A, K("cwd")], S::Path),
        R::url(&[K("mcpServers"), A, K("url")], &["transport"]),
        R::scalar(
            &[K("mcpServers"), A, K("type")],
            S::Enum(&["stdio", "http", "sse", "streamable-http"]),
        ),
        R::scalar(&[K("mcpServers"), A, K("enabled")], S::Boolean),
        R::scalar(&[K("mcpServers"), A, K("disabled")], S::Boolean),
        R::hide_values(&[K("mcpServers"), A, K("env")]),
        R::hide_values(&[K("mcpServers"), A, K("headers")]),
        R::hide_values(&[K("mcpServers"), A, K("http_headers")]),
        R::scalar(&[A, K("command")], S::Executable),
        R::argv(&[A, K("args")], &ARGV),
        R::scalar(&[A, K("cwd")], S::Path),
        R::url(&[A, K("url")], &["transport"]),
        R::scalar(
            &[A, K("type")],
            S::Enum(&["stdio", "http", "sse", "streamable-http"]),
        ),
        R::scalar(&[A, K("enabled")], S::Boolean),
        R::scalar(&[A, K("disabled")], S::Boolean),
        R::hide_values(&[A, K("env")]),
        R::hide_values(&[A, K("headers")]),
        R::hide_values(&[A, K("http_headers")]),
    ],
};

pub(crate) fn source_preview(source: &AgentAssetSourceSpec) -> AgentSourcePreviewPolicy {
    match source.native_source_key.as_str() {
        "config" | "workspace-config" | "system-config" => {
            AgentSourcePreviewPolicy::structured(&CONFIG)
        }
        key if key.starts_with("profile:") || key.starts_with("ancestor-config:") => {
            AgentSourcePreviewPolicy::structured(&CONFIG)
        }
        key if key.starts_with("codex-plugin-manifest:")
            && matches!(
                key.rsplit(':').next(),
                Some("agent" | "codex" | "claude" | "cursor")
            ) =>
        {
            AgentSourcePreviewPolicy::structured(&PLUGIN_MANIFEST)
        }
        key if key.starts_with("codex-plugin-mcp:")
            && matches!(key.rsplit(':').next(), Some("agent" | "legacy")) =>
        {
            AgentSourcePreviewPolicy::structured(&PLUGIN_MCP)
        }
        _ => AgentSourcePreviewPolicy::metadata_only(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentAssetCategory, AgentAssetScope, AgentAssetSourceKind};
    use crate::services::agent_cli::environment::{
        preview::{render_preview, AgentPreviewMetadataReason},
        source, SourceInput,
    };
    use std::path::Path;

    fn policy(key: &str) -> AgentSourcePreviewPolicy {
        source_preview(&source(SourceInput {
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            native_source_key: key,
            label: "fixture plugin",
            path: Path::new("/fixture/plugin.json").to_path_buf(),
            allowed_root: Path::new("/fixture"),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: &[AgentAssetCategory::Plugin],
        }))
    }

    #[test]
    fn plugin_previews_keep_safe_native_parameters_and_hide_payloads_in_both_formats() {
        let secret = "bh-private-preview-sentinel";
        let server = serde_json::json!({
            "command":"node", "args":["server.js", "--port", "8123", "--token", secret],
            "enabled":false, "env":{"TOKEN":secret},
            "headers":{"Authorization":secret}, "extension_data": secret,
        });
        for wrapped in [false, true] {
            let map = serde_json::json!({"fixture":server});
            let document = if wrapped {
                serde_json::json!({"mcpServers":map})
            } else {
                map
            };
            for format in ["agent", "legacy"] {
                let output = render_preview(
                    policy(&format!("codex-plugin-mcp:fixture@local:local:{format}")),
                    &serde_json::to_vec(&document).unwrap(),
                );
                let content = output.content.unwrap();
                assert!(content.contains("8123"));
                assert!(content.contains("false"));
                assert!(!content.contains(secret));
                assert!(output.redacted);
            }
        }
    }

    #[test]
    fn plugin_preview_rejects_duplicate_keys_and_keeps_unrecognised_sources_metadata_only() {
        let invalid = render_preview(
            policy("codex-plugin-manifest:fixture@local:local:codex"),
            br#"{"name":"one","name":"two"}"#,
        );
        assert_eq!(
            invalid.metadata_reason,
            Some(AgentPreviewMetadataReason::InvalidDocument)
        );
        let unknown = render_preview(policy("codex-skill:fixture"), b"private skill body");
        assert!(unknown.content.is_none());
        assert_eq!(
            unknown.metadata_reason,
            Some(AgentPreviewMetadataReason::UnsupportedSchema)
        );
    }
}
