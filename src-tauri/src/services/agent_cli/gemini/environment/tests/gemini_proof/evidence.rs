use super::*;

/// Native assessment must account for every supplied source peer. Give actual
/// parser output another source identity; no candidate supplies expected state.
pub(super) fn decoded_peer(
    trace: &D2ProjectionTrace,
    source_key: &str,
    peer_key: &str,
) -> (AgentAssetSourceSpec, Vec<ParsedAgentAsset>) {
    let mut source = trace
        .sources
        .iter()
        .find(|source| source.native_source_key == source_key)
        .unwrap()
        .clone();
    source.native_source_key = peer_key.to_owned();
    source.path = format!("/tmp/gemini/{peer_key}.json").into();
    let declarations = trace
        .declarations
        .iter()
        .filter(|declaration| declaration.source_key == source_key)
        .map(|declaration| {
            let mut peer = declaration.clone();
            peer.source_key = peer_key.to_owned();
            reset_id(&context(), &mut peer);
            peer
        })
        .collect::<Vec<_>>();
    assert!(!declarations.is_empty());
    (source, declarations)
}

#[test]
fn gemini_a5_hook_policy_requires_every_matching_cross_group_witness() {
    let trace = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                20,
                r#"{"hooksConfig":{"disabled":["run"]},"hooks":{"BeforeTool":[{"hooks":[{"type":"runtime","name":"run"},{"type":"runtime","name":"idle"}]}]},"footer":true}"#,
            ),
            settings(
                "system-defaults",
                10,
                r#"{"hooksConfig":{"disabled":["run"]}}"#,
            ),
            settings(
                "system-settings",
                30,
                r#"{"hooksConfig":{"disabled":["unrelated"]}}"#,
            ),
        ],
    );
    assert_eq!(trace.records.len(), 3);
    assert_eq!(
        record(&trace, AgentAssetCategory::Hook, "BeforeTool:0:0").effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        record(&trace, AgentAssetCategory::Hook, "BeforeTool:0:1").effective_state,
        AgentAssetState::Enabled
    );
    assert_no_projection_diagnostics(&trace);
    let (state, _) = assessed(&trace, AgentAssetCategory::Hook, "BeforeTool:0:0");
    let AgentAssetDeclaredStateProofDraft::Policy {
        declaration_ids,
        outcome: AgentAssetDeclaredState::Disabled,
    } = &state.declared
    else {
        panic!("known native disabled policy");
    };
    assert_eq!(declaration_ids.len(), 2);
    assert_eq!(state.declared_members.len(), 2);
    let matching_sources = declaration_ids
        .iter()
        .map(|id| {
            let declaration = trace
                .declarations
                .iter()
                .find(|declaration| declaration.declaration_id == *id)
                .unwrap();
            assert_eq!(declaration.role, AgentAssetDeclarationRole::PolicyOverlay);
            assert_ne!(declaration.resolution_group_key, "BeforeTool:0:0");
            declaration.source_key.as_str()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        matching_sources,
        BTreeSet::from(["settings", "system-defaults"])
    );
    let nonmatching_id = trace
        .declarations
        .iter()
        .find(|declaration| {
            declaration.source_key == "system-settings"
                && declaration.category == AgentAssetCategory::Hook
        })
        .unwrap()
        .declaration_id
        .clone();

    for attack in 0..4 {
        let mut drafts = trace.drafts.clone();
        let draft = drafts
            .iter_mut()
            .find(|draft| draft.native_id == "BeforeTool:0:0")
            .unwrap();
        assert!(declaration_ids.iter().all(|id| {
            !draft.contributor_ids.contains(id) && !draft.represented_declaration_ids.contains(id)
        }));
        let AgentAssetDeclaredStateProofDraft::Policy {
            declaration_ids,
            outcome,
        } = &mut draft.state_proof.declared
        else {
            panic!("native policy candidate");
        };
        match attack {
            0 => {
                declaration_ids.pop();
            }
            1 => declaration_ids.push(nonmatching_id.clone()),
            2 => declaration_ids.push(declaration_ids[0].clone()),
            3 => *outcome = AgentAssetDeclaredState::Unknown,
            _ => unreachable!(),
        }
        let forged = reproject(&trace, drafts);
        rejected(&forged, AgentAssetCategory::Hook, "BeforeTool:0:0");
        assert_eq!(forged.records.len(), 2, "candidate attack {attack}");
        assert_eq!(forged.declarations.len(), trace.declarations.len());
    }

    for attack in 0..4 {
        let mut declarations = trace.declarations.clone();
        let witness = declarations
            .iter_mut()
            .find(|declaration| declaration.declaration_id == declaration_ids[0])
            .unwrap();
        match attack {
            0 => witness.role = AgentAssetDeclarationRole::StateOverlay,
            1 => {
                witness.category = AgentAssetCategory::Mcp;
                reset_id(&context(), witness);
            }
            2 => witness.logical_origin.scope = AgentAssetScope::Workspace,
            3 => {
                witness.participation = AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                };
            }
            _ => unreachable!(),
        }
        let forged = project_trace(
            context(),
            trace.sources.clone(),
            declarations,
            trace.drafts.clone(),
            Vec::new(),
        );
        rejected(&forged, AgentAssetCategory::Hook, "BeforeTool:0:0");
        assert_eq!(forged.records.len(), 1, "input attack {attack}");
        assert_eq!(forged.records[0].native_id, "footer");
    }
}

#[test]
fn gemini_a5_context_unavailable_requires_complete_valid_overlay_peers() {
    let initial = run_d2_sources(
        context(),
        vec![
            json_source(
                "gemini-extension-manifest:alpha",
                &[AgentAssetCategory::Extension],
                10,
                r#"{"name":"alpha","version":"1.0.0"}"#,
            ),
            json_source(
                "extension-enablement",
                &[AgentAssetCategory::Extension],
                30,
                r#"{"alpha":{"overrides":["/tmp/workspace/*"]}}"#,
            ),
            settings("settings", 20, r#"{"footer":true}"#),
        ],
    );
    let (peer, peer_declarations) = decoded_peer(
        &initial,
        "extension-enablement",
        "peer-extension-enablement",
    );
    let mut sources = initial.sources;
    sources.push(peer);
    let mut declarations = initial.declarations;
    declarations.extend(peer_declarations);
    let trace = resolve_raw(context(), sources, declarations);
    assert_eq!(trace.records.len(), 2);
    assert_eq!(
        record(&trace, AgentAssetCategory::Extension, "alpha").effective_state,
        AgentAssetState::Unknown
    );
    assert_no_projection_diagnostics(&trace);
    assert!(!trace
        .diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Malformed { .. })));
    let (state, _) = assessed(&trace, AgentAssetCategory::Extension, "alpha");
    assert!(matches!(
        state.declared,
        AgentAssetDeclaredStateProofDraft::Unknown {
            cause: AgentAssetDeclaredUnknownCauseDraft::ContextUnavailable,
            ..
        }
    ));
    assert_eq!(state.declared_members.len(), 2);
    assert!(state.declared_members.iter().all(|member| matches!(
        member.kind,
        AgentAssetEvidenceKind::Overlay {
            scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup
        }
    )));

    // Equivalent whole-source evidence is accepted; overlapping notation is not.
    let mut source_aliases = trace.drafts.clone();
    let draft = source_aliases
        .iter_mut()
        .find(|draft| draft.native_id == "alpha")
        .unwrap();
    let AgentAssetDeclaredStateProofDraft::Unknown { evidence, .. } =
        &mut draft.state_proof.declared
    else {
        panic!("native context evidence");
    };
    *evidence = ["extension-enablement", "peer-extension-enablement"]
        .into_iter()
        .map(|source_key| AgentAssetStateEvidenceRefDraft::Source {
            source_key: source_key.to_owned(),
        })
        .collect();
    let aliases = reproject(&trace, source_aliases);
    assert_eq!(aliases.records.len(), 2);
    assert_no_projection_diagnostics(&aliases);

    for attack in 0..4 {
        let mut drafts = trace.drafts.clone();
        let draft = drafts
            .iter_mut()
            .find(|draft| draft.native_id == "alpha")
            .unwrap();
        let AgentAssetDeclaredStateProofDraft::Unknown { evidence, cause } =
            &mut draft.state_proof.declared
        else {
            unreachable!()
        };
        match attack {
            0 => {
                evidence.pop();
            }
            1 => *cause = AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl,
            2 => evidence.push(AgentAssetStateEvidenceRefDraft::Source {
                source_key: "extension-enablement".to_owned(),
            }),
            3 => evidence.push(evidence[0].clone()),
            _ => unreachable!(),
        }
        let forged = reproject(&trace, drafts);
        rejected(&forged, AgentAssetCategory::Extension, "alpha");
        assert_eq!(forged.records.len(), 1, "context evidence attack {attack}");
        assert_eq!(forged.records[0].native_id, "footer");
    }

    for attack in 0..3 {
        let mut declarations = trace.declarations.clone();
        for witness in declarations
            .iter_mut()
            .filter(|declaration| declaration.role == AgentAssetDeclarationRole::StateOverlay)
        {
            match attack {
                0 => {
                    witness.native_payload =
                        AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
                            GeminiExtensionEnablementUnknownCause::InvalidEntry,
                        );
                }
                1 => {
                    witness.native_payload = AgentAssetNativePayload::None;
                    witness.declared_state = AgentAssetDeclaredState::Enabled;
                    if let AgentAssetDetails::Extension { enabled, .. } = &mut witness.details {
                        *enabled = AgentAssetDeclaredState::Enabled;
                    }
                }
                2 => {
                    witness.native_payload = AgentAssetNativePayload::InvalidControl(
                        AgentAssetInvalidControl::ExtensionEnablement,
                    );
                }
                _ => unreachable!(),
            }
        }
        let forged = project_trace(
            context(),
            trace.sources.clone(),
            declarations,
            trace.drafts.clone(),
            Vec::new(),
        );
        rejected(&forged, AgentAssetCategory::Extension, "alpha");
        assert_eq!(forged.records.len(), 1, "wrong context marker {attack}");
        assert_eq!(forged.records[0].native_id, "footer");
    }
}

#[test]
fn gemini_a5_global_invalid_controls_preserve_definition_default_and_overlay_axes() {
    let initial = run_d2_sources(
        context(),
        vec![
            settings(
                "settings",
                20,
                r#"{"mcpServers":{"server":{"command":"run"}},"footer":true}"#,
            ),
            json_source(
                "gemini-extension-manifest:alpha",
                &[AgentAssetCategory::Extension],
                10,
                r#"{"name":"alpha","version":"1.0.0"}"#,
            ),
            json_source("mcp-enablement", &[AgentAssetCategory::Mcp], 30, "["),
            json_source(
                "extension-enablement",
                &[AgentAssetCategory::Extension],
                30,
                "[",
            ),
        ],
    );
    assert_eq!(initial.records.len(), 3);
    assert_no_projection_diagnostics(&initial);
    let known_overlays = run_d2_sources(
        context(),
        vec![
            json_source(
                "mcp-enablement",
                &[AgentAssetCategory::Mcp],
                40,
                r#"{"server":{"enabled":false}}"#,
            ),
            json_source(
                "extension-enablement",
                &[AgentAssetCategory::Extension],
                40,
                r#"{"alpha":{"overrides":[]}}"#,
            ),
        ],
    );
    assert!(known_overlays.records.is_empty());
    assert_eq!(known_overlays.declarations.len(), 2);
    for with_overlay in [false, true] {
        for multiple_controls in [false, true] {
            let mut sources = initial.sources.clone();
            let mut declarations = initial.declarations.clone();
            for (original_key, overlay_key, invalid_key) in [
                ("mcp-enablement", "known-mcp-peer", "invalid-mcp-peer"),
                (
                    "extension-enablement",
                    "known-extension-peer",
                    "invalid-extension-peer",
                ),
            ] {
                if with_overlay {
                    let (source, peers) = decoded_peer(&known_overlays, original_key, overlay_key);
                    sources.push(source);
                    declarations.extend(peers);
                }
                if multiple_controls {
                    let (source, peers) = decoded_peer(&initial, original_key, invalid_key);
                    sources.push(source);
                    declarations.extend(peers);
                }
            }
            let trace = resolve_raw(context(), sources, declarations);
            assert_eq!(trace.records.len(), 3);
            assert_no_projection_diagnostics(&trace);
            for (category, id) in [
                (AgentAssetCategory::Mcp, "server"),
                (AgentAssetCategory::Extension, "alpha"),
            ] {
                let row = record(&trace, category, id);
                assert_eq!(row.effective_state, AgentAssetState::Unknown);
                assert_eq!(
                    row.resolution.terminal,
                    Some(AgentAssetResolutionTerminal::Unknown)
                );
                let (state, _) = assessed(&trace, category, id);
                if with_overlay {
                    let expected = if category == AgentAssetCategory::Mcp {
                        AgentAssetDeclaredState::Disabled
                    } else {
                        AgentAssetDeclaredState::Enabled
                    };
                    assert_eq!(state.declared_state, expected);
                    assert!(matches!(
                        &state.declared,
                        AgentAssetDeclaredStateProofDraft::Overlay {
                            declaration_ids,
                            outcome,
                            scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup,
                        } if *outcome == expected && declaration_ids.len() == 1
                    ));
                } else {
                    assert_eq!(state.declared_state, AgentAssetDeclaredState::Enabled);
                    if category == AgentAssetCategory::Mcp {
                        assert!(matches!(
                            state.declared,
                            AgentAssetDeclaredStateProofDraft::Definition { .. }
                        ));
                    } else {
                        assert!(matches!(
                            state.declared,
                            AgentAssetDeclaredStateProofDraft::NativeDefault { .. }
                        ));
                    }
                }
                let control = state.control.unwrap();
                assert_eq!(control.cause, AgentAssetTerminalCauseDraft::InvalidControl);
                assert_eq!(control.members.len(), if multiple_controls { 2 } else { 1 });
                assert_eq!(control.authorities.len(), control.members.len());
                assert!(control.authorities.iter().all(|authority| matches!(
                    authority,
                    AgentAssetControlAuthority::Declaration(_)
                )));
                if multiple_controls {
                    assert!(row.resolution.control_source.is_none());
                } else {
                    assert_eq!(
                        row.resolution.control_source,
                        Some(AgentAssetPolicyReference::Declaration {
                            declaration_id: control.members[0].declaration_id.clone(),
                        })
                    );
                }
            }
            assert_eq!(
                record(&trace, AgentAssetCategory::StatusUi, "footer").effective_state,
                AgentAssetState::Enabled
            );
        }
    }
}
