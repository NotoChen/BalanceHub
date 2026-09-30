use super::*;

const DEFAULT_INSTALLATION: &str = "installation:profile-default";
const REVIEW_INSTALLATION: &str = "installation:profile-review";

fn profile_contexts(
    request: AgentContextDiscoveryRequest<'_>,
    output: &mut dyn AgentDiagnosticOutput,
) -> Vec<crate::models::AgentConfigurationContext> {
    let mut default = fixture_contexts(request, output).remove(0);
    default.compatible_installation_ids = vec![DEFAULT_INSTALLATION.to_owned()];
    let mut review = default.clone();
    review.profile = "review".to_owned();
    review.compatible_installation_ids = vec![REVIEW_INSTALLATION.to_owned()];
    let mut duplicate = default.clone();
    duplicate
        .schema_facts
        .insert("duplicate-profile-context".to_owned(), "merged".to_owned());
    vec![default, review, duplicate]
}

fn profile_definition() -> AgentCliDefinition {
    AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            profile_contexts,
            fixture_file_source,
            None,
            "fixture-agent",
            fixture_parse,
            fixture_resolve,
            fixture_assessor,
        ),
        ..fixture_definition(fixture_file_source, None)
    }
}

fn profile_installation(home: &Path, profile: &str, id: &str) -> AgentInstallation {
    let mut installation = fixture_installation();
    let path = home.join(format!("fixture-cli-{profile}"));
    installation.id = id.to_owned();
    installation.executable_path = Some(path.to_string_lossy().into_owned());
    let identity = installation.executable_identity.as_mut().unwrap();
    identity.owner = profile.to_owned();
    identity.canonical_path = path.to_string_lossy().into_owned();
    installation
}

#[test]
fn same_root_profiles_keep_distinct_inventory_ids_and_merge_only_matching_profiles() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().canonicalize().unwrap();
    let config_root = home.join("fixture");
    fs::create_dir(&config_root).unwrap();
    let config = config_root.join("config.json");
    fs::write(&config, b"{}").unwrap();
    let (installations, _) = FakeInstallationPort::new(vec![
        profile_installation(&home, "default", DEFAULT_INSTALLATION),
        profile_installation(&home, "review", REVIEW_INSTALLATION),
    ]);
    let snapshots = RealSnapshotPort::default();
    let settings = crate::models::AppSettings::default();
    let inventory = build_inventory_with(
        InventoryInput {
            home: &home,
            workspace: None,
            settings: Some(&settings),
        },
        InventoryPipelineDeps {
            definitions: &[profile_definition()],
            limits: AgentAssetLimits::DEFAULT,
            clock: Arc::new(ManualClock::new()),
            installations: &installations,
            snapshots: &snapshots,
            checkpoint_probe: None,
        },
    )
    .unwrap();

    // Three offered contexts share every identity input except profile. The
    // duplicate default context must merge without merging the review profile.
    assert_eq!(inventory.installations.len(), 2);
    assert_eq!(inventory.contexts.len(), 2);
    assert_eq!(inventory.sources.len(), 2);
    assert_eq!(inventory.declarations.len(), 4);
    assert_eq!(inventory.assets.len(), 4);
    assert!(
        inventory.diagnostics.is_empty(),
        "{:?}",
        inventory.diagnostics
    );
    let contexts = inventory
        .contexts
        .iter()
        .map(|context| (context.profile.as_str(), context))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        contexts.keys().copied().collect::<Vec<_>>(),
        vec!["default", "review"]
    );
    assert_ne!(contexts["default"].id, contexts["review"].id);
    assert_eq!(
        contexts["default"]
            .schema_facts
            .get("duplicate-profile-context")
            .map(String::as_str),
        Some("merged"),
    );
    assert!(!contexts["review"]
        .schema_facts
        .contains_key("duplicate-profile-context"));

    let native_ids = BTreeSet::from(["one", "two"]);
    for (profile, installation) in [
        ("default", DEFAULT_INSTALLATION),
        ("review", REVIEW_INSTALLATION),
    ] {
        let context = contexts[profile];
        let compatible = vec![installation.to_owned()];
        assert!(context.id.starts_with("context:"));
        assert_eq!(context.environment_id, inventory.environment.id);
        assert_eq!(context.agent_kind, AgentCliKind::Codex);
        assert_eq!(Path::new(&context.config_root), config_root);
        assert_eq!(context.workspace_id, None);
        assert_eq!(context.trust_context, AgentTrustState::Unknown);
        assert_eq!(context.compatible_installation_ids, compatible);

        let sources = inventory
            .sources
            .iter()
            .filter(|source| source.context_id == context.id)
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 1);
        let source = sources[0];
        assert!(source.id.starts_with("source:"));
        assert_eq!(Path::new(&source.path), config);
        assert!(!source.revision.is_missing);
        assert_eq!(source.revision.size_bytes, Some(2));
        assert!(source.diagnostics.is_empty());

        let declarations = inventory
            .declarations
            .iter()
            .filter(|declaration| declaration.context_id == context.id)
            .collect::<Vec<_>>();
        assert_eq!(declarations.len(), 2);
        assert_eq!(
            declarations
                .iter()
                .map(|declaration| declaration.native_id.as_str())
                .collect::<BTreeSet<_>>(),
            native_ids
        );
        for declaration in declarations {
            assert!(declaration.id.starts_with("declaration:"));
            assert_eq!(declaration.source_id, source.id);
            assert_eq!(declaration.role, AgentAssetDeclarationRole::Definition);
            assert!(declaration.diagnostics.is_empty());
        }

        let assets = inventory
            .assets
            .iter()
            .filter(|asset| asset.context_id == context.id)
            .collect::<Vec<_>>();
        assert_eq!(assets.len(), 2);
        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.native_id.as_str())
                .collect::<BTreeSet<_>>(),
            native_ids
        );
        for asset in assets {
            assert!(asset.stable_id.starts_with("asset:"));
            assert_eq!(asset.inspection_source_id, source.id);
            assert_eq!(asset.source_ids, vec![source.id.clone()]);
            assert_eq!(asset.compatible_installation_ids, compatible);
            assert_eq!(asset.declared_state, AgentAssetState::Enabled);
            assert_eq!(asset.effective_state, AgentAssetState::Enabled);
            assert!(asset.diagnostics.is_empty());
            assert!(asset.resolution.diagnostics.is_empty());
        }
    }
    assert_eq!(
        inventory
            .sources
            .iter()
            .map(|source| &source.id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        inventory
            .declarations
            .iter()
            .map(|declaration| &declaration.id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        inventory
            .assets
            .iter()
            .map(|asset| &asset.stable_id)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(fs::read(&config).unwrap(), b"{}");
}
