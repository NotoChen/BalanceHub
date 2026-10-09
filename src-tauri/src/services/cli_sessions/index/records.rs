use super::{chunk_message, role_key, stream::Batch};
use crate::{
    services::agent_cli::contracts::{
        SessionHistoryRecord, SessionIndexMessage, SessionIndexMutation,
    },
    util::unix_millis,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(super) fn apply_batch(
    connection: &mut Connection,
    workspace: &str,
    record: &SessionHistoryRecord,
    source: &str,
    expected: Option<&str>,
    batch: &Batch,
) -> Result<bool, String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let actual: Option<String> = transaction.query_row(
        "SELECT checkpoint FROM session_sources WHERE workspace=?1 AND session_id=?2 AND source_key=?3",
        params![workspace, record.record_key, source], |row| row.get(0),
    ).optional().map_err(|error| error.to_string())?;
    // Two independent searches can scan the same prefix. Only the reader of
    // the current checkpoint may commit, so chunks can never be appended twice.
    if actual.as_deref() != expected {
        return Ok(false);
    }
    store_summary(&transaction, workspace, record)?;
    let key = MessageKey {
        workspace,
        session: &record.record_key,
        source,
    };
    let mut delta = 0;
    if batch.reset {
        delta -= truncate(&transaction, &key, None)?;
    }
    for (order, mutation) in &batch.mutations {
        super::super::workbench::check_read_budget()?;
        delta += match mutation {
            SessionIndexMutation::Put { message, priority } => {
                put(&transaction, &key, message, *priority, *order, false)?
            }
            SessionIndexMutation::Append { message, priority } => {
                put(&transaction, &key, message, *priority, *order, true)?
            }
            SessionIndexMutation::Clear => -truncate(&transaction, &key, None)?,
            SessionIndexMutation::Remove(id) => {
                remember_position(&transaction, &key, id, *order)?;
                -remove(&transaction, &key, Some(id), None)?
            }
            SessionIndexMutation::Rewind(id) => {
                let position: Option<i64> = transaction.query_row(
                    "SELECT MIN(message_order) FROM message_positions WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_id=?4",
                    params![key.workspace, key.session, key.source, id], |row| row.get(0),
                ).map_err(|error| error.to_string())?;
                -truncate(&transaction, &key, position)?
            }
        };
    }
    let checkpoint = serde_json::to_string(&batch.checkpoint).map_err(|error| error.to_string())?;
    if checkpoint.len() > 64 * 1024 {
        return Err("会话索引状态异常".into());
    }
    transaction.execute(
        "INSERT OR REPLACE INTO session_sources(workspace, session_id, source_key, checkpoint) VALUES (?1, ?2, ?3, ?4)",
        params![workspace, record.record_key, source, checkpoint],
    ).map_err(|error| error.to_string())?;
    transaction.execute(
        "UPDATE sessions SET indexed_bytes=MAX(0, indexed_bytes+?3), indexed_at=?4, last_used_at=?5 WHERE workspace=?1 AND session_id=?2",
        params![workspace, record.record_key, delta, chrono::Utc::now().to_rfc3339(), unix_millis() as i64],
    ).map_err(|error| error.to_string())?;
    transaction
        .commit()
        .map_err(|error| format!("保存会话索引进度失败：{error}"))?;
    Ok(true)
}

pub(super) fn reconcile_sources(
    connection: &mut Connection,
    workspace: &str,
    session: &str,
    sources: &[String],
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let known = {
        let mut statement = transaction
            .prepare("SELECT source_key FROM session_sources WHERE workspace=?1 AND session_id=?2")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![workspace, session], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    for source in known.into_iter().filter(|source| !sources.contains(source)) {
        let delta = truncate(
            &transaction,
            &MessageKey {
                workspace,
                session,
                source: &source,
            },
            None,
        )?;
        transaction.execute(
            "DELETE FROM session_sources WHERE workspace=?1 AND session_id=?2 AND source_key=?3",
            params![workspace, session, source],
        ).map_err(|error| error.to_string())?;
        transaction.execute(
            "UPDATE sessions SET indexed_bytes=MAX(0,indexed_bytes-?3) WHERE workspace=?1 AND session_id=?2",
            params![workspace, session, delta],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

struct MessageKey<'a> {
    workspace: &'a str,
    session: &'a str,
    source: &'a str,
}

fn remember_position(
    transaction: &Transaction<'_>,
    key: &MessageKey<'_>,
    id: &str,
    order: u64,
) -> Result<i64, String> {
    // A non-searchable native message can still be a rewind boundary. Keep
    // its position even when its visible text is removed or replaced.
    transaction.query_row(
        "INSERT INTO message_positions(workspace,session_id,source_key,message_id,message_order) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(workspace,session_id,source_key,message_id) DO UPDATE SET message_order=message_positions.message_order
         RETURNING message_order",
        params![key.workspace, key.session, key.source, id, order as i64], |row| row.get(0),
    ).map_err(|error| error.to_string())
}

fn truncate(
    transaction: &Transaction<'_>,
    key: &MessageKey<'_>,
    from_order: Option<i64>,
) -> Result<i64, String> {
    let removed = remove(transaction, key, None, from_order)?;
    transaction.execute(
        "DELETE FROM message_positions WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND (?4 IS NULL OR message_order>=?4)",
        params![key.workspace, key.session, key.source, from_order],
    ).map_err(|error| error.to_string())?;
    Ok(removed)
}

fn remove(
    transaction: &Transaction<'_>,
    key: &MessageKey<'_>,
    id: Option<&str>,
    from_order: Option<i64>,
) -> Result<i64, String> {
    let bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(length(CAST(content AS BLOB))),0) FROM messages WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND (?4 IS NULL OR message_id=?4) AND (?5 IS NULL OR message_order>=?5)",
        params![key.workspace, key.session, key.source, id, from_order], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    transaction.execute(
        "DELETE FROM messages WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND (?4 IS NULL OR message_id=?4) AND (?5 IS NULL OR message_order>=?5)",
        params![key.workspace, key.session, key.source, id, from_order],
    ).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn put(
    transaction: &Transaction<'_>,
    key: &MessageKey<'_>,
    message: &SessionIndexMessage,
    priority: i64,
    order: u64,
    append: bool,
) -> Result<i64, String> {
    let previous: Option<(i64, String)> = transaction.query_row(
        "SELECT chunk_index, content FROM messages WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_id=?4 ORDER BY chunk_index DESC LIMIT 1",
        params![key.workspace, key.session, key.source, message.id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(|error| error.to_string())?;
    let saved_order = remember_position(transaction, key, &message.id, order)?;
    let (first_chunk, content, mut delta) = if let Some((chunk, text)) = previous.filter(|_| append)
    {
        transaction.execute(
            "DELETE FROM messages WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_id=?4 AND chunk_index=?5",
            params![key.workspace, key.session, key.source, message.id, chunk],
        ).map_err(|error| error.to_string())?;
        let removed = text.len() as i64;
        (chunk, text + &message.content, -removed)
    } else {
        (
            0,
            message.content.clone(),
            -remove(transaction, key, Some(&message.id), None)?,
        )
    };
    for (index, content) in chunk_message(&content).into_iter().enumerate() {
        transaction.execute(
            "INSERT INTO messages(workspace, session_id, source_key, message_id, role, chunk_index, content, priority, message_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![key.workspace, key.session, key.source, message.id, role_key(message.role),
                first_chunk + index as i64, content, priority, saved_order],
        ).map_err(|error| error.to_string())?;
        delta += content.len() as i64;
    }
    Ok(delta)
}

fn store_summary(
    transaction: &Transaction<'_>,
    workspace: &str,
    record: &SessionHistoryRecord,
) -> Result<(), String> {
    let summary = &record.summary;
    transaction.execute(
        "INSERT INTO sessions(workspace, session_id, title, preview, model, models_json, workdir, created_at, updated_at, fingerprint, source_bytes, indexed_bytes, indexed_at, last_used_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'incremental',0,0,?10,?11)
         ON CONFLICT(workspace,session_id) DO UPDATE SET title=excluded.title, preview=excluded.preview, model=excluded.model, models_json=excluded.models_json, updated_at=excluded.updated_at, last_used_at=excluded.last_used_at",
        params![workspace, record.record_key, summary.title, summary.preview, summary.model,
            serde_json::to_string(&summary.models).map_err(|error| error.to_string())?,
            summary.workdir, summary.created_at, summary.updated_at, chrono::Utc::now().to_rfc3339(), unix_millis() as i64],
    ).map_err(|error| error.to_string())?;
    Ok(())
}
