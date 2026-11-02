//! 中转站对模型的推理能力声明。Agent 表单只展示声明与原生档位的交集：
//! 声明是「这个模型实际接受什么」，原生档位是「这个 CLI 会发什么」，两者
//! 同时成立才允许选择；声明不支持的模型直接锁定推理字段。
use super::*;

/// 推理开关与档位字段。各 Agent 的原生 id 一致，但取值形态不同：
/// Claude / Codex / Grok 用小写档位，Gemini 用原生大写枚举。
const REASONING_FIELDS: [&str; 2] = ["effort", "thinkingBudget"];

pub(super) fn annotate(
    fields: &mut [BoundSetting],
    model: &str,
    config: Option<&ProviderModelConfig>,
) {
    let Some(config) = config else {
        return;
    };
    if model.trim().is_empty() {
        return;
    }
    if config.sends_reasoning(model) == Some(false) {
        for field in fields
            .iter_mut()
            .filter(|field| REASONING_FIELDS.contains(&field.ui.id.as_str()))
        {
            if field.ui.locked_reason.is_empty() {
                field.ui.locked_reason = "所选模型声明不接受推理参数".into();
            }
        }
        return;
    }
    let declared = config.reasoning_levels(model);
    if declared.is_empty() {
        return;
    }
    let Some(field) = fields.iter_mut().find(|field| field.ui.id == "effort") else {
        return;
    };
    if !field.ui.locked_reason.is_empty() {
        return;
    }
    if field.ui.choices.is_empty() {
        // 原生 CLI 不枚举档位（例如 Grok）：声明本身就是要展示的选项。
        field.ui.choices = declared
            .iter()
            .map(|level| choice(level, level, "中转站声明的推理档位"))
            .collect();
        field.ui.allow_custom = false;
        return;
    }
    let offered: Vec<AgentSettingChoice> = field
        .ui
        .choices
        .iter()
        .filter(|offered| {
            declared
                .iter()
                .any(|level| level.eq_ignore_ascii_case(&offered.value))
        })
        .cloned()
        .collect();
    if offered.is_empty() {
        field.ui.choices.clear();
        field.ui.allow_custom = false;
        field.ui.locked_reason = "所选模型声明的推理档位与当前 Agent 不兼容".into();
        return;
    }
    field.ui.choices = offered;
    field.ui.allow_custom = false;
}
