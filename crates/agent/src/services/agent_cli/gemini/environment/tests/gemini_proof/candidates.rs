use super::*;

fn parent_fixture() -> D2ProjectionTrace {
    run_d2_sources(context(), parent_inputs())
}

fn parent_inputs() -> Vec<(AgentAssetSourceSpec, AgentAssetSnapshot)> {
    vec![
        extension(),
        json_source(
            "gemini-extension-hooks:alpha",
            &[AgentAssetCategory::Hook],
            10,
            r#"{"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"child-hook"}]}]}}"#,
        ),
        json_source(
            "gemini-extension-skill:alpha:writer",
            &[AgentAssetCategory::Skill],
            10,
            "---\nname: writer\ndescription: writer\n---\n",
        ),
        settings("settings", 20, r#"{"footer":true}"#),
    ]
}

#[test]
fn gemini_a5_rejected_or_missing_enabled_parent_rejects_dependent_rows() {
    let trace = parent_fixture();
    assert_eq!(trace.records.len(), 5);
    let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
    assert_eq!(parent.effective_state, AgentAssetState::Enabled);
    let children = [
        (AgentAssetCategory::Mcp, "child"),
        (AgentAssetCategory::Hook, "alpha:BeforeTool:0:0"),
        (AgentAssetCategory::Skill, "writer"),
    ];
    for (category, id) in children {
        let child = record(&trace, category, id);
        assert_eq!(child.effective_state, AgentAssetState::Enabled);
        assert_eq!(
            child.relationships.provided_by.as_deref(),
            Some(parent.stable_id.as_str())
        );
        assert_eq!(
            child.relationships.action_owner,
            child.relationships.provided_by
        );
        let draft = trace
            .drafts
            .iter()
            .find(|draft| draft.native_kind == category && draft.native_id == id)
            .unwrap();
        assert!(matches!(
            &draft.state_proof.effective,
            AgentAssetEffectiveStateProofDraft::ParentGate { parent, input }
                if parent.native_id == "alpha"
                    && parent.category == AgentAssetCategory::Extension
                    && **input == AgentAssetEffectiveStateProofDraft::Intrinsic
        ));
    }
    assert_no_projection_diagnostics(&trace);

    for missing in [false, true] {
        let mut drafts = trace.drafts.clone();
        if missing {
            drafts.retain(|draft| draft.native_kind != AgentAssetCategory::Extension);
        } else {
            let parent = drafts
                .iter_mut()
                .find(|draft| draft.native_kind == AgentAssetCategory::Extension)
                .unwrap();
            let AgentAssetDetails::Extension { trusted, .. } = &mut parent.details else {
                panic!("real Extension details");
            };
            assert_eq!(*trusted, AgentTrustState::Unknown);
            *trusted = AgentTrustState::Trusted;
        }
        let forged = reproject(&trace, drafts);
        assert_eq!(
            forged.records.len(),
            1,
            "missing parent candidate = {missing}; surviving rows: {:?}",
            forged
                .records
                .iter()
                .map(|asset| (&asset.native_id, asset.effective_state))
                .collect::<Vec<_>>()
        );
        assert_eq!(forged.records[0].native_id, "footer");
        assert_eq!(forged.records[0].effective_state, AgentAssetState::Enabled);
        assert_eq!(forged.sources.len(), trace.sources.len());
        assert_eq!(forged.declarations.len(), trace.declarations.len());
        for (category, id) in children {
            rejected(&forged, category, id);
        }
    }

    for (category, id) in children {
        let mut drafts = trace.drafts.clone();
        drafts
            .iter_mut()
            .find(|draft| draft.native_kind == category && draft.native_id == id)
            .unwrap()
            .state_proof
            .effective = AgentAssetEffectiveStateProofDraft::Intrinsic;
        let forged = reproject(&trace, drafts);
        rejected(&forged, category, id);
        assert_eq!(forged.records.len(), 4, "stripped dependency: {category:?}");
    }
}

#[test]
fn gemini_a5_native_basis_rejects_independent_candidate_metadata_forgery() {
    let trace = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                20,
                r#"{"mcpServers":{"server":{"command":"run"}},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"ordinary-hook"}]}]},"footer":true}"#,
            ),
            json_source(
                "gemini-extension-manifest:alpha",
                &[AgentAssetCategory::Extension],
                10,
                r#"{"name":"alpha","version":"1.0.0"}"#,
            ),
            json_source(
                "gemini-skill-manifest:skills:writer",
                &[AgentAssetCategory::Skill],
                10,
                "---\nname: writer\ndescription: writer\n---\n",
            ),
        ],
    );
    let identities = [
        (AgentAssetCategory::Mcp, "server"),
        (AgentAssetCategory::Hook, "BeforeTool:0:0"),
        (AgentAssetCategory::StatusUi, "footer"),
        (AgentAssetCategory::Extension, "alpha"),
        (AgentAssetCategory::Skill, "writer"),
    ];
    assert_eq!(trace.records.len(), identities.len());
    for (category, id) in identities {
        let row = record(&trace, category, id);
        assert_eq!(row.effective_state, AgentAssetState::Enabled);
        assert_eq!(row.trust_state, AgentTrustState::Trusted);
    }
    assert!(matches!(
        record(&trace, AgentAssetCategory::Extension, "alpha").details,
        AgentAssetDetails::Extension {
            trusted: AgentTrustState::Unknown,
            ..
        }
    ));
    assert_no_projection_diagnostics(&trace);

    for forgery in [
        "transport",
        "approval",
        "status-mode",
        "status-command",
        "hook-managed",
        "skill-invocation",
        "extension-trust",
    ] {
        let category = match forgery {
            "transport" | "approval" => AgentAssetCategory::Mcp,
            "status-mode" | "status-command" => AgentAssetCategory::StatusUi,
            "hook-managed" => AgentAssetCategory::Hook,
            "skill-invocation" => AgentAssetCategory::Skill,
            "extension-trust" => AgentAssetCategory::Extension,
            _ => unreachable!(),
        };
        let mut drafts = trace.drafts.clone();
        let draft = drafts
            .iter_mut()
            .find(|draft| draft.native_kind == category)
            .unwrap();
        let id = draft.native_id.clone();
        match (forgery, &mut draft.details) {
            ("transport", AgentAssetDetails::Mcp { transport, .. }) => {
                *transport = AgentMcpTransport::Http;
            }
            ("approval", AgentAssetDetails::Mcp { approval_state, .. }) => {
                *approval_state = AgentMcpApprovalState::Approved;
            }
            (
                "status-mode",
                AgentAssetDetails::StatusUi {
                    mode,
                    command_present,
                },
            ) => {
                *mode = AgentStatusUiMode::Command;
                *command_present = true;
            }
            (
                "status-command",
                AgentAssetDetails::StatusUi {
                    command_present, ..
                },
            ) => {
                *command_present = true;
            }
            ("hook-managed", AgentAssetDetails::Hook { managed, .. }) => *managed = true,
            (
                "skill-invocation",
                AgentAssetDetails::Skill {
                    invocation_policy, ..
                },
            ) => {
                *invocation_policy = AgentSkillInvocationPolicy::ManualOnly;
            }
            ("extension-trust", AgentAssetDetails::Extension { trusted, .. }) => {
                *trusted = AgentTrustState::Trusted;
            }
            _ => panic!("fixture details for {forgery}"),
        }
        let forged = reproject(&trace, drafts);
        rejected(&forged, category, &id);
        assert_eq!(forged.records.len(), 4, "{forgery}");
        assert_eq!(forged.declarations.len(), trace.declarations.len());
    }
    for (category, id) in identities {
        let mut drafts = trace.drafts.clone();
        drafts
            .iter_mut()
            .find(|draft| draft.native_kind == category)
            .unwrap()
            .trust_state = AgentTrustState::Unknown;
        let forged = reproject(&trace, drafts);
        rejected(&forged, category, id);
        assert_eq!(forged.records.len(), 4, "record trust: {category:?}");
    }
}

#[test]
fn gemini_a5_missing_and_ambiguous_native_parents_keep_raw_child_evidence() {
    let initial = parent_fixture();
    assert_eq!(initial.records.len(), 5);
    assert_no_projection_diagnostics(&initial);
    let original_count = initial.declarations.len();
    for missing in [true, false] {
        let mut sources = initial.sources.clone();
        let mut declarations = initial.declarations.clone();
        if missing {
            declarations
                .retain(|declaration| declaration.category != AgentAssetCategory::Extension);
        } else {
            let (peer, peer_declarations) = super::evidence::decoded_peer(
                &initial,
                "gemini-extension-manifest:alpha",
                "gemini-extension-manifest:alpha-peer",
            );
            assert_eq!(peer_declarations.len(), 2);
            sources.push(peer);
            declarations.extend(peer_declarations);
        }
        assert_eq!(
            declarations
                .iter()
                .map(|declaration| &declaration.declaration_id)
                .collect::<BTreeSet<_>>()
                .len(),
            declarations.len(),
            "parent ambiguity must not come from duplicate declaration IDs"
        );
        let trace = resolve_raw(context(), sources, declarations);
        assert_eq!(trace.records.len(), if missing { 1 } else { 2 });
        assert_eq!(
            trace.declarations.len(),
            if missing {
                original_count - 1
            } else {
                original_count + 2
            }
        );
        assert_eq!(
            record(&trace, AgentAssetCategory::StatusUi, "footer").effective_state,
            AgentAssetState::Enabled
        );
        for category in [
            AgentAssetCategory::Mcp,
            AgentAssetCategory::Hook,
            AgentAssetCategory::Skill,
        ] {
            assert_eq!(
                trace
                    .records
                    .iter()
                    .filter(|asset| asset.category == category)
                    .count(),
                0,
                "dependent category {category:?}"
            );
            assert_eq!(
                trace
                    .declarations
                    .iter()
                    .filter(|asset| {
                        asset.category == category
                            && asset.role == AgentAssetDeclarationRole::Definition
                    })
                    .count(),
                if !missing && category == AgentAssetCategory::Mcp {
                    2
                } else {
                    1
                }
            );
        }
        if !missing {
            let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
            assert_eq!(parent.effective_state, AgentAssetState::Unknown);
            assert_eq!(
                parent.resolution.relation,
                AgentAssetResolutionRelation::Unknown
            );
            assert_eq!(parent.represented_declaration_ids.len(), 2);
        }
        assert!(trace.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidResolution { .. }
        )));
        // These children are omitted by the native decision before a graph
        // record exists. The native/common completeness diagnostic represents
        // that failure; UnresolvedRelationship is for admitted graph records.
        for (category, id) in [
            (AgentAssetCategory::Mcp, "child"),
            (AgentAssetCategory::Hook, "alpha:BeforeTool:0:0"),
            (AgentAssetCategory::Skill, "writer"),
        ] {
            assert_eq!(
                assessment(&trace, &context(), target(category, id)),
                AgentAssetAssessmentResult::Unsupported(if missing {
                    AgentAssetAssessmentFailure::MissingTarget
                } else {
                    AgentAssetAssessmentFailure::InvalidNativeInput
                })
            );
        }
    }
}

#[test]
fn gemini_a5_parent_routes_preserve_earlier_policy_and_invalid_control_causes() {
    for (enablement, parent_state) in [
        (r#"{"alpha":{"overrides":[]}}"#, AgentAssetState::Enabled),
        (
            r#"{"alpha":{"overrides":["!/tmp/workspace/*"]}}"#,
            AgentAssetState::Disabled,
        ),
        (r#"{"alpha":false}"#, AgentAssetState::Unknown),
    ] {
        for invalid_hook in [false, true] {
            let mut inputs = parent_inputs();
            inputs[3] = settings(
                "settings",
                20,
                if invalid_hook {
                    r#"{"mcp":{"allowed":[]},"hooksConfig":{"disabled":false},"footer":true}"#
                } else {
                    r#"{"mcp":{"allowed":[]},"hooksConfig":{"disabled":["child-hook"]},"footer":true}"#
                },
            );
            inputs.push(json_source(
                "extension-enablement",
                &[AgentAssetCategory::Extension],
                30,
                enablement,
            ));
            let trace = run_d2_sources_with_options(
                context(),
                inputs,
                false,
                false,
                Some(Path::new("/tmp/workspace/project")),
                None,
            );
            assert_eq!(trace.records.len(), 5);
            assert_no_projection_diagnostics(&trace);
            let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
            assert_eq!(parent.effective_state, parent_state);
            let mcp = record(&trace, AgentAssetCategory::Mcp, "child");
            assert_eq!(mcp.declared_state, AgentAssetState::Enabled);
            assert_eq!(mcp.effective_state, AgentAssetState::Blocked);
            let mcp_draft = trace
                .drafts
                .iter()
                .find(|draft| draft.native_id == "child")
                .unwrap();
            assert!(matches!(
                &mcp_draft.state_proof.effective,
                AgentAssetEffectiveStateProofDraft::Terminal {
                    cause: AgentAssetTerminalCauseDraft::TypedPolicy,
                    input,
                    ..
                } if **input == AgentAssetEffectiveStateProofDraft::Intrinsic
            ));
            let hook = record(&trace, AgentAssetCategory::Hook, "alpha:BeforeTool:0:0");
            let hook_draft = trace
                .drafts
                .iter()
                .find(|draft| draft.native_kind == AgentAssetCategory::Hook)
                .unwrap();
            let expected_hook = if !invalid_hook || parent_state == AgentAssetState::Disabled {
                AgentAssetState::Disabled
            } else {
                AgentAssetState::Unknown
            };
            assert_eq!(hook.effective_state, expected_hook);
            assert_eq!(
                hook.declared_state,
                if invalid_hook {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            if !invalid_hook {
                assert_eq!(
                    hook_draft.state_proof.effective,
                    AgentAssetEffectiveStateProofDraft::Intrinsic
                );
            } else if parent_state == AgentAssetState::Disabled {
                assert!(matches!(
                    hook_draft.state_proof.effective,
                    AgentAssetEffectiveStateProofDraft::ParentGate { .. }
                ));
                assert!(hook.resolution.control_source.is_none());
            } else {
                assert!(matches!(
                    &hook_draft.state_proof.effective,
                    AgentAssetEffectiveStateProofDraft::Terminal {
                        cause: AgentAssetTerminalCauseDraft::InvalidControl,
                        input,
                        ..
                    } if **input == AgentAssetEffectiveStateProofDraft::Intrinsic
                ));
                assert!(hook.resolution.control_source.is_some());
            }
            let skill = record(&trace, AgentAssetCategory::Skill, "writer");
            assert_eq!(skill.declared_state, AgentAssetState::Enabled);
            assert_eq!(skill.effective_state, parent_state);
            for child in [mcp, hook, skill] {
                assert_eq!(
                    child.relationships.provided_by.as_deref(),
                    Some(parent.stable_id.as_str())
                );
                assert_eq!(
                    child.relationships.action_owner,
                    child.relationships.provided_by
                );
            }
        }
    }
}
