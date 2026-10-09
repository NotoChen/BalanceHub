use super::{initialize_schema, records, stream, MESSAGE_CHUNK_OVERLAP_CHARS};
use crate::{
    models::{AgentCliKind, CliSessionMessageRole},
    services::agent_cli::contracts::{
        SessionHistoryRecord, SessionIndexMessage, SessionIndexSource,
    },
};
use rusqlite::Connection;
use std::path::Path;

/// Exercise each adapter through real checkpoint transactions, one native
/// record per batch, so filtering tests also cover cross-batch parser state.
pub(crate) fn read_indexed_messages(sources: &[SessionIndexSource]) -> Vec<SessionIndexMessage> {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize_schema(&connection).unwrap();
    let record = SessionHistoryRecord::identity(
        AgentCliKind::Codex,
        "fixture".into(),
        Path::new("fixture"),
        Path::new("fixture").into(),
    );
    for source in sources {
        let key = source.path.to_string_lossy();
        let mut previous = None;
        for pass in 0..10_000 {
            assert!(pass < 9_999, "source must finish scanning");
            let batch =
                stream::read_batch_with_limit(source, previous.as_ref(), &|| true, 1).unwrap();
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
            )
            .unwrap());
            let complete = batch.checkpoint.complete;
            previous = Some(batch.checkpoint);
            if complete {
                break;
            }
        }
    }
    let mut statement = connection.prepare("SELECT message_id, role, chunk_index, content FROM messages
        WHERE priority=(SELECT MAX(priority) FROM messages) ORDER BY source_key, message_order, message_id, chunk_index").unwrap();
    let mut rows = statement.query([]).unwrap();
    let mut messages: Vec<SessionIndexMessage> = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        let chunk: i64 = row.get(2).unwrap();
        let content: String = row.get(3).unwrap();
        if chunk == 0 {
            messages.push(SessionIndexMessage {
                id: row.get(0).unwrap(),
                role: match row.get::<_, String>(1).unwrap().as_str() {
                    "user" => CliSessionMessageRole::User,
                    "assistant" => CliSessionMessageRole::Assistant,
                    _ => panic!("tools cannot enter the conversation index"),
                },
                content,
            });
        } else {
            messages
                .last_mut()
                .unwrap()
                .content
                .extend(content.chars().skip(MESSAGE_CHUNK_OVERLAP_CHARS));
        }
    }
    messages
}
