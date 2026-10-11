use crate::{
    models::CliSessionMessageRole,
    services::{
        agent_cli::contracts::{
            SessionIndexFormat, SessionIndexMessage, SessionIndexMutation, SessionIndexSource,
        },
        cli_sessions::{json_text, normalize_timestamp},
    },
};
use serde_json::Value;
use std::path::Path;

const INDEX_PARSER_VERSION: u32 = 2;
mod decoder;

pub(super) fn index_source(path: &Path) -> SessionIndexSource {
    SessionIndexSource {
        path: path.to_path_buf(),
        parser_version: INDEX_PARSER_VERSION,
        format: SessionIndexFormat::JsonLines,
        decode: decoder::decode,
    }
}

fn response_message_text(payload: &Value) -> Option<String> {
    let content = payload.get("content")?;
    if let Some(content) = content.as_str() {
        return (!content.trim().is_empty()).then(|| content.to_string());
    }
    let text = content
        .as_array()?
        .iter()
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                None | Some("text") | Some("input_text") | Some("output_text")
            )
        })
        .filter_map(|part| {
            part.get("text")
                .or_else(|| part.get("input_text"))
                .or_else(|| part.get("output_text"))
                .and_then(Value::as_str)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}
