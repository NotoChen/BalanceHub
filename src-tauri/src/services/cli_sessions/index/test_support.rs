use super::{initialize_schema, records, stream};
use crate::{
    models::{AgentCliKind, CliSessionDetail, CliSessionMessage},
    services::agent_cli::contracts::{
        SessionContentSearchRequest, SessionContentSearchResult, SessionHistoryAdapter,
        SessionHistoryRecord, SessionIndexMessage, SessionIndexSource,
    },
};
use rusqlite::Connection;
use std::path::Path;

fn indexed_sources(
    sources: &[SessionIndexSource],
    current: &dyn Fn() -> bool,
) -> Result<Connection, String> {
    let mut connection = Connection::open_in_memory().map_err(|error| error.to_string())?;
    initialize_schema(&connection)?;
    let record = SessionHistoryRecord::identity(
        AgentCliKind::Codex,
        "fixture".into(),
        Path::new("fixture"),
        Path::new("fixture").into(),
    );
    for source in sources {
        let key = source.path.to_string_lossy();
        let mut previous = None;
        for pass in 0..100_000 {
            assert!(pass < 99_999, "source must finish scanning");
            let batch = stream::read_batch_with_limit(source, previous.as_ref(), current, 1)?;
            let expected = previous
                .as_ref()
                .map(|value| serde_json::to_string(value).unwrap());
            assert!(records::apply_batch(
                &mut connection,
                "fixture",
                &record,
                &key,
                expected.as_deref(),
                &batch
            )?);
            let complete = batch.checkpoint.complete;
            previous = Some(batch.checkpoint);
            if complete {
                break;
            }
        }
    }
    Ok(connection)
}

/// Fixtures go through the real checkpoint transaction and visibility rules.
/// A record-sized batch also exercises parser state across arbitrary boundaries.
pub(crate) fn read_indexed_messages(sources: &[SessionIndexSource]) -> Vec<SessionIndexMessage> {
    let connection = indexed_sources(sources, &|| true).unwrap();
    super::reader::read_all_for_test(&connection, "fixture", "fixture").unwrap()
}

pub(crate) fn search_sources(
    sources: &[SessionIndexSource],
    request: &SessionContentSearchRequest,
    current: &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String> {
    let connection = indexed_sources(sources, current)?;
    super::search_indexed_messages(&connection, "fixture", "fixture", request)
}

pub(crate) fn read_history_detail(
    adapter: &SessionHistoryAdapter,
    record: &SessionHistoryRecord,
) -> Result<CliSessionDetail, String> {
    let sources = (adapter.index)(record)?;
    let messages = read_indexed_messages(&sources)
        .into_iter()
        .map(|message| {
            let changes =
                super::super::presentation::file_changes(&message.metadata, &message.content);
            CliSessionMessage {
                id: message.id,
                role: message.role,
                content_length: message.content.chars().count(),
                content: message.content,
                timestamp: message.metadata.timestamp.clone(),
                model: message.metadata.model.clone(),
                tool_name: message.metadata.tool_name.clone(),
                metadata: message.metadata,
                changes,
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();
    Ok(CliSessionDetail {
        session: record.summary.clone(),
        page: crate::models::SessionReadPage {
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
    search_sources(&(adapter.index)(record)?, request, current)
}
