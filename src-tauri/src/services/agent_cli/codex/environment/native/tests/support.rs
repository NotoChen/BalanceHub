use super::*;
use crate::services::agent_cli::contracts::{
    AgentContextDiscoveryRequest, AgentDiagnosticEmission, AgentDiagnosticOutput,
    AgentSourceDiscoveryRequest, AgentWorkspaceTrustSourceRequest, EnvironmentAdapter,
    InitialSourceOutput,
};
use crate::services::agent_cli::environment::{build_inventory_for_test, default_context};
use std::{
    cell::RefCell,
    ops::ControlFlow,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub(super) struct Capture {
    pub(super) declarations: Vec<ParsedAgentAsset>,
    pub(super) sources: Vec<AgentAssetSourceSpec>,
    pub(super) drafts: Vec<AgentAssetProjectedDraft>,
}

thread_local! {
    static CAPTURE: RefCell<Capture> = const { RefCell::new(Capture {
        declarations: Vec::new(), sources: Vec::new(), drafts: Vec::new(),
    }) };
}

struct IsolatedSources<'a> {
    home: &'a Path,
    output: &'a mut dyn InitialSourceOutput,
}

impl IsolatedSources<'_> {
    fn isolate(&self, mut source: AgentAssetSourceSpec) -> AgentAssetSourceSpec {
        if matches!(
            source.native_source_key.as_str(),
            "system-config" | "system-requirements" | "admin-skills"
        ) {
            let root = self.home.join(".codex-fixture-system");
            source.path = root.join(source.path.file_name().unwrap());
            source.allowed_root = root;
        }
        assert!(
            source.path.starts_with(self.home),
            "fixture source escaped its isolated home"
        );
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

fn contexts(
    request: AgentContextDiscoveryRequest<'_>,
    _: &mut dyn AgentDiagnosticOutput,
) -> Vec<AgentConfigurationContext> {
    // Never resolve the test context through the real CODEX_HOME variable.
    default_context(
        request,
        request.home.join(".codex"),
        super::super::super::PARSER_VERSION,
    )
}

fn sources(request: AgentSourceDiscoveryRequest<'_>, output: &mut dyn InitialSourceOutput) {
    super::super::sources::discover_sources(
        request,
        &mut IsolatedSources {
            home: request.home,
            output,
        },
    );
}

fn trust_sources(
    request: AgentWorkspaceTrustSourceRequest<'_>,
    output: &mut dyn InitialSourceOutput,
) {
    super::super::super::discover_workspace_trust_sources(
        request,
        &mut IsolatedSources {
            home: request.home,
            output,
        },
    );
}

fn resolve<const REVERSE: bool>(
    request: AgentAssetResolveRequest<'_>,
    output: &mut dyn AgentResolveOutput,
) {
    let mut declarations = request.declarations.to_vec();
    let mut sources = request.sources.to_vec();
    if REVERSE {
        declarations.reverse();
        sources.reverse();
    }
    CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        capture.declarations = declarations.clone();
        capture.sources = sources.iter().map(|source| source.spec.clone()).collect();
    });
    struct Tee<'a>(&'a mut dyn AgentResolveOutput);
    impl AgentDiagnosticOutput for Tee<'_> {
        fn has_regular_capacity(&self) -> bool {
            self.0.has_regular_capacity()
        }
        fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
            self.0.emit_diagnostic(value)
        }
    }
    impl AgentResolveOutput for Tee<'_> {
        fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
            CAPTURE.with(|capture| capture.borrow_mut().drafts.push(value.clone()));
            self.0.emit_draft(value)
        }
    }
    super::super::super::resolve_assets(
        AgentAssetResolveRequest {
            context: request.context,
            declarations: &declarations,
            sources: &sources,
        },
        &mut Tee(output),
    );
}

fn assess<const REVERSE: bool>(
    request: AgentAssetAssessmentRequest<'_>,
) -> AgentAssetAssessmentIndex {
    let mut declarations = request.declarations.to_vec();
    let mut sources = request.sources.to_vec();
    if REVERSE {
        declarations.reverse();
        sources.reverse();
    }
    super::super::super::assess_assets(AgentAssetAssessmentRequest {
        context: request.context,
        targets: request.targets,
        declarations: &declarations,
        sources: &sources,
    })
}

pub(super) struct Fixture {
    pub(super) home: PathBuf,
    pub(super) workspace: PathBuf,
}

impl Fixture {
    pub(super) fn new(name: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "balancehub-codex-assets-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        for relative in [".codex", ".codex-fixture-system", "workspace/.codex"] {
            fs::create_dir_all(home.join(relative)).unwrap();
        }
        let home = home.canonicalize().unwrap();
        let workspace = home.join("workspace");
        Self { home, workspace }
    }

    pub(super) fn plugin(&self, id: &str, version: &str, namespace: &str) -> PathBuf {
        let (name, marketplace) = super::super::plugin::plugin_id_parts(id).unwrap();
        let root = self
            .home
            .join(".codex/plugins/cache")
            .join(marketplace)
            .join(name)
            .join(version);
        write(
            &root.join(".codex-plugin/plugin.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": namespace, "description": PRIVATE_DESCRIPTION,
            }))
            .unwrap(),
        );
        root
    }

    pub(super) fn run<const REVERSE: bool>(&self, config: &str) -> Run {
        self.run_with_workspace::<REVERSE>(config, true)
    }

    pub(super) fn run_with_workspace<const REVERSE: bool>(
        &self,
        config: &str,
        workspace: bool,
    ) -> Run {
        self.run_with_trust::<REVERSE>(config, workspace.then_some(true))
    }

    pub(super) fn run_with_trust<const REVERSE: bool>(
        &self,
        config: &str,
        workspace_trust: Option<bool>,
    ) -> Run {
        let trust = if let Some(trusted) = workspace_trust {
            format!(
                "[projects.{}]\ntrust_level = '{}'\n",
                toml::Value::String(self.workspace.to_string_lossy().into_owned()),
                if trusted { "trusted" } else { "untrusted" },
            )
        } else {
            String::new()
        };
        write(
            &self.home.join(".codex/config.toml"),
            format!("{config}\n{trust}"),
        );
        let mut definition = *crate::services::agent_cli::definition(AgentCliKind::Codex);
        definition.environment = EnvironmentAdapter::with_pipeline(
            contexts,
            sources,
            Some(super::super::sources::discover_follow_up_sources),
            "codex-native-assets-fixture",
            super::super::super::parse_assets,
            resolve::<REVERSE>,
            assess::<REVERSE>,
        )
        .with_workspace_trust_authority(
            trust_sources,
            super::super::super::resolve_workspace_trust,
        );
        CAPTURE.with(|capture| *capture.borrow_mut() = Capture::default());
        let inventory = build_inventory_for_test(
            &self.home,
            workspace_trust.map(|_| self.workspace.as_path()),
            &[definition],
        )
        .unwrap();
        let capture = CAPTURE.with(|capture| std::mem::take(&mut *capture.borrow_mut()));
        assert_eq!(inventory.contexts.len(), 1);
        assert_eq!(inventory.contexts[0].parser_version, 3);
        assert_eq!(
            Path::new(&inventory.contexts[0].config_root),
            self.home.join(".codex")
        );
        assert!(inventory
            .sources
            .iter()
            .all(|source| Path::new(&source.path).starts_with(&self.home)));
        for diagnostic in inventory
            .diagnostics
            .iter()
            .chain(
                inventory
                    .sources
                    .iter()
                    .flat_map(|source| &source.diagnostics),
            )
            .chain(
                inventory
                    .declarations
                    .iter()
                    .flat_map(|declaration| &declaration.diagnostics),
            )
            .chain(inventory.assets.iter().flat_map(|asset| &asset.diagnostics))
        {
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
        let public = serde_json::to_string(&inventory).unwrap();
        let native_debug = format!("{:?}", capture.declarations);
        for private in [
            PRIVATE_DESCRIPTION,
            PRIVATE_BODY,
            PRIVATE_COMMAND,
            PRIVATE_TOKEN,
        ] {
            assert!(
                !public.contains(private),
                "private source content crossed IPC"
            );
            assert!(
                !native_debug.contains(private),
                "private source content crossed Debug"
            );
        }
        Run { inventory, capture }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

pub(super) struct Run {
    pub(super) inventory: AgentEnvironmentInventory,
    pub(super) capture: Capture,
}

impl Run {
    pub(super) fn rows(&self, category: AgentAssetCategory) -> Vec<&AgentAssetRecord> {
        self.inventory
            .assets
            .iter()
            .filter(|row| row.category == category)
            .collect()
    }
    pub(super) fn row(&self, category: AgentAssetCategory, name: &str) -> &AgentAssetRecord {
        let matches = self
            .rows(category)
            .into_iter()
            .filter(|row| {
                row.native_id == name
                    && row.resolution.relation != AgentAssetResolutionRelation::Replaced
            })
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "missing or duplicate native row {name}");
        matches[0]
    }
    pub(super) fn draft(
        &self,
        category: AgentAssetCategory,
        name: &str,
    ) -> &AgentAssetProjectedDraft {
        self.capture
            .drafts
            .iter()
            .find(|draft| {
                draft.native_kind == category
                    && draft.native_id == name
                    && draft.resolution.relation != AgentAssetResolutionRelation::Replaced
            })
            .unwrap()
    }
    pub(super) fn assess(
        &self,
        declarations: &[ParsedAgentAsset],
        category: AgentAssetCategory,
        name: &str,
    ) -> AgentAssetAssessmentResult {
        let target = AgentAssetAssessmentTarget {
            category,
            resolution_group_key: name.to_owned(),
            exact_native_id: name.to_owned(),
            subject: AgentAssetAssessmentSubject::Bucket,
        };
        super::super::super::assess_assets(AgentAssetAssessmentRequest {
            context: &self.inventory.contexts[0],
            targets: std::slice::from_ref(&target),
            declarations,
            sources: &self.capture.sources,
        })
        .get(&target)
        .unwrap()
        .clone()
    }
    pub(super) fn normalized_rows(&self) -> Value {
        let mut assets = self.inventory.assets.clone();
        for row in &mut assets {
            row.revision.observed_at = "2026-09-13T00:00:00+00:00".to_owned();
        }
        assets.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
        serde_json::to_value(assets).unwrap()
    }
}

pub(super) fn write(path: &Path, content: impl AsRef<[u8]>) {
    if fs::read(path).is_ok_and(|existing| existing == content.as_ref()) {
        return;
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

pub(super) fn skill(path: &Path, name: &str) {
    write(
        path,
        format!("---\nname: '{name}'\ndescription: {PRIVATE_DESCRIPTION}\n---\n{PRIVATE_BODY}\n"),
    );
}
