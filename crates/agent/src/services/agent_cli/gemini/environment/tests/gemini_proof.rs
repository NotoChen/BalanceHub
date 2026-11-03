//! Production parser → native decision → independent assessment regressions.
use super::*;

#[path = "gemini_proof/candidates.rs"]
mod candidates;
#[path = "gemini_proof/evidence.rs"]
mod evidence;

fn context() -> AgentConfigurationContext {
    AgentConfigurationContext {
        trust_context: AgentTrustState::Trusted,
        ..test_context()
    }
}

fn json_source(
    key: &str,
    categories: &[AgentAssetCategory],
    precedence: u32,
    document: &str,
) -> (AgentAssetSourceSpec, AgentAssetSnapshot) {
    (
        d2_source(
            key,
            categories.to_vec(),
            precedence,
            &format!("/tmp/gemini/{key}.json"),
        ),
        AgentAssetSnapshot::File {
            bytes: document.as_bytes().to_vec(),
            revision: Default::default(),
        },
    )
}

fn settings(
    key: &str,
    precedence: u32,
    document: &str,
) -> (AgentAssetSourceSpec, AgentAssetSnapshot) {
    json_source(
        key,
        &[
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Hook,
            AgentAssetCategory::StatusUi,
        ],
        precedence,
        document,
    )
}

fn extension() -> (AgentAssetSourceSpec, AgentAssetSnapshot) {
    json_source(
        "gemini-extension-manifest:alpha",
        &[AgentAssetCategory::Extension, AgentAssetCategory::Mcp],
        10,
        r#"{"name":"alpha","version":"1.0.0","mcpServers":{"child":{"command":"child"}}}"#,
    )
}

fn target(category: AgentAssetCategory, id: &str) -> AgentAssetAssessmentTarget {
    AgentAssetAssessmentTarget {
        category,
        resolution_group_key: if category == AgentAssetCategory::Mcp {
            id.trim().to_ascii_lowercase()
        } else {
            id.to_owned()
        },
        exact_native_id: id.to_owned(),
        subject: AgentAssetAssessmentSubject::Bucket,
    }
}

fn assessment(
    trace: &D2ProjectionTrace,
    context: &AgentConfigurationContext,
    target: AgentAssetAssessmentTarget,
) -> AgentAssetAssessmentResult {
    assess_assets(AgentAssetAssessmentRequest {
        context,
        targets: std::slice::from_ref(&target),
        declarations: &trace.declarations,
        sources: &trace.sources,
    })
    .get(&target)
    .expect("one requested native assessment")
    .clone()
}

fn assessed(
    trace: &D2ProjectionTrace,
    category: AgentAssetCategory,
    id: &str,
) -> (AgentAssetNativeAssessment, AgentAssetProjectionAssessment) {
    match assessment(trace, &context(), target(category, id)) {
        AgentAssetAssessmentResult::Assessed { state, projection } => (state, *projection),
        other => panic!("supported target was not assessed: {other:?}"),
    }
}

fn reproject(
    trace: &D2ProjectionTrace,
    drafts: Vec<AgentAssetProjectedDraft>,
) -> D2ProjectionTrace {
    project_trace(
        context(),
        trace.sources.clone(),
        trace.declarations.clone(),
        drafts,
        Vec::new(),
    )
}

fn resolve_raw(
    context: AgentConfigurationContext,
    sources: Vec<AgentAssetSourceSpec>,
    declarations: Vec<ParsedAgentAsset>,
) -> D2ProjectionTrace {
    let snapshots = sources
        .iter()
        .map(|_| AgentAssetSnapshot::Missing {
            revision: Default::default(),
        })
        .collect::<Vec<_>>();
    let refs = sources
        .iter()
        .zip(&snapshots)
        .map(|(spec, snapshot)| AgentAssetResolveSource { spec, snapshot })
        .collect::<Vec<_>>();
    let mut output = ResolveCollector::default();
    resolve_assets(
        AgentAssetResolveRequest {
            context: &context,
            sources: &refs,
            declarations: &declarations,
        },
        &mut output,
    );
    project_trace(
        context,
        sources,
        declarations,
        output.drafts,
        output.diagnostics,
    )
}

fn reset_id(context: &AgentConfigurationContext, declaration: &mut ParsedAgentAsset) {
    declaration.declaration_id = crate::services::agent_cli::environment::stable_id(
        "declaration",
        &[
            &context.id,
            &declaration.source_key,
            &declaration.category.key(),
            &declaration.declaration_key,
        ],
    );
}

fn rejected(trace: &D2ProjectionTrace, category: AgentAssetCategory, id: &str) {
    assert!(!trace
        .records
        .iter()
        .any(|asset| asset.category == category && asset.native_id == id));
    assert!(trace.diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        AgentAssetDiagnostic::InvalidProjection { .. }
            | AgentAssetDiagnostic::InvalidResolution { .. }
    )));
}

#[test]
fn exact_key_replacement_precedes_normalized_collision() {
    let trace = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                10,
                r#"{"mcpServers":{"Server":{"url":"https://one.invalid"}}}"#,
            ),
            settings(
                "system-defaults",
                5,
                r#"{"mcpServers":{"server":{"url":"https://two.invalid"}}}"#,
            ),
        ],
    );
    assert_eq!(trace.records.len(), 2);
    for id in ["Server", "server"] {
        let row = record(&trace, AgentAssetCategory::Mcp, id);
        assert_eq!(
            row.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(row.resolution.qualified_collision);
        assert_eq!(row.effective_state, AgentAssetState::Enabled);
    }
    assert_no_projection_diagnostics(&trace);
}

#[test]
fn hook_policy_uses_exact_matcher_and_invalid_non_match_is_unknown() {
    let trace = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                10,
                r#"{"hooksConfig":{"disabled":["run"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"run"},{"type":"runtime","name":"Run"}]}]}}"#,
            ),
            settings(
                "system-defaults",
                5,
                r#"{"hooksConfig":{"disabled":false}}"#,
            ),
        ],
    );
    assert_eq!(trace.records.len(), 2);
    let disabled = record(&trace, AgentAssetCategory::Hook, "BeforeTool:0:0");
    let unknown = record(&trace, AgentAssetCategory::Hook, "BeforeTool:0:1");
    assert_eq!(
        (disabled.declared_state, disabled.effective_state),
        (AgentAssetState::Disabled, AgentAssetState::Disabled)
    );
    assert_eq!(
        (unknown.declared_state, unknown.effective_state),
        (AgentAssetState::Enabled, AgentAssetState::Unknown)
    );
    assert_no_projection_diagnostics(&trace);
}

#[test]
fn gemini_a5_allowed_intersection_precedes_extension_alias_matching() {
    for (first, second, expected) in [
        (
            r#"["child"]"#,
            r#"["ext:alpha:child"]"#,
            AgentAssetState::Blocked,
        ),
        (
            r#"["child","ext:alpha:child"]"#,
            r#"["ext:alpha:child"]"#,
            AgentAssetState::Enabled,
        ),
        (
            r#"["child","ext:alpha:child"]"#,
            r#"["child"]"#,
            AgentAssetState::Enabled,
        ),
    ] {
        let trace = run_d2_sources(
            context(),
            vec![
                settings(
                    "settings",
                    20,
                    &format!(r#"{{"mcp":{{"allowed":{first}}}}}"#),
                ),
                settings(
                    "system-defaults",
                    10,
                    &format!(r#"{{"mcp":{{"allowed":{second}}}}}"#),
                ),
                extension(),
            ],
        );
        assert_eq!(trace.records.len(), 2);
        let child = record(&trace, AgentAssetCategory::Mcp, "child");
        assert_eq!(child.effective_state, expected);
        assert_no_projection_diagnostics(&trace);
        let (state, _) = assessed(&trace, AgentAssetCategory::Mcp, "child");
        if expected == AgentAssetState::Blocked {
            let control = state.control.unwrap();
            assert_eq!(control.members.len(), 2);
            assert_eq!(control.authorities.len(), 2);
            assert!(child.resolution.control_source.is_none());
        } else {
            assert!(state.control.is_none());
        }
    }
}

#[test]
fn gemini_a5_all_native_set_authorities_survive_precedence_and_peer_ties() {
    for family in ["allowed", "excluded"] {
        for low in [10, 20] {
            let values = if family == "allowed" {
                "[]"
            } else {
                r#"["server"]"#
            };
            let trace = run_d2_sources(
                context(),
                vec![
                    settings(
                        "settings",
                        20,
                        &format!(
                            r#"{{"mcp":{{"{family}":{values}}},"mcpServers":{{"server":{{"command":"run"}}}},"footer":true}}"#
                        ),
                    ),
                    settings(
                        "system-defaults",
                        low,
                        &format!(r#"{{"mcp":{{"{family}":{values}}}}}"#),
                    ),
                ],
            );
            assert_eq!(trace.records.len(), 2);
            let row = record(&trace, AgentAssetCategory::Mcp, "server");
            assert_eq!(row.effective_state, AgentAssetState::Blocked);
            assert!(row.resolution.control_source.is_none());
            let (state, _) = assessed(&trace, AgentAssetCategory::Mcp, "server");
            let control = state.control.unwrap();
            assert_eq!(control.members.len(), 2);
            assert_eq!(control.authorities.len(), 2);
            assert!(control
                .authorities
                .iter()
                .all(|authority| matches!(authority, AgentAssetControlAuthority::Declaration(_))));
            assert_no_projection_diagnostics(&trace);
            for forge in 0..2 {
                let mut drafts = trace.drafts.clone();
                let draft = drafts
                    .iter_mut()
                    .find(|draft| draft.native_id == "server")
                    .unwrap();
                if forge == 0 {
                    draft.resolution.control_source =
                        Some(AgentAssetPolicyReferenceDraft::Declaration {
                            declaration_id: control.members[0].declaration_id.clone(),
                        });
                } else if let AgentAssetEffectiveStateProofDraft::Terminal { evidence, .. } =
                    &mut draft.state_proof.effective
                {
                    evidence.pop();
                }
                let rejected_trace = reproject(&trace, drafts);
                rejected(&rejected_trace, AgentAssetCategory::Mcp, "server");
                assert_eq!(rejected_trace.records.len(), 1);
            }
        }
    }
}

#[test]
fn gemini_a5_single_authority_owner_and_overlay_unknown_axes_are_exact() {
    let trace = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                20,
                r#"{"mcp":{"allowed":[]},"mcpServers":{"server":{"command":"run"}},"footer":true}"#,
            ),
            json_source(
                "mcp-enablement",
                &[AgentAssetCategory::Mcp],
                30,
                r#"{"server":{"enabled":"bad"}}"#,
            ),
        ],
    );
    assert_eq!(trace.records.len(), 2);
    let (state, _) = assessed(&trace, AgentAssetCategory::Mcp, "server");
    assert!(matches!(
        state.declared,
        AgentAssetDeclaredStateProofDraft::Unknown {
            cause: AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl,
            ..
        }
    ));
    assert_eq!(state.control.as_ref().unwrap().authorities.len(), 1);
    assert_eq!(
        record(&trace, AgentAssetCategory::Mcp, "server").effective_state,
        AgentAssetState::Blocked
    );
    let mut drafts = trace.drafts.clone();
    drafts
        .iter_mut()
        .find(|draft| draft.native_id == "server")
        .unwrap()
        .resolution
        .control_source = None;
    let forged = reproject(&trace, drafts);
    rejected(&forged, AgentAssetCategory::Mcp, "server");
    assert_eq!(forged.records.len(), 1);
    assert_no_projection_diagnostics(&trace);
}

#[test]
fn gemini_a5_override_validation_finishes_before_workspace_evaluation() {
    for (document, expected) in [
        (
            r#"{"alpha":{"overrides":["*","!"]}}"#,
            Some(GeminiExtensionEnablementUnknownCause::InvalidEntry),
        ),
        (
            r#"{"alpha":{"overrides":["/tmp/workspace/*","!"]}}"#,
            Some(GeminiExtensionEnablementUnknownCause::InvalidEntry),
        ),
        (
            r#"{"alpha":{"overrides":["/tmp/workspace/*"]}}"#,
            Some(GeminiExtensionEnablementUnknownCause::WorkspaceUnavailable),
        ),
        (r#"{"alpha":{"overrides":[]}}"#, None),
    ] {
        let parent_only = json_source(
            "gemini-extension-manifest:alpha",
            &[AgentAssetCategory::Extension],
            10,
            r#"{"name":"alpha","version":"1.0.0"}"#,
        );
        let trace = run_d2_sources(
            context(),
            vec![
                parent_only,
                json_source(
                    "extension-enablement",
                    &[AgentAssetCategory::Extension],
                    30,
                    document,
                ),
            ],
        );
        assert_eq!(trace.records.len(), 1);
        let overlay = trace
            .declarations
            .iter()
            .find(|asset| asset.role == AgentAssetDeclarationRole::StateOverlay)
            .unwrap();
        if let Some(cause) = expected {
            assert!(
                matches!(overlay.native_payload, AgentAssetNativePayload::GeminiExtensionEnablementUnknown(actual) if actual == cause)
            );
            assert_eq!(trace.records[0].effective_state, AgentAssetState::Unknown);
            assert_eq!(
                trace
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| matches!(
                        diagnostic,
                        AgentAssetDiagnostic::Malformed { .. }
                    ))
                    .count(),
                usize::from(cause == GeminiExtensionEnablementUnknownCause::InvalidEntry)
            );
        } else {
            assert_eq!(trace.records[0].effective_state, AgentAssetState::Enabled);
        }
        assert!(trace.records[0].resolution.terminal.is_none());
        assert_no_projection_diagnostics(&trace);
    }
    for forms in [
        (Some(Path::new("/tmp/workspace/project")), None),
        (None, Some(Path::new("/tmp/workspace/project"))),
    ] {
        let trace = run_d2_sources_with_options(
            context(),
            vec![
                extension(),
                json_source(
                    "extension-enablement",
                    &[AgentAssetCategory::Extension],
                    30,
                    r#"{"alpha":{"overrides":["!/tmp/workspace/*","/tmp/workspace/project","!/tmp/workspace/project"]}}"#,
                ),
            ],
            false,
            false,
            forms.0,
            forms.1,
        );
        assert_eq!(trace.records.len(), 2);
        assert_eq!(
            record(&trace, AgentAssetCategory::Extension, "alpha").effective_state,
            AgentAssetState::Disabled
        );
        assert_no_projection_diagnostics(&trace);
    }
}

#[test]
fn gemini_a5_footer_unknown_preserves_known_structure_and_false_command_flag() {
    for footer in [
        "null",
        "false",
        "true",
        "{}",
        r#""runner""#,
        r#""""#,
        r#""  ""#,
        "42",
        "[]",
    ] {
        let trace = run_d2_sources(
            context(),
            vec![
                settings("settings", 20, &format!(r#"{{"footer":{footer}}}"#)),
                settings("system-defaults", 10, r#"{"footer":"lower"}"#),
            ],
        );
        assert_eq!(trace.records.len(), 2);
        let winner = trace
            .records
            .iter()
            .find(|asset| asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner)
            .unwrap();
        let (expected_mode, expected_state) = match footer {
            "null" | "false" => (AgentStatusUiMode::Disabled, AgentAssetState::Disabled),
            "true" | "{}" => (AgentStatusUiMode::BuiltIn, AgentAssetState::Enabled),
            r#""runner""# => (AgentStatusUiMode::Command, AgentAssetState::Enabled),
            _ => (AgentStatusUiMode::Unknown, AgentAssetState::Unknown),
        };
        assert_eq!(winner.declared_state, expected_state);
        assert_eq!(winner.effective_state, expected_state);
        assert_eq!(
            winner.details,
            AgentAssetDetails::StatusUi {
                mode: expected_mode,
                command_present: expected_mode == AgentStatusUiMode::Command
            }
        );
        assert_eq!(
            trace
                .records
                .iter()
                .filter(|asset| asset.effective_state == AgentAssetState::Shadowed)
                .count(),
            1
        );
        assert_no_projection_diagnostics(&trace);
    }
}
