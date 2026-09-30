//! Durable recovery copies and observation-only reconciliation for native Hooks.
//!
//! A pending intent is written before any native file. Refresh may settle an
//! intent only from complete before/after evidence; it never replays a write.
use super::{
    hook_definition,
    native::hooks::{HookNativeDefinition, HookNativeDestination, HookNativeRule},
    observation::DefinitionReader,
    package,
    repository::{Library, Receipt},
};
use crate::{
    models::*,
    services::agent_cli::environment::mutation::{GuardedFile, MutationInventory},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct HookObservation {
    pub context_id: String,
    pub source_id: String,
    pub scope: AgentAssetScope,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum HookBindingState {
    Active,
    Suspended,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct HookReceipt {
    pub destination: HookNativeDestination,
    /// The complete original group remains intact while suspended, even when
    /// a newer shared definition is applied to the retained desired value.
    pub rule: HookNativeRule,
    pub desired: HookNativeDefinition,
    pub state: HookBindingState,
    pub pending: Option<HookIntent>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct HookFileEvidence {
    pub source_id: Option<String>,
    pub root: String,
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct HookIntent {
    pub id: String,
    /// Nested snapshots never themselves contain a pending intent.
    pub before: Option<Box<Receipt>>,
    pub after: Option<Box<Receipt>>,
    pub files: Vec<HookFileEvidence>,
}

pub(super) enum HookPresence {
    Active,
    Missing,
    Suspended,
    Unknown,
}

pub(super) fn reconcile(library: &mut Library, reader: &mut DefinitionReader<'_>) {
    let snapshot = reader.snapshot;
    for entry in library
        .entries
        .values_mut()
        .filter(|entry| entry.category == AgentAssetCategory::Hook)
    {
        let mut settled = Vec::new();
        for (id, receipt) in &entry.receipts {
            let Some(intent) = receipt.hook.as_ref().and_then(|hook| hook.pending.as_ref()) else {
                continue;
            };
            if intent.files.is_empty() {
                continue;
            }
            let facts = intent
                .files
                .iter()
                .map(|file| observe_file(snapshot, file))
                .collect::<Vec<_>>();
            if facts.iter().zip(&intent.files).all(|(observed, file)| {
                observed
                    .as_ref()
                    .is_ok_and(|digest| digest.as_deref() == Some(file.after.as_str()))
            }) {
                settled.push((id.clone(), intent.after.as_deref().cloned()));
            } else if facts.iter().zip(&intent.files).all(|(observed, file)| {
                observed.as_ref().is_ok_and(|digest| digest == &file.before)
            }) {
                settled.push((id.clone(), intent.before.as_deref().cloned()));
            }
        }
        for (id, receipt) in settled {
            if let Some(receipt) = receipt {
                entry.receipts.insert(id, receipt);
            } else {
                entry.receipts.remove(&id);
            }
        }
        for receipt in entry.receipts.values_mut() {
            let observed = receipt
                .hook
                .as_ref()
                .filter(|hook| hook.pending.is_none())
                .map(|_| observe_receipt_rules(reader, receipt));
            let Some(hook) = &mut receipt.hook else {
                continue;
            };
            hook.rule.native_asset_id = None;
            let Some(Ok(rules)) = observed else {
                continue;
            };
            let mut matches = rules
                .rules
                .iter()
                .filter(|rule| rule.definition == hook.desired);
            if let Some(rule) = matches.next() {
                if matches.next().is_none() {
                    hook.rule = rule.clone();
                    // An external writer can restore a suspended definition.
                    // Observe its complete identity and current anchor without
                    // replaying the saved operation or trusting its old index.
                    hook.state = HookBindingState::Active;
                }
            }
        }
    }
    rebind_aliases(library, reader);
}

fn observe_file(
    snapshot: &MutationInventory,
    evidence: &HookFileEvidence,
) -> Result<Option<String>, String> {
    let file = if let Some(id) = &evidence.source_id {
        let source = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| {
                &source.id == id
                    && source.path == evidence.path
                    && source.allowed_root == evidence.root
            })
            .ok_or("Hook 提交来源暂时不可观察")?;
        GuardedFile::capture(source, &snapshot.source_anchors).map_err(|_| "Hook 提交来源已变化")?
    } else {
        // Only app-private auxiliary ownership files use this branch. The
        // caller stored their verified root before committing the intent.
        package::capture_file(
            Path::new(&evidence.root),
            Path::new(&evidence.path),
            package::MAX_FILE_BYTES,
        )?
    };
    Ok(file.bytes().map(super::digest))
}

fn rebind_aliases(library: &mut Library, reader: &mut DefinitionReader<'_>) {
    let snapshot = reader.snapshot;
    let mut observed = Vec::new();
    for asset in snapshot
        .inventory
        .assets
        .iter()
        .filter(|asset| asset.category == AgentAssetCategory::Hook)
    {
        if let Ok(rule) = reader.hook(asset) {
            observed.push((asset, hook_definition::fingerprint(&rule.definition)));
        }
    }
    for entry in library
        .entries
        .values_mut()
        .filter(|entry| entry.category == AgentAssetCategory::Hook)
    {
        let previous = std::mem::take(&mut entry.aliases);
        for (id, mut observation) in previous {
            let Some(source) = &observation.hook_source else {
                continue;
            };
            let candidates = observed
                .iter()
                .filter(|(asset, fingerprint)| {
                    asset.context_id == source.context_id
                        && asset.inspection_source_id == source.source_id
                        && asset.scope == source.scope
                        && observation.fingerprint.as_ref() == Some(fingerprint)
                })
                .collect::<Vec<_>>();
            let candidate = match candidates.as_slice() {
                [single] => Some(single.0),
                _ if entry.current().is_none() => candidates
                    .iter()
                    .find(|(asset, _)| asset.stable_id == id)
                    .map(|(asset, _)| *asset),
                _ => None,
            };
            if let Some(asset) = candidate {
                observation.physical_key = reader.physical_key(asset);
                entry.aliases.insert(asset.stable_id.clone(), observation);
            }
        }
        // A receipt binds only after unique exact-content re-identification.
        // Never keep a previous array slot as an alias for its new occupant.
        for receipt in entry.receipts.values() {
            let Some(hook) = &receipt.hook else {
                continue;
            };
            if hook.pending.is_some() || hook.state != HookBindingState::Active {
                continue;
            }
            let Some(id) = &hook.rule.native_asset_id else {
                continue;
            };
            if let Some((asset, fingerprint)) =
                observed.iter().find(|(asset, _)| &asset.stable_id == id)
            {
                entry.aliases.insert(
                    id.clone(),
                    super::repository::Observation {
                        fingerprint: Some(fingerprint.clone()),
                        physical_key: reader.physical_key(asset),
                        hook_source: Some(HookObservation {
                            context_id: asset.context_id.clone(),
                            source_id: asset.inspection_source_id.clone(),
                            scope: asset.scope,
                        }),
                    },
                );
            }
        }
    }
}

pub(super) fn presence(reader: &mut DefinitionReader<'_>, receipt: &Receipt) -> HookPresence {
    let Some(hook) = &receipt.hook else {
        return HookPresence::Unknown;
    };
    if hook.pending.is_some() {
        return HookPresence::Unknown;
    }
    if hook.state == HookBindingState::Active && hook.rule.native_asset_id.is_some() {
        return HookPresence::Active;
    }
    match observe_receipt_rules(reader, receipt) {
        Ok(rules)
            if !rules
                .rules
                .iter()
                .any(|rule| rule.definition == hook.desired) =>
        {
            if hook.state == HookBindingState::Suspended {
                HookPresence::Suspended
            } else {
                HookPresence::Missing
            }
        }
        _ => HookPresence::Unknown,
    }
}

fn observe_receipt_rules(
    reader: &mut DefinitionReader<'_>,
    receipt: &Receipt,
) -> Result<std::sync::Arc<super::observation::HookSource>, String> {
    let snapshot = reader.snapshot;
    let hook = receipt.hook.as_ref().ok_or("Hook 恢复记录无效")?;
    let Some(source) = snapshot.inventory.sources.iter().find(|source| {
        source.id == hook.rule.source_id
            && source.context_id == receipt.context_id
            && source.path == receipt.path
            && source.scope == receipt.scope
    }) else {
        return Err("Hook 恢复来源当前不可观察".to_owned());
    };
    reader.hook_source(receipt.agent_kind, &source.id)
}
