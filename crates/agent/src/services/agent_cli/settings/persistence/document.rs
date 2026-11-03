use super::*;
use crate::services::agent_cli::{
    config_support::{rewrite_json_document, text_document},
    configuration::native_support,
};

pub(super) fn parse(text: &str, format: AgentConfigurationFormat) -> Result<Value, String> {
    if text_document::body(text).trim().is_empty() {
        return Ok(json!({}));
    }
    let root = native_support::parse(text, format)
        .map_err(|_| "配置格式无效，请在原文中检查语法、重复字段和文件格式")?;
    if !root.is_object() {
        return Err("配置根节点必须是对象，请在原文中核对".into());
    }
    Ok(root)
}

pub(in crate::services::agent_cli::settings) fn set(
    root: &mut Value,
    pointer: &str,
    value: Option<Value>,
) -> Result<(), String> {
    let path = pointer
        .trim_start_matches('/')
        .split('/')
        .map(|key| key.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    set_at(root, &path, value)
}

fn set_at(root: &mut Value, path: &[String], value: Option<Value>) -> Result<(), String> {
    let Some((key, rest)) = path.split_first() else {
        return Err("配置字段路径为空".into());
    };
    let object = root
        .as_object_mut()
        .ok_or("配置字段的父节点不是对象，请在原文中核对")?;
    if rest.is_empty() {
        if let Some(value) = value {
            object.insert(key.clone(), value);
        } else {
            object.remove(key);
        }
    } else if value.is_none() && !object.contains_key(key) {
        return Ok(());
    } else {
        let child = object.entry(key).or_insert_with(|| json!({}));
        set_at(child, rest, value)?;
        if child.as_object().is_some_and(|object| object.is_empty()) {
            object.remove(key);
        }
    }
    Ok(())
}

pub(super) fn rewrite(
    text: &str,
    desired: &Value,
    format: AgentConfigurationFormat,
) -> Result<String, String> {
    let before = parse(text, format)?;
    if &before == desired {
        return Ok(text.into());
    }
    let output = if format == AgentConfigurationFormat::Toml {
        let mut original = text_document::body(text)
            .parse::<toml_edit::Document>()
            .map_err(|_| "TOML 格式无效，请在原文中检查语法")?;
        let generated = toml::to_string(desired)
            .map_err(|_| "无法将当前字段写为 TOML，请核对原文结构")?
            .parse::<toml_edit::Document>()
            .map_err(|_| "无法生成有效的 TOML 草稿")?;
        merge_item(
            original.as_item_mut(),
            generated.as_item(),
            &before,
            desired,
        );
        original.to_string()
    } else if text_document::body(text).trim().is_empty() {
        serde_json::to_string_pretty(desired).map_err(|_| "无法生成有效的 JSON 草稿")?
    } else {
        rewrite_json_document(
            text_document::body(text),
            desired,
            format == AgentConfigurationFormat::Jsonc,
        )
        .map_err(|_| "无法保留现有 JSON 结构，请在原文中核对后重试")?
    };
    let output = text_document::restore_layout(text, &output);
    if parse(&output, format)? != *desired {
        return Err("可视化修改未能保留配置结构，草稿未更新".into());
    }
    Ok(output)
}

fn merge_item(
    target: &mut toml_edit::Item,
    generated: &toml_edit::Item,
    before: &Value,
    after: &Value,
) {
    if before == after {
        return;
    }
    if let (Some(table), Some(next), Some(before), Some(after)) = (
        target.as_table_mut(),
        generated.as_table(),
        before.as_object(),
        after.as_object(),
    ) {
        let removed = table
            .iter()
            .filter(|(key, _)| !after.contains_key(*key))
            .map(|(key, _)| key.to_owned())
            .collect::<Vec<_>>();
        for key in removed {
            table.remove(&key);
        }
        for (key, item) in next.iter() {
            if let Some(target) = table.get_mut(key) {
                merge_item(
                    target,
                    item,
                    before.get(key).unwrap_or(&Value::Null),
                    after.get(key).unwrap_or(&Value::Null),
                );
            } else {
                table.insert(key, item.clone());
            }
        }
    } else {
        let decor = target.as_value().map(|value| value.decor().clone());
        *target = generated.clone();
        if let (Some(value), Some(decor)) = (target.as_value_mut(), decor) {
            *value.decor_mut() = decor;
        }
    }
}
