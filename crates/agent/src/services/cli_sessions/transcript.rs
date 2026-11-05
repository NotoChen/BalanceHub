//! On-demand transcript reading. Native records are decoded when a list search,
//! reader page or message expansion actually needs them; there is no derived
//! database to build, migrate or clean up.
//!
//! The decoders still emit the same checkpointed mutation stream used for
//! incremental reads, so rewinds, replaces and appends keep their native
//! meaning. The stream is applied to a request-local transcript instead of
//! SQLite.
use super::{presentation, record_stream as stream, SearchAccumulator, SearchQuery};
use crate::{
    models::*,
    services::agent_cli::contracts::{
        SessionContentSearchRequest, SessionContentSearchResult, SessionHistoryAdapter,
        SessionHistoryRecord, SessionReadBudget, SessionTranscriptFormat, SessionTranscriptMessage,
        SessionTranscriptMutation, SessionTranscriptSource,
    },
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

const PAGE_MESSAGES: usize = 80;
const PREVIEW_CHARS: usize = 8 * 1024;
const PAGE_CHARS: usize = 256 * 1024;
pub(crate) const CONTENT_PAGE_CHARS: usize = 128 * 1024;
/// A single native transcript larger than this is not decoded in full when it
/// cannot be read as a window. Search keeps its own full-read limit.
pub(crate) const MAX_TRANSCRIPT_BYTES: u64 = 256 * 1024 * 1024;
/// A single-source JSONL transcript above this size is opened as a window, so
/// the reader never decodes a multi-hundred-megabyte body at once.
const WINDOWED_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
/// Bounded window decoded for an oversized transcript. Showing the latest
/// turns keeps the detail view usable without loading a multi-gigabyte file.
const TRANSCRIPT_WINDOW_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
struct Message {
    source: String,
    native_id: String,
    public_id: String,
    role: CliSessionMessageRole,
    content: String,
    metadata: SessionMessageMetadata,
    priority: i64,
    order: u64,
    content_version: u64,
    length: usize,
    preview: String,
}

#[derive(Default)]
struct SourceState {
    messages: HashMap<String, Message>,
    positions: HashMap<String, u64>,
    usage: BTreeMap<String, SessionUsageRecord>,
}

impl SourceState {
    fn reset(&mut self) {
        self.messages.clear();
        self.positions.clear();
        self.usage.clear();
    }

    fn apply(&mut self, source: &str, order: u64, mutation: SessionTranscriptMutation) {
        match mutation {
            SessionTranscriptMutation::Usage(record) => put_usage(&mut self.usage, &record),
            SessionTranscriptMutation::Put { message, priority } => {
                self.put(source, message, priority, order, false)
            }
            SessionTranscriptMutation::Append { message, priority } => {
                self.put(source, message, priority, order, true)
            }
            SessionTranscriptMutation::Clear => self.reset(),
            SessionTranscriptMutation::RemoveGroup(id) => {
                self.positions.entry(id.clone()).or_insert(order);
                self.usage
                    .retain(|_, record| record.message_id.as_deref() != Some(id.as_str()));
                let prefix = format!("{id}:");
                self.messages
                    .retain(|message_id, _| message_id != &id && !message_id.starts_with(&prefix));
            }
            SessionTranscriptMutation::Rewind(id) => {
                let position = self.positions.get(&id).copied();
                match position {
                    Some(position) => {
                        self.usage.retain(|_, record| {
                            record
                                .message_id
                                .as_deref()
                                .and_then(|id| self.positions.get(id))
                                .is_none_or(|order| *order < position)
                        });
                        self.messages.retain(|_, message| message.order < position);
                        self.positions.retain(|_, order| *order < position);
                    }
                    None => self.reset(),
                }
            }
        }
    }

    fn put(
        &mut self,
        source: &str,
        message: SessionTranscriptMessage,
        priority: i64,
        order: u64,
        append: bool,
    ) {
        let content_version = order;
        let order = match message.metadata.group_id.as_deref() {
            Some(group) => *self.positions.entry(group.to_string()).or_insert(order),
            None => order,
        };
        let saved_order = *self.positions.entry(message.id.clone()).or_insert(order);
        let previous = self
            .messages
            .remove(&message.id)
            .filter(|_| append)
            .map(|previous| previous.content);
        let content = match previous {
            Some(previous) => previous + &message.content,
            None => message.content,
        };
        if content.is_empty() {
            return;
        }
        let length = content.chars().count();
        let preview = content.chars().take(120).collect();
        self.messages.insert(
            message.id.clone(),
            Message {
                source: source.to_string(),
                public_id: message_key(source, &message.id),
                native_id: message.id,
                role: message.role,
                content,
                metadata: message.metadata,
                priority,
                order: saved_order,
                content_version,
                length,
                preview,
            },
        );
    }
}

fn put_usage(usage: &mut BTreeMap<String, SessionUsageRecord>, record: &SessionUsageRecord) {
    let id = crate::services::agent_cli::cache::key(record.id.as_bytes());
    let mut record = record.clone();
    if let Some(previous) = usage.get(&id) {
        let decreased = record.tokens.retain_maximum(&previous.tokens);
        record.uncertain |= previous.uncertain || decreased;
        if previous.model != record.model {
            record.model = None;
        }
    }
    usage.insert(id, record);
}

fn message_key(source: &str, id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(source.as_bytes());
    hash.update([0]);
    hash.update(id.as_bytes());
    format!("{:x}", hash.finalize())
}

/// A cheap identity for the native files behind one transcript. The reader
/// cache uses it to notice appends, rewrites and relocations without decoding.
pub(crate) fn sources_signature(sources: &[SessionTranscriptSource]) -> Result<String, String> {
    let mut parts = Vec::with_capacity(sources.len());
    for source in sources {
        parts.push((
            source.path.to_string_lossy().into_owned(),
            stream::source_signature(source)?,
        ));
    }
    parts.sort();
    let mut hash = Sha256::new();
    for (path, signature) in parts {
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(signature.as_bytes());
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// The byte range a windowed transcript actually covers. `has_earlier` and
/// `has_later` are derived at read time; the window never merges with another
/// one, so every page is decoded from one self-contained file range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceWindow {
    pub start: u64,
    pub end: u64,
    pub file_bytes: u64,
}

/// Public window facts, including the estimated fragment position used by
/// whole-session search and the reader page bar.
pub(crate) fn session_read_window(window: SourceWindow) -> SessionReadWindow {
    let has_later = window.end < window.file_bytes;
    // Window boundaries are record-aligned, so fragment numbers are
    // estimates. The last window is always shown as the final fragment.
    let total = window.file_bytes.div_ceil(TRANSCRIPT_WINDOW_BYTES).max(1);
    let index = (window.start / TRANSCRIPT_WINDOW_BYTES + 1).min(total);
    SessionReadWindow {
        start: window.start,
        end: window.end,
        file_bytes: window.file_bytes,
        index: if has_later { index } else { total },
        total,
        has_earlier: window.start > 0,
        has_later,
    }
}

/// A decoded native transcript with the same visibility, ordering and usage
/// semantics the reader had when its messages lived in a derived index.
pub(crate) struct Transcript {
    pub(crate) complete: bool,
    pub(crate) skipped_records: u64,
    /// The source exceeded the full-decode limit; only one byte window is
    /// loaded. `complete` is false for such transcripts as well.
    pub(crate) truncated: bool,
    pub(crate) window: Option<SourceWindow>,
    revision: String,
    messages: Vec<Message>,
    usage: SessionUsageSummary,
}

impl Transcript {
    pub(crate) fn load(
        workspace: &str,
        session: &str,
        record: &SessionHistoryRecord,
        adapter: &SessionHistoryAdapter,
        budget: &SessionReadBudget,
    ) -> Result<Self, String> {
        let sources = (adapter.transcript)(record)?;
        Self::from_sources(workspace, session, &sources, budget)
    }

    pub(crate) fn from_sources(
        workspace: &str,
        session: &str,
        sources: &[SessionTranscriptSource],
        budget: &SessionReadBudget,
    ) -> Result<Self, String> {
        Self::from_sources_with_window(
            workspace,
            session,
            sources,
            stream::WindowRequest::Default,
            budget,
        )
    }

    pub(crate) fn from_sources_with_window(
        workspace: &str,
        session: &str,
        sources: &[SessionTranscriptSource],
        window_request: stream::WindowRequest,
        budget: &SessionReadBudget,
    ) -> Result<Self, String> {
        let mut signatures = Vec::new();
        let mut scans = Vec::new();
        let mut complete = true;
        let mut truncated = false;
        let mut skipped_records = 0u64;
        // A window is only meaningful when one file holds the whole
        // conversation. Multi-file sessions keep the previous per-source read
        // and do not expose window navigation.
        let allow_window = sources.len() == 1;
        for source in sources {
            budget.check()?;
            let path = source.path.to_string_lossy().into_owned();
            let signature = stream::source_signature(source)?;
            let scan = read_source(&path, source, allow_window, window_request, budget)?;
            complete &= scan.complete;
            truncated |= scan.truncated;
            skipped_records = skipped_records.saturating_add(scan.skipped_records);
            signatures.push((path, signature));
            scans.push(scan);
        }
        let window = (scans.len() == 1).then(|| scans[0].window).flatten();
        let window_content = window
            .and_then(|_| scans.first())
            .and_then(|scan| scan.content_hash.as_deref());
        let mut hash = Sha256::new();
        hash.update(workspace.as_bytes());
        hash.update([0]);
        hash.update(session.as_bytes());
        let revision = if let (Some(window), Some(content_hash)) = (window, window_content) {
            // A window's identity is its own bytes, not the whole file. An
            // append changes the file signature but not an earlier window, so
            // its revision, cursors and cached decode stay valid. The window
            // request itself is recovered from the suffix when a message is
            // expanded.
            hash.update(sources[0].path.to_string_lossy().as_bytes());
            hash.update([0]);
            hash.update(content_hash.as_bytes());
            hash.update([0]);
            hash.update(window.start.to_le_bytes());
            hash.update(window.end.to_le_bytes());
            format!("{:x}@{}-{}", hash.finalize(), window.start, window.end)
        } else {
            signatures.sort();
            for (path, signature) in &signatures {
                hash.update(path.as_bytes());
                hash.update([0]);
                hash.update(signature.as_bytes());
                hash.update([0]);
            }
            format!("{:x}", hash.finalize())
        };
        let (messages, usage) =
            visible_messages(scans.into_iter().map(|scan| scan.state).collect());
        Ok(Self {
            complete,
            skipped_records,
            truncated,
            window,
            revision,
            messages,
            usage,
        })
    }

    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }

    /// Whether this decoded slice contains the public message id. Used to
    /// decide whether a search hit needs a full transcript instead of the
    /// newest window.
    pub(crate) fn has_message(&self, message_id: &str) -> bool {
        self.messages
            .iter()
            .any(|message| message.public_id == message_id)
    }

    /// Approximate resident size of the decoded messages, used by the reader
    /// cache's byte budget. Metadata and allocator overhead are folded into a
    /// small per-message constant.
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.messages
            .iter()
            .map(|message| message.content.len().saturating_add(512))
            .sum::<usize>()
            .max(1)
    }

    pub(crate) fn search(
        &self,
        request: &SessionContentSearchRequest,
    ) -> SessionContentSearchResult {
        content_result(&self.search_hits(request), request)
    }

    pub(crate) fn search_hits(
        &self,
        request: &SessionContentSearchRequest,
    ) -> Vec<SessionSearchHit> {
        let mut found: Vec<SessionSearchHit> = Vec::new();
        let mut positions: HashMap<&str, usize> = HashMap::new();
        for term in &request.terms {
            for message in &self.messages {
                if message.metadata.kind == SessionMessageKind::Image {
                    continue;
                }
                let Some(offset) = match_offset(&message.content, &term.value) else {
                    continue;
                };
                if let Some(position) = positions.get(message.public_id.as_str()) {
                    let hit = &mut found[*position];
                    if !hit.terms.contains(&term.value) {
                        hit.terms.push(term.value.clone());
                    }
                } else {
                    positions.insert(&message.public_id, found.len());
                    found.push(SessionSearchHit {
                        message_id: message.public_id.clone(),
                        offset,
                        snippet: snippet(&message.content, offset),
                        terms: vec![term.value.clone()],
                        kind: message.metadata.kind,
                    });
                }
            }
        }
        found
    }

    pub(crate) fn read_page(
        &self,
        summary: CliSessionSummary,
        request: &AgentSessionDetailRequest,
        indexing: bool,
        budget: &SessionReadBudget,
    ) -> Result<CliSessionDetail, String> {
        let mut headers = self
            .messages
            .iter()
            .map(|message| Header {
                message,
                metadata: message.metadata.clone(),
            })
            .collect::<Vec<_>>();
        let mut outline = Vec::<SessionTurnSummary>::new();
        let mut current_turn = None::<String>;
        let mut turns = HashMap::<String, usize>::new();
        for header in &mut headers {
            budget.check()?;
            if header.message.role == CliSessionMessageRole::User
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
                    .unwrap_or_else(|| header.message.public_id.clone());
                if !turns.contains_key(&id) {
                    turns.insert(id.clone(), outline.len());
                    outline.push(SessionTurnSummary {
                        id: id.clone(),
                        message_id: header.message.public_id.clone(),
                        title: if header.metadata.kind == SessionMessageKind::Image {
                            "图片或附件".into()
                        } else {
                            super::clean_text(&header.message.preview, 100)
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
        let query = SearchQuery::new(&request.query)?;
        let terms = SearchAccumulator::new(&query).content_request();
        let hits = self.search_hits(&terms);
        let ids: std::collections::HashSet<&str> =
            hits.iter().map(|hit| hit.message_id.as_str()).collect();
        let matching: Vec<usize> = headers
            .iter()
            .enumerate()
            .filter_map(|(index, header)| {
                ids.contains(header.message.public_id.as_str())
                    .then_some(index)
            })
            .collect();
        let anchor = request.anchor.as_ref().and_then(|id| {
            headers
                .iter()
                .position(|header| &header.message.public_id == id)
        });
        let keep_revision = request.cursor.as_deref() == Some(self.revision.as_str());
        let focus = anchor.or_else(|| {
            ((request.cursor.is_none() || keep_revision) && !matching.is_empty())
                .then(|| matching[request.match_position.unwrap_or(0).min(matching.len() - 1)])
        });
        let window = self.window;
        let default_start = match focus {
            Some(index) => index.saturating_sub(8),
            // A window opens at the newest turn: the caller asked for the
            // session's recent context, not the oldest byte of the window.
            None if window.is_some() => last_page_start(&headers),
            None => 0,
        };
        let position =
            focus.and_then(|index| matching.iter().position(|candidate| *candidate == index));
        let (start, stop) = if let Some(cursor) = &request.cursor {
            let mut parts = cursor.split(':');
            let version = parts.next().ok_or("会话分页位置无效")?;
            // `earlier`, `later` and `latest` intentionally load a different
            // byte window. Their boundary is enough to validate the relative
            // move; a file append changes the whole-file revision but does not
            // move an earlier window. Page indices are window-relative and
            // still require the exact revision.
            match parts.next() {
                // A bare revision keeps the current transcript (full or one
                // window) and applies the search focus, so searching again
                // does not jump back to the newest window.
                None if version == self.revision => (default_start, headers.len()),
                Some("earlier") => {
                    let end = parts
                        .next()
                        .ok_or("会话分页位置无效")?
                        .parse::<u64>()
                        .map_err(|_| "会话分页位置无效")?;
                    if parts.next().is_some() || window.is_none_or(|value| value.end != end) {
                        return Err("会话分页位置已失效，请刷新".into());
                    }
                    (last_page_start(&headers), headers.len())
                }
                Some("later") => {
                    let offset = parts
                        .next()
                        .ok_or("会话分页位置无效")?
                        .parse::<u64>()
                        .map_err(|_| "会话分页位置无效")?;
                    if parts.next().is_some() || window.is_none_or(|value| value.start != offset) {
                        return Err("会话分页位置已失效，请刷新".into());
                    }
                    (0, headers.len())
                }
                Some("earliest") => {
                    if parts.next().is_some() || window.is_none() {
                        return Err("会话分页位置已失效，请刷新".into());
                    }
                    (0, headers.len())
                }
                Some("latest") => {
                    // "Latest" is relative to the file, not to the window it
                    // was requested from. An append changes the revision but
                    // must not turn this action into an error.
                    if parts.next().is_some() {
                        return Err("会话分页位置无效".into());
                    }
                    (last_page_start(&headers), headers.len())
                }
                Some(value) => {
                    if version != self.revision {
                        return Err("会话内容已更新，请刷新后继续阅读".into());
                    }
                    let start = value.parse::<usize>().map_err(|_| "会话分页位置无效")?;
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
                }
                // A bare revision asks for "the transcript I was reading".
                // If the source changed since that page was loaded, continue
                // with the refreshed transcript instead of trapping refresh
                // and retry in a stale-cursor error.
                None => (default_start, headers.len()),
            }
        } else {
            (default_start, headers.len())
        };
        if start > stop || stop > headers.len() {
            return Err("会话分页位置已失效，请刷新".into());
        }
        let mut messages = Vec::new();
        let mut chars = 0;
        for header in headers.iter().take(stop).skip(start).take(PAGE_MESSAGES) {
            budget.check()?;
            if chars >= PAGE_CHARS {
                break;
            }
            let content_offset = if (request.cursor.is_none() || keep_revision)
                && focus.is_some_and(|index| {
                    headers[index].message.public_id == header.message.public_id
                }) {
                hits.iter()
                    .find(|hit| hit.message_id == header.message.public_id)
                    .map(|hit| hit.offset.saturating_sub(300))
                    .unwrap_or(0)
            } else {
                0
            };
            let content = slice_chars(
                &header.message.content,
                content_offset,
                PREVIEW_CHARS.min(PAGE_CHARS - chars),
            );
            let loaded = content.chars().count();
            chars += loaded;
            let changes = if loaded == header.message.length {
                presentation::file_changes(&header.metadata, &content)
            } else {
                Vec::new()
            };
            messages.push(CliSessionMessage {
                id: header.message.public_id.clone(),
                role: header.message.role,
                content,
                timestamp: header.metadata.timestamp.clone(),
                model: header.metadata.model.clone(),
                tool_name: header.metadata.tool_name.clone(),
                metadata: header.metadata.clone(),
                content_version: header.message.content_version,
                content_length: header.message.length,
                content_offset,
                has_more_content: content_offset + loaded < header.message.length,
                changes,
            });
        }
        let end = start + messages.len();
        let mut previous_start = start;
        let mut previous_chars = 0;
        while previous_start > 0 && start - previous_start < PAGE_MESSAGES {
            let length = headers[previous_start - 1]
                .message
                .length
                .min(PREVIEW_CHARS);
            if previous_chars + length > PAGE_CHARS {
                break;
            }
            previous_chars += length;
            previous_start -= 1;
        }
        let mut usage = self.usage.clone();
        // A window only sees its own usage events, so the totals are a
        // confirmed lower bound rather than the whole session.
        usage.partial |= indexing || self.truncated || self.window.is_some();
        let previous_cursor = if start > 0 {
            Some(format!("{}:{previous_start}:{start}", self.revision))
        } else {
            window
                .filter(|value| value.start > 0)
                .map(|value| format!("{}:earlier:{}", self.revision, value.start))
        };
        let next_cursor = if end < headers.len() || indexing {
            Some(format!("{}:{}", self.revision, end))
        } else {
            window
                .filter(|value| value.end < value.file_bytes)
                .map(|value| format!("{}:later:{}", self.revision, value.end))
        };
        Ok(CliSessionDetail {
            usage,
            session: summary,
            messages,
            page: SessionReadPage {
                revision: self.revision.clone(),
                previous_cursor,
                next_cursor,
                total_messages: headers.len(),
                start,
                indexing,
                truncated: self.truncated,
                window: window.map(session_read_window),
            },
            outline,
            find: SessionFindState {
                query: request.query.trim().into(),
                total: matching.len(),
                position,
                active_message_id: focus.and_then(|index| {
                    headers
                        .get(index)
                        .map(|header| header.message.public_id.clone())
                }),
            },
        })
    }

    pub(crate) fn read_content(
        &self,
        request: &AgentSessionMessageRequest,
        budget: &SessionReadBudget,
    ) -> Result<AgentSessionMessageContent, String> {
        budget.check()?;
        if self.revision != request.revision {
            return Err("会话内容已更新，请刷新后展开".into());
        }
        let header = self
            .messages
            .iter()
            .find(|message| message.public_id == request.message_id)
            .ok_or("该消息已不存在，请刷新会话")?;
        if header.content_version != request.content_version {
            return Err("消息内容已更新，请刷新后展开".into());
        }
        if request.offset > header.length {
            return Err("消息读取位置无效".into());
        }
        let content = slice_chars(&header.content, request.offset, CONTENT_PAGE_CHARS);
        let end = request.offset + content.chars().count();
        let changes = if end == header.length
            && matches!(
                header.metadata.tool_kind,
                Some(SessionToolKind::Edit | SessionToolKind::Write)
            ) {
            presentation::file_changes(&header.metadata, &header.content)
        } else {
            Vec::new()
        };
        Ok(AgentSessionMessageContent {
            message_id: header.public_id.clone(),
            content,
            offset: request.offset,
            total_chars: header.length,
            next_offset: (end < header.length).then_some(end),
            changes,
        })
    }
}

/// Rebuilds the window a message-expansion request belongs to. A plain
/// revision belongs to a full or multi-source transcript, whose reader reads
/// the source normally; a windowed revision carries its byte range after `@`.
pub(crate) fn window_request_for_revision(revision: &str) -> Result<stream::WindowRequest, String> {
    let Some((_, span)) = revision.split_once('@') else {
        return Ok(stream::WindowRequest::Default);
    };
    let (start, end) = span.split_once('-').ok_or("会话分页位置无效")?;
    let start = start
        .parse::<u64>()
        .map_err(|_| "会话分页位置无效".to_string())?;
    let end = end
        .parse::<u64>()
        .map_err(|_| "会话分页位置无效".to_string())?;
    Ok(stream::WindowRequest::Range { start, end })
}

/// Reads the window request a page cursor asks for before the transcript is
/// loaded. `earlier` and `later` cross into the adjacent window; any other
/// cursor stays in the window named by its revision.
pub(crate) fn window_request_for_cursor(
    cursor: Option<&str>,
) -> Result<stream::WindowRequest, String> {
    let Some(cursor) = cursor else {
        return Ok(stream::WindowRequest::Latest);
    };
    let mut parts = cursor.split(':');
    let revision = parts.next().ok_or("会话分页位置无效")?;
    match parts.next() {
        Some("earlier") => {
            let end = parts
                .next()
                .ok_or("会话分页位置无效")?
                .parse::<u64>()
                .map_err(|_| "会话分页位置无效".to_string())?;
            if parts.next().is_some() {
                return Err("会话分页位置无效".into());
            }
            Ok(stream::WindowRequest::Before(end))
        }
        Some("later") => {
            let start = parts
                .next()
                .ok_or("会话分页位置无效")?
                .parse::<u64>()
                .map_err(|_| "会话分页位置无效".to_string())?;
            if parts.next().is_some() {
                return Err("会话分页位置无效".into());
            }
            Ok(stream::WindowRequest::After(start))
        }
        Some("latest") => {
            if parts.next().is_some() {
                return Err("会话分页位置无效".into());
            }
            Ok(stream::WindowRequest::Latest)
        }
        Some("earliest") => {
            if parts.next().is_some() {
                return Err("会话分页位置无效".into());
            }
            Ok(stream::WindowRequest::First)
        }
        Some(_) | None => window_request_for_revision(revision),
    }
}

/// A cursor that stays on the loaded transcript names its exact revision, so
/// the reader can reuse the cached decode. That is a bare revision (search,
/// match cycling, outline and refresh) or `revision:start[:stop]` page bounds.
/// `earlier`, `later` and `latest` deliberately name another window.
pub(crate) fn same_transcript_cursor_revision(cursor: Option<&str>) -> Option<&str> {
    let mut parts = cursor?.split(':');
    let revision = parts.next()?;
    match parts.next() {
        None => Some(revision),
        Some(value) if value.parse::<usize>().is_ok() => Some(revision),
        Some(_) => None,
    }
}

/// The start of the last page in a window: at most `PAGE_MESSAGES` messages
/// and `PAGE_CHARS` preview characters, always including the final message.
fn last_page_start(headers: &[Header<'_>]) -> usize {
    let mut start = headers.len();
    let mut chars = 0usize;
    while start > 0 && headers.len() - start < PAGE_MESSAGES {
        let length = headers[start - 1].message.length.min(PREVIEW_CHARS);
        if chars > 0 && chars.saturating_add(length) > PAGE_CHARS {
            break;
        }
        chars = chars.saturating_add(length);
        start -= 1;
    }
    start
}

struct Header<'a> {
    message: &'a Message,
    metadata: SessionMessageMetadata,
}

fn read_source(
    path: &str,
    source: &SessionTranscriptSource,
    allow_window: bool,
    request: stream::WindowRequest,
    budget: &SessionReadBudget,
) -> Result<SourceScan, String> {
    let file_bytes = std::fs::metadata(&source.path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let windowable = allow_window
        && source.window_safe
        && source.format == SessionTranscriptFormat::JsonLines
        && !super::source::compressed(&source.path);
    let window_request = match request {
        stream::WindowRequest::Latest if windowable && file_bytes > WINDOWED_TRANSCRIPT_BYTES => {
            Some(request)
        }
        stream::WindowRequest::First
        | stream::WindowRequest::Before(_)
        | stream::WindowRequest::After(_)
        | stream::WindowRequest::Range { .. } => {
            if !windowable {
                return Err("该会话格式不支持分段读取".into());
            }
            Some(request)
        }
        _ => None,
    };
    if let Some(request) = window_request {
        let window = stream::read_window(source, request, TRANSCRIPT_WINDOW_BYTES, &|| {
            budget.check().is_ok()
        })?;
        return Ok(window_scan(path, window));
    }
    if file_bytes > MAX_TRANSCRIPT_BYTES {
        if source.format != SessionTranscriptFormat::JsonLines
            || super::source::compressed(&source.path)
        {
            return Err("会话正文超过按需读取上限，无法打开".into());
        }
        let tail = stream::read_window(
            source,
            stream::WindowRequest::Default,
            TRANSCRIPT_WINDOW_BYTES,
            &|| budget.check().is_ok(),
        )?;
        let mut state = SourceState::default();
        for (order, mutation) in tail.mutations {
            state.apply(path, order, mutation);
        }
        return Ok(SourceScan {
            state,
            complete: false,
            truncated: true,
            skipped_records: tail.skipped_records,
            window: None,
            content_hash: None,
        });
    }
    let mut state = SourceState::default();
    let mut previous: Option<stream::Checkpoint> = None;
    let mut passes = 0u32;
    loop {
        if budget.check().is_err() {
            break;
        }
        passes = passes.saturating_add(1);
        if passes > 1_000_000 {
            return Err("会话记录数量异常，已停止读取".into());
        }
        let batch = match super::workbench::with_read_budget(budget, || {
            stream::read_batch(source, previous.as_ref(), &|| budget.check().is_ok())
        }) {
            Ok(batch) => batch,
            Err(error) => {
                if budget.check().is_err() {
                    break;
                }
                return Err(error);
            }
        };
        if batch.reset {
            state.reset();
        }
        let mut stopped = false;
        for (order, mutation) in batch.mutations {
            state.apply(path, order, mutation);
            if budget.check().is_err() {
                stopped = true;
                break;
            }
        }
        let checkpoint = batch.checkpoint;
        if checkpoint.complete || stopped {
            return Ok(SourceScan {
                state,
                complete: checkpoint.complete,
                truncated: false,
                skipped_records: checkpoint.skipped_records,
                window: None,
                content_hash: None,
            });
        }
        if previous.as_ref() == Some(&checkpoint) {
            return Err("会话文件没有可继续读取的新记录".into());
        }
        previous = Some(checkpoint);
    }
    Ok(SourceScan {
        state,
        complete: false,
        truncated: false,
        skipped_records: previous
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.skipped_records),
        window: None,
        content_hash: None,
    })
}

fn window_scan(path: &str, window: stream::WindowBatch) -> SourceScan {
    let mut state = SourceState::default();
    for (order, mutation) in window.mutations {
        state.apply(path, order, mutation);
    }
    let complete = window.start == 0 && window.end >= window.file_bytes;
    SourceScan {
        state,
        complete,
        // A bounded window can walk to adjacent windows; it is not the
        // one-way truncated tail kept for formats that cannot be split.
        truncated: false,
        skipped_records: window.skipped_records,
        window: Some(SourceWindow {
            start: window.start,
            end: window.end,
            file_bytes: window.file_bytes,
        }),
        content_hash: Some(window.content_hash),
    }
}

struct SourceScan {
    state: SourceState,
    complete: bool,
    truncated: bool,
    skipped_records: u64,
    window: Option<SourceWindow>,
    /// Digest of the range actually covered by a byte window. A full-file
    /// decode uses its file signature for the revision instead.
    content_hash: Option<String>,
}

fn visible_messages(states: Vec<SourceState>) -> (Vec<Message>, SessionUsageSummary) {
    let mut messages = Vec::new();
    let mut usage_rows = Vec::new();
    for state in states {
        messages.extend(state.messages.into_values());
        usage_rows.extend(state.usage.into_values());
    }
    messages.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.order.cmp(&right.order))
            .then_with(|| left.native_id.cmp(&right.native_id))
    });
    let max_negative = messages
        .iter()
        .filter(|message| message.priority < 0)
        .map(|message| message.priority)
        .max();
    let max_visible = messages
        .iter()
        .filter(|message| message.priority >= 0)
        .map(|message| message.priority)
        .max();
    let mut visible: Vec<Message> = messages
        .into_iter()
        .filter(|message| {
            if message.priority < 0 {
                Some(message.priority) == max_negative
            } else {
                Some(message.priority) == max_visible
            }
        })
        .collect();
    enrich_tools(&mut visible);
    (visible, summarize_usage(usage_rows))
}

fn summarize_usage(mut rows: Vec<SessionUsageRecord>) -> SessionUsageSummary {
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    let mut unique = BTreeMap::<String, SessionUsageRecord>::new();
    for mut record in rows {
        let id = crate::services::agent_cli::cache::key(record.id.as_bytes());
        if let Some(previous) = unique.get(&id) {
            let decreased = record.tokens.retain_maximum(&previous.tokens);
            record.uncertain |= previous.uncertain || decreased;
            if previous.model != record.model {
                record.model = None;
            }
        }
        unique.insert(id, record);
    }
    let mut summary = SessionUsageSummary::default();
    let mut models = BTreeMap::<Option<String>, SessionModelUsage>::new();
    for record in unique.values() {
        summary.records += 1;
        summary.partial |= record.uncertain
            || record.tokens.input_tokens.is_none()
            || record.tokens.output_tokens.is_none();
        summary.native_cumulative |= record.cumulative;
        summary.tokens.add(&record.tokens, summary.records == 1);
        let model = models
            .entry(record.model.clone())
            .or_insert_with(|| SessionModelUsage {
                model: record.model.clone(),
                ..Default::default()
            });
        model.tokens.add(&record.tokens, model.records == 0);
        model.records += 1;
    }
    summary.models = models.into_values().collect();
    summary
}

fn enrich_tools(headers: &mut [Message]) {
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

fn content_result(
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

fn slice_chars(content: &str, offset: usize, limit: usize) -> String {
    let Some(start) = content.char_indices().nth(offset).map(|(index, _)| index) else {
        return String::new();
    };
    let end = content[start..]
        .char_indices()
        .nth(limit)
        .map_or(content.len(), |(index, _)| start + index);
    content[start..end].to_string()
}

fn match_offset(content: &str, term: &str) -> Option<usize> {
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

#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use std::{sync::Arc, time::Duration};

    fn budget(current: &dyn Fn() -> bool) -> SessionReadBudget {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if !current() {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
        }
        SessionReadBudget::new(
            std::time::Instant::now() + Duration::from_secs(120),
            cancelled,
            u64::MAX,
        )
    }

    pub(crate) fn read_indexed_messages(
        sources: &[SessionTranscriptSource],
    ) -> Vec<SessionTranscriptMessage> {
        let transcript =
            Transcript::from_sources("fixture", "fixture", sources, &budget(&|| true)).unwrap();
        transcript
            .messages
            .into_iter()
            .map(|message| SessionTranscriptMessage {
                id: message.native_id,
                role: message.role,
                content: message.content,
                metadata: message.metadata,
            })
            .collect()
    }

    pub(crate) fn search_sources(
        sources: &[SessionTranscriptSource],
        request: &SessionContentSearchRequest,
        current: &dyn Fn() -> bool,
    ) -> Result<SessionContentSearchResult, String> {
        let transcript = Transcript::from_sources("fixture", "fixture", sources, &budget(current))?;
        if !current() {
            return Err("会话读取已取消".into());
        }
        Ok(transcript.search(request))
    }

    pub(crate) fn read_history_detail(
        adapter: &SessionHistoryAdapter,
        record: &SessionHistoryRecord,
    ) -> Result<CliSessionDetail, String> {
        let sources = (adapter.transcript)(record)?;
        let transcript =
            Transcript::from_sources("fixture", "fixture", &sources, &budget(&|| true))?;
        let messages = transcript
            .messages
            .iter()
            .map(|message| {
                let changes = presentation::file_changes(&message.metadata, &message.content);
                CliSessionMessage {
                    id: message.public_id.clone(),
                    role: message.role,
                    content_length: message.length,
                    content: message.content.clone(),
                    timestamp: message.metadata.timestamp.clone(),
                    model: message.metadata.model.clone(),
                    tool_name: message.metadata.tool_name.clone(),
                    metadata: message.metadata.clone(),
                    changes,
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        Ok(CliSessionDetail {
            usage: Default::default(),
            session: record.summary.clone(),
            page: SessionReadPage {
                total_messages: messages.len(),
                ..Default::default()
            },
            messages,
            outline: Vec::new(),
            find: Default::default(),
        })
    }

    pub(crate) fn search_history(
        adapter: &SessionHistoryAdapter,
        record: &SessionHistoryRecord,
        request: &SessionContentSearchRequest,
        current: &dyn Fn() -> bool,
    ) -> Result<SessionContentSearchResult, String> {
        search_sources(&(adapter.transcript)(record)?, request, current)
    }
}

#[cfg(test)]
mod tests;
