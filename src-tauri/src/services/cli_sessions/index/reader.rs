//! The list, full-text search and reader address the same persisted messages.
//! Content is sliced in SQLite chunks; opening a page never loads the full transcript.
use super::{HistoryIndex, MESSAGE_CHUNK_CHARS, MESSAGE_CHUNK_OVERLAP_CHARS};
use crate::{
    models::*,
    services::agent_cli::contracts::{
        SessionContentSearchRequest, SessionContentSearchResult, SessionReadBudget,
    },
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const PAGE_MESSAGES: usize = 80;
const PREVIEW_CHARS: usize = 8 * 1024;
const PAGE_CHARS: usize = 256 * 1024;
pub(crate) const CONTENT_PAGE_CHARS: usize = 128 * 1024;
const CHUNK_STRIDE: usize = MESSAGE_CHUNK_CHARS - MESSAGE_CHUNK_OVERLAP_CHARS;

pub(super) fn message_key(source: &str, id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(source.as_bytes());
    hash.update([0]);
    hash.update(id.as_bytes());
    format!("{:x}", hash.finalize())
}

struct Header {
    id: String,
    source: String,
    native_id: String,
    role: CliSessionMessageRole,
    metadata: SessionMessageMetadata,
    content_version: u64,
    length: usize,
    preview: String,
}

fn headers(
    connection: &Connection,
    workspace: &str,
    session: &str,
    message_id: Option<&str>,
) -> Result<Vec<Header>, String> {
    let mut statement = connection.prepare(
        "SELECT p.public_id,m.source_key,m.message_id,m.role,p.metadata,
         MAX(m.chunk_index*?3+length(m.content)),
         MAX(CASE WHEN m.chunk_index=0 THEN substr(m.content,1,120) ELSE '' END),p.content_version
         FROM visible_messages m JOIN message_positions p
         ON p.workspace=m.workspace AND p.session_id=m.session_id
         AND p.source_key=m.source_key AND p.message_id=m.message_id
         WHERE m.workspace=?1 AND m.session_id=?2 AND (?4 IS NULL OR p.public_id=?4)
         GROUP BY m.source_key,m.message_id ORDER BY m.source_key,MIN(m.message_order),m.message_id"
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![workspace, session, CHUNK_STRIDE as i64, message_id],
            |row| {
                let role: String = row.get(3)?;
                let metadata: String = row.get(4)?;
                Ok(Header {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    native_id: row.get(2)?,
                    role: match role.as_str() {
                        "assistant" => CliSessionMessageRole::Assistant,
                        "tool" => CliSessionMessageRole::Tool,
                        _ => CliSessionMessageRole::User,
                    },
                    metadata: serde_json::from_str(&metadata).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                    length: row.get::<_, i64>(5)?.max(0) as usize,
                    preview: row.get(6)?,
                    content_version: row.get(7)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        super::super::workbench::check_read_budget()?;
        result.push(row.map_err(|error| error.to_string())?);
    }
    Ok(result)
}

fn revision(connection: &Connection, workspace: &str, session: &str) -> Result<String, String> {
    let mut statement = connection.prepare("SELECT source_key,checkpoint FROM session_sources WHERE workspace=?1 AND session_id=?2 ORDER BY source_key").map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    hash.update(workspace.as_bytes());
    hash.update([0]);
    hash.update(session.as_bytes());
    let rows = statement
        .query_map(params![workspace, session], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (source, checkpoint) = row.map_err(|error| error.to_string())?;
        let checkpoint: serde_json::Value =
            serde_json::from_str(&checkpoint).map_err(|error| error.to_string())?;
        hash.update(source.as_bytes());
        for field in ["identity", "fingerprint"] {
            hash.update(
                checkpoint
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .as_bytes(),
            );
            hash.update([0]);
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn read_slice(
    connection: &Connection,
    workspace: &str,
    session: &str,
    header: &Header,
    offset: usize,
    limit: usize,
) -> Result<String, String> {
    let first = offset / CHUNK_STRIDE;
    let last = offset.saturating_add(limit) / CHUNK_STRIDE;
    let mut statement = connection.prepare("SELECT chunk_index,content FROM messages WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_id=?4 AND chunk_index BETWEEN ?5 AND ?6 ORDER BY chunk_index").map_err(|error|error.to_string())?;
    let rows = statement
        .query_map(
            params![
                workspace,
                session,
                header.source,
                header.native_id,
                first as i64,
                last as i64
            ],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let mut content = String::new();
    let mut written = 0;
    for row in rows {
        super::super::workbench::check_read_budget()?;
        let (index, text) = row.map_err(|error| error.to_string())?;
        let start = index * CHUNK_STRIDE;
        let skip = offset.saturating_add(written).saturating_sub(start);
        for character in text.chars().skip(skip).take(limit.saturating_sub(written)) {
            content.push(character);
            written += 1;
        }
        if written >= limit {
            break;
        }
    }
    Ok(content)
}

pub(super) fn search_hits(
    connection: &Connection,
    workspace: &str,
    session: Option<&str>,
    request: &SessionContentSearchRequest,
) -> Result<HashMap<String, Vec<SessionSearchHit>>, String> {
    let mut found: HashMap<String, Vec<SessionSearchHit>> = HashMap::new();
    let mut positions: HashMap<(String, String), usize> = HashMap::new();
    for term in &request.terms {
        super::super::workbench::check_read_budget()?;
        let fts = term.value.chars().count() >= 3;
        let condition = if fts {
            "JOIN message_fts f ON f.rowid=m.row_id WHERE message_fts MATCH ?3"
        } else {
            "WHERE instr(lower(m.content),?3)>0"
        };
        // A list needs one example per term/session, while a reader needs one
        // per message. Never transfer every matching chunk into Rust.
        let group = if session.is_some() {
            "m.source_key,m.message_id"
        } else {
            "m.session_id"
        };
        let sql=format!("WITH candidates AS (
          SELECT MIN(m.row_id) AS row_id FROM visible_messages m {condition}
          AND m.searchable=1 AND m.workspace=?1 AND (?2 IS NULL OR m.session_id=?2) GROUP BY {group}
        ) SELECT m.session_id,p.public_id,m.content,p.metadata,m.chunk_index
          FROM candidates c JOIN messages m ON m.row_id=c.row_id JOIN message_positions p
          ON p.workspace=m.workspace AND p.session_id=m.session_id AND p.source_key=m.source_key AND p.message_id=m.message_id
          ORDER BY m.source_key,m.message_order,m.chunk_index");
        let needle = if fts {
            format!("\"{}\"", term.value.replace('"', "\"\""))
        } else {
            term.value.clone()
        };
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![workspace, session, needle], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, usize>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            super::super::workbench::check_read_budget()?;
            let (session, id, content, metadata, chunk) = row.map_err(|error| error.to_string())?;
            let key = (session.clone(), id.clone());
            let entries = found.entry(session).or_default();
            if let Some(position) = positions.get(&key) {
                let hit = &mut entries[*position];
                if !hit.terms.contains(&term.value) {
                    hit.terms.push(term.value.clone());
                }
            } else {
                let metadata: SessionMessageMetadata =
                    serde_json::from_str(&metadata).map_err(|error| error.to_string())?;
                let offset = chunk * CHUNK_STRIDE + match_offset(&content, &term.value);
                positions.insert(key, entries.len());
                entries.push(SessionSearchHit {
                    message_id: id,
                    offset,
                    snippet: snippet(&content, offset - chunk * CHUNK_STRIDE),
                    terms: vec![term.value.clone()],
                    kind: metadata.kind,
                });
            }
        }
    }
    Ok(found)
}

pub(super) fn content_result(
    hits: &[SessionSearchHit],
    request: &SessionContentSearchRequest,
) -> SessionContentSearchResult {
    SessionContentSearchResult {
        matched_term_indexes: request
            .terms
            .iter()
            .filter(|term| hits.iter().any(|hit| hit.terms.contains(&term.value)))
            .map(|term| term.index)
            .collect(),
        has_content: !hits.is_empty(),
        hit: hits.first().cloned(),
    }
}

fn match_offset(content: &str, term: &str) -> usize {
    // Lowercasing can expand one Unicode character (for example İ). Preserve
    // offsets in the original string rather than indexing it with folded bytes.
    let mut folded = String::new();
    let mut origins = Vec::new();
    for (offset, character) in content.chars().enumerate() {
        for lower in character.to_lowercase() {
            origins.extend(std::iter::repeat_n(offset, lower.len_utf8()));
            folded.push(lower);
        }
    }
    folded
        .find(&term.to_lowercase())
        .and_then(|byte| origins.get(byte))
        .copied()
        .unwrap_or(0)
}

fn snippet(content: &str, position: usize) -> String {
    let start = position.saturating_sub(64);
    let text: String = content.chars().skip(start).take(210).collect();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        text.trim(),
        if content.chars().count() > start + 210 {
            "…"
        } else {
            ""
        }
    )
}

fn enrich_tools(headers: &mut [Header]) {
    let calls: HashMap<_, _> = headers
        .iter()
        .filter(|header| header.metadata.kind == SessionMessageKind::ToolCall)
        .filter_map(|header| {
            header
                .metadata
                .call_id
                .as_ref()
                .map(|id| ((header.source.clone(), id.clone()), header.metadata.clone()))
        })
        .collect();
    let results: HashMap<_, _> = headers
        .iter()
        .filter(|header| header.metadata.kind == SessionMessageKind::ToolResult)
        .filter_map(|header| {
            header
                .metadata
                .call_id
                .as_ref()
                .map(|id| ((header.source.clone(), id.clone()), header.metadata.clone()))
        })
        .collect();
    for header in headers {
        let Some(id) = &header.metadata.call_id else {
            continue;
        };
        let key = (header.source.clone(), id.clone());
        if header.metadata.kind == SessionMessageKind::ToolResult {
            if let Some(call) = calls.get(&key) {
                header.metadata.tool_name = call.tool_name.clone();
                header.metadata.tool_kind = call.tool_kind;
                header.metadata.title = call.title.clone();
                if header.metadata.turn_id.is_none() {
                    header.metadata.turn_id = call.turn_id.clone();
                }
            }
        } else if let Some(result) = results.get(&key) {
            header.metadata.status = result.status;
            header.metadata.exit_code = result.exit_code;
        }
    }
}

#[cfg(test)]
pub(super) fn read_all_for_test(
    connection: &Connection,
    workspace: &str,
    session: &str,
) -> Result<Vec<crate::services::agent_cli::contracts::SessionIndexMessage>, String> {
    let mut headers = headers(connection, workspace, session, None)?;
    enrich_tools(&mut headers);
    headers
        .into_iter()
        .map(|header| {
            let content = read_slice(connection, workspace, session, &header, 0, header.length)?;
            Ok(crate::services::agent_cli::contracts::SessionIndexMessage {
                id: header.native_id,
                role: header.role,
                content,
                metadata: header.metadata,
            })
        })
        .collect()
}

impl HistoryIndex {
    pub(crate) fn read_page(
        &self,
        workspace: &str,
        session: &str,
        summary: CliSessionSummary,
        request: &AgentSessionDetailRequest,
        indexing: bool,
        budget: &SessionReadBudget,
    ) -> Result<CliSessionDetail, String> {
        self.connection(budget, |connection| {
            let revision = revision(connection, workspace, session)?;
            let mut headers = headers(connection, workspace, session, None)?;
            enrich_tools(&mut headers);
            let mut outline = Vec::<SessionTurnSummary>::new();
            let mut current_turn = None::<String>;
            let mut turns = HashMap::<String, usize>::new();
            for header in &mut headers {
                if header.role == CliSessionMessageRole::User
                    && matches!(
                        header.metadata.kind,
                        SessionMessageKind::Text | SessionMessageKind::Image
                    )
                    && !header.metadata.injected
                {
                    let id = header
                        .metadata
                        .turn_id
                        .clone()
                        .unwrap_or_else(|| header.id.clone());
                    if !turns.contains_key(&id) {
                        turns.insert(id.clone(), outline.len());
                        outline.push(SessionTurnSummary {
                            id: id.clone(),
                            message_id: header.id.clone(),
                            title: if header.metadata.kind == SessionMessageKind::Image {
                                "图片或附件".into()
                            } else {
                                super::super::clean_text(&header.preview, 100)
                                    .unwrap_or_else(|| "图片或附件".into())
                            },
                            timestamp: header.metadata.timestamp.clone(),
                            steps: 0,
                            failures: 0,
                        });
                    }
                    current_turn = Some(id);
                }
                if header
                    .metadata
                    .turn_id
                    .as_ref()
                    .is_none_or(|id| !turns.contains_key(id))
                {
                    header.metadata.turn_id = current_turn.clone();
                }
                if let Some(index) = header
                    .metadata
                    .turn_id
                    .as_ref()
                    .and_then(|id| turns.get(id))
                {
                    if header.metadata.kind == SessionMessageKind::ToolCall {
                        outline[*index].steps += 1;
                    }
                    if header.metadata.kind == SessionMessageKind::ToolResult
                        && header.metadata.status == Some(SessionToolStatus::Failed)
                    {
                        outline[*index].failures += 1;
                    }
                }
            }
            let query = super::super::SearchQuery::new(&request.query)?;
            let terms = super::super::SearchAccumulator::new(&query).content_request();
            let hits = search_hits(connection, workspace, Some(session), &terms)?
                .remove(session)
                .unwrap_or_default();
            let ids: HashSet<&str> = hits.iter().map(|hit| hit.message_id.as_str()).collect();
            let matching: Vec<usize> = headers
                .iter()
                .enumerate()
                .filter_map(|(index, header)| ids.contains(header.id.as_str()).then_some(index))
                .collect();
            let anchor = request
                .anchor
                .as_ref()
                .and_then(|id| headers.iter().position(|header| &header.id == id));
            let focus = anchor.or_else(|| {
                (request.cursor.is_none() && !matching.is_empty())
                    .then(|| matching[request.match_position.unwrap_or(0).min(matching.len() - 1)])
            });
            let position =
                focus.and_then(|index| matching.iter().position(|candidate| *candidate == index));
            let (start, stop) = if let Some(cursor) = &request.cursor {
                let mut parts = cursor.split(':');
                let version = parts.next().ok_or("会话分页位置无效")?;
                if version != revision {
                    return Err("会话内容已更新，请刷新后继续阅读".into());
                }
                let start = parts
                    .next()
                    .ok_or("会话分页位置无效")?
                    .parse::<usize>()
                    .map_err(|_| "会话分页位置无效")?;
                let stop = parts
                    .next()
                    .map(str::parse::<usize>)
                    .transpose()
                    .map_err(|_| "会话分页位置无效")?
                    .unwrap_or(headers.len());
                if parts.next().is_some() {
                    return Err("会话分页位置无效".into());
                }
                (start, stop)
            } else {
                (focus.unwrap_or(0).saturating_sub(8), headers.len())
            };
            if start > stop || stop > headers.len() {
                return Err("会话分页位置已失效，请刷新".into());
            }
            let mut messages = Vec::new();
            let mut chars = 0;
            for header in headers.iter().take(stop).skip(start).take(PAGE_MESSAGES) {
                if chars >= PAGE_CHARS {
                    break;
                }
                let content_offset = if request.cursor.is_none()
                    && focus.is_some_and(|index| headers[index].id == header.id)
                {
                    hits.iter()
                        .find(|hit| hit.message_id == header.id)
                        .map(|hit| hit.offset.saturating_sub(300))
                        .unwrap_or(0)
                } else {
                    0
                };
                let content = read_slice(
                    connection,
                    workspace,
                    session,
                    header,
                    content_offset,
                    PREVIEW_CHARS.min(PAGE_CHARS - chars),
                )?;
                let loaded = content.chars().count();
                chars += loaded;
                let changes = if loaded == header.length {
                    super::super::presentation::file_changes(&header.metadata, &content)
                } else {
                    Vec::new()
                };
                messages.push(CliSessionMessage {
                    id: header.id.clone(),
                    role: header.role,
                    content,
                    timestamp: header.metadata.timestamp.clone(),
                    model: header.metadata.model.clone(),
                    tool_name: header.metadata.tool_name.clone(),
                    metadata: header.metadata.clone(),
                    content_version: header.content_version,
                    content_length: header.length,
                    content_offset,
                    has_more_content: content_offset + loaded < header.length,
                    changes,
                });
            }
            let end = start + messages.len();
            let mut previous_start = start;
            let mut previous_chars = 0;
            while previous_start > 0 && start - previous_start < PAGE_MESSAGES {
                let length = headers[previous_start - 1].length.min(PREVIEW_CHARS);
                if previous_chars + length > PAGE_CHARS {
                    break;
                }
                previous_chars += length;
                previous_start -= 1;
            }
            Ok(CliSessionDetail {
                usage: super::usage::read(connection, workspace, session, indexing)?,
                session: summary,
                messages,
                page: SessionReadPage {
                    revision: revision.clone(),
                    previous_cursor: (start > 0)
                        .then(|| format!("{revision}:{previous_start}:{start}")),
                    next_cursor: (end < headers.len() || indexing)
                        .then(|| format!("{revision}:{end}")),
                    total_messages: headers.len(),
                    start,
                    indexing,
                },
                outline,
                find: SessionFindState {
                    query: request.query.trim().into(),
                    total: matching.len(),
                    position,
                    active_message_id: focus
                        .and_then(|index| headers.get(index).map(|header| header.id.clone())),
                },
            })
        })
    }

    pub(crate) fn read_content(
        &self,
        workspace: &str,
        session: &str,
        request: &AgentSessionMessageRequest,
        budget: &SessionReadBudget,
    ) -> Result<AgentSessionMessageContent, String> {
        self.connection(budget, |connection| {
            if revision(connection, workspace, session)? != request.revision {
                return Err("会话内容已更新，请刷新后展开".into());
            }
            let headers = headers(connection, workspace, session, Some(&request.message_id))?;
            let header = headers
                .iter()
                .find(|header| header.id == request.message_id)
                .ok_or("该消息已不存在，请刷新会话")?;
            if header.content_version != request.content_version {
                return Err("消息内容已更新，请刷新后展开".into());
            }
            if request.offset > header.length {
                return Err("消息读取位置无效".into());
            }
            let content = read_slice(
                connection,
                workspace,
                session,
                header,
                request.offset,
                CONTENT_PAGE_CHARS,
            )?;
            let end = request.offset + content.chars().count();
            let mut metadata = header.metadata.clone();
            if end == header.length && metadata.kind == SessionMessageKind::ToolResult && metadata.tool_kind.is_none() {
                if let Some(call_id) = &metadata.call_id {
                    let call: Option<String> = connection.query_row(
                        "SELECT metadata FROM message_positions WHERE workspace=?1 AND session_id=?2 AND source_key=?3
                        AND json_extract(metadata,'$.callId')=?4 AND json_extract(metadata,'$.kind')='toolCall' LIMIT 1",
                        params![workspace,session,header.source,call_id], |row| row.get(0)
                    ).optional().map_err(|error|error.to_string())?;
                    if let Some(call) = call {
                        let call: SessionMessageMetadata = serde_json::from_str(&call).map_err(|error|error.to_string())?;
                        metadata.tool_kind = call.tool_kind;
                    }
                }
            }
            let changes = if end == header.length
                && matches!(metadata.tool_kind, Some(SessionToolKind::Edit | SessionToolKind::Write)) {
                let complete = if request.offset == 0 {
                    content.clone()
                } else {
                    read_slice(connection, workspace, session, header, 0, header.length)?
                };
                super::super::presentation::file_changes(&metadata, &complete)
            } else {
                Vec::new()
            };
            Ok(AgentSessionMessageContent {
                message_id: header.id.clone(),
                content,
                offset: request.offset,
                total_chars: header.length,
                next_offset: (end < header.length).then_some(end),
                changes,
            })
        })
    }
}
