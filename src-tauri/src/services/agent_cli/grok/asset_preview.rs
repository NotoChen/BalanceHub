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
    schema_id: "grok.config",
    schema_version: 1,
    policy_id: "grok.config.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Toml,
    rules: &[
        R::url(&[K("endpoints"), K("models_base_url")], &[]),
        R::scalar(&[K("reasoning_effort")], S::Identifier),
        R::scalar(&[K("permission"), K("default")], S::Identifier),
        R::scalar(&[K("ui"), K("screen_mode")], S::Identifier),
        R::scalar(&[K("telemetry"), K("enabled")], S::Boolean),
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
        R::scalar(&[K("disabled_mcp_servers"), I], S::Identifier),
        R::scalar(&[K("plugins"), K("enabled"), I], S::Identifier),
        R::scalar(&[K("plugins"), K("disabled"), I], S::Identifier),
        R::scalar(&[K("status_line")], S::Boolean),
        R::hide_values(&[K("env")]),
        R::hide(&[K("auth")]),
        R::hide(&[K("api_key")]),
    ],
};

pub(crate) fn source_preview(source: &AgentAssetSourceSpec) -> AgentSourcePreviewPolicy {
    match source.native_source_key.as_str() {
        "config" | "workspace-config" | "hook-user-managed" | "hook-user-requirements" => {
            AgentSourcePreviewPolicy::structured(&CONFIG)
        }
        _ => AgentSourcePreviewPolicy::metadata_only(),
    }
}
