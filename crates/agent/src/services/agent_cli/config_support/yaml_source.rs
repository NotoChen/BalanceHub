//! Lossless YAML documents for native configuration reads and provider
//! switches. Parsing for snapshots goes through `serde_yaml_ng`; writing keeps
//! the user's comments, ordering, and indentation through `yaml-edit`.

use serde_json::Value;
use std::str::FromStr;
use yaml_edit::{Document, Mapping, Sequence, YamlNode};

/// Reads a single YAML document as JSON-shaped data. Multi-document files are
/// refused: a native config is one document, and guessing which one to use
/// would make previews and switches disagree.
pub(crate) fn value(text: &str) -> Result<Value, ()> {
    let parsed = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text).map_err(|_| ())?;
    serde_json::to_value(parsed).map_err(|_| ())
}

/// Runs `edit` against the root mapping and returns the document with its
/// original layout. An empty file starts as an empty mapping.
pub(crate) fn edit(
    source: &str,
    context: &str,
    edit: impl FnOnce(&Mapping) -> Result<(), String>,
) -> Result<String, String> {
    let document = if source.trim().is_empty() {
        Document::new_mapping()
    } else {
        Document::from_str(source).map_err(|_| format!("{context}的 YAML 语法无效"))?
    };
    let mapping = document
        .as_mapping()
        .ok_or_else(|| format!("{context}的根节点必须是映射"))?;
    edit(&mapping)?;
    Ok(document.to_string())
}

/// Runs `edit` against the root sequence and returns the document with its
/// original layout. An empty file starts as an empty sequence.
pub(crate) fn edit_sequence(
    source: &str,
    context: &str,
    edit: impl FnOnce(&Sequence) -> Result<(), String>,
) -> Result<String, String> {
    let document = if source.trim().is_empty() {
        Document::from_str("[]").map_err(|_| format!("{context}的 YAML 语法无效"))?
    } else {
        Document::from_str(source).map_err(|_| format!("{context}的 YAML 语法无效"))?
    };
    let sequence = document
        .as_sequence()
        .ok_or_else(|| format!("{context}的根节点必须是序列"))?;
    edit(&sequence)?;
    Ok(document.to_string())
}

/// Returns the nested mapping `key`, creating an empty one when absent.
pub(crate) fn mapping_member(
    parent: &Mapping,
    key: &str,
    context: &str,
) -> Result<Mapping, String> {
    if let Some(current) = parent.get(key) {
        return current
            .as_mapping()
            .cloned()
            .ok_or_else(|| format!("{context}中的 {key} 必须是映射"));
    }
    parent.set(key, Mapping::new());
    parent
        .get_mapping(key)
        .ok_or_else(|| format!("{context}中的 {key} 必须是映射"))
}

/// Returns the sequence `key`, creating an empty one when absent.
pub(crate) fn sequence_member(
    parent: &Mapping,
    key: &str,
    context: &str,
) -> Result<Sequence, String> {
    if let Some(current) = parent.get(key) {
        return current
            .as_sequence()
            .cloned()
            .ok_or_else(|| format!("{context}中的 {key} 必须是序列"));
    }
    parent.set(key, Sequence::new());
    parent
        .get_sequence(key)
        .ok_or_else(|| format!("{context}中的 {key} 必须是序列"))
}

/// The scalar text of a node, unquoted and unescaped.
pub(crate) fn scalar_text(node: &YamlNode) -> Option<String> {
    node.as_scalar().map(|scalar| scalar.as_string())
}

/// The mapping at `key`, when present and of that shape.
pub(crate) fn mapping_at(parent: &Mapping, key: &str) -> Option<Mapping> {
    parent.get_mapping(key)
}

/// The scalar text at `key`, when present.
pub(crate) fn text_at(parent: &Mapping, key: &str) -> Option<String> {
    parent.get(key).as_ref().and_then(scalar_text)
}

/// The non-empty scalar text at `key`.
pub(crate) fn nonempty_text_at(parent: &Mapping, key: &str) -> Option<String> {
    text_at(parent, key)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
