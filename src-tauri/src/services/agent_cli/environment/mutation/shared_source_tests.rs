use super::super::{
    inventory::{native_environment, InstallationDiscoveryPort, InstallationDiscoveryRequest},
    run::AgentInventoryRun,
};
use super::{
    native_test_support::NativeFixtureInspector, prepared::prepare_request, MutationExecution,
    MutationInspector, MutationService,
};
use crate::{
    models::*,
    services::agent_cli::{contracts::*, definition, AgentCliDefinition},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

const MCP_COUNT: usize = 10;
const TARGETS: [&str; 2] = ["server-0", "server-1"];

struct FixtureInstallation(AgentInstallation);

impl InstallationDiscoveryPort for FixtureInstallation {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        _: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        assert_eq!(request.definition.kind, AgentCliKind::Codex);
        vec![self.0.clone()]
    }
}

struct IsolatedSystemSources<'a> {
    output: &'a mut dyn InitialSourceOutput,
    root: PathBuf,
}

impl IsolatedSystemSources<'_> {
    fn isolate(&self, mut source: AgentAssetSourceSpec) -> AgentAssetSourceSpec {
        if matches!(
            source.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && !source.path.starts_with(&self.root)
        {
            source.path = self
                .root
                .join(format!("fixture-{}", source.native_source_key));
            source.allowed_root = self.root.clone();
        }
        source
    }
}

impl AgentDiagnosticOutput for IsolatedSystemSources<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(value)
    }
}

impl InitialSourceOutput for IsolatedSystemSources<'_> {
    fn emit_initial(&mut self, source: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        let source = self.isolate(source);
        self.output.emit_initial(source)
    }

    fn snapshot_initial(
        &mut self,
        source: AgentAssetSourceSpec,
    ) -> ControlFlow<AgentOutputStop, Option<AgentAssetSnapshot>> {
        let source = self.isolate(source);
        self.output.snapshot_initial(source)
    }
}

fn isolated_codex() -> AgentCliDefinition {
    let native = definition(AgentCliKind::Codex);
    AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            |request, output| {
                // Retain native context metadata, but never inherit CODEX_HOME
                // or BALANCEHUB_CODEX_HOME from the process running the test.
                let root = request.home.join(".codex");
                let mut contexts = definition(AgentCliKind::Codex)
                    .environment()
                    .discover_contexts(request, output);
                for context in &mut contexts {
                    context.config_root = root.to_string_lossy().into_owned();
                }
                contexts
            },
            |request, output| {
                let root = PathBuf::from(&request.context.config_root);
                definition(AgentCliKind::Codex)
                    .environment()
                    .discover_sources(request, &mut IsolatedSystemSources { output, root });
            },
            Some(|request, output| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .discover_follow_up_sources(request, output)
            }),
            "@openai/codex",
            |request, output| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .parse(request, output)
            },
            |request, output| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .resolve(request, output)
            },
            native.environment().state_assessor(),
        )
        .with_asset_mutation(
            |kind| definition(kind).environment().mechanism_records(kind),
            |request| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .prepare_mutation(request)
            },
            |category, action| {
                definition(AgentCliKind::Codex)
                    .environment()
                    .native_unavailable_reason(category, action)
            },
        ),
        ..*native
    }
}

fn inspector(home: &Path) -> NativeFixtureInspector {
    let home = home.canonicalize().unwrap();
    let executable = home.join("fixture-codex-not-executable");
    fs::write(
        &executable,
        "Atomic MCP fixture; never execute this file.\n",
    )
    .unwrap();
    let installation = AgentInstallation {
        id: "fixture-codex-shared-source".into(),
        environment_id: native_environment().id,
        agent_kind: AgentCliKind::Codex,
        label: "Codex shared-source fixture".into(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some(executable.to_string_lossy().into_owned()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: "fixture".into(),
            canonical_path: executable.to_string_lossy().into_owned(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-codex-executable".into()),
        installed_version: Some("0.154.0".into()),
        discovery_source: AgentDiscoverySource::Configured,
        distribution: AgentCliDistribution::Npm,
        channel: AgentInstallationChannel::Stable,
        installed_version_source: AgentVersionSource::LocalExecutable,
        diagnostics: vec![],
    };
    NativeFixtureInspector {
        home,
        workspace: None,
        settings: AppSettings::default(),
        definitions: vec![isolated_codex()],
        installations: Arc::new(FixtureInstallation(installation)),
    }
}

fn config_text(disabled: &[&str]) -> String {
    let mut text = String::from(
        "# Preserve this root comment and unknown native fields.\n\
         fixture_unknown = 'keep-root-value'\n\
         [fixture_metadata]\n\
         notes = ['keep', 'ordered']\n",
    );
    for index in 0..MCP_COUNT {
        let name = format!("server-{index}");
        writeln!(
            text,
            "\n# Preserve MCP {index}.\n\
             [mcp_servers.\"{name}\"]\n\
             command = 'balancehub-fixture-mcp-never-run'\n\
             args = ['--fixture', '{index}']\n\
             enabled = {}",
            !disabled.contains(&name.as_str()),
        )
        .unwrap();
    }
    text
}

fn mcp_assets<'a>(
    inventory: &'a AgentEnvironmentInventory,
    config: &Path,
) -> BTreeMap<String, &'a AgentAssetRecord> {
    let sources = inventory
        .sources
        .iter()
        .filter(|source| Path::new(&source.path) == config)
        .collect::<Vec<_>>();
    assert_eq!(sources.len(), 1);
    let source = sources[0];
    let definitions = inventory
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.native_kind == AgentAssetCategory::Mcp
                && declaration.role == AgentAssetDeclarationRole::Definition
        })
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), MCP_COUNT);
    assert_eq!(inventory.assets.len(), MCP_COUNT);
    let assets = inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Mcp)
        .map(|asset| (asset.native_id.clone(), asset))
        .collect::<BTreeMap<_, _>>();
    let expected_names = (0..MCP_COUNT)
        .map(|index| format!("server-{index}"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        definitions
            .iter()
            .map(|declaration| declaration.native_id.clone())
            .collect::<BTreeSet<_>>(),
        expected_names,
    );
    assert_eq!(
        assets.keys().cloned().collect::<BTreeSet<_>>(),
        expected_names
    );
    assert_eq!(
        assets
            .values()
            .map(|asset| &asset.stable_id)
            .collect::<BTreeSet<_>>()
            .len(),
        MCP_COUNT,
    );
    for declaration in definitions {
        assert!(declaration.id.starts_with("declaration:"));
        assert_eq!(declaration.source_id, source.id);
        let asset = assets[&declaration.native_id];
        let hash = asset.stable_id.strip_prefix("asset:").unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().any(|character| character != '0'));
        assert_eq!(asset.context_id, source.context_id);
        assert_eq!(asset.inspection_source_id, source.id);
        assert!(asset.source_ids.contains(&source.id));
        assert!(asset.represented_declaration_ids.contains(&declaration.id));
        assert!(asset.resolution.contributor_ids.contains(&declaration.id));
        assert_eq!(
            asset.resolution.relation,
            AgentAssetResolutionRelation::Independent
        );
        assert!(asset.resolution.terminal.is_none());
    }
    assets
}

fn disable_request(asset: &AgentAssetRecord) -> AgentAssetPlanRequest {
    AgentAssetPlanRequest {
        asset_id: asset.stable_id.clone(),
        action: AgentAssetActionKind::Disable,
        workspace: None,
        expected_revision: asset.revision.identity.clone(),
        installation_id: asset.selected_action_installation_id.clone(),
    }
}

#[test]
fn codex_ten_mcp_entries_share_a_serialized_source_and_replan_without_lost_updates() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = Arc::new(inspector(temporary.path()));
    let config = inspector.home.join(".codex/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    let original = config_text(&[]);
    fs::write(&config, &original).unwrap();

    let before = inspector.inspect().unwrap();
    assert!(before
        .inventory
        .sources
        .iter()
        .all(|source| Path::new(&source.path).starts_with(&inspector.home)));
    let initial = mcp_assets(&before.inventory, &config);
    for asset in initial.values() {
        assert_eq!(asset.declared_state, AgentAssetState::Enabled);
        assert_eq!(asset.effective_state, AgentAssetState::Enabled);
        assert!(asset
            .actions
            .iter()
            .any(|action| action.action == AgentAssetActionKind::Disable && action.available));
    }
    let original_ids = initial
        .iter()
        .map(|(name, asset)| (name.clone(), asset.stable_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let requests = TARGETS.map(|name| disable_request(initial[name]));
    assert_ne!(requests[0].asset_id, requests[1].asset_id);
    let shared_source = initial[TARGETS[0]].inspection_source_id.clone();
    assert_eq!(initial[TARGETS[1]].inspection_source_id, shared_source);

    // Use the default inspector preparer, retaining real source/executable
    // guards and the production physical/configuration mutation domains.
    let prepared = requests.each_ref().map(|request| {
        prepare_request(inspector.as_ref(), &before, request)
            .unwrap()
            .0
    });
    for mutation in &prepared {
        assert!(
            matches!(&mutation.execution, MutationExecution::AtomicFile { source_id, .. } if source_id == &shared_source)
        );
        assert_eq!(mutation.write_source_ids, vec![shared_source.clone()]);
        let file = mutation
            .files
            .iter()
            .find(|file| file.source_id == shared_source)
            .unwrap();
        assert!(mutation.domains.contains(&file.domain()));
    }
    assert!(!prepared[0].domains.is_empty());
    assert_eq!(prepared[0].domains, prepared[1].domains);
    assert_ne!(prepared[0].signature, prepared[1].signature);

    let service = MutationService::default();
    let actor = "codex-shared-source-fixture";
    let operations = requests
        .iter()
        .map(|request| {
            let plan = service
                .plan(actor, request.clone(), inspector.clone())
                .unwrap();
            assert_eq!(plan.source_ids, vec![shared_source.clone()]);
            let operation = service
                .start(
                    actor,
                    &AgentAssetApplyRequest {
                        plan_token: plan.token,
                        asset_id: request.asset_id.clone(),
                        action: request.action,
                    },
                )
                .unwrap();
            assert_eq!(operation.phase, AgentAssetOperationPhase::Preparing);
            operation
        })
        .collect::<Vec<_>>();
    assert_eq!(fs::read_to_string(&config).unwrap(), original);

    let barrier = Barrier::new(operations.len());
    std::thread::scope(|scope| {
        for operation in &operations {
            let barrier = &barrier;
            let service = &service;
            scope.spawn(move || {
                barrier.wait();
                service.run_operation(&operation.id);
            });
        }
    });
    let completed = operations
        .iter()
        .map(|operation| {
            let completed = service.operation(actor, &operation.id).unwrap();
            assert_eq!(completed.phase, AgentAssetOperationPhase::Completed);
            completed
        })
        .collect::<Vec<_>>();
    for outcome in [
        AgentAssetOperationOutcome::AppliedVerified,
        AgentAssetOperationOutcome::UnchangedConflict,
    ] {
        assert_eq!(
            completed
                .iter()
                .filter(|operation| operation.outcome == Some(outcome))
                .count(),
            1
        );
    }
    let conflict = completed
        .iter()
        .position(|operation| {
            operation.outcome == Some(AgentAssetOperationOutcome::UnchangedConflict)
        })
        .unwrap();
    let applied_name = TARGETS[1 - conflict];
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        config_text(&[applied_name])
    );

    let fresh = inspector.inspect().unwrap();
    let current = mcp_assets(&fresh.inventory, &config);
    assert_eq!(
        current[TARGETS[conflict]].effective_state,
        AgentAssetState::Enabled
    );
    assert_eq!(
        current[applied_name].effective_state,
        AgentAssetState::Disabled
    );
    assert_eq!(
        current[TARGETS[conflict]].stable_id,
        requests[conflict].asset_id
    );
    assert_ne!(
        current[TARGETS[conflict]].revision.identity,
        requests[conflict].expected_revision
    );
    let replan = service
        .plan(
            actor,
            disable_request(current[TARGETS[conflict]]),
            inspector.clone(),
        )
        .unwrap();
    let retry = service
        .start(
            actor,
            &AgentAssetApplyRequest {
                plan_token: replan.token,
                asset_id: current[TARGETS[conflict]].stable_id.clone(),
                action: AgentAssetActionKind::Disable,
            },
        )
        .unwrap();
    service.run_operation(&retry.id);
    assert_eq!(
        service.operation(actor, &retry.id).unwrap().outcome,
        Some(AgentAssetOperationOutcome::AppliedVerified)
    );

    let after = inspector.inspect().unwrap();
    for (name, asset) in mcp_assets(&after.inventory, &config) {
        let expected = if TARGETS.contains(&name.as_str()) {
            AgentAssetState::Disabled
        } else {
            AgentAssetState::Enabled
        };
        assert_eq!(asset.stable_id, original_ids[&name]);
        assert_eq!(asset.declared_state, expected);
        assert_eq!(asset.effective_state, expected);
    }
    // Exact bytes also preserve all eight unrelated MCP blocks, unknown root
    // fields, the unknown table, comments, quoting and ordering.
    assert_eq!(fs::read_to_string(&config).unwrap(), config_text(&TARGETS));
}
