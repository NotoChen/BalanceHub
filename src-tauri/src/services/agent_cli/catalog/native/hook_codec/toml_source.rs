//! Retain TOML source structure, including inline arrays/tables and comments
//! inside a modified matcher group. Only the selected semantic leaves change.
use crate::services::agent_cli::{
    config_support::align_json_array,
    environment::config_document::{self, ConfigDocumentFormat},
};
use serde_json::Value;

pub(super) fn render_toml(
    text: &str,
    original: &Value,
    desired: &Value,
) -> Result<Vec<u8>, String> {
    let mut document = text
        .parse::<toml_edit::Document>()
        .map_err(|_| "Hook TOML 配置无效")?;
    let rendered = toml::to_string(desired).map_err(|_| "Hook 定义不能无损表示为 TOML")?;
    let replacement = rendered
        .parse::<toml_edit::Document>()
        .map_err(|_| "Hook TOML 转换失败")?;
    patch_table(
        document.as_table_mut(),
        replacement.as_table(),
        original,
        desired,
    )?;
    let bytes = document.to_string().into_bytes();
    if config_document::parse(&bytes, ConfigDocumentFormat::Toml).as_ref() != Some(desired) {
        return Err("Hook TOML 修改未能保留目标结构".to_owned());
    }
    Ok(bytes)
}

fn patch_table(
    table: &mut toml_edit::Table,
    replacement: &toml_edit::Table,
    original: &Value,
    desired: &Value,
) -> Result<(), String> {
    let old = original.as_object().ok_or("Hook TOML 原始表无效")?;
    let new = desired.as_object().ok_or("Hook TOML 目标表无效")?;
    for key in old.keys().filter(|key| !new.contains_key(*key)) {
        table.remove(key);
    }
    for (key, value) in new {
        if old.get(key) == Some(value) {
            continue;
        }
        let desired_item = replacement.get(key).ok_or("Hook TOML 字段转换失败")?;
        if let (Some(current), Some(previous)) = (table.get_mut(key), old.get(key)) {
            patch_item(current, desired_item, previous, value)?;
        } else {
            table.insert(key, desired_item.clone());
        }
    }
    if new.is_empty() {
        // Removing the last child of an implicit or dotted parent must not
        // remove the retained empty object from the serialized document.
        table.set_implicit(false);
        table.set_dotted(false);
    }
    Ok(())
}

fn assignment(old: &[Value], new: &[Value]) -> Result<Vec<Option<usize>>, String> {
    let (old_to_new, _) = align_json_array(old, new)?;
    let mut result = vec![None; new.len()];
    for (previous, next) in old_to_new.into_iter().enumerate() {
        if let Some(next) = next {
            result[next] = Some(previous);
        }
    }
    Ok(result)
}

fn patch_item(
    item: &mut toml_edit::Item,
    replacement: &toml_edit::Item,
    original: &Value,
    desired: &Value,
) -> Result<(), String> {
    if original == desired {
        return Ok(());
    }
    if let (Some(table), Some(replacement_table)) = (item.as_table_mut(), replacement.as_table()) {
        return patch_table(table, replacement_table, original, desired);
    }
    if let (Some(array), Some(replacements), Some(old), Some(new)) = (
        item.as_array_of_tables_mut(),
        replacement.as_array_of_tables(),
        original.as_array(),
        desired.as_array(),
    ) {
        let mut retained = Vec::new();
        for (index, previous) in assignment(old, new)?.into_iter().enumerate() {
            let replacement = replacements.get(index).ok_or("Hook TOML 数组转换失败")?;
            if let Some(previous) = previous {
                let mut table = array.get(previous).ok_or("Hook TOML 数组证据无效")?.clone();
                patch_table(&mut table, replacement, &old[previous], &new[index])?;
                retained.push(table);
            } else {
                retained.push(replacement.clone());
            }
        }
        array.clear();
        for table in retained {
            array.push(table);
        }
        return Ok(());
    }
    if let Some(value) = item.as_value_mut() {
        let replacement = replacement
            .clone()
            .into_value()
            .map_err(|_| "Hook TOML 值转换失败")?;
        return patch_value(value, &replacement, original, desired);
    }
    *item = replacement.clone();
    Ok(())
}

fn patch_value(
    value: &mut toml_edit::Value,
    replacement: &toml_edit::Value,
    original: &Value,
    desired: &Value,
) -> Result<(), String> {
    if original == desired {
        return Ok(());
    }
    if let (Some(table), Some(replacement), Some(old), Some(new)) = (
        value.as_inline_table_mut(),
        replacement.as_inline_table(),
        original.as_object(),
        desired.as_object(),
    ) {
        for key in old.keys().filter(|key| !new.contains_key(*key)) {
            table.remove(key);
        }
        for (key, next) in new {
            if old.get(key) == Some(next) {
                continue;
            }
            let replaced = replacement.get(key).ok_or("Hook TOML 内联表转换失败")?;
            if let (Some(current), Some(previous)) = (table.get_mut(key), old.get(key)) {
                patch_value(current, replaced, previous, next)?;
            } else {
                table.insert(key, replaced.clone());
            }
        }
        return Ok(());
    }
    if let (Some(array), Some(replacements), Some(old), Some(new)) = (
        value.as_array_mut(),
        replacement.as_array(),
        original.as_array(),
        desired.as_array(),
    ) {
        let mut retained = Vec::new();
        for (index, previous) in assignment(old, new)?.into_iter().enumerate() {
            let replacement = replacements
                .get(index)
                .ok_or("Hook TOML 内联数组转换失败")?;
            if let Some(previous) = previous {
                let mut value = array
                    .get(previous)
                    .ok_or("Hook TOML 内联数组证据无效")?
                    .clone();
                patch_value(&mut value, replacement, &old[previous], &new[index])?;
                retained.push(value);
            } else {
                retained.push(replacement.clone());
            }
        }
        array.clear();
        for value in retained {
            array.push_formatted(value);
        }
        return Ok(());
    }
    let decor = value.decor().clone();
    *value = replacement.clone();
    *value.decor_mut() = decor;
    Ok(())
}
