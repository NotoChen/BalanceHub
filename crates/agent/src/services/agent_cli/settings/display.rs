//! Value certainty is part of the IPC contract. Unknown defaults and unreadable
//! sources must never be presented as successfully loaded configuration.
use super::*;

pub(super) fn summary(field: &AgentNativeSetting) -> AgentSettingValueSummary {
    AgentSettingValueSummary {
        value: field.current_value.clone(),
        label: field.current_label.clone(),
        source: field.source.clone(),
        state: field.current_state,
        detail: field.current_detail.clone(),
    }
}

pub(super) fn effective(field: &AgentNativeSetting) -> AgentSettingEffectiveValue {
    AgentSettingEffectiveValue {
        summary: summary(field),
        model_mappings: field.model_mappings.clone(),
    }
}

pub(super) fn reset_error(detail: String) -> AgentSettingValueSummary {
    AgentSettingValueSummary {
        value: None,
        label: "取消后取值无法解析".into(),
        source: String::new(),
        state: AgentSettingValueState::ReadError,
        detail,
    }
}

pub(super) fn set_summary(field: &mut AgentNativeSetting, value: AgentSettingValueSummary) {
    field.current_value = value.value;
    field.current_label = value.label;
    field.source = value.source;
    field.current_state = value.state;
    field.current_detail = value.detail;
}

pub(super) fn check_sources(
    snapshot: &NativeSnapshot,
    section: &str,
    value: &mut AgentSettingValueSummary,
) {
    if value.state == AgentSettingValueState::ReadError {
        return;
    }
    if let Some(problem) = snapshot.read_problem(&value.source, section) {
        value.state = AgentSettingValueState::ReadError;
        value.detail = if value.value.is_some() {
            format!("已解析的参考值：{}\n{problem}", value.label)
        } else {
            problem
        };
        value.label = "配置读取失败".into();
    }
}

pub(super) fn annotate(fields: &mut [BoundSetting], snapshot: &NativeSnapshot) {
    for field in fields {
        let mut value = summary(&field.ui);
        check_sources(snapshot, &field.ui.section, &mut value);
        set_summary(&mut field.ui, value);
    }
}

pub(super) fn default_model(snapshot: &NativeSnapshot) -> AgentSettingValueSummary {
    let mut result = if let Some(observed) =
        model_observed(snapshot).filter(|observed| !observed.text().trim().is_empty())
    {
        AgentSettingValueSummary {
            value: Some(observed.text()),
            label: observed.text(),
            source: observed.source,
            state: observed.state,
            detail: String::new(),
        }
    } else {
        let (label, detail) = match snapshot.kind {
            AgentCliKind::ClaudeCode => (
                "Claude 账号默认模型",
                "未设置 model 或 ANTHROPIC_MODEL。Claude 会根据 CLI 版本、登录方式和账号选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Codex => (
                "Codex 默认模型",
                "未设置 model，当前也未取得此 CLI 的确切默认模型 ID。可从 CLI 模型列表选择本次使用的模型。",
            ),
            AgentCliKind::Gemini => (
                "Gemini 自动选择模型",
                "未设置 model.name 或 GEMINI_MODEL。具体模型由 Gemini 的自动路由和账号可用模型确定。",
            ),
            AgentCliKind::Grok => (
                "Grok 默认模型",
                "未设置 models.default 或 GROK_DEFAULT_MODEL。当前未取得 Grok 登录账号的具体默认模型 ID。",
            ),
            AgentCliKind::QwenCode => (
                "Qwen Code 默认模型",
                "未设置 model.name 或 OPENAI_MODEL。具体模型由 Qwen Code 的认证方式和原生设置确定；当前尚未取得确切模型 ID。",
            ),
            AgentCliKind::OpenCode => (
                "OpenCode 默认模型",
                "未设置 model。OpenCode 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Pi => (
                "Pi 默认模型",
                "未设置 defaultModel。Pi 会按原生配置和可用模型选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::PrimeAgent => (
                "Prime Agent 默认模型",
                "未设置 defaultModel。Prime Agent 会按原生配置和可用模型选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Crush => (
                "Crush 默认模型",
                "未设置 models.large。Crush 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Copilot => (
                "Copilot CLI 默认模型",
                "未设置 model。Copilot CLI 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Droid => (
                "Droid 默认模型",
                "未设置 sessionDefaultSettings.model。Droid 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Qoder => (
                "Qoder 默认模型",
                "未设置 model.name。Qoder 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::CodeBuddy => (
                "CodeBuddy 默认模型",
                "未设置 model。CodeBuddy 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::OpenClaw => (
                "OpenClaw 默认模型",
                "未设置 agents.defaults.model。OpenClaw 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Jcode => (
                "jcode 默认模型",
                "未设置 provider.default_model。jcode 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::MothX => (
                "MothX 默认模型",
                "未设置 defaultModel。MothX 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::KimiCode => (
                "Kimi Code 默认模型",
                "未设置 default_model。Kimi Code 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::StepCode => (
                "StepCode 默认模型",
                "未设置 defaultModel。StepCode 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::DevEco => (
                "DevEco Code 默认模型",
                "未设置 model。DevEco Code 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::MiMoCode => (
                "MiMo Code 默认模型",
                "未设置 model。MiMo Code 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::AtomCode => (
                "AtomCode 默认模型",
                "未设置 default_model。AtomCode 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Cline => (
                "Cline 默认模型",
                "未设置 providers.openai-compatible.settings.model。Cline 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Penguin => (
                "PenguinHarness 默认模型",
                "未设置 default_model.model_id。PenguinHarness 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Goose => (
                "Goose 默认模型",
                "未设置 providers.balancehub.model 或 GOOSE_MODEL。Goose 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Hermes => (
                "Hermes Agent 默认模型",
                "未设置 model.default。Hermes Agent 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Dsh => (
                "dsh 默认模型",
                "未设置 agent-default-model.model。dsh 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Omp => (
                "omp 默认模型",
                "未设置 modelRoles.default。omp 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Morph => (
                "Mister Morph 默认模型",
                "未设置 llm.model。Mister Morph 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::MiniMaxCode => (
                "MiniMax Code 默认模型",
                "未设置 defaultModel。MiniMax Code 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
            AgentCliKind::Raycast => (
                "Raycast 默认模型",
                "Raycast 在应用内选择模型；providers.yaml 只提供连接与模型目录，当前没有可读取的默认模型字段。",
            ),
            AgentCliKind::Omo => (
                "OmO 默认模型",
                "未设置 defaultModel。OmO 会按原生配置选择默认模型；当前尚未取得具体模型 ID。",
            ),
        };
        AgentSettingValueSummary {
            value: None,
            label: label.into(),
            source: String::new(),
            state: AgentSettingValueState::Default,
            detail: detail.into(),
        }
    };
    check_sources(snapshot, "model", &mut result);
    result
}

pub(super) fn selected_model(model: &str) -> AgentSettingValueSummary {
    AgentSettingValueSummary {
        value: Some(model.trim().into()),
        label: model.trim().into(),
        source: "本次启动选择".into(),
        state: AgentSettingValueState::Configured,
        detail: String::new(),
    }
}

pub(super) fn history_model(model: &str) -> AgentSettingValueSummary {
    let model = model.trim();
    AgentSettingValueSummary {
        value: (!model.is_empty()).then(|| model.into()),
        label: if model.is_empty() {
            "恢复原会话模型".into()
        } else {
            format!("原会话模型：{model}")
        },
        source: "所选历史会话记录".into(),
        state: if model.is_empty() {
            AgentSettingValueState::Default
        } else {
            AgentSettingValueState::Configured
        },
        detail: if model.is_empty() {
            "当前会话信息没有提供具体模型 ID，继续会话时由 Agent 从原记录恢复。".into()
        } else {
            "未指定本次模型时，继续使用原会话的模型。".into()
        },
    }
}
