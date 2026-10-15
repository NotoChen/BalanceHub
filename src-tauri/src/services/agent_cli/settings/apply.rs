use super::*;
use crate::services::agent_cli::contracts::TemporaryLaunchPlan;

pub(crate) fn apply(
    settings: &TemporaryCliRunSettings,
    resolved: &ResolvedOptions,
    plan: &mut TemporaryLaunchPlan,
) -> Result<(), String> {
    validate_options(settings, resolved)?;
    let mut config = json_config(plan)?;
    let mut changed_json = false;
    let mut gemini_model = json!({});
    for field in &resolved.fields {
        let Some(value) = settings.native_options.get(&field.ui.id) else {
            continue;
        };
        match &field.binding {
            Binding::ModelMappings => {
                model_mappings::apply(value, &resolved.snapshot, &mut config, plan)?;
                changed_json |= resolved.snapshot.kind == AgentCliKind::Gemini;
            }
            Binding::Codex(key, kind) => codex_config(plan, key, typed(value, *kind)?)?,
            Binding::CodexSubagentLimit => {
                codex_config(plan, codex::V2_LIMIT_KEY, codex::v2_total_count(value)?)?;
            }
            Binding::CodexApproval => {
                let policy = if value == "granular" {
                    let selected = settings
                        .native_options
                        .get("approvalCategories")
                        .map(|value| entries(value));
                    let mut categories = serde_json::Map::new();
                    for category in [
                        "sandbox_approval",
                        "rules",
                        "mcp_elicitations",
                        "request_permissions",
                        "skill_approval",
                    ] {
                        categories.insert(
                            category.into(),
                            json!(selected.as_ref().is_none_or(|selected| selected
                                .iter()
                                .any(|value| value == category))),
                        );
                    }
                    json!({"granular": categories})
                } else {
                    json!(value)
                };
                codex_config(plan, "approval_policy", policy)?;
            }
            // The parent approval option emits one complete granular object.
            Binding::CodexGranular => {}
            Binding::CodexSubagents => {
                let enabled = typed(value, ValueType::Boolean)?;
                codex_config(plan, "agents.enabled", enabled.clone())?;
                codex_config(plan, "features.multi_agent", enabled)?;
                if value == "false" && resolved.snapshot.get("/features/multi_agent_v2").is_some() {
                    codex_config(plan, "features.multi_agent_v2", json!(false))?;
                }
            }
            Binding::Flag(flag) => plan.args.extend([(*flag).into(), value.clone()]),
            Binding::Switch(flag) => plan.args.push((*flag).into()),
            Binding::FlagList(flag) => plan.args.extend([(*flag).into(), entries(value).join(",")]),
            Binding::FlagRules(flag) => {
                let rules = entries(value);
                if resolved.snapshot.kind == AgentCliKind::Grok {
                    for rule in rules {
                        plan.args.extend([(*flag).into(), rule]);
                    }
                } else if !rules.is_empty() {
                    plan.args.push((*flag).into());
                    plan.args.extend(rules);
                }
            }
            Binding::Json(pointer, kind) => {
                set(&mut config, pointer, typed(value, *kind)?)?;
                changed_json = true;
            }
            Binding::JsonAppend(pointer) => {
                let mut values = resolved.snapshot.arrays(pointer);
                if let Some(existing) = config.pointer(pointer).and_then(Value::as_array) {
                    for value in existing {
                        if !values.contains(value) {
                            values.push(value.clone());
                        }
                    }
                }
                for rule in entries(value) {
                    let rule = json!(rule);
                    if !values.contains(&rule) {
                        values.push(rule);
                    }
                }
                set(&mut config, pointer, json!(values))?;
                changed_json = true;
            }
            Binding::Env(name) => plan.environment.set(*name, value),
            Binding::GeminiModel(key) => {
                let kind = if *key == "thinkingConfig.thinkingLevel" {
                    ValueType::Text
                } else {
                    ValueType::Integer
                };
                set(
                    &mut gemini_model,
                    &format!("/generateContentConfig/{}", key.replace('.', "/")),
                    typed(value, kind)?,
                )?;
            }
            Binding::Readonly => return Err(format!("{} 不支持单次覆盖", field.ui.label)),
        }
    }
    if gemini_model
        .as_object()
        .is_some_and(|value| !value.is_empty())
    {
        let mut overrides = resolved.snapshot.arrays("/modelConfigs/customOverrides");
        // Match the primary conversation only. Gemini orders model overrides
        // by alias depth, specificity, then declaration order.
        let target = json!({"model":resolved.model, "isChatModel":true, "overrideScope":"core"});
        if let Some(selected) = gemini_model
            .pointer("/generateContentConfig/thinkingConfig")
            .cloned()
        {
            let mut thinking =
                gemini::model_value(&resolved.snapshot, &resolved.model, "thinkingConfig")
                    .map(|value| value.value)
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({"includeThoughts":true}));
            merge(&mut thinking, &selected);
            let fields = thinking
                .as_object_mut()
                .ok_or("Gemini thinkingConfig 必须为对象")?;
            if settings.native_options.contains_key("effort") {
                fields.remove("thinkingBudget");
            }
            if settings.native_options.contains_key("thinkingBudget") {
                fields.remove("thinkingLevel");
            }
            // Two ordered overrides replace this nested object after alias
            // inheritance. Removing a key from our overlay alone would leave
            // the opposite thinking option inherited from an earlier alias.
            overrides.push(json!({"match":target, "modelConfig":{"generateContentConfig":{"thinkingConfig":null}}}));
            set(
                &mut gemini_model,
                "/generateContentConfig/thinkingConfig",
                thinking,
            )?;
        }
        overrides.push(json!({"match":target,"modelConfig":gemini_model}));
        set(
            &mut config,
            "/modelConfigs/customOverrides",
            json!(overrides),
        )?;
        changed_json = true;
    }
    if changed_json {
        plan.auxiliary_file_content =
            Some(serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?);
    }
    Ok(())
}

fn codex_config(plan: &mut TemporaryLaunchPlan, key: &str, value: Value) -> Result<(), String> {
    let value = toml::Value::try_from(value).map_err(|_| format!("无法生成 {key} 的 TOML 值"))?;
    plan.args.extend(["-c".into(), format!("{key}={value}")]);
    Ok(())
}

fn set(root: &mut Value, pointer: &str, value: Value) -> Result<(), String> {
    let mut segments = pointer.trim_start_matches('/').split('/').peekable();
    let mut target = root;
    while let Some(segment) = segments.next() {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        let object = target
            .as_object_mut()
            .ok_or_else(|| format!("本次原生配置 {pointer} 的父节点必须是对象"))?;
        if segments.peek().is_none() {
            object.insert(segment, value);
            return Ok(());
        }
        target = object.entry(segment).or_insert_with(|| json!({}));
    }
    Err("原生设置路径为空".into())
}

pub(super) fn merge(target: &mut Value, overlay: &Value) {
    if let (Some(target), Some(overlay)) = (target.as_object_mut(), overlay.as_object()) {
        for (key, value) in overlay {
            merge(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    } else {
        *target = overlay.clone();
    }
}

pub(crate) fn json_config(plan: &TemporaryLaunchPlan) -> Result<Value, String> {
    plan.auxiliary_file_content
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map(|v| v.unwrap_or_else(|| json!({})))
        .map_err(|e| format!("解析本次启动配置失败：{e}"))
}
