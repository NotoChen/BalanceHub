//! Disposable list metadata only: no native contents, access grants or path authority.
use super::{inputs::CatalogInputs, CatalogService};
use crate::{models::*, services::agent_cli::cache};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 9;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 8;

#[derive(Serialize, Deserialize)]
pub(crate) struct DisplaySnapshot {
    schema: u32,
    signature: String,
    pub(crate) catalog: AgentAssetCatalog,
    pub(crate) inputs: CatalogInputs,
}

impl CatalogService {
    pub(crate) fn restore_display(&self, workspace: Option<&Path>) -> Option<DisplaySnapshot> {
        let path = self.display_path(workspace)?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        let snapshot: DisplaySnapshot = serde_json::from_slice(&bytes).ok()?;
        (snapshot.schema == SCHEMA
            && snapshot
                .catalog
                .inventory
                .workspace
                .as_deref()
                .map(Path::new)
                == workspace
            && snapshot.inputs.cacheable())
        .then_some(snapshot)
    }

    pub(crate) fn resume_display(
        &self,
        snapshot: &mut DisplaySnapshot,
        settings: &AppSettings,
    ) -> bool {
        if !snapshot.inputs.is_current(settings) {
            return false;
        }
        let Ok(mut revisions) = self.revisions.lock() else {
            return false;
        };
        let current = revisions
            .entry(
                snapshot
                    .catalog
                    .inventory
                    .workspace
                    .clone()
                    .unwrap_or_default(),
            )
            .or_insert_with(|| {
                (
                    snapshot.signature.clone(),
                    snapshot.catalog.revision.clone(),
                )
            });
        if current.0 != snapshot.signature {
            return false;
        }
        // A fresh mutation projection must compare against the same semantic
        // signature; restoring a list does not manufacture a new revision.
        snapshot.catalog.revision = current.1.clone();
        true
    }

    pub(crate) fn persist_display(&self, catalog: &AgentAssetCatalog, inputs: &CatalogInputs) {
        if !inputs.cacheable() {
            return;
        }
        let Some(path) = self.display_path(catalog.inventory.workspace.as_deref().map(Path::new))
        else {
            return;
        };
        let signature = self.revisions.lock().ok().and_then(|revisions| {
            revisions
                .get(&catalog.inventory.workspace.clone().unwrap_or_default())
                .filter(|(_, revision)| revision == &catalog.revision)
                .map(|(signature, _)| signature.clone())
        });
        let Some(signature) = signature else {
            return;
        };
        let mut catalog = catalog.clone();
        for source in &mut catalog.inventory.sources {
            source.access = AgentAssetAccess::default();
        }
        for asset in &mut catalog.inventory.assets {
            asset.access = AgentAssetAccess::default();
        }
        for binding in catalog
            .assets
            .iter_mut()
            .flat_map(|asset| &mut asset.bindings)
        {
            binding.native.access = AgentAssetAccess::default();
        }
        let snapshot = DisplaySnapshot {
            schema: SCHEMA,
            signature,
            catalog,
            inputs: inputs.clone(),
        };
        // Cache failure must never turn a successful inventory into a UI error.
        let _ = save(&path, &snapshot);
    }

    fn display_path(&self, workspace: Option<&Path>) -> Option<PathBuf> {
        let key = cache::key(workspace.map_or(b"native".as_slice(), |path| {
            path.as_os_str().as_encoded_bytes()
        }));
        Some(
            self.display_cache_root
                .as_ref()?
                .join(format!("{key}.json")),
        )
    }
}

fn save(path: &Path, snapshot: &DisplaySnapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Ok(());
    }
    let parent = path.parent().ok_or("资产缓存目录不可用")?;
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory
        .create(parent)
        .map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", super::opaque_id()?));
    let result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())?;
    let mut entries = fs::read_dir(parent)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.len() == 69
                && name.ends_with(".json")
                && name.as_bytes()[..64]
                    .iter()
                    .all(|byte| byte.is_ascii_hexdigit())
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(modified, _)| *modified);
    let remove = entries.len().saturating_sub(MAX_SNAPSHOTS);
    for (_, old) in entries.into_iter().take(remove) {
        let _ = fs::remove_file(old);
    }
    Ok(())
}
