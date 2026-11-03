use super::*;

fn write(path: &Path, content: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn skill(path: &Path, name: &str) {
    write(path, format!("---\nname: {name}\ndescription: private-skill-description-sentinel\n---\nprivate-skill-body-sentinel\n"));
}

fn isolated_sources(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    let mut native = FollowUpCollector::default();
    discover_sources(request, &mut native);
    for mut source in native.initial_sources {
        if matches!(
            source.native_source_key.as_str(),
            "system-defaults" | "system-settings"
        ) {
            source.allowed_root = Path::new(&request.context.config_root).join("managed-fixture");
            source.path = source
                .allowed_root
                .join(format!("{}.json", source.native_source_key));
        }
        if output.emit_initial(source).is_break() {
            return;
        }
    }
}

pub(super) fn inventory(home: &Path, workspace: Option<&Path>) -> AgentEnvironmentInventory {
    let native = crate::services::agent_cli::definition(AgentCliKind::Gemini);
    let definition = crate::services::agent_cli::AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            discover_contexts,
            isolated_sources,
            Some(discover_follow_up_sources),
            "@google/gemini-cli",
            parse_assets,
            resolve_assets,
            assess_assets,
        )
        .with_workspace_trust_authority(discover_workspace_trust_sources, resolve_workspace_trust),
        ..*native
    };
    let inventory = crate::services::agent_cli::environment::build_inventory_for_test(
        home,
        workspace,
        &[definition],
    )
    .unwrap();
    let diagnostics = inventory
        .diagnostics
        .iter()
        .chain(
            inventory
                .sources
                .iter()
                .flat_map(|source| &source.diagnostics),
        )
        .chain(inventory.assets.iter().flat_map(|asset| &asset.diagnostics));
    for diagnostic in diagnostics {
        assert!(
            !matches!(
                diagnostic,
                AgentAssetDiagnostic::InvalidProjection { .. }
                    | AgentAssetDiagnostic::InvalidResolution { .. }
                    | AgentAssetDiagnostic::UnresolvedRelationship { .. }
            ),
            "{diagnostic:?}"
        );
    }
    let debug = format!("{inventory:?}");
    for sentinel in [
        "private-skill-description-sentinel",
        "private-skill-body-sentinel",
        "private-disabled-name-sentinel",
    ] {
        assert!(!debug.contains(sentinel));
    }
    assert!(inventory
        .contexts
        .iter()
        .all(|context| context.parser_version == 3));
    inventory
}

fn row<'a>(inventory: &'a AgentEnvironmentInventory, name: &str) -> &'a AgentAssetRecord {
    inventory
        .assets
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Skill
                && asset.native_id == name
                && asset.resolution.relation != AgentAssetResolutionRelation::Replaced
        })
        .unwrap_or_else(|| panic!("missing Skill {name}"))
}

fn trust(home: &Path, workspace: &Path, trusted: bool) {
    write(
        &home.join(".gemini/trustedFolders.json"),
        serde_json::to_vec(&BTreeMap::from([(
            workspace.to_string_lossy().into_owned(),
            if trusted {
                "TRUST_FOLDER"
            } else {
                "DO_NOT_TRUST"
            },
        )]))
        .unwrap(),
    );
}

#[test]
fn real_skill_policy_union_keeps_exact_names_and_recovers_as_controls_change() {
    let home = inventory_test_root("skill-disabled-union");
    let workspace = home.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let home = fs::canonicalize(home).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    trust(&home, &workspace, true);
    for (directory, name) in [
        ("upper", "Release"),
        ("lower", "release"),
        ("space", "Spacey"),
        ("manual", "manual"),
    ] {
        skill(
            &home.join(format!(".gemini/skills/{directory}/SKILL.md")),
            name,
        );
    }
    let cases = [
        (
            r#"{"footer":true,"skills":{"disabled":["RELEASE"," Spacey ","RELEASE","private-disabled-name-sentinel"]}}"#,
            r#"{"skills":{"disabled":["manual"]}}"#,
            [
                AgentAssetState::Disabled,
                AgentAssetState::Disabled,
                AgentAssetState::Enabled,
                AgentAssetState::Disabled,
            ],
        ),
        (
            r#"{"footer":true,"skills":{"disabled":[]}}"#,
            r#"{"skills":{"disabled":["manual"]}}"#,
            [
                AgentAssetState::Enabled,
                AgentAssetState::Enabled,
                AgentAssetState::Enabled,
                AgentAssetState::Disabled,
            ],
        ),
        (
            r#"{"footer":true,"skills":{"disabled":["release"]}}"#,
            r#"{"skills":{"disabled":[12]}}"#,
            [
                AgentAssetState::Disabled,
                AgentAssetState::Disabled,
                AgentAssetState::Unknown,
                AgentAssetState::Unknown,
            ],
        ),
        (
            r#"{"footer":true,"skills":false}"#,
            r#"{"skills":{"disabled":[]}}"#,
            [AgentAssetState::Unknown; 4],
        ),
        (r#"{"footer":true}"#, "{}", [AgentAssetState::Enabled; 4]),
    ];
    let mut identities = None;
    for (user, project, expected) in cases {
        write(&home.join(".gemini/settings.json"), user);
        write(&workspace.join(".gemini/settings.json"), project);
        let result = inventory(&home, Some(&workspace));
        assert_eq!(result.assets.len(), 5);
        let current = ["Release", "release", "Spacey", "manual"]
            .into_iter()
            .zip(expected)
            .map(|(name, expected)| {
                let asset = row(&result, name);
                assert_eq!(asset.native_id, name);
                assert_eq!(asset.effective_state, expected, "{user}; {project}; {name}");
                assert_eq!(
                    asset.declared_state,
                    if expected == AgentAssetState::Unknown {
                        AgentAssetState::Enabled
                    } else {
                        expected
                    }
                );
                assert!(Path::new(asset.path.as_deref().unwrap())
                    .file_name()
                    .is_some_and(|name| name == "SKILL.md"));
                assert!(!asset.is_directory);
                assert_eq!(
                    asset.resolution.relation,
                    AgentAssetResolutionRelation::Independent
                );
                asset.stable_id.clone()
            })
            .collect::<Vec<_>>();
        if let Some(previous) = &identities {
            assert_eq!(previous, &current);
        } else {
            identities = Some(current);
        }
        let footer = result
            .assets
            .iter()
            .find(|asset| asset.category == AgentAssetCategory::StatusUi)
            .unwrap();
        assert_eq!(footer.effective_state, AgentAssetState::Enabled);
    }
    fs::remove_file(home.join(".gemini/settings.json")).unwrap();
    fs::remove_file(workspace.join(".gemini/settings.json")).unwrap();
    let absent = inventory(&home, Some(&workspace));
    assert_eq!(absent.assets.len(), 4);
    assert!(absent
        .assets
        .iter()
        .all(|asset| asset.effective_state == AgentAssetState::Enabled));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn real_workspace_skill_definitions_and_policies_follow_workspace_trust() {
    let home = inventory_test_root("skill-trust");
    let workspace = home.join("workspace");
    skill(&home.join(".gemini/skills/user/SKILL.md"), "user");
    skill(
        &workspace.join(".gemini/skills/project/SKILL.md"),
        "project",
    );
    write(
        &workspace.join(".gemini/settings.json"),
        r#"{"skills":{"disabled":["user"]}}"#,
    );
    let home = fs::canonicalize(home).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    for trusted in [false, true, false] {
        trust(&home, &workspace, trusted);
        let result = inventory(&home, Some(&workspace));
        assert_eq!(result.assets.len(), if trusted { 2 } else { 1 });
        assert_eq!(
            row(&result, "user").effective_state,
            if trusted {
                AgentAssetState::Disabled
            } else {
                AgentAssetState::Enabled
            }
        );
        let project = result
            .declarations
            .iter()
            .find(|asset| asset.native_id == "project")
            .unwrap();
        assert_eq!(
            project.participation,
            if trusted {
                AgentAssetResolutionParticipation::Participates
            } else {
                AgentAssetResolutionParticipation::Suppressed {
                    reason: AgentAssetSuppressionReason::UntrustedWorkspace,
                }
            }
        );
        if trusted {
            assert_eq!(
                row(&result, "project").effective_state,
                AgentAssetState::Enabled
            );
        }
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn real_skill_source_precedence_replaces_with_exact_own_origins() {
    let home = inventory_test_root("skill-precedence");
    let workspace = home.join("workspace");
    write(
        &home.join(".gemini/extensions/alpha/gemini-extension.json"),
        r#"{"name":"alpha","version":"1.0.0"}"#,
    );
    let paths = [
        home.join(".gemini/extensions/alpha/skills/same/SKILL.md"),
        home.join(".gemini/skills/same/SKILL.md"),
        home.join(".agents/skills/same/SKILL.md"),
        workspace.join(".gemini/skills/same/SKILL.md"),
        workspace.join(".agents/skills/same/SKILL.md"),
    ];
    for path in &paths {
        skill(path, "same");
    }
    let paths = paths.map(|path| fs::canonicalize(path).unwrap());
    let home = fs::canonicalize(home).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    trust(&home, &workspace, true);
    for count in (1..=paths.len()).rev() {
        let result = inventory(&home, Some(&workspace));
        assert_eq!(result.assets.len(), count + 1);
        let active = row(&result, "same");
        assert_eq!(
            active.path.as_deref(),
            Some(paths[count - 1].to_str().unwrap())
        );
        assert_eq!(active.effective_state, AgentAssetState::Enabled);
        assert_eq!(active.precedence, [1, 10, 20, 30, 40][count - 1]);
        assert_eq!(
            active.resolution.relation,
            if count == 1 {
                AgentAssetResolutionRelation::Independent
            } else {
                AgentAssetResolutionRelation::ReplaceWinner
            }
        );
        assert_eq!(
            result
                .assets
                .iter()
                .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
                .count(),
            count - 1
        );
        for asset in result
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
        {
            assert_eq!(asset.declared_state, AgentAssetState::Enabled);
            let from_extension = Path::new(asset.path.as_deref().unwrap())
                .starts_with(home.join(".gemini/extensions/alpha"));
            assert_eq!(asset.relationships.provided_by.is_some(), from_extension);
            assert_eq!(
                asset.relationships.action_owner,
                asset.relationships.provided_by
            );
        }
        fs::remove_file(&paths[count - 1]).unwrap();
    }
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn skill_policy_keeps_complete_witnesses_and_parent_routes() {
    let settings = |key: &str, precedence, document: &str| {
        (
            d2_source(
                key,
                vec![AgentAssetCategory::Skill],
                precedence,
                &format!("/tmp/gemini/{key}.json"),
            ),
            AgentAssetSnapshot::File {
                bytes: document.as_bytes().to_vec(),
                revision: Default::default(),
            },
        )
    };
    let skill_source = |source_key: &str, name: &str| {
        (
            d2_source(
                source_key,
                vec![AgentAssetCategory::Skill],
                0,
                "/tmp/gemini/child/SKILL.md",
            ),
            AgentAssetSnapshot::File {
                bytes: format!("---\nname: {name}\ndescription: hidden\n---\n").into_bytes(),
                revision: Default::default(),
            },
        )
    };
    for (enablement, disabled, parent_state, expected) in [
        (
            r#"{"alpha":{"overrides":["!/tmp/project/*"]}}"#,
            "[]",
            AgentAssetState::Disabled,
            AgentAssetState::Disabled,
        ),
        (
            r#"{"alpha":{"overrides":["/tmp/project/*"]}}"#,
            "[]",
            AgentAssetState::Enabled,
            AgentAssetState::Unknown,
        ),
        (
            r#"{"alpha":{"overrides":false}}"#,
            "[]",
            AgentAssetState::Unknown,
            AgentAssetState::Unknown,
        ),
        (
            r#"{"alpha":{"overrides":false}}"#,
            r#"["Child"]"#,
            AgentAssetState::Unknown,
            AgentAssetState::Disabled,
        ),
    ] {
        let mut context = test_context();
        context.workspace_id = Some("/tmp/project".to_owned());
        context.trust_context = AgentTrustState::Trusted;
        let inputs = vec![
            (
                d2_source(
                    "gemini-extension-manifest:alpha",
                    vec![AgentAssetCategory::Extension],
                    0,
                    "/tmp/gemini/alpha.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: br#"{"name":"alpha","version":"1.0.0"}"#.to_vec(),
                    revision: Default::default(),
                },
            ),
            (
                d2_source(
                    "extension-enablement",
                    vec![AgentAssetCategory::Extension],
                    100,
                    "/tmp/gemini/enablement.json",
                ),
                AgentAssetSnapshot::File {
                    bytes: enablement.as_bytes().to_vec(),
                    revision: Default::default(),
                },
            ),
            skill_source("gemini-extension-skill:alpha:child", "Child"),
            settings(
                "settings",
                10,
                &format!(r#"{{"skills":{{"disabled":{disabled}}}}}"#),
            ),
            settings("system-settings", 30, r#"{"skills":{"disabled":42}}"#),
        ];
        let trace = run_d2_sources_with_options(
            context.clone(),
            inputs.clone(),
            false,
            false,
            Some(Path::new("/tmp/project")),
            Some(Path::new("/tmp/project")),
        );
        let reversed = run_d2_sources_with_options(
            context,
            inputs,
            true,
            true,
            Some(Path::new("/tmp/project")),
            Some(Path::new("/tmp/project")),
        );
        for trace in [trace, reversed] {
            assert_eq!(trace.records.len(), 2);
            assert_no_projection_diagnostics(&trace);
            let parent = record(&trace, AgentAssetCategory::Extension, "alpha");
            assert_eq!(parent.declared_state, parent_state, "{enablement}");
            assert_eq!(parent.effective_state, parent_state, "{enablement}");
            let parent_draft = trace
                .drafts
                .iter()
                .find(|draft| draft.native_kind == AgentAssetCategory::Extension)
                .unwrap();
            let enablement_declaration = trace
                .declarations
                .iter()
                .find(|declaration| declaration.source_key == "extension-enablement")
                .unwrap();
            if parent_state == AgentAssetState::Unknown {
                assert!(matches!(
                    &enablement_declaration.native_payload,
                    AgentAssetNativePayload::GeminiExtensionEnablementUnknown(
                        GeminiExtensionEnablementUnknownCause::InvalidEntry
                    )
                ));
                assert_eq!(
                    parent_draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::Unknown {
                        evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                            declaration_id: enablement_declaration.declaration_id.clone(),
                        }],
                        cause: AgentAssetDeclaredUnknownCauseDraft::InvalidTypedControl,
                    }
                );
            } else {
                assert_eq!(
                    parent_draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::Overlay {
                        declaration_ids: vec![enablement_declaration.declaration_id.clone()],
                        scope: AgentAssetStateOverlayScopeDraft::ResolutionGroup,
                        outcome: enablement_declaration.declared_state,
                    }
                );
            }
            let child = record(&trace, AgentAssetCategory::Skill, "Child");
            assert_eq!(child.effective_state, expected, "{enablement}; {disabled}");
            assert_eq!(
                child.relationships.provided_by.as_deref(),
                Some(parent.stable_id.as_str())
            );
            assert_eq!(
                child.relationships.action_owner,
                child.relationships.provided_by
            );
            let child_draft = trace
                .drafts
                .iter()
                .find(|draft| draft.native_kind == AgentAssetCategory::Skill)
                .unwrap();
            if expected == AgentAssetState::Disabled && parent_state != AgentAssetState::Disabled {
                let policy = trace
                    .declarations
                    .iter()
                    .find(|declaration| {
                        declaration.source_key == "settings"
                            && declaration.category == AgentAssetCategory::Skill
                            && declaration.role == AgentAssetDeclarationRole::PolicyOverlay
                    })
                    .unwrap();
                assert_eq!(child.declared_state, AgentAssetState::Disabled);
                assert_eq!(
                    child_draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::Policy {
                        declaration_ids: vec![policy.declaration_id.clone()],
                        outcome: AgentAssetDeclaredState::Disabled,
                    }
                );
                assert_eq!(
                    child_draft.state_proof.effective,
                    AgentAssetEffectiveStateProofDraft::Intrinsic
                );
                assert_eq!(child.resolution.terminal, None);
            } else {
                let definition = trace
                    .declarations
                    .iter()
                    .find(|declaration| {
                        declaration.category == AgentAssetCategory::Skill
                            && declaration.role == AgentAssetDeclarationRole::Definition
                    })
                    .unwrap();
                assert_eq!(child.declared_state, AgentAssetState::Enabled);
                assert_eq!(
                    child_draft.state_proof.declared,
                    AgentAssetDeclaredStateProofDraft::NativeDefault {
                        definition_id: definition.declaration_id.clone(),
                        outcome: AgentAssetDeclaredState::Enabled,
                    }
                );
                if parent_state == AgentAssetState::Disabled {
                    assert!(matches!(
                        &child_draft.state_proof.effective,
                        AgentAssetEffectiveStateProofDraft::ParentGate { parent, input }
                            if parent.category == AgentAssetCategory::Extension
                                && parent.native_id == "alpha"
                                && **input == AgentAssetEffectiveStateProofDraft::Intrinsic
                    ));
                    assert_eq!(child.resolution.terminal, None);
                    assert_eq!(child.resolution.control_source, None);
                } else {
                    let invalid = trace
                        .declarations
                        .iter()
                        .find(|declaration| declaration.source_key == "system-settings")
                        .unwrap();
                    assert_eq!(
                        child_draft.state_proof.effective,
                        AgentAssetEffectiveStateProofDraft::Terminal {
                            terminal: AgentAssetResolutionTerminal::Unknown,
                            cause: AgentAssetTerminalCauseDraft::InvalidControl,
                            evidence: vec![AgentAssetStateEvidenceRefDraft::Declaration {
                                declaration_id: invalid.declaration_id.clone(),
                            }],
                            input: Box::new(AgentAssetEffectiveStateProofDraft::Intrinsic),
                        }
                    );
                    assert_eq!(
                        child.resolution.control_source,
                        Some(AgentAssetPolicyReference::Declaration {
                            declaration_id: invalid.declaration_id.clone(),
                        })
                    );
                }
            }
        }
    }

    let trace = run_d2_sources(
        test_context(),
        vec![
            skill_source("gemini-skill-manifest:skills:child", "Child"),
            settings("system-defaults", 5, r#"{"skills":{"disabled":["Child"]}}"#),
            settings("settings", 10, r#"{"skills":{"disabled":["CHILD"]}}"#),
            settings("system-settings", 30, r#"{"skills":{"disabled":[]}}"#),
        ],
    );
    assert_eq!(trace.records.len(), 1);
    assert_no_projection_diagnostics(&trace);
    let draft = &trace.drafts[0];
    let AgentAssetDeclaredStateProofDraft::Policy {
        declaration_ids,
        outcome,
    } = &draft.state_proof.declared
    else {
        panic!("complete union policy")
    };
    assert_eq!(declaration_ids.len(), 2);
    assert_eq!(*outcome, AgentAssetDeclaredState::Disabled);
    assert!(declaration_ids
        .iter()
        .all(|id| !draft.contributor_ids.contains(id)));
}
