//! Rollouts remain readable when a native database has not indexed them, was
//! removed, or predates the current CLI. Only metadata windows are read here.
use super::*;
use crate::services::cli_sessions::{
    first_non_empty, normalize_timestamp, read_json_lines_limited, source,
    workbench::cached_history_record,
};

pub(super) struct Scan<'a> {
    pub kind: AgentCliKind,
    pub root: &'a Path,
    pub workdirs: &'a [PathBuf],
    pub indexed_locators: &'a HashSet<PathBuf>,
    pub titles: &'a HashMap<String, String>,
    pub budget: &'a SessionReadBudget,
    pub mode: SessionHistoryReadMode,
}

impl Scan<'_> {
    pub(super) fn append(
        &self,
        files: Option<HashMap<String, PathBuf>>,
        result: &mut SessionHistoryScan,
    ) {
        let files = match files
            .map(Ok)
            .unwrap_or_else(|| rollout_files(self.root, self.budget))
        {
            Ok(files) => files,
            Err(error) => {
                result.failed(error);
                return;
            }
        };
        let indexed = self
            .indexed_locators
            .iter()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
            .collect::<HashSet<_>>();
        let mut known_ids = result
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.records)
            .filter(|record| record.content_unavailable_reason.is_none())
            .map(|record| record.summary.id.clone())
            .collect::<HashSet<_>>();
        let unavailable_ids = result
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.records)
            .filter(|record| record.content_unavailable_reason.is_some())
            .map(|record| record.summary.id.clone())
            .collect::<HashSet<_>>();
        let mut files = files.into_values().collect::<Vec<_>>();
        files.sort();
        for path in files {
            if let Err(error) = self.budget.check() {
                result.failed(error);
                break;
            }
            if indexed.contains(&path.canonicalize().unwrap_or_else(|_| path.clone())) {
                continue;
            }
            let read = || read_record(self.kind, self.root, &path, self.mode);
            let loaded = if self.mode == SessionHistoryReadMode::Identities {
                read()
            } else {
                cached_history_record(&path, read)
            };
            let mut record = match loaded {
                Ok(Some(record)) => record,
                Ok(None) => continue,
                Err(error) => {
                    result.failed(error);
                    continue;
                }
            };
            if record.summary.id.is_empty() {
                result.failed("Codex 日志缺少可识别的原生会话 ID");
                continue;
            }
            let workdir = PathBuf::from(&record.summary.workdir);
            let identity = workdir.canonicalize().unwrap_or_else(|_| workdir.clone());
            if !self.workdirs.is_empty()
                && !self
                    .workdirs
                    .iter()
                    .any(|path| path.canonicalize().unwrap_or_else(|_| path.clone()) == identity)
            {
                continue;
            }
            if !known_ids.insert(record.summary.id.clone()) {
                continue;
            }
            // A readable native record supersedes an index-only placeholder.
            if unavailable_ids.contains(&record.summary.id) {
                for workspace in &mut result.workspaces {
                    workspace
                        .records
                        .retain(|existing| existing.summary.id != record.summary.id);
                }
            }
            if let Some(title) = self
                .titles
                .get(&record.summary.id)
                .and_then(|title| clean_text(title, 100))
            {
                record.summary.title = title;
            }
            result.workspace(&workdir).records.push(record);
        }
    }
}

pub(super) fn read_record(
    kind: AgentCliKind,
    root: &Path,
    path: &Path,
    mode: SessionHistoryReadMode,
) -> Result<Option<SessionHistoryRecord>, String> {
    if mode == SessionHistoryReadMode::Identities {
        return read_metadata_header(path, false)
            .map(|metadata| metadata.map(|metadata| identity_record(kind, path, &metadata)));
    }
    let mut metadata = None;
    let mut preview = None;
    let mut model = None;
    let mut models = std::collections::BTreeSet::new();
    let mut updated_at = None;
    let truncated =
        read_json_lines_limited(path, 64 * 1024, "读取 Codex 会话摘要", |_, value| {
            let Some(payload) = value.get("payload") else {
                return;
            };
            let family = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match family {
                "session_meta" if metadata.is_none() => {
                    metadata = Some(payload.clone());
                }
                "turn_context" => {
                    if let Some(value) = payload.get("model").and_then(Value::as_str) {
                        model = clean_text(value, 120);
                        models.extend(model.clone());
                    }
                }
                "event_msg"
                    if preview.is_none()
                        && payload.get("type").and_then(Value::as_str) == Some("user_message") =>
                {
                    preview = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(|text| clean_text(text, 240));
                }
                "response_item"
                    if preview.is_none()
                        && payload.get("role").and_then(Value::as_str) == Some("user") =>
                {
                    preview = super::super::rollout::response_message_text(payload)
                        .filter(|text| {
                            ![
                                "<environment_context>",
                                "<system-reminder>",
                                "# AGENTS.md instructions",
                                "<INSTRUCTIONS>",
                            ]
                            .iter()
                            .any(|prefix| text.trim_start().starts_with(prefix))
                        })
                        .and_then(|text| clean_text(text, 240));
                }
                _ => {}
            }
            if let Some(timestamp) =
                normalize_timestamp(value.get("timestamp").and_then(Value::as_str))
            {
                updated_at = Some(timestamp);
            }
        })?;
    if metadata.is_none() && truncated {
        metadata = read_metadata_header(path, true)?;
    }
    if metadata.is_none() && fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
        return Ok(None);
    }
    let metadata = metadata.unwrap_or(Value::Null);
    let mut record = identity_record(kind, path, &metadata);
    let (role, parent) = decode_role(
        metadata.get("source").map(Value::to_string).as_deref(),
        metadata
            .get("parent_thread_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    );
    record.role = role;
    record.parent_native_id = parent;
    record.summary.title = first_non_empty([
        metadata
            .get("name")
            .and_then(Value::as_str)
            .and_then(|text| clean_text(text, 100)),
        preview.as_ref().and_then(|text| clean_text(text, 100)),
    ]);
    record.summary.preview = preview;
    record.summary.model = model;
    record.summary.models = models.into_iter().collect();
    record.summary.created_at =
        normalize_timestamp(metadata.get("timestamp").and_then(Value::as_str));
    record.summary.updated_at = updated_at.or_else(|| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from)
            .map(|time| time.to_rfc3339())
    });
    record.summary.cli_version = metadata
        .get("cli_version")
        .and_then(Value::as_str)
        .map(str::to_owned);
    record.summary.archived =
        source::logical_path(path).starts_with(root.join("archived_sessions"));
    record.summary.metadata_source = "codexRollout".into();
    set_resume_capability(&mut record);
    Ok(Some(record))
}

fn identity_record(kind: AgentCliKind, path: &Path, metadata: &Value) -> SessionHistoryRecord {
    let id = metadata
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let workdir = metadata
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default();
    SessionHistoryRecord::identity(kind, id, Path::new(workdir), path.into())
}

fn read_metadata_header(path: &Path, need_role: bool) -> Result<Option<Value>, String> {
    let mut observed = None;
    let result = crate::services::cli_sessions::read_session_header(
        path,
        &[
            "type",
            "payload.id",
            "payload.cwd",
            "payload.source",
            "payload.parent_thread_id",
            "payload.timestamp",
            "payload.cli_version",
            "payload.name",
        ],
        |fields, _| {
            if fields
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind != "session_meta")
            {
                return false;
            }
            let Some(metadata) = fields
                .get("payload")
                .filter(|metadata| metadata.get("id").is_some())
            else {
                return false;
            };
            observed = Some(metadata.clone());
            metadata.get("cwd").is_some() && (!need_role || metadata.get("source").is_some())
        },
    );
    crate::services::cli_sessions::workbench::check_read_budget()?;
    match result {
        Ok(_) => Ok(observed),
        // Older logs may omit optional source fields. Preserve identity fields
        // already proven before the bounded header ended, without guessing them.
        Err(_) if observed.is_some() => Ok(observed),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::cli_sessions::workbench::{
        with_history_cache, with_read_budget, with_source_root,
    };
    use serde_json::json;
    use std::{
        sync::{atomic::AtomicBool, Arc, Mutex},
        time::{Duration, Instant},
    };

    #[test]
    fn oversized_native_instructions_do_not_hide_unindexed_session_identity_or_directory() {
        let root = std::env::temp_dir().join(format!(
            "balancehub-large-codex-header-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("native.jsonl");
        let workdir = root.join("unregistered-project");
        let header = format!(
            r#"{{"type":"session_meta","payload":{{"id":"large-native","cwd":{},"source":"cli","base_instructions":"{}"}}}}"#,
            serde_json::to_string(&workdir).unwrap(),
            "instructions ".repeat(150_000)
        );
        fs::write(&path, format!("{header}\n{}\n", json!({"type":"event_msg","payload":{"type":"user_message","message":"native preview"}}))).unwrap();
        for mode in [
            SessionHistoryReadMode::Summaries,
            SessionHistoryReadMode::Identities,
        ] {
            let budget = SessionReadBudget::new(
                Instant::now() + Duration::from_secs(5),
                Arc::new(AtomicBool::new(false)),
                128 * 1024,
            );
            let record = with_read_budget(&budget, || {
                read_record(AgentCliKind::Codex, &root, &path, mode)
            })
            .unwrap()
            .unwrap();
            assert_eq!(record.summary.id, "large-native");
            assert_eq!(record.summary.workdir, workdir.to_string_lossy());
            if mode == SessionHistoryReadMode::Summaries {
                assert_eq!(record.role, AgentSessionRole::Main);
                assert!(record.summary.can_resume);
                assert_eq!(record.summary.preview.as_deref(), Some("native preview"));
            }
            assert!(budget.bytes.load(std::sync::atomic::Ordering::Relaxed) < 80 * 1024);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_live_and_archived_rollouts_without_database_and_reuses_native_cache_after_index_changes(
    ) {
        let root =
            std::env::temp_dir().join(format!("balancehub-unindexed-codex-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sessions")).unwrap();
        fs::create_dir_all(root.join("archived_sessions")).unwrap();
        let root = root.canonicalize().unwrap();
        let workdir = root.join("unregistered-project");
        fs::create_dir_all(&workdir).unwrap();
        for (id, directory) in [("live", "sessions"), ("archived", "archived_sessions")] {
            let log = format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":id,"cwd":workdir,"source":"cli"}}),
                json!({"type":"event_msg","payload":{"type":"user_message","message":"synthetic native history"}})
            );
            let path = root.join(directory).join(format!("{id}.jsonl"));
            fs::write(&path, &log).unwrap();
            if id == "archived" {
                fs::write(
                    path.with_extension("jsonl.zst"),
                    zstd::stream::encode_all(log.as_bytes(), 1).unwrap(),
                )
                .unwrap();
            }
        }
        let cache = Arc::new(Mutex::new(Default::default()));
        let scan = || {
            let budget = SessionReadBudget::new(
                Instant::now() + Duration::from_secs(5),
                Arc::new(AtomicBool::new(false)),
                1024 * 1024,
            );
            with_read_budget(&budget, || {
                with_source_root(&root, || {
                    with_history_cache(&cache, || {
                        (HISTORY.scan)(
                            AgentCliKind::Codex,
                            &root,
                            &[],
                            &budget,
                            SessionHistoryReadMode::Summaries,
                        )
                    })
                })
            })
            .unwrap()
        };
        let first = scan();
        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        assert_eq!(first.workspaces.len(), 1);
        assert_eq!(
            first.workspaces[0].records.len(),
            2,
            "compressed copies must not duplicate history"
        );
        assert!(
            first.workspaces[0]
                .records
                .iter()
                .find(|record| record.summary.id == "live")
                .unwrap()
                .summary
                .can_resume
        );
        assert!(
            !first.workspaces[0]
                .records
                .iter()
                .find(|record| record.summary.id == "archived")
                .unwrap()
                .summary
                .can_resume
        );
        let database = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
        database.execute_batch("CREATE TABLE threads (id TEXT, cwd TEXT, title TEXT, rollout_path TEXT, source TEXT)").unwrap();
        database.execute("INSERT INTO threads VALUES ('live', ?1, 'updated native title', 'sessions/live.jsonl', 'cli')", [workdir.to_string_lossy().as_ref()]).unwrap();
        drop(database);
        let indexed = scan();
        assert_eq!(indexed.workspaces[0].records.len(), 2);
        assert_eq!(
            indexed.workspaces[0]
                .records
                .iter()
                .find(|record| record.summary.id == "live")
                .unwrap()
                .summary
                .title,
            "updated native title"
        );
        fs::remove_file(root.join("state_5.sqlite")).unwrap();
        let after_removal = scan();
        let live = after_removal.workspaces[0]
            .records
            .iter()
            .find(|record| record.summary.id == "live")
            .unwrap();
        assert_eq!(
            live.summary.title, "synthetic native history",
            "database overlays must not contaminate the native file cache"
        );
        assert!(live.summary.can_resume);
        fs::remove_dir_all(root).unwrap();
    }
}
