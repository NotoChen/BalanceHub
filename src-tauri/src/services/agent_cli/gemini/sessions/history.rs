use super::*;
use crate::{
    models::AgentSessionRole,
    services::{
        agent_cli::contracts::{
            SessionHistoryAdapter, SessionHistoryReadMode, SessionHistoryRecord,
            SessionHistorySource, SessionHistoryWorkspace, SessionReadBudget,
        },
        cli_sessions::workbench::{check_read_budget, read_session_metadata_text_file_limited},
    },
};

pub(crate) const HISTORY: SessionHistoryAdapter = SessionHistoryAdapter {
    source,
    scan,
    detail,
    search,
    index,
};
fn source() -> Result<SessionHistorySource, String> {
    let config_root =
        super::super::config::config_dir().ok_or_else(|| "无法定位 Gemini 原生目录".to_string())?;
    let (launch_environment, resume_reason) =
        match super::super::native_context::launch_environment(&config_root) {
            Ok(environment) => (environment, None),
            Err(reason) => (
                crate::services::agent_cli::contracts::EnvironmentPatch::default(),
                Some(reason.to_owned()),
            ),
        };
    Ok(SessionHistorySource {
        config_root,
        launch_environment,
        resume_reason,
    })
}

// The project map and direct .project_root markers are read once per source,
// then reused for every selected workdir, including missing mount paths.
#[derive(Default)]
pub(super) struct ProjectIndex {
    projects: HashMap<String, BTreeSet<String>>,
    diagnostics: Vec<String>,
}
impl ProjectIndex {
    pub(super) fn read(config_dir: &Path) -> Result<Self, String> {
        let mut index = Self::default();
        let mapping = config_dir.join("projects.json");
        if mapping.is_file() {
            match read_session_metadata_text_file_limited(
                &mapping,
                4 * 1024 * 1024,
                "读取 Gemini 项目索引",
            )
            .and_then(|text| {
                serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())
            }) {
                Ok(value) => {
                    if let Some(projects) = value.get("projects").and_then(Value::as_object) {
                        for (path, id) in projects {
                            if let Some(id) = id.as_str().filter(|id| safe_component(id)) {
                                index
                                    .projects
                                    .entry(path_key(Path::new(path)))
                                    .or_default()
                                    .insert(id.into());
                            }
                        }
                    }
                }
                Err(error) => index.diagnostics.push(error),
            }
        }
        let entries = match fs::read_dir(config_dir.join("tmp")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(index),
            Err(error) => {
                index.diagnostics.push(error.to_string());
                return Ok(index);
            }
        };
        for entry in entries {
            check_read_budget()?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    index.diagnostics.push(error.to_string());
                    continue;
                }
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let marker = entry.path().join(".project_root");
            if !marker.is_file() {
                continue;
            }
            match read_session_metadata_text_file_limited(&marker, 4096, "读取 Gemini 项目目录标记")
            {
                Ok(path) => {
                    index
                        .projects
                        .entry(path_key(Path::new(path.trim())))
                        .or_default()
                        .insert(entry.file_name().to_string_lossy().to_string());
                }
                Err(error) => index.diagnostics.push(error),
            }
        }
        Ok(index)
    }
    pub(super) fn ids(&self, root: &Path, workdir: &Path) -> BTreeSet<String> {
        let mut ids = self
            .projects
            .get(&path_key(workdir))
            .cloned()
            .unwrap_or_default();
        for path in [workdir.to_path_buf(), canonical_or_original(workdir)] {
            let id = format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()));
            if root.join("tmp").join(&id).join("chats").is_dir() {
                ids.insert(id);
            }
        }
        ids
    }
}
fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|value| value.is_control() || value == '/' || value == '\\')
}
fn scan(
    kind: AgentCliKind,
    root: &Path,
    workdirs: &[PathBuf],
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<Vec<SessionHistoryWorkspace>, String> {
    let project_index = ProjectIndex::read(root)?;
    let mut result = Vec::new();
    for workdir in workdirs {
        let mut workspace = SessionHistoryWorkspace::new(workdir);
        for diagnostic in &project_index.diagnostics {
            workspace.failed(diagnostic.clone());
        }
        for project_id in project_index.ids(root, workdir) {
            let chats = root.join("tmp").join(project_id).join("chats");
            if let Err(error) = scan_chats(kind, &chats, &mut workspace, budget, mode) {
                workspace.failed(error);
            }
        }
        result.push(workspace);
    }
    Ok(result)
}
fn scan_chats(
    kind: AgentCliKind,
    chats: &Path,
    workspace: &mut SessionHistoryWorkspace,
    budget: &SessionReadBudget,
    mode: SessionHistoryReadMode,
) -> Result<(), String> {
    budget.check()?;
    let entries = match fs::read_dir(chats) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    for entry in entries {
        budget.check()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                workspace.failed(error.to_string());
                continue;
            }
        };
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_file() {
            add_record(kind, &entry.path(), None, workspace, mode);
        }
        if ty.is_dir() {
            let parent = entry.file_name().to_string_lossy().to_string();
            // Native sanitizer is reversible for UUID IDs only. For any other
            // spelling keep relationship unknown rather than inventing an ID.
            let parent = valid_native_uuid(&parent).then_some(parent);
            let children = fs::read_dir(entry.path()).map_err(|e| e.to_string())?;
            for child in children {
                budget.check()?;
                match child {
                    Ok(child) if child.file_type().is_ok_and(|ty| ty.is_file()) => {
                        add_record(kind, &child.path(), parent.clone(), workspace, mode)
                    }
                    Ok(_) => {}
                    Err(error) => workspace.failed(error.to_string()),
                }
            }
        }
    }
    Ok(())
}
fn valid_native_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
fn add_record(
    kind: AgentCliKind,
    path: &Path,
    parent_native_id: Option<String>,
    workspace: &mut SessionHistoryWorkspace,
    mode: SessionHistoryReadMode,
) {
    if !path
        .extension()
        .is_some_and(|ext| ext == "jsonl" || ext == "json")
    {
        return;
    }
    let result = crate::services::cli_sessions::workbench::cached_history_record(path, || {
        if mode == SessionHistoryReadMode::Identities {
            return crate::services::cli_sessions::read_session_header(
                path,
                &["sessionId"],
                |fields, _| native_session_id(fields.get("sessionId")).is_some(),
            )
            .map(|metadata| {
                metadata
                    .and_then(|metadata| native_session_id(metadata.get("sessionId")))
                    .map(|id| {
                        SessionHistoryRecord::identity(kind, id, &workspace.workdir, path.into())
                    })
            });
        }
        Ok(
            parse_session(kind, path, &workspace.workdir)?.map(|(summary, child)| {
                SessionHistoryRecord {
                    record_key: summary.id.clone(),
                    summary,
                    locator: path.into(),
                    role: if child {
                        AgentSessionRole::Subagent
                    } else {
                        AgentSessionRole::Main
                    },
                    parent_native_id: if child { parent_native_id } else { None },
                    resume_reason: child.then(|| "Gemini 子会话需由父会话继续".into()),
                    content_unavailable_reason: None,
                }
            }),
        )
    });
    match result {
        Ok(Some(record)) => workspace.records.push(record),
        Ok(None) => {}
        Err(error) => workspace.failed(error),
    }
}

fn search(
    record: &SessionHistoryRecord,
    request: &SessionContentSearchRequest,
    current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    search_conversation(&record.locator, request, current)
}
fn index(record: &SessionHistoryRecord) -> Result<Vec<SessionIndexSource>, String> {
    if let Some(reason) = &record.content_unavailable_reason {
        return Err(reason.clone());
    }
    Ok(vec![index_source(&record.locator)])
}
fn detail(
    record: &SessionHistoryRecord,
    limits: SessionReadLimits,
) -> Result<CliSessionDetail, String> {
    let (conversation, source_truncated) =
        load_conversation_limited(&record.locator, limits.max_file_bytes)?;
    let mut collector = SessionMessageCollector::new(limits);
    for (index, message) in conversation.messages.into_iter().enumerate() {
        let timestamp = normalize_timestamp(message.timestamp.as_deref());
        let model = message.model.clone();
        match message.kind.as_str() {
            "user" => {
                if let Some(text) = message.text {
                    if !is_ignored_user_content(text.trim()) {
                        collector.push(
                            format!("gemini-{index}"),
                            CliSessionMessageRole::User,
                            text,
                            timestamp.clone(),
                            None,
                            None,
                        );
                    }
                }
            }
            "gemini" => {
                if let Some(text) = message.text {
                    collector.push(
                        format!("gemini-{index}"),
                        CliSessionMessageRole::Assistant,
                        text,
                        timestamp.clone(),
                        model.clone(),
                        None,
                    );
                }
            }
            kind if kind.contains("tool") => {
                if let Some(text) = message.text {
                    collector.push(
                        format!("gemini-{index}"),
                        CliSessionMessageRole::Tool,
                        text,
                        timestamp.clone(),
                        model.clone(),
                        None,
                    );
                }
            }
            _ => {}
        }
        for (tool_index, tool) in message.tool_calls.into_iter().enumerate() {
            collector.push(
                format!("gemini-{index}-tool-{tool_index}"),
                CliSessionMessageRole::Tool,
                tool.content,
                timestamp.clone(),
                model.clone(),
                Some(tool.name),
            );
        }
    }
    let (messages, truncated, omitted_message_count) = collector.finish(source_truncated);
    Ok(CliSessionDetail {
        session: record.summary.clone(),
        messages,
        truncated,
        omitted_message_count,
        content_source: "geminiTranscript".to_string(),
    })
}
