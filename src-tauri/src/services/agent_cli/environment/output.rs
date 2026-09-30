//! Incrementally bounded outputs exposed to Agent-owned adapters.

use super::{
    diagnostics::DiagnosticOwner,
    identity::lexical_identity,
    run::{AgentInventoryRun, AgentInventoryStage},
};
use crate::{
    models::{
        AgentAssetCategory, AgentAssetDiagnostic, AgentAssetLimitKind, AgentAssetRevision,
        AgentAssetScope, AgentAssetSourceKind,
    },
    services::agent_cli::contracts::{
        AgentAssetDirectoryEntry, AgentAssetProjectedDraft, AgentAssetSnapshot,
        AgentAssetSourcePathPolicy, AgentAssetSourceSpec, AgentDiagnosticEmission,
        AgentDiagnosticOutput, AgentFollowUpSourceSpec, AgentFollowUpSourceTarget, AgentOutputStop,
        AgentParseOutput, AgentResolveOutput, FollowUpSourceOutput, InitialSourceOutput,
        ParsedAgentAsset,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
};

pub(super) type SourceOrderKey = (String, String, u8, String);
pub(super) type PhysicalSourceKey = (String, String, u8);
pub(super) const MAX_FOLLOW_UP_DEPTH: u8 = 2;

#[derive(Debug, Clone)]
pub(super) struct PendingSource {
    pub spec: AgentAssetSourceSpec,
    pub depth: u8,
}

pub(super) struct ContextSourceState {
    limit: usize,
    raw_offers: usize,
    offers: BTreeMap<SourceOrderKey, PendingSource>,
    duplicate_order_keys: BTreeSet<SourceOrderKey>,
    pub(super) cached_snapshots: BTreeMap<PhysicalSourceKey, AgentAssetSnapshot>,
    closed: bool,
    diagnostics_finalized: bool,
}

impl ContextSourceState {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            raw_offers: 0,
            offers: BTreeMap::new(),
            duplicate_order_keys: BTreeSet::new(),
            cached_snapshots: BTreeMap::new(),
            closed: false,
            diagnostics_finalized: false,
        }
    }

    pub(super) fn retained_sources(&self) -> Vec<PendingSource> {
        let mut native_counts = BTreeMap::<String, usize>::new();
        let mut physical_counts = BTreeMap::<PhysicalSourceKey, usize>::new();
        for (order_key, pending) in &self.offers {
            *native_counts
                .entry(pending.spec.native_source_key.clone())
                .or_default() += 1;
            *physical_counts
                .entry(source_keys(&pending.spec).1)
                .or_default() += 1;
            if self.duplicate_order_keys.contains(order_key) {
                *native_counts
                    .entry(pending.spec.native_source_key.clone())
                    .or_default() += 1;
            }
        }
        self.offers
            .iter()
            .filter(|(order_key, pending)| {
                !self.duplicate_order_keys.contains(*order_key)
                    && native_counts
                        .get(&pending.spec.native_source_key)
                        .is_some_and(|count| *count == 1)
                    && physical_counts
                        .get(&source_keys(&pending.spec).1)
                        .is_some_and(|count| *count == 1)
            })
            .take(self.limit)
            .map(|(_, pending)| pending.clone())
            .collect()
    }

    /// Return only the deterministic, collision-free top-K view. Provisional
    /// offers outside this view must never be snapshotted or expanded.
    pub(super) fn eligible_sources_at_depth(&self, depth: u8) -> Vec<PendingSource> {
        self.retained_sources()
            .into_iter()
            .filter(|pending| pending.depth == depth)
            .collect()
    }

    pub(super) fn is_eligible(&self, pending: &PendingSource) -> bool {
        let candidate_key = source_keys(&pending.spec).0;
        self.retained_sources().into_iter().any(|retained| {
            retained.depth == pending.depth && source_keys(&retained.spec).0 == candidate_key
        })
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed
    }

    fn conflicts_with_shallower_source(&self, candidate: &AgentAssetSourceSpec, depth: u8) -> bool {
        let candidate_physical = source_keys(candidate).1;
        self.offers.values().any(|pending| {
            pending.depth < depth
                && (pending.spec.native_source_key == candidate.native_source_key
                    || source_keys(&pending.spec).1 == candidate_physical)
        })
    }

    pub(super) fn close(&mut self) {
        self.closed = true;
    }

    pub(super) fn cache_snapshot(&mut self, key: PhysicalSourceKey, snapshot: AgentAssetSnapshot) {
        self.cached_snapshots.insert(key, snapshot);
    }

    pub(super) fn take_snapshot(&mut self, key: &PhysicalSourceKey) -> Option<AgentAssetSnapshot> {
        self.cached_snapshots.remove(key)
    }

    pub(super) fn restore_snapshot(
        &mut self,
        key: PhysicalSourceKey,
        snapshot: AgentAssetSnapshot,
    ) {
        self.cached_snapshots.insert(key, snapshot);
    }

    pub(super) fn finish(
        &mut self,
        run: &mut AgentInventoryRun,
        context_id: &str,
    ) -> Vec<AgentAssetSourceSpec> {
        self.close();
        if !self.diagnostics_finalized {
            let retained = self.retained_sources();
            let mut native_counts = BTreeMap::<String, usize>::new();
            let mut physical_counts = BTreeMap::<PhysicalSourceKey, usize>::new();
            for pending in self.offers.values() {
                *native_counts
                    .entry(pending.spec.native_source_key.clone())
                    .or_default() += 1;
                *physical_counts
                    .entry(source_keys(&pending.spec).1)
                    .or_default() += 1;
            }
            let mut collision_ids = BTreeSet::new();
            for pending in self.offers.values() {
                if native_counts
                    .get(&pending.spec.native_source_key)
                    .is_some_and(|count| *count > 1)
                    || physical_counts
                        .get(&source_keys(&pending.spec).1)
                        .is_some_and(|count| *count > 1)
                {
                    collision_ids.insert(pending.spec.native_source_key.clone());
                }
            }
            for order_key in &self.duplicate_order_keys {
                if let Some(pending) = self.offers.get(order_key) {
                    collision_ids.insert(pending.spec.native_source_key.clone());
                }
            }
            for native_source_key in collision_ids {
                run.emit(
                    DiagnosticOwner::Context(context_id.to_string()),
                    AgentAssetDiagnostic::InvalidProjection {
                        projection_key: format!("source:{native_source_key}"),
                    },
                );
            }
            if self.raw_offers > self.limit {
                run.emit(
                    DiagnosticOwner::Context(context_id.to_string()),
                    AgentAssetDiagnostic::Truncated {
                        limit: AgentAssetLimitKind::SourcesPerContext,
                        accepted: retained.len() as u64,
                        observed_at_least: self.raw_offers as u64,
                    },
                );
            }
            self.diagnostics_finalized = true;
        }
        self.retained_sources()
            .into_iter()
            .map(|pending| pending.spec)
            .collect()
    }
}

fn source_keys(source: &AgentAssetSourceSpec) -> (SourceOrderKey, PhysicalSourceKey) {
    let kind = match source.source_kind {
        AgentAssetSourceKind::File => 0,
        AgentAssetSourceKind::Directory => 1,
    };
    let physical = (
        lexical_identity(&source.path),
        lexical_identity(&source.allowed_root),
        kind,
    );
    let order = (
        physical.0.clone(),
        physical.1.clone(),
        physical.2,
        source.native_source_key.clone(),
    );
    (order, physical)
}

pub(super) fn source_queue_key(source: &AgentAssetSourceSpec) -> PhysicalSourceKey {
    source_keys(source).1
}

pub(super) struct BoundedSourceOutput<'a, 'b, 'c> {
    state: &'a mut ContextSourceState,
    run: &'b mut AgentInventoryRun,
    owner: DiagnosticOwner,
    stage: AgentInventoryStage,
    parent: Option<FollowUpParentView<'c>>,
    snapshot_reader: Option<&'c mut DiscoverySnapshotReader<'c>>,
}

type DiscoverySnapshotReader<'a> =
    dyn FnMut(&AgentAssetSourceSpec, &mut AgentInventoryRun) -> AgentAssetSnapshot + 'a;

struct FollowUpParentView<'a> {
    parent: &'a AgentAssetSourceSpec,
    manifest: &'a [AgentAssetDirectoryEntry],
    depth: u8,
    manifest_revision: Option<&'a AgentAssetRevision>,
}

impl<'a, 'b> BoundedSourceOutput<'a, 'b, 'static> {
    pub(super) fn initial(
        state: &'a mut ContextSourceState,
        run: &'b mut AgentInventoryRun,
        context_id: &str,
    ) -> Self {
        Self {
            state,
            run,
            owner: DiagnosticOwner::Context(context_id.to_string()),
            stage: AgentInventoryStage::InitialSourceDiscovery,
            parent: None,
            snapshot_reader: None,
        }
    }

    pub(super) fn with_snapshot_reader<'c>(
        self,
        reader: &'c mut DiscoverySnapshotReader<'c>,
    ) -> BoundedSourceOutput<'a, 'b, 'c> {
        BoundedSourceOutput {
            state: self.state,
            run: self.run,
            owner: self.owner,
            stage: self.stage,
            parent: None,
            snapshot_reader: Some(reader),
        }
    }
}

impl<'a, 'b, 'c> BoundedSourceOutput<'a, 'b, 'c> {
    pub(super) fn follow_up(
        state: &'a mut ContextSourceState,
        run: &'b mut AgentInventoryRun,
        context_id: &str,
        parent: &'c AgentAssetSourceSpec,
        manifest: &'c [AgentAssetDirectoryEntry],
        depth: u8,
    ) -> Self {
        Self {
            state,
            run,
            owner: DiagnosticOwner::Context(context_id.to_string()),
            stage: AgentInventoryStage::FollowUpSourceDiscovery,
            parent: Some(FollowUpParentView {
                parent,
                manifest,
                depth,
                manifest_revision: None,
            }),
            snapshot_reader: None,
        }
    }

    pub(super) fn with_manifest_revision(mut self, revision: &'c AgentAssetRevision) -> Self {
        if let Some(parent) = &mut self.parent {
            parent.manifest_revision = Some(revision);
        }
        self
    }

    fn begin_raw_offer(&mut self) -> Result<bool, ControlFlow<AgentOutputStop>> {
        if self.state.closed {
            return Err(ControlFlow::Break(AgentOutputStop::SourceLimit));
        }
        if !self.run.checkpoint(self.stage) {
            self.state.closed = true;
            return Err(ControlFlow::Break(AgentOutputStop::Deadline));
        }
        self.state.raw_offers = self.state.raw_offers.saturating_add(1);
        Ok(self.state.raw_offers > self.state.limit)
    }

    fn finish_raw_offer(&mut self, over_limit: bool) -> ControlFlow<AgentOutputStop> {
        if over_limit {
            self.state.closed = true;
            ControlFlow::Break(AgentOutputStop::SourceLimit)
        } else {
            ControlFlow::Continue(())
        }
    }

    fn emit_spec_after_gate(
        &mut self,
        mut value: AgentAssetSourceSpec,
        depth: u8,
        over_limit: bool,
    ) -> ControlFlow<AgentOutputStop> {
        let (order_key, _) = source_keys(&value);
        let origins_valid = value.normalize_logical_origins();
        if value.native_source_key.trim().is_empty()
            || !value.path.is_absolute()
            || !value.allowed_root.is_absolute()
            || !origins_valid
        {
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: format!("source:{}", value.native_source_key),
            });
        } else if depth > 0 && self.state.conflicts_with_shallower_source(&value, depth) {
            // A child is discovered only after its parent level has been
            // admitted. It cannot retroactively invalidate a shallower source
            // that may already have paid for a snapshot. Same-depth collisions
            // continue through the ordinary order-independent fail-closed path.
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: format!("source:{}", value.native_source_key),
            });
        } else if self
            .state
            .offers
            .insert(order_key.clone(), PendingSource { spec: value, depth })
            .is_some()
        {
            self.state.duplicate_order_keys.insert(order_key);
        }
        self.finish_raw_offer(over_limit)
    }
}

impl AgentDiagnosticOutput for BoundedSourceOutput<'_, '_, '_> {
    fn has_regular_capacity(&self) -> bool {
        self.run.diagnostics().has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.run.emit(self.owner.clone(), value)
    }
}

impl InitialSourceOutput for BoundedSourceOutput<'_, '_, '_> {
    fn emit_initial(&mut self, value: AgentAssetSourceSpec) -> ControlFlow<AgentOutputStop> {
        let over_limit = match self.begin_raw_offer() {
            Ok(over_limit) => over_limit,
            Err(stop) => return stop,
        };
        if matches!(
            value.path_policy,
            AgentAssetSourcePathPolicy::ReadonlySkillLink { .. }
        ) {
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: format!(
                    "source:{}:reference-without-manifest",
                    value.native_source_key
                ),
            });
            return self.finish_raw_offer(over_limit);
        }
        self.emit_spec_after_gate(value, 0, over_limit)
    }

    fn snapshot_initial(
        &mut self,
        source: AgentAssetSourceSpec,
    ) -> ControlFlow<AgentOutputStop, Option<AgentAssetSnapshot>> {
        self.emit_initial(source.clone())?;
        let pending = PendingSource {
            spec: source.clone(),
            depth: 0,
        };
        if !self.state.is_eligible(&pending) {
            return ControlFlow::Continue(None);
        }
        let key = source_queue_key(&source);
        if let Some(snapshot) = self.state.cached_snapshots.get(&key) {
            return ControlFlow::Continue(Some(snapshot.clone()));
        }
        if !self.run.checkpoint(AgentInventoryStage::Snapshot) {
            self.state.close();
            return ControlFlow::Break(AgentOutputStop::Deadline);
        }
        if !self.run.reads_open() {
            self.run.report_closed_read(self.owner.clone());
            return ControlFlow::Continue(None);
        }
        let Some(reader) = self.snapshot_reader.as_mut() else {
            return ControlFlow::Continue(None);
        };
        let snapshot = reader(&source, self.run);
        self.state.cache_snapshot(key, snapshot.clone());
        ControlFlow::Continue(Some(snapshot))
    }
}

impl FollowUpSourceOutput for BoundedSourceOutput<'_, '_, '_> {
    fn emit_follow_up(&mut self, value: AgentFollowUpSourceSpec) -> ControlFlow<AgentOutputStop> {
        let over_limit = match self.begin_raw_offer() {
            Ok(over_limit) => over_limit,
            Err(stop) => return stop,
        };
        let Some(view) = self.parent.as_ref() else {
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: "follow-up:without-parent".to_owned(),
            });
            return self.finish_raw_offer(over_limit);
        };
        materialize_follow_up(
            self,
            value,
            view.parent,
            view.manifest,
            view.depth,
            view.manifest_revision,
            over_limit,
        )
    }
}

fn materialize_follow_up(
    sink: &mut BoundedSourceOutput<'_, '_, '_>,
    value: AgentFollowUpSourceSpec,
    parent: &AgentAssetSourceSpec,
    manifest: &[AgentAssetDirectoryEntry],
    parent_depth: u8,
    manifest_revision: Option<&AgentAssetRevision>,
    over_limit: bool,
) -> ControlFlow<AgentOutputStop> {
    let (entry_name, relative_path) = match &value.target {
        AgentFollowUpSourceTarget::ManifestFile { entry_name } => (entry_name, None),
        AgentFollowUpSourceTarget::Descendant {
            directory_entry_name,
            relative_path,
        } => (directory_entry_name, Some(relative_path)),
        AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink {
            directory_entry_name,
        } => (directory_entry_name, None),
    };
    let projection_key = format!("follow-up:{}:{}", value.parent_source_key, entry_name);
    if value.parent_source_key != parent.native_source_key
        || parent.source_kind != AgentAssetSourceKind::Directory
        || parent_depth >= MAX_FOLLOW_UP_DEPTH
    {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    }
    if !safe_manifest_entry_name(entry_name) {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    }
    let mut entries = manifest.iter().filter(|entry| entry.name == *entry_name);
    let Some(entry) = entries.next() else {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    };
    if entries.next().is_some() {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    }
    let readonly_link = matches!(
        value.target,
        AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink { .. }
    );
    let path_policy = if readonly_link {
        let (AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot { shared_root }, Some(revision)) =
            (&parent.path_policy, manifest_revision)
        else {
            sink.emit_diagnostic(AgentAssetDiagnostic::SymlinkRejected {
                source_id: parent.native_source_key.clone(),
            });
            return sink.finish_raw_offer(over_limit);
        };
        if !entry.is_symlink
            || value.source_kind != AgentAssetSourceKind::File
            || !matches!(
                parent.scope,
                AgentAssetScope::User | AgentAssetScope::Workspace
            )
            || value.scope != parent.scope
            || value.precedence != parent.precedence
            || parent.categories != [AgentAssetCategory::Skill]
            || value.categories != [AgentAssetCategory::Skill]
            || !revision.is_directory
            || revision.is_missing
            || revision.is_symlink
        {
            sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
            return sink.finish_raw_offer(over_limit);
        }
        AgentAssetSourcePathPolicy::ReadonlySkillLink {
            shared_root: shared_root.clone(),
            manifest_path: parent.path.clone(),
            manifest_revision: revision.clone(),
            entry_name: entry_name.clone(),
        }
    } else {
        AgentAssetSourcePathPolicy::NoFollow
    };
    if entry.is_symlink && !readonly_link {
        sink.emit_diagnostic(AgentAssetDiagnostic::SymlinkRejected {
            source_id: parent.native_source_key.clone(),
        });
        return sink.finish_raw_offer(over_limit);
    }
    if matches!(&value.target, AgentFollowUpSourceTarget::Descendant { .. })
        && relative_path.is_some_and(|path| !safe_relative_path(path))
    {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    }
    let path = match (&value.target, entry.source_kind, relative_path) {
        (AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink { .. }, _, None) => {
            parent.path.join(entry_name).join("SKILL.md")
        }
        (AgentFollowUpSourceTarget::ManifestFile { .. }, AgentAssetSourceKind::File, None)
            if value.source_kind == AgentAssetSourceKind::File =>
        {
            parent.path.join(entry_name)
        }
        (AgentFollowUpSourceTarget::ManifestFile { .. }, AgentAssetSourceKind::File, None) => {
            sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
            return sink.finish_raw_offer(over_limit);
        }
        (AgentFollowUpSourceTarget::ManifestFile { .. }, actual, None) => {
            sink.emit_diagnostic(AgentAssetDiagnostic::SourceTypeMismatch {
                source_id: parent.native_source_key.clone(),
                expected: AgentAssetSourceKind::File,
                actual,
            });
            return sink.finish_raw_offer(over_limit);
        }
        (
            AgentFollowUpSourceTarget::Descendant { .. },
            AgentAssetSourceKind::Directory,
            Some(relative_path),
        ) => parent.path.join(entry_name).join(relative_path),
        (AgentFollowUpSourceTarget::Descendant { .. }, actual, Some(_)) => {
            sink.emit_diagnostic(AgentAssetDiagnostic::SourceTypeMismatch {
                source_id: parent.native_source_key.clone(),
                expected: AgentAssetSourceKind::Directory,
                actual,
            });
            return sink.finish_raw_offer(over_limit);
        }
        _ => {
            sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
            return sink.finish_raw_offer(over_limit);
        }
    };
    if value.native_source_key.trim().is_empty() {
        sink.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection { projection_key });
        return sink.finish_raw_offer(over_limit);
    }
    let writable = !readonly_link
        && matches!(
            value.scope,
            crate::models::AgentAssetScope::User
                | crate::models::AgentAssetScope::Workspace
                | crate::models::AgentAssetScope::Local
        );
    sink.emit_spec_after_gate(
        AgentAssetSourceSpec {
            path_policy,
            verified_physical_path: None,
            native_source_key: value.native_source_key,
            label: value.label,
            scope: value.scope,
            origin: if readonly_link {
                crate::models::AgentAssetInstallationOrigin::Linked
            } else {
                parent.origin
            },
            provider: parent.provider,
            path,
            allowed_root: parent.allowed_root.clone(),
            precedence: value.precedence,
            writable,
            sensitive: value.sensitive,
            source_kind: value.source_kind,
            hook_definition_source: value
                .categories
                .contains(&crate::models::AgentAssetCategory::Hook),
            categories: value.categories,
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: value.scope,
                    precedence: value.precedence,
                },
            ],
        },
        parent_depth + 1,
        over_limit,
    )
}

fn safe_relative_path(path: &std::path::Path) -> bool {
    let raw = path.to_string_lossy();
    if raw.len() >= 2 && raw.as_bytes()[0].is_ascii_alphabetic() && raw.as_bytes()[1] == b':' {
        return false;
    }
    let mut has_component = false;
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                let value = value.to_string_lossy();
                if value.is_empty()
                    || value == "."
                    || value == ".."
                    || value.contains(['/', '\\', '\0'])
                    || value.chars().any(char::is_control)
                {
                    return false;
                }
                has_component = true;
            }
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return false,
        }
    }
    has_component
}

fn safe_manifest_entry_name(name: &str) -> bool {
    let invalid = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
        || (name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && name.as_bytes().get(1) == Some(&b':'));
    !invalid
        && matches!(
            {
                let mut components = std::path::Path::new(name).components();
                (components.next(), components.next())
            },
            (Some(std::path::Component::Normal(_)), None)
        )
}

pub(super) struct BoundedParseOutput<'a> {
    run: &'a mut AgentInventoryRun,
    owner: DiagnosticOwner,
    source_key: String,
    allowed_logical_origins: Vec<crate::services::agent_cli::contracts::AgentAssetLogicalOrigin>,
    categories: Vec<crate::models::AgentAssetCategory>,
    raw_offers: usize,
    limit: usize,
    declarations: BTreeMap<String, ParsedAgentAsset>,
    duplicate_ids: BTreeSet<String>,
    rejected_categories: BTreeSet<crate::models::AgentAssetCategory>,
    stopped: bool,
    complete: bool,
}

impl<'a> BoundedParseOutput<'a> {
    pub(super) fn new(
        run: &'a mut AgentInventoryRun,
        source_id: &str,
        source: &AgentAssetSourceSpec,
    ) -> Self {
        let limit = run.limits().first_level_entries;
        Self {
            run,
            owner: DiagnosticOwner::Source(source_id.to_string()),
            source_key: source.native_source_key.clone(),
            allowed_logical_origins: source.allowed_logical_origins.clone(),
            categories: source.categories.clone(),
            raw_offers: 0,
            limit,
            declarations: BTreeMap::new(),
            duplicate_ids: BTreeSet::new(),
            rejected_categories: BTreeSet::new(),
            stopped: false,
            complete: true,
        }
    }

    pub(super) fn finish(mut self) -> Vec<ParsedAgentAsset> {
        let rejected = self.duplicate_ids;
        self.declarations
            .retain(|declaration_id, _| !rejected.contains(declaration_id));
        self.declarations.into_values().take(self.limit).collect()
    }

    /// A bounded parse is not authoritative after a deadline or entry-limit
    /// stop. The common pipeline reads this private bit before consuming the
    /// sink, while retaining the deterministic declaration prefix for display.
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }

    /// Rejected declarations affect their own category. An invalid MCP entry
    /// in a shared settings file does not erase its accepted Hook definitions.
    pub(super) fn is_category_complete(&self, category: crate::models::AgentAssetCategory) -> bool {
        self.complete && !self.rejected_categories.contains(&category)
    }

    fn stop_at_limit(&mut self) -> ControlFlow<AgentOutputStop> {
        self.stopped = true;
        self.complete = false;
        let accepted = self
            .declarations
            .keys()
            .filter(|key| !self.duplicate_ids.contains(*key))
            .take(self.limit)
            .count() as u64;
        self.emit_diagnostic(AgentAssetDiagnostic::Truncated {
            limit: AgentAssetLimitKind::FirstLevelEntries,
            accepted,
            observed_at_least: self.raw_offers as u64,
        });
        ControlFlow::Break(AgentOutputStop::EntryLimit)
    }
}

impl AgentDiagnosticOutput for BoundedParseOutput<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.run.diagnostics().has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.run.emit(self.owner.clone(), value)
    }
}

impl AgentParseOutput for BoundedParseOutput<'_> {
    fn emit_declaration(&mut self, value: ParsedAgentAsset) -> ControlFlow<AgentOutputStop> {
        if self.stopped {
            return ControlFlow::Break(AgentOutputStop::EntryLimit);
        }
        if !self.run.checkpoint(AgentInventoryStage::Parse) {
            self.stopped = true;
            self.complete = false;
            self.declarations.clear();
            return ControlFlow::Break(AgentOutputStop::Deadline);
        }
        self.raw_offers = self.raw_offers.saturating_add(1);
        let over_limit = self.raw_offers > self.limit;

        let valid = value.source_key == self.source_key
            && self.allowed_logical_origins.contains(&value.logical_origin)
            && self.categories.contains(&value.category)
            && !value.native_id.trim().is_empty()
            && !value.declaration_key.trim().is_empty()
            && !value.declaration_key.chars().any(char::is_control)
            && !value.declaration_id.trim().is_empty();
        if !valid {
            self.rejected_categories.insert(value.category);
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: value.declaration_id,
            });
            return if over_limit {
                self.stop_at_limit()
            } else {
                ControlFlow::Continue(())
            };
        }
        if self.declarations.contains_key(&value.declaration_id) {
            self.rejected_categories.insert(value.category);
            if let Some(previous) = self.declarations.get(&value.declaration_id) {
                self.rejected_categories.insert(previous.category);
            }
            self.duplicate_ids.insert(value.declaration_id.clone());
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: value.declaration_id.clone(),
            });
        } else if self.duplicate_ids.contains(&value.declaration_id) {
            self.rejected_categories.insert(value.category);
        } else {
            self.declarations
                .insert(value.declaration_id.clone(), value);
        }

        if over_limit {
            return self.stop_at_limit();
        }
        ControlFlow::Continue(())
    }
}

pub(super) struct BoundedResolveOutput<'a> {
    run: &'a mut AgentInventoryRun,
    owner: DiagnosticOwner,
    raw_offers: usize,
    limit: usize,
    drafts: BTreeMap<String, AgentAssetProjectedDraft>,
    rejected_keys: BTreeSet<String>,
    invalid: bool,
}

impl<'a> BoundedResolveOutput<'a> {
    pub(super) fn new(
        run: &'a mut AgentInventoryRun,
        context_id: &str,
        declaration_count: usize,
    ) -> Self {
        Self {
            run,
            owner: DiagnosticOwner::Context(context_id.to_string()),
            raw_offers: 0,
            limit: declaration_count,
            drafts: BTreeMap::new(),
            rejected_keys: BTreeSet::new(),
            invalid: false,
        }
    }

    pub(super) fn finish(self) -> Option<Vec<AgentAssetProjectedDraft>> {
        (!self.invalid).then(|| self.drafts.into_values().collect())
    }
}

impl AgentDiagnosticOutput for BoundedResolveOutput<'_> {
    fn has_regular_capacity(&self) -> bool {
        self.run.diagnostics().has_regular_capacity()
    }

    fn emit_diagnostic(&mut self, value: AgentAssetDiagnostic) -> AgentDiagnosticEmission {
        self.run.emit(self.owner.clone(), value)
    }
}

impl AgentResolveOutput for BoundedResolveOutput<'_> {
    fn emit_draft(&mut self, value: AgentAssetProjectedDraft) -> ControlFlow<AgentOutputStop> {
        if self.invalid {
            return ControlFlow::Break(AgentOutputStop::InvalidOutput);
        }
        if !self.run.checkpoint(AgentInventoryStage::Resolve) {
            self.invalid = true;
            self.drafts.clear();
            return ControlFlow::Break(AgentOutputStop::Deadline);
        }
        self.raw_offers = self.raw_offers.saturating_add(1);
        if self.raw_offers > self.limit || value.projection_key.trim().is_empty() {
            self.invalid = true;
            self.drafts.clear();
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: value.projection_key,
            });
            return ControlFlow::Break(AgentOutputStop::InvalidOutput);
        }
        if self.rejected_keys.contains(&value.projection_key)
            || self.drafts.remove(&value.projection_key).is_some()
        {
            self.rejected_keys.insert(value.projection_key.clone());
            self.invalid = true;
            self.drafts.clear();
            self.emit_diagnostic(AgentAssetDiagnostic::InvalidProjection {
                projection_key: value.projection_key,
            });
            return ControlFlow::Break(AgentOutputStop::InvalidOutput);
        }
        self.drafts.insert(value.projection_key.clone(), value);
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AgentAssetDeclaredState, AgentAssetDetails, AgentAssetLimits, AgentAssetPresence,
        AgentAssetScope, AgentAssetSourceKind, AgentSkillInvocationPolicy, AgentTrustState,
    };
    use crate::services::agent_cli::environment::run::ManualClock;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    fn source(native_source_key: &str, path: &str) -> AgentAssetSourceSpec {
        AgentAssetSourceSpec {
            verified_physical_path: None,
            hook_definition_source: false,
            provider: crate::models::AgentAssetProviderOrigin::Unknown,
            origin: crate::models::AgentAssetInstallationOrigin::Unknown,
            path_policy: Default::default(),
            native_source_key: native_source_key.to_owned(),
            label: native_source_key.to_owned(),
            scope: AgentAssetScope::User,
            path: PathBuf::from(path),
            allowed_root: PathBuf::from("/tmp"),
            precedence: 1,
            writable: true,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![crate::models::AgentAssetCategory::Skill],
            allowed_logical_origins: vec![
                crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                    scope: AgentAssetScope::User,
                    precedence: 1,
                },
            ],
        }
    }

    fn directory_source(native_source_key: &str, path: &str) -> AgentAssetSourceSpec {
        AgentAssetSourceSpec {
            source_kind: AgentAssetSourceKind::Directory,
            ..source(native_source_key, path)
        }
    }

    fn follow_up(
        parent_source_key: &str,
        parent_entry_name: &str,
        relative_path: &str,
        native_source_key: &str,
    ) -> AgentFollowUpSourceSpec {
        AgentFollowUpSourceSpec {
            parent_source_key: parent_source_key.to_owned(),
            target: AgentFollowUpSourceTarget::Descendant {
                directory_entry_name: parent_entry_name.to_owned(),
                relative_path: std::path::PathBuf::from(relative_path),
            },
            native_source_key: native_source_key.to_owned(),
            label: native_source_key.to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::Directory,
            categories: vec![crate::models::AgentAssetCategory::Skill],
        }
    }

    fn direct_file_follow_up(parent_source_key: &str, entry_name: &str) -> AgentFollowUpSourceSpec {
        AgentFollowUpSourceSpec {
            parent_source_key: parent_source_key.to_owned(),
            target: AgentFollowUpSourceTarget::ManifestFile {
                entry_name: entry_name.to_owned(),
            },
            native_source_key: "direct-file".to_owned(),
            label: "direct-file".to_owned(),
            scope: AgentAssetScope::User,
            precedence: 1,
            sensitive: false,
            source_kind: AgentAssetSourceKind::File,
            categories: vec![crate::models::AgentAssetCategory::Skill],
        }
    }

    fn declaration(source_key: &str, declaration_id: &str) -> ParsedAgentAsset {
        ParsedAgentAsset {
            declaration_id: declaration_id.to_owned(),
            resolution_group_key: declaration_id.to_owned(),
            source_key: source_key.to_owned(),
            native_id: declaration_id.to_owned(),
            declaration_key: declaration_id.to_owned(),
            label: declaration_id.to_owned(),
            category: crate::models::AgentAssetCategory::Skill,
            logical_origin: crate::services::agent_cli::contracts::AgentAssetLogicalOrigin {
                scope: AgentAssetScope::User,
                precedence: 1,
            },
            presence: AgentAssetPresence::Present,
            declared_state: AgentAssetDeclaredState::Enabled,
            trust_state: AgentTrustState::Unknown,
            role: crate::models::AgentAssetDeclarationRole::Definition,
            participation: crate::models::AgentAssetResolutionParticipation::Participates,
            provided_by: None,
            action_owner: None,
            explicitly_affected: Vec::new(),
            details: AgentAssetDetails::Skill {
                enabled: AgentAssetDeclaredState::Enabled,
                invocation_policy: AgentSkillInvocationPolicy::ModelInvocable,
            },
            facts: BTreeMap::new(),
            native_payload: crate::services::agent_cli::contracts::AgentAssetNativePayload::None,
        }
    }

    fn run_with_sources_limit(limit: usize) -> AgentInventoryRun {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.sources_per_context = limit;
        run_with_limits(limits)
    }

    fn run_with_limits(limits: AgentAssetLimits) -> AgentInventoryRun {
        AgentInventoryRun::with_clock(limits, Arc::new(ManualClock::new()))
    }

    fn source_output<'a, 'b>(
        state: &'a mut ContextSourceState,
        run: &'b mut AgentInventoryRun,
    ) -> BoundedSourceOutput<'a, 'b, 'static> {
        BoundedSourceOutput::initial(state, run, "context:test")
    }

    fn skill_link_parent() -> (
        AgentAssetSourceSpec,
        AgentAssetRevision,
        Vec<AgentAssetDirectoryEntry>,
    ) {
        let mut parent = directory_source("skills", "/tmp/home/.claude/skills");
        parent.path_policy = AgentAssetSourcePathPolicy::ReadonlySkillLinkRoot {
            shared_root: PathBuf::from("/tmp/home/.agents/skills"),
        };
        let revision = AgentAssetRevision {
            identity: "complete-manifest".to_owned(),
            observed_at: String::new(),
            size_bytes: None,
            is_missing: false,
            is_directory: true,
            is_symlink: false,
        };
        let manifest = vec![AgentAssetDirectoryEntry {
            name: "alias".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: true,
        }];
        (parent, revision, manifest)
    }

    fn skill_link_offer(entry: &str) -> AgentFollowUpSourceSpec {
        AgentFollowUpSourceSpec {
            target: AgentFollowUpSourceTarget::ReadonlySkillDirectoryLink {
                directory_entry_name: entry.to_owned(),
            },
            native_source_key: format!("skill:{entry}"),
            ..direct_file_follow_up("skills", entry)
        }
    }

    #[test]
    fn readonly_skill_link_admission_preserves_scope_identity_and_forces_readonly() {
        for scope in [AgentAssetScope::User, AgentAssetScope::Workspace] {
            let (mut parent, revision, manifest) = skill_link_parent();
            parent.scope = scope;
            let mut offer = skill_link_offer("alias");
            offer.scope = scope;
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            )
            .with_manifest_revision(&revision);
            assert!(output.emit_follow_up(offer).is_continue());
            drop(output);
            let values = state.finish(&mut run, "context:test");
            assert_eq!(values.len(), 1);
            let child = &values[0];
            assert_eq!(child.native_source_key, "skill:alias");
            assert_eq!(child.path, parent.path.join("alias/SKILL.md"));
            assert_eq!(child.allowed_root, parent.allowed_root);
            assert_eq!(child.scope, scope);
            assert_eq!(child.precedence, parent.precedence);
            assert!(!child.writable);
            assert!(
                matches!(&child.path_policy, AgentAssetSourcePathPolicy::ReadonlySkillLink {
                manifest_revision, entry_name, shared_root, ..
            } if manifest_revision == &revision && entry_name == "alias" && shared_root == &PathBuf::from("/tmp/home/.agents/skills"))
            );
            assert!(run.finish_diagnostics().is_empty());

            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            assert!(source_output(&mut state, &mut run)
                .emit_initial(child.clone())
                .is_continue());
            assert!(state.finish(&mut run, "context:test").is_empty());
        }
    }

    #[test]
    fn readonly_skill_link_admission_rejects_missing_or_inconsistent_manifest_authority() {
        for case in [
            "no opt-in",
            "no revision",
            "wrong parent",
            "parent scope",
            "child scope",
            "precedence",
            "parent category",
            "child category",
            "child type",
            "ordinary entry",
            "duplicate entry",
            "unknown entry",
            "missing directory",
            "wrong directory type",
            "linked directory",
            "depth",
        ] {
            let (mut parent, mut revision, mut manifest) = skill_link_parent();
            let mut offer = skill_link_offer("alias");
            let mut depth = 0;
            match case {
                "no opt-in" => parent.path_policy = AgentAssetSourcePathPolicy::NoFollow,
                "no revision" => {}
                "wrong parent" => offer.parent_source_key = "other".to_owned(),
                "parent scope" => parent.scope = AgentAssetScope::Managed,
                "child scope" => offer.scope = AgentAssetScope::Workspace,
                "precedence" => offer.precedence += 1,
                "parent category" => parent.categories.push(AgentAssetCategory::Hook),
                "child category" => offer.categories.push(AgentAssetCategory::Mcp),
                "child type" => offer.source_kind = AgentAssetSourceKind::Directory,
                "ordinary entry" => manifest[0].is_symlink = false,
                "duplicate entry" => manifest.push(manifest[0].clone()),
                "unknown entry" => manifest.clear(),
                "missing directory" => revision.is_missing = true,
                "wrong directory type" => revision.is_directory = false,
                "linked directory" => revision.is_symlink = true,
                "depth" => depth = MAX_FOLLOW_UP_DEPTH,
                _ => unreachable!(),
            }
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                depth,
            );
            if case != "no revision" {
                output = output.with_manifest_revision(&revision);
            }
            assert!(output.emit_follow_up(offer).is_continue(), "{case}");
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty(), "{case}");
            assert!(!run.finish_diagnostics().is_empty(), "{case}");
        }
    }

    #[test]
    fn readonly_skill_link_follow_up_retains_source_cap_and_deadline_breaks() {
        let (parent, revision, mut manifest) = skill_link_parent();
        manifest = (0..3)
            .map(|index| AgentAssetDirectoryEntry {
                name: format!("alias-{index}"),
                ..manifest[0].clone()
            })
            .collect();
        let mut run = run_with_sources_limit(2);
        let mut state = ContextSourceState::new(2);
        let mut output = BoundedSourceOutput::follow_up(
            &mut state,
            &mut run,
            "context:test",
            &parent,
            &manifest,
            0,
        )
        .with_manifest_revision(&revision);
        for (index, entry) in manifest.iter().enumerate() {
            assert_eq!(
                output
                    .emit_follow_up(skill_link_offer(&entry.name))
                    .is_break(),
                index == 2
            );
        }
        drop(output);
        assert_eq!(state.finish(&mut run, "context:test").len(), 2);
        assert!(run.finish_diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::SourcesPerContext,
                ..
            }
        )));

        let clock = Arc::new(ManualClock::new());
        let mut run = AgentInventoryRun::with_clock(AgentAssetLimits::DEFAULT, clock.clone());
        clock.advance(Duration::from_millis(
            AgentAssetLimits::DEFAULT.refresh_budget_ms,
        ));
        let mut state = ContextSourceState::new(8);
        let mut output = BoundedSourceOutput::follow_up(
            &mut state,
            &mut run,
            "context:test",
            &parent,
            &manifest,
            0,
        )
        .with_manifest_revision(&revision);
        assert_eq!(
            output.emit_follow_up(skill_link_offer("alias-0")),
            ControlFlow::Break(AgentOutputStop::Deadline)
        );
        drop(output);
        assert!(state.finish(&mut run, "context:test").is_empty());
    }

    #[test]
    fn source_one_over_keeps_deterministic_lexical_prefix_before_stopping() {
        let mut run = run_with_sources_limit(2);
        let mut state = ContextSourceState::new(2);
        let mut output = source_output(&mut state, &mut run);
        assert!(output.emit_initial(source("z", "/tmp/z")).is_continue());
        assert!(output.emit_initial(source("a", "/tmp/a")).is_continue());
        assert!(output.emit_initial(source("b", "/tmp/b")).is_break());
        drop(output);
        let values = state.finish(&mut run, "context:test");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].native_source_key, "a");
        assert_eq!(values[1].native_source_key, "b");
    }

    #[test]
    fn direct_manifest_file_requires_exact_file_entry_and_materializes_without_child_directory() {
        let mut run = run_with_sources_limit(4);
        let parent = directory_source("skills", "/tmp/skills");
        let manifest = [
            AgentAssetDirectoryEntry {
                name: "SKILL.md".to_owned(),
                source_kind: AgentAssetSourceKind::File,
                is_symlink: false,
            },
            AgentAssetDirectoryEntry {
                name: "folder".to_owned(),
                source_kind: AgentAssetSourceKind::Directory,
                is_symlink: false,
            },
        ];
        let mut state = ContextSourceState::new(4);
        let mut output = BoundedSourceOutput::follow_up(
            &mut state,
            &mut run,
            "context:test",
            &parent,
            &manifest,
            0,
        );
        assert!(output
            .emit_follow_up(direct_file_follow_up("skills", "SKILL.md"))
            .is_continue());
        drop(output);
        let retained = state.finish(&mut run, "context:test");
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].path, PathBuf::from("/tmp/skills/SKILL.md"));
        assert_eq!(retained[0].source_kind, AgentAssetSourceKind::File);
    }

    #[test]
    fn direct_manifest_file_rejections_and_shared_limit_are_typed() {
        let parent = directory_source("skills", "/tmp/skills");
        let cases = [
            (
                vec![AgentAssetDirectoryEntry {
                    name: "SKILL.md".to_owned(),
                    source_kind: AgentAssetSourceKind::Directory,
                    is_symlink: false,
                }],
                "type",
            ),
            (
                vec![AgentAssetDirectoryEntry {
                    name: "other.md".to_owned(),
                    source_kind: AgentAssetSourceKind::File,
                    is_symlink: false,
                }],
                "missing",
            ),
        ];
        for (manifest, label) in cases {
            let mut run = run_with_sources_limit(4);
            let mut state = ContextSourceState::new(4);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            );
            assert!(
                output
                    .emit_follow_up(direct_file_follow_up("skills", "SKILL.md"))
                    .is_continue(),
                "{label}"
            );
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty());
            let diagnostics = run.finish_diagnostics();
            assert_eq!(diagnostics.len(), 1, "{label}");
            if label == "type" {
                assert!(matches!(
                    diagnostics[0],
                    AgentAssetDiagnostic::SourceTypeMismatch {
                        expected: AgentAssetSourceKind::File,
                        actual: AgentAssetSourceKind::Directory,
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    diagnostics[0],
                    AgentAssetDiagnostic::InvalidProjection { .. }
                ));
            }
        }

        let manifest = [AgentAssetDirectoryEntry {
            name: "SKILL.md".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        }];
        let mut run = run_with_sources_limit(1);
        let mut state = ContextSourceState::new(1);
        let mut output = BoundedSourceOutput::follow_up(
            &mut state,
            &mut run,
            "context:test",
            &parent,
            &manifest,
            0,
        );
        assert!(output
            .emit_follow_up(direct_file_follow_up("skills", "SKILL.md"))
            .is_continue());
        let mut second = direct_file_follow_up("skills", "SKILL.md");
        second.native_source_key = "direct-file-second".to_owned();
        assert!(output.emit_follow_up(second).is_break());
        drop(output);
        assert!(state.finish(&mut run, "context:test").is_empty());
        assert!(run
            .finish_diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic, AgentAssetDiagnostic::Truncated { .. })));
    }

    #[test]
    fn source_collision_indexes_do_not_reject_unrelated_sources_after_removal() {
        let mut run = run_with_sources_limit(8);
        let mut state = ContextSourceState::new(8);
        let mut output = source_output(&mut state, &mut run);
        assert!(output
            .emit_initial(source("same", "/tmp/one"))
            .is_continue());
        assert!(output
            .emit_initial(source("same", "/tmp/two"))
            .is_continue());
        assert!(output
            .emit_initial(source("other", "/tmp/three"))
            .is_continue());
        drop(output);
        let values = state.finish(&mut run, "context:test");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].native_source_key, "other");
    }

    #[test]
    fn eligible_depth_zero_sources_skip_collisions_before_snapshot_expansion() {
        let mut run = run_with_sources_limit(4);
        let mut state = ContextSourceState::new(4);
        let mut output = source_output(&mut state, &mut run);
        for (native, path) in [
            ("collision", "/tmp/one"),
            ("collision", "/tmp/two"),
            ("valid-z", "/tmp/z"),
            ("valid-a", "/tmp/a"),
        ] {
            if output.emit_initial(source(native, path)).is_break() {
                break;
            }
        }
        drop(output);
        let eligible = state.eligible_sources_at_depth(0);
        assert_eq!(
            eligible
                .iter()
                .map(|pending| pending.spec.native_source_key.as_str())
                .collect::<Vec<_>>(),
            ["valid-a", "valid-z"]
        );
        assert!(!eligible
            .iter()
            .any(|pending| pending.spec.native_source_key == "collision"));
    }

    #[test]
    fn overlapping_native_and_physical_collisions_are_order_independent() {
        fn collect(order: [(&str, &str); 3]) -> Vec<String> {
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = source_output(&mut state, &mut run);
            for (native, path) in order {
                assert!(output.emit_initial(source(native, path)).is_continue());
            }
            drop(output);
            state
                .finish(&mut run, "context:test")
                .into_iter()
                .map(|source| source.native_source_key)
                .collect()
        }

        let first = collect([
            ("same", "/tmp/one"),
            ("same", "/tmp/two"),
            ("other", "/tmp/one"),
        ]);
        let second = collect([
            ("other", "/tmp/one"),
            ("same", "/tmp/one"),
            ("same", "/tmp/two"),
        ]);
        let third = collect([
            ("same", "/tmp/two"),
            ("other", "/tmp/one"),
            ("same", "/tmp/one"),
        ]);
        assert!(first.is_empty());
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn exact_duplicate_source_order_keys_are_rejected_with_a_stable_diagnostic() {
        fn collect(order: [usize; 3]) -> (Vec<String>, Vec<AgentAssetDiagnostic>) {
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let values = [
                source("same", "/tmp/one"),
                source("same", "/tmp/one"),
                source("other", "/tmp/two"),
            ];
            let mut output = source_output(&mut state, &mut run);
            for index in order {
                assert!(output.emit_initial(values[index].clone()).is_continue());
            }
            drop(output);
            let retained = state
                .finish(&mut run, "context:test")
                .into_iter()
                .map(|value| value.native_source_key)
                .collect();
            (retained, run.finish_diagnostics())
        }

        let first = collect([0, 1, 2]);
        let second = collect([2, 0, 1]);
        assert_eq!(first.0, vec!["other"]);
        assert_eq!(first.0, second.0);
        assert_eq!(
            first
                .1
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::InvalidProjection { projection_key }
                        if projection_key == "source:same"
                ))
                .count(),
            1
        );
        assert_eq!(first.1, second.1);
    }

    #[test]
    fn parse_exact_boundary_commits_sorted_declarations_without_truncation() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 2;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        assert!(output
            .emit_declaration(declaration("source", "z"))
            .is_continue());
        assert!(output
            .emit_declaration(declaration("source", "a"))
            .is_continue());
        assert!(output.is_complete());
        let values = output.finish();
        assert_eq!(
            values
                .iter()
                .map(|value| value.declaration_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(run.finish_diagnostics().is_empty());
    }

    #[test]
    fn parse_one_over_processes_candidate_and_keeps_deterministic_prefix() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 2;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        assert!(output
            .emit_declaration(declaration("source", "z"))
            .is_continue());
        assert!(output
            .emit_declaration(declaration("source", "a"))
            .is_continue());
        assert!(output
            .emit_declaration(declaration("source", "b"))
            .is_break());
        assert!(!output.is_complete());
        let values = output.finish();
        assert_eq!(
            values
                .iter()
                .map(|value| value.declaration_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(run.finish_diagnostics().iter().any(|value| matches!(
            value,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FirstLevelEntries,
                accepted: 2,
                observed_at_least: 3,
            }
        )));
    }

    #[test]
    fn parse_duplicate_id_is_rejected_independently_of_input_order() {
        fn collect(order: [&str; 3]) -> (Vec<String>, Vec<AgentAssetDiagnostic>) {
            let mut limits = AgentAssetLimits::DEFAULT;
            limits.first_level_entries = 3;
            let mut run = run_with_limits(limits);
            let source = source("source", "/tmp/config.json");
            let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
            for id in order {
                assert!(output
                    .emit_declaration(declaration("source", id))
                    .is_continue());
            }
            let values = output
                .finish()
                .into_iter()
                .map(|value| value.declaration_id)
                .collect();
            (values, run.finish_diagnostics())
        }

        let first = collect(["same", "other", "same"]);
        let second = collect(["same", "same", "other"]);
        assert_eq!(first.0, vec!["other"]);
        assert_eq!(first.0, second.0);
        assert_eq!(
            first
                .1
                .iter()
                .filter(|value| matches!(value, AgentAssetDiagnostic::InvalidProjection { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn invalid_parse_offers_share_the_raw_limit_and_stop_the_producer() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 2;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        let mut constructed = 0;
        for index in 0..100 {
            constructed += 1;
            let value = declaration("wrong-source", &format!("invalid-{index}"));
            if output.emit_declaration(value).is_break() {
                break;
            }
        }
        assert_eq!(constructed, 3);
        assert!(output.finish().is_empty());
        assert!(run.finish_diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FirstLevelEntries,
                accepted: 0,
                observed_at_least: 3,
            }
        )));
    }

    #[test]
    fn invalid_and_duplicate_parse_offers_share_the_same_one_over_gate() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 2;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        assert!(output
            .emit_declaration(declaration("source", "same"))
            .is_continue());
        assert!(output
            .emit_declaration(declaration("wrong-source", "invalid"))
            .is_continue());
        assert!(output
            .emit_declaration(declaration("source", "same"))
            .is_break());
        assert!(output.finish().is_empty());
        let diagnostics = run.finish_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FirstLevelEntries,
                accepted: 0,
                observed_at_least: 3,
            }
        )));
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic,
                    AgentAssetDiagnostic::InvalidProjection { .. }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn zero_limit_invalid_parse_offer_still_emits_diagnostic_and_closes() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 0;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        assert!(output
            .emit_declaration(declaration("wrong-source", ""))
            .is_break());
        assert!(output.finish().is_empty());
        let diagnostics = run.finish_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { .. }
        )));
        assert!(diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FirstLevelEntries,
                accepted: 0,
                observed_at_least: 1,
            }
        )));
    }

    #[test]
    fn parse_limit_zero_processes_one_sentinel_offer_and_retains_nothing() {
        let mut limits = AgentAssetLimits::DEFAULT;
        limits.first_level_entries = 0;
        let mut run = run_with_limits(limits);
        let source = source("source", "/tmp/config.json");
        let mut output = BoundedParseOutput::new(&mut run, "source-id", &source);
        assert!(output
            .emit_declaration(declaration("source", "sentinel"))
            .is_break());
        assert!(output.finish().is_empty());
        assert!(run.finish_diagnostics().iter().any(|value| matches!(
            value,
            AgentAssetDiagnostic::Truncated {
                limit: AgentAssetLimitKind::FirstLevelEntries,
                accepted: 0,
                observed_at_least: 1,
            }
        )));
    }

    #[test]
    fn source_producer_stops_constructing_after_sink_break() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut run = run_with_sources_limit(2);
        let mut state = ContextSourceState::new(2);
        let mut output = source_output(&mut state, &mut run);
        let constructed = AtomicUsize::new(0);
        for index in 0..100 {
            let ordinal = constructed.fetch_add(1, Ordering::SeqCst);
            let value = source(&format!("source-{ordinal}"), &format!("/tmp/{index}"));
            if output.emit_initial(value).is_break() {
                break;
            }
        }
        assert_eq!(constructed.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn source_producer_stops_constructing_when_deadline_breaks_sink() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let clock = Arc::new(ManualClock::new());
        let mut run = AgentInventoryRun::with_clock(AgentAssetLimits::DEFAULT, clock.clone());
        clock.advance(Duration::from_millis(
            AgentAssetLimits::DEFAULT.refresh_budget_ms,
        ));
        let mut state = ContextSourceState::new(AgentAssetLimits::DEFAULT.sources_per_context);
        let mut output = source_output(&mut state, &mut run);
        let constructed = AtomicUsize::new(0);
        for index in 0..100 {
            let ordinal = constructed.fetch_add(1, Ordering::SeqCst);
            let value = source(&format!("source-{ordinal}"), &format!("/tmp/{index}"));
            if output.emit_initial(value).is_break() {
                break;
            }
        }
        assert_eq!(constructed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn follow_up_sources_share_the_initial_source_cap_and_are_bounded_at_depth_two() {
        let mut run = run_with_sources_limit(2);
        let mut state = ContextSourceState::new(2);
        let parent = directory_source("parent", "/tmp/assets");
        let manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }];
        {
            let mut initial = source_output(&mut state, &mut run);
            assert!(initial.emit_initial(parent.clone()).is_continue());
        }
        {
            let mut follow_up_output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            );
            assert!(follow_up_output
                .emit_follow_up(follow_up("parent", "skills", "nested", "child"))
                .is_continue());
            assert!(follow_up_output
                .emit_follow_up(follow_up("parent", "skills", "second", "second-child"))
                .is_break());
        }
        assert!(state.eligible_sources_at_depth(2).is_empty());
        let sources = state.finish(&mut run, "context:test");
        assert_eq!(sources.len(), 2);
        assert!(sources
            .iter()
            .any(|source| source.native_source_key == "parent"));
        assert!(sources
            .iter()
            .any(|source| source.native_source_key == "child"));
    }

    #[test]
    fn follow_up_at_max_depth_is_rejected_without_publishing_a_child() {
        let mut run = run_with_sources_limit(8);
        let mut state = ContextSourceState::new(8);
        let parent = directory_source("parent", "/tmp/assets");
        let manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }];
        let mut output = BoundedSourceOutput::follow_up(
            &mut state,
            &mut run,
            "context:test",
            &parent,
            &manifest,
            MAX_FOLLOW_UP_DEPTH,
        );
        assert!(output
            .emit_follow_up(follow_up("parent", "skills", "nested", "child"))
            .is_continue());
        drop(output);

        assert!(state.cached_snapshots.is_empty());
        assert!(state.finish(&mut run, "context:test").is_empty());
        assert!(run.finish_diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            AgentAssetDiagnostic::InvalidProjection { projection_key }
                if projection_key == "follow-up:parent:skills"
        )));
    }

    #[test]
    fn follow_up_retained_prefix_is_stable_when_offer_order_is_reversed() {
        fn collect(order: [&str; 2]) -> Vec<String> {
            let mut run = run_with_sources_limit(2);
            let mut state = ContextSourceState::new(2);
            let parent = directory_source("parent", "/tmp/assets/parent");
            let manifest = [
                AgentAssetDirectoryEntry {
                    name: "a".to_owned(),
                    source_kind: AgentAssetSourceKind::Directory,
                    is_symlink: false,
                },
                AgentAssetDirectoryEntry {
                    name: "z".to_owned(),
                    source_kind: AgentAssetSourceKind::Directory,
                    is_symlink: false,
                },
            ];
            {
                let mut initial = source_output(&mut state, &mut run);
                assert!(initial.emit_initial(parent.clone()).is_continue());
            }
            {
                let mut output = BoundedSourceOutput::follow_up(
                    &mut state,
                    &mut run,
                    "context:test",
                    &parent,
                    &manifest,
                    0,
                );
                for entry in order {
                    let offer = follow_up("parent", entry, "config", &format!("child-{entry}"));
                    if output.emit_follow_up(offer).is_break() {
                        break;
                    }
                }
            }
            state
                .finish(&mut run, "context:test")
                .into_iter()
                .map(|source| source.native_source_key)
                .collect()
        }

        assert_eq!(collect(["a", "z"]), ["parent", "child-a"]);
        assert_eq!(collect(["z", "a"]), ["parent", "child-a"]);
    }

    #[test]
    fn follow_up_rejects_unsafe_relative_paths_and_unknown_entries() {
        let parent = directory_source("parent", "/tmp/assets");
        let manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }];
        for relative in [
            "",
            ".",
            "..",
            "../escape",
            "/absolute",
            "C:/escape",
            "a\\b",
            "line\nfeed",
            "nul\0byte",
        ] {
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            );
            assert!(output
                .emit_follow_up(follow_up("parent", "skills", relative, "child"))
                .is_continue());
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty());
            let diagnostics = run.finish_diagnostics();
            assert_eq!(diagnostics.len(), 1);
            assert!(matches!(
                diagnostics.first(),
                Some(AgentAssetDiagnostic::InvalidProjection { projection_key })
                    if projection_key == "follow-up:parent:skills"
            ));
            assert!(!diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::SourceTypeMismatch {
                    expected: AgentAssetSourceKind::Directory,
                    actual: AgentAssetSourceKind::Directory,
                    ..
                }
            )));
        }
        for entry in ["missing", "skills"] {
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            );
            let offer = if entry == "missing" {
                follow_up("parent", entry, "nested", "child")
            } else {
                follow_up("wrong-parent", entry, "nested", "child")
            };
            assert!(output.emit_follow_up(offer).is_continue());
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty());
        }
    }

    #[test]
    fn invalid_follow_up_offers_share_the_raw_limit_and_stop_the_producer() {
        fn assert_invalid_spam<F>(
            label: &str,
            parent: &AgentAssetSourceSpec,
            manifest: &[AgentAssetDirectoryEntry],
            build_offer: F,
        ) where
            F: Fn() -> AgentFollowUpSourceSpec,
        {
            let mut run = run_with_sources_limit(2);
            let mut state = ContextSourceState::new(2);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                parent,
                manifest,
                0,
            );
            let mut constructed = 0;
            for _ in 0..100 {
                constructed += 1;
                if output.emit_follow_up(build_offer()).is_break() {
                    break;
                }
            }
            assert_eq!(constructed, 3, "{label} must stop at limit + 1");
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty());
            assert!(run.finish_diagnostics().iter().any(|diagnostic| matches!(
                diagnostic,
                AgentAssetDiagnostic::Truncated {
                    limit: AgentAssetLimitKind::SourcesPerContext,
                    accepted: 0,
                    observed_at_least: 3,
                }
            )));
        }

        let parent = directory_source("parent", "/tmp/assets");
        let valid_manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: false,
        }];
        assert_invalid_spam("wrong parent", &parent, &valid_manifest, || {
            follow_up("wrong-parent", "skills", "nested", "child")
        });
        assert_invalid_spam("missing entry", &parent, &valid_manifest, || {
            follow_up("parent", "missing", "nested", "child")
        });

        let symlink_manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::Directory,
            is_symlink: true,
        }];
        assert_invalid_spam("symlink entry", &parent, &symlink_manifest, || {
            follow_up("parent", "skills", "nested", "child")
        });

        let file_manifest = [AgentAssetDirectoryEntry {
            name: "skills".to_owned(),
            source_kind: AgentAssetSourceKind::File,
            is_symlink: false,
        }];
        assert_invalid_spam("non-directory entry", &parent, &file_manifest, || {
            follow_up("parent", "skills", "nested", "child")
        });

        for relative in ["", ".", "..", "../escape", "/absolute"] {
            assert_invalid_spam(relative, &parent, &valid_manifest, || {
                follow_up("parent", "skills", relative, "child")
            });
        }
    }

    #[test]
    fn follow_up_rejects_symlink_and_non_directory_manifest_entries() {
        let parent = directory_source("parent", "/tmp/assets");
        for (source_kind, is_symlink) in [
            (AgentAssetSourceKind::Directory, true),
            (AgentAssetSourceKind::File, false),
        ] {
            let manifest = [AgentAssetDirectoryEntry {
                name: "skills".to_owned(),
                source_kind,
                is_symlink,
            }];
            let mut run = run_with_sources_limit(8);
            let mut state = ContextSourceState::new(8);
            let mut output = BoundedSourceOutput::follow_up(
                &mut state,
                &mut run,
                "context:test",
                &parent,
                &manifest,
                0,
            );
            assert!(output
                .emit_follow_up(follow_up("parent", "skills", "nested", "child"))
                .is_continue());
            drop(output);
            assert!(state.finish(&mut run, "context:test").is_empty());
        }
    }
}
