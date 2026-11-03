//! Native alias bindings shared by persistent configuration and launch overlays.
//! These mappings never implement an HTTP proxy or change provider credentials.
use super::*;
use crate::services::agent_cli::contracts::TemporaryLaunchPlan;
use std::collections::{BTreeMap, BTreeSet};

mod display;
mod native;
type Mappings = BTreeMap<String, AgentModelMappingValue>;

pub(super) fn source_pointers(kind: AgentCliKind, id: &str) -> Vec<String> {
    let paths = native::paths(kind, id);
    std::iter::once(paths.model)
        .chain(paths.display_name)
        .chain(paths.parent)
        .collect()
}

pub(super) fn field(
    snapshot: &NativeSnapshot,
    model: &str,
    persistent: bool,
) -> Option<BoundSetting> {
    if snapshot.kind == AgentCliKind::Codex {
        return None;
    }
    let (label, key, description) = match snapshot.kind {
        AgentCliKind::ClaudeCode => ("模型档位映射", "env.ANTHROPIC_DEFAULT_*_MODEL / CLAUDE_CODE_SUBAGENT_MODEL", "为各档位指定实际模型 ID；显示名称只改变 /model 菜单文字。主模型切换不会改写这些映射。子 Agent 未单独指定时按当前会话与角色配置选择。"),
        AgentCliKind::Gemini => ("模型别名", "modelConfigs.customAliases", "为模型添加可复用的别名。目标填写实际模型 ID；继承配置可选择已有别名，保留其生成参数。主模型中选择该别名后生效。"),
        AgentCliKind::Grok => ("模型别名", "model.<别名>.model / name", "别名对应 Grok 自定义模型条目，实际模型与显示名称分别保存。连接地址、认证及生成参数继续使用条目的原生设置；新别名使用 Grok 默认连接。"),
        AgentCliKind::Codex => return None,
    };
    let mut field = BoundSetting::new("modelMappings", "model", label, key, Binding::ModelMappings)
        .describe(description);
    field.ui.control = AgentSettingControl::ModelMappings;
    let values = match native::effective(snapshot) {
        Ok(values) => values,
        Err(error) => {
            field.ui.locked_reason = error;
            Mappings::new()
        }
    };
    let mut entries: Vec<_> = native::ids(snapshot)
        .into_iter()
        .map(|id| {
            let paths = native::paths(snapshot.kind, &id);
            let label = display::label(snapshot.kind, &id);
            let mut current = display::value(snapshot, &values, &id, model);
            if !field.ui.locked_reason.is_empty() {
                current.state = AgentSettingValueState::ReadError;
                current.label = "映射读取失败".into();
                current.detail = field.ui.locked_reason.clone();
            }
            AgentModelMappingEntry {
                supports_display_name: paths.display_name.is_some(),
                native_key: paths.model.trim_start_matches('/').replace('/', "."),
                current,
                target_default: None,
                reset_value: None,
                locked_reason: if native::managed(snapshot, &id) {
                    "由系统或组织的管理配置指定".into()
                } else {
                    String::new()
                },
                id,
                label,
            }
        })
        .collect();
    if snapshot.kind == AgentCliKind::ClaudeCode {
        entries.sort_by_key(|entry| {
            ["sonnet", "opus", "haiku", "fable", "subagent"]
                .iter()
                .position(|id| *id == entry.id)
                .unwrap_or(5)
        });
    }
    field.ui.source = entries
        .iter()
        .map(|entry| entry.current.source.as_str())
        .filter(|source| !source.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n");
    field.ui.model_mappings = Some(AgentModelMappingOptions {
        allow_custom_aliases: snapshot.kind != AgentCliKind::ClaudeCode,
        display_names: snapshot.kind != AgentCliKind::Gemini,
        inheritance: snapshot.kind == AgentCliKind::Gemini,
        entries,
    });
    field.ui.current_value = serde_json::to_string(&values).ok();
    let count = values
        .values()
        .filter(|value| **value != AgentModelMappingValue::default())
        .count();
    field.ui.current_label = if count == 0 {
        "未设置模型映射".into()
    } else {
        format!("已设置 {count} 项模型映射")
    };
    field.ui.current_state = if count == 0 {
        AgentSettingValueState::Unset
    } else {
        AgentSettingValueState::Configured
    };
    field.ui.current_detail.clear();
    if !field.ui.locked_reason.is_empty() {
        field.ui.current_state = AgentSettingValueState::ReadError;
        field.ui.current_label = "模型映射读取失败".into();
        field.ui.current_detail = field.ui.locked_reason.clone();
    }
    field.ui.choices = models::choices(snapshot);
    if snapshot.kind == AgentCliKind::Gemini && native::managed(snapshot, "") {
        field.ui.locked_reason = "模型别名由 Gemini 系统配置管理".into();
    }
    if snapshot.kind == AgentCliKind::Grok && !persistent {
        field.ui.locked_reason = "Grok 暂无原生单次别名覆盖入口；可在 Agent 可视化配置中编辑，本次启动可直接选择已有别名。".into();
    }
    Some(field)
}

pub(super) fn decode(value: &str) -> Result<Mappings, String> {
    serde_json::from_str(value).map_err(|_| "模型映射格式无效，请重新填写".into())
}

pub(super) fn without_row(
    value: &str,
    id: &str,
    model_only: bool,
) -> Result<Option<String>, String> {
    let mut values = decode(value)?;
    if model_only {
        if let Some(value) = values.get_mut(id) {
            value.model = None;
            if *value == AgentModelMappingValue::default() {
                values.remove(id);
            }
        }
    } else {
        values.remove(id);
    }
    if values.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&values)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub(super) fn row_value(
    snapshot: &NativeSnapshot,
    id: &str,
    model: &str,
) -> AgentSettingValueSummary {
    match native::effective(snapshot) {
        Ok(values) => display::value(snapshot, &values, id, model),
        Err(error) => super::display::reset_error(error),
    }
}

pub(super) fn validate(
    value: &str,
    options: Option<&AgentModelMappingOptions>,
) -> Result<(), String> {
    let options = options.ok_or("当前 Agent 未提供模型映射")?;
    let values = decode(value)?;
    if values.len() > 256 {
        return Err("模型映射最多可编辑 256 项".into());
    }
    for (id, item) in &values {
        if id.trim() != id || id.is_empty() || id.len() > 160 || id.chars().any(char::is_control) {
            return Err("请输入有效的模型别名，不能包含控制字符或首尾空白".into());
        }
        if !options.allow_custom_aliases && !options.entries.iter().any(|entry| &entry.id == id) {
            return Err(format!("当前 Agent 不提供 {id} 模型档位"));
        }
        let display_names = options
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .map_or(options.display_names, |entry| entry.supports_display_name);
        if (!display_names && item.display_name.is_some())
            || (!options.inheritance && item.extends.is_some())
        {
            return Err(format!("{id} 包含当前 Agent 不支持的模型属性"));
        }
        for text in [&item.model, &item.display_name, &item.extends]
            .into_iter()
            .flatten()
        {
            if text.trim().is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
                return Err(format!("{id} 的模型属性必须是有效文本"));
            }
        }
        if *item == AgentModelMappingValue::default() {
            return Err(format!("请为 {id} 选择实际模型或继承配置，或移除此映射"));
        }
    }
    Ok(())
}

pub(super) fn read(kind: AgentCliKind, root: &Value) -> Result<Mappings, String> {
    native::from_root(kind, root)
}

pub(super) fn write(
    kind: AgentCliKind,
    root: &mut Value,
    value: Option<&str>,
    snapshot: &NativeSnapshot,
) -> Result<(), String> {
    let next = value.map(decode).transpose()?.unwrap_or_default();
    let previous = read(kind, root)?;
    for id in previous.keys().chain(next.keys()).collect::<BTreeSet<_>>() {
        let before = previous.get(id).cloned().unwrap_or_default();
        let after = next.get(id).cloned().unwrap_or_default();
        if before == after {
            continue;
        }
        if next.contains_key(id) && native::managed(snapshot, id) {
            return Err(format!("{id} 由系统或组织的管理配置指定，不能覆盖"));
        }
        let paths = native::paths(kind, id);
        for (path, before, after) in [
            (Some(paths.model.clone()), &before.model, &after.model),
            (
                paths.display_name,
                &before.display_name,
                &after.display_name,
            ),
            (paths.parent, &before.extends, &after.extends),
        ] {
            if let Some(path) = path {
                if before != after {
                    persistence::document::set(
                        root,
                        &path,
                        after.as_ref().map(|text| json!(text)),
                    )?;
                }
            }
        }
        if kind == AgentCliKind::ClaudeCode && before.model != after.model && id != "subagent" {
            // These declarations describe the previous model. Preserve unrelated env keys.
            for suffix in ["_DESCRIPTION", "_SUPPORTED_CAPABILITIES"] {
                persistence::document::set(root, &format!("{}{suffix}", paths.model), None)?;
            }
        }
    }
    Ok(())
}

pub(super) fn validate_snapshot(snapshot: &NativeSnapshot) -> Result<(), String> {
    if snapshot.kind == AgentCliKind::Gemini {
        let values = native::effective(snapshot)?;
        for id in values.keys() {
            native::resolved(snapshot.kind, &values, id, "")?;
        }
    }
    Ok(())
}

pub(super) fn overlay(
    snapshot: &mut NativeSnapshot,
    options: &BTreeMap<String, String>,
) -> Result<(), String> {
    let Some(value) = options.get("modelMappings") else {
        return Ok(());
    };
    let field = field(snapshot, "", false).ok_or("当前 Agent 不提供模型别名覆盖")?;
    field.validate(value)?;
    let mut root = json!({});
    write(snapshot.kind, &mut root, Some(value), snapshot)?;
    snapshot.layers.push(sources::Layer {
        precedence: u32::MAX,
        value: root,
        path: "本次启动模型映射".into(),
        scope: AgentAssetScope::Local,
        role: "launch-model-mappings".into(),
        requirements: false,
    });
    validate_snapshot(snapshot)
}

pub(super) fn apply(
    value: &str,
    snapshot: &NativeSnapshot,
    config: &mut Value,
    plan: &mut TemporaryLaunchPlan,
) -> Result<(), String> {
    match snapshot.kind {
        AgentCliKind::ClaudeCode => {
            for (id, mapping) in decode(value)? {
                let paths = native::paths(snapshot.kind, &id);
                if let Some(model) = mapping.model {
                    plan.environment
                        .set(paths.model.trim_start_matches("/env/"), &model);
                    // A temporary model must not inherit capability declarations
                    // for another target from the user's settings.json.
                    if id != "subagent" {
                        for suffix in ["_DESCRIPTION", "_SUPPORTED_CAPABILITIES"] {
                            plan.environment.set(
                                format!("{}{suffix}", paths.model.trim_start_matches("/env/")),
                                "",
                            );
                        }
                    }
                }
                if let (Some(path), Some(name)) = (paths.display_name, mapping.display_name) {
                    plan.environment
                        .set(path.trim_start_matches("/env/"), &name);
                }
            }
            Ok(())
        }
        AgentCliKind::Gemini => write(snapshot.kind, config, Some(value), snapshot),
        _ => Err("此 Agent 不支持单次模型别名覆盖，请在原生配置中管理".into()),
    }
}

pub(super) fn choices(snapshot: &NativeSnapshot) -> Vec<AgentSettingChoice> {
    if snapshot.kind == AgentCliKind::Codex {
        return Vec::new();
    }
    let Ok(values) = native::effective(snapshot) else {
        return Vec::new();
    };
    let mut choices: Vec<_> = values
        .iter()
        .filter(|(id, value)| {
            snapshot.kind != AgentCliKind::ClaudeCode
                || (id.as_str() != "subagent" && **value != AgentModelMappingValue::default())
        })
        .map(|(id, value)| {
            let resolved = native::resolved(snapshot.kind, &values, id, "")
                .ok()
                .flatten();
            let label = match (value.display_name.as_deref(), resolved.as_deref()) {
                (Some(name), Some(model)) => format!("{name} · {id} → {model}"),
                (_, Some(model)) if model != id => format!("{id} → {model}"),
                (Some(name), _) => format!("{name} · {id}"),
                _ => id.clone(),
            };
            choice(id, &label, "Agent 原生模型别名")
        })
        .collect();
    choices.extend(
        values
            .values()
            .filter_map(|value| value.model.as_deref())
            .map(|id| choice(id, id, "原生配置中的实际模型 ID")),
    );
    choices
}
