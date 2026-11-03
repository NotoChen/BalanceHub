use super::format::{Baseline, Manifest, SyncDocuments};
use crate::models::{CloudSyncChange, CloudSyncResolution, CloudSyncSide};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Merge {
    pub documents: SyncDocuments,
    pub changes: Vec<CloudSyncChange>,
    pub unresolved: bool,
}

/// Three-way merge of portable records. A deletion is compared against the
/// persisted common ancestor, so offline devices cannot resurrect old data.
pub(super) fn merge(
    baseline: &Baseline,
    local: &SyncDocuments,
    remote: &SyncDocuments,
    manifest: &Manifest,
    resolutions: &[CloudSyncResolution],
) -> Result<Merge, String> {
    let choices: BTreeMap<_, _> = resolutions
        .iter()
        .map(|v| (v.key.as_str(), v.side))
        .collect();
    if choices.len() != resolutions.len() {
        return Err("同一项不能重复选择处理方式".to_owned());
    }
    let keys: BTreeSet<_> = local
        .keys()
        .chain(remote.keys())
        .chain(baseline.entries.keys())
        .chain(manifest.entries.keys())
        .cloned()
        .collect();
    let mut result = Merge {
        documents: BTreeMap::new(),
        changes: Vec::new(),
        unresolved: false,
    };
    for key in keys {
        if key.starts_with("blob/") {
            continue;
        }
        let left = local.get(&key);
        let right = remote.get(&key);
        let left_hash = left.map(|v| v.hash()).transpose()?;
        let right_hash = right.map(|v| v.hash()).transpose()?;
        let base_hash = baseline
            .entries
            .get(&key)
            .filter(|v| v.object.is_some())
            .map(|v| v.hash.clone());
        // On first connection, an absent record is not a local deletion.
        let left_changed = if baseline.initialized {
            left_hash != base_hash
        } else {
            left.is_some()
        };
        let right_changed = if baseline.initialized {
            right_hash != base_hash
                || (!baseline.entries.contains_key(&key)
                    && manifest
                        .entries
                        .get(&key)
                        .is_some_and(|entry| entry.object.is_none()))
        } else {
            manifest.entries.contains_key(&key)
        };
        let both_changed = left_hash != right_hash && left_changed && right_changed;
        let combined = if both_changed && key == "order/providers" {
            combine_order(left, right)
        } else {
            None
        };
        let conflict = both_changed && combined.is_none();
        let selected = if let Some(value) = &combined {
            Some(value)
        } else if conflict {
            match choices.get(key.as_str()) {
                Some(CloudSyncSide::Local) => left,
                Some(CloudSyncSide::Remote) => right,
                None => {
                    result.unresolved = true;
                    left
                }
            }
        } else if left_hash == right_hash || left_changed {
            left
        } else {
            right
        };
        if let Some(value) = selected {
            result.documents.insert(key.clone(), value.clone());
        }
        if left_hash != right_hash {
            let metadata = left.or(right);
            let historical = baseline
                .entries
                .get(&key)
                .or_else(|| manifest.entries.get(&key));
            result.changes.push(CloudSyncChange {
                key: key.clone(),
                title: metadata
                    .map(|v| v.title.clone())
                    .or_else(|| historical.map(|v| v.title.clone()))
                    .unwrap_or_else(|| key.clone()),
                category: metadata
                    .map(|v| v.category.clone())
                    .or_else(|| historical.map(|v| v.category.clone()))
                    .unwrap_or_default(),
                conflict,
                upload: conflict || combined.is_some() || left_changed,
                download: conflict || combined.is_some() || !left_changed,
                local_deleted: left.is_none(),
                remote_deleted: right.is_none(),
            });
        }
    }
    // Files are content-addressed attachments. Merge the owning asset as one
    // configuration, then retain exactly the files referenced by that choice.
    let mut attachments = BTreeSet::new();
    for (key, document) in &result.documents {
        if key.starts_with("asset/") {
            if let Some(files) = document
                .value
                .get("files")
                .and_then(|value| value.as_object())
            {
                for file in files.values() {
                    attachments.insert(
                        file.get("blob")
                            .and_then(|value| value.as_str())
                            .ok_or("共享资源引用无效")?
                            .to_owned(),
                    );
                }
            }
        }
    }
    for key in attachments {
        if !key.starts_with("blob/") {
            return Err("共享资源引用无效".to_owned());
        }
        let document = local
            .get(&key)
            .or_else(|| remote.get(&key))
            .ok_or("共享资源文件缺失，未应用同步数据")?;
        result.documents.insert(key, document.clone());
    }
    for key in choices.keys() {
        if !result
            .changes
            .iter()
            .any(|change| change.key == *key && change.conflict)
        {
            return Err("冲突列表已变化，请重新查看同步预览".to_owned());
        }
    }
    Ok(result)
}

/// Independent additions should not create an artificial sorting conflict.
/// Preserve both sequences when common items still have the same order.
fn combine_order(
    left: Option<&super::format::SyncDocument>,
    right: Option<&super::format::SyncDocument>,
) -> Option<super::format::SyncDocument> {
    let (left, right) = (left?, right?);
    let a: Vec<String> = serde_json::from_value(left.value.clone()).ok()?;
    let b: Vec<String> = serde_json::from_value(right.value.clone()).ok()?;
    let a_set: BTreeSet<_> = a.iter().collect();
    let b_set: BTreeSet<_> = b.iter().collect();
    let common_a: Vec<_> = a.iter().filter(|id| b_set.contains(id)).collect();
    let common_b: Vec<_> = b.iter().filter(|id| a_set.contains(id)).collect();
    if common_a != common_b {
        return None;
    }
    let mut a_iter = a.iter().peekable();
    let mut b_iter = b.iter().peekable();
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |id: &String| {
        if seen.insert(id.clone()) {
            order.push(id.clone());
        }
    };
    for anchor in common_a {
        while let Some(id) = a_iter.next_if(|id| *id != anchor) {
            push(id);
        }
        while let Some(id) = b_iter.next_if(|id| *id != anchor) {
            push(id);
        }
        a_iter.next();
        b_iter.next();
        push(anchor);
    }
    for id in a_iter.chain(b_iter) {
        push(id);
    }
    Some(super::format::SyncDocument {
        value: serde_json::json!(order),
        ..left.clone()
    })
}
