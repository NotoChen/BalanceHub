use super::*;
use crate::services::agent_cli::contracts::{
    AgentAssetAssessmentFailure, AgentAssetControlAuthority, AgentAssetEvidenceKind,
    AgentAssetTerminalCauseDraft,
};
use schema::feature::{HookFeatureLayer, POLICY_KEY};

fn feature<'a>(run: &'a Run, source_key: &str) -> &'a ParsedAgentAsset {
    let observations = run
        .capture
        .declarations
        .iter()
        .filter(|asset| {
            asset.source_key == source_key
                && matches!(
                    asset.native_payload,
                    AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy { .. })
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 1, "{source_key}");
    let observation = observations[0];
    assert_eq!(observation.declaration_key, POLICY_KEY);
    assert_eq!(observation.role, AgentAssetDeclarationRole::PolicyOverlay);
    assert!(run.inventory.assets.iter().all(|row| {
        !row.represented_declaration_ids
            .contains(&observation.declaration_id)
            && !row
                .resolution
                .contributor_ids
                .contains(&observation.declaration_id)
    }));
    observation
}

fn standalone(fixture: &Fixture) -> (String, String) {
    let document = hook_file("bh-feature-hook");
    let path = fixture.home.join(".codex/hooks.json");
    write(&path, serde_json::to_vec(&document).unwrap());
    let key = format!("{}:session_start:0:0", path.display());
    let trust = trusted_state(&key, &document);
    (key, trust)
}

#[test]
fn hook_feature_gate_preserves_rule_state_trust_count_and_identity() {
    let fixture = Fixture::new("hook-feature-state");
    let (key, trust) = standalone(&fixture);
    let mut identity = None;
    for (config, gate) in [
        ("", None),
        ("[features]\nhooks=true\n", None),
        ("[features]\nhooks=false\n", Some(AgentAssetState::Blocked)),
        (
            "[features]\nhooks='invalid'\n",
            Some(AgentAssetState::Unknown),
        ),
        ("features='invalid'\n", Some(AgentAssetState::Unknown)),
    ] {
        for (state, enabled, record_trust) in [
            (String::new(), true, AgentTrustState::Untrusted),
            (trust.clone(), true, AgentTrustState::Trusted),
            (
                format!("{trust}enabled=false\n"),
                false,
                AgentTrustState::Trusted,
            ),
        ] {
            let config = format!("{config}{state}");
            let run = fixture.run_with_workspace::<false>(&config, false);
            assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
            assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(1));
            let row = run.row(AgentAssetCategory::Hook, &key);
            let declared = if enabled {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            };
            let intrinsic = if !enabled {
                AgentAssetState::Disabled
            } else if record_trust == AgentTrustState::Trusted {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Unknown
            };
            assert_eq!(row.declared_state, declared, "{config}");
            assert_eq!(row.effective_state, gate.unwrap_or(intrinsic), "{config}");
            assert_eq!(row.trust_state, record_trust, "{config}");
            assert_eq!(
                row.details,
                AgentAssetDetails::Hook {
                    managed: false,
                    enabled: if enabled {
                        AgentAssetDeclaredState::Enabled
                    } else {
                        AgentAssetDeclaredState::Disabled
                    },
                    rule_count: Some(1),
                }
            );
            assert_eq!(
                row.resolution.terminal,
                gate.map(|gate| if gate == AgentAssetState::Blocked {
                    AgentAssetResolutionTerminal::PolicyBlocked
                } else {
                    AgentAssetResolutionTerminal::Unknown
                })
            );
            assert_eq!(row.represented_declaration_ids.len(), 1);
            assert_eq!(
                row.resolution.contributor_ids,
                row.represented_declaration_ids
            );
            let definition = run
                .capture
                .declarations
                .iter()
                .find(|asset| asset.native_id == key)
                .unwrap();
            assert_eq!(definition.declared_state, AgentAssetDeclaredState::Unknown);
            assert_eq!(definition.trust_state, AgentTrustState::Unknown);
            let observation = feature(&run, "config");
            assert_eq!(
                row.resolution.control_source,
                gate.map(|_| AgentAssetPolicyReference::Declaration {
                    declaration_id: observation.declaration_id.clone(),
                })
            );
            let current = (
                row.stable_id.clone(),
                row.represented_declaration_ids.clone(),
                row.resolution.contributor_ids.clone(),
            );
            if let Some(previous) = &identity {
                assert_eq!(previous, &current);
            }
            identity = Some(current);
            let reversed = fixture.run_with_workspace::<true>(&config, false);
            assert_eq!(run.normalized_rows(), reversed.normalized_rows());
        }
    }
}

#[test]
fn hook_feature_layers_follow_native_table_replacement_and_leaf_preservation() {
    let fixture = Fixture::new("hook-feature-merge");
    let (key, trust) = standalone(&fixture);
    for (user, workspace, expected, owner) in [
        (
            "[features]\nhooks=false\n",
            "[features]\nhooks=true\n",
            AgentAssetState::Enabled,
            None,
        ),
        (
            "[features]\nhooks=true\n",
            "[features]\nhooks=false\n",
            AgentAssetState::Blocked,
            Some("workspace-config"),
        ),
        (
            "[features]\nhooks='invalid'\n",
            "[features]\nhooks=true\n",
            AgentAssetState::Enabled,
            None,
        ),
        (
            "features='invalid'\n",
            "[features]\n",
            AgentAssetState::Enabled,
            None,
        ),
        (
            "[features]\nhooks='invalid'\n",
            "[features]\n",
            AgentAssetState::Unknown,
            Some("config"),
        ),
        (
            "[features]\nhooks=false\n",
            "[features]\n",
            AgentAssetState::Blocked,
            Some("config"),
        ),
        (
            "[features]\nhooks=false\n",
            "[features]\nother_feature=true\n",
            AgentAssetState::Blocked,
            Some("config"),
        ),
        (
            "[features]\nhooks=false\n",
            "",
            AgentAssetState::Blocked,
            Some("config"),
        ),
        (
            "[features]\nhooks=true\n",
            "features='invalid'\n",
            AgentAssetState::Unknown,
            Some("workspace-config"),
        ),
    ] {
        write(&fixture.workspace.join(".codex/config.toml"), workspace);
        let config = format!("{user}{trust}");
        let run = fixture.run::<false>(&config);
        assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
        assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(1));
        let row = run.row(AgentAssetCategory::Hook, &key);
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.trust_state, AgentTrustState::Trusted);
        assert_eq!(
            row.effective_state, expected,
            "user={user}; workspace={workspace}"
        );
        assert_eq!(
            row.resolution.control_source,
            owner.map(|source| AgentAssetPolicyReference::Declaration {
                declaration_id: feature(&run, source).declaration_id.clone(),
            })
        );
        let reversed = fixture.run::<true>(&config);
        assert_eq!(run.normalized_rows(), reversed.normalized_rows());
    }
}

#[cfg(unix)]
#[test]
fn hook_system_feature_and_unavailable_source_keep_distinct_control_authorities() {
    let fixture = Fixture::new("hook-feature-system");
    let (key, trust) = standalone(&fixture);
    for (system, user, expected) in [
        ("[features]\nhooks=false\n", "", AgentAssetState::Blocked),
        (
            "[features]\nhooks=false\n",
            "[features]\nhooks=true\n",
            AgentAssetState::Enabled,
        ),
        (
            "[features]\nhooks='invalid'\n",
            "[features]\nhooks=true\n",
            AgentAssetState::Enabled,
        ),
        ("[", "[features]\nhooks=true\n", AgentAssetState::Unknown),
    ] {
        write(
            &fixture.home.join(".codex-fixture-system/config.toml"),
            system,
        );
        let config = format!("{user}{trust}");
        let run = fixture.run_with_workspace::<false>(&config, false);
        assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
        let row = run.row(AgentAssetCategory::Hook, &key);
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.trust_state, AgentTrustState::Trusted);
        assert_eq!(row.effective_state, expected);
        assert_eq!(
            row.resolution.control_source,
            (expected != AgentAssetState::Enabled).then(|| {
                AgentAssetPolicyReference::Declaration {
                    declaration_id: feature(&run, "system-config").declaration_id.clone(),
                }
            })
        );
        if system == "[" {
            assert!(matches!(
                feature(&run, "system-config").native_payload,
                AgentAssetNativePayload::CodexHook(CodexHookPayload::FeaturePolicy {
                    layer: HookFeatureLayer::Unavailable
                })
            ));
        } else {
            assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(1));
        }
    }
}

#[test]
fn untrusted_workspace_cannot_override_the_participating_hook_feature_gate() {
    let fixture = Fixture::new("hook-feature-workspace-trust");
    let (key, trust) = standalone(&fixture);
    write(
        &fixture.workspace.join(".codex/config.toml"),
        "[features]\nhooks=true\n",
    );
    let config = format!("[features]\nhooks=false\n{trust}");
    let run = fixture.run_with_trust::<false>(&config, Some(false));
    assert_eq!(
        run.inventory.contexts[0].trust_context,
        AgentTrustState::Untrusted
    );
    assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
    assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(1));
    assert_eq!(
        run.row(AgentAssetCategory::Hook, &key).effective_state,
        AgentAssetState::Blocked
    );
    assert_eq!(
        feature(&run, "workspace-config").participation,
        AgentAssetResolutionParticipation::Suppressed {
            reason: AgentAssetSuppressionReason::UntrustedWorkspace,
        }
    );
    assert_eq!(
        run.row(AgentAssetCategory::Hook, &key)
            .resolution
            .control_source,
        Some(AgentAssetPolicyReference::Declaration {
            declaration_id: feature(&run, "config").declaration_id.clone(),
        })
    );
    let reversed = fixture.run_with_trust::<true>(&config, Some(false));
    assert_eq!(run.normalized_rows(), reversed.normalized_rows());
}

#[test]
fn hook_feature_gate_keeps_plugin_children_and_parent_configuration_intact() {
    let fixture = Fixture::new("hook-feature-plugin");
    let root = fixture.plugin("hooked@fixture", "local", "hooked");
    let document = hook_file("bh-feature-plugin");
    write(
        &root.join("hooks/hooks.json"),
        serde_json::to_vec(&document).unwrap(),
    );
    let key = "hooked@fixture:hooks/hooks.json:session_start:0:0";
    let mut identity = None;
    for parent_enabled in [true, false] {
        for feature_enabled in [true, false] {
            let config = format!("[features]\nhooks={feature_enabled}\n[plugins.'hooked@fixture']\nenabled={parent_enabled}\n{}", trusted_state(key, &document));
            let run = fixture.run::<false>(&config);
            assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
            assert_eq!(run.inventory.hook_rule_counts[0].rule_count, Some(1));
            let row = run.row(AgentAssetCategory::Hook, key);
            let parent = run.row(AgentAssetCategory::Plugin, "hooked@fixture");
            assert_eq!(
                parent.effective_state,
                if parent_enabled {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            assert_eq!(row.declared_state, AgentAssetState::Enabled);
            assert_eq!(row.trust_state, AgentTrustState::Trusted);
            assert_eq!(
                row.effective_state,
                if !feature_enabled {
                    AgentAssetState::Blocked
                } else if parent_enabled {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            assert_eq!(
                row.relationships.provided_by.as_deref(),
                Some(parent.stable_id.as_str())
            );
            assert_eq!(
                row.relationships.action_owner.as_deref(),
                Some(parent.stable_id.as_str())
            );
            assert!(!row.writable);
            assert_eq!(row.represented_declaration_ids.len(), 1);
            feature(&run, "config");
            if let Some(previous) = &identity {
                assert_eq!(previous, &row.stable_id);
            }
            identity = Some(row.stable_id.clone());
        }
    }
}

#[test]
fn independent_hook_feature_assessment_rejects_missing_duplicate_and_wrong_origin_witnesses() {
    let fixture = Fixture::new("hook-feature-proof");
    let (key, trust) = standalone(&fixture);
    let run = fixture.run::<false>(&format!("[features]\nhooks=false\n{trust}"));
    assert_eq!(run.rows(AgentAssetCategory::Hook).len(), 1);
    let raw = &run.capture.declarations;
    let witness = feature(&run, "config");
    let AgentAssetAssessmentResult::Assessed { state, .. } =
        run.assess(raw, AgentAssetCategory::Hook, &key)
    else {
        panic!("valid native inputs must be independently assessed")
    };
    assert_eq!(state.intrinsic.trust_state, AgentTrustState::Trusted);
    let control = state.control.unwrap();
    assert_eq!(control.cause, AgentAssetTerminalCauseDraft::TypedPolicy);
    assert_eq!(control.members.len(), 1);
    assert_eq!(control.members[0].declaration_id, witness.declaration_id);
    assert_eq!(control.members[0].kind, AgentAssetEvidenceKind::Policy);
    assert_eq!(
        control.authorities,
        [AgentAssetControlAuthority::Declaration(
            witness.declaration_id.clone()
        )]
        .into_iter()
        .collect()
    );
    for source in ["config", "workspace-config"] {
        let id = feature(&run, source).declaration_id.as_str();
        let missing = raw
            .iter()
            .filter(|asset| asset.declaration_id != id)
            .cloned()
            .collect::<Vec<_>>();
        assert!(matches!(
            run.assess(&missing, AgentAssetCategory::Hook, &key),
            AgentAssetAssessmentResult::Unsupported(AgentAssetAssessmentFailure::IncompleteInput)
        ));
    }
    let mut duplicate = raw.clone();
    duplicate.push(witness.clone());
    assert!(matches!(
        run.assess(&duplicate, AgentAssetCategory::Hook, &key),
        AgentAssetAssessmentResult::Unsupported(_)
    ));
    for mutation in 0..4 {
        let mut changed = raw.clone();
        let observation = changed
            .iter_mut()
            .find(|asset| asset.declaration_id == witness.declaration_id)
            .unwrap();
        match mutation {
            0 => observation.logical_origin.precedence += 1,
            1 => observation.role = AgentAssetDeclarationRole::Definition,
            2 => {
                observation.participation = AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                }
            }
            _ => observation.trust_state = AgentTrustState::Trusted,
        }
        assert!(matches!(
            run.assess(&changed, AgentAssetCategory::Hook, &key),
            AgentAssetAssessmentResult::Unsupported(
                AgentAssetAssessmentFailure::InvalidNativeInput
            )
        ));
    }
}
