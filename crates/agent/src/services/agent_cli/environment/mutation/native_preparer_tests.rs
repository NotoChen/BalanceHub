use super::super::{
    inventory::{native_environment, InstallationDiscoveryPort, InstallationDiscoveryRequest},
    run::AgentInventoryRun,
};
use super::{
    native_test_support::NativeFixtureInspector, MutationInspector, MutationInventory,
    MutationPreparation, MutationVerification, PreparedMutation,
};
use crate::{
    models::*,
    services::agent_cli::{contracts::*, definition, AgentCliDefinition},
};
use std::{
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::Arc,
};

struct FixtureInstallation(AgentInstallation);

impl InstallationDiscoveryPort for FixtureInstallation {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        _: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        assert_eq!(request.definition.kind, AgentCliKind::Gemini);
        vec![self.0.clone()]
    }
}

struct IsolatedSources<'a> {
    output: &'a mut dyn InitialSourceOutput,
    root: PathBuf,
}

impl IsolatedSources<'_> {
    fn isolate(&self, mut source: AgentAssetSourceSpec) -> AgentAssetSourceSpec {
        if matches!(
            source.native_source_key.as_str(),
            "system-defaults" | "system-settings"
        ) {
            source.allowed_root = self.root.join("managed-fixture");
            source.path = source
                .allowed_root
                .join(format!("{}.json", source.native_source_key));
        }
        source
    }
}

impl AgentDiagnosticOutput for IsolatedSources<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.output.has_regular_capacity()
    }
    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.output.emit_diagnostic(value)
    }
}

impl InitialSourceOutput for IsolatedSources<'_> {
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

fn isolated_gemini() -> AgentCliDefinition {
    let native = definition(AgentCliKind::Gemini);
    AgentCliDefinition {
        environment: EnvironmentAdapter::with_pipeline(
            |request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .discover_contexts(request, output)
            },
            |request, output| {
                let root = PathBuf::from(&request.context.config_root);
                definition(AgentCliKind::Gemini)
                    .environment()
                    .discover_sources(request, &mut IsolatedSources { output, root });
            },
            Some(|request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .discover_follow_up_sources(request, output)
            }),
            "@google/gemini-cli",
            |request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .parse(request, output)
            },
            |request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .resolve(request, output)
            },
            native.environment().state_assessor(),
        )
        .with_workspace_trust_authority(
            |request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .discover_workspace_trust_sources(request, output)
            },
            |request, output| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .resolve_workspace_trust(request, output)
            },
        )
        .with_asset_mutation(
            |kind| definition(kind).environment().mechanism_records(kind),
            |request| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .prepare_mutation(request)
            },
            |category, action| {
                definition(AgentCliKind::Gemini)
                    .environment()
                    .native_unavailable_reason(category, action)
            },
        ),
        ..*native
    }
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn inspector(root: &Path) -> NativeFixtureInspector {
    let home = root.canonicalize().unwrap();
    let executable = home.join("fixture-gemini");
    write(
        &executable,
        "native preparer fixture; this file is never executed",
    );
    let installation = AgentInstallation {
        id: "fixture-gemini".into(),
        environment_id: native_environment().id,
        agent_kind: AgentCliKind::Gemini,
        label: "Gemini fixture".into(),
        availability: AgentInstallationAvailability::Available,
        executable_path: Some(executable.to_string_lossy().into_owned()),
        executable_identity: Some(AgentExecutableIdentity {
            owner: "fixture".into(),
            canonical_path: executable.to_string_lossy().into_owned(),
            installation_source: AgentDiscoverySource::Configured,
        }),
        executable_revision: Some("fixture-executable".into()),
        installed_version: Some("0.59.0".into()),
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
        definitions: vec![isolated_gemini()],
        installations: Arc::new(FixtureInstallation(installation)),
    }
}

fn prepare_extension(
    inspector: &NativeFixtureInspector,
    snapshot: &MutationInventory,
) -> Result<PreparedMutation, AgentAssetMutationError> {
    let inventory = &snapshot.inventory;
    let asset = inventory
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Extension)
        .unwrap();
    let context = inventory
        .contexts
        .iter()
        .find(|context| context.id == asset.context_id)
        .unwrap();
    let mechanism = inventory
        .mechanisms
        .iter()
        .find(|mechanism| {
            mechanism.category == asset.category && mechanism.action == AgentAssetActionKind::Enable
        })
        .unwrap();
    inspector.prepare(MutationPreparation {
        inventory,
        source_anchors: &snapshot.source_anchors,
        context,
        asset,
        installation: &inventory.installations[0],
        mechanism,
        action: AgentAssetActionKind::Enable,
        home: inspector.home(),
        workspace: inspector.workspace(),
    })
}

#[test]
fn gemini_extension_plan_guards_and_verifies_same_name_settings_winner() {
    let temporary = tempfile::tempdir().unwrap();
    let inspector = inspector(temporary.path());
    let root = inspector.home.join(".gemini");
    let settings = root.join("settings.json");
    let extension_state = root.join("extensions/extension-enablement.json");
    let mcp_state = root.join("mcp-server-enablement.json");
    write(
        &settings,
        br#"{"mcpServers":{"Shared":{"command":"fixture-settings"}}}"#,
    );
    write(&root.join("extensions/review-kit/gemini-extension.json"), br#"{"name":"review-kit","version":"1.0.0","mcpServers":{"Shared":{"command":"fixture-extension"}}}"#);
    let home_pattern = format!("{}/*", inspector.home.display());
    write(
        &extension_state,
        serde_json::to_vec(
            &serde_json::json!({"review-kit": {"overrides": [format!("!{home_pattern}")]}}),
        )
        .unwrap(),
    );
    write(&mcp_state, br#"{"shared":{"enabled":false}}"#);
    let before = inspector.inspect().unwrap();
    let parent = before
        .inventory
        .assets
        .iter()
        .find(|asset| asset.category == AgentAssetCategory::Extension)
        .unwrap();
    assert_eq!(parent.effective_state, AgentAssetState::Disabled);
    assert!(before
        .inventory
        .contexts
        .iter()
        .all(|context| context.workspace_id.is_none()));
    assert!(before
        .inventory
        .sources
        .iter()
        .all(|source| source.workspace_id.is_none()));
    let winner = before
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.resolution.relation == AgentAssetResolutionRelation::ReplaceWinner
        })
        .unwrap();
    let replaced = before
        .inventory
        .assets
        .iter()
        .find(|asset| {
            asset.category == AgentAssetCategory::Mcp
                && asset.resolution.relation == AgentAssetResolutionRelation::Replaced
        })
        .unwrap();
    assert!(winner.relationships.provided_by.is_none());
    assert_eq!(winner.effective_state, AgentAssetState::Disabled);
    assert_eq!(replaced.effective_state, AgentAssetState::Shadowed);
    let prepared = prepare_extension(&inspector, &before).unwrap();
    for asset in [parent, winner, replaced] {
        assert!(prepared.affected_asset_ids.contains(&asset.stable_id));
    }
    assert!(prepared.files.iter().any(|file| file.path() == settings));
    assert!(prepared
        .changes
        .iter()
        .any(|change| change.label.contains("同名 MCP")));
    prepared.revalidate().unwrap();
    let verified = |inventory: &AgentEnvironmentInventory, affected: &[String]| {
        (prepared.verify)(MutationVerification {
            inventory,
            asset_id: &parent.stable_id,
            action: AgentAssetActionKind::Enable,
            affected_asset_ids: affected,
        })
    };
    assert!(!verified(&before.inventory, &prepared.affected_asset_ids));
    write(
        &extension_state,
        serde_json::to_vec(&serde_json::json!({"review-kit": {"overrides": [home_pattern]}}))
            .unwrap(),
    );
    assert!(!verified(
        &inspector.inspect().unwrap().inventory,
        &prepared.affected_asset_ids
    ));
    write(&mcp_state, br#"{"shared":{"enabled":true}}"#);
    let enabled = inspector.inspect().unwrap();
    assert_eq!(
        enabled
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == parent.stable_id)
            .unwrap()
            .effective_state,
        AgentAssetState::Enabled
    );
    assert!(verified(&enabled.inventory, &prepared.affected_asset_ids));
    let incomplete = prepared
        .affected_asset_ids
        .iter()
        .filter(|id| *id != &winner.stable_id)
        .cloned()
        .collect::<Vec<_>>();
    assert!(!verified(&enabled.inventory, &incomplete));
    assert_eq!(
        enabled
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == winner.stable_id)
            .unwrap()
            .effective_state,
        AgentAssetState::Enabled
    );
    write(&settings, br#"{"mcpServers":{"Shared":{"command":"fixture-settings"}},"mcp":{"excluded":["Shared"]}}"#);
    let blocked = inspector.inspect().unwrap();
    assert_eq!(
        blocked
            .inventory
            .assets
            .iter()
            .find(|asset| asset.stable_id == winner.stable_id)
            .unwrap()
            .effective_state,
        AgentAssetState::Blocked
    );
    assert!(verified(&blocked.inventory, &prepared.affected_asset_ids));
    write(&mcp_state, "{");
    assert!(!verified(
        &inspector.inspect().unwrap().inventory,
        &prepared.affected_asset_ids
    ));
    fs::remove_file(&mcp_state).unwrap();
    assert!(verified(
        &inspector.inspect().unwrap().inventory,
        &prepared.affected_asset_ids
    ));
    let fresh = inspector.inspect().unwrap();
    let guarded = prepare_extension(&inspector, &fresh).unwrap();
    write(
        &settings,
        br#"{"mcpServers":{"Shared":{"command":"changed-settings"}}}"#,
    );
    assert_eq!(
        guarded.revalidate().err().unwrap().kind,
        AgentAssetMutationErrorKind::SourceConflict
    );
    let mut missing_definition = inspector.inspect().unwrap();
    missing_definition
        .inventory
        .declarations
        .retain(|declaration| {
            !replaced
                .represented_declaration_ids
                .contains(&declaration.id)
        });
    assert_eq!(
        prepare_extension(&inspector, &missing_definition)
            .err()
            .unwrap()
            .reason,
        Some(AgentAssetActionUnavailableReason::UnsupportedSchema)
    );
}
