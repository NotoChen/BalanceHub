use super::environment::ShellEnvironmentSnapshot;
use super::resolve_launch_model;
use super::resolve_resume_id;
use super::resolve_session_name;
use super::script::{
    claude_settings_content, cli_args, effective_model, escape_cmd_value, format_cli_command,
    insert_resume_id, preview_claude_settings_path, temporary_script_path, windows_launch_payload,
    WINDOWS_LAUNCH_PAYLOAD_COMMAND,
};
#[cfg(not(target_os = "windows"))]
use super::script::{
    login_shell_bootstrap, set_executable, shell_quote, shell_supports_posix_source, user_shell,
    write_launch_script, LaunchScriptInput,
};
use super::terminal::WINDOWS_POWERSHELL_SCRIPT_COMMAND;
#[cfg(target_os = "macos")]
use super::terminal::{
    build_macos_ghostty_activation_applescript, build_macos_ghostty_applescript,
    build_macos_iterm2_applescript, build_macos_terminal_applescript, warp_launcher_script_path,
};
use crate::models::{
    AppSettings, AuthMode, LivenessCliKind, Provider, ProviderInput, ProxyMode,
    TemporaryCliSessionMode, TemporaryCliTerminalKind,
};
use crate::network;
#[cfg(not(target_os = "windows"))]
use crate::{
    services::liveness::{anthropic_base_url, openai_base_url},
    util::unix_millis as now_millis,
};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(not(target_os = "windows"))]
use std::{env, fs, process::Command};

fn provider_with_liveness_model(model: &str) -> Provider {
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
            liveness: crate::models::ProviderLivenessInput {
                model: model.to_string(),
                ..ProviderInput::default().liveness
            },
            ..ProviderInput::default()
        },
        "provider/test".to_string(),
    )
}

#[test]
fn effective_model_prefers_provider_model() {
    let settings = AppSettings {
        liveness_model: "gpt-5.5".to_string(),
        ..AppSettings::default()
    };
    let provider = provider_with_liveness_model("claude-opus-4-6");

    assert_eq!(effective_model(&settings, &provider), "claude-opus-4-6");
}

#[test]
fn effective_model_falls_back_to_global_model() {
    let settings = AppSettings {
        liveness_model: "gpt-5.5".to_string(),
        ..AppSettings::default()
    };
    let provider = provider_with_liveness_model("");

    assert_eq!(effective_model(&settings, &provider), "gpt-5.5");
}

#[test]
fn resumed_session_without_override_preserves_its_model() {
    let settings = AppSettings {
        liveness_model: "gpt-5.5".to_string(),
        ..AppSettings::default()
    };
    let provider = provider_with_liveness_model("claude-opus-4-6");

    assert_eq!(
        resolve_launch_model(&settings, &provider, "", TemporaryCliSessionMode::History),
        ""
    );
    assert_eq!(
        resolve_launch_model(
            &settings,
            &provider,
            "claude-sonnet-4-5",
            TemporaryCliSessionMode::History,
        ),
        "claude-sonnet-4-5"
    );
}

#[test]
fn history_launch_requires_a_concrete_resume_id() {
    assert_eq!(
        resolve_resume_id(TemporaryCliSessionMode::New, "").unwrap(),
        ""
    );
    assert!(resolve_resume_id(TemporaryCliSessionMode::History, " ")
        .unwrap_err()
        .contains("请选择一个历史会话"));
    assert!(
        resolve_resume_id(TemporaryCliSessionMode::History, "session\n1")
            .unwrap_err()
            .contains("控制字符")
    );
    assert_eq!(
        resolve_resume_id(TemporaryCliSessionMode::History, "  session-1  ").unwrap(),
        "session-1"
    );
}

#[test]
fn session_name_is_only_forwarded_to_new_claude_sessions() {
    assert_eq!(
        resolve_session_name(
            LivenessCliKind::ClaudeCode,
            TemporaryCliSessionMode::New,
            "  Refactor billing  ",
        )
        .unwrap(),
        "Refactor billing"
    );
    assert_eq!(
        resolve_session_name(
            LivenessCliKind::ClaudeCode,
            TemporaryCliSessionMode::History,
            "Should not leak",
        )
        .unwrap(),
        ""
    );
    assert_eq!(
        resolve_session_name(
            LivenessCliKind::Codex,
            TemporaryCliSessionMode::New,
            "Codex title",
        )
        .unwrap(),
        ""
    );
}

#[test]
fn session_name_rejects_control_characters() {
    let error = resolve_session_name(
        LivenessCliKind::ClaudeCode,
        TemporaryCliSessionMode::New,
        "bad\nname",
    )
    .unwrap_err();
    assert!(error.contains("控制字符"));
}

#[test]
fn codex_args_override_provider_without_ignoring_user_config() {
    let args = cli_args(
        LivenessCliKind::Codex,
        "Relay Site",
        "https://relay.example.com/v1",
        "gpt-5.5",
        "",
        TemporaryCliSessionMode::New,
        None,
    );

    assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5.5"]));
    assert!(args.contains(&"model_provider=\"custom\"".to_string()));
    assert!(args.contains(&"model_providers.custom.name=\"Relay Site\"".to_string()));
    assert!(args
        .contains(&"model_providers.custom.base_url=\"https://relay.example.com/v1\"".to_string()));
    assert!(args.contains(&"model_providers.custom.env_key=\"OPENAI_API_KEY\"".to_string()));
    assert!(args.contains(&"model_providers.custom.wire_api=\"responses\"".to_string()));
    assert!(args.contains(&"model_providers.custom.requires_openai_auth=true".to_string()));
    assert!(!args.iter().any(|arg| arg.contains("balancehub")));
    assert!(!args.iter().any(|arg| arg.contains("identity.base_url")));
    assert!(!args.contains(&"--ignore-user-config".to_string()));
}

#[test]
fn codex_args_escape_toml_values() {
    let args = cli_args(
        LivenessCliKind::Codex,
        "Relay \"Site\"",
        "https://relay.example.com/openai/\"tenant\"",
        "",
        "",
        TemporaryCliSessionMode::New,
        None,
    );

    assert!(!args.contains(&"-m".to_string()));
    assert!(args.contains(&"model_providers.custom.name=\"Relay \\\"Site\\\"\"".to_string()));
    assert!(args.contains(
        &"model_providers.custom.base_url=\"https://relay.example.com/openai/\\\"tenant\\\"\""
            .to_string()
    ));
}

#[test]
fn launch_preview_command_keeps_plaintext_codex_credentials() {
    let command = format_cli_command(
        LivenessCliKind::Codex,
        "/opt/codex",
        &["--model".to_string(), "gpt-5.5".to_string()],
        "sk-preview",
        &[(
            "HTTPS_PROXY".to_string(),
            "socks5h://127.0.0.1:1080".to_string(),
        )],
    );

    assert!(command.contains("sk-preview"));
    assert!(command.contains("/opt/codex"));
    assert!(command.contains("gpt-5.5"));
    assert!(command.contains("socks5h://127.0.0.1:1080"));
}

#[test]
fn launch_preview_includes_plaintext_claude_settings() {
    let content =
        claude_settings_content("sk-preview", "https://relay.example.com/anthropic").unwrap();

    assert!(content.contains("sk-preview"));
    assert!(content.contains("https://relay.example.com/anthropic"));
    assert_eq!(
        preview_claude_settings_path(LivenessCliKind::ClaudeCode)
            .unwrap()
            .to_string_lossy(),
        "<temporary-claude-settings.json>"
    );
    assert!(preview_claude_settings_path(LivenessCliKind::Codex).is_none());
}

#[test]
fn official_session_modes_are_appended_after_provider_overrides() {
    let codex_history = cli_args(
        LivenessCliKind::Codex,
        "Relay Site",
        "https://relay.example.com/v1",
        "",
        "",
        TemporaryCliSessionMode::History,
        None,
    );
    assert_eq!(codex_history.last().map(String::as_str), Some("resume"));
    assert!(!codex_history.contains(&"-m".to_string()));

    let claude_history = cli_args(
        LivenessCliKind::ClaudeCode,
        "Relay Site",
        "https://relay.example.com",
        "claude-sonnet-4-5",
        "",
        TemporaryCliSessionMode::History,
        None,
    );
    assert_eq!(claude_history.last().map(String::as_str), Some("--resume"));
    assert!(claude_history
        .windows(2)
        .any(|pair| pair == ["--model", "claude-sonnet-4-5"]));

    let mut codex_resume = cli_args(
        LivenessCliKind::Codex,
        "Relay Site",
        "https://relay.example.com/v1",
        "",
        "",
        TemporaryCliSessionMode::History,
        None,
    );
    insert_resume_id(
        &mut codex_resume,
        LivenessCliKind::Codex,
        "019facdb-session",
    );
    assert_eq!(
        &codex_resume[codex_resume.len() - 2..],
        ["resume".to_string(), "019facdb-session".to_string()]
    );

    let mut claude_resume = cli_args(
        LivenessCliKind::ClaudeCode,
        "Relay Site",
        "https://relay.example.com",
        "",
        "",
        TemporaryCliSessionMode::History,
        None,
    );
    insert_resume_id(
        &mut claude_resume,
        LivenessCliKind::ClaudeCode,
        "019facdb-session",
    );
    assert_eq!(
        &claude_resume[claude_resume.len() - 2..],
        ["--resume".to_string(), "019facdb-session".to_string()]
    );
}

#[test]
fn claude_args_include_settings_and_model_when_configured() {
    let settings_path = Path::new("/tmp/claude settings.json");

    assert_eq!(
        cli_args(
            LivenessCliKind::ClaudeCode,
            "Relay Site",
            "https://relay.example.com",
            "claude-sonnet-4-5",
            "",
            TemporaryCliSessionMode::New,
            Some(settings_path)
        ),
        vec![
            "--settings".to_string(),
            "/tmp/claude settings.json".to_string(),
            "--model".to_string(),
            "claude-sonnet-4-5".to_string(),
        ]
    );
    assert_eq!(
        cli_args(
            LivenessCliKind::ClaudeCode,
            "Relay Site",
            "https://relay.example.com",
            "",
            "",
            TemporaryCliSessionMode::New,
            Some(settings_path)
        ),
        vec![
            "--settings".to_string(),
            "/tmp/claude settings.json".to_string(),
        ]
    );
}

#[test]
fn claude_args_include_name_only_for_new_sessions() {
    let named = cli_args(
        LivenessCliKind::ClaudeCode,
        "Relay Site",
        "https://relay.example.com",
        "",
        "Billing refactor",
        TemporaryCliSessionMode::New,
        None,
    );
    assert_eq!(
        named,
        vec!["--name".to_string(), "Billing refactor".to_string()]
    );

    let resumed = cli_args(
        LivenessCliKind::ClaudeCode,
        "Relay Site",
        "https://relay.example.com",
        "",
        "Billing refactor",
        TemporaryCliSessionMode::History,
        None,
    );
    assert_eq!(resumed, vec!["--resume".to_string()]);
}

#[test]
fn temporary_script_path_sanitizes_provider_id() {
    let provider = provider_with_liveness_model("");
    let path = temporary_script_path(&provider, LivenessCliKind::Codex);
    let text = path.to_string_lossy();

    assert!(text.contains("balancehub-temporary-cli-provider_test-"));
    assert!(
        text.ends_with("codex.command")
            || text.ends_with("codex.sh")
            || text.ends_with("codex.cmd")
    );
}

#[test]
fn cli_option_serialization_matches_frontend_values() {
    assert_eq!(
        serde_json::to_string(&TemporaryCliTerminalKind::ITerm2).unwrap(),
        "\"iTerm2\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliTerminalKind::WezTerm).unwrap(),
        "\"wezTerm\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliTerminalKind::Kaku).unwrap(),
        "\"kaku\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliTerminalKind::WindowsTerminal).unwrap(),
        "\"windowsTerminal\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliTerminalKind::PowerShell).unwrap(),
        "\"powerShell\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliSessionMode::History).unwrap(),
        "\"history\""
    );
    assert_eq!(
        serde_json::to_string(&TemporaryCliSessionMode::New).unwrap(),
        "\"new\""
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn generated_codex_launch_script_runs_without_terminal_ui() {
    let root = env::temp_dir().join(format!(
        "balancehub-temporary-cli-launch-test-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let workdir = root.join("work dir");
    let bindir = root.join("fake bin");
    fs::create_dir_all(&workdir).unwrap();
    fs::create_dir_all(&bindir).unwrap();

    let capture = root.join("capture.txt");
    let fake_codex = bindir.join("codex");
    fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex 0.0.0"
  exit 0
fi
{{
  printf 'PWD=%s\n' "$(pwd)"
  printf 'OPENAI_API_KEY=%s\n' "$OPENAI_API_KEY"
  printf 'HTTP_PROXY=%s\n' "$HTTP_PROXY"
  printf 'HTTPS_PROXY=%s\n' "$HTTPS_PROXY"
  printf 'ALL_PROXY=%s\n' "$ALL_PROXY"
  printf 'NO_PROXY=%s\n' "$NO_PROXY"
  printf 'NO_COLOR=%s\n' "$NO_COLOR"
  printf 'CLICOLOR=%s\n' "$CLICOLOR"
  printf 'ARGS=%s\n' "$*"
}} > {}
"#,
            shell_quote(&capture.to_string_lossy())
        ),
    )
    .unwrap();
    set_executable(&fake_codex).unwrap();

    let mut provider = provider_with_liveness_model("");
    provider.identity.name = "Relay Site".to_string();
    let script_dir = root.join("launch");
    fs::create_dir_all(&script_dir).unwrap();
    let script = script_dir.join("codex.sh");
    let status_path = root.join("status.json");
    let proxy = network::resolve_global_proxy(&AppSettings {
        proxy_mode: ProxyMode::Custom,
        proxy_url: "socks5h://127.0.0.1:1080".to_string(),
        ..AppSettings::default()
    });
    let proxy_environment = proxy.environment();
    write_launch_script(&LaunchScriptInput {
        script: &script,
        cli_kind: LivenessCliKind::Codex,
        cli_path: &fake_codex.to_string_lossy(),
        workdir: &workdir,
        provider_name: &provider.identity.name,
        api_key: &provider.auth.api_key,
        base_url: &openai_base_url(&provider),
        model: "gpt-5.5",
        session_name: "",
        resume_id: "",
        session_mode: TemporaryCliSessionMode::New,
        status_path: &status_path,
        proxy_environment: &proxy_environment,
    })
    .unwrap();
    let status = Command::new("/bin/sh")
        .arg(&script)
        .env("BALANCEHUB_LOGIN_ENV_READY", "1")
        .env("NO_COLOR", "1")
        .status()
        .unwrap();
    assert!(status.success());

    let captured = fs::read_to_string(&capture).unwrap();
    let args_line = captured
        .lines()
        .find(|line| line.starts_with("ARGS="))
        .unwrap_or_default();
    assert!(captured.contains(&format!("PWD={}", workdir.to_string_lossy())));
    assert!(captured.contains("OPENAI_API_KEY=sk-test"));
    assert!(captured.contains("HTTP_PROXY=socks5h://127.0.0.1:1080"));
    assert!(captured.contains("HTTPS_PROXY=socks5h://127.0.0.1:1080"));
    assert!(captured.contains("ALL_PROXY=socks5h://127.0.0.1:1080"));
    assert!(captured.contains("NO_PROXY=127.0.0.1,localhost,::1"));
    assert!(captured.lines().any(|line| line == "NO_COLOR="));
    assert!(captured.lines().any(|line| line == "CLICOLOR=1"));
    assert!(args_line.contains("-m gpt-5.5"));
    assert!(args_line.contains("model_provider=\"custom\""));
    assert!(args_line.contains("model_providers.custom.name=\"Relay Site\""));
    assert!(args_line.contains("model_providers.custom.base_url=\"https://relay.example.com/v1\""));
    assert!(!args_line.contains("balancehub"));

    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn generated_launch_script_can_be_sourced_by_zsh() {
    let root = env::temp_dir().join(format!(
        "balancehub-temporary-zsh-launch-test-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let workdir = root.join("work dir");
    let script_dir = root.join("launch");
    fs::create_dir_all(&workdir).unwrap();
    fs::create_dir_all(&script_dir).unwrap();

    let script = script_dir.join("codex.command");
    let status_path = root.join("status.json");
    let proxy_environment = network::resolve_global_proxy(&AppSettings::default()).environment();
    let provider = provider_with_liveness_model("");
    write_launch_script(&LaunchScriptInput {
        script: &script,
        cli_kind: LivenessCliKind::Codex,
        cli_path: "/usr/bin/true",
        workdir: &workdir,
        provider_name: &provider.identity.name,
        api_key: &provider.auth.api_key,
        base_url: &openai_base_url(&provider),
        model: "gpt-5.5",
        session_name: "",
        resume_id: "",
        session_mode: TemporaryCliSessionMode::New,
        status_path: &status_path,
        proxy_environment: &proxy_environment,
    })
    .unwrap();

    let source_command = format!(". {}", shell_quote(&script.to_string_lossy()));
    let status = Command::new("/bin/zsh")
        .args(["-lic", source_command.as_str()])
        .env("BALANCEHUB_LOGIN_ENV_READY", "1")
        .status()
        .unwrap();

    assert!(status.success());
    let stored = fs::read_to_string(&status_path).unwrap();
    assert!(stored.contains("\"status\":\"exited\""));
    assert!(stored.contains("\"exitCode\":0"));

    let _ = fs::remove_dir_all(root);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn generated_claude_launch_script_uses_settings_without_env_api_key() {
    let root = env::temp_dir().join(format!(
        "balancehub-temporary-claude-launch-test-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let workdir = root.join("work dir");
    let bindir = root.join("fake bin");
    fs::create_dir_all(&workdir).unwrap();
    fs::create_dir_all(&bindir).unwrap();

    let capture = root.join("capture.txt");
    let fake_claude = bindir.join("claude");
    fs::write(
        &fake_claude,
        format!(
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "claude 0.0.0"
  exit 0
fi
settings_path=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--settings" ]; then
settings_path="$arg"
  fi
  previous="$arg"
done
{{
  printf 'PWD=%s\n' "$(pwd)"
  printf 'ANTHROPIC_API_KEY=%s\n' "$ANTHROPIC_API_KEY"
  printf 'ANTHROPIC_AUTH_TOKEN=%s\n' "$ANTHROPIC_AUTH_TOKEN"
  printf 'ANTHROPIC_BASE_URL=%s\n' "$ANTHROPIC_BASE_URL"
  printf 'NO_COLOR=%s\n' "$NO_COLOR"
  printf 'CLICOLOR=%s\n' "$CLICOLOR"
  printf 'ARGS=%s\n' "$*"
  printf 'SETTINGS_BEGIN\n'
  if [ -n "$settings_path" ]; then
cat "$settings_path"
  fi
  printf '\nSETTINGS_END\n'
}} > {}
"#,
            shell_quote(&capture.to_string_lossy())
        ),
    )
    .unwrap();
    set_executable(&fake_claude).unwrap();

    let provider = provider_with_liveness_model("");
    let script_dir = root.join("launch");
    fs::create_dir_all(&script_dir).unwrap();
    let script = script_dir.join("claude.sh");
    let status_path = root.join("status.json");
    let proxy = network::resolve_global_proxy(&AppSettings {
        proxy_mode: ProxyMode::NoProxy,
        ..AppSettings::default()
    });
    let proxy_environment = proxy.environment();
    write_launch_script(&LaunchScriptInput {
        script: &script,
        cli_kind: LivenessCliKind::ClaudeCode,
        cli_path: &fake_claude.to_string_lossy(),
        workdir: &workdir,
        provider_name: &provider.identity.name,
        api_key: &provider.auth.api_key,
        base_url: &anthropic_base_url(&provider),
        model: "claude-sonnet-4-5",
        session_name: "Release smoke test",
        resume_id: "",
        session_mode: TemporaryCliSessionMode::New,
        status_path: &status_path,
        proxy_environment: &proxy_environment,
    })
    .unwrap();
    let status = Command::new("/bin/sh")
        .arg(&script)
        .env("BALANCEHUB_LOGIN_ENV_READY", "1")
        .env("NO_COLOR", "1")
        .status()
        .unwrap();
    assert!(status.success());

    let captured = fs::read_to_string(&capture).unwrap();
    let args_line = captured
        .lines()
        .find(|line| line.starts_with("ARGS="))
        .unwrap_or_default();
    assert!(captured.contains(&format!("PWD={}", workdir.to_string_lossy())));
    assert!(captured.contains("ANTHROPIC_API_KEY="));
    assert!(captured.contains("ANTHROPIC_AUTH_TOKEN="));
    assert!(captured.contains("ANTHROPIC_BASE_URL="));
    assert!(captured.lines().any(|line| line == "NO_COLOR="));
    assert!(captured.lines().any(|line| line == "CLICOLOR=1"));
    assert!(args_line.contains("--settings"));
    assert!(args_line.contains("--model claude-sonnet-4-5"));
    assert!(args_line.contains("--name Release smoke test"));
    assert!(captured.contains("\"ANTHROPIC_AUTH_TOKEN\": \"sk-test\""));
    assert!(captured.contains("\"ANTHROPIC_BASE_URL\": \"https://relay.example.com\""));
    assert!(!captured.contains("\"ANTHROPIC_API_KEY\""));

    let _ = fs::remove_dir_all(root);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn shell_quote_handles_single_quotes() {
    assert_eq!(shell_quote("/tmp/a'b"), "'/tmp/a'\\''b'");
}

#[cfg(not(target_os = "windows"))]
#[test]
fn temporary_script_enters_the_interactive_login_shell_once() {
    let bootstrap = login_shell_bootstrap(Path::new("/tmp/launch test.command"));

    assert!(bootstrap.contains("BALANCEHUB_LOGIN_ENV_READY"));
    assert!(bootstrap.contains(" -lic "));
    if shell_supports_posix_source(&user_shell()) {
        assert!(bootstrap.contains(". "));
    } else {
        assert!(bootstrap.contains("exec /bin/sh"));
    }
    assert!(bootstrap.contains("/tmp/launch test.command"));
}

#[cfg(not(target_os = "windows"))]
#[test]
fn unix_cli_invocation_prefers_login_shell_aliases_and_functions() {
    let invocation = super::script::unix_cli_invocation(
        LivenessCliKind::Codex,
        "/opt/codex/bin/codex",
        &["--model".to_string(), "gpt-5.5".to_string()],
    );

    assert!(invocation.contains("alias codex"));
    assert!(invocation.contains("typeset -f codex"));
    assert!(invocation.contains("  codex '--model' 'gpt-5.5'"));
    assert!(invocation.contains("  '/opt/codex/bin/codex' '--model' 'gpt-5.5'"));
}

#[test]
fn escape_cmd_value_neutralizes_batch_metacharacters() {
    assert_eq!(escape_cmd_value("sk-abc%TEMP%def"), "sk-abc%%TEMP%%def");
    assert_eq!(escape_cmd_value("sk-a\"b\r\ndel C:\\*"), "sk-abdel C:\\*",);
    assert_eq!(escape_cmd_value("sk-normal-key"), "sk-normal-key");
}

#[test]
fn windows_launch_payload_preserves_cli_arguments_and_credentials() {
    let args = vec![
        "--model".to_string(),
        "model %TEMP% \\\"quoted\\\" C:\\models\\".to_string(),
        "line one\r\nline two".to_string(),
    ];
    let payload = windows_launch_payload(
        LivenessCliKind::Codex,
        r#"C:\Program Files\Codex\codex.cmd"#,
        &args,
        "sk-%TEMP%-\"quoted\"\r\nkey",
        &network::resolve_global_proxy(&AppSettings {
            proxy_mode: ProxyMode::Custom,
            proxy_url: "socks5h://127.0.0.1:1080".to_string(),
            ..AppSettings::default()
        })
        .environment(),
        &ShellEnvironmentSnapshot {
            variables: std::collections::BTreeMap::from([
                ("OPENAI_API_KEY".to_string(), "profile-key".to_string()),
                ("Path".to_string(), "C:\\profile-bin".to_string()),
            ]),
            aliases: std::collections::BTreeMap::from([(
                "codex".to_string(),
                "codex.cmd".to_string(),
            )]),
            functions: std::collections::BTreeMap::new(),
        },
    );

    assert_eq!(
        payload["cliPath"],
        serde_json::json!(r#"C:\Program Files\Codex\codex.cmd"#)
    );
    assert_eq!(payload["args"], serde_json::json!(args));
    assert_eq!(
        payload["setEnv"]["OPENAI_API_KEY"],
        serde_json::json!("sk-%TEMP%-\"quoted\"\r\nkey")
    );
    assert_eq!(
        payload["setEnv"]["Path"],
        serde_json::json!(r#"C:\profile-bin"#)
    );
    assert_eq!(payload["cliCommandName"], serde_json::json!("codex"));
    assert_eq!(payload["aliases"]["codex"], serde_json::json!("codex.cmd"));
    assert_eq!(
        payload["setEnv"]["HTTPS_PROXY"],
        serde_json::json!("socks5h://127.0.0.1:1080")
    );
    assert_eq!(
        payload["setEnv"]["ALL_PROXY"],
        serde_json::json!("socks5h://127.0.0.1:1080")
    );
    let removed = payload["removeEnv"].as_array().unwrap();
    assert!(removed.contains(&serde_json::json!("CODEX_API_KEY")));
    assert!(removed.contains(&serde_json::json!("CODEX_ACCESS_TOKEN")));
    assert!(removed.contains(&serde_json::json!("HTTPS_PROXY")));
    assert!(removed.contains(&serde_json::json!("https_proxy")));
}

#[test]
fn windows_launch_commands_avoid_batch_command_string_quoting() {
    assert!(WINDOWS_LAUNCH_PAYLOAD_COMMAND.contains("[string]$launch.cliPath"));
    assert!(WINDOWS_LAUNCH_PAYLOAD_COMMAND.contains("[string]$launch.cliCommandName"));
    assert!(WINDOWS_LAUNCH_PAYLOAD_COMMAND.contains("$launch.args"));
    assert!(!WINDOWS_LAUNCH_PAYLOAD_COMMAND.contains("cmd /c"));
    assert_eq!(
        WINDOWS_POWERSHELL_SCRIPT_COMMAND,
        "& $env:BALANCEHUB_TEMPORARY_CLI_SCRIPT"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ghostty_launch_returns_the_created_terminal_id() {
    let script = build_macos_ghostty_applescript(Path::new("/tmp/launch test.command"));

    assert!(script.contains("set target_window to new window with configuration"));
    assert!(script.contains("set target_terminal to focused terminal of target_tab"));
    assert!(script.contains("return id of target_terminal"));
    assert!(script.contains("/bin/sh '/tmp/launch test.command'"));
    assert!(!script.contains("/bin/zsh"));
}

#[cfg(target_os = "macos")]
#[test]
fn ghostty_activation_targets_the_stored_terminal_id() {
    let script = build_macos_ghostty_activation_applescript("terminal-123");

    assert!(script.contains(r#"set target_id to "terminal-123""#));
    assert!(script.contains("every terminal whose id is target_id"));
    assert!(script.contains("focus item 1 of matching_terminals"));
}

#[cfg(target_os = "macos")]
#[test]
fn terminal_launch_avoids_cold_start_default_window() {
    let script = build_macos_terminal_applescript(Path::new("/tmp/launch test.command"));

    assert!(script.contains(r#"set was_running to application "Terminal" is running"#));
    assert!(script.contains("launch\n        do script launcher_script"));
    assert!(script.contains("exec /bin/sh '/tmp/launch test.command'"));
    assert!(!script.contains(r#"tell application "Terminal" to do script"#));
}

#[cfg(target_os = "macos")]
#[test]
fn iterm_launch_reuses_cold_start_default_window() {
    let script = build_macos_iterm2_applescript(Path::new("/tmp/launch test.command"));

    assert!(script.contains(r#"set was_running to application "iTerm" is running"#));
    assert!(script.contains("create tab with default profile"));
    assert!(script.contains("repeat while (count of windows) = 0"));
    assert!(script.contains("write text launcher_script"));
    assert!(script.contains("exec /bin/sh '/tmp/launch test.command'"));
    assert!(!script.contains("activate\ncreate window with default profile"));
}

#[cfg(target_os = "macos")]
#[test]
fn warp_launcher_uses_extensionless_sibling() {
    let launcher = warp_launcher_script_path(Path::new("/tmp/run dir/claude.command"));

    assert_eq!(launcher, PathBuf::from("/tmp/run dir/warp-launcher"));
}
