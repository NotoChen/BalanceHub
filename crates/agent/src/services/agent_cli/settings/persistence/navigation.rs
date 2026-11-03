//! Source references follow the same storage bindings as form edits.
use super::{bindings::Storage, *};
use crate::services::agent_cli::config_support::document_locations;
use std::collections::BTreeSet;

pub(super) struct SourcePath {
    parts: Vec<String>,
    row_id: Option<String>,
}

impl SourcePath {
    fn new(pointer: &str) -> Self {
        Self {
            parts: pointer
                .trim_start_matches('/')
                .split('/')
                .map(|part| part.replace("~1", "/").replace("~0", "~"))
                .collect(),
            row_id: None,
        }
    }
}

pub(super) fn paths(
    storage: &Storage,
    field: &AgentNativeSetting,
    root: &Value,
    model: &str,
) -> Vec<SourcePath> {
    let pointers = match storage {
        Storage::Key(pointer, _) | Storage::BooleanFlag(pointer) => vec![pointer.clone()],
        Storage::ClaudeModel => vec!["/env/ANTHROPIC_MODEL".into(), "/model".into()],
        Storage::Sandbox => vec!["/tools/sandbox".into()],
        Storage::Approval => vec!["/approval_policy".into()],
        Storage::ApprovalCategories => vec!["/approval_policy/granular".into()],
        Storage::Subagents => vec![
            "/agents/enabled".into(),
            "/features/multi_agent".into(),
            if root
                .pointer("/features/multi_agent_v2")
                .is_some_and(Value::is_object)
            {
                "/features/multi_agent_v2/enabled"
            } else {
                "/features/multi_agent_v2"
            }
            .into(),
        ],
        Storage::SubagentLimit => vec![codex::V2_LIMIT_POINTER.into()],
        Storage::GeminiModel(key) => gemini_model::source_pointers(root, model, key),
        Storage::ModelMappings(kind) => {
            return field
                .model_mappings
                .iter()
                .flat_map(|options| &options.entries)
                .flat_map(|row| {
                    model_mappings::source_pointers(*kind, &row.id)
                        .into_iter()
                        .map(|pointer| {
                            let mut path = SourcePath::new(&pointer);
                            path.row_id = Some(row.id.clone());
                            path
                        })
                })
                .collect();
        }
    };
    pointers
        .iter()
        .map(|pointer| SourcePath::new(pointer))
        .collect()
}

pub(super) fn attach(
    fields: &mut [AgentNativeSetting],
    paths: Vec<Vec<SourcePath>>,
    text: &str,
    root: &Value,
    format: AgentConfigurationFormat,
) -> Result<(), String> {
    let requested: Vec<_> = paths
        .iter()
        .flatten()
        .map(|path| path.parts.clone())
        .collect();
    let ranges = document_locations::read(text, format, root, &requested)?;
    let offsets = utf16_offsets(text, &ranges);
    for (field, paths) in fields.iter_mut().zip(paths) {
        field.source_references = paths
            .into_iter()
            .filter_map(|path| {
                // A reference identifies an existing key in this draft. An
                // inherited/default value has no location in the current file.
                let range = ranges.get(&path.parts)?;
                let from = offsets[&range.key.start];
                let to = offsets[&range.key.end];
                let value_end = offsets[&range.value.end];
                Some(AgentSettingSourceReference {
                    key: path
                        .parts
                        .iter()
                        .map(|part| {
                            if !part.is_empty()
                                && part
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                            {
                                part.clone()
                            } else {
                                serde_json::to_string(part).expect("string is JSON serializable")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("."),
                    row_id: path.row_id,
                    from,
                    to,
                    value_end: value_end.max(to),
                })
            })
            .collect();
    }
    Ok(())
}

/// Convert only required boundaries in one pass. CodeMirror counts UTF-16 code
/// units and represents each CRLF separator as one character in its document.
fn utf16_offsets(text: &str, ranges: &document_locations::Ranges) -> BTreeMap<usize, usize> {
    let boundaries: BTreeSet<_> = ranges
        .values()
        .flat_map(|range| [range.key.start, range.key.end, range.value.end])
        .collect();
    let mut remaining = boundaries.into_iter().peekable();
    let mut offsets = BTreeMap::new();
    let mut position = 0;
    for (byte, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        while remaining.peek().is_some_and(|boundary| *boundary <= byte) {
            offsets.insert(remaining.next().expect("checked boundary"), position);
        }
        if remaining.peek().is_none() {
            break;
        }
        if character != '\r' || text.as_bytes().get(byte + 1) != Some(&b'\n') {
            position += character.len_utf16();
        }
    }
    offsets
}
