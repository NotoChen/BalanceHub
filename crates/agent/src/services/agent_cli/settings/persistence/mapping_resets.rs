use super::*;

/// Removing one row or only its model keeps sibling aliases and every other
/// setting in the edited file. Reuse the actual native writer on a memory copy.
pub(super) fn fill(
    field: &mut AgentNativeSetting,
    value: &str,
    source: &ConfigurationSourceAuthority,
    root: &Value,
    context: &FormContext,
) {
    let Some(options) = field.model_mappings.as_mut() else {
        return;
    };
    let Ok(own) = model_mappings::decode(value) else {
        return;
    };
    for entry in &mut options.entries {
        if !own.contains_key(&entry.id) || !entry.locked_reason.is_empty() {
            continue;
        }
        for model_only in [true, false] {
            let result = model_mappings::without_row(value, &entry.id, model_only)
                .and_then(|value| {
                    let mut removed = root.clone();
                    model_mappings::write(
                        context.draft.kind,
                        &mut removed,
                        value.as_deref(),
                        &context.draft,
                    )?;
                    let mut fallback = context.clone();
                    fallback.overlay(source, &removed);
                    let model = target_model(&removed, &context.inherited);
                    Ok(model_mappings::row_value(
                        &fallback.draft,
                        &entry.id,
                        &model,
                    ))
                })
                .unwrap_or_else(display::reset_error);
            if model_only {
                entry.target_default = Some(result);
            } else {
                entry.reset_value = Some(result);
            }
        }
    }
}
