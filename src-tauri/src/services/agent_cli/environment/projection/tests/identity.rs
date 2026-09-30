use super::*;

#[test]
fn participating_mcp_requires_trust_without_claiming_suppression() {
    let category = AgentAssetCategory::Mcp;
    let sources = [source("definition", "plugin.json", &[category])];
    for trust in [AgentTrustState::Required, AgentTrustState::Untrusted] {
        let mut definition = declaration(
            &context(),
            "definition",
            "server",
            category,
            AgentAssetDeclaredState::Enabled,
        );
        definition.trust_state = trust;
        let mut candidate = draft(
            &definition,
            "mcp:server",
            AgentAssetResolutionRelation::Independent,
            &[&definition],
        );
        candidate.effective_state = AgentAssetState::Unknown;
        let AgentAssetDetails::Mcp {
            effective_availability,
            ..
        } = &mut candidate.details
        else {
            unreachable!();
        };
        *effective_availability = AgentAssetEffectiveAvailability::TrustRequired;
        let declarations = [definition.clone()];
        let result = project(&sources, &declarations, vec![candidate.clone()]);
        assert_rows(&result, &["server"]);
        let row = &result.records[0];
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.effective_state, AgentAssetState::Unknown);
        assert_eq!(row.trust_state, trust);
        assert_eq!(row.resolution.contributor_ids, [definition.declaration_id]);
        assert!(row.resolution.terminal.is_none());
        assert!(matches!(
            row.details,
            AgentAssetDetails::Mcp {
                effective_availability: AgentAssetEffectiveAvailability::TrustRequired,
                ..
            }
        ));

        for invalid in 0..3 {
            let mut forged = candidate.clone();
            match invalid {
                0 => {
                    forged.effective_state = AgentAssetState::Enabled;
                    if let AgentAssetDetails::Mcp {
                        effective_availability,
                        ..
                    } = &mut forged.details
                    {
                        *effective_availability = AgentAssetEffectiveAvailability::Available;
                    }
                }
                1 => forged.contributor_ids.clear(),
                2 => {
                    forged.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
                    forged.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
                        terminal: AgentAssetResolutionTerminal::Unknown,
                        cause: AgentAssetTerminalCauseDraft::TrustSuppressed,
                        evidence: vec![],
                        input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                    };
                }
                _ => unreachable!(),
            }
            assert!(project(&sources, &declarations, vec![forged])
                .records
                .is_empty());
        }
    }

    let mut trusted = declaration(
        &context(),
        "definition",
        "server",
        category,
        AgentAssetDeclaredState::Enabled,
    );
    trusted.trust_state = AgentTrustState::Trusted;
    let mut forged = draft(
        &trusted,
        "mcp:server",
        AgentAssetResolutionRelation::Independent,
        &[&trusted],
    );
    forged.effective_state = AgentAssetState::Unknown;
    if let AgentAssetDetails::Mcp {
        effective_availability,
        ..
    } = &mut forged.details
    {
        *effective_availability = AgentAssetEffectiveAvailability::TrustRequired;
    }
    assert!(project(&sources, &[trusted], vec![forged])
        .records
        .is_empty());
}

#[test]
fn control_source_changes_preserve_identity_and_full_evidence() {
    let category = AgentAssetCategory::Mcp;
    let definition = declaration(
        &context(),
        "definition",
        "server",
        category,
        AgentAssetDeclaredState::Enabled,
    );
    let definition_source = source("definition", "manifest.json", &[category]);
    let baseline = project(
        std::slice::from_ref(&definition_source),
        std::slice::from_ref(&definition),
        vec![draft(
            &definition,
            "mcp:server",
            AgentAssetResolutionRelation::Independent,
            &[&definition],
        )],
    );
    assert_rows(&baseline, &["server"]);
    for control_path in ["settings.json", "other-settings.json"] {
        let control_source = source("control", control_path, &[category]);
        let mut control = declaration_with_key(
            &context(),
            "control",
            "server",
            "disabled:server",
            category,
            AgentAssetDeclaredState::Disabled,
        );
        control.role = AgentAssetDeclarationRole::StateOverlay;
        let mut disabled = draft(
            &definition,
            "mcp:server",
            AgentAssetResolutionRelation::Independent,
            &[&definition, &control],
        );
        disabled.declared_state = AgentAssetDeclaredState::Disabled;
        disabled.effective_state = AgentAssetState::Disabled;
        disabled.details = details(category, AgentAssetDeclaredState::Disabled);
        disabled.state_proof.declared = AgentAssetDeclaredStateProofDraft::Overlay {
            declaration_ids: vec![control.declaration_id.clone()],
            scope: crate::services::agent_cli::contracts::AgentAssetStateOverlayScopeDraft::ExactNativeId,
            outcome: AgentAssetDeclaredState::Disabled,
        };
        let sources = [definition_source.clone(), control_source.clone()];
        let declarations = [definition.clone(), control.clone()];
        let changed = project(&sources, &declarations, vec![disabled.clone()]);
        assert_rows(&changed, &["server"]);
        let row = &changed.records[0];
        assert_eq!(row.stable_id, baseline.records[0].stable_id);
        assert_eq!(row.effective_state, AgentAssetState::Disabled);
        assert_eq!(row.source_ids.len(), 2);
        assert!(row
            .source_ids
            .contains(&source_stable_id(&context(), &control_source.path)));
        assert!(row
            .represented_declaration_ids
            .contains(&control.declaration_id));
        assert!(row
            .resolution
            .contributor_ids
            .contains(&control.declaration_id));
        let mut moved_sources = sources;
        moved_sources[0].path.set_file_name("moved-manifest.json");
        let moved = project(&moved_sources, &declarations, vec![disabled]);
        assert_rows(&moved, &["server"]);
        assert_ne!(moved.records[0].stable_id, row.stable_id);
    }
}

#[test]
fn trust_suppressed_identity_tracks_all_definitions_but_not_control_sources() {
    let category = AgentAssetCategory::Mcp;
    let mut definition = declaration(
        &context(),
        "main",
        "server",
        category,
        AgentAssetDeclaredState::Unknown,
    );
    definition.trust_state = AgentTrustState::Untrusted;
    definition.participation = AgentAssetResolutionParticipation::Suppressed {
        reason: crate::models::AgentAssetSuppressionReason::UntrustedWorkspace,
    };
    let mut peer = definition.clone();
    peer.source_key = "peer".into();
    peer.declaration_id = declaration(
        &context(),
        "peer",
        "server",
        category,
        AgentAssetDeclaredState::Unknown,
    )
    .declaration_id;
    let mut control = definition.clone();
    control.source_key = "control".into();
    control.declaration_id = declaration(
        &context(),
        "control",
        "server",
        category,
        AgentAssetDeclaredState::Unknown,
    )
    .declaration_id;
    control.role = AgentAssetDeclarationRole::StateOverlay;
    let sources = [
        source("main", "main.json", &[category]),
        source("peer", "peer.json", &[category]),
        source("control", "control.json", &[category]),
    ];
    let run = |sources: &[AgentAssetSourceSpec], declarations: &[ParsedAgentAsset]| {
        let mut candidate = draft(
            &definition,
            "mcp:server",
            AgentAssetResolutionRelation::Unknown,
            &[],
        );
        candidate.represented_declaration_ids = declarations
            .iter()
            .map(|asset| asset.declaration_id.clone())
            .collect();
        candidate.resolution.terminal = Some(AgentAssetResolutionTerminal::Unknown);
        let mut evidence = declarations
            .iter()
            .filter(|asset| asset.role == AgentAssetDeclarationRole::Definition)
            .map(|asset| AgentAssetStateEvidenceRefDraft::Declaration {
                declaration_id: asset.declaration_id.clone(),
            })
            .collect::<Vec<_>>();
        evidence.sort();
        candidate.state_proof.declared = AgentAssetDeclaredStateProofDraft::Unknown {
            evidence,
            cause: AgentAssetDeclaredUnknownCauseDraft::IntrinsicUnknown,
        };
        candidate.state_proof.effective = AgentAssetEffectiveStateProofDraft::Terminal {
            terminal: AgentAssetResolutionTerminal::Unknown,
            cause: AgentAssetTerminalCauseDraft::TrustSuppressed,
            evidence: vec![],
            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
        };
        if let AgentAssetDetails::Mcp {
            effective_availability,
            ..
        } = &mut candidate.details
        {
            *effective_availability = AgentAssetEffectiveAvailability::TrustRequired;
        }
        let result = project(sources, declarations, vec![candidate]);
        assert_rows(&result, &["server"]);
        let row = result.records.into_iter().next().unwrap();
        assert!(row.resolution.contributor_ids.is_empty());
        assert_eq!(row.effective_state, AgentAssetState::Unknown);
        row
    };
    let one = run(&sources, std::slice::from_ref(&definition));
    let two = run(&sources, &[definition.clone(), peer.clone()]);
    assert_ne!(one.stable_id, two.stable_id);
    let controlled = run(
        &sources,
        &[definition.clone(), peer.clone(), control.clone()],
    );
    assert_eq!(controlled.stable_id, two.stable_id);
    assert_eq!(controlled.source_ids.len(), 3);
    assert!(controlled
        .represented_declaration_ids
        .contains(&control.declaration_id));
    let mut moved = sources;
    moved[2].path.set_file_name("other-control.json");
    assert_eq!(
        run(&moved, &[definition.clone(), peer.clone(), control.clone()]).stable_id,
        two.stable_id
    );
    moved[1].path.set_file_name("other-peer.json");
    assert_ne!(
        run(&moved, &[definition.clone(), peer, control]).stable_id,
        two.stable_id
    );
}
