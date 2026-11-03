use crate::models::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::BTreeMap;

pub(super) fn put(
    transaction: &Transaction<'_>,
    workspace: &str,
    session: &str,
    source: &str,
    record: &SessionUsageRecord,
) -> Result<(), String> {
    let id = super::super::super::agent_cli::cache::key(record.id.as_bytes());
    let previous: Option<String> = transaction.query_row("SELECT facts FROM session_usage WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND usage_id=?4", params![workspace,session,source,id], |row| row.get(0)).optional().map_err(|error| error.to_string())?;
    let mut record = record.clone();
    if let Some(previous) = previous {
        let previous: SessionUsageRecord =
            serde_json::from_str(&previous).map_err(|error| error.to_string())?;
        let decreased = record.tokens.retain_maximum(&previous.tokens);
        record.uncertain |= previous.uncertain || decreased;
        if previous.model != record.model {
            record.model = None;
        }
    }
    let facts = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    transaction.execute("INSERT INTO session_usage(workspace,session_id,source_key,usage_id,facts,message_id) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(workspace,session_id,source_key,usage_id) DO UPDATE SET facts=excluded.facts,message_id=excluded.message_id", params![workspace,session,source,id,facts,record.message_id]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn remove_message(
    connection: &Connection,
    workspace: &str,
    session: &str,
    source: &str,
    message: &str,
) -> Result<(), String> {
    connection.execute("DELETE FROM session_usage WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_id=?4", params![workspace, session, source, message]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn rewind(
    connection: &Connection,
    workspace: &str,
    session: &str,
    source: &str,
    position: Option<i64>,
) -> Result<(), String> {
    connection.execute("DELETE FROM session_usage WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND (?4 IS NULL OR message_id IN (SELECT message_id FROM message_positions WHERE workspace=?1 AND session_id=?2 AND source_key=?3 AND message_order>=?4))", params![workspace, session, source, position]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn clear(
    connection: &Connection,
    workspace: &str,
    session: &str,
    source: &str,
) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM session_usage WHERE workspace=?1 AND session_id=?2 AND source_key=?3",
            params![workspace, session, source],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    workspace: &str,
    session: &str,
    indexing: bool,
) -> Result<SessionUsageSummary, String> {
    let mut statement = connection.prepare("SELECT usage_id,facts FROM session_usage WHERE workspace=?1 AND session_id=?2 ORDER BY source_key,usage_id").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace, session], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut unique = BTreeMap::<String, SessionUsageRecord>::new();
    for row in rows {
        super::super::workbench::check_read_budget()?;
        let (id, text) = row.map_err(|error| error.to_string())?;
        let mut record: SessionUsageRecord =
            serde_json::from_str(&text).map_err(|error| error.to_string())?;
        if let Some(previous) = unique.get(&id) {
            let decreased = record.tokens.retain_maximum(&previous.tokens);
            record.uncertain |= previous.uncertain || decreased;
            if previous.model != record.model {
                record.model = None;
            }
        }
        unique.insert(id, record);
    }
    let mut summary = SessionUsageSummary {
        partial: indexing,
        ..Default::default()
    };
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
    Ok(summary)
}
