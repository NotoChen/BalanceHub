use super::*;

#[derive(Default)]
struct StopAfterFirst {
    declarations: usize,
}

impl AgentDiagnosticOutput for StopAfterFirst {
    fn has_regular_capacity(&self) -> bool {
        true
    }

    fn emit_diagnostic(
        &mut self,
        _value: AgentAssetDiagnostic,
    ) -> crate::services::agent_cli::contracts::AgentDiagnosticEmission {
        crate::services::agent_cli::contracts::AgentDiagnosticEmission::Accepted
    }
}

impl AgentParseOutput for StopAfterFirst {
    fn emit_declaration(&mut self, _value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        self.declarations += 1;
        ControlFlow::Break(AgentOutputStop::EntryLimit)
    }
}

#[test]
fn parser_stops_before_plugins_after_mcp_break() {
    let context = crate::models::AgentConfigurationContext {
        id: "grok-context".to_owned(),
        environment_id: "native".to_owned(),
        agent_kind: crate::models::AgentCliKind::Grok,
        config_root: "/tmp/grok".to_owned(),
        profile: "default".to_owned(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: 1,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    };
    let source = crate::services::agent_cli::contracts::AgentAssetSourceSpec {
        verified_physical_path: None,
        hook_definition_source: true,
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        provider: crate::models::AgentAssetProviderOrigin::Unknown,
        path_policy: Default::default(),
        native_source_key: "config".to_owned(),
        label: "config".to_owned(),
        scope: AgentAssetScope::User,
        path: "/tmp/grok/config.toml".into(),
        allowed_root: "/tmp/grok".into(),
        precedence: 10,
        writable: true,
        sensitive: true,
        source_kind: AgentAssetSourceKind::File,
        categories: vec![
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Plugin,
            AgentAssetCategory::StatusUi,
        ],
        allowed_logical_origins: vec![
            crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            },
        ],
    };
    let snapshot = AgentAssetSnapshot::File {
        bytes: br#"[mcp_servers.first]
command = "server"
[plugins]
enabled = ["plugin"]
[status_line]
command = "status"
"#
        .to_vec(),
        revision: Default::default(),
    };
    let mut output = StopAfterFirst::default();
    parse_assets(
        AgentAssetParseRequest {
            native_home: None,
            context: &context,
            source: &source,
            snapshot: &snapshot,
            workspace_canonical: None,
            workspace_lexical: None,
        },
        &mut output,
    );
    assert_eq!(output.declarations, 1);
}
