use crate::services::agent_cli::contracts::{SessionHistoryRecord, SessionReadBudget};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

thread_local! { static ACTIVE_BUDGET: RefCell<Option<SessionReadBudget>> = const { RefCell::new(None) }; }

// The adapter's synchronous read stack inherits one request budget. This avoids
// independent limits in summary/detail/index decoders, and is local to each
// worker thread. The previous context is restored even during unwinding.
pub(crate) fn with_read_budget<T>(budget: &SessionReadBudget, read: impl FnOnce() -> T) -> T {
    struct Restore(Option<SessionReadBudget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_BUDGET.with(|active| active.replace(self.0.take()));
        }
    }
    let _restore = Restore(ACTIVE_BUDGET.with(|active| active.replace(Some(budget.clone()))));
    read()
}

pub(crate) fn check_read_budget() -> Result<(), String> {
    ACTIVE_BUDGET.with(|active| {
        active
            .borrow()
            .as_ref()
            .map_or(Ok(()), SessionReadBudget::check)
    })
}

pub(crate) struct BudgetFile {
    file: File,
    budget: Option<SessionReadBudget>,
}
pub(crate) fn open_session_file(path: impl AsRef<Path>) -> io::Result<BudgetFile> {
    check_read_budget().map_err(io::Error::other)?;
    let path = path.as_ref();
    verify_source_path(path)?;
    Ok(BudgetFile {
        file: File::open(path)?,
        budget: ACTIVE_BUDGET.with(|active| active.borrow().clone()),
    })
}
impl BudgetFile {
    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        self.file.metadata()
    }
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            budget: self.budget.clone(),
        })
    }
}
impl Read for BudgetFile {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(budget) = &self.budget {
            budget.check().map_err(io::Error::other)?;
        }
        let length = bytes.len().min(64 * 1024);
        let read = self.file.read(&mut bytes[..length])?;
        if let Some(budget) = &self.budget {
            budget.record(read).map_err(io::Error::other)?;
        }
        Ok(read)
    }
}
impl Seek for BudgetFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

pub(crate) fn read_session_text_file_limited(
    path: &Path,
    max_bytes: usize,
    context: &str,
) -> Result<String, String> {
    let file = open_session_file(path).map_err(|error| format!("{context}失败：{error}"))?;
    if file.metadata().map_err(|error| error.to_string())?.len() > max_bytes as u64 {
        return Err(format!("{context}超过读取上限"));
    }
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{context}失败：{error}"))?;
    if bytes.len() > max_bytes {
        return Err(format!("{context}超过读取上限"));
    }
    String::from_utf8(bytes).map_err(|_| format!("{context}不是有效 UTF-8"))
}

struct CachedHistoryRecord {
    fingerprint: String,
    result: Result<Option<HistoryRecordFacts>, String>,
}
#[derive(Clone)]
pub(crate) struct HistoryRecordFacts {
    pub record: SessionHistoryRecord,
    pub native_origin_workdir: Option<PathBuf>,
}
struct CachedMetadataText {
    fingerprint: String,
    max_bytes: usize,
    text: String,
}
#[derive(Default)]
pub(crate) struct HistoryReadCacheData {
    records: HashMap<PathBuf, CachedHistoryRecord>,
    metadata: HashMap<PathBuf, CachedMetadataText>,
}
pub(crate) type HistoryReadCache = Arc<Mutex<HistoryReadCacheData>>;
thread_local! { static ACTIVE_HISTORY_CACHE: RefCell<Option<HistoryReadCache>> = const { RefCell::new(None) }; }
pub(crate) fn with_history_cache<T>(cache: &HistoryReadCache, read: impl FnOnce() -> T) -> T {
    struct Restore(Option<HistoryReadCache>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_HISTORY_CACHE.with(|active| active.replace(self.0.take()));
        }
    }
    let _restore = Restore(ACTIVE_HISTORY_CACHE.with(|active| active.replace(Some(cache.clone()))));
    read()
}
pub(crate) fn cached_history_record(
    path: &Path,
    read: impl FnOnce() -> Result<Option<SessionHistoryRecord>, String>,
) -> Result<Option<SessionHistoryRecord>, String> {
    cached_history_record_facts(path, || {
        read().map(|record| {
            record.map(|record| HistoryRecordFacts {
                record,
                native_origin_workdir: None,
            })
        })
    })
    .map(|facts| facts.map(|facts| facts.record))
}

pub(crate) fn cached_history_record_facts(
    path: &Path,
    read: impl FnOnce() -> Result<Option<HistoryRecordFacts>, String>,
) -> Result<Option<HistoryRecordFacts>, String> {
    check_read_budget()?;
    verify_source_path(path).map_err(|error| error.to_string())?;
    let cache = ACTIVE_HISTORY_CACHE.with(|active| active.borrow().clone());
    let Some(cache) = cache else {
        return read();
    };
    let fingerprint = history_cache_fingerprint(path)?;
    let key = path.canonicalize().map_err(|error| error.to_string())?;
    let cached = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .records
        .get(&key)
        .filter(|cached| cached.fingerprint == fingerprint)
        .map(|cached| cached.result.clone());
    if let Some(result) = cached {
        check_read_budget()?;
        return result;
    }
    let result = read();
    check_read_budget()?;
    // Only successful metadata reads are reusable; cancelled or interrupted
    // reads must be allowed to continue under the next request budget.
    if result.is_ok() {
        cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .records
            .insert(
                key,
                CachedHistoryRecord {
                    fingerprint,
                    result: result.clone(),
                },
            );
    }
    result
}

fn history_cache_fingerprint(path: &Path) -> Result<String, String> {
    Ok(super::super::session_index_source_fingerprint(path, 1)?.0)
}

/// Only source/project metadata belongs here. Session bodies keep their normal
/// budgeted reader so the snapshot does not retain a second full transcript.
pub(crate) fn read_session_metadata_text_file_limited(
    path: &Path,
    max_bytes: usize,
    context: &str,
) -> Result<String, String> {
    check_read_budget()?;
    verify_source_path(path).map_err(|error| format!("{context}失败：{error}"))?;
    let cache = ACTIVE_HISTORY_CACHE.with(|active| active.borrow().clone());
    let Some(cache) = cache else {
        return read_session_text_file_limited(path, max_bytes, context);
    };
    let key = path.canonicalize().map_err(|error| error.to_string())?;
    let fingerprint = history_cache_fingerprint(path)?;
    let cached = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .metadata
        .get(&key)
        .filter(|cached| cached.fingerprint == fingerprint && cached.max_bytes == max_bytes)
        .map(|cached| cached.text.clone());
    if let Some(text) = cached {
        check_read_budget()?;
        return Ok(text);
    }
    let text = read_session_text_file_limited(path, max_bytes, context)?;
    check_read_budget()?;
    cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .metadata
        .insert(
            key,
            CachedMetadataText {
                fingerprint,
                max_bytes,
                text: text.clone(),
            },
        );
    Ok(text)
}

thread_local! { static ACTIVE_SOURCE_ROOT: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) }; }
pub(crate) fn with_source_root<T>(root: &Path, read: impl FnOnce() -> T) -> T {
    struct Restore(Option<std::path::PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_SOURCE_ROOT.with(|active| active.replace(self.0.take()));
        }
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let _restore = Restore(ACTIVE_SOURCE_ROOT.with(|active| active.replace(Some(root))));
    read()
}
fn verify_source_path(path: &Path) -> io::Result<()> {
    ACTIVE_SOURCE_ROOT.with(|active| {
        if let Some(root) = active.borrow().as_ref() {
            let canonical = path.canonicalize()?;
            if !canonical.starts_with(root) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "会话文件已离开选定原生来源",
                ));
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::Ordering, Arc},
        time::{Duration, Instant},
    };

    #[test]
    fn read_budgets_charge_consumed_bytes_instead_of_rejecting_the_source_size() {
        let root =
            std::env::temp_dir().join(format!("balancehub-read-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("large.jsonl");
        let prefix = "{\"native\":true}\n";
        std::fs::write(
            &path,
            format!(
                "{prefix}{}",
                serde_json::json!({"padding":"x".repeat(2048)})
            ),
        )
        .unwrap();
        let fresh = |bytes| {
            SessionReadBudget::new(
                Instant::now() + Duration::from_secs(5),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                bytes,
            )
        };
        let prefix_budget = fresh(128);
        let mut observed = 0;
        with_read_budget(&prefix_budget, || {
            crate::services::cli_sessions::read_json_lines_limited(
                &path,
                prefix.len() * 2,
                "读取摘要窗口",
                |_, _| observed += 1,
            )
            .unwrap();
        });
        assert_eq!(observed, 1);
        assert_eq!(
            prefix_budget.bytes.load(Ordering::Relaxed),
            (prefix.len() * 2 + 1) as u64
        );
        let small = fresh(128);
        let error = with_read_budget(&small, || {
            crate::services::cli_sessions::scan_json_records(
                &path,
                "读取会话",
                &|| true,
                |_, _| false,
            )
        })
        .unwrap_err();
        assert!(error.contains("字节预算"));
        assert!(small.bytes.load(Ordering::Relaxed) > 128);
        let sufficient = fresh(4096);
        observed = 0;
        with_read_budget(&sufficient, || {
            crate::services::cli_sessions::scan_json_records(
                &path,
                "读取会话",
                &|| true,
                |_, _| {
                    observed += 1;
                    false
                },
            )
        })
        .unwrap();
        assert_eq!(observed, 2);
        assert_eq!(
            sufficient.bytes.load(Ordering::Relaxed),
            std::fs::metadata(&path).unwrap().len()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_summary_windows_ignore_split_utf8_and_keep_complete_records() {
        let path = std::env::temp_dir().join(format!(
            "balancehub-prefix-read-limit-{}.jsonl",
            std::process::id()
        ));
        let first = "{\"sessionId\":\"native-id\"}\n";
        std::fs::write(&path, format!("{first}{}", "会".repeat(1024))).unwrap();
        let limit = (first.len() + 1) * 2;
        let budget = SessionReadBudget::new(
            Instant::now() + Duration::from_secs(5),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            128,
        );
        let mut values = Vec::new();
        with_read_budget(&budget, || {
            crate::services::cli_sessions::read_json_lines_limited(
                &path,
                limit,
                "读取前缀",
                |_, value| values.push(value),
            )
            .unwrap();
        });
        assert_eq!(budget.bytes.load(Ordering::Relaxed), limit as u64 + 1);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["sessionId"], "native-id");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn metadata_cache_revalidates_fingerprints_limits_and_request_guards() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-metadata-read-cache-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(".cwd");
        let original = "/fixture/project-a";
        std::fs::write(&path, original).unwrap();
        let cache: HistoryReadCache = Default::default();
        let fresh_budget = |max_bytes| {
            SessionReadBudget::new(
                Instant::now() + Duration::from_secs(5),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                max_bytes,
            )
        };
        let read = |budget: &SessionReadBudget, max_bytes| {
            with_read_budget(budget, || {
                with_history_cache(&cache, || {
                    with_source_root(&root, || {
                        read_session_metadata_text_file_limited(&path, max_bytes, "读取项目映射")
                    })
                })
            })
        };
        let first = fresh_budget(128);
        assert_eq!(read(&first, 64).unwrap(), original);
        assert_eq!(first.bytes.load(Ordering::Relaxed), original.len() as u64);
        let hit = fresh_budget(128);
        assert_eq!(read(&hit, 64).unwrap(), original);
        assert_eq!(hit.bytes.load(Ordering::Relaxed), 0);

        let changed = "/fixture/changed-project-with-new-length";
        std::fs::write(&path, changed).unwrap();
        let changed_budget = fresh_budget(128);
        assert_eq!(read(&changed_budget, 64).unwrap(), changed);
        assert_eq!(
            changed_budget.bytes.load(Ordering::Relaxed),
            changed.len() as u64
        );
        assert!(read(&fresh_budget(128), 4)
            .unwrap_err()
            .contains("读取上限"));
        let cached_small_budget = fresh_budget(16);
        assert_eq!(read(&cached_small_budget, 64).unwrap(), changed);
        assert_eq!(cached_small_budget.bytes.load(Ordering::Relaxed), 0);

        let cancelled = fresh_budget(128);
        cancelled.cancelled.store(true, Ordering::Release);
        assert!(read(&cancelled, 64).unwrap_err().contains("已取消"));
        let mut expired = fresh_budget(128);
        expired.deadline = Instant::now();
        assert!(read(&expired, 64).unwrap_err().contains("时间预算"));
        let wrong_root = fresh_budget(128);
        let error = with_read_budget(&wrong_root, || {
            with_history_cache(&cache, || {
                with_source_root(&root.join("another-source"), || {
                    read_session_metadata_text_file_limited(&path, 64, "读取项目映射")
                })
            })
        })
        .unwrap_err();
        assert!(error.contains("离开选定原生来源"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
