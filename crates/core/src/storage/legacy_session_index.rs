//! 旧版本把会话正文写入 `session-index` 下的 SQLite 派生库。检索改为按需
//! 读取原生日志后，这些库不再被任何代码使用；升级时按旧命名规则清掉，
//! 避免缓存目录长期占用磁盘。
use crate::models::AgentCliKind;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const DIRECTORY_NAME: &str = "session-index";
const DATABASE_SUFFIX: &str = ".sqlite3";
const SIDECAR_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

/// 旧设置里用户自定义的索引根目录；空值或缺失表示使用默认缓存目录。
pub(super) fn custom_directory(value: &Value) -> Option<PathBuf> {
    value
        .get("settings")?
        .get("sessionIndexDirectory")?
        .as_str()
        .map(str::trim)
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
}

/// 清理默认缓存目录和旧设置指向的自定义目录。只删除旧索引的固定命名，
/// 目录里出现其他文件时保留目录本身。返回是否真的删除了文件。
pub(super) fn cleanup(cache_dir: &Path, custom_directory: Option<&Path>) -> Result<bool, String> {
    let mut directories = BTreeSet::from([cache_dir.join(DIRECTORY_NAME)]);
    if let Some(custom_directory) = custom_directory {
        directories.insert(custom_directory.join(DIRECTORY_NAME));
    }

    let mut removed = false;
    let mut errors = Vec::new();
    for directory in directories {
        match cleanup_directory(&directory) {
            Ok(cleaned) => removed |= cleaned,
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(errors.join("；"))
    }
}

fn cleanup_directory(directory: &Path) -> Result<bool, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("读取 {} 失败：{error}", directory.display())),
    };

    let mut removed = false;
    let mut errors = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!("读取 {} 失败：{error}", directory.display()));
                continue;
            }
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !legacy_database_name(name)
            || !entry.file_type().is_ok_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let path = entry.path();
        match fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(error) => errors.push(format!("删除 {} 失败：{error}", path.display())),
        }
    }

    if removed {
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => errors.push(format!("删除 {} 失败：{error}", directory.display())),
        }
    }

    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(errors.join("；"))
    }
}

/// 与旧实现一致：`<agent key>.sqlite3` 或 `<agent key>-<hash>.sqlite3`，
/// 以及 SQLite 的 `-wal` / `-shm` / `-journal` 边车文件。
fn legacy_database_name(name: &str) -> bool {
    let mut base = name;
    for suffix in SIDECAR_SUFFIXES {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped;
            break;
        }
    }
    let Some(stem) = base.strip_suffix(DATABASE_SUFFIX) else {
        return false;
    };
    AgentCliKind::ALL.iter().any(|kind| {
        stem == kind.key()
            || stem
                .strip_prefix(kind.key())
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "balancehub-legacy-index-{name}-{}-{now}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory should be created");
        path
    }

    #[test]
    fn removes_known_databases_and_sidecars_but_keeps_unrelated_files() {
        let root = test_dir("keep-unrelated");
        let cache = root.join("cache");
        let directory = cache.join(DIRECTORY_NAME);
        fs::create_dir_all(&directory).expect("index directory should be created");
        for name in [
            "codex-0123.sqlite3",
            "codex-0123.sqlite3-wal",
            "claudeCode.sqlite3-shm",
            "gemini-4567.sqlite3-journal",
            "notes.txt",
            "codex-0123.sqlite3.bak",
            "codex2-0123.sqlite3",
        ] {
            fs::write(directory.join(name), "fixture").expect("fixture should be writable");
        }

        assert_eq!(cleanup(&cache, None), Ok(true));
        assert!(!directory.join("codex-0123.sqlite3").exists());
        assert!(!directory.join("codex-0123.sqlite3-wal").exists());
        assert!(!directory.join("claudeCode.sqlite3-shm").exists());
        assert!(!directory.join("gemini-4567.sqlite3-journal").exists());
        assert!(directory.join("notes.txt").exists());
        assert!(directory.join("codex-0123.sqlite3.bak").exists());
        assert!(directory.join("codex2-0123.sqlite3").exists());
        assert!(directory.exists(), "unrelated files keep the directory");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removes_an_index_directory_that_becomes_empty() {
        let root = test_dir("remove-empty");
        let cache = root.join("cache");
        let directory = cache.join(DIRECTORY_NAME);
        fs::create_dir_all(&directory).expect("index directory should be created");
        fs::write(directory.join("grok-89ab.sqlite3"), "fixture")
            .expect("fixture should be writable");

        assert_eq!(cleanup(&cache, None), Ok(true));
        assert!(!directory.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleans_the_custom_directory_from_old_settings() {
        let root = test_dir("custom");
        let cache = root.join("cache");
        let custom = root.join("custom-root");
        let custom_directory = custom.join(DIRECTORY_NAME);
        fs::create_dir_all(&custom_directory).expect("custom index directory should be created");
        fs::write(custom_directory.join("codex-abc.sqlite3"), "fixture")
            .expect("fixture should be writable");

        assert_eq!(cleanup(&cache, Some(&custom)), Ok(true));
        assert!(!custom_directory.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_directories_are_ignored() {
        let root = test_dir("missing");
        assert_eq!(cleanup(&root.join("cache"), None), Ok(false));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn custom_directory_is_read_only_when_the_old_setting_exists() {
        let value = serde_json::json!({
            "settings": { "sessionIndexDirectory": "  /tmp/balancehub-index  " }
        });
        assert_eq!(
            custom_directory(&value),
            Some(PathBuf::from("/tmp/balancehub-index"))
        );
        assert_eq!(
            custom_directory(&serde_json::json!({ "settings": { "sessionIndexDirectory": "" } })),
            None
        );
        assert_eq!(
            custom_directory(&serde_json::json!({ "settings": {} })),
            None
        );
    }
}
