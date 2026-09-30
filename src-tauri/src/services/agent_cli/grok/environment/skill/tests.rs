use super::*;
use crate::models::*;
use crate::services::agent_cli::contracts::{
    AgentAssetDirectoryEntry, AgentDiagnosticEmission, AgentDiagnosticOutput, AgentOutputStop,
};
use crate::services::agent_cli::environment::{source, SourceInput};
use std::{
    collections::BTreeSet,
    fs,
    ops::ControlFlow,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "balancehub-grok-skill-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn write(path: &Path, content: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn skill(path: &Path, name: &str) {
    write(path, format!("---\nname: '{name}'\ndescription: private-grok-description-sentinel\n---\nprivate-grok-body-sentinel\n"));
}

fn inventory(home: &Path, workspace: Option<&Path>) -> AgentEnvironmentInventory {
    let definition = crate::services::agent_cli::definition(AgentCliKind::Grok);
    let inventory = crate::services::agent_cli::environment::build_inventory_for_test(
        home,
        workspace,
        std::slice::from_ref(definition),
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
    assert!(inventory
        .contexts
        .iter()
        .all(|context| context.parser_version == 3));
    let debug = format!("{inventory:?}");
    assert!(!debug.contains("private-grok-description-sentinel"));
    assert!(!debug.contains("private-grok-body-sentinel"));
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

#[test]
fn real_grok_skill_manifests_prove_defaults_and_exact_user_disable_lists() {
    let home = root("state");
    skill(
        &home.join(".grok/skills/mixed/SKILL.md"),
        "Mixed_Name With Space",
    );
    write(
        &home.join(".grok/skills/plain-text/SKILL.md"),
        "private-grok-body-sentinel\n\nAnother paragraph.",
    );
    write(
        &home.join(".grok/skills/empty-meta/SKILL.md"),
        "---\nversion: 1\n---\nprivate-grok-body-sentinel",
    );
    write(&home.join(".grok/skills/manual/SKILL.md"), "---\nname: Manual\ndescription: private-grok-description-sentinel\ndisable-model-invocation: true\n---\nprivate-grok-body-sentinel");
    fs::create_dir_all(home.join(".grok/skills/directory-only")).unwrap();
    let home = fs::canonicalize(home).unwrap();
    let cases = [
        ("", [AgentAssetState::Enabled; 4]),
        (
            "[skills]\ndisabled=['mixed-name-with-space','plain-text','absent','plain-text']\n",
            [
                AgentAssetState::Disabled,
                AgentAssetState::Disabled,
                AgentAssetState::Enabled,
                AgentAssetState::Enabled,
            ],
        ),
        (
            "[skills]\ndisabled=['MIXED-NAME-WITH-SPACE',' plain-text ']\n",
            [AgentAssetState::Enabled; 4],
        ),
        (
            "[skills]\ndisabled=[12,'plain-text']\n",
            [AgentAssetState::Unknown; 4],
        ),
        ("skills=false\n", [AgentAssetState::Unknown; 4]),
        ("[skills]\ndisabled=[]\n", [AgentAssetState::Enabled; 4]),
    ];
    let mut identities = None;
    for (controls, expected) in cases {
        write(&home.join(".grok/config.toml"), format!("status_line=true\n{controls}[mcp_servers.healthy]\ncommand='balancehub-fixture-never-run'\n"));
        let result = inventory(&home, None);
        assert_eq!(result.assets.len(), 6);
        assert_eq!(
            result
                .assets
                .iter()
                .filter(|asset| asset.category == AgentAssetCategory::Skill)
                .count(),
            4
        );
        let current = [
            "mixed-name-with-space",
            "plain-text",
            "empty-meta",
            "manual",
        ]
        .into_iter()
        .zip(expected)
        .map(|(name, expected)| {
            let asset = row(&result, name);
            assert_eq!(asset.effective_state, expected, "{controls}; {name}");
            assert_eq!(
                asset.declared_state,
                if expected == AgentAssetState::Unknown {
                    AgentAssetState::Enabled
                } else {
                    expected
                }
            );
            assert_eq!(
                asset.resolution.relation,
                AgentAssetResolutionRelation::Independent
            );
            assert_eq!(asset.scope, AgentAssetScope::User);
            assert!(Path::new(asset.path.as_deref().unwrap())
                .file_name()
                .is_some_and(|name| name == "SKILL.md"));
            assert!(!asset.is_directory);
            let definitions = result
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.native_kind == AgentAssetCategory::Skill
                        && declaration.native_id == name
                        && declaration.role == AgentAssetDeclarationRole::Definition
                })
                .collect::<Vec<_>>();
            assert_eq!(definitions.len(), 1);
            let definition = definitions[0];
            assert_eq!(definition.declaration_key, "SKILL.md");
            assert_eq!(asset.inspection_source_id, definition.source_id);
            let overlays = result
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.native_kind == AgentAssetCategory::Skill
                        && declaration.native_id == name
                        && declaration.role == AgentAssetDeclarationRole::StateOverlay
                })
                .collect::<Vec<_>>();
            assert_eq!(
                overlays.len(),
                usize::from(expected == AgentAssetState::Disabled)
            );
            for overlay in &overlays {
                assert_eq!(overlay.declaration_key, format!("skills.disabled:{name}"));
                assert_eq!(overlay.declared_state, AgentAssetDeclaredState::Disabled);
                let source = result
                    .sources
                    .iter()
                    .find(|source| source.id == overlay.source_id)
                    .unwrap();
                assert_eq!(Path::new(&source.path), home.join(".grok/config.toml"));
                assert_eq!(source.scope, AgentAssetScope::User);
            }
            let represented_ids = std::iter::once(definition.id.as_str())
                .chain(overlays.iter().map(|overlay| overlay.id.as_str()))
                .collect::<BTreeSet<_>>();
            assert_eq!(
                asset
                    .represented_declaration_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                represented_ids
            );
            assert_eq!(
                asset
                    .resolution
                    .contributor_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                represented_ids
            );
            assert_eq!(
                asset
                    .source_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                std::iter::once(definition.source_id.as_str())
                    .chain(overlays.iter().map(|overlay| overlay.source_id.as_str()))
                    .collect::<BTreeSet<_>>()
            );
            (
                asset.stable_id.clone(),
                definition.id.clone(),
                definition.source_id.clone(),
            )
        })
        .collect::<Vec<_>>();
        if let Some(previous) = &identities {
            assert_eq!(
                previous, &current,
                "asset and manifest identities changed: {controls}"
            );
        } else {
            identities = Some(current);
        }
        assert!(matches!(
            row(&result, "manual").details,
            AgentAssetDetails::Skill {
                invocation_policy: AgentSkillInvocationPolicy::ManualOnly,
                ..
            }
        ));
        assert!(result
            .assets
            .iter()
            .filter(|asset| asset.category != AgentAssetCategory::Skill)
            .all(|asset| asset.effective_state == AgentAssetState::Enabled));
        assert!(!result
            .declarations
            .iter()
            .any(|asset| asset.native_id == "directory-only"));
    }
    fs::remove_file(home.join(".grok/config.toml")).unwrap();
    let absent = inventory(&home, None);
    assert_eq!(absent.assets.len(), 4);
    assert!(absent
        .assets
        .iter()
        .all(|asset| asset.effective_state == AgentAssetState::Enabled));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn real_grok_skill_priorities_and_user_control_preserve_replaced_origins() {
    let home = root("precedence");
    let workspace = home.join("workspace");
    let paths = [
        home.join(".agents/skills/same/SKILL.md"),
        home.join(".grok/skills/same/SKILL.md"),
        workspace.join(".agents/skills/same/SKILL.md"),
        workspace.join(".grok/skills/same/SKILL.md"),
    ];
    for path in &paths {
        skill(path, "same");
    }
    let paths = paths.map(|path| fs::canonicalize(path).unwrap());
    let home = fs::canonicalize(home).unwrap();
    let workspace = fs::canonicalize(workspace).unwrap();
    write(
        &home.join(".grok/config.toml"),
        "[skills]\ndisabled=['same']\n",
    );
    write(
        &workspace.join(".grok/config.toml"),
        "[skills]\ndisabled=[]\n",
    );
    for count in (1..=paths.len()).rev() {
        let result = inventory(&home, Some(&workspace));
        assert_eq!(result.assets.len(), count);
        let active = row(&result, "same");
        assert_eq!(active.effective_state, AgentAssetState::Disabled);
        assert_eq!(
            active.path.as_deref(),
            Some(paths[count - 1].to_str().unwrap())
        );
        assert_eq!(active.precedence, [10, 20, 30, 40][count - 1]);
        assert_eq!(
            active.resolution.relation,
            if count == 1 {
                AgentAssetResolutionRelation::Independent
            } else {
                AgentAssetResolutionRelation::ReplaceWinner
            }
        );
        let losers = result
            .assets
            .iter()
            .filter(|asset| asset.resolution.relation == AgentAssetResolutionRelation::Replaced)
            .collect::<Vec<_>>();
        assert_eq!(losers.len(), count - 1);
        for loser in losers {
            assert_eq!(loser.declared_state, AgentAssetState::Enabled);
            assert_ne!(loser.inspection_source_id, active.inspection_source_id);
        }
        fs::remove_file(&paths[count - 1]).unwrap();
    }
    skill(&home.join(".grok/skills/user/SKILL.md"), "user");
    fs::remove_file(home.join(".grok/config.toml")).unwrap();
    write(
        &workspace.join(".grok/config.toml"),
        "[skills]\ndisabled=['user']\n",
    );
    let workspace_only = inventory(&home, Some(&workspace));
    assert_eq!(workspace_only.assets.len(), 1);
    assert_eq!(
        row(&workspace_only, "user").effective_state,
        AgentAssetState::Enabled
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn real_grok_malformed_skill_manifests_are_local_and_native_normalization_is_bounded() {
    let home = root("names");
    for (directory, name) in [
        ("edge-one", "  Two__  Spaces  "),
        ("edge-two", "mixed.name"),
        ("edge-three", ""),
    ] {
        skill(
            &home.join(format!(".grok/skills/{directory}/SKILL.md")),
            name,
        );
    }
    write(
        &home.join(".grok/skills/broken/SKILL.md"),
        "---\nname: [broken\n---\nprivate-grok-body-sentinel",
    );
    write(
        &home.join(".grok/skills/non-string/SKILL.md"),
        "---\nname: 12\n---\nprivate-grok-body-sentinel",
    );
    let home = fs::canonicalize(home).unwrap();
    let result = inventory(&home, None);
    assert_eq!(result.assets.len(), 3);
    assert_eq!(
        result
            .assets
            .iter()
            .map(|asset| asset.native_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["two-spaces", "mixed-name", "edge-three"])
    );
    assert!(result
        .assets
        .iter()
        .all(|asset| asset.effective_state == AgentAssetState::Enabled));
    assert_eq!(
        result
            .sources
            .iter()
            .filter(
                |source| source.diagnostics.iter().any(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::Malformed {
                        format: AgentAssetDocumentFormat::Manifest,
                        ..
                    }
                ))
            )
            .count(),
        2
    );
    fs::remove_dir_all(home).unwrap();
}

#[derive(Default)]
struct FirstFollowUp {
    count: usize,
}
impl AgentDiagnosticOutput for FirstFollowUp {
    fn has_regular_capacity(&self) -> bool {
        true
    }
    fn emit_diagnostic(&mut self, _: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        AgentDiagnosticEmission::Accepted
    }
}
impl FollowUpSourceOutput for FirstFollowUp {
    fn emit_follow_up(&mut self, _: AgentFollowUpSourceSpec) -> ControlFlow<AgentOutputStop> {
        self.count += 1;
        ControlFlow::Break(AgentOutputStop::SourceLimit)
    }
}

#[test]
fn skill_follow_up_stops_on_first_break_without_recursive_targets() {
    let home = Path::new("/tmp/balancehub-grok-skill-follow-up");
    let parent = source(SourceInput {
        origin: crate::models::AgentAssetInstallationOrigin::Unknown,
        native_source_key: "skills",
        label: "skills",
        path: home.join("skills"),
        allowed_root: home,
        scope: AgentAssetScope::User,
        precedence: 20,
        sensitive: false,
        source_kind: AgentAssetSourceKind::Directory,
        categories: &[AgentAssetCategory::Skill],
    });
    let manifest = ["one", "two"].map(|name| AgentAssetDirectoryEntry {
        name: name.to_owned(),
        source_kind: AgentAssetSourceKind::Directory,
        is_symlink: false,
    });
    let mut output = FirstFollowUp::default();
    discover_follow_up_sources(
        AgentFollowUpSourceDiscoveryRequest {
            parent: &parent,
            manifest: &manifest,
        },
        &mut output,
    );
    assert_eq!(output.count, 1);
}
