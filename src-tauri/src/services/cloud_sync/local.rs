use super::{
    core::Replica,
    files,
    format::{self, SyncDocuments},
    projection,
};
use crate::{models::AppData, state::AppState, storage};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use tauri::{AppHandle, Emitter, Manager};

pub(super) struct AppReplica {
    pub app: AppHandle,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    id: String,
    committed: bool,
    before_data: AppData,
    after_data: AppData,
    before_library: Option<String>,
    after_library: Option<String>,
    recovery: SyncDocuments,
}

/// Both the app mutation gate and library transaction remain held here.
/// A library save failure cannot race a second local writer during rollback.
fn coordinated_apply(
    root: &Path,
    mut journal: Journal,
    publish_library: &mut dyn FnMut() -> Result<(), String>,
    save_data: impl FnOnce() -> Result<(), String>,
) -> Result<Option<String>, String> {
    let path = root.join("cloud-sync/apply-journal.json");
    let result = (|| {
        files::write_json(&path, &journal)?;
        publish_library()?;
        save_data()?;
        journal.committed = true;
        if let Err(error) = files::write_json(&path, &journal) {
            // Replacement may have succeeded before directory fsync failed.
            // Once the commit marker is visible, keep both new files and
            // publish matching memory rather than rolling back half a commit.
            let persisted: Option<Journal> = files::read_json(&path)?;
            if persisted.is_some_and(|saved| saved.id == journal.id && saved.committed) {
                return Ok(Some(format!(
                    "配置已应用，但提交标记落盘未完全确认：{error}；请重新同步"
                )));
            }
            return Err(error);
        }
        Ok(None)
    })();
    if let Err(error) = result {
        recover(root).map_err(|recovery| format!("{error}；回滚未完成：{recovery}"))?;
        return Err(error);
    }
    result.map(|warning| {
        // Preserve the previous recovery point until both configuration files
        // have committed. The journal completes this step after a crash.
        match files::write_json(&root.join("cloud-sync/recovery.json"), &journal.recovery) {
            Ok(()) => warning,
            Err(error) => Some(format!("配置已应用，恢复点将在下次启动时完成保存：{error}")),
        }
    })
}

/// Runs before AppState and the catalog are loaded. Only files matching this
/// transaction's old/new versions may be recovered; external edits are kept.
pub(crate) fn recover(root: &Path) -> Result<(), String> {
    let path = root.join("cloud-sync/apply-journal.json");
    let Some(journal): Option<Journal> = files::read_json(&path)? else {
        return Ok(());
    };
    if journal.committed {
        files::write_json(&root.join("cloud-sync/recovery.json"), &journal.recovery)?;
        fs::remove_file(&path).map_err(|_| "无法清理已完成的同步事务")?;
        return Ok(());
    }
    let data_path = root.join("data.json");
    let current: Option<AppData> = files::read_json(&data_path)?;
    let current =
        serde_json::to_value(current.unwrap_or_default()).map_err(|_| "同步恢复检查失败")?;
    let before = serde_json::to_value(&journal.before_data).map_err(|_| "同步恢复检查失败")?;
    let after = serde_json::to_value(&journal.after_data).map_err(|_| "同步恢复检查失败")?;
    if current != before && current != after {
        return Err("上次同步未完成，配置又被外部修改；已保留所有文件，请先备份后处理".to_owned());
    }
    let library_path = root.join("agent-asset-library/library.json");
    let library_before = journal
        .before_library
        .as_ref()
        .map(|value| STANDARD.decode(value).map_err(|_| "共享库恢复点损坏"))
        .transpose()?;
    let library_after = journal
        .after_library
        .as_ref()
        .map(|value| STANDARD.decode(value).map_err(|_| "共享库恢复点损坏"))
        .transpose()?;
    if let Some(after) = &library_after {
        let current = match fs::read(&library_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("无法读取待恢复的共享库".to_owned()),
        };
        let matches_before = current.as_ref().map(|bytes| format::digest(bytes))
            == library_before.as_ref().map(|bytes| format::digest(bytes));
        let matches_after = current
            .as_ref()
            .is_some_and(|bytes| format::digest(bytes) == format::digest(after));
        if !matches_before && !matches_after {
            return Err("上次同步未完成，共享库又被修改；已保留原文件，未自动覆盖".to_owned());
        }
    }
    files::write_json(&data_path, &journal.before_data)?;
    if library_after.is_some() {
        if let Some(bytes) = library_before {
            files::write_bytes(&library_path, &bytes)?;
        } else if let Err(error) = fs::remove_file(&library_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err("无法恢复同步前的空共享库".to_owned());
            }
        }
    }
    fs::remove_file(path).map_err(|_| "同步恢复完成，但事务标记清理失败".to_owned())
}

impl Replica for AppReplica {
    fn validate(&self, documents: &SyncDocuments) -> Result<(), String> {
        let state = self.app.state::<AppState>();
        let data = state.data.read().map_err(|_| "应用配置不可用")?;
        let next = projection::apply_app_documents(&data, documents)?;
        let submitted: SyncDocuments = documents
            .iter()
            .filter(|(key, _)| !key.starts_with("asset/") && !key.starts_with("blob/"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if projection::app_documents(&next)? != submitted {
            return Err("云端配置不能无损应用，请核对配置格式与应用版本".to_owned());
        }
        drop(data);
        state
            .agent_catalog(&self.app)?
            .validate_cloud_snapshot(documents)
    }
    fn snapshot(&self) -> Result<SyncDocuments, String> {
        let state = self.app.state::<AppState>();
        if let Some(error) = state.load_error() {
            return Err(error);
        }
        let data = state.data.read().map_err(|_| "应用配置不可用")?;
        let mut documents = projection::app_documents(&data)?;
        drop(data);
        documents.extend(state.agent_catalog(&self.app)?.cloud_snapshot()?);
        Ok(documents)
    }

    fn apply(&self, expected: &SyncDocuments, desired: &SyncDocuments) -> Result<(), String> {
        if expected == desired {
            return Ok(());
        }
        let state = self.app.state::<AppState>();
        let _transaction = state
            .mutation_gate
            .lock()
            .map_err(|_| "应用配置事务不可用")?;
        if let Some(error) = state.load_error() {
            return Err(error);
        }
        let current = state.data.read().map_err(|_| "应用配置不可用")?.clone();
        let expected_app: SyncDocuments = expected
            .iter()
            .filter(|(key, _)| !key.starts_with("asset/") && !key.starts_with("blob/"))
            .map(|(key, doc)| (key.clone(), doc.clone()))
            .collect();
        if projection::app_documents(&current)? != expected_app {
            return Err("本地配置在同步期间已变化，已保留新修改；请重新同步".to_owned());
        }
        let mut next = projection::apply_app_documents(&current, desired)?;
        let revision = state.next_revision();
        next.revision = revision;
        for provider in &mut next.providers {
            provider.revision = revision;
        }
        let root = self
            .app
            .path()
            .app_config_dir()
            .map_err(|_| "无法获取配置目录")?;
        let journal_path = root.join("cloud-sync/apply-journal.json");
        let catalog = state.agent_catalog(&self.app)?;
        let result =
            catalog.apply_cloud_snapshot(expected, desired, |before, after, publish_catalog| {
                let journal = Journal {
                    id: format::random_id()?,
                    committed: false,
                    before_data: current.clone(),
                    after_data: next.clone(),
                    before_library: before.map(|bytes| STANDARD.encode(bytes)),
                    after_library: after.map(|bytes| STANDARD.encode(bytes)),
                    recovery: expected.clone(),
                };
                let warning = coordinated_apply(&root, journal, publish_catalog, || {
                    storage::save_app_data(&self.app, &next)
                })?;
                *state
                    .data
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = next.clone();
                Ok(warning)
            });
        let warning = match result {
            Ok(value) => value,
            Err(error) => {
                if journal_path.exists() {
                    state.block_storage(error.clone());
                }
                return Err(error);
            }
        };
        if warning.is_none() {
            let _ = fs::remove_file(journal_path);
        }
        state.cloud_sync_signal.changed();
        let _ = self.app.emit("cloud-sync-applied", revision);
        let _ = self.app.emit("providers-changed", ());
        warning.map_or(Ok(()), Err)
    }

    fn recovery(&self) -> Result<Option<SyncDocuments>, String> {
        let root = self
            .app
            .path()
            .app_config_dir()
            .map_err(|_| "无法获取配置目录")?;
        files::read_json(&root.join("cloud-sync/recovery.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ThemeMode;

    fn journal() -> Journal {
        let mut before = AppData::default();
        before.settings.theme_mode = ThemeMode::Dark;
        let mut after = before.clone();
        after.settings.theme_mode = ThemeMode::Light;
        Journal {
            id: "fixture-transaction".to_owned(),
            committed: false,
            before_data: before,
            after_data: after,
            before_library: None,
            after_library: Some(STANDARD.encode(b"fixture new library")),
            recovery: projection::app_documents(&AppData::default()).unwrap(),
        }
    }

    #[test]
    fn unfinished_transaction_rolls_back_a_new_library_and_app_data() {
        for library_was_written in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let journal = journal();
            files::write_json(&root.path().join("cloud-sync/apply-journal.json"), &journal)
                .unwrap();
            files::write_json(&root.path().join("data.json"), &journal.after_data).unwrap();
            let library = root.path().join("agent-asset-library/library.json");
            if library_was_written {
                files::write_bytes(&library, b"fixture new library").unwrap();
            }
            recover(root.path()).unwrap();
            let restored: AppData = files::read_json(&root.path().join("data.json"))
                .unwrap()
                .unwrap();
            assert!(matches!(restored.settings.theme_mode, ThemeMode::Dark));
            assert!(!library.exists());
            assert!(!root.path().join("cloud-sync/apply-journal.json").exists());
        }
    }

    #[test]
    fn recovery_preserves_exact_old_library_bytes_and_refuses_external_edits() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = journal();
        let original = b"{\n  \"fixture\": \"old bytes with whitespace\"\n}";
        journal.before_library = Some(STANDARD.encode(original));
        files::write_json(&root.path().join("cloud-sync/apply-journal.json"), &journal).unwrap();
        files::write_json(&root.path().join("data.json"), &journal.after_data).unwrap();
        let library = root.path().join("agent-asset-library/library.json");
        files::write_bytes(&library, b"fixture external edit").unwrap();
        assert!(recover(root.path()).unwrap_err().contains("被修改"));
        assert_eq!(fs::read(&library).unwrap(), b"fixture external edit");
        files::write_bytes(&library, b"fixture new library").unwrap();
        recover(root.path()).unwrap();
        assert_eq!(fs::read(&library).unwrap(), original);
    }

    #[test]
    fn a_committed_journal_is_only_cleaned_and_never_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = journal();
        journal.committed = true;
        files::write_json(&root.path().join("cloud-sync/apply-journal.json"), &journal).unwrap();
        files::write_json(&root.path().join("data.json"), &journal.after_data).unwrap();
        recover(root.path()).unwrap();
        let current: AppData = files::read_json(&root.path().join("data.json"))
            .unwrap()
            .unwrap();
        assert!(matches!(current.settings.theme_mode, ThemeMode::Light));
        let recovery: SyncDocuments =
            files::read_json(&root.path().join("cloud-sync/recovery.json"))
                .unwrap()
                .unwrap();
        assert_eq!(recovery, journal.recovery);
    }

    #[test]
    fn failed_apply_rolls_back_both_files_and_preserves_previous_recovery() {
        for fail_library in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut journal = journal();
            let original = b"fixture original library";
            journal.before_library = Some(STANDARD.encode(original));
            let data_path = root.path().join("data.json");
            let library_path = root.path().join("agent-asset-library/library.json");
            let recovery_path = root.path().join("cloud-sync/recovery.json");
            files::write_json(&data_path, &journal.before_data).unwrap();
            files::write_bytes(&library_path, original).unwrap();
            let previous_recovery = SyncDocuments::default();
            files::write_json(&recovery_path, &previous_recovery).unwrap();
            let after_data = journal.after_data.clone();
            let error = coordinated_apply(
                root.path(),
                journal,
                &mut || {
                    files::write_bytes(&library_path, b"fixture new library")?;
                    if fail_library {
                        Err("fixture library write acknowledgement failure".to_owned())
                    } else {
                        Ok(())
                    }
                },
                || {
                    files::write_json(&data_path, &after_data)?;
                    Err("fixture data write acknowledgement failure".to_owned())
                },
            )
            .unwrap_err();
            assert!(error.contains("acknowledgement failure"));
            assert_eq!(fs::read(&library_path).unwrap(), original);
            let data: AppData = files::read_json(&data_path).unwrap().unwrap();
            assert!(matches!(data.settings.theme_mode, ThemeMode::Dark));
            let recovery: SyncDocuments = files::read_json(&recovery_path).unwrap().unwrap();
            assert_eq!(recovery, previous_recovery);
            assert!(!root.path().join("cloud-sync/apply-journal.json").exists());
        }
    }

    #[test]
    fn successful_apply_publishes_both_files_before_replacing_recovery() {
        let root = tempfile::tempdir().unwrap();
        let journal = journal();
        let after_data = journal.after_data.clone();
        let expected_recovery = journal.recovery.clone();
        let library_path = root.path().join("agent-asset-library/library.json");
        let data_path = root.path().join("data.json");
        let recovery_path = root.path().join("cloud-sync/recovery.json");
        files::write_json(&data_path, &journal.before_data).unwrap();
        let warning = coordinated_apply(
            root.path(),
            journal,
            &mut || {
                assert!(!recovery_path.exists());
                files::write_bytes(&library_path, b"fixture new library")
            },
            || {
                assert!(!recovery_path.exists());
                files::write_json(&data_path, &after_data)
            },
        )
        .unwrap();
        assert!(warning.is_none());
        recover(root.path()).unwrap();
        assert_eq!(fs::read(&library_path).unwrap(), b"fixture new library");
        let data: AppData = files::read_json(&data_path).unwrap().unwrap();
        assert!(matches!(data.settings.theme_mode, ThemeMode::Light));
        let recovery: SyncDocuments = files::read_json(&recovery_path).unwrap().unwrap();
        assert_eq!(recovery, expected_recovery);
    }
}
