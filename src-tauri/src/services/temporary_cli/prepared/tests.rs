use super::*;
use crate::models::{AgentRuntimeScope, ProviderInput, ProxyMode, TemporaryCliSessionMode};

fn identity() -> AgentSessionLaunchIdentity {
    AgentSessionLaunchIdentity {
        session_ref: "synthetic-reference".to_owned(),
        source_identity: "synthetic-source".to_owned(),
        native_session_id: "native-resume-123".to_owned(),
        runtime_scope: AgentRuntimeScope::Native,
    }
}

#[test]
fn native_preparation_uses_no_provider_and_registers_identity_before_any_hook() {
    for cli_kind in [
        AgentCliKind::Codex,
        AgentCliKind::ClaudeCode,
        AgentCliKind::Gemini,
        AgentCliKind::Grok,
    ] {
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let root = std::env::temp_dir().join(format!(
            "bh-native-resume-fixture-{:x}",
            u64::from_ne_bytes(nonce)
        ));
        fs::create_dir_all(&root).unwrap();
        let cli = agent_cli::AgentCliExecutable {
            path: root.join("verified-cli").to_string_lossy().into_owned(),
            version: "fixture".to_owned(),
        };
        let settings = AppSettings {
            proxy_mode: ProxyMode::NoProxy,
            ..AppSettings::default()
        };
        let identity = identity();
        let mut source_environment = EnvironmentPatch::default();
        source_environment.set("BALANCEHUB_SYNTHETIC_SOURCE", "fixed-source");
        let prepared = prepare_launch(CliLaunchRequest {
            settings: &settings,
            target: CliLaunchTarget::Native,
            cli: &cli,
            cli_kind,
            workdir: &root,
            options: LaunchOptions {
                api_key_override: "",
                model_override: "",
                session_name_override: "",
                session_title: "",
                resume_id: &identity.native_session_id,
                session_mode: TemporaryCliSessionMode::History,
                api_key_label: "",
                api_key_local_id: None,
            },
            native_session: Some(&identity),
            source_environment: Some(&source_environment),
        })
        .unwrap();
        assert!(prepared.instance().provider_id.is_none());
        assert!(prepared.instance().provider_name.is_none());
        assert!(prepared.instance().api_key_local_id.is_none());
        assert_eq!(prepared.instance().native_session.as_ref(), Some(&identity));
        let reloaded = cli_runtime::instance(&prepared.instance().id)
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.native_session.as_ref(), Some(&identity));
        assert!(reloaded.provider_id.is_none());
        let script = fs::read_to_string(&prepared.script).unwrap();
        assert!(script.contains("native-resume-123"));
        assert!(script.contains("BALANCEHUB_SYNTHETIC_SOURCE='fixed-source'"));
        assert!(script.contains(&format!("'{}'", cli.path)));
        assert!(!script.contains("bh_use_shell_cli"));
        for provider_override in [
            "model_provider=",
            "OPENAI_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "GEMINI_API_KEY",
            "GROK_MODELS_BASE_URL",
            "--settings",
        ] {
            assert!(
                !script.contains(provider_override),
                "{cli_kind:?}: {provider_override}"
            );
        }
        assert!(prepared.auxiliary_file_name.is_none());
        let script_path = prepared.script.clone();
        let metadata_dir = prepared.registered.status_path.parent().unwrap().to_owned();
        prepared.cancel();
        assert!(!script_path.exists());
        assert_eq!(
            cli_runtime::instance(&reloaded.id).unwrap().unwrap().status,
            crate::models::TemporaryCliInstanceStatus::Exited
        );
        fs::remove_dir_all(metadata_dir).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn provider_resume_uses_the_same_preparation_and_exact_native_identity() {
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "bh-provider-resume-fixture-{:x}",
        u64::from_ne_bytes(nonce)
    ));
    fs::create_dir_all(&root).unwrap();
    let cli = agent_cli::AgentCliExecutable {
        path: root.join("verified-cli").to_string_lossy().into_owned(),
        version: "fixture".to_owned(),
    };
    let settings = AppSettings {
        proxy_mode: ProxyMode::NoProxy,
        ..AppSettings::default()
    };
    let mut provider =
        Provider::from_input(ProviderInput::default(), "synthetic-provider".to_owned());
    provider.identity.base_url = "https://relay.invalid".to_owned();
    provider.auth.api_key = "sk-synthetic-key".to_owned();
    let identity = identity();
    let prepared = prepare_launch(CliLaunchRequest {
        settings: &settings,
        target: CliLaunchTarget::Provider(&provider),
        cli: &cli,
        cli_kind: AgentCliKind::Codex,
        workdir: &root,
        options: LaunchOptions {
            api_key_override: "",
            model_override: "",
            session_name_override: "",
            session_title: "",
            resume_id: &identity.native_session_id,
            session_mode: TemporaryCliSessionMode::History,
            api_key_label: "test-key",
            api_key_local_id: Some("local-key"),
        },
        native_session: Some(&identity),
        source_environment: None,
    })
    .unwrap();
    let script = fs::read_to_string(&prepared.script).unwrap();
    assert!(script.contains("'resume' 'native-resume-123'"));
    assert!(script.contains("OPENAI_API_KEY='sk-synthetic-key'"));
    assert!(!script.contains("bh_use_shell_cli"));
    assert_eq!(
        prepared.instance().provider_id.as_deref(),
        Some("synthetic-provider")
    );
    assert_eq!(
        prepared.instance().api_key_local_id.as_deref(),
        Some("local-key")
    );
    assert_eq!(prepared.instance().native_session.as_ref(), Some(&identity));
    let metadata_dir = prepared.registered.status_path.parent().unwrap().to_owned();
    prepared.cancel();
    fs::remove_dir_all(metadata_dir).unwrap();
    fs::remove_dir_all(root).unwrap();
}
