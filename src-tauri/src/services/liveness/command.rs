use crate::{models::Provider, util::unix_millis as now_millis};

use super::LivenessContext;
use crate::models::LivenessCliKind;
use std::{env, path::Path, path::PathBuf, process::Command};

pub(super) fn build_codex_command(
    command: &mut Command,
    context: &LivenessContext,
    provider: &Provider,
) {
    command
        .env_remove("CODEX_API_KEY")
        .env_remove("CODEX_ACCESS_TOKEN");
    command
        .arg("--ask-for-approval")
        .arg("never")
        .arg("--sandbox")
        .arg("read-only")
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--ephemeral")
        .arg("--ignore-user-config")
        .arg("--ignore-rules")
        .arg("--json")
        .arg("-m")
        .arg(&context.model)
        .arg("-c")
        .arg("model_provider=\"balancehub\"")
        .arg("-c")
        .arg("model_providers.balancehub.identity.name=\"BalanceHub\"")
        .arg("-c")
        .arg(format!(
            "model_providers.balancehub.identity.base_url=\"{}\"",
            escape_toml_string(&context.base_url)
        ))
        .arg("-c")
        .arg("model_providers.balancehub.wire_api=\"responses\"")
        .arg("-c")
        .arg("model_providers.balancehub.env_key=\"OPENAI_API_KEY\"")
        .arg("-c")
        .arg("model_providers.balancehub.requires_openai_auth=true")
        .env("OPENAI_API_KEY", provider.auth.api_key.trim());
}

pub(super) fn apply_codex_isolated_home(command: &mut Command, path: &Path) {
    apply_common_isolated_home(command, path);
    command.env("CODEX_HOME", path);
}

pub(super) fn build_claude_command(
    command: &mut Command,
    context: &LivenessContext,
    provider: &Provider,
) {
    command
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("ANTHROPIC_BASE_URL")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .env_remove("CLAUDE_CODE_USE_BEDROCK")
        .env_remove("CLAUDE_CODE_USE_VERTEX")
        .env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY")
        .env_remove("AWS_SESSION_TOKEN")
        .env_remove("GOOGLE_APPLICATION_CREDENTIALS")
        .arg("--bare")
        .arg("-p")
        .arg(&context.prompt)
        .arg("--output-format")
        .arg("json")
        .arg("--model")
        .arg(&context.model)
        .arg("--max-budget-usd")
        .arg("0.02")
        .arg("--no-session-persistence")
        .arg("--tools")
        .arg("")
        .env("ANTHROPIC_API_KEY", provider.auth.api_key.trim())
        .env("ANTHROPIC_BASE_URL", &context.base_url)
        .env("DISABLE_TELEMETRY", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("CLAUDE_CODE_ATTRIBUTION_HEADER", "0")
        .env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "128")
        .env("CLAUDE_CODE_MAX_RETRIES", "0")
        .env(
            "API_TIMEOUT_MS",
            (context.timeout_seconds * 1000).to_string(),
        );
}

pub(super) fn apply_claude_isolated_home(command: &mut Command, path: &Path) {
    apply_common_isolated_home(command, path);
    command.env("CLAUDE_CONFIG_DIR", path.join(".claude"));
}

fn apply_common_isolated_home(command: &mut Command, path: &Path) {
    command
        .env("HOME", path)
        .env("USERPROFILE", path)
        .env("APPDATA", path.join("AppData/Roaming"))
        .env("LOCALAPPDATA", path.join("AppData/Local"))
        .env("XDG_CONFIG_HOME", path.join(".config"))
        .env("XDG_CACHE_HOME", path.join(".cache"))
        .env("XDG_DATA_HOME", path.join(".local/share"));
}

pub(super) fn build_command_preview(
    cli_kind: LivenessCliKind,
    cli_path: &str,
    model: &str,
    base_url: &str,
    prompt: &str,
) -> String {
    match cli_kind {
        LivenessCliKind::Codex => format!(
            "CODEX_HOME=/tmp/balancehub-codex-home-<random> OPENAI_API_KEY=*** {} --ask-for-approval never --sandbox read-only exec --skip-git-repo-check --ephemeral --ignore-user-config --ignore-rules --json -m {} -c 'model_provider=\"balancehub\"' -c 'model_providers.balancehub.identity.name=\"BalanceHub\"' -c 'model_providers.balancehub.identity.base_url=\"{}\"' -c 'model_providers.balancehub.wire_api=\"responses\"' -c 'model_providers.balancehub.env_key=\"OPENAI_API_KEY\"' -c 'model_providers.balancehub.requires_openai_auth=true' -o /tmp/balancehub-codex-<random>.txt '{}'",
            shell_quote(cli_path),
            shell_quote(model),
            base_url,
            prompt.replace('\'', "'\\''")
        ),
        LivenessCliKind::ClaudeCode => format!(
            "HOME=/tmp/balancehub-claude-home-<random> ANTHROPIC_API_KEY=*** ANTHROPIC_BASE_URL={} {} --bare -p '{}' --output-format json --model {} --max-budget-usd 0.02 --no-session-persistence --tools ''",
            shell_quote(base_url),
            shell_quote(cli_path),
            prompt.replace('\'', "'\\''"),
            shell_quote(model)
        ),
    }
}

pub(super) fn unique_output_path(provider: &Provider) -> PathBuf {
    env::temp_dir().join(format!(
        "balancehub-codex-{}-{}-{}.txt",
        provider.identity.id.replace('/', "_"),
        std::process::id(),
        now_millis()
    ))
}

pub(super) fn unique_codex_home_path(provider: &Provider) -> PathBuf {
    env::temp_dir().join(format!(
        "balancehub-codex-home-{}-{}-{}",
        provider.identity.id.replace('/', "_"),
        std::process::id(),
        now_millis()
    ))
}

pub(super) fn unique_claude_home_path(provider: &Provider) -> PathBuf {
    env::temp_dir().join(format!(
        "balancehub-claude-home-{}-{}-{}",
        provider.identity.id.replace('/', "_"),
        std::process::id(),
        now_millis()
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_preview_uses_isolated_home_and_env_key_auth() {
        let preview = build_command_preview(
            LivenessCliKind::Codex,
            "/usr/local/bin/codex",
            "gpt-5.5",
            "https://relay.example.com/v1",
            "reply pong",
        );

        assert!(preview.contains("CODEX_HOME=/tmp/balancehub-codex-home-<random>"));
        assert!(preview.contains("OPENAI_API_KEY=***"));
        assert!(preview.contains("model_providers.balancehub.env_key=\"OPENAI_API_KEY\""));
        assert!(preview.contains("model_providers.balancehub.requires_openai_auth=true"));
    }

    #[test]
    fn claude_preview_uses_isolated_home_and_provider_api_key() {
        let preview = build_command_preview(
            LivenessCliKind::ClaudeCode,
            "/usr/local/bin/claude",
            "claude-opus-4-6",
            "https://relay.example.com",
            "reply pong",
        );

        assert!(preview.contains("HOME=/tmp/balancehub-claude-home-<random>"));
        assert!(preview.contains("ANTHROPIC_API_KEY=***"));
        assert!(preview.contains("ANTHROPIC_BASE_URL='https://relay.example.com'"));
    }
}
