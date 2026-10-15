use super::*;

fn target(model: &str) -> Value {
    json!({"model": model, "isChatModel": true, "overrideScope": "core"})
}
fn pointer(key: &str) -> String {
    format!("/generateContentConfig/{}", key.replace('.', "/"))
}

pub(super) fn source_pointers(root: &Value, model: &str, key: &str) -> Vec<String> {
    let target = target(model);
    let items = root
        .pointer("/modelConfigs/customOverrides")
        .and_then(Value::as_array);
    let indices: Vec<_> = items
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, item)| item.get("match") == Some(&target))
        .map(|(index, _)| index)
        .collect();
    // Only actual matching overrides have source locations; the last one wins.
    indices
        .into_iter()
        .rev()
        .map(|index| {
            format!(
                "/modelConfigs/customOverrides/{index}/modelConfig{}",
                pointer(key)
            )
        })
        .collect()
}

pub(super) fn read(root: &Value, model: &str, key: &str) -> Option<String> {
    let target = target(model);
    let mut selected = json!({});
    for item in root.pointer("/modelConfigs/customOverrides")?.as_array()? {
        if item.get("match") == Some(&target) {
            if let Some(config) = item.get("modelConfig") {
                super::super::apply::merge(&mut selected, config);
            }
        }
    }
    selected
        .pointer(&pointer(key))
        .filter(|value| !value.is_null())
        .map(sources::display_value)
}

pub(super) fn write(
    root: &mut Value,
    model: &str,
    key: &str,
    value: Option<&str>,
    snapshot: &NativeSnapshot,
) -> Result<(), String> {
    if model.is_empty() || model.starts_with("auto") {
        return Err("先设置明确的默认模型，再调整该模型的生成参数".into());
    }
    let target = target(model);
    let mut items = match root.pointer("/modelConfigs/customOverrides") {
        Some(Value::Array(items)) => items.clone(),
        None => Vec::new(),
        _ => return Err("modelConfigs.customOverrides 必须是数组，请核对原文".into()),
    };
    let mut selected = json!({});
    let mut retained = Vec::new();
    for item in items.drain(..) {
        if item.get("match") == Some(&target)
            && item.as_object().is_some_and(|object| object.len() == 2)
        {
            if let Some(config) = item.get("modelConfig") {
                super::super::apply::merge(&mut selected, config);
            }
        } else {
            retained.push(item);
        }
    }
    let typed = value
        .map(|value| {
            typed(
                value,
                if key == "thinkingConfig.thinkingLevel" {
                    ValueType::Text
                } else {
                    ValueType::Integer
                },
            )
        })
        .transpose()?;
    if key.starts_with("thinkingConfig.")
        && selected
            .get("generateContentConfig")
            .and_then(|value| value.get("thinkingConfig"))
            .is_some_and(Value::is_null)
    {
        document::set(&mut selected, "/generateContentConfig/thinkingConfig", None)?;
    }
    document::set(&mut selected, &pointer(key), typed)?;
    if value.is_some() && key.starts_with("thinkingConfig.") {
        let opposite = if key.ends_with("thinkingLevel") {
            "thinkingConfig.thinkingBudget"
        } else {
            "thinkingConfig.thinkingLevel"
        };
        document::set(&mut selected, &pointer(opposite), None)?;
    }
    if let Some(selected_thinking) = selected.pointer("/generateContentConfig/thinkingConfig") {
        let has_level = selected_thinking.get("thinkingLevel").is_some();
        let has_budget = selected_thinking.get("thinkingBudget").is_some();
        if has_level && has_budget {
            return Err("推理级别与思考 token 预算只能设置一项，请移除其中一项设置".into());
        }
        if has_level || has_budget {
            let mut thinking = gemini::model_value(snapshot, model, "thinkingConfig")
                .map(|value| value.value)
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({"includeThoughts": true}));
            super::super::apply::merge(&mut thinking, selected_thinking);
            let fields = thinking
                .as_object_mut()
                .ok_or("Gemini thinkingConfig 必须为对象")?;
            fields.remove(if has_level {
                "thinkingBudget"
            } else {
                "thinkingLevel"
            });
            document::set(
                &mut selected,
                "/generateContentConfig/thinkingConfig",
                Some(thinking),
            )?;
            // Retain this reset even when a later edit only changes output size.
            // It clears the opposite inherited mode while preserving the other
            // thinking properties, such as includeThoughts.
            retained.push(json!({"match": target, "modelConfig": {"generateContentConfig": {"thinkingConfig": null}}}));
        }
    }
    if selected
        .as_object()
        .is_some_and(|object| !object.is_empty())
    {
        retained.push(json!({"match": target, "modelConfig": selected}));
    }
    document::set(
        root,
        "/modelConfigs/customOverrides",
        if retained.is_empty() {
            None
        } else {
            Some(json!(retained))
        },
    )
}
