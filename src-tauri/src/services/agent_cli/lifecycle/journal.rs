//! Durable task facts only. Restart recovery never restores executable plans.
use crate::models::*;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::OnceLock};

static ROOT: OnceLock<PathBuf> = OnceLock::new();
pub(crate) fn initialize(root: PathBuf) {
    let _ = ROOT.set(root);
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    pub operation: AgentLifecycleOperation,
    pub installer_started: bool,
}

pub(super) struct Journal {
    root: Option<PathBuf>,
}
impl Journal {
    pub fn new() -> Self {
        Self {
            root: ROOT.get().cloned(),
        }
    }
    pub fn save(&self, record: &Record) -> Result<(), String> {
        let Some(root) = &self.root else {
            return if cfg!(test) {
                Ok(())
            } else {
                Err("升级记录目录未初始化".into())
            };
        };
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let name = crate::services::agent_cli::cache::key(record.operation.id.as_bytes());
        let path = root.join(format!("{name}.json"));
        let temporary = root.join(format!("{name}.tmp"));
        let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::File::open(root)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let path = root.join(format!(
            "{}.json",
            crate::services::agent_cli::cache::key(id.as_bytes())
        ));
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn load(&self) -> Result<Vec<Record>, String> {
        let Some(root) = &self.root else {
            return if cfg!(test) {
                Ok(Vec::new())
            } else {
                Err("升级记录目录未初始化".into())
            };
        };
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut records = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 512 * 1024 {
                return Err("升级记录超过大小限制".into());
            }
            let record: Record =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            records.push((path, record));
        }
        records.sort_by(|a, b| b.1.operation.created_at.cmp(&a.1.operation.created_at));
        let mut settled = 0;
        let mut kept = Vec::new();
        for (path, record) in records {
            if record.operation.outcome.is_some() {
                settled += 1;
            }
            if record.operation.outcome.is_some() && settled > 128 {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            } else {
                kept.push(record);
            }
        }
        Ok(kept)
    }
}
