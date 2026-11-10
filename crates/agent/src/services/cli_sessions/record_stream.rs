//! Bounded native-record reads with durable byte checkpoints. A batch limit
//! yields work; it never disqualifies a normally growing transcript.
use crate::services::agent_cli::contracts::{
    SessionTranscriptFormat, SessionTranscriptMutation, SessionTranscriptSource,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::Metadata,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    time::{Duration, Instant, UNIX_EPOCH},
};
mod compressed;
mod sqlite;

const BATCH_BYTES: u64 = 8 * 1024 * 1024;
const BATCH_TIME: Duration = Duration::from_millis(120);
const RECORD_BYTES: usize = 32 * 1024 * 1024;
const ANCHOR_BYTES: u64 = 4096;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    identity: String,
    fingerprint: String,
    pub source_bytes: u64,
    pub offset: u64,
    sequence: u64,
    mutation_order: u64,
    head_bytes: u64,
    head_hash: String,
    boundary_hash: String,
    pub complete: bool,
    pub skipped_records: u64,
    discarding_line: bool,
    json_stage: JsonStage,
    json_skip: Option<JsonValueState>,
    decoder: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
enum JsonStage {
    #[default]
    Start,
    Key,
    Messages,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct JsonValueState {
    depth: usize,
    quoted: bool,
    escaped: bool,
    scalar: bool,
}

pub(super) struct Batch {
    pub checkpoint: Checkpoint,
    pub reset: bool,
    pub mutations: Vec<(u64, SessionTranscriptMutation)>,
}

fn identity(metadata: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        format!("{:?}", metadata.created().ok())
    }
}

fn fingerprint(metadata: &Metadata, version: u32) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    format!("{version}:{}:{modified}", metadata.len())
}

pub(super) fn source_signature(source: &SessionTranscriptSource) -> Result<String, String> {
    if let Some(query) = &source.sqlite {
        return Ok(format!(
            "{}:{}:{}",
            source.path.to_string_lossy(),
            query.scope,
            sqlite::fingerprint(&source.path)?
        ));
    }
    let file =
        super::workbench::open_session_file(&source.path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    Ok(format!(
        "{}:{}:{}",
        source.path.to_string_lossy(),
        identity(&metadata),
        fingerprint(&metadata, source.parser_version)
    ))
}

fn digest(file: &mut (impl Read + Seek), start: u64, length: u64) -> Result<String, String> {
    file.seek(SeekFrom::Start(start))
        .map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut remaining = length;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let chunk = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..chunk])
            .map_err(|_| "会话文件在读取期间发生变化，请重试".to_string())?;
        hasher.update(&buffer[..chunk]);
        remaining -= chunk as u64;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn read_batch(
    source: &SessionTranscriptSource,
    previous: Option<&Checkpoint>,
    current: &dyn Fn() -> bool,
) -> Result<Batch, String> {
    read_batch_with_limit(source, previous, current, BATCH_BYTES)
}

pub(super) fn read_batch_with_limit(
    source: &SessionTranscriptSource,
    previous: Option<&Checkpoint>,
    current: &dyn Fn() -> bool,
    batch_bytes: u64,
) -> Result<Batch, String> {
    if super::source::compressed(&source.path) {
        return compressed::read_batch(source, previous, current, batch_bytes);
    }
    if source.sqlite.is_some() {
        return sqlite::read_batch(source, previous, current, batch_bytes);
    }
    if source.format == SessionTranscriptFormat::Document {
        return read_json_value(source, previous, current);
    }
    if !current() {
        return Err("会话读取已取消".into());
    }
    let mut file = super::workbench::open_session_file(&source.path)
        .map_err(|error| format!("读取会话正文失败：{error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let file_identity = identity(&metadata);
    let file_fingerprint = fingerprint(&metadata, source.parser_version);
    let same = previous.is_some_and(|checkpoint| {
        checkpoint.identity == file_identity && checkpoint.fingerprint == file_fingerprint
    });
    if same && previous.is_some_and(|checkpoint| checkpoint.complete) {
        return Ok(Batch {
            checkpoint: previous.unwrap().clone(),
            reset: false,
            mutations: Vec::new(),
        });
    }
    let mut reusable = same;
    if !same && source.format == SessionTranscriptFormat::Lines {
        if let Some(checkpoint) = previous.filter(|checkpoint| {
            checkpoint.identity == file_identity
                && metadata.len() > checkpoint.source_bytes
                && checkpoint.offset <= checkpoint.source_bytes
                && checkpoint
                    .fingerprint
                    .starts_with(&format!("{}:", source.parser_version))
        }) {
            let boundary_start = checkpoint.offset.saturating_sub(ANCHOR_BYTES);
            reusable = digest(&mut file, 0, checkpoint.head_bytes)? == checkpoint.head_hash
                && digest(
                    &mut file,
                    boundary_start,
                    checkpoint.offset - boundary_start,
                )? == checkpoint.boundary_hash;
        }
    }
    let mut checkpoint = if reusable {
        previous.cloned().unwrap_or_default()
    } else {
        Checkpoint::default()
    };
    checkpoint.identity = file_identity;
    checkpoint.fingerprint = file_fingerprint;
    checkpoint.source_bytes = metadata.len();
    checkpoint.complete = false;
    if !reusable {
        checkpoint.head_bytes = ANCHOR_BYTES.min(metadata.len());
        checkpoint.head_hash = digest(&mut file, 0, checkpoint.head_bytes)?;
    }
    file.seek(SeekFrom::Start(checkpoint.offset))
        .map_err(|error| error.to_string())?;
    let mut reader =
        BufReader::with_capacity(16 * 1024, file.take(metadata.len() - checkpoint.offset));
    let started = Instant::now();
    let start_offset = checkpoint.offset;
    let mut mutations = Vec::new();
    let mut first = true;
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        if !first
            && (checkpoint.offset.saturating_sub(start_offset) >= batch_bytes
                || started.elapsed() >= BATCH_TIME)
        {
            break;
        }
        first = false;
        let value = match source.format {
            SessionTranscriptFormat::Lines => next_line(&mut reader, &mut checkpoint)?,
            SessionTranscriptFormat::Messages => next_json_message(&mut reader, &mut checkpoint)?,
            SessionTranscriptFormat::Document => return Err("会话格式不支持流式读取".into()),
        };
        match value {
            Record::Value(bytes) => {
                for mutation in (source.decode)(
                    &source.path,
                    checkpoint.sequence,
                    &bytes,
                    &mut checkpoint.decoder,
                ) {
                    checkpoint.mutation_order = checkpoint.mutation_order.saturating_add(1);
                    mutations.push((checkpoint.mutation_order, mutation));
                }
                checkpoint.sequence = checkpoint.sequence.saturating_add(1);
            }
            Record::Continue => {}
            Record::End => {
                checkpoint.complete = true;
                break;
            }
        }
    }
    let mut file = reader.into_inner().into_inner();
    let after = file.metadata().map_err(|error| error.to_string())?;
    // Concurrent appends are safe: this batch read a fixed prefix. Rewrites
    // must never commit events decoded from two different file generations.
    if identity(&after) != checkpoint.identity
        || after.len() < metadata.len()
        || (fingerprint(&after, source.parser_version) != checkpoint.fingerprint
            && (source.format != SessionTranscriptFormat::Lines || after.len() == metadata.len()))
    {
        return Err("会话文件在读取期间发生变化，请重试".into());
    }
    let boundary_start = checkpoint.offset.saturating_sub(ANCHOR_BYTES);
    checkpoint.boundary_hash = digest(
        &mut file,
        boundary_start,
        checkpoint.offset - boundary_start,
    )?;
    Ok(Batch {
        checkpoint,
        reset: !reusable,
        mutations,
    })
}

/// One bounded JSON document, decoded once. Used by agents that store each
/// message as its own file; the document itself is the record.
fn read_json_value(
    source: &SessionTranscriptSource,
    previous: Option<&Checkpoint>,
    current: &dyn Fn() -> bool,
) -> Result<Batch, String> {
    if !current() {
        return Err("会话读取已取消".into());
    }
    let mut file = super::workbench::open_session_file(&source.path)
        .map_err(|error| format!("读取会话正文失败：{error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if metadata.len() > RECORD_BYTES as u64 {
        return Err("单条会话记录超过读取上限".into());
    }
    let file_identity = identity(&metadata);
    let file_fingerprint = fingerprint(&metadata, source.parser_version);
    let same = previous.is_some_and(|checkpoint| {
        checkpoint.identity == file_identity && checkpoint.fingerprint == file_fingerprint
    });
    if same && previous.is_some_and(|checkpoint| checkpoint.complete) {
        return Ok(Batch {
            checkpoint: previous.unwrap().clone(),
            reset: false,
            mutations: Vec::new(),
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let after = file.metadata().map_err(|error| error.to_string())?;
    if identity(&after) != file_identity
        || fingerprint(&after, source.parser_version) != file_fingerprint
    {
        return Err("会话文件在读取期间发生变化，请重试".into());
    }
    let mut checkpoint = if same {
        previous.cloned().unwrap_or_default()
    } else {
        Checkpoint::default()
    };
    checkpoint.identity = file_identity;
    checkpoint.fingerprint = file_fingerprint;
    checkpoint.source_bytes = metadata.len();
    checkpoint.offset = metadata.len();
    checkpoint.complete = true;
    let mut mutations = Vec::new();
    for mutation in (source.decode)(
        &source.path,
        checkpoint.sequence,
        &bytes,
        &mut checkpoint.decoder,
    ) {
        checkpoint.mutation_order = checkpoint.mutation_order.saturating_add(1);
        mutations.push((checkpoint.mutation_order, mutation));
    }
    checkpoint.sequence = checkpoint.sequence.saturating_add(1);
    Ok(Batch {
        checkpoint,
        reset: !same,
        mutations,
    })
}

/// Which bounded part of a JSONL transcript to decode. `Default` keeps the
/// search path's full read (up to the hard cap) and `Latest` gives the reader
/// a window below it; `Before`, `After` and `Range` navigate between reader
/// windows without merging their decoded state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowRequest {
    Default,
    Latest,
    First,
    Before(u64),
    After(u64),
    Range { start: u64, end: u64 },
}

impl WindowRequest {
    pub(super) fn cache_key(self) -> String {
        match self {
            Self::Default => "default".into(),
            Self::Latest => "latest".into(),
            Self::First => "first".into(),
            Self::Before(offset) => format!("before:{offset}"),
            Self::After(offset) => format!("after:{offset}"),
            Self::Range { start, end } => format!("range:{start}:{end}"),
        }
    }
}

/// One decoded byte window. Its `start` and `end` are the offsets actually
/// covered after aligning the start to the next whole record. `content_hash`
/// covers that actual range, so reopening the same range yields the same
/// identity even when the file only grows past it.
pub(super) struct WindowBatch {
    pub mutations: Vec<(u64, SessionTranscriptMutation)>,
    pub skipped_records: u64,
    pub start: u64,
    pub end: u64,
    pub file_bytes: u64,
    pub content_hash: String,
}

/// Reads one bounded window of a JSONL transcript. Compressed and non-JSONL
/// sources cannot be seeked this way and keep their explicit "too large" error.
pub(super) fn read_window(
    source: &SessionTranscriptSource,
    request: WindowRequest,
    window_bytes: u64,
    current: &dyn Fn() -> bool,
) -> Result<WindowBatch, String> {
    if source.format != SessionTranscriptFormat::Lines || super::source::compressed(&source.path) {
        return Err("该会话格式不支持分段读取".into());
    }
    if !current() {
        return Err("会话读取已取消".into());
    }
    let mut file = super::workbench::open_session_file(&source.path)
        .map_err(|error| format!("读取会话正文失败：{error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let file_bytes = metadata.len();
    let window_bytes = window_bytes.max(1);
    let (requested_start, requested_end) = match request {
        WindowRequest::Default | WindowRequest::Latest => {
            (file_bytes.saturating_sub(window_bytes), file_bytes)
        }
        WindowRequest::First => (0, file_bytes.min(window_bytes)),
        WindowRequest::Before(offset) => {
            let end = offset.min(file_bytes);
            (end.saturating_sub(window_bytes), end)
        }
        WindowRequest::After(offset) => {
            let start = offset.min(file_bytes);
            (start, file_bytes.min(start.saturating_add(window_bytes)))
        }
        WindowRequest::Range { start, end } => {
            let start = start.min(file_bytes);
            (start, end.min(file_bytes).max(start))
        }
    };
    if requested_start >= requested_end {
        return Ok(WindowBatch {
            mutations: Vec::new(),
            skipped_records: 0,
            start: requested_start,
            end: requested_start,
            file_bytes,
            content_hash: digest(&mut file, requested_start, 0)?,
        });
    }
    let mut skip_partial_line = false;
    if requested_start > 0 {
        file.seek(SeekFrom::Start(requested_start - 1))
            .map_err(|error| error.to_string())?;
        let mut previous = [0u8; 1];
        file.read_exact(&mut previous)
            .map_err(|error| error.to_string())?;
        skip_partial_line = previous[0] != b'\n';
    }
    file.seek(SeekFrom::Start(requested_start))
        .map_err(|error| error.to_string())?;
    let mut reader =
        BufReader::with_capacity(16 * 1024, file.take(requested_end - requested_start));
    let mut checkpoint = Checkpoint {
        identity: identity(&metadata),
        fingerprint: fingerprint(&metadata, source.parser_version),
        source_bytes: file_bytes,
        offset: requested_start,
        ..Checkpoint::default()
    };
    if skip_partial_line {
        checkpoint.offset += discard_partial_line(&mut reader, current)?;
    }
    let read_start = checkpoint.offset;
    let mut mutations = Vec::new();
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        match next_line(&mut reader, &mut checkpoint)? {
            Record::Value(bytes) => {
                for mutation in (source.decode)(
                    &source.path,
                    checkpoint.sequence,
                    &bytes,
                    &mut checkpoint.decoder,
                ) {
                    checkpoint.mutation_order = checkpoint.mutation_order.saturating_add(1);
                    mutations.push((checkpoint.mutation_order, mutation));
                }
                checkpoint.sequence = checkpoint.sequence.saturating_add(1);
            }
            Record::Continue => {}
            Record::End => break,
        }
    }
    // Appends are safe because the window is fixed; a rewrite must not mix two
    // file generations into one view. A window that starts inside one oversized
    // record yields no whole record: keep the requested boundary so the page
    // can still walk further back instead of asking for the same window again.
    let start = if read_start >= requested_end {
        requested_start
    } else {
        read_start
    };
    let end = checkpoint.offset.min(requested_end);
    // Hash the range actually covered after record alignment. Reopening the
    // same range (for example when a search hit jumps to this window) then
    // reproduces the same revision even though `end` may be short of the
    // requested boundary by one record.
    let content_hash = {
        let mut hash_file =
            super::workbench::open_session_file(&source.path).map_err(|error| error.to_string())?;
        digest(&mut hash_file, start, end.saturating_sub(start))?
    };
    let after = std::fs::metadata(&source.path).map_err(|error| error.to_string())?;
    if identity(&after) != checkpoint.identity || after.len() < requested_end {
        return Err("会话文件在读取期间发生变化，请重试".into());
    }
    Ok(WindowBatch {
        mutations,
        skipped_records: checkpoint.skipped_records,
        start,
        end,
        file_bytes,
        content_hash,
    })
}

fn discard_partial_line(
    reader: &mut impl BufRead,
    current: &dyn Fn() -> bool,
) -> Result<u64, String> {
    let mut consumed = 0u64;
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        let buffer = reader.fill_buf().map_err(|error| error.to_string())?;
        if buffer.is_empty() {
            return Ok(consumed);
        }
        match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                reader.consume(index + 1);
                return Ok(consumed + (index + 1) as u64);
            }
            None => {
                let count = buffer.len();
                reader.consume(count);
                consumed += count as u64;
            }
        }
    }
}

enum Record {
    Value(Vec<u8>),
    Continue,
    End,
}

fn next_line(reader: &mut impl BufRead, checkpoint: &mut Checkpoint) -> Result<Record, String> {
    if checkpoint.discarding_line {
        let available = reader.fill_buf().map_err(|error| error.to_string())?;
        if available.is_empty() {
            return Ok(Record::End);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        reader.consume(count);
        checkpoint.offset += count as u64;
        if newline.is_some() {
            checkpoint.discarding_line = false;
            checkpoint.sequence += 1;
        }
        return Ok(Record::Continue);
    }
    let mut bytes = Vec::new();
    let count = reader
        .by_ref()
        .take((RECORD_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Ok(Record::End);
    }
    if bytes.len() > RECORD_BYTES {
        checkpoint.offset += count as u64;
        checkpoint.skipped_records += 1;
        checkpoint.discarding_line = !bytes.ends_with(b"\n");
        if !checkpoint.discarding_line {
            checkpoint.sequence += 1;
        }
        return Ok(Record::Continue);
    }
    if !bytes.ends_with(b"\n") && serde_json::from_slice::<serde::de::IgnoredAny>(&bytes).is_err() {
        // The CLI is still writing its last record. Retry from this record's
        // start after the file grows, without repeatedly polling unchanged data.
        return Ok(Record::End);
    }
    checkpoint.offset += count as u64;
    Ok(Record::Value(bytes))
}

fn peek(reader: &mut impl BufRead) -> Result<Option<u8>, String> {
    Ok(reader
        .fill_buf()
        .map_err(|error| error.to_string())?
        .first()
        .copied())
}

fn consume(reader: &mut impl BufRead, checkpoint: &mut Checkpoint, bytes: usize) {
    reader.consume(bytes);
    checkpoint.offset += bytes as u64;
}

fn whitespace(reader: &mut impl BufRead, checkpoint: &mut Checkpoint) -> Result<(), String> {
    loop {
        let bytes = reader.fill_buf().map_err(|error| error.to_string())?;
        let count = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        if count == 0 {
            return Ok(());
        }
        consume(reader, checkpoint, count);
    }
}

fn next_json_message(
    reader: &mut impl BufRead,
    checkpoint: &mut Checkpoint,
) -> Result<Record, String> {
    if let Some(mut skip) = checkpoint.json_skip.take() {
        let (count, finished) = consume_json_value(reader, &mut skip, None)?;
        checkpoint.offset += count as u64;
        if !finished {
            checkpoint.json_skip = Some(skip);
        }
        return Ok(Record::Continue);
    }
    whitespace(reader, checkpoint)?;
    let Some(byte) = peek(reader)? else {
        return if checkpoint.json_stage == JsonStage::Done {
            Ok(Record::End)
        } else {
            Err("JSON 会话尚未写入完整，请稍后重试".into())
        };
    };
    match checkpoint.json_stage {
        JsonStage::Start => {
            if byte != b'{' {
                return Err("JSON 会话应为包含 messages 的对象".into());
            }
            consume(reader, checkpoint, 1);
            checkpoint.json_stage = JsonStage::Key;
            Ok(Record::Continue)
        }
        JsonStage::Key => {
            if byte == b',' {
                consume(reader, checkpoint, 1);
                return Ok(Record::Continue);
            }
            if byte == b'}' {
                consume(reader, checkpoint, 1);
                checkpoint.json_stage = JsonStage::Done;
                return Ok(Record::End);
            }
            let key = json_value(reader, checkpoint)?.ok_or("JSON 会话字段名过长")?;
            let key: String = serde_json::from_slice(&key).map_err(|error| error.to_string())?;
            whitespace(reader, checkpoint)?;
            if peek(reader)? != Some(b':') {
                return Err("JSON 会话字段缺少冒号".into());
            }
            consume(reader, checkpoint, 1);
            whitespace(reader, checkpoint)?;
            if key == "messages" && peek(reader)? == Some(b'[') {
                consume(reader, checkpoint, 1);
                checkpoint.json_stage = JsonStage::Messages;
                Ok(Record::Continue)
            } else if let Some(value) = json_value(reader, checkpoint)? {
                let mut record = vec![b'{'];
                record.extend(serde_json::to_vec(&key).map_err(|error| error.to_string())?);
                record.push(b':');
                record.extend(value);
                record.push(b'}');
                Ok(Record::Value(record))
            } else {
                Ok(Record::Continue)
            }
        }
        JsonStage::Messages => {
            if byte == b',' {
                consume(reader, checkpoint, 1);
                return Ok(Record::Continue);
            }
            if byte == b']' {
                consume(reader, checkpoint, 1);
                checkpoint.json_stage = JsonStage::Key;
                return Ok(Record::Continue);
            }
            Ok(json_value(reader, checkpoint)?.map_or(Record::Continue, Record::Value))
        }
        JsonStage::Done => Ok(Record::End),
    }
}

fn json_value(
    reader: &mut impl BufRead,
    checkpoint: &mut Checkpoint,
) -> Result<Option<Vec<u8>>, String> {
    let first = peek(reader)?.ok_or("JSON 会话尚未写入完整")?;
    let mut bytes = vec![first];
    consume(reader, checkpoint, 1);
    let mut state = JsonValueState {
        depth: usize::from(first == b'{' || first == b'['),
        quoted: first == b'"',
        escaped: false,
        scalar: first != b'{' && first != b'[' && first != b'"',
    };
    loop {
        let (count, finished) = consume_json_value(reader, &mut state, Some(&mut bytes))?;
        checkpoint.offset += count as u64;
        if bytes.len() > RECORD_BYTES {
            checkpoint.skipped_records += 1;
            if !finished {
                checkpoint.json_skip = Some(state);
            }
            return Ok(None);
        }
        if finished {
            return Ok(Some(bytes));
        }
    }
}

/// Consumes one buffer at a time, retaining at most one normal-sized record.
/// Oversized values keep only lexical state across batches.
fn consume_json_value(
    reader: &mut impl BufRead,
    state: &mut JsonValueState,
    bytes: Option<&mut Vec<u8>>,
) -> Result<(usize, bool), String> {
    let available = reader.fill_buf().map_err(|error| error.to_string())?;
    if available.is_empty() {
        return if state.scalar {
            Ok((0, true))
        } else {
            Err("JSON 会话尚未写入完整".into())
        };
    }
    let mut count = 0;
    let mut finished = false;
    for byte in available {
        if !state.quoted
            && state.depth == 0
            && state.scalar
            && (byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']'))
        {
            finished = true;
            break;
        }
        count += 1;
        if state.quoted {
            if state.escaped {
                state.escaped = false;
            } else if *byte == b'\\' {
                state.escaped = true;
            } else if *byte == b'"' {
                state.quoted = false;
                if state.depth == 0 {
                    finished = true;
                    break;
                }
            }
        } else {
            match byte {
                b'"' => state.quoted = true,
                b'{' | b'[' => state.depth += 1,
                b'}' | b']' => {
                    state.depth = state.depth.checked_sub(1).ok_or("JSON 会话括号不匹配")?;
                    if state.depth == 0 {
                        finished = true;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(bytes) = bytes {
        bytes.extend_from_slice(&available[..count]);
    }
    reader.consume(count);
    Ok((count, finished))
}

/// Summary/detail windows share the JSON record reader with indexing. Large
/// JSON documents never need to be deserialized as one in-memory Value.
pub(crate) fn read_json_messages_limited(
    path: &std::path::Path,
    max_bytes: usize,
    current: &dyn Fn() -> bool,
    mut observe: impl FnMut(Value),
) -> Result<bool, String> {
    let file = super::workbench::open_session_file(path).map_err(|error| error.to_string())?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    if size == 0 {
        return Ok(false);
    }
    let truncated = size > max_bytes as u64;
    let mut reader = BufReader::new(file.take(max_bytes as u64));
    let mut checkpoint = Checkpoint::default();
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        super::workbench::check_read_budget()?;
        match next_json_message(&mut reader, &mut checkpoint) {
            Ok(Record::Value(bytes)) => {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    observe(value);
                }
            }
            Ok(Record::Continue) => {}
            Ok(Record::End) => return Ok(truncated),
            Err(error) if truncated && error.contains("尚未写入完整") => return Ok(true),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
