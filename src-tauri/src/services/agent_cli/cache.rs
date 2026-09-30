//! Disposable display/discovery caches. Native files remain the source of truth.
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::UNIX_EPOCH};

pub(crate) fn key(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(crate) fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("缓存目录不可用")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

pub(super) fn observe_environment(digest: &mut Sha256, kind: crate::models::AgentCliKind) {
    let prefix = format!("{}_", kind.key().to_ascii_uppercase());
    let variables = std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            matches!(
                name.as_ref(),
                "HOME" | "USERPROFILE" | "PATH" | "PATHEXT" | "NODE_OPTIONS" | "NODE_PATH"
            ) || name.starts_with("XDG_")
                || name.starts_with(prefix.as_str())
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for (name, value) in variables {
        digest.update(name.as_encoded_bytes());
        digest.update([0]);
        digest.update(value.as_encoded_bytes());
        digest.update([0]);
    }
}

/// Only stat known paths; do not read configuration/session contents here.
pub(super) fn observe_path(digest: &mut Sha256, path: &Path) {
    observe_path_metadata(digest, path, true);
}

/// Ancestor directory identity matters; unrelated child writes do not.
pub(super) fn observe_path_identity(digest: &mut Sha256, path: &Path) {
    observe_path_metadata(digest, path, false);
}

fn observe_path_metadata(digest: &mut Sha256, path: &Path, contents: bool) {
    digest.update(path.as_os_str().as_encoded_bytes());
    digest.update([0]);
    for metadata in [fs::symlink_metadata(path), fs::metadata(path)] {
        match metadata {
            Ok(metadata) => {
                digest.update([
                    1,
                    u8::from(metadata.is_dir()),
                    u8::from(metadata.permissions().readonly()),
                ]);
                if contents {
                    digest.update(metadata.len().to_le_bytes());
                }
                let observed_time = if contents {
                    metadata.modified()
                } else {
                    metadata.created()
                };
                if let Ok(modified) = observed_time.and_then(|time| {
                    time.duration_since(UNIX_EPOCH)
                        .map_err(std::io::Error::other)
                }) {
                    digest.update(modified.as_nanos().to_le_bytes());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    digest.update(metadata.dev().to_le_bytes());
                    digest.update(metadata.ino().to_le_bytes());
                    digest.update(metadata.mode().to_le_bytes());
                    if contents {
                        digest.update(metadata.ctime().to_le_bytes());
                        digest.update(metadata.ctime_nsec().to_le_bytes());
                    }
                }
            }
            Err(error) => {
                digest.update([0]);
                digest.update(format!("{:?}", error.kind()).as_bytes());
            }
        }
    }
    if let Ok(target) = fs::read_link(path) {
        digest.update(target.as_os_str().as_encoded_bytes());
    }
}

pub(super) fn include_path(
    paths: &mut std::collections::BTreeSet<std::path::PathBuf>,
    path: &Path,
    root: Option<&Path>,
) {
    paths.insert(path.to_path_buf());
    if let Some(root) = root.filter(|root| !root.as_os_str().is_empty() && path.starts_with(root)) {
        for parent in path.ancestors().skip(1) {
            if !parent.starts_with(root) {
                break;
            }
            paths.insert(parent.to_path_buf());
        }
    } else if let Some(parent) = path.parent() {
        paths.insert(parent.to_path_buf());
    }
}
