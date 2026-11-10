//! Qoder CLI native session history.
//!
//! Qoder CLI writes Claude-compatible JSONL transcripts under
//! `<QODER_CONFIG_DIR>/projects/<project-slug>/<session-id>.jsonl` (older
//! builds used `logs/sessions`). The records share Claude Code's line shape,
//! so summaries and decoding reuse the Claude parser with a Qoder id prefix.

mod decoder;

#[cfg(test)]
mod tests;

use super::config;
use crate::{
    models::{AgentCliKind, AgentSessionRole, CliSessionSummary},
    services::{
        agent_cli::{
            claude::sessions::parse_transcript,
            contracts::{
                EnvironmentPatch, SessionHistoryAdapter, SessionHistoryReadMode,
                SessionHistoryRecord, SessionHistoryScan, SessionHistorySource, SessionReadBudget,
                SessionTranscriptFormat, SessionTranscriptSource,
            },
        },
        cli_sessions::{
            read_session_header,
            workbench::{cached_history_record_facts, HistoryRecordFacts},
        },
    },
};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const PARSER_VERSION: u32 = 1;
const METADATA_SOURCE: &str = "qoderTranscript";
const MAX_SESSION_FILES: usize = 100_000;
const MAX_DEPTH: usize = 4;

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    launch_environment,
    source,
    scan,
    transcript,
    relocation: None,
};

fn launch_environment(root: &Path) -> Result<EnvironmentPatch, &'static str> {
    if !root.is_absolute() && crate::services::wsl::guest_path(&root.to_string_lossy()).is_err() {
        return Err("Qoder 原生目录必须是绝对路径");
    }
    let mut environment = EnvironmentPatch::default();
    environment.set_path("QODER_CONFIG_DIR", root);
    Ok(environment)
}

fn source() -> Result<SessionHistorySource, String> {
    let home = crate::services::cli_paths::user_home()
        .ok_or_else(|| "无法定位用户目录".to_string())?;
    let root = config::config_dir_for_home(&home);
    let (launch_environment, resume_reason) = match launch_environment(&root) {
        Ok(environment) => (environment, None),
        Err(reason) => (EnvironmentPatch::default(), Some(reason.to_owned())),
    };
    Ok(SessionHistorySource {
        config_root: root,
        launch_environment,
        resume_reason,
    })
}

fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<SessionHistoryScan, String> {
    let mut context = ScanContext {
        kind,
        fallback_workdir: (workdirs.len() == 1).then(|| workdirs[0].as_path()),
        budget,
        mode,
        seen: HashSet::new(),
        files: 0,
        result: SessionHistoryScan::new(&[]),
    };
    for sessions in session_roots(root) {
        context.walk(&sessions, 0)?;
    }
    Ok(context.result)
}

fn session_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for candidate in [root.join("projects"), root.join("logs/sessions")] {
        if candidate.is_dir() {
            roots.push(candidate);
        }
    }
    roots
}

struct ScanContext<'a> {
    kind: AgentCliKind,
    fallback_workdir: Option<&'a Path>,
    budget: &'a SessionReadBudget,
    mode: SessionHistoryReadMode,
    seen: HashSet<String>,
    files: usize,
    result: SessionHistoryScan,
}

impl ScanContext<'_> {
    fn walk(&mut self, directory: &Path, depth: usize) -> Result<(), String> {
        self.budget.check()?;
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("Qoder 会话目录不可读：{error}")),
        };
        for entry in entries {
            self.budget.check()?;
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                self.walk(&path, depth + 1)?;
                continue;
            }
            if !file_type.is_file()
                || path.extension().and_then(|value| value.to_str()) != Some("jsonl")
            {
                continue;
            }
            self.files = self.files.saturating_add(1);
            if self.files > MAX_SESSION_FILES {
                self.result.failed("Qoder 会话文件数量超过读取上限");
                return Ok(());
            }
            match add_record(self.kind, &path, self.fallback_workdir, self.mode) {
                Ok(Some(record)) => {
                    if !self.seen.insert(record.record_key.clone()) {
                        continue;
                    }
                    let workdir = PathBuf::from(&record.summary.workdir);
                    self.result.workspace(&workdir).records.push(record);
                }
                Ok(None) => {}
                Err(error) => self.result.failed(error),
            }
        }
        Ok(())
    }
}

fn add_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
    mode: SessionHistoryReadMode,
) -> Result<Option<SessionHistoryRecord>, String> {
    let read = || match mode {
        SessionHistoryReadMode::Identities => identity_record(kind, path, fallback_workdir),
        SessionHistoryReadMode::Summaries => summary_record(kind, path, fallback_workdir),
    };
    cached_history_record_facts(path, mode, None, read).map(|facts| facts.map(|facts| facts.record))
}

fn identity_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<HistoryRecordFacts>, String> {
    let Some((id, workdir)) = read_identity(path)? else {
        return Ok(None);
    };
    let workdir = workdir
        .or_else(|| fallback_workdir.map(Path::to_path_buf))
        .unwrap_or_default();
    Ok(Some(HistoryRecordFacts {
        record: SessionHistoryRecord::identity(kind, id, &workdir, path.to_path_buf()),
        native_origin_workdir: (!workdir.as_os_str().is_empty()).then_some(workdir),
    }))
}

fn read_identity(path: &Path) -> Result<Option<(String, Option<PathBuf>)>, String> {
    let mut id = None;
    let mut workdir = None;
    let header = read_session_header(
        path,
        &["sessionId", "session_id", "cwd", "workdir"],
        |fields: &Map<String, Value>, _complete| {
            if id.is_none() {
                id = field(fields, &["sessionId", "session_id"]);
            }
            if workdir.is_none() {
                workdir = field(fields, &["cwd", "workdir"])
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute());
            }
            id.is_some()
        },
    )?;
    if header.is_none() {
        return Ok(None);
    }
    let Some(id) = id else {
        return Ok(None);
    };
    Ok(Some((id, workdir)))
}

fn summary_record(
    kind: AgentCliKind,
    path: &Path,
    fallback_workdir: Option<&Path>,
) -> Result<Option<HistoryRecordFacts>, String> {
    let Some(parsed) = parse_transcript(kind, path, "Qoder", METADATA_SOURCE)? else {
        return Ok(None);
    };
    let mut summary: CliSessionSummary = parsed.summary;
    if summary.workdir.is_empty() {
        if let Some(workdir) = fallback_workdir {
            summary.workdir = workdir.to_string_lossy().into_owned();
        }
    }
    summary.can_resume = true;
    let native_origin_workdir =
        (!summary.workdir.is_empty()).then(|| PathBuf::from(&summary.workdir));
    let record = SessionHistoryRecord {
        record_key: summary.id.clone(),
        summary,
        locator: path.to_path_buf(),
        role: AgentSessionRole::Main,
        parent_native_id: None,
        resume_reason: None,
        content_unavailable_reason: None,
    };
    Ok(Some(HistoryRecordFacts {
        record,
        native_origin_workdir,
    }))
}

fn transcript(record: &SessionHistoryRecord) -> Result<Vec<SessionTranscriptSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    if !record.locator.is_file() {
        return Err("Qoder 会话文件不存在".into());
    }
    Ok(vec![SessionTranscriptSource {
        path: record.locator.clone(),
        parser_version: PARSER_VERSION,
        format: SessionTranscriptFormat::Lines,
        window_safe: true,
        sqlite: None,
        decode: decoder::decode,
    }])
}

fn field(fields: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        fields
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}
