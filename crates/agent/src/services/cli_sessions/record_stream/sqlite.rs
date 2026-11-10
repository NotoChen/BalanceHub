//! Bounded SQLite transcript batches.
//!
//! Native Agents that keep their conversation in SQLite describe rows through
//! [`SqliteTranscript`]: an integer row key plus a JSON payload. The shared
//! reader still owns batching, cancellation, budgets, decoder state and
//! checkpoints; only the row query is agent-specific.

use super::{Batch, Checkpoint, BATCH_TIME};
use crate::services::{
    agent_cli::contracts::SessionTranscriptSource, cli_sessions::workbench::check_read_budget,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};

const ROW_LIMIT: i64 = 2_000;
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn read_batch(
    source: &SessionTranscriptSource,
    previous: Option<&Checkpoint>,
    current: &dyn Fn() -> bool,
    batch_bytes: u64,
) -> Result<Batch, String> {
    let Some(query) = &source.sqlite else {
        return Err("会话没有 SQLite 记录来源".into());
    };
    if !current() {
        return Err("会话读取已取消".into());
    }
    let fingerprint = fingerprint(&source.path)?;
    let identity = source.path.to_string_lossy().into_owned();
    let same = previous.is_some_and(|checkpoint| {
        checkpoint.identity == identity && checkpoint.fingerprint == fingerprint
    });
    if same && previous.is_some_and(|checkpoint| checkpoint.complete) {
        return Ok(Batch {
            checkpoint: previous.unwrap().clone(),
            reset: false,
            mutations: Vec::new(),
        });
    }
    let mut checkpoint = if same {
        previous.cloned().unwrap_or_default()
    } else {
        Checkpoint::default()
    };
    checkpoint.identity = identity;
    checkpoint.fingerprint = fingerprint.clone();
    checkpoint.source_bytes = fs::metadata(&source.path)
        .map(|value| value.len())
        .unwrap_or(0);
    checkpoint.complete = false;
    if !same {
        checkpoint.offset = 0;
        checkpoint.sequence = 0;
        checkpoint.mutation_order = 0;
        checkpoint.decoder = Value::Null;
    }
    let connection = Connection::open_with_flags(
        &source.path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("打开会话数据库失败：{error}"))?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(query.query)
        .map_err(|error| format!("读取会话数据库失败：{error}"))?;
    let mut rows = statement
        .query(rusqlite::params![
            i64::try_from(checkpoint.offset).unwrap_or(i64::MAX),
            ROW_LIMIT,
            query.scope
        ])
        .map_err(|error| format!("读取会话数据库失败：{error}"))?;
    let started = Instant::now();
    let mut mutations = Vec::new();
    let mut rows_read = 0usize;
    let mut payload_bytes = 0u64;
    let mut exhausted = true;
    loop {
        if !current() {
            return Err("会话读取已取消".into());
        }
        check_read_budget()?;
        let Some(row) = rows.next().map_err(|error| error.to_string())? else {
            break;
        };
        let key: i64 = row.get(0).map_err(|error| error.to_string())?;
        let payload: String = row.get(1).map_err(|error| error.to_string())?;
        if key < 0 {
            continue;
        }
        let key = key as u64;
        if key <= checkpoint.offset {
            continue;
        }
        for mutation in (source.decode)(
            &source.path,
            checkpoint.sequence,
            payload.as_bytes(),
            &mut checkpoint.decoder,
        ) {
            checkpoint.mutation_order = checkpoint.mutation_order.saturating_add(1);
            mutations.push((checkpoint.mutation_order, mutation));
        }
        checkpoint.sequence = checkpoint.sequence.saturating_add(1);
        checkpoint.offset = key;
        rows_read += 1;
        payload_bytes = payload_bytes.saturating_add(payload.len() as u64);
        if payload_bytes >= batch_bytes || started.elapsed() >= BATCH_TIME {
            exhausted = false;
            break;
        }
    }
    checkpoint.complete = exhausted && rows_read < ROW_LIMIT as usize;
    Ok(Batch {
        checkpoint,
        reset: !same,
        mutations,
    })
}

pub(super) fn fingerprint(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("读取会话数据库失败：{error}"))?;
    let wal = fs::metadata(format!("{}-wal", path.to_string_lossy())).ok();
    Ok(format!(
        "{}:{}:{}:{}",
        modified(&metadata),
        metadata.len(),
        wal.as_ref().map(modified).unwrap_or_default(),
        wal.as_ref().map(fs::Metadata::len).unwrap_or_default()
    ))
}

fn modified(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .unwrap_or_default()
}
