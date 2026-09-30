//! A resource editor selects one native node; saving rejoins the verified file.
use super::native_support;
use crate::{
    models::{AgentConfigurationError, AgentConfigurationErrorKind, AgentConfigurationFormat},
    services::agent_cli::config_support::json_source::json_value_range,
};
use serde_json::Value;

#[derive(Clone, Default)]
pub(super) enum DocumentSelection {
    #[default]
    Whole,
    Structured {
        path: Vec<String>,
        format: AgentConfigurationFormat,
    },
}

impl DocumentSelection {
    pub(super) fn text(&self, source: &str) -> Result<String, AgentConfigurationError> {
        self.text_with_document(source, None)
    }

    pub(super) fn text_with_document(
        &self,
        source: &str,
        parsed: Option<&toml_edit::Document>,
    ) -> Result<String, AgentConfigurationError> {
        match self {
            Self::Whole => Ok(source.to_owned()),
            Self::Structured { path, format } if *format != AgentConfigurationFormat::Toml => {
                let range =
                    json_value_range(source, path, *format == AgentConfigurationFormat::Jsonc)
                        .map_err(rejected)?;
                Ok(source[range].to_owned())
            }
            Self::Structured { path, .. } => {
                let owned;
                let document = match parsed {
                    Some(document) => document,
                    None => {
                        owned = source
                            .parse::<toml_edit::Document>()
                            .map_err(|_| invalid())?;
                        &owned
                    }
                };
                let mut item = document.as_item();
                for key in path {
                    item = if item.is_array() || item.is_array_of_tables() {
                        item.get(key.parse::<usize>().map_err(|_| invalid())?)
                    } else {
                        item.get(key.as_str())
                    }
                    .ok_or_else(invalid)?;
                }
                if let Some(table) = item.as_table() {
                    let mut selected = toml_edit::Document::new();
                    *selected.as_table_mut() = table.clone();
                    Ok(selected.to_string())
                } else {
                    // Inline TOML has no standalone document representation.
                    // Read its complete source instead of manufacturing one.
                    Err(rejected(
                        "此条目使用内联 TOML，显示完整来源文件以保留原文".to_owned(),
                    ))
                }
            }
        }
    }

    pub(super) fn replace(
        &self,
        source: &str,
        text: &str,
    ) -> Result<String, AgentConfigurationError> {
        match self {
            Self::Whole => Ok(text.to_owned()),
            Self::Structured { path, format } => {
                let replacement = native_support::parse(text, *format)?;
                let original = native_support::parse(source, *format)?;
                let mut desired = original.clone();
                *value_at_mut(&mut desired, path).ok_or_else(invalid)? = replacement;
                let output = if *format == AgentConfigurationFormat::Toml {
                    replace_toml_table(source, path, text)?
                } else {
                    let range =
                        json_value_range(source, path, *format == AgentConfigurationFormat::Jsonc)
                            .map_err(rejected)?;
                    let mut output = source.to_owned();
                    output.replace_range(range, text);
                    output
                };
                if native_support::parse(&output, *format)? != desired {
                    return Err(rejected("保存不能改变其他配置条目".to_owned()));
                }
                Ok(output)
            }
        }
    }
}

fn replace_toml_table(
    source: &str,
    path: &[String],
    text: &str,
) -> Result<String, AgentConfigurationError> {
    let mut document = source
        .parse::<toml_edit::Document>()
        .map_err(|_| invalid())?;
    let replacement = text.parse::<toml_edit::Document>().map_err(|_| invalid())?;
    let mut item = document.as_item_mut();
    for key in path {
        // get_mut on a missing TOML key inserts it; resource paths must already exist.
        if item.is_array() || item.is_array_of_tables() {
            let index = key.parse::<usize>().map_err(|_| invalid())?;
            if item.get(index).is_none() {
                return Err(invalid());
            }
            item = item.get_mut(index).ok_or_else(invalid)?;
        } else {
            if item.get(key.as_str()).is_none() {
                return Err(invalid());
            }
            item = item.get_mut(key.as_str()).ok_or_else(invalid)?;
        }
    }
    let previous = item.as_table().ok_or_else(invalid)?;
    let mut table = replacement.as_table().clone();
    retain_table_positions(&mut table, Some(previous));
    *table.decor_mut() = previous.decor().clone();
    let trailing = replacement.trailing().as_str().unwrap_or_default();
    if !trailing.trim().is_empty() {
        // A standalone fragment's trailing comment belongs to this table,
        // rather than to the end of the entire Agent configuration file.
        let suffix = table
            .decor()
            .suffix()
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned();
        table
            .decor_mut()
            .set_suffix(format!("{suffix}\n{}", trailing.trim_end()));
    }
    *item = toml_edit::Item::Table(table);
    Ok(document.to_string())
}

fn retain_table_positions(table: &mut toml_edit::Table, previous: Option<&toml_edit::Table>) {
    if let Some(position) = previous.and_then(toml_edit::Table::position) {
        table.set_position(position);
    } else {
        table.set_position(usize::MAX);
    }
    for (key, item) in table.iter_mut() {
        let before = previous.and_then(|table| table.get(key.get()));
        if let Some(table) = item.as_table_mut() {
            retain_table_positions(table, before.and_then(toml_edit::Item::as_table));
        } else if let Some(tables) = item.as_array_of_tables_mut() {
            for (index, table) in tables.iter_mut().enumerate() {
                retain_table_positions(
                    table,
                    before
                        .and_then(toml_edit::Item::as_array_of_tables)
                        .and_then(|tables| tables.get(index)),
                );
            }
        }
    }
}

pub(super) fn value_at<'a>(mut value: &'a Value, path: &[String]) -> Option<&'a Value> {
    for key in path {
        value = if let Some(object) = value.as_object() {
            object.get(key)?
        } else {
            value.as_array()?.get(key.parse::<usize>().ok()?)?
        };
    }
    Some(value)
}

fn value_at_mut<'a>(value: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let Some((key, remaining)) = path.split_first() else {
        return Some(value);
    };
    let child = if value.is_object() {
        value.as_object_mut()?.get_mut(key)?
    } else {
        value.as_array_mut()?.get_mut(key.parse::<usize>().ok()?)?
    };
    value_at_mut(child, remaining)
}

pub(super) fn rejected(message: String) -> AgentConfigurationError {
    AgentConfigurationError {
        kind: AgentConfigurationErrorKind::SourceUnavailable,
        message,
        diagnostics: Vec::new(),
    }
}
fn invalid() -> AgentConfigurationError {
    AgentConfigurationError::new(AgentConfigurationErrorKind::InvalidSyntax)
}
