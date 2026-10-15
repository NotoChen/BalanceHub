//! Configured values are available immediately. Native model directories are
//! queried separately so form rendering never waits for a remote model list.
use super::*;

pub(super) fn choices(snapshot: &NativeSnapshot) -> Vec<AgentSettingChoice> {
    let mut configured = model_mappings::choices(snapshot);
    if let Some(model) = model_observed(snapshot) {
        configured.push(choice(&model.text(), &model.text(), "当前原生配置"));
    }
    for layer in snapshot.layers.iter().filter(|layer| !layer.requirements) {
        let root = &layer.value;
        match snapshot.kind {
            AgentCliKind::ClaudeCode => {
                for pointer in ["/modelSettings", "/modelOverrides"] {
                    if let Some(models) = root.pointer(pointer).and_then(Value::as_object) {
                        configured.extend(models.keys().map(|id| choice(id, id, "原生模型配置")));
                    }
                }
                if let Some(models) = root.get("availableModels").and_then(Value::as_array) {
                    configured.extend(
                        models
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|id| choice(id, id, "原生模型列表")),
                    );
                }
            }
            AgentCliKind::Gemini | AgentCliKind::Grok | AgentCliKind::Codex => {}
        }
    }
    model_catalog::normalize(&mut configured);
    configured
}

pub(super) fn context(snapshot: &NativeSnapshot, cli_path: &str) -> AgentModelCatalogContext {
    AgentModelCatalogContext {
        cli_kind: snapshot.kind,
        cli_path: if snapshot.include_process_environment {
            cli_path.into()
        } else {
            String::new()
        },
        config_root: crate::platform::paths::compatible_path(&snapshot.root)
            .to_string_lossy()
            .into_owned(),
        catalog_path: (snapshot.kind == AgentCliKind::Codex)
            .then(|| snapshot.get("/model_catalog_json"))
            .flatten()
            .map(|configured| {
                codex::configured_catalog_path(snapshot, &configured)
                    .map(|path| {
                        crate::platform::paths::compatible_path(&path)
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default()
            }),
    }
}
