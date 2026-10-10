mod persistence;

use super::{
    definition::{DefinitionPayload, StoredDefinition},
    opaque_id, package,
};
use crate::models::*;
use crate::services::agent_cli::environment::{
    mutation::GuardedFile, verified_path::reopen_verified_path,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(super) const LIBRARY_LIMIT: usize = 64 * 1024 * 1024;
pub(super) const MAX_LIBRARY_ENTRIES: usize = 4096;
const RETAINED_DEFINITION_VERSIONS: usize = 32;

#[derive(Default, Clone, Serialize, Deserialize)]
pub(super) struct Library {
    pub entries: BTreeMap<String, Entry>,
    #[serde(default)]
    pub relations: super::relations::CatalogRelations,
}

pub(super) struct LibrarySnapshot {
    pub library: Library,
    pub guard: GuardedFile,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Entry {
    pub name: String,
    pub category: AgentAssetCategory,
    pub aliases: BTreeMap<String, Observation>,
    pub variants: BTreeMap<String, String>,
    pub versions: Vec<StoredDefinition>,
    pub receipts: BTreeMap<String, Receipt>,
    /// Cloud deletion removes the shared definition without uninstalling any
    /// local binding or discarding the historical receipt recovery material.
    #[serde(default)]
    pub shared_deleted: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Observation {
    pub fingerprint: Option<String>,
    pub physical_key: Option<String>,
    pub hook_source: Option<super::hook_receipts::HookObservation>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Receipt {
    pub context_id: String,
    pub scope: AgentAssetScope,
    pub agent_kind: AgentCliKind,
    pub path: String,
    pub name: String,
    pub version: u64,
    pub fingerprint: String,
    pub hook: Option<super::hook_receipts::HookReceipt>,
}

pub(super) struct DistributionSource {
    pub name: String,
    pub definition: StoredDefinition,
    pub receipts: BTreeMap<String, Receipt>,
}

impl Entry {
    pub fn new(name: String, category: AgentAssetCategory) -> Self {
        Self {
            name,
            category,
            aliases: BTreeMap::new(),
            variants: BTreeMap::new(),
            versions: Vec::new(),
            receipts: BTreeMap::new(),
            shared_deleted: false,
        }
    }
    pub fn current(&self) -> Option<&StoredDefinition> {
        if self.shared_deleted {
            None
        } else {
            self.versions.last()
        }
    }
    pub fn push_version(&mut self, value: StoredDefinition) -> Result<(), String> {
        if self
            .versions
            .last()
            .is_some_and(|current| value.version <= current.version)
        {
            return Err("共享版本必须递增".to_owned());
        }
        self.versions.push(value);
        let recent_start = self
            .versions
            .len()
            .saturating_sub(RETAINED_DEFINITION_VERSIONS);
        let mut index = 0;
        self.versions.retain(|version| {
            let keep = index >= recent_start
                || self
                    .receipts
                    .values()
                    .any(|receipt| receipt.version == version.version);
            index += 1;
            keep
        });
        Ok(())
    }
    pub fn set_definition(
        &mut self,
        name: &str,
        payload: DefinitionPayload,
    ) -> Result<StoredDefinition, String> {
        if self.name == name {
            if let Some(current) = self
                .current()
                .filter(|current| current.payload.fingerprint() == payload.fingerprint())
            {
                return Ok(current.clone());
            }
        }
        let version = self
            .versions
            .last()
            .map_or(0, |current| current.version)
            .checked_add(1)
            .ok_or("共享版本已达到上限")?;
        let value = StoredDefinition { version, payload };
        self.push_version(value.clone())?;
        self.shared_deleted = false;
        name.clone_into(&mut self.name);
        Ok(value)
    }
    pub fn variant(&mut self, fingerprint: &str) -> Result<String, String> {
        if let Some(id) = self.variants.get(fingerprint) {
            return Ok(id.clone());
        }
        let id = opaque_id()?;
        self.variants.insert(fingerprint.to_owned(), id.clone());
        Ok(id)
    }
}

pub(super) struct Repository {
    root: PathBuf,
    lock: Mutex<()>,
    entry_cache: Mutex<Option<LibrarySnapshot>>,
    sync_signal: Option<std::sync::Arc<crate::services::cloud_sync::SyncSignal>>,
    #[cfg(test)]
    persistence_fault: Mutex<Option<PersistenceFault>>,
    #[cfg(test)]
    failure_after_save: Mutex<bool>,
}

#[cfg(test)]
type PersistenceFault = fn(&[u8], &[u8]) -> bool;

impl Repository {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: Mutex::new(()),
            entry_cache: Mutex::new(None),
            sync_signal: None,
            #[cfg(test)]
            persistence_fault: Mutex::new(None),
            #[cfg(test)]
            failure_after_save: Mutex::new(false),
        }
    }
    pub fn set_sync_signal(
        &mut self,
        signal: std::sync::Arc<crate::services::cloud_sync::SyncSignal>,
    ) {
        self.sync_signal = Some(signal);
    }
    pub fn transact<T>(
        &self,
        action: impl FnOnce(&mut Library) -> Result<T, String>,
    ) -> Result<T, String> {
        self.transact_observed(action).map(|(output, _)| output)
    }

    /// Return the exact persisted input under the same transaction lock. A
    /// catalog's freshness stamp must not observe a later writer's library.
    pub fn transact_observed<T>(
        &self,
        action: impl FnOnce(&mut Library) -> Result<T, String>,
    ) -> Result<(T, GuardedFile), String> {
        self.checkpointed_inner(None, true, |library, _, _| action(library))
    }

    /// Reading a preview must not create the private directory or an empty file.
    pub fn read_snapshot(&self) -> Result<LibrarySnapshot, String> {
        let _lock = self.lock.lock().map_err(|_| "共享库访问失败")?;
        let guard =
            package::capture_file(&self.root, &self.root.join("library.json"), LIBRARY_LIMIT)?;
        let library = Self::decode(&guard)?;
        Ok(LibrarySnapshot { library, guard })
    }

    /// Display reads need one entry, not a clone/parse of every package and
    /// historical version. Write/plan snapshots continue to read live bytes.
    pub fn read_entry<T>(
        &self,
        id: &str,
        read: impl FnOnce(&Entry) -> Result<T, String>,
    ) -> Result<T, String> {
        self.read_view(|library| {
            read(
                library
                    .entries
                    .get(id)
                    .ok_or("该资产已离开共享库，请刷新列表")?,
            )
        })
    }

    /// Related display metadata is read from one validated library, including
    /// libraries above the cache limit. Callers return only the needed fields.
    pub fn read_view<T>(
        &self,
        read: impl FnOnce(&Library) -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = self.lock.lock().map_err(|_| "共享库访问失败")?;
        let mut cache = self.entry_cache.lock().map_err(|_| "共享库读取状态失效")?;
        if let Some(snapshot) = cache.as_ref().filter(|snapshot| {
            snapshot
                .guard
                .verified_anchor()
                .is_some_and(|anchor| reopen_verified_path(anchor).is_ok())
        }) {
            return read(&snapshot.library);
        }
        *cache = None;
        let guard =
            package::capture_file(&self.root, &self.root.join("library.json"), LIBRARY_LIMIT)?;
        let library = Self::decode(&guard)?;
        let output = read(&library);
        if guard
            .bytes()
            .is_some_and(|bytes| bytes.len() <= 8 * 1024 * 1024)
        {
            *cache = Some(LibrarySnapshot { library, guard });
        }
        output
    }

    fn decode(file: &GuardedFile) -> Result<Library, String> {
        match file.bytes() {
            Some(bytes) => serde_json::from_slice(bytes)
                .map_err(|_| "共享库损坏，已保留原文件，未覆盖".to_owned()),
            None => Ok(Library::default()),
        }
    }

    pub fn guarded_transact<T>(
        &self,
        expected: &GuardedFile,
        action: impl FnOnce(&mut Library) -> Result<T, String>,
    ) -> Result<T, String> {
        self.checkpointed_inner(Some(expected), true, |library, _, _| action(library))
            .map(|(output, _)| output)
    }

    /// Hold the library lock across a native operation while making its
    /// recovery intent durable first. An error after a checkpoint deliberately
    /// leaves that checkpoint on disk for observation-only reconciliation.
    pub fn checkpointed<T>(
        &self,
        action: impl FnOnce(
            &mut Library,
            &mut dyn FnMut(&Library) -> Result<(), String>,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        self.checkpointed_inner(None, true, |library, _, checkpoint| {
            action(library, checkpoint)
        })
        .map(|(output, _)| output)
    }

    /// The coordinator explicitly publishes and rolls back while the library
    /// lock is held. Never perform a second implicit save after it commits.
    pub fn checkpointed_with_original<T>(
        &self,
        action: impl FnOnce(
            &mut Library,
            Option<&[u8]>,
            &mut dyn FnMut(&Library) -> Result<(), String>,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        self.checkpointed_inner(None, false, action)
            .map(|(output, _)| output)
    }

    fn checkpointed_inner<T>(
        &self,
        expected: Option<&GuardedFile>,
        save_at_end: bool,
        action: impl FnOnce(
            &mut Library,
            Option<&[u8]>,
            &mut dyn FnMut(&Library) -> Result<(), String>,
        ) -> Result<T, String>,
    ) -> Result<(T, GuardedFile), String> {
        let _lock = self.lock.lock().map_err(|_| "共享库访问失败")?;
        if let Some(expected) = expected {
            expected
                .revalidate()
                .map_err(|_| "共享库在预览后已变化，请重新计划")?;
        }
        let parent = self.root.parent().ok_or("共享库目录无效")?;
        package::ensure_directory(parent, &self.root)?;
        let path = self.root.join("library.json");
        let mut file = package::capture_file(&self.root, &path, LIBRARY_LIMIT)?;
        if expected.is_some_and(|expected| expected.bytes() != file.bytes()) {
            return Err("共享库在提交前已变化，未覆盖当前内容".to_owned());
        }
        let mut library = Self::decode(&file)?;
        let original = file.bytes().map(<[u8]>::to_vec);
        let mut before = serde_json::to_vec(&library).map_err(|_| "共享库格式无效")?;
        let mut checkpoint = |library: &Library| -> Result<(), String> {
            file.revalidate()
                .map_err(|_| "共享库在事务期间被外部修改")?;
            if library.entries.len() > MAX_LIBRARY_ENTRIES {
                return Err("共享库超过 4096 项上限".to_owned());
            }
            let after = serde_json::to_vec(library).map_err(|_| "共享库格式无效")?;
            if after.len() > LIBRARY_LIMIT {
                return Err("共享库超过 64 MiB，未保存此次变更".to_owned());
            }
            if before != after || file.bytes().is_none() {
                #[cfg(test)]
                {
                    let mut fault = self
                        .persistence_fault
                        .lock()
                        .map_err(|_| "共享库测试存储失败")?;
                    if fault.is_some_and(|probe| probe(&before, &after)) {
                        *fault = None;
                        return Err("共享库测试原子保存失败".to_owned());
                    }
                }
                persistence::save(&file, &after)?;
                if let Some(signal) = &self.sync_signal {
                    signal.changed();
                }
                #[cfg(test)]
                if std::mem::take(
                    &mut *self
                        .failure_after_save
                        .lock()
                        .map_err(|_| "共享库测试存储失败")?,
                ) {
                    return Err("共享库测试替换后确认失败".to_owned());
                }
                file = package::capture_file(&self.root, &path, LIBRARY_LIMIT)?;
                if file.bytes() != Some(after.as_slice()) {
                    return Err("共享库在原子保存后被外部修改".to_owned());
                }
                before = after;
            }
            Ok(())
        };
        let output = action(&mut library, original.as_deref(), &mut checkpoint)?;
        if save_at_end {
            checkpoint(&library)?;
        }
        Ok((output, file))
    }
    #[cfg(all(test, unix))]
    pub(super) fn inject_persistence_fault(&self, fault: PersistenceFault) {
        *self.persistence_fault.lock().unwrap() = Some(fault);
    }
    #[cfg(all(test, unix))]
    pub(super) fn inject_failure_after_save(&self) {
        *self.failure_after_save.lock().unwrap() = true;
    }
    pub fn current(&self, id: &str) -> Result<(String, StoredDefinition), String> {
        self.read_entry(id, |entry| {
            Ok((
                entry.name.clone(),
                entry.current().cloned().ok_or("该资产尚未收录")?,
            ))
        })
    }

    pub fn distribution_source(&self, id: &str) -> Result<DistributionSource, String> {
        let snapshot = self.read_snapshot()?;
        let entry = snapshot.library.entries.get(id).ok_or("共享定义不存在")?;
        Ok(DistributionSource {
            name: entry.name.clone(),
            definition: entry.current().cloned().ok_or("该资产尚未收录")?,
            receipts: entry.receipts.clone(),
        })
    }
}

pub(super) fn receipt_matches(
    receipt: &Receipt,
    asset: &AgentAssetRecord,
    source_path: Option<&str>,
) -> bool {
    if let Some(hook) = &receipt.hook {
        return asset.category == AgentAssetCategory::Hook
            && hook.pending.is_none()
            && hook.state == super::hook_receipts::HookBindingState::Active
            && hook.rule.native_asset_id.as_deref() == Some(asset.stable_id.as_str())
            && hook.rule.source_id == asset.inspection_source_id
            && receipt.context_id == asset.context_id
            && receipt.scope == asset.scope
            && receipt.agent_kind == asset.agent_kind
            && source_path == Some(receipt.path.as_str());
    }
    if asset.category == AgentAssetCategory::Hook {
        return false;
    }
    receipt.context_id == asset.context_id
        && receipt.scope == asset.scope
        && receipt.agent_kind == asset.agent_kind
        && (source_path == Some(receipt.path.as_str())
            || asset
                .path
                .as_ref()
                .is_some_and(|path| Path::new(path).starts_with(&receipt.path)))
        && (receipt.name == asset.native_id || asset.category == AgentAssetCategory::Skill)
}
