//! Compose adapter-prepared node changes against a single captured file. This
//! keeps unrelated data/comments and rejects overlapping contradictory edits.
use crate::services::agent_cli::environment::config_document::{self, ConfigDocumentFormat};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Delta {
    path: Vec<String>,
    before: Option<Value>,
    after: Option<Value>,
}

pub(super) fn compose(
    before: &[u8],
    replacements: &[Vec<u8>],
    format: ConfigDocumentFormat,
) -> Result<Vec<u8>, String> {
    let base = if before.is_empty() {
        serde_json::json!({})
    } else {
        config_document::parse(before, format).ok_or("原生配置不能安全合成")?
    };
    let mut deltas = BTreeMap::<Vec<String>, Delta>::new();
    for replacement in replacements {
        let desired = config_document::parse(replacement, format).ok_or("原生变更不是有效配置")?;
        let mut changes = Vec::new();
        diff(Some(&base), Some(&desired), &[], &mut changes);
        for change in changes {
            for existing in deltas.values() {
                if change.path == existing.path {
                    if change.after != existing.after {
                        return Err("多个目标对同一配置字段要求矛盾变更".to_owned());
                    }
                } else if change.path.starts_with(&existing.path)
                    || existing.path.starts_with(&change.path)
                {
                    return Err("原生变更存在父子字段冲突".to_owned());
                }
            }
            deltas.insert(change.path.clone(), change);
        }
    }
    let mut value = base.clone();
    for change in deltas.values() {
        apply_json(&mut value, &change.path, &change.before, &change.after)?;
    }
    if format == ConfigDocumentFormat::Json {
        return serde_json::to_vec_pretty(&value).map_err(|_| "无法生成合并配置".to_owned());
    }
    let text = std::str::from_utf8(before).map_err(|_| "配置不是 UTF-8")?;
    let mut document = text
        .parse::<toml_edit::Document>()
        .map_err(|_| "TOML 配置无效")?;
    for change in deltas.values() {
        apply_toml(document.as_item_mut(), &change.path, &change.after)?;
    }
    let bytes = document.to_string().into_bytes();
    if config_document::parse(&bytes, format) != Some(value) {
        return Err("合并配置验证失败".to_owned());
    }
    Ok(bytes)
}

fn diff(before: Option<&Value>, after: Option<&Value>, path: &[String], changes: &mut Vec<Delta>) {
    if before == after {
        return;
    }
    if let Some(after_object) = after.and_then(Value::as_object) {
        if before.is_none() || before.is_some_and(Value::is_object) {
            let keys = before
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|object| object.keys())
                .chain(after_object.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let mut nested = path.to_vec();
                nested.push(key.clone());
                diff(
                    before.and_then(|value| value.get(&key)),
                    after_object.get(&key),
                    &nested,
                    changes,
                );
            }
            return;
        }
    }
    changes.push(Delta {
        path: path.to_vec(),
        before: before.cloned(),
        after: after.cloned(),
    });
}

fn apply_json(
    root: &mut Value,
    path: &[String],
    before: &Option<Value>,
    after: &Option<Value>,
) -> Result<(), String> {
    let (head, tail) = path.split_first().ok_or("不允许替换整份配置")?;
    let object = root.as_object_mut().ok_or("配置父节点类型变化")?;
    if tail.is_empty() {
        if object.get(head) != before.as_ref() {
            return Err("配置节点前置条件冲突".to_owned());
        }
        if let Some(value) = after {
            object.insert(head.clone(), value.clone());
        } else {
            object.remove(head);
        }
        return Ok(());
    }
    apply_json(
        object.entry(head).or_insert_with(|| serde_json::json!({})),
        tail,
        before,
        after,
    )
}

fn apply_toml(
    root: &mut toml_edit::Item,
    path: &[String],
    after: &Option<Value>,
) -> Result<(), String> {
    let (head, tail) = path.split_first().ok_or("TOML 路径为空")?;
    let table = root.as_table_like_mut().ok_or("TOML 目标不是表")?;
    if tail.is_empty() {
        match after {
            None => {
                table.remove(head);
            }
            Some(value) => {
                let text = toml::to_string(&BTreeMap::from([("value", value)]))
                    .map_err(|_| "TOML 字段转换失败")?;
                let item = text
                    .parse::<toml_edit::Document>()
                    .map_err(|_| "TOML 字段无效")?["value"]
                    .clone();
                table.insert(head, item);
            }
        }
        return Ok(());
    }
    if !table.contains_key(head) {
        table.insert(head, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    apply_toml(table.get_mut(head).ok_or("TOML 父节点不存在")?, tail, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_mcp_two_target_merge_preserves_other_nodes_and_comments() {
        let mut before = "# preserve\n[unrelated]\nsecret = 'unchanged'\n".to_owned();
        for index in 0..10 {
            before.push_str(&format!(
                "[mcp_servers.item{index}]\ncommand = 'runner'\nenabled = true\n"
            ));
        }
        let changes = [2, 7].map(|index| {
            let mut doc = before.parse::<toml_edit::Document>().unwrap();
            doc["mcp_servers"][&format!("item{index}")]["enabled"] = toml_edit::value(false);
            doc.to_string().into_bytes()
        });
        let result = compose(before.as_bytes(), &changes, ConfigDocumentFormat::Toml).unwrap();
        assert!(std::str::from_utf8(&result).unwrap().contains("# preserve"));
        let value = config_document::parse(&result, ConfigDocumentFormat::Toml).unwrap();
        assert_eq!(value["mcp_servers"].as_object().unwrap().len(), 10);
        for index in 0..10 {
            assert_eq!(
                value["mcp_servers"][format!("item{index}")]["enabled"],
                ![2, 7].contains(&index)
            );
        }
        assert_eq!(value["unrelated"]["secret"], "unchanged");
    }
    #[test]
    fn incompatible_edits_are_rejected_before_write() {
        assert!(compose(
            br#"{"value":0}"#,
            &[br#"{"value":1}"#.to_vec(), br#"{"value":2}"#.to_vec()],
            ConfigDocumentFormat::Json
        )
        .is_err());
    }
}
