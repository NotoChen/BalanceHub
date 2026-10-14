//! Local pre-change snapshots. Contents are stored once by hash, outside sync
//! payloads. Only an explicit native-operation scope records writes.
use crate::{
    models::AgentHistoryEntry,
    services::agent_cli::environment::{
        mutation::GuardedFile,
        verified_path::{inspect_verified_path, VerifiedPathAnchor},
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

static STORE: OnceLock<Arc<HistoryStore>> = OnceLock::new();
thread_local! { static GROUP: RefCell<Option<(String, String)>> = const { RefCell::new(None) }; }
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn initialize(app_root: &Path, config_root: &Path) -> Result<Arc<HistoryStore>, String> {
    let root = app_root.join("agent-native-history");
    let store = STORE.get_or_init(|| {
        Arc::new(HistoryStore {
            root,
            app_root: app_root.to_path_buf(),
            config_root: config_root.to_path_buf(),
            writing: Mutex::new(()),
        })
    });
    if store.app_root != app_root {
        return Err("原生历史目录与当前应用身份不一致".to_owned());
    }
    Ok(Arc::clone(store))
}

pub(crate) fn store() -> Result<Arc<HistoryStore>, String> {
    STORE
        .get()
        .map(Arc::clone)
        .ok_or_else(|| "本地变更记录未初始化，未修改原生配置".to_owned())
}

pub(crate) fn scope(id: &str, title: &str) -> HistoryScope {
    // Standalone services do not own an application data directory. Desktop
    // setup enables history before exposing native configuration commands.
    if STORE.get().is_none() {
        return HistoryScope { owns: false };
    }
    let previous = GROUP.with(|group| {
        let mut group = group.borrow_mut();
        if group.is_some() {
            return None;
        }
        *group = Some((id.to_owned(), title.to_owned()));
        Some(())
    });
    HistoryScope {
        owns: previous.is_some(),
    }
}
pub(crate) struct HistoryScope {
    owns: bool,
}
impl Drop for HistoryScope {
    fn drop(&mut self) {
        if self.owns {
            GROUP.with(|group| *group.borrow_mut() = None);
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryFile {
    pub path: PathBuf,
    pub root: PathBuf,
    pub kind: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub bytes: u64,
    pub mode: Option<u32>,
    pub link_target: Option<PathBuf>,
    #[serde(default)]
    pub link_junction: bool,
}
#[derive(Serialize, Deserialize)]
struct Record {
    title: String,
    created_at: String,
    file: HistoryFile,
}
pub(crate) struct HistorySnapshot {
    pub entry: AgentHistoryEntry,
    pub files: Vec<HistoryFile>,
}
pub(crate) struct HistoryStore {
    root: PathBuf,
    app_root: PathBuf,
    config_root: PathBuf,
    writing: Mutex<()>,
}

pub(crate) fn record_file(file: &GuardedFile, after: Option<&[u8]>) -> Result<(), String> {
    record(
        file.allowed_root(),
        file.path(),
        "file",
        file.bytes(),
        after,
        None,
    )
}
pub(crate) fn record_removed(
    anchor: &VerifiedPathAnchor,
    bytes: Option<&[u8]>,
) -> Result<(), String> {
    record(
        anchor.allowed_root(),
        anchor.physical_path(),
        if bytes.is_some() { "file" } else { "directory" },
        bytes,
        None,
        None,
    )
}
pub(crate) fn record_link(
    root: &Path,
    path: &Path,
    target: &Path,
    junction: bool,
) -> Result<(), String> {
    record(
        root,
        path,
        "link",
        None,
        None,
        Some((target.to_path_buf(), junction)),
    )
}
pub(crate) fn record_created_link(
    root: &Path,
    path: &Path,
    target: &Path,
    junction: bool,
) -> Result<(), String> {
    record(
        root,
        path,
        "createdLink",
        None,
        None,
        Some((target.to_path_buf(), junction)),
    )
}

fn record(
    root: &Path,
    path: &Path,
    kind: &str,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    link: Option<(PathBuf, bool)>,
) -> Result<(), String> {
    let Some((id, title)) = GROUP.with(|group| group.borrow().clone()) else {
        return Ok(());
    };
    let store = store()?;
    if store.is_internal(path) {
        return Ok(());
    }
    validate_path(root, path)?;
    let _writing = store.writing.lock().map_err(|_| "历史记录暂不可用")?;
    store.ensure_dirs()?;
    let before_hash = before.map(|bytes| store.save_blob(bytes)).transpose()?;
    let after_hash = after.map(hash);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path)
            .ok()
            .map(|meta| meta.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode = None;
    let record = Record {
        title,
        created_at: chrono::Local::now().to_rfc3339(),
        file: HistoryFile {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
            kind: kind.to_owned(),
            before: before_hash,
            after: after_hash,
            bytes: before.map_or(0, |bytes| bytes.len() as u64),
            mode,
            link_junction: link.as_ref().is_some_and(|(_, junction)| *junction),
            link_target: link.map(|(path, _)| path),
        },
    };
    if !valid_id(&id) {
        return Err("历史记录标识无效".to_owned());
    }
    let directory = store.root.join("operations");
    let guard = inspect_verified_path(
        &[&store.app_root],
        &store.app_root,
        &directory,
        crate::models::AgentAssetSourceKind::Directory,
    )
    .map_err(|_| "历史目录不能安全访问")?;
    let name = format!("{id}.jsonl");
    #[cfg(unix)]
    let mut file = {
        use rustix::fs::{openat, Mode, OFlags};
        std::fs::File::from(
            openat(
                guard.source_handle(),
                name.as_str(),
                OFlags::CREATE
                    | OFlags::APPEND
                    | OFlags::WRONLY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|_| "无法保存变更记录")?,
        )
    };
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory.join(name))
            .map_err(|_| "无法保存变更记录")?;
        if file
            .metadata()
            .map_err(|_| "无法检查变更记录")?
            .file_attributes()
            & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("变更记录不能是重解析点".to_owned());
        }
        file
    };
    let mut bytes = serde_json::to_vec(&record).map_err(|_| "变更记录编码失败")?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "无法保存变更记录，原生操作已停止")?;
    guard.revalidate_identity().map_err(|_| "历史目录已变化")?;
    Ok(())
}

impl HistoryStore {
    fn is_internal(&self, path: &Path) -> bool {
        path.starts_with(&self.app_root) || path.starts_with(&self.config_root)
    }
    fn ensure_dirs(&self) -> Result<(), String> {
        for path in [
            self.root.clone(),
            self.root.join("blobs"),
            self.root.join("operations"),
        ] {
            super::directories::ensure_directory(&self.app_root, &path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| "无法保护本地历史目录")?;
            }
        }
        Ok(())
    }
    fn save_blob(&self, bytes: &[u8]) -> Result<String, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err("文件超过本地快照的单文件内存预算".to_owned());
        }
        let id = hash(bytes);
        let path = self.root.join("blobs").join(&id);
        let source = GuardedFile::capture_path(&self.app_root, &path, MAX_FILE_BYTES)
            .map_err(|_| "历史内容文件不能安全访问")?;
        if let Some(existing) = source.bytes() {
            if existing != bytes {
                return Err("历史内容校验失败".to_owned());
            }
            return Ok(id);
        }
        let mut staged = super::file_writer::StagedFile::prepare(&source, bytes, || Ok(()))
            .map_err(|error| error.message)?;
        staged.commit(|| Ok(())).map_err(|error| error.message)?;
        Ok(id)
    }
    pub(crate) fn blob(&self, id: &str) -> Result<Vec<u8>, String> {
        if !valid_id(id) {
            return Err("历史内容标识无效".to_owned());
        }
        let file = GuardedFile::capture_path(
            &self.app_root,
            &self.root.join("blobs").join(id),
            MAX_FILE_BYTES,
        )
        .map_err(|_| "历史内容无法读取")?;
        let bytes = file.bytes().ok_or("历史内容已缺失")?;
        if hash(bytes) != id {
            return Err("历史内容校验失败，未进行恢复".to_owned());
        }
        Ok(bytes.to_vec())
    }
    pub(crate) fn snapshot(&self, id: &str) -> Result<HistorySnapshot, String> {
        if !valid_id(id) {
            return Err("历史记录标识无效".to_owned());
        }
        let path = self.root.join("operations").join(format!("{id}.jsonl"));
        let file = GuardedFile::capture_path(&self.app_root, &path, 32 * 1024 * 1024)
            .map_err(|_| "历史记录无法读取")?;
        let mut records = BTreeMap::new();
        let mut title = String::new();
        let mut created_at = String::new();
        for line in BufReader::new(file.bytes().ok_or("历史记录不存在")?).lines() {
            let line = line.map_err(|_| "历史记录读取失败")?;
            let record: Record =
                serde_json::from_str(&line).map_err(|_| "历史记录损坏，未进行恢复")?;
            validate_path(&record.file.root, &record.file.path)?;
            if self.is_internal(&record.file.path) {
                return Err("历史记录包含应用内部路径".to_owned());
            }
            if title.is_empty() {
                title = record.title;
                created_at = record.created_at;
            }
            records
                .entry(record.file.path.clone())
                .or_insert(record.file);
        }
        let files: Vec<_> = records.into_values().collect();
        let entry = AgentHistoryEntry {
            id: id.to_owned(),
            title,
            created_at,
            file_count: files.len(),
            error: None,
            bytes: files.iter().map(|file| file.bytes).sum(),
            paths: files
                .iter()
                .map(|file| {
                    crate::platform::paths::compatible_path(&file.path)
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
        };
        Ok(HistorySnapshot { entry, files })
    }
    pub(crate) fn list(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<AgentHistoryEntry>, String> {
        let dir = self.root.join("operations");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let guard = inspect_verified_path(
            &[&self.app_root],
            &self.app_root,
            &dir,
            crate::models::AgentAssetSourceKind::Directory,
        )
        .map_err(|_| "历史目录无法读取")?;
        let mut entries = guard
            .read_entries()
            .map_err(|_| "历史记录无法枚举")?
            .filter_map(Result::ok)
            .filter(|entry| {
                !entry.is_symlink && entry.source_kind == crate::models::AgentAssetSourceKind::File
            })
            .filter_map(|entry| {
                let path = dir.join(entry.name);
                let id = path.file_stem()?.to_str()?.to_owned();
                valid_id(&id).then_some((path, id))
            })
            .map(|(path, id)| {
                (
                    std::fs::metadata(path)
                        .and_then(|meta| meta.modified())
                        .ok(),
                    id,
                )
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.cmp(a));
        Ok(entries
            .into_iter()
            .skip(offset)
            .take(limit.clamp(1, 100))
            .map(|(_, id)| {
                self.snapshot(&id)
                    .map(|snapshot| snapshot.entry)
                    .unwrap_or_else(|error| AgentHistoryEntry {
                        id,
                        title: "无法读取的变更记录".to_owned(),
                        created_at: String::new(),
                        file_count: 0,
                        bytes: 0,
                        paths: Vec::new(),
                        error: Some(error),
                    })
            })
            .collect())
    }
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn validate_path(root: &Path, path: &Path) -> Result<(), String> {
    if !root.is_absolute()
        || !path.is_absolute()
        || path == root
        || !path.starts_with(root)
        || root
            .components()
            .chain(path.components())
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("历史路径超出原配置范围".to_owned());
    }
    Ok(())
}
