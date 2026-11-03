use super::*;

pub(super) fn label(kind: AgentCliKind, id: &str) -> String {
    if kind == AgentCliKind::ClaudeCode {
        match id {
            "sonnet" => "Sonnet",
            "opus" => "Opus",
            "haiku" => "Haiku",
            "fable" => "Fable",
            "subagent" => "子 Agent",
            _ => id,
        }
        .into()
    } else {
        id.into()
    }
}

pub(super) fn value(
    snapshot: &NativeSnapshot,
    values: &Mappings,
    id: &str,
    main: &str,
) -> AgentSettingValueSummary {
    let mapping = values.get(id);
    let mut source = native::source(snapshot, id);
    if snapshot.kind == AgentCliKind::Gemini {
        let mut current = id;
        let mut visited = BTreeSet::new();
        while visited.insert(current) {
            let Some(value) = values.get(current) else {
                break;
            };
            if value.model.is_some() {
                source = native::source(snapshot, current);
                break;
            }
            let Some(parent) = value.extends.as_deref() else {
                break;
            };
            current = parent;
        }
    }
    let mut result = match native::resolved(snapshot.kind, values, id, main) {
        Err(error) => AgentSettingValueSummary {
            value: None,
            label: "映射解析失败".into(),
            source,
            state: AgentSettingValueState::ReadError,
            detail: error,
        },
        Ok(Some(model)) => {
            let follows_main = snapshot.kind == AgentCliKind::ClaudeCode
                && id == "subagent"
                && mapping.is_none_or(|value| value.model.is_none());
            AgentSettingValueSummary {
                label: if follows_main {
                    format!("跟随主会话：{model}")
                } else {
                    model.clone()
                },
                value: Some(model),
                source: if follows_main {
                    "主会话模型".into()
                } else {
                    source
                },
                state: if follows_main {
                    AgentSettingValueState::Default
                } else {
                    AgentSettingValueState::Configured
                },
                detail: if follows_main {
                    "未设置统一的子 Agent 模型，默认跟随主会话；单个角色声明的模型仍可覆盖此默认行为。".into()
                } else if let Some(parent) = mapping.and_then(|value| value.extends.as_deref()) {
                    format!("此别名继承 {parent} 的配置；具体模型按完整继承关系解析。")
                } else {
                    String::new()
                },
            }
        }
        Ok(None) => {
            let (label, detail, state) = match snapshot.kind {
                AgentCliKind::ClaudeCode if id == "subagent" => (
                    "跟随主会话模型".into(),
                    "未单独设置子 Agent 模型，主会话模型目前也未确定；单个角色声明的模型仍会生效。".into(),
                    AgentSettingValueState::Unset,
                ),
                AgentCliKind::ClaudeCode => (
                    format!("Claude 默认 {}", label(snapshot.kind, id)),
                    format!("未设置 {} 档位的模型映射。具体型号由当前 Claude CLI 和账号提供的默认档位确定；尚未取得实际模型 ID。", label(snapshot.kind, id)),
                    AgentSettingValueState::Unset,
                ),
                AgentCliKind::Gemini => if let Some(parent) = mapping.and_then(|value| value.extends.as_deref()) {
                    (format!("继承 {parent}，模型未确定"), format!("当前已读取的 {parent} 继承链没有提供具体模型。请核对继承目标，或为此别名指定模型。"), AgentSettingValueState::Unset)
                } else {
                    ("别名未指定模型".into(), "此别名没有 modelConfig.model 或可解析的继承模型；已有生成参数仍保留。".into(), AgentSettingValueState::Unset)
                },
                AgentCliKind::Grok => (
                    "条目未指定实际模型".into(),
                    "此模型条目未设置 model 字段，尚未确认 Grok 最终使用的模型 ID；连接与其他参数仍保留。".into(),
                    AgentSettingValueState::Unset,
                ),
                AgentCliKind::Codex => unreachable!("Codex has purpose-specific model settings"),
            };
            AgentSettingValueSummary {
                value: None,
                label,
                source,
                state,
                detail,
            }
        }
    };
    super::super::display::check_sources(snapshot, "model", &mut result);
    result
}
