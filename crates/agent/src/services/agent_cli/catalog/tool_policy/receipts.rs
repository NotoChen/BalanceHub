//! Device-local ownership. These receipts never travel with shared definitions.
use crate::services::agent_cli::{
    config_support::{directories, file_writer::StagedFile},
    environment::mutation::GuardedFile,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};

const LIMIT: usize = 4 * 1024 * 1024;
#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Receipt {
    pub owned: Vec<String>,
    pub pending: Option<Intent>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Intent {
    pub before: Option<String>,
    pub after: String,
    pub owned: Vec<String>,
}
pub(in crate::services::agent_cli::catalog) struct Store {
    root: PathBuf,
    lock: Mutex<()>,
}
impl Receipt {
    pub(super) fn reconciled(&self, current_hash: Option<&str>) -> Self {
        let Some(pending) = &self.pending else {
            return self.clone();
        };
        // On ambiguous external changes, newly requested rules are never
        // claimed as ours. Previously owned rules remain removable.
        Self {
            owned: if current_hash == Some(&pending.after) {
                pending.owned.clone()
            } else {
                self.owned.clone()
            },
            pending: None,
        }
    }
}
impl Store {
    pub(in crate::services::agent_cli::catalog) fn new(root: PathBuf) -> Self {
        Self {
            root: root.join("mcp-tool-rules"),
            lock: Mutex::new(()),
        }
    }
    pub(super) fn read(&self, key: &str) -> Result<Receipt, String> {
        let _lock = self.lock.lock().map_err(|_| "工具管理记录不可用")?;
        let file = self.file(key)?;
        decode(&file)
    }
    pub(super) fn replace(
        &self,
        key: &str,
        expected: &Receipt,
        desired: &Receipt,
    ) -> Result<(), String> {
        let _lock = self.lock.lock().map_err(|_| "工具管理记录不可用")?;
        let parent = self.root.parent().ok_or("工具管理目录无效")?;
        let base = parent.parent().ok_or("工具管理目录无效")?;
        directories::ensure_directory(base, &self.root)?;
        let file = self.file(key)?;
        if &decode(&file)? != expected {
            return Err("工具管理记录已变化，请重新预览".to_owned());
        }
        let bytes = serde_json::to_vec(desired).map_err(|_| "工具管理记录编码失败")?;
        if bytes.len() > LIMIT {
            return Err("工具规则数量超过存储预算".to_owned());
        }
        let mut stage =
            StagedFile::prepare(&file, &bytes, || Ok(())).map_err(|error| error.message)?;
        stage.commit(|| Ok(())).map_err(|error| error.message)?;
        stage.verify().map_err(|error| error.message)
    }
    fn file(&self, key: &str) -> Result<GuardedFile, String> {
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("工具管理标识无效".to_owned());
        }
        GuardedFile::capture_path(&self.root, &self.root.join(format!("{key}.json")), LIMIT)
            .map_err(|error| error.message)
    }
}
fn decode(file: &GuardedFile) -> Result<Receipt, String> {
    file.bytes()
        .map(serde_json::from_slice)
        .transpose()
        .map(Option::unwrap_or_default)
        .map_err(|_| "工具管理记录无法读取，已保留原有规则".to_owned())
}
