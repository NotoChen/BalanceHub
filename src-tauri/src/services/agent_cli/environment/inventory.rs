//! Adapter-owned logical inventory orchestration.
//!
//! This module coordinates bounded discovery. It deliberately does not know any
//! Agent-native configuration key or resolution rule.

use super::super::{contracts::*, definitions};
use super::{
    access_registry::AgentSourceAccessEvidence,
    diagnostics::{rebind_source_id, DiagnosticOwner},
    identity::{context_stable_id, lexical_absolute, source_stable_id},
    output::{
        source_queue_key, BoundedParseOutput, BoundedResolveOutput, BoundedSourceOutput,
        ContextSourceState, PhysicalSourceKey,
    },
    projection::{project_records, AgentAssetProjectionInput},
    run::{AgentInventoryRun, AgentInventoryStage, MonotonicClock, SystemMonotonicClock},
    snapshot::{
        now_string, path_has_symlink_component, snapshot_revision_of, RealSnapshotPort,
        SnapshotPort, SnapshotRequest,
    },
};
use crate::{
    models::*,
    services::{agent_cli, cli_paths::user_home},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

/// One bounded scan provides both public facts and private object anchors.
/// Only the UI command publishes these anchors as a new access generation.
#[derive(Clone)]
pub(crate) struct InventoryBuild {
    pub(crate) inventory: AgentEnvironmentInventory,
    pub(crate) access_evidence: Vec<AgentSourceAccessEvidence>,
}

/// Display-only incremental file reuse; authoritative mutation scans stay fresh.
pub(crate) fn display_inventory(
    settings: &AppSettings,
    workspace: Option<&Path>,
    kinds: Option<&BTreeSet<AgentCliKind>>,
    canceled: Option<Arc<AtomicBool>>,
) -> Result<InventoryBuild, String> {
    let home = user_home().ok_or("无法定位用户目录")?;
    let lexical = workspace.map(Path::to_path_buf);
    let workspace = normalize_optional_workspace(workspace)?;
    let registered = definitions()
        .iter()
        .filter(|definition| kinds.is_none_or(|kinds| kinds.contains(&definition.kind)))
        .cloned()
        .collect::<Vec<_>>();
    let snapshots = RealSnapshotPort::for_display();
    let started = std::time::Instant::now();
    let result = build_inventory_with_lexical(
        InventoryInput {
            home: &home,
            workspace: workspace.as_deref(),
            settings: Some(settings),
        },
        InventoryPipelineDeps {
            definitions: &registered,
            limits: production_limits(),
            clock: Arc::new(SystemMonotonicClock),
            installations: &RealInstallationDiscoveryPort,
            snapshots: &snapshots,
            #[cfg(test)]
            checkpoint_probe: None,
        },
        lexical.as_deref(),
        canceled,
    );
    if cfg!(debug_assertions) {
        let (hits, reads, bytes) = snapshots.metrics();
        eprintln!(
            "Agent 展示扫描：Agent数={}，缓存命中={}，文件读取={}，读取字节={}，耗时={}ms",
            registered.len(),
            hits,
            reads,
            bytes,
            started.elapsed().as_millis()
        );
    }
    result
}

pub(crate) fn inventory_with_access(
    settings: &crate::models::AppSettings,
    workspace: Option<&Path>,
) -> Result<InventoryBuild, String> {
    inventory_for_definitions(settings, workspace, definitions(), None)
}

/// Selected adapters share one refresh budget, including follow-up sources.
pub(crate) fn inventory_for_agents_cancellable(
    settings: &AppSettings,
    workspace: Option<&Path>,
    kinds: &BTreeSet<AgentCliKind>,
    canceled: Option<Arc<AtomicBool>>,
) -> Result<InventoryBuild, String> {
    let registered = definitions()
        .iter()
        .filter(|definition| kinds.contains(&definition.kind))
        .cloned()
        .collect::<Vec<_>>();
    inventory_for_definitions(settings, workspace, &registered, canceled)
}

fn inventory_for_definitions(
    settings: &crate::models::AppSettings,
    workspace: Option<&Path>,
    registered_definitions: &[super::super::AgentCliDefinition],
    canceled: Option<Arc<AtomicBool>>,
) -> Result<InventoryBuild, String> {
    let home = user_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    // Keep the caller's absolute spelling for native trust lookup. The
    // canonical path below remains the access/identity path.
    let workspace_lexical = workspace.map(Path::to_path_buf);
    let workspace = normalize_optional_workspace(workspace)?;
    build_inventory(
        &home,
        workspace.as_deref(),
        workspace_lexical.as_deref(),
        Some(settings),
        registered_definitions,
        canceled,
    )
}

pub(super) struct InventoryInput<'a> {
    pub(super) home: &'a Path,
    pub(super) workspace: Option<&'a Path>,
    pub(super) settings: Option<&'a crate::models::AppSettings>,
}

type AuthorityCacheKey = (String, String, Option<String>, PhysicalSourceKey);

#[derive(Debug, Clone)]
struct AuthoritySnapshot {
    context_id: String,
    source_id: String,
    spec: AgentAssetSourceSpec,
    snapshot: AgentAssetSnapshot,
}

pub(super) struct InstallationDiscoveryRequest<'a> {
    pub(super) preferred_path: &'a str,
    pub(super) definition: &'a super::super::AgentCliDefinition,
    pub(super) include_shell: bool,
}

pub(super) trait InstallationDiscoveryPort: Send + Sync {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation>;
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct RealInstallationDiscoveryPort;

impl InstallationDiscoveryPort for RealInstallationDiscoveryPort {
    fn discover(
        &self,
        request: InstallationDiscoveryRequest<'_>,
        run: &mut AgentInventoryRun,
    ) -> Vec<AgentInstallation> {
        agent_cli::discover_installations(
            request.preferred_path,
            request.definition,
            request.include_shell,
            run,
        )
    }
}

pub(super) struct InventoryPipelineDeps<'a> {
    pub(super) definitions: &'a [super::super::AgentCliDefinition],
    pub(super) limits: AgentAssetLimits,
    pub(super) clock: Arc<dyn MonotonicClock>,
    pub(super) installations: &'a dyn InstallationDiscoveryPort,
    pub(super) snapshots: &'a dyn SnapshotPort,
    #[cfg(test)]
    pub(super) checkpoint_probe: Option<Arc<dyn super::run::InventoryCheckpointProbe>>,
}

pub(super) fn production_limits() -> AgentAssetLimits {
    // One production refresh covers all Agents and their expanded package
    // sources. Keep every other read/discovery limit at its default value.
    AgentAssetLimits {
        refresh_budget_ms: AgentAssetLimits::HARD_CAP.refresh_budget_ms,
        sources_per_context: AgentAssetLimits::HARD_CAP.sources_per_context,
        ..AgentAssetLimits::defaults()
    }
}

fn build_inventory(
    home: &Path,
    workspace: Option<&Path>,
    workspace_lexical: Option<&Path>,
    settings: Option<&crate::models::AppSettings>,
    registered_definitions: &[super::super::AgentCliDefinition],
    canceled: Option<Arc<AtomicBool>>,
) -> Result<InventoryBuild, String> {
    let installations = RealInstallationDiscoveryPort;
    let snapshots = RealSnapshotPort::default();
    let input = InventoryInput {
        home,
        workspace,
        settings,
    };
    build_inventory_with_lexical(
        input,
        InventoryPipelineDeps {
            definitions: registered_definitions,
            limits: production_limits(),
            clock: Arc::new(SystemMonotonicClock),
            installations: &installations,
            snapshots: &snapshots,
            #[cfg(test)]
            checkpoint_probe: None,
        },
        workspace_lexical,
        canceled,
    )
}

#[cfg(test)]
pub(super) fn build_inventory_with(
    input: InventoryInput<'_>,
    deps: InventoryPipelineDeps<'_>,
) -> Result<AgentEnvironmentInventory, String> {
    let workspace_lexical = input.workspace;
    build_inventory_with_lexical(input, deps, workspace_lexical, None).map(|build| build.inventory)
}

fn build_inventory_with_lexical(
    input: InventoryInput<'_>,
    deps: InventoryPipelineDeps<'_>,
    workspace_lexical: Option<&Path>,
    canceled: Option<Arc<AtomicBool>>,
) -> Result<InventoryBuild, String> {
    let home = input.home;
    let workspace = input.workspace;
    let settings = input.settings;
    let environment = native_environment();
    #[cfg(test)]
    let mut run = AgentInventoryRun::with_clock_and_probe(
        deps.limits,
        Arc::clone(&deps.clock),
        deps.checkpoint_probe,
    );
    #[cfg(not(test))]
    let mut run = AgentInventoryRun::with_clock(deps.limits, Arc::clone(&deps.clock));
    run.set_cancellation(canceled);
    let limits = run.limits().clone();
    let mut installations = Vec::new();
    let mut contexts = Vec::new();
    let mut sources = BTreeMap::<String, AgentAssetSource>::new();
    let mut declarations = Vec::new();
    let mut assets = Vec::new();
    let mut hook_rule_counts = super::hook_counts::HookRuleCounts::default();
    let mut capabilities = Vec::new();
    let mut mechanisms = Vec::new();
    let mut access_evidence = BTreeMap::new();

    for registered in deps.definitions {
        if !run.checkpoint(AgentInventoryStage::InstallationDiscovery) {
            break;
        }
        mechanisms.extend(registered.environment().mechanism_records(registered.kind));
        let agent_installations = settings
            .map(|settings| {
                deps.installations.discover(
                    InstallationDiscoveryRequest {
                        preferred_path: settings.agent_cli_path(registered.kind),
                        definition: registered,
                        include_shell: false,
                    },
                    &mut run,
                )
            })
            .unwrap_or_default();
        let readonly_external_roots = registered
            .environment()
            .readonly_external_roots(&agent_installations);
        installations.extend(agent_installations.iter().cloned());

        if !run.checkpoint(AgentInventoryStage::ContextDiscovery) {
            break;
        }
        #[cfg(test)]
        run.record_event(super::run::InventoryPipelineEvent::ContextDiscovery {
            agent_kind: registered.kind,
        });
        let (mut agent_contexts, mut authority_snapshots) = discover_normalized_contexts(
            registered,
            &environment,
            home,
            (workspace, workspace_lexical),
            &agent_installations,
            deps.snapshots,
            &mut run,
        );
        for context in &agent_contexts {
            rebind_authority_diagnostics(context, &mut authority_snapshots, &mut run);
        }

        let mut agent_capabilities =
            BTreeMap::<AgentAssetCategory, BTreeSet<AgentAssetScope>>::new();
        for context in &agent_contexts {
            // Keep the public declaration boundary for this context.  A
            // later context must never clear diagnostics already materialized
            // on declarations from an earlier context.
            let public_declaration_start = declarations.len();
            if !run.checkpoint(AgentInventoryStage::InitialSourceDiscovery) {
                break;
            }
            #[cfg(test)]
            run.record_event(super::run::InventoryPipelineEvent::InitialSourceDiscovery {
                context_id: context.id.clone(),
            });

            // Source discovery and follow-up discovery share one bounded state.
            // The sinks are deliberately short-lived so snapshotting can
            // borrow the run independently of adapter callbacks.
            let mut source_state = ContextSourceState::new(limits.sources_per_context);
            {
                let mut snapshot_seed =
                    |source: &AgentAssetSourceSpec, run: &mut AgentInventoryRun| {
                        let source_id = source_stable_id(context, &source.path);
                        let roots = trusted_roots(
                            home,
                            context,
                            workspace,
                            Some(source),
                            &readonly_external_roots,
                        );
                        cached_authority_snapshot(&authority_snapshots, context, source, &source_id)
                            .unwrap_or_else(|| {
                                deps.snapshots.snapshot(
                                    SnapshotRequest {
                                        source,
                                        source_id: &source_id,
                                        trusted_roots: &roots,
                                    },
                                    run,
                                )
                            })
                    };
                let mut source_output =
                    BoundedSourceOutput::initial(&mut source_state, &mut run, &context.id)
                        .with_snapshot_reader(&mut snapshot_seed);
                registered.environment().discover_sources(
                    AgentSourceDiscoveryRequest {
                        context,
                        home,
                        workspace,
                        installations: &agent_installations,
                    },
                    &mut source_output,
                );
            }

            'source_expansion: for depth in 0..=super::output::MAX_FOLLOW_UP_DEPTH {
                let parents = source_state.eligible_sources_at_depth(depth);
                for parent in parents {
                    if source_state.is_closed() {
                        break 'source_expansion;
                    }
                    // A shallower callback can add a lexically earlier source
                    // or a collision. Re-check the deterministic retained view
                    // before paying for a snapshot from the stale depth batch.
                    if !source_state.is_eligible(&parent) {
                        continue;
                    }
                    if !run.checkpoint(AgentInventoryStage::Snapshot) {
                        break 'source_expansion;
                    }
                    if !run.reads_open() {
                        run.report_closed_read(DiagnosticOwner::Context(context.id.clone()));
                        break 'source_expansion;
                    }
                    let parent_source_id = source_stable_id(context, &parent.spec.path);
                    let parent_key = source_queue_key(&parent.spec);
                    if !source_state.cached_snapshots.contains_key(&parent_key) {
                        let trusted_roots = trusted_roots(
                            home,
                            context,
                            workspace,
                            Some(&parent.spec),
                            &readonly_external_roots,
                        );
                        let snapshot = cached_authority_snapshot(
                            &authority_snapshots,
                            context,
                            &parent.spec,
                            &parent_source_id,
                        )
                        .unwrap_or_else(|| {
                            deps.snapshots.snapshot(
                                SnapshotRequest {
                                    source: &parent.spec,
                                    source_id: &parent_source_id,
                                    trusted_roots: &trusted_roots,
                                },
                                &mut run,
                            )
                        });
                        source_state.cache_snapshot(parent_key.clone(), snapshot);
                    }

                    let complete =
                        source_state
                            .cached_snapshots
                            .get(&parent_key)
                            .is_some_and(|snapshot| {
                                matches!(
                                    snapshot,
                                    AgentAssetSnapshot::DirectoryManifest { complete: true, .. }
                                )
                            });
                    if depth >= super::output::MAX_FOLLOW_UP_DEPTH
                        || !complete
                        || !registered.environment().has_follow_up_sources()
                        || !run.checkpoint(AgentInventoryStage::FollowUpSourceDiscovery)
                        || !run.reads_open()
                    {
                        continue;
                    }
                    if let Some(parent_snapshot) = source_state.take_snapshot(&parent_key) {
                        let manifest = match &parent_snapshot {
                            AgentAssetSnapshot::DirectoryManifest {
                                entries,
                                revision,
                                complete: true,
                                ..
                            } => Some((entries.as_slice(), revision)),
                            _ => None,
                        };
                        if let Some((manifest, manifest_revision)) = manifest {
                            #[cfg(test)]
                            run.record_event(
                                super::run::InventoryPipelineEvent::FollowUpSourceDiscovery {
                                    context_id: context.id.clone(),
                                    parent_source_key: parent.spec.native_source_key.clone(),
                                },
                            );
                            {
                                let mut follow_up_output = BoundedSourceOutput::follow_up(
                                    &mut source_state,
                                    &mut run,
                                    &context.id,
                                    &parent.spec,
                                    manifest,
                                    depth,
                                )
                                .with_manifest_revision(manifest_revision);
                                registered.environment().discover_follow_up_sources(
                                    AgentFollowUpSourceDiscoveryRequest {
                                        parent: &parent.spec,
                                        manifest,
                                    },
                                    &mut follow_up_output,
                                );
                            }
                            if source_state.is_closed() {
                                source_state.restore_snapshot(parent_key, parent_snapshot);
                                break 'source_expansion;
                            }
                        }
                        source_state.restore_snapshot(parent_key, parent_snapshot);
                    }
                }
            }

            let accepted_source_specs = source_state.finish(&mut run, &context.id);
            // Discovery admission is not enough to make a source authoritative.
            // Only sources with a committed snapshot may affect downstream
            // capabilities, resolution, or projection.
            let mut source_specs = Vec::new();

            let mut parsed = Vec::new();
            // Resolution is only authoritative when every accepted source has
            // completed its bounded parse. A deterministic prefix remains
            // useful for inventory display, but must not be treated as the
            // effective configuration after a parse stop.
            let mut parse_complete = true;
            for source in &accepted_source_specs {
                let source_id = source_stable_id(context, &source.path);
                let snapshot_key = source_queue_key(source);
                if !source_state.cached_snapshots.contains_key(&snapshot_key) {
                    // Do not open an accepted source after the shared gate is
                    // closed. Cached snapshots have already paid their read
                    // charge and must still be published below.
                    if !run.checkpoint(AgentInventoryStage::Snapshot) {
                        parse_complete = false;
                        break;
                    }
                    if !run.reads_open() {
                        run.report_closed_read(DiagnosticOwner::Source(source_id.clone()));
                        continue;
                    }
                    let trusted_roots = trusted_roots(
                        home,
                        context,
                        workspace,
                        Some(source),
                        &readonly_external_roots,
                    );
                    let snapshot = cached_authority_snapshot(
                        &authority_snapshots,
                        context,
                        source,
                        &source_id,
                    )
                    .unwrap_or_else(|| {
                        deps.snapshots.snapshot(
                            SnapshotRequest {
                                source,
                                source_id: &source_id,
                                trusted_roots: &trusted_roots,
                            },
                            &mut run,
                        )
                    });
                    source_state.cache_snapshot(snapshot_key.clone(), snapshot);
                }
                let Some(snapshot) = source_state.cached_snapshots.get(&snapshot_key).cloned()
                else {
                    // A source accepted by discovery but never snapshotted is
                    // intentionally not exposed or parsed.
                    continue;
                };
                let revision = snapshot_revision_of(&snapshot);
                let snapshot_anchor = deps.snapshots.access_anchor(&revision);
                let mut captured_source = source.clone();
                captured_source.verified_physical_path = snapshot_anchor
                    .as_ref()
                    .map(|anchor| anchor.physical_path().to_path_buf());
                let source = &captured_source;
                source_specs.push(source.clone());
                sources
                    .entry(source_id.clone())
                    .or_insert_with(|| AgentAssetSource {
                        id: source_id.clone(),
                        context_id: context.id.clone(),
                        label: source.label.clone(),
                        scope: source.scope,
                        origin: source.origin,
                        environment_id: environment.id.clone(),
                        workspace_id: context.workspace_id.clone(),
                        path: source.path.to_string_lossy().into_owned(),
                        allowed_root: source.allowed_root.to_string_lossy().into_owned(),
                        precedence: source.precedence,
                        writable: source.writable,
                        sensitive: source.sensitive,
                        source_kind: source.source_kind,
                        categories: source.categories.clone(),
                        revision: revision.clone(),
                        diagnostics: Vec::new(),
                        access: AgentAssetAccess::default(),
                        actions: super::read_only_action_set()
                            .into_iter()
                            .filter(|action| {
                                !matches!(
                                    action.action,
                                    AgentAssetActionKind::Enable
                                        | AgentAssetActionKind::Disable
                                        | AgentAssetActionKind::Remove
                                )
                            })
                            .collect(),
                    });
                if let Some(anchor) = snapshot_anchor {
                    access_evidence.entry(source_id.clone()).or_insert_with(|| {
                        AgentSourceAccessEvidence {
                            source_id: source_id.clone(),
                            anchor,
                            policy: registered.environment().source_preview(source),
                        }
                    });
                }
                if let AgentAssetSnapshot::Blocked { diagnostic, .. } = &snapshot {
                    run.emit(
                        DiagnosticOwner::Source(source_id.clone()),
                        diagnostic.clone(),
                    );
                }
                if let Some(public_source) = sources.get_mut(&source_id) {
                    merge_source_metadata(public_source, source, &mut run);
                }

                if !run.checkpoint(AgentInventoryStage::Parse) {
                    materialize_source_diagnostics(&source_id, &mut sources, &mut run);
                    parse_complete = false;
                    break;
                }
                #[cfg(test)]
                run.record_event(super::run::InventoryPipelineEvent::Parse {
                    context_id: context.id.clone(),
                    native_source_key: source.native_source_key.clone(),
                });
                let mut parse_output = BoundedParseOutput::new(&mut run, &source_id, source);
                registered.environment().parse(
                    AgentAssetParseRequest {
                        context,
                        source,
                        snapshot: &snapshot,
                        native_home: Some(home),
                        workspace_canonical: workspace,
                        workspace_lexical,
                    },
                    &mut parse_output,
                );
                let source_parse_complete = parse_output.is_complete();
                let hook_parse_complete =
                    parse_output.is_category_complete(AgentAssetCategory::Hook);
                let parse_values = parse_output.finish();
                hook_rule_counts.observe(context, source, &parse_values, hook_parse_complete);
                parse_complete &= source_parse_complete;
                declarations.extend(to_public_declarations(
                    context,
                    &source_id,
                    &snapshot,
                    &parse_values,
                ));
                parsed.extend(parse_values);
                materialize_source_diagnostics(&source_id, &mut sources, &mut run);
                if !run.checkpoint(AgentInventoryStage::Parse) {
                    parse_complete = false;
                    break;
                }
            }

            collect_capabilities(&source_specs, &mut agent_capabilities);

            let resolve_allowed = run.checkpoint(AgentInventoryStage::Resolve);
            if resolve_allowed && parse_complete {
                validate_resolver_inputs(context, &source_specs, &parsed, &mut run);
                let suppressions = registered.environment().definition_suppressions(
                    crate::services::agent_cli::contracts::AgentDefinitionSelectionRequest {
                        context,
                        sources: &source_specs,
                        declarations: &parsed,
                    },
                );
                suppress_selected_definitions(
                    &suppressions,
                    &mut parsed,
                    &mut declarations[public_declaration_start..],
                    &mut run,
                );
            }
            if resolve_allowed && parse_complete {
                #[cfg(test)]
                run.record_event(super::run::InventoryPipelineEvent::Resolve {
                    context_id: context.id.clone(),
                });
                let mut resolve_output =
                    BoundedResolveOutput::new(&mut run, &context.id, parsed.len());
                let resolve_sources = source_specs
                    .iter()
                    .filter_map(|source| {
                        source_state
                            .cached_snapshots
                            .get(&source_queue_key(source))
                            .map(|snapshot| AgentAssetResolveSource {
                                spec: source,
                                snapshot,
                            })
                    })
                    .collect::<Vec<_>>();
                registered.environment().resolve(
                    AgentAssetResolveRequest {
                        context,
                        declarations: &parsed,
                        sources: &resolve_sources,
                    },
                    &mut resolve_output,
                );
                if let Some(drafts) = resolve_output.finish() {
                    if run.checkpoint(AgentInventoryStage::Project) {
                        #[cfg(test)]
                        run.record_event(super::run::InventoryPipelineEvent::Project {
                            context_id: context.id.clone(),
                        });
                        let projection = project_records(
                            &environment,
                            context,
                            &source_specs,
                            &parsed,
                            AgentAssetProjectionInput {
                                drafts,
                                state_assessor: registered.environment().state_assessor(),
                            },
                            &sources,
                            &mut run,
                        );
                        assets.extend(projection.records);
                    }
                }
            }
            // Projection emits declaration-owned suppression diagnostics. Keep
            // parse diagnostics and materialize the complete declaration slice
            // only after projection has finished emitting its diagnostics.
            materialize_declaration_diagnostics(
                &mut declarations[public_declaration_start..],
                &mut run,
            );
        }

        capabilities.push(AgentCapabilities {
            agent_kind: registered.kind,
            assets: materialize_capabilities(agent_capabilities),
        });
        contexts.append(&mut agent_contexts);
    }

    for installation in &mut installations {
        installation.diagnostics =
            run.take_diagnostics_for(DiagnosticOwner::Installation(installation.id.clone()));
    }
    let diagnostics = run.finish_diagnostics();
    contexts.sort_by(|left, right| left.id.cmp(&right.id));
    declarations.sort_by(|left, right| left.id.cmp(&right.id));
    assets.sort_by(|left, right| left.stable_id.cmp(&right.stable_id));
    let mut source_values = sources.into_values().collect::<Vec<_>>();
    source_values.sort_by(|left, right| left.id.cmp(&right.id));
    let mut inventory = AgentEnvironmentInventory {
        environment,
        installations,
        sources: source_values,
        capabilities,
        assets,
        hook_rule_counts: Vec::new(),
        scanned_at: now_string(),
        workspace: workspace.map(|path| path.to_string_lossy().into_owned()),
        contexts,
        declarations,
        limits,
        diagnostics,
        mechanisms,
    };
    inventory.hook_rule_counts = hook_rule_counts.finish(
        &inventory,
        deps.definitions.iter().map(|definition| definition.kind),
    );
    super::mechanism::materialize_actions(&mut inventory, deps.definitions);
    Ok(InventoryBuild {
        inventory,
        access_evidence: access_evidence.into_values().collect(),
    })
}

fn resolve_workspace_trust_authority(
    registered: &super::super::AgentCliDefinition,
    provisional: &AgentConfigurationContext,
    home: &Path,
    workspace: &Path,
    workspace_lexical: Option<&Path>,
    snapshots: &dyn SnapshotPort,
    run: &mut AgentInventoryRun,
) -> (
    AgentTrustState,
    BTreeMap<AuthorityCacheKey, AuthoritySnapshot>,
) {
    let mut state = ContextSourceState::new(run.limits().sources_per_context);
    let readonly_external_roots = registered.environment().readonly_external_roots(&[]);
    {
        let mut output = BoundedSourceOutput::initial(&mut state, run, &provisional.id);
        registered.environment().discover_workspace_trust_sources(
            AgentWorkspaceTrustSourceRequest {
                home,
                workspace,
                config_root: Path::new(&provisional.config_root),
            },
            &mut output,
        );
    }
    let specs = state.eligible_sources_at_depth(0);
    let mut retained = BTreeMap::new();
    for pending in specs {
        if !run.checkpoint(AgentInventoryStage::Snapshot) || !run.reads_open() {
            break;
        }
        let key = source_queue_key(&pending.spec);
        let source_id = source_stable_id(provisional, &pending.spec.path);
        let trusted_roots = trusted_roots(
            home,
            provisional,
            Some(workspace),
            Some(&pending.spec),
            &readonly_external_roots,
        );
        let snapshot = snapshots.snapshot(
            SnapshotRequest {
                source: &pending.spec,
                source_id: &source_id,
                trusted_roots: &trusted_roots,
            },
            run,
        );
        retained.insert(
            (
                provisional.config_root.clone(),
                provisional.profile.clone(),
                provisional.workspace_id.clone(),
                key,
            ),
            AuthoritySnapshot {
                context_id: provisional.id.clone(),
                source_id,
                spec: pending.spec,
                snapshot,
            },
        );
    }
    let source_views = retained
        .values()
        .map(|entry| AgentAssetResolveSource {
            spec: &entry.spec,
            snapshot: &entry.snapshot,
        })
        .collect::<Vec<_>>();
    let trust = registered.environment().resolve_workspace_trust(
        AgentWorkspaceTrustResolveRequest {
            workspace,
            workspace_lexical,
            sources: &source_views,
        },
        run,
    );
    (trust, retained)
}

fn cached_authority_snapshot(
    snapshots: &BTreeMap<AuthorityCacheKey, AuthoritySnapshot>,
    context: &AgentConfigurationContext,
    source: &AgentAssetSourceSpec,
    source_id: &str,
) -> Option<AgentAssetSnapshot> {
    let key = (
        context.config_root.clone(),
        context.profile.clone(),
        context.workspace_id.clone(),
        source_queue_key(source),
    );
    let authority = snapshots.get(&key)?;
    source_specs_compatible(&authority.spec, source)
        .then(|| rebind_snapshot_source_id(&authority.snapshot, source_id))
}

fn rebind_authority_diagnostics(
    context: &AgentConfigurationContext,
    snapshots: &mut BTreeMap<AuthorityCacheKey, AuthoritySnapshot>,
    run: &mut AgentInventoryRun,
) {
    let matching_keys = snapshots
        .iter()
        .filter(|(key, authority)| {
            key.0 == context.config_root
                && key.1 == context.profile
                && key.2 == context.workspace_id
                && authority.context_id != context.id
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in matching_keys {
        let Some(authority) = snapshots.get_mut(&key) else {
            continue;
        };
        let old_context_id = authority.context_id.clone();
        let old_source_id = authority.source_id.clone();
        let final_source_id = source_stable_id(context, &authority.spec.path);
        run.rebind_diagnostics(
            DiagnosticOwner::Source(old_source_id),
            DiagnosticOwner::Source(final_source_id.clone()),
            Some(&final_source_id),
        );
        run.rebind_diagnostics(
            DiagnosticOwner::Context(old_context_id),
            DiagnosticOwner::Context(context.id.clone()),
            None,
        );
        authority.context_id = context.id.clone();
        authority.source_id = final_source_id;
        authority.snapshot = rebind_snapshot_source_id(&authority.snapshot, &authority.source_id);
    }
}

fn rebind_snapshot_source_id(snapshot: &AgentAssetSnapshot, source_id: &str) -> AgentAssetSnapshot {
    match snapshot {
        AgentAssetSnapshot::Blocked {
            revision,
            diagnostic,
        } => AgentAssetSnapshot::Blocked {
            revision: revision.clone(),
            diagnostic: rebind_source_id(diagnostic.clone(), source_id),
        },
        _ => snapshot.clone(),
    }
}

fn source_specs_compatible(left: &AgentAssetSourceSpec, right: &AgentAssetSourceSpec) -> bool {
    left.native_source_key == right.native_source_key
        && left.path == right.path
        && left.allowed_root == right.allowed_root
        && left.path_policy == right.path_policy
        && left.scope == right.scope
        && left.origin == right.origin
        && left.provider == right.provider
        && left.precedence == right.precedence
        && left.allowed_logical_origins == right.allowed_logical_origins
        && left.writable == right.writable
        && left.sensitive == right.sensitive
        && left.source_kind == right.source_kind
        && left.categories == right.categories
}

fn normalize_contexts(
    values: Vec<AgentConfigurationContext>,
    environment: &AgentEnvironmentDescriptor,
    kind: AgentCliKind,
    installation_ids: &[String],
    run: &mut AgentInventoryRun,
) -> Vec<AgentConfigurationContext> {
    let valid_installations = installation_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut contexts = BTreeMap::<String, AgentConfigurationContext>::new();
    for mut context in values {
        let root = Path::new(&context.config_root);
        if !root.is_absolute() {
            run.emit(
                DiagnosticOwner::Inventory,
                AgentAssetDiagnostic::InvalidProjection {
                    projection_key: format!("context:{}:relative-root", kind.key()),
                },
            );
            continue;
        }
        let Some(root) = lexical_absolute(root) else {
            run.emit(
                DiagnosticOwner::Inventory,
                AgentAssetDiagnostic::InvalidProjection {
                    projection_key: format!("context:{}:unsafe-root", kind.key()),
                },
            );
            continue;
        };
        context.environment_id = environment.id.clone();
        context.agent_kind = kind;
        // Preserve the lexical root so the snapshot layer can still detect a
        // symlinked config root. The context ID separately uses canonical
        // identity where safe and available.
        context.config_root = root.to_string_lossy().into_owned();
        let mut compatible = Vec::new();
        for id in context.compatible_installation_ids {
            if valid_installations.contains(&id) {
                compatible.push(id);
            } else {
                run.emit(
                    DiagnosticOwner::Inventory,
                    AgentAssetDiagnostic::InvalidCompatibleInstallation {
                        installation_id: id,
                    },
                );
            }
        }
        compatible.sort();
        compatible.dedup();
        context.compatible_installation_ids = compatible;
        context.id = context_stable_id(environment, kind, &context);
        contexts
            .entry(context.id.clone())
            .and_modify(|current| {
                current
                    .compatible_installation_ids
                    .extend(context.compatible_installation_ids.iter().cloned());
                current.compatible_installation_ids.sort();
                current.compatible_installation_ids.dedup();
                current.schema_facts.extend(context.schema_facts.clone());
            })
            .or_insert(context);
    }
    contexts.into_values().collect()
}

fn merge_source_metadata(
    target: &mut AgentAssetSource,
    source: &AgentAssetSourceSpec,
    run: &mut AgentInventoryRun,
) {
    if Path::new(&target.allowed_root) != source.allowed_root
        || Path::new(&target.path) != source.path
        || target.source_kind != source.source_kind
    {
        run.emit(
            DiagnosticOwner::Source(target.id.clone()),
            AgentAssetDiagnostic::SourceOutsideAllowedRoot {
                source_id: target.id.clone(),
            },
        );
    }
    // Multiple native declarations may observe the same physical file through
    // different entry points. Keep exact origins on Definition provenance and
    // avoid selecting this source-level summary by insertion order.
    if target.origin != source.origin {
        target.origin = crate::models::AgentAssetInstallationOrigin::Unknown;
    }
    target.writable |= source.writable;
    target.sensitive |= source.sensitive;
    target.precedence = target.precedence.max(source.precedence);
    target.categories.extend(source.categories.iter().copied());
    target.categories.sort();
    target.categories.dedup();
}

pub(super) fn materialize_source_diagnostics(
    source_id: &str,
    sources: &mut BTreeMap<String, AgentAssetSource>,
    run: &mut AgentInventoryRun,
) {
    if let Some(source) = sources.get_mut(source_id) {
        source.diagnostics =
            run.take_diagnostics_for(DiagnosticOwner::Source(source_id.to_owned()));
    }
}

fn trusted_roots<'a>(
    home: &'a Path,
    context: &'a AgentConfigurationContext,
    workspace: Option<&'a Path>,
    source: Option<&'a AgentAssetSourceSpec>,
    readonly_external_roots: &'a [PathBuf],
) -> Vec<&'a Path> {
    let mut roots = vec![home, Path::new(&context.config_root)];
    if let Some(workspace) = workspace {
        roots.push(workspace);
    }
    if let Some(source) = source {
        let path = lexical_absolute(&source.path);
        let allowed_root = lexical_absolute(&source.allowed_root);
        let exact_system_file = matches!(
            source.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && !source.writable
            && source.source_kind == AgentAssetSourceKind::File
            && path.as_deref().and_then(Path::parent) == allowed_root.as_deref()
            && allowed_root
                .as_deref()
                .is_some_and(|root| root != Path::new("/"));
        if exact_system_file {
            roots.push(&source.allowed_root);
        }
        if matches!(
            source.scope,
            AgentAssetScope::System | AgentAssetScope::Managed
        ) && !source.writable
        {
            for registered_root in readonly_external_roots {
                let Some(registered) = lexical_absolute(registered_root) else {
                    continue;
                };
                if registered != Path::new("/")
                    && allowed_root
                        .as_ref()
                        .is_some_and(|allowed| allowed.starts_with(&registered))
                    && path
                        .as_ref()
                        .is_some_and(|path| path.starts_with(&registered))
                {
                    roots.push(registered_root);
                }
            }
        }
    }
    roots
}

fn collect_capabilities(
    sources: &[AgentAssetSourceSpec],
    target: &mut BTreeMap<AgentAssetCategory, BTreeSet<AgentAssetScope>>,
) {
    for source in sources {
        for category in &source.categories {
            for origin in &source.allowed_logical_origins {
                target.entry(*category).or_default().insert(origin.scope);
            }
        }
    }
}

fn validate_resolver_inputs(
    context: &AgentConfigurationContext,
    sources: &[AgentAssetSourceSpec],
    declarations: &[ParsedAgentAsset],
    run: &mut AgentInventoryRun,
) {
    if context.id.trim().is_empty() {
        run.emit(
            DiagnosticOwner::Inventory,
            AgentAssetDiagnostic::InvalidProjection {
                projection_key: "context:missing-id".to_owned(),
            },
        );
    }
    for declaration in declarations {
        let valid_source = sources.iter().any(|source| {
            source.native_source_key == declaration.source_key
                && source.allows(declaration.logical_origin)
                && source.categories.contains(&declaration.category)
        });
        if !valid_source
            || declaration.resolution_group_key.trim().is_empty()
            || declaration
                .resolution_group_key
                .chars()
                .any(char::is_control)
        {
            run.emit(
                DiagnosticOwner::Declaration(declaration.declaration_id.clone()),
                AgentAssetDiagnostic::InvalidProjection {
                    projection_key: declaration.declaration_id.clone(),
                },
            );
        }
        if !run.diagnostics().has_regular_capacity() {
            break;
        }
    }
}

fn materialize_declaration_diagnostics(
    declarations: &mut [crate::models::AgentAssetDeclaration],
    run: &mut AgentInventoryRun,
) {
    for declaration in declarations {
        declaration.diagnostics =
            run.take_diagnostics_for(DiagnosticOwner::Declaration(declaration.id.clone()));
    }
}

fn suppress_selected_definitions(
    suppressions: &[crate::services::agent_cli::contracts::AgentDefinitionSuppression],
    parsed: &mut [ParsedAgentAsset],
    public: &mut [crate::models::AgentAssetDeclaration],
    run: &mut AgentInventoryRun,
) {
    use crate::models::{
        AgentAssetDeclarationRole, AgentAssetResolutionParticipation, AgentAssetSuppressionReason,
    };
    let mut selected = BTreeMap::<&str, Vec<AgentAssetSuppressionReason>>::new();
    for suppression in suppressions {
        selected
            .entry(&suppression.declaration_id)
            .or_default()
            .push(suppression.reason);
    }
    for (id, reasons) in selected {
        let mut matches = parsed
            .iter_mut()
            .filter(|declaration| declaration.declaration_id == id);
        let Some(declaration) = matches.next() else {
            continue;
        };
        if reasons.len() != 1
            || !matches!(
                reasons[0],
                AgentAssetSuppressionReason::DuplicatePhysicalSource
                    | AgentAssetSuppressionReason::ParentNotSelected
            )
            || matches.next().is_some()
            || declaration.role != AgentAssetDeclarationRole::Definition
            || declaration.participation != AgentAssetResolutionParticipation::Participates
        {
            run.emit(
                DiagnosticOwner::Declaration(id.to_owned()),
                AgentAssetDiagnostic::InvalidProjection {
                    projection_key: "definition-selection".to_owned(),
                },
            );
            continue;
        }
        declaration.participation =
            AgentAssetResolutionParticipation::Suppressed { reason: reasons[0] };
        for item in public.iter_mut().filter(|item| item.id == id) {
            item.participation = declaration.participation;
        }
    }
}

fn materialize_capabilities(
    values: BTreeMap<AgentAssetCategory, BTreeSet<AgentAssetScope>>,
) -> Vec<AgentAssetCapability> {
    values
        .into_iter()
        .map(|(category, scopes)| AgentAssetCapability {
            category,
            discovery: scopes.into_iter().collect(),
            mutation: AgentAssetMutation::ReadOnly,
            requires_restart: false,
            requires_trust: false,
        })
        .collect()
}

fn to_public_declarations(
    context: &AgentConfigurationContext,
    source_id: &str,
    snapshot: &AgentAssetSnapshot,
    parsed: &[ParsedAgentAsset],
) -> Vec<crate::models::AgentAssetDeclaration> {
    let revision = snapshot_revision_of(snapshot);
    parsed
        .iter()
        .map(|value| crate::models::AgentAssetDeclaration {
            id: value.declaration_id.clone(),
            context_id: context.id.clone(),
            source_id: source_id.to_string(),
            scope: value.logical_origin.scope,
            native_kind: value.category,
            native_id: value.native_id.clone(),
            declaration_key: value.declaration_key.clone(),
            label: value.label.clone(),
            precedence: value.logical_origin.precedence,
            presence: value.presence,
            declared_state: value.declared_state,
            trust_state: value.trust_state,
            role: value.role,
            participation: value.participation,
            evidence: AgentAssetEvidence {
                revision: revision.clone(),
                parser_version: context.parser_version,
                observed_at: now_string(),
                facts: value.facts.clone(),
            },
            diagnostics: Vec::new(),
            provided_by: value.provided_by.clone(),
            action_owner: value.action_owner.clone(),
            explicitly_affected: value.explicitly_affected.clone(),
        })
        .collect()
}

pub(crate) fn normalize_optional_workspace(
    workspace: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    workspace.map(normalize_workspace).transpose()
}

fn normalize_workspace(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("工作区路径必须是绝对路径".to_string());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("工作区不存在: {error}"))?;
    if metadata.file_type().is_symlink() || path_has_symlink_component(path) {
        return Err("出于安全原因不支持符号链接工作区".to_string());
    }
    if !metadata.is_dir() {
        return Err("工作区路径不是目录".to_string());
    }
    fs::canonicalize(path).map_err(|error| format!("无法解析工作区路径: {error}"))
}

pub(crate) fn native_environment() -> AgentEnvironmentDescriptor {
    let platform = native_host_platform();
    let label = match platform {
        AgentHostPlatform::Macos => "macOS",
        AgentHostPlatform::Linux => "Linux",
        AgentHostPlatform::Windows => "Windows",
    };
    AgentEnvironmentDescriptor {
        id: format!("native:{}", env::consts::OS),
        kind: AgentEnvironmentKind::Native,
        host_platform: platform,
        host_architecture: AgentHostArchitecture::current(),
        guest_platform: None,
        display_name: format!("本机 ({label})"),
        capabilities: vec![
            AgentEnvironmentCapability::ReadOnlyInventory,
            AgentEnvironmentCapability::BoundedPreview,
        ],
    }
}

fn native_host_platform() -> AgentHostPlatform {
    #[cfg(target_os = "macos")]
    {
        AgentHostPlatform::Macos
    }
    #[cfg(target_os = "linux")]
    {
        AgentHostPlatform::Linux
    }
    #[cfg(target_os = "windows")]
    {
        AgentHostPlatform::Windows
    }
}

/// Native configuration and authentication reuse the inventory's context/trust
/// identity without scanning every resource package or requiring a CLI.
pub(crate) struct ConfigurationContexts {
    pub(crate) environment: AgentEnvironmentDescriptor,
    pub(crate) contexts: Vec<AgentConfigurationContext>,
    pub(crate) installations: Vec<AgentInstallation>,
}

/// Installation and configuration-root identities only; never enumerates assets.
pub(crate) fn installation_contexts(
    home: &Path,
    settings: &AppSettings,
) -> Result<ConfigurationContexts, String> {
    let environment = native_environment();
    let mut run = AgentInventoryRun::with_clock(
        AgentAssetLimits {
            refresh_budget_ms: 10_000,
            ..production_limits()
        },
        Arc::new(SystemMonotonicClock),
    );
    let snapshots = RealSnapshotPort::default();
    let mut all = ConfigurationContexts {
        environment: environment.clone(),
        installations: Vec::new(),
        contexts: Vec::new(),
    };
    for definition in definitions() {
        let installations = RealInstallationDiscoveryPort.discover(
            InstallationDiscoveryRequest {
                preferred_path: settings.agent_cli_path(definition.kind),
                definition,
                include_shell: false,
            },
            &mut run,
        );
        let (contexts, _) = discover_normalized_contexts(
            definition,
            &environment,
            home,
            (None, None),
            &installations,
            &snapshots,
            &mut run,
        );
        all.installations.extend(installations);
        all.contexts.extend(contexts);
    }
    if !run.checkpoint(AgentInventoryStage::ContextDiscovery) {
        return Err("安装检测超时，请重新检测".to_owned());
    }
    Ok(all)
}

pub(crate) fn configuration_contexts(
    home: &Path,
    workspace: Option<&Path>,
    workspace_lexical: Option<&Path>,
    kind: AgentCliKind,
    settings: Option<&crate::models::AppSettings>,
) -> ConfigurationContexts {
    configuration_contexts_with_snapshots(
        home,
        workspace,
        workspace_lexical,
        kind,
        settings,
        &RealSnapshotPort::default(),
    )
}

fn configuration_contexts_with_snapshots(
    home: &Path,
    workspace: Option<&Path>,
    workspace_lexical: Option<&Path>,
    kind: AgentCliKind,
    settings: Option<&crate::models::AppSettings>,
    snapshots: &dyn SnapshotPort,
) -> ConfigurationContexts {
    let registered = super::super::definition(kind);
    let environment = native_environment();
    let mut run = AgentInventoryRun::with_clock(
        AgentAssetLimits {
            refresh_budget_ms: 10_000,
            ..production_limits()
        },
        Arc::new(SystemMonotonicClock),
    );
    let installations = settings
        .map(|settings| {
            RealInstallationDiscoveryPort.discover(
                InstallationDiscoveryRequest {
                    preferred_path: settings.agent_cli_path(kind),
                    definition: registered,
                    include_shell: false,
                },
                &mut run,
            )
        })
        .unwrap_or_default();
    let (contexts, _) = discover_normalized_contexts(
        registered,
        &environment,
        home,
        (workspace, workspace_lexical),
        &installations,
        snapshots,
        &mut run,
    );
    ConfigurationContexts {
        environment,
        contexts,
        installations,
    }
}

fn discover_normalized_contexts(
    registered: &super::super::AgentCliDefinition,
    environment: &AgentEnvironmentDescriptor,
    home: &Path,
    workspaces: (Option<&Path>, Option<&Path>),
    installations: &[AgentInstallation],
    snapshots: &dyn SnapshotPort,
    run: &mut AgentInventoryRun,
) -> (
    Vec<AgentConfigurationContext>,
    BTreeMap<AuthorityCacheKey, AuthoritySnapshot>,
) {
    let (workspace, workspace_lexical) = workspaces;
    let installation_ids = installations
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut context_values = registered.environment().discover_contexts(
        AgentContextDiscoveryRequest {
            environment_id: &environment.id,
            agent_kind: registered.kind,
            home,
            workspace,
            installations,
            workspace_trust: AgentTrustState::Unknown,
        },
        run,
    );
    let mut authority_snapshots = BTreeMap::<AuthorityCacheKey, AuthoritySnapshot>::new();
    if registered.environment().has_workspace_trust_authority() {
        if let Some(workspace) = workspace {
            let mut authority_contexts = context_values.clone();
            for provisional in &mut authority_contexts {
                // Trust is part of the public context identity, so the
                // authority read starts from a unique provisional ID and
                // is rebound after the authoritative trust result arrives.
                if provisional.id.trim().is_empty() {
                    provisional.id = context_stable_id(environment, registered.kind, provisional);
                }
                let (trust, snapshots) = resolve_workspace_trust_authority(
                    registered,
                    provisional,
                    home,
                    workspace,
                    workspace_lexical,
                    snapshots,
                    run,
                );
                provisional.trust_context = trust;
                authority_snapshots.extend(snapshots);
            }
            for context in &mut context_values {
                context.trust_context = authority_contexts
                    .iter()
                    .find(|candidate| {
                        candidate.config_root == context.config_root
                            && candidate.workspace_id == context.workspace_id
                            && candidate.profile == context.profile
                    })
                    .map(|candidate| candidate.trust_context)
                    .unwrap_or(AgentTrustState::Unknown);
            }
        }
    }
    let contexts = normalize_contexts(
        context_values,
        environment,
        registered.kind,
        &installation_ids,
        run,
    );
    (contexts, authority_snapshots)
}

pub(crate) fn configuration_source_allowed(
    home: &Path,
    context: &AgentConfigurationContext,
    workspace: Option<&Path>,
    source: &AgentAssetSourceSpec,
    installations: &[AgentInstallation],
) -> bool {
    if source.source_kind != AgentAssetSourceKind::File
        || !matches!(source.path_policy, AgentAssetSourcePathPolicy::NoFollow)
    {
        return false;
    }
    let external = super::super::definition(context.agent_kind)
        .environment()
        .readonly_external_roots(installations);
    let roots = trusted_roots(home, context, workspace, Some(source), &external);
    let Some(root) = lexical_absolute(&source.allowed_root) else {
        return false;
    };
    let Some(path) = lexical_absolute(&source.path) else {
        return false;
    };
    path.starts_with(&root) && roots.iter().any(|trusted| root.starts_with(trusted))
}

/// Test fixture port changes only external system locations; native discovery,
/// trust parsing, source IDs and real guarded reads remain in the shared path.
#[cfg(test)]
mod tests {
    use super::*;

    fn source_spec() -> AgentAssetSourceSpec {
        AgentAssetSourceSpec {
            hook_definition_source: true,
            verified_physical_path: None,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: "authority".to_owned(),
            label: "authority".to_owned(),
            scope: AgentAssetScope::User,
            path: PathBuf::from("/tmp/authority.json"),
            allowed_root: PathBuf::from("/tmp"),
            precedence: 10,
            writable: true,
            sensitive: true,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![AgentAssetCategory::Mcp],
            allowed_logical_origins: vec![AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 10,
            }],
        }
    }

    #[test]
    fn authority_source_compatibility_includes_physical_owner_fields() {
        let source = source_spec();
        assert!(source_specs_compatible(&source, &source));

        let mut changed = source.clone();
        changed.scope = AgentAssetScope::Local;
        assert!(!source_specs_compatible(&source, &changed));

        let mut changed = source.clone();
        changed.precedence = 30;
        assert!(!source_specs_compatible(&source, &changed));

        let mut changed = source.clone();
        changed.allowed_logical_origins[0].precedence = 30;
        assert!(!source_specs_compatible(&source, &changed));
    }

    #[test]
    fn registered_external_directory_is_bounded_read_only_and_no_follow() {
        use super::super::verified_path::inspect_verified_path;
        let root = env::temp_dir().join(format!(
            "bh-external-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let skills = root.join("system/skills");
        fs::create_dir_all(skills.join("child")).unwrap();
        fs::write(skills.join("child/SKILL.md"), "fixture").unwrap();
        let root = root.canonicalize().unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let skills = root.join("system/skills");
        let home = root.join("home");
        let context = AgentConfigurationContext {
            id: "external-roots".to_owned(),
            environment_id: "native:test".to_owned(),
            agent_kind: AgentCliKind::Codex,
            config_root: home.join("config").to_string_lossy().into_owned(),
            profile: "default".to_owned(),
            workspace_id: None,
            trust_context: AgentTrustState::Unknown,
            parser_version: 1,
            schema_facts: BTreeMap::new(),
            compatible_installation_ids: Vec::new(),
        };
        let registered = vec![skills.clone()];
        let mut source = source_spec();
        source.scope = AgentAssetScope::Managed;
        source.writable = false;
        source.path = skills.clone();
        source.allowed_root = skills.clone();
        source.source_kind = AgentAssetSourceKind::Directory;
        let check = |source: &AgentAssetSourceSpec, registered: &[PathBuf]| {
            let roots = trusted_roots(&home, &context, None, Some(source), registered);
            inspect_verified_path(
                &roots,
                &source.allowed_root,
                &source.path,
                source.source_kind,
            )
            .is_ok()
        };
        assert!(!check(&source, &[]));
        assert!(check(&source, &registered));
        let mut child = source.clone();
        child.path = skills.join("child/SKILL.md");
        child.source_kind = AgentAssetSourceKind::File;
        assert!(check(&child, &registered));
        let mut wider = child.clone();
        wider.allowed_root = root.join("system");
        assert!(!check(&wider, &registered));
        let mut writable = source.clone();
        writable.writable = true;
        assert!(!check(&writable, &registered));
        let mut user = source.clone();
        user.scope = AgentAssetScope::User;
        assert!(!check(&user, &registered));
        let mut sibling = source.clone();
        sibling.path = root.join("system/sibling");
        sibling.allowed_root = sibling.path.clone();
        fs::create_dir_all(&sibling.path).unwrap();
        assert!(!check(&sibling, &registered));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(skills.join("child"), skills.join("linked")).unwrap();
            child.path = skills.join("linked/SKILL.md");
            assert!(!check(&child, &registered));
        }
    }
}
