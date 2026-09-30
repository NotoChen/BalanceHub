//! Explicit ownership relations. Comparison is read-only; mutations consume a
//! bound preview and only change minimal library ownership metadata.
mod rules;

use super::{
    comparison,
    operations::cache::encoded_bytes,
    planning,
    repository::{Entry, Library},
    CatalogService,
};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{
        token::DEFAULT_PLAN_TTL, GuardedFile, MutationInspector, MutationInventory,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

#[derive(Default, Clone, Serialize, Deserialize)]
pub(super) struct CatalogRelations {
    separations: BTreeMap<String, Separation>,
    associations: BTreeMap<String, ManualAssociation>,
    diagnostics: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Separation {
    left: String,
    right: String,
    hide_candidate: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct AssociationOrigin {
    name: String,
    category: AgentAssetCategory,
    binding_ids: BTreeSet<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ManualAssociation {
    destination_asset_id: String,
    destination_binding_ids: BTreeSet<String>,
    sources: BTreeMap<String, AssociationOrigin>,
}

impl CatalogRelations {
    pub(super) fn has_association(&self, id: &str) -> bool {
        self.associations.values().any(|association| {
            association.destination_asset_id == id || association.sources.contains_key(id)
        })
    }
    pub(super) fn forget_separations(&mut self, id: &str) {
        self.separations
            .retain(|_, pair| pair.left != id && pair.right != id);
    }
    pub(super) fn blocks_members(&self, members: &[String]) -> bool {
        self.separations
            .values()
            .any(|pair| members.contains(&pair.left) && members.contains(&pair.right))
    }
    pub(super) fn hides(&self, left: &str, right: &str) -> bool {
        self.separations
            .get(&pair_key(left, right))
            .is_some_and(|pair| pair.hide_candidate)
    }
    pub(super) fn separated(&self, id: &str) -> Vec<String> {
        self.separations
            .values()
            .filter_map(|pair| {
                if pair.left == id {
                    Some(pair.right.clone())
                } else if pair.right == id {
                    Some(pair.left.clone())
                } else {
                    None
                }
            })
            .collect()
    }
    pub(super) fn invalidate_physical(&mut self, members: &[String]) {
        let before = self.separations.len();
        self.separations
            .retain(|_, pair| !(members.contains(&pair.left) && members.contains(&pair.right)));
        if self.separations.len() != before {
            let note = "原先保留分开的资产现已共用同一物理来源；该分开约束已失效，完整来源保持一起"
                .to_owned();
            if !self.diagnostics.contains(&note) {
                self.diagnostics.push(note);
            }
        }
    }
    pub(super) fn remap(&mut self, owner: &str, members: &[String]) {
        let pairs = std::mem::take(&mut self.separations);
        for mut pair in pairs.into_values() {
            if members.contains(&pair.left) {
                pair.left = owner.to_owned();
            }
            if members.contains(&pair.right) {
                pair.right = owner.to_owned();
            }
            if pair.left == pair.right {
                continue;
            }
            self.separate(&pair.left, &pair.right, pair.hide_candidate);
        }
        for association in self.associations.values_mut() {
            if members.contains(&association.destination_asset_id) {
                association.destination_asset_id = owner.to_owned();
            }
        }
    }
    fn separate(&mut self, left: &str, right: &str, hide: bool) {
        let (left, right) = ordered_pair(left, right);
        self.separations
            .entry(pair_key(left, right))
            .and_modify(|pair| pair.hide_candidate |= hide)
            .or_insert_with(|| Separation {
                left: left.to_owned(),
                right: right.to_owned(),
                hide_candidate: hide,
            });
    }
}

pub(super) struct RelationPlan {
    intent: AgentCatalogRelationIntent,
    guard: GuardedFile,
    native_stamp: String,
    change: rules::Change,
    inspector: Arc<dyn MutationInspector>,
    _permit: planning::PreviewPermit,
}

impl CatalogService {
    pub(crate) fn preview_relation(
        &self,
        actor: &str,
        request: AgentCatalogRelationPreviewRequest,
        inspector: Arc<dyn MutationInspector>,
    ) -> Result<AgentCatalogRelationPreview, String> {
        if actor.is_empty()
            || request.workspace.as_deref().map(std::path::Path::new) != inspector.workspace()
        {
            return Err("关系预览范围无效".to_owned());
        }
        self.prune_previews();
        let snapshot = inspector.inspect().map_err(|_| "原生资产盘点失败")?;
        let (catalog, mut base) = self.read_catalog(&snapshot)?;
        if catalog.revision != request.expected_revision {
            return Err("资产目录已变化，请刷新后比较".to_owned());
        }
        // Preserve pending recovery barriers from disk even when a dry native
        // projection can already observe the likely reconciliation outcome.
        if let Some(bytes) = base.guard.bytes() {
            let persisted: Library = serde_json::from_slice(bytes).map_err(|_| "共享库格式无效")?;
            for (id, entry) in &persisted.entries {
                if entry.receipts.values().any(|receipt| {
                    receipt
                        .hook
                        .as_ref()
                        .is_some_and(|hook| hook.pending.is_some())
                }) {
                    if let Some(projected) = base.library.entries.get_mut(id) {
                        projected.receipts = entry.receipts.clone();
                    }
                }
            }
        }
        let ((left, left_entry), (right, right_entry)) =
            comparison_pair(&catalog, &base.library, &snapshot, &request.intent)?;
        let (sides, equality, differences) =
            comparison::compare(&snapshot, (&left, &left_entry), (&right, &right_entry));
        let action = action(&request.intent);
        let relation_key = relation_key(&request.intent);
        let mut capabilities = Vec::new();
        let intents = if matches!(request.intent, AgentCatalogRelationIntent::Detach { .. }) {
            vec![request.intent.clone()]
        } else {
            vec![
                AgentCatalogRelationIntent::Merge {
                    destination_asset_id: left.id.clone(),
                    source_asset_id: right.id.clone(),
                },
                AgentCatalogRelationIntent::Merge {
                    destination_asset_id: right.id.clone(),
                    source_asset_id: left.id.clone(),
                },
                AgentCatalogRelationIntent::KeepSeparate {
                    left_asset_id: left.id.clone(),
                    right_asset_id: right.id.clone(),
                    hide_candidate: true,
                },
                AgentCatalogRelationIntent::RestoreHint {
                    left_asset_id: left.id.clone(),
                    right_asset_id: right.id.clone(),
                },
            ]
        };
        for intent in intents {
            let result = rules::prepare(&base.library, &snapshot, &intent)
                .and_then(|change| self.check_relation_admission(&change));
            capabilities.push(AgentCatalogRelationCapability {
                intent,
                available: result.is_ok(),
                reason: result.err(),
            });
        }
        let selected = if action == AgentCatalogRelationAction::Compare {
            None
        } else {
            Some(
                rules::prepare(&base.library, &snapshot, &request.intent).and_then(|change| {
                    self.check_relation_admission(&change)?;
                    Ok(change)
                }),
            )
        };
        let reason = selected
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .cloned();
        let mut preview = AgentCatalogRelationPreview {
            token: None,
            relation_key: relation_key.clone(),
            action,
            expires_at: None,
            sides,
            equality,
            differences,
            capabilities,
            affected_binding_ids: Vec::new(),
            affected_receipt_target_ids: Vec::new(),
            available: reason.is_none(),
            reason,
            notes: vec![
                "比较基于完整定义；较大内容仅展示部分预览。关系整理不会修改原生配置或共享版本。"
                    .to_owned(),
            ],
        };
        if let Some(Ok(change)) = selected {
            preview.affected_binding_ids = change.binding_ids.iter().cloned().collect();
            preview.affected_receipt_target_ids = change.receipt_ids.iter().cloned().collect();
            let size = base
                .guard
                .bytes()
                .map_or(0, <[u8]>::len)
                .saturating_add(change.estimate_bytes())
                .saturating_add(encoded_bytes(inspector.settings()));
            let permit = planning::reserve(&self.preview_bytes, size)?;
            let plan = RelationPlan {
                intent: request.intent,
                guard: base.guard,
                native_stamp: planning::native_stamp(&snapshot)?,
                change,
                inspector,
                _permit: permit,
            };
            preview.token = Some(
                self.relation_plans
                    .issue(
                        actor,
                        &relation_key,
                        action,
                        plan,
                        Instant::now(),
                        DEFAULT_PLAN_TTL,
                    )
                    .map_err(|error| error.message)?,
            );
            preview.expires_at = Some(
                (chrono::Local::now()
                    + chrono::Duration::seconds(DEFAULT_PLAN_TTL.as_secs() as i64))
                .to_rfc3339(),
            );
        }
        Ok(preview)
    }

    pub(crate) fn commit_relation(
        &self,
        actor: &str,
        request: AgentCatalogRelationCommitRequest,
    ) -> Result<AgentCatalogRelationCommitResult, String> {
        self.prune_previews();
        // Admission -> native domains -> repository, matching catalog start.
        // New operations cannot enter between overlap checks and the CAS.
        let operations = self.operations.lock().map_err(|_| "后台任务状态不可用")?;
        let bound = self
            .relation_plans
            .consume_bound(
                actor,
                &request.plan_token,
                &request.relation_key,
                &request.action,
                Instant::now(),
            )
            .map_err(|error| error.message)?;
        let plan = bound.value;
        if operations.values().any(|operation| {
            operation.overlaps(
                &plan.change.asset_ids,
                &plan.change.binding_ids,
                &plan.change.domains,
            )
        }) {
            return Err("相关资产正在执行后台任务，请完成后重新预览".to_owned());
        }
        let locks = self.native.domain_locks();
        let canceled = AtomicBool::new(false);
        let _domains = locks
            .acquire(
                &plan.change.domains,
                &canceled,
                Instant::now() + Duration::from_secs(15),
            )
            .map_err(|_| "等待关系写域超时；请刷新后重新预览".to_owned())?;
        let snapshot = plan.inspector.inspect().map_err(|_| "确认前原生盘点失败")?;
        plan.guard
            .revalidate()
            .map_err(|_| "共享库已变化，请重新预览关系")?;
        if planning::native_stamp(&snapshot)? != plan.native_stamp {
            return Err("来源、范围或能力已变化，请重新比较".to_owned());
        }
        let result = self.repository.guarded_transact(&plan.guard, |library| {
            plan.change.seed_missing(library)?;
            let fresh = rules::prepare(library, &snapshot, &plan.intent)?;
            if fresh.signature()? != plan.change.signature()? {
                return Err("当前归属或恢复记录已变化，未变更关系".to_owned());
            }
            rules::apply(library, &snapshot, &plan.intent, &fresh)
        })?;
        self.clear_unpersisted_observations();
        Ok(result)
    }

    fn check_relation_admission(&self, change: &rules::Change) -> Result<(), String> {
        let operations = self.operations.lock().map_err(|_| "后台任务状态不可用")?;
        if operations.values().any(|operation| {
            operation.overlaps(&change.asset_ids, &change.binding_ids, &change.domains)
        }) {
            return Err("相关资产正在执行后台任务，请完成后再整理关系".to_owned());
        }
        Ok(())
    }
}

pub(super) fn summaries(
    library: &Library,
    snapshot: &MutationInventory,
    id: &str,
) -> Vec<AgentCatalogManualAssociationSummary> {
    library
        .relations
        .associations
        .iter()
        .filter(|(_, record)| record.destination_asset_id == id)
        .map(|(key, record)| {
            let result = rules::prepare(
                library,
                snapshot,
                &AgentCatalogRelationIntent::Detach {
                    association_id: key.clone(),
                },
            );
            AgentCatalogManualAssociationSummary {
                id: key.clone(),
                label: "手工合并的资源对应关系".to_owned(),
                source_asset_ids: record.sources.keys().cloned().collect(),
                can_detach: result.is_ok(),
                reason: result.err(),
            }
        })
        .collect()
}

pub(super) fn diagnostics(library: &Library) -> &[String] {
    &library.relations.diagnostics
}

type ComparisonSubject = (AgentCatalogAsset, Entry);

fn comparison_pair(
    catalog: &AgentAssetCatalog,
    library: &Library,
    snapshot: &MutationInventory,
    intent: &AgentCatalogRelationIntent,
) -> Result<(ComparisonSubject, ComparisonSubject), String> {
    let pair = |left: &str, right: &str| {
        if left == right {
            return Err("请选择两项不同资产".to_owned());
        }
        let side = |id: &str| -> Result<ComparisonSubject, String> {
            Ok((
                catalog
                    .assets
                    .iter()
                    .find(|asset| asset.id == id)
                    .cloned()
                    .ok_or("比较资产已离开当前目录")?,
                library.entries.get(id).cloned().ok_or("比较资产不存在")?,
            ))
        };
        Ok((side(left)?, side(right)?))
    };
    match intent {
        AgentCatalogRelationIntent::Compare {
            left_asset_id,
            right_asset_id,
        }
        | AgentCatalogRelationIntent::KeepSeparate {
            left_asset_id,
            right_asset_id,
            ..
        }
        | AgentCatalogRelationIntent::RestoreHint {
            left_asset_id,
            right_asset_id,
        } => pair(left_asset_id, right_asset_id),
        AgentCatalogRelationIntent::Merge {
            destination_asset_id,
            source_asset_id,
        } => pair(destination_asset_id, source_asset_id),
        AgentCatalogRelationIntent::Detach { association_id } => {
            let record = library
                .relations
                .associations
                .get(association_id)
                .ok_or("手工对应关系已不存在")?;
            let (source_id, source) = record.sources.iter().next().ok_or("手工对应记录无效")?;
            let destination = catalog
                .assets
                .iter()
                .find(|asset| asset.id == record.destination_asset_id)
                .cloned()
                .ok_or("对应资产已离开当前目录")?;
            let destination_entry = library
                .entries
                .get(&record.destination_asset_id)
                .cloned()
                .ok_or("对应资产不存在")?;
            // The current physical group can include aliases added after the
            // manual merge. Compare the same complete move set that commit
            // approves; a rejected detach still shows its recorded origin.
            let binding_ids = rules::prepare(library, snapshot, intent)
                .map(|change| change.bindings_moving_to(source_id))
                .unwrap_or_else(|_| source.binding_ids.clone());
            let mut detached = destination.clone();
            detached.id = source_id.clone();
            detached.name = source.name.clone();
            detached.version = None;
            detached.ownership = AgentCatalogOwnership::Observed;
            detached
                .bindings
                .retain(|binding| binding_ids.contains(&binding.id));
            detached.unresolved_targets.clear();
            let mut detached_entry = Entry::new(source.name.clone(), source.category);
            detached_entry.aliases = destination_entry
                .aliases
                .iter()
                .filter(|(id, _)| binding_ids.contains(*id))
                .map(|(id, observation)| (id.clone(), observation.clone()))
                .collect();
            Ok(((destination, destination_entry), (detached, detached_entry)))
        }
    }
}

fn ordered_pair<'a>(left: &'a str, right: &'a str) -> (&'a str, &'a str) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}
fn pair_key(left: &str, right: &str) -> String {
    let (left, right) = ordered_pair(left, right);
    format!("{left}:{right}")
}
fn relation_key(intent: &AgentCatalogRelationIntent) -> String {
    match intent {
        AgentCatalogRelationIntent::Compare {
            left_asset_id,
            right_asset_id,
        }
        | AgentCatalogRelationIntent::KeepSeparate {
            left_asset_id,
            right_asset_id,
            ..
        }
        | AgentCatalogRelationIntent::RestoreHint {
            left_asset_id,
            right_asset_id,
        } => pair_key(left_asset_id, right_asset_id),
        AgentCatalogRelationIntent::Merge {
            destination_asset_id,
            source_asset_id,
        } => format!("merge:{destination_asset_id}:{source_asset_id}"),
        AgentCatalogRelationIntent::Detach { association_id } => format!("detach:{association_id}"),
    }
}
fn action(intent: &AgentCatalogRelationIntent) -> AgentCatalogRelationAction {
    match intent {
        AgentCatalogRelationIntent::Compare { .. } => AgentCatalogRelationAction::Compare,
        AgentCatalogRelationIntent::Merge { .. } => AgentCatalogRelationAction::Merge,
        AgentCatalogRelationIntent::KeepSeparate { .. } => AgentCatalogRelationAction::KeepSeparate,
        AgentCatalogRelationIntent::Detach { .. } => AgentCatalogRelationAction::Detach,
        AgentCatalogRelationIntent::RestoreHint { .. } => AgentCatalogRelationAction::RestoreHint,
    }
}
