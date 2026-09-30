//! Claude Skill metadata through native discovery, parsing, resolution and
//! independent projection. Inputs and executable metadata are synthetic.
use super::super::{snapshot::revision_for_missing, verified_path::VerifiedPathAnchor};
use super::*;
use crate::models::{AgentAssetIoErrorKind, AgentAssetRecord, AgentEnvironmentInventory};

const TRANSITIVE_MERGE: &[u8] = b"---\nbase: &base\n  disable-model-invocation: true\ndefaults: &defaults\n  <<: *base\n<<: *defaults\nname: review\n---";
const INVALID_NESTED_MERGE: &[u8] = b"---\n<<:\n  <<: invalid-merge-value\n---";

fn skill_row<'a>(inventory: &'a AgentEnvironmentInventory, name: &str) -> &'a AgentAssetRecord {
    let rows = claude_assets(inventory, AgentAssetCategory::Skill, name);
    assert_eq!(rows.len(), 1, "missing or duplicate {name}: {inventory:?}");
    rows[0]
}

#[test]
fn claude_skill_frontmatter_inventory_distinguishes_valid_invalid_limited_and_unreadable() {
    let files = [
        ("skill-manifest:skills:punctuation", b"---\nname: punctuation\ndescription: Review a change, then discuss the result.\n---\nfixture-private-body".to_vec()),
        ("skill-manifest:skills:multiline", b"---\nname: multiline\ndescription: |\n  fixture-private-description\n  ---\n  With another line.\nmetadata:\n  nested: [fixture-private-metadata, true]\n---".to_vec()),
        ("skill-manifest:skills:manual", b"---\nname: manual\ndisable-model-invocation: true\n---".to_vec()),
        ("skill-manifest:skills:transitive", TRANSITIVE_MERGE.to_vec()),
        ("skill-manifest:skills:invalid-merge", INVALID_NESTED_MERGE.to_vec()),
        ("skill-manifest:skills:damaged", b"---\nname: [unfinished\n---".to_vec()),
        ("skill-manifest:skills:wrong-policy", b"---\ndisable-model-invocation: 'false'\n---".to_vec()),
        ("skill-manifest:skills:limited", format!("---\n{}---", "# bounded metadata\n".repeat(64)).into_bytes()),
    ];
    let names = [
        "punctuation",
        "multiline",
        "manual",
        "transitive",
        "invalid-merge",
        "damaged",
        "wrong-policy",
        "limited",
        "unreadable",
        "missing",
    ];
    let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.270",
        AgentInstallationChannel::Stable,
    )]);
    let (root, _, inventory, _) = build_claude_inventory(
        "claude-skill-yaml",
        files,
        [(
            "skills",
            claude_entries(names.map(|name| (name, AgentAssetSourceKind::Directory, false))),
        )],
        [(
            "skill-manifest:skills:unreadable",
            AgentAssetDiagnostic::ReadFailed {
                source_id: "fixture".to_owned(),
                error_kind: AgentAssetIoErrorKind::PermissionDenied,
            },
        )],
        &installations,
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
            .count(),
        8
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.native_kind == AgentAssetCategory::Skill)
            .count(),
        8
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "punctuation",
            "multiline",
            "manual",
            "transitive",
            "invalid-merge",
            "damaged",
            "wrong-policy",
            "limited"
        ]),
    );
    for (name, policy) in [
        ("punctuation", AgentSkillInvocationPolicy::ModelInvocable),
        ("multiline", AgentSkillInvocationPolicy::ModelInvocable),
        ("manual", AgentSkillInvocationPolicy::ManualOnly),
        ("transitive", AgentSkillInvocationPolicy::ManualOnly),
    ] {
        let row = skill_row(&inventory, name);
        assert_eq!(row.declared_state, AgentAssetState::Enabled);
        assert_eq!(row.effective_state, AgentAssetState::Enabled);
        assert_eq!(
            row.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(matches!(row.details, AgentAssetDetails::Skill {
            enabled: AgentAssetDeclaredState::Enabled, invocation_policy,
        } if invocation_policy == policy));
        let source = inventory
            .sources
            .iter()
            .find(|source| source.id == row.inspection_source_id)
            .unwrap();
        assert!(
            source.diagnostics.is_empty(),
            "{name}: {:?}",
            source.diagnostics
        );
    }
    for (name, diagnostic) in [
        (
            "invalid-merge",
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.syntax".to_owned()),
            },
        ),
        (
            "damaged",
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.syntax".to_owned()),
            },
        ),
        (
            "wrong-policy",
            AgentAssetDiagnostic::Malformed {
                format: AgentAssetDocumentFormat::Yaml,
                location: Some("frontmatter.disable-model-invocation".to_owned()),
            },
        ),
        (
            "limited",
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FrontmatterLines,
                accepted: 64,
                observed_at_least: 65,
            },
        ),
    ] {
        let row = skill_row(&inventory, name);
        assert_eq!(row.declared_state, AgentAssetState::Unknown);
        assert_eq!(row.effective_state, AgentAssetState::Unknown);
        assert!(matches!(
            row.details,
            AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Unknown,
                invocation_policy: AgentSkillInvocationPolicy::Unknown,
            }
        ));
        let source = inventory
            .sources
            .iter()
            .find(|source| source.id == row.inspection_source_id)
            .unwrap();
        assert_eq!(source.diagnostics, [diagnostic]);
    }
    for name in ["unreadable", "missing"] {
        assert!(claude_assets(&inventory, AgentAssetCategory::Skill, name).is_empty());
        assert!(claude_declarations(&inventory, AgentAssetCategory::Skill, name).is_empty());
    }
    let unreadable = inventory
        .sources
        .iter()
        .find(|source| {
            Path::new(&source.path).ends_with(Path::new("skills").join("unreadable/SKILL.md"))
        })
        .unwrap();
    assert!(matches!(
        unreadable.diagnostics.as_slice(),
        [AgentAssetDiagnostic::ReadFailed {
            error_kind: AgentAssetIoErrorKind::PermissionDenied,
            ..
        }]
    ));
    assert_no_structural_projection_diagnostics(&inventory);
    let public = serde_json::to_string(&inventory).unwrap();
    for sentinel in [
        "fixture-private-body",
        "fixture-private-description",
        "fixture-private-metadata",
    ] {
        assert!(!public.contains(sentinel));
    }
    remove_claude_fixture(root);
}

struct SkillSnapshots<'a> {
    root: &'a Path,
    real: RealSnapshotPort,
}

impl SnapshotPort for SkillSnapshots<'_> {
    fn snapshot(
        &self,
        request: SnapshotRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> AgentAssetSnapshot {
        if !request.source.path.starts_with(self.root) {
            assert!(matches!(
                request.source.native_source_key.as_str(),
                "managed-settings" | "managed-mcp"
            ));
            return AgentAssetSnapshot::Missing {
                revision: revision_for_missing(&request.source.path),
            };
        }
        self.real.snapshot(request, run)
    }

    fn access_anchor(&self, revision: &AgentAssetRevision) -> Option<VerifiedPathAnchor> {
        self.real.access_anchor(revision)
    }
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn scan_package(root: &Path, home: &Path) -> AgentEnvironmentInventory {
    let _config_root = ClaudeConfigRootOverrideGuard::new(home.join(".claude"));
    let native = definition(AgentCliKind::ClaudeCode);
    let definitions = [AgentCliDefinition {
        environment: native
            .environment
            .with_test_contexts(claude_test_discover_contexts),
        ..*native
    }];
    let (installations, _) = FakeInstallationPort::new(vec![claude_fixture_installation(
        "2.1.270",
        AgentInstallationChannel::Stable,
    )]);
    build_inventory_with(
        InventoryInput {
            home,
            workspace: None,
            settings: None,
        },
        InventoryPipelineDeps {
            definitions: &definitions,
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &SkillSnapshots {
                root,
                real: RealSnapshotPort::default(),
            },
            checkpoint_probe: None,
        },
    )
    .unwrap()
}

#[test]
fn claude_plugin_skill_yaml_keeps_unknown_diagnostics_and_parent_gate() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("home");
    let package = home.join("packages/fixture");
    write(
        &package.join(".claude-plugin/plugin.json"),
        br#"{"name":"fixture"}"#,
    );
    write(&home.join(".claude/plugins/installed_plugins.json"), serde_json::to_vec(&serde_json::json!({
        "version": 2,
        "plugins": {"fixture@local": [{"scope": "user", "installPath": package, "version": "1.0.0"}]},
    })).unwrap());
    let original_skills = [
        (package.join("skills/normal/SKILL.md"), b"---\nname: normal\ndescription: Review a change, then discuss.\nmetadata:\n  private: [fixture-hidden-metadata]\n---\nfixture-hidden-body".to_vec()),
        (package.join("skills/manual/SKILL.md"), b"---\nname: manual\ndisable-model-invocation: true\n---".to_vec()),
        (package.join("skills/transitive/SKILL.md"), TRANSITIVE_MERGE.to_vec()),
        (package.join("skills/override/SKILL.md"), std::str::from_utf8(TRANSITIVE_MERGE).unwrap().replace("name: review", "name: review\ndisable-model-invocation: false").into_bytes()),
        (package.join("skills/invalid-merge/SKILL.md"), INVALID_NESTED_MERGE.to_vec()),
        (package.join("skills/broken/SKILL.md"), vec![0xff, 0xfe]),
    ];
    for (path, bytes) in &original_skills {
        write(path, bytes);
    }
    for enabled in [true, false] {
        write(
            &home.join(".claude/settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "enabledPlugins": {"fixture@local": enabled},
            }))
            .unwrap(),
        );
        let inventory = scan_package(&root, &home);
        let names = inventory
            .assets
            .iter()
            .filter(|asset| asset.category == AgentAssetCategory::Skill)
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "fixture:normal",
                "fixture:manual",
                "fixture:transitive",
                "fixture:override",
                "fixture:invalid-merge",
                "fixture:broken"
            ])
        );
        let plugins = claude_assets(&inventory, AgentAssetCategory::Plugin, "fixture@local");
        assert_eq!(plugins.len(), 1);
        assert_eq!(
            plugins[0].effective_state,
            if enabled {
                AgentAssetState::Enabled
            } else {
                AgentAssetState::Disabled
            }
        );
        for (name, policy) in [
            ("fixture:normal", AgentSkillInvocationPolicy::ModelInvocable),
            ("fixture:manual", AgentSkillInvocationPolicy::ManualOnly),
            ("fixture:transitive", AgentSkillInvocationPolicy::ManualOnly),
            (
                "fixture:override",
                AgentSkillInvocationPolicy::ModelInvocable,
            ),
        ] {
            let row = skill_row(&inventory, name);
            assert_eq!(row.declared_state, AgentAssetState::Enabled);
            assert_eq!(
                row.effective_state,
                if enabled {
                    AgentAssetState::Enabled
                } else {
                    AgentAssetState::Disabled
                }
            );
            assert!(matches!(row.details, AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Enabled, invocation_policy,
            } if invocation_policy == policy));
            assert_eq!(
                row.relationships.provided_by.as_deref(),
                Some(plugins[0].stable_id.as_str())
            );
        }
        for (name, location) in [
            ("fixture:broken", "frontmatter.encoding"),
            ("fixture:invalid-merge", "frontmatter.syntax"),
        ] {
            let invalid = skill_row(&inventory, name);
            assert_eq!(invalid.declared_state, AgentAssetState::Unknown);
            assert_eq!(invalid.effective_state, AgentAssetState::Unknown);
            assert_eq!(
                invalid.relationships.provided_by.as_deref(),
                Some(plugins[0].stable_id.as_str())
            );
            let source = inventory
                .sources
                .iter()
                .find(|source| source.id == invalid.inspection_source_id)
                .unwrap();
            assert_eq!(
                source.diagnostics,
                [AgentAssetDiagnostic::Malformed {
                    format: AgentAssetDocumentFormat::Yaml,
                    location: Some(location.to_owned()),
                }]
            );
        }
        assert_no_structural_projection_diagnostics(&inventory);
        let public = serde_json::to_string(&inventory).unwrap();
        assert!(!public.contains("fixture-hidden-metadata"));
        assert!(!public.contains("fixture-hidden-body"));
        for (path, original) in &original_skills {
            assert_eq!(fs::read(path).unwrap(), *original);
        }
    }
}
