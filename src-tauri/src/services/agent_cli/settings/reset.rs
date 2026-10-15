//! Resolve reset previews against the remaining in-memory invocation options.
//! No file writes, source rediscovery, remote model query or asset scan occurs.
use super::*;
use std::collections::BTreeMap;

fn overlay(
    base: &NativeSnapshot,
    options: &BTreeMap<String, String>,
) -> Result<NativeSnapshot, String> {
    let mut snapshot = base.clone();
    model_mappings::overlay(&mut snapshot, options)?;
    Ok(snapshot)
}

pub(super) fn launch_values(
    base: &NativeSnapshot,
    request: &TemporaryCliSettingsRequest,
    model: &str,
    help: &str,
    fields: &mut [BoundSetting],
) {
    for field in fields {
        let Some(value) = request.native_options.get(&field.ui.id) else {
            continue;
        };
        let mut remaining = request.native_options.clone();
        remaining.remove(&field.ui.id);
        let reset = overlay(base, &remaining).map(|snapshot| {
            let mut reset_fields = fields_for(
                &snapshot,
                model,
                help,
                &request.cli_path,
                false,
                Some(&remaining),
            );
            display::annotate(&mut reset_fields, &snapshot);
            reset_fields
                .into_iter()
                .find(|item| item.ui.id == field.ui.id)
                .map(|item| display::effective(&item.ui))
        });
        field.ui.reset_value = match reset {
            Ok(value) => value,
            Err(error) => Some(AgentSettingEffectiveValue {
                summary: display::reset_error(error),
                model_mappings: None,
            }),
        };
        let Some(options) = field.ui.model_mappings.as_mut() else {
            continue;
        };
        let Ok(own) = model_mappings::decode(value) else {
            continue;
        };
        for entry in &mut options.entries {
            if !own.contains_key(&entry.id) || !entry.locked_reason.is_empty() {
                continue;
            }
            for model_only in [true, false] {
                let resolved = model_mappings::without_row(value, &entry.id, model_only)
                    .and_then(|value| {
                        let mut remaining = request.native_options.clone();
                        if let Some(value) = value {
                            remaining.insert("modelMappings".into(), value);
                        } else {
                            remaining.remove("modelMappings");
                        }
                        overlay(base, &remaining)
                            .map(|snapshot| model_mappings::row_value(&snapshot, &entry.id, model))
                    })
                    .unwrap_or_else(display::reset_error);
                if model_only {
                    entry.target_default = Some(resolved);
                } else {
                    entry.reset_value = Some(resolved);
                }
            }
        }
    }
}
