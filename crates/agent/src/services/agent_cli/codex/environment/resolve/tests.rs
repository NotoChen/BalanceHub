use super::*;
use crate::models::{AgentCliKind, AgentConfigurationContext};
use crate::services::agent_cli::contracts::{
    AgentAssetParseRequest, AgentAssetSnapshot, AgentDiagnosticEmission, AgentDiagnosticOutput,
    AgentOutputStop, AgentParseOutput,
};
use std::ops::ControlFlow;

#[derive(Default)]
struct Parsed {
    declarations: Vec<ParsedAgentAsset>,
    stop_after: Option<usize>,
}

impl AgentDiagnosticOutput for Parsed {
    fn has_regular_capacity(&self) -> bool {
        true
    }
    fn emit_diagnostic(&mut self, _: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        AgentDiagnosticEmission::Accepted
    }
}
impl AgentParseOutput for Parsed {
    fn emit_declaration(&mut self, asset: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        self.declarations.push(asset);
        if self.stop_after == Some(self.declarations.len()) {
            ControlFlow::Break(AgentOutputStop::EntryLimit)
        } else {
            ControlFlow::Continue(())
        }
    }
}

fn context() -> AgentConfigurationContext {
    let root = std::env::temp_dir().join("balancehub-codex-requirements-fixture");
    AgentConfigurationContext {
        id: "codex-requirements-fixture".to_owned(),
        environment_id: "fixture".to_owned(),
        agent_kind: AgentCliKind::Codex,
        config_root: root.to_string_lossy().into_owned(),
        profile: "default".to_owned(),
        workspace_id: None,
        trust_context: AgentTrustState::Unknown,
        parser_version: super::super::PARSER_VERSION,
        schema_facts: BTreeMap::new(),
        compatible_installation_ids: Vec::new(),
    }
}

fn parse(
    bytes: Option<&[u8]>,
    stop_after: Option<usize>,
) -> (AgentAssetSourceSpec, Vec<ParsedAgentAsset>) {
    let context = context();
    let root = std::path::Path::new(&context.config_root);
    let source = super::super::system_paths::requirements_source(root);
    let snapshot = match bytes {
        Some(bytes) => AgentAssetSnapshot::File {
            bytes: bytes.to_vec(),
            revision: Default::default(),
        },
        None => AgentAssetSnapshot::Missing {
            revision: Default::default(),
        },
    };
    let mut output = Parsed {
        stop_after,
        ..Default::default()
    };
    super::super::parse_assets(
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
    (source, output.declarations)
}

#[test]
fn requirements_parser_preserves_absence_completion_and_invalidity() {
    let cases = [
        (None, CodexRequirementsPayload::MissingFile),
        (
            Some(b"allowed_approval_policies = [\"on-request\"]".as_slice()),
            CodexRequirementsPayload::NoPolicy,
        ),
        (
            Some(b"[mcp_servers]".as_slice()),
            CodexRequirementsPayload::AllowlistRoot { entry_count: 0 },
        ),
        (
            Some(b"mcp_servers = []".as_slice()),
            CodexRequirementsPayload::InvalidRequirements,
        ),
        (
            Some(b"\xff".as_slice()),
            CodexRequirementsPayload::InvalidRequirements,
        ),
    ];
    for (bytes, expected) in cases {
        let (source, declarations) = parse(bytes, None);
        assert_eq!(declarations.len(), 1);
        assert!(
            matches!(&declarations[0].native_payload, AgentAssetNativePayload::CodexRequirements(actual) if actual == &expected)
        );
        assert!(requirements_index(&context(), &declarations, &[source]).is_ok());
    }
}

#[test]
fn interrupted_requirements_cannot_become_empty_allowlist() {
    let (source, declarations) = parse(
        Some(b"[mcp_servers.one]\nidentity = { command = \"runner\" }"),
        Some(1),
    );
    assert_eq!(declarations.len(), 1);
    assert!(matches!(
        declarations[0].native_payload,
        AgentAssetNativePayload::CodexRequirements(CodexRequirementsPayload::Entry(_))
    ));
    assert!(matches!(
        requirements_index(&context(), &declarations, &[source]),
        Err(AgentAssetAssessmentFailure::IncompleteInput)
    ));
}

#[test]
fn requirements_reject_missing_duplicate_or_contradictory_roots() {
    let (source, no_policy) = parse(None, None);
    assert!(matches!(
        requirements_index(&context(), &no_policy, &[]),
        Err(AgentAssetAssessmentFailure::IncompleteInput)
    ));
    assert!(matches!(
        requirements_index(&context(), &[], std::slice::from_ref(&source)),
        Err(AgentAssetAssessmentFailure::IncompleteInput)
    ));
    assert!(matches!(
        requirements_index(&context(), &no_policy, &[source.clone(), source.clone()]),
        Err(AgentAssetAssessmentFailure::InvalidNativeInput)
    ));
    let duplicate = [no_policy[0].clone(), no_policy[0].clone()];
    assert!(matches!(
        requirements_index(&context(), &duplicate, std::slice::from_ref(&source)),
        Err(AgentAssetAssessmentFailure::InvalidNativeInput)
    ));
    let (_, with_entry) = parse(
        Some(b"[mcp_servers.one]\nidentity = { command = \"runner\" }"),
        None,
    );
    assert_eq!(with_entry.len(), 2);
    for bytes in [None, Some(b"other = true".as_slice())] {
        let (_, mut absent) = parse(bytes, None);
        absent.push(with_entry[0].clone());
        assert!(matches!(
            requirements_index(&context(), &absent, std::slice::from_ref(&source)),
            Err(AgentAssetAssessmentFailure::InvalidNativeInput)
        ));
    }
    let mut wrong_role = no_policy;
    wrong_role[0].role = AgentAssetDeclarationRole::Definition;
    assert!(matches!(
        requirements_index(&context(), &wrong_role, &[source]),
        Err(AgentAssetAssessmentFailure::InvalidNativeInput)
    ));
}
