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

static SETTINGS: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "gemini.settings",
    schema_version: 1,
    policy_id: "gemini.settings.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Jsonc,
    rules: &[
        R::scalar(
            &[K("security"), K("auth"), K("selectedType")],
            S::Identifier,
        ),
        R::scalar(&[K("security"), K("auth"), K("useExternal")], S::Boolean),
        R::scalar(&[K("context"), K("fileName")], S::Path),
        R::scalar(&[K("context"), K("fileName"), I], S::Path),
        R::scalar(&[K("ui"), K("theme")], S::Identifier),
        R::scalar(&[K("ui"), K("showLineNumbers")], S::Boolean),
        R::scalar(&[K("general"), K("vimMode")], S::Boolean),
        R::scalar(&[K("general"), K("previewFeatures")], S::Boolean),
        R::scalar(&[K("model"), K("maxSessionTurns")], S::Number),
        R::scalar(&[K("mcpServers"), A, K("command")], S::Executable),
        R::argv(&[K("mcpServers"), A, K("args")], &ARGV),
        R::scalar(&[K("mcpServers"), A, K("cwd")], S::Path),
        R::url(&[K("mcpServers"), A, K("url")], &["transport"]),
        R::scalar(&[K("mcpServers"), A, K("enabled")], S::Boolean),
        R::scalar(&[K("mcpServers"), A, K("disabled")], S::Boolean),
        R::scalar(
            &[K("mcpServers"), A, K("type")],
            S::Enum(&["stdio", "http", "sse", "websocket"]),
        ),
        R::scalar(
            &[K("mcpServers"), A, K("transport")],
            S::Enum(&["stdio", "http", "sse", "websocket"]),
        ),
        R::scalar(&[K("mcpServers"), A, K("timeout")], S::Number),
        R::scalar(&[K("mcpServers"), A, K("startup_timeout_sec")], S::Number),
        R::hide_values(&[K("mcpServers"), A, K("env")]),
        R::hide_values(&[K("mcpServers"), A, K("headers")]),
        R::hide_values(&[K("mcpServers"), A, K("http_headers")]),
        R::hide_values(&[K("mcpServers"), A, K("env_http_headers")]),
        R::hide_values(&[K("mcpServers"), A, K("auth")]),
        R::hide_values(&[K("mcpServers"), A, K("oauth")]),
        R::hide(&[K("mcpServers"), A, K("api_key")]),
        R::hide(&[K("mcpServers"), A, K("apiKey")]),
        R::hide(&[K("mcpServers"), A, K("token")]),
        R::hide(&[K("mcpServers"), A, K("bearer_token")]),
        R::hide(&[K("mcpServers"), A, K("password")]),
        R::hide(&[K("mcpServers"), A, K("cookie")]),
        R::scalar(
            &[K("hooks"), A, I, K("hooks"), I, K("type")],
            S::Enum(&["command", "prompt", "http"]),
        ),
        R::scalar(
            &[K("hooks"), A, I, K("hooks"), I, K("command")],
            S::Executable,
        ),
        R::url(&[K("hooks"), A, I, K("hooks"), I, K("url")], &[]),
        R::scalar(&[K("hooks"), A, I, K("hooks"), I, K("timeout")], S::Number),
        R::hide_values(&[K("hooks"), A, I, K("hooks"), I, K("headers")]),
        R::hide_values(&[K("hooks"), A, I, K("hooks"), I, K("env")]),
        R::hide(&[K("hooks"), A, I, K("hooks"), I, K("prompt")]),
        R::scalar(&[K("model"), K("name")], S::Identifier),
        R::scalar(&[K("ui"), K("footer")], S::Boolean),
        R::scalar(&[K("footer")], S::Boolean),
        R::scalar(&[K("hooksConfig"), K("enabled")], S::Boolean),
        R::hide_values(&[K("env")]),
    ],
};

// Dotenv is parsed by the configuration codec; these JSON-shaped rules only
// classify its static key/value map, never execute or reparse a shell file.
static ENVIRONMENT: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "gemini.environment",
    schema_version: 1,
    policy_id: "gemini.environment.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Json,
    rules: &[
        R::url(&[K("GOOGLE_GEMINI_BASE_URL")], &[]),
        R::scalar(&[K("GOOGLE_CLOUD_PROJECT")], S::Identifier),
        R::scalar(&[K("GOOGLE_CLOUD_LOCATION")], S::Identifier),
        R::scalar(&[K("GOOGLE_APPLICATION_CREDENTIALS")], S::Path),
        R::scalar(&[K("GEMINI_MODEL")], S::Identifier),
        R::hide(&[K("GEMINI_API_KEY")]),
        R::hide(&[K("GOOGLE_API_KEY")]),
    ],
};

static ENABLEMENT: AgentPreviewPolicy = AgentPreviewPolicy {
    schema_id: "gemini.mcp-enablement",
    schema_version: 1,
    policy_id: "gemini.mcp-enablement.preview",
    policy_version: 1,
    format: AgentPreviewFormat::Json,
    rules: &[R::scalar(&[A, K("enabled")], S::Boolean)],
};

pub(crate) fn source_preview(source: &AgentAssetSourceSpec) -> AgentSourcePreviewPolicy {
    match source.native_source_key.as_str() {
        "settings" | "workspace-settings" | "system-settings" | "system-defaults" => {
            AgentSourcePreviewPolicy::structured(&SETTINGS)
        }
        key if key.starts_with("environment")
            || key.starts_with("workspace-environment")
            || key.starts_with("ancestor-environment:") =>
        {
            AgentSourcePreviewPolicy::structured(&ENVIRONMENT)
        }
        "mcp-enablement" => AgentSourcePreviewPolicy::structured(&ENABLEMENT),
        _ => AgentSourcePreviewPolicy::metadata_only(),
    }
}
