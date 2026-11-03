use super::{catalog::ModelInfo, *};
use std::collections::BTreeMap;

pub(in crate::services::agent_cli::settings) const V2_LIMIT_KEY: &str =
    "features.multi_agent_v2.max_concurrent_threads_per_session";
pub(in crate::services::agent_cli::settings) const V2_LIMIT_POINTER: &str =
    "/features/multi_agent_v2/max_concurrent_threads_per_session";

// Codex 0.162.1 core/src/config/mod.rs: V1 permits six spawned agents;
// V2 defaults to four total threads and subtracts the primary thread.
const V1_DEFAULT_CHILDREN: i64 = 6;
const V2_DEFAULT_CHILDREN: i64 = 3;

pub(in crate::services::agent_cli::settings) fn v2_child_count(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|value| *value >= 1)
        .map(|value| value - 1)
}

pub(in crate::services::agent_cli::settings) fn v2_total_count(
    value: &str,
) -> Result<Value, String> {
    value
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= 0)
        .and_then(|value| value.checked_add(1))
        .map(|value| json!(value))
        .ok_or_else(|| "子 Agent 并发数超出原生范围".into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    V1,
    V2,
    Disabled,
}

fn backend(
    snapshot: &NativeSnapshot,
    info: Option<&ModelInfo>,
    override_enabled: Option<bool>,
) -> (Backend, String, AgentSettingValueState) {
    if override_enabled == Some(false) {
        return (
            Backend::Disabled,
            "本次启动选择".into(),
            AgentSettingValueState::Configured,
        );
    }
    if let Some(v2) = snapshot.get("/features/multi_agent_v2").filter(|value| {
        value
            .value
            .as_bool()
            .or_else(|| value.value.get("enabled").and_then(Value::as_bool))
            == Some(true)
    }) {
        return (
            Backend::V2,
            format!("{} · features.multi_agent_v2", v2.source),
            v2.state,
        );
    }
    if let Some(enabled) = snapshot
        .get("/agents/enabled")
        .filter(|value| override_enabled.is_none() && value.value == false)
    {
        return (Backend::Disabled, enabled.source, enabled.state);
    }
    if let Some(version) = info.and_then(|info| info.observed("multi_agent_version")) {
        let backend = match version.value.as_str() {
            Some("v1") => Some(Backend::V1),
            Some("v2") => Some(Backend::V2),
            Some("disabled") => Some(Backend::Disabled),
            _ => None,
        };
        if let Some(backend) = backend {
            return (backend, version.source, version.state);
        }
    }
    if override_enabled == Some(true) {
        return (
            Backend::V1,
            "本次启动选择".into(),
            AgentSettingValueState::Configured,
        );
    }
    if let Some(enabled) = snapshot.get("/features/multi_agent") {
        return (
            if enabled.value == false {
                Backend::Disabled
            } else {
                Backend::V1
            },
            enabled.source,
            enabled.state,
        );
    }
    (
        Backend::V1,
        "Codex 内置默认值 · 模型未另行声明后端".into(),
        AgentSettingValueState::Default,
    )
}

fn common_limit(snapshot: &NativeSnapshot) -> Option<(&'static str, Observed)> {
    snapshot
        .layers
        .iter()
        .rev()
        .filter(|layer| !layer.requirements)
        .find_map(|layer| {
            [
                "agents.max_concurrent_threads_per_session",
                "agents.max_threads",
            ]
            .into_iter()
            .find_map(|key| {
                snapshot
                    .value_from_layer(layer, &format!("/{}", key.replace('.', "/")))
                    .map(|value| (key, value))
            })
        })
}

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model_info: Option<&ModelInfo>,
    cli_path: &str,
    native_options: Option<&BTreeMap<String, String>>,
) -> Vec<BoundSetting> {
    let (current_backend, source, state) = backend(snapshot, model_info, None);
    let (backend, _, _) = backend(
        snapshot,
        model_info,
        native_options
            .and_then(|options| options.get("subagents"))
            .and_then(|value| value.parse().ok()),
    );
    let mut agents = BoundSetting::new(
        "subagents", "tools", "子 Agent", "-c agents.enabled / features.multi_agent",
        Binding::CodexSubagents,
    )
    .choices(&boolean())
    .describe("控制多 Agent 协作工具。按显式配置、模型声明的后端和原生功能开关解析；停用会同时关闭优先级更高的 V2 开关。")
    .default(json!(current_backend != Backend::Disabled), &source);
    agents.ui.current_state = state;
    if snapshot.requirement("/features/multi_agent").is_some() {
        agents = agents.locked("系统或组织策略已固定多 Agent 设置");
    }
    let common = common_limit(snapshot);
    let v2_limit = (backend == Backend::V2)
        .then(|| snapshot.get(V2_LIMIT_POINTER))
        .flatten();
    let mut limit = if let Some(observed) = v2_limit {
        let count = v2_child_count(&observed.value);
        let mut field = BoundSetting::new(
            "subagentLimit", "tools", "子 Agent 并发数", &format!("-c {V2_LIMIT_KEY}"),
            Binding::CodexSubagentLimit,
        )
        .number(0, Some(i64::MAX - 1), "个")
        .describe("当前 V2 专用设置包含主 Agent。面板展示子 Agent 数量，写入原生配置时自动加 1；此设置优先于 agents.max_concurrent_threads_per_session。")
        .current(Some(observed), "V2 总线程数未提供");
        if let Some(count) = count {
            field.ui.current_value = Some(count.to_string());
            field.ui.current_label = count.to_string();
        } else {
            field.ui.current_value = None;
            field.ui.current_label = "总线程数配置无效".into();
            field.ui.current_state = AgentSettingValueState::ReadError;
            field.ui.current_detail =
                format!("{V2_LIMIT_KEY} 必须是至少为 1 的整数，其中包含主 Agent；请核对原文值。");
        }
        field
    } else {
        let key = common
            .as_ref()
            .map(|(key, _)| *key)
            .unwrap_or("agents.max_concurrent_threads_per_session");
        BoundSetting::new(
            "subagentLimit", "tools", "子 Agent 并发数", &format!("-c {key}"),
            Binding::Codex(key, ValueType::Integer),
        )
        .number(1, None, "个")
        .describe("限制同时打开的子 Agent 线程数，不含主 Agent。原生 V1 默认 6 个；V2 默认总共 4 个线程，即 3 个子 Agent。")
        .current(common.map(|(_, value)| value), "")
        .default(
            json!(if backend == Backend::V2 { V2_DEFAULT_CHILDREN } else { V1_DEFAULT_CHILDREN }),
            if backend == Backend::V2 { "Codex V2 默认 · 4 个总线程减去 1 个主 Agent" } else { "Codex V1 默认 · 6 个子 Agent" },
        )
    };
    limit.ui.hint = match backend {
        Backend::V2 => "V2 后端 · 此数量不含主 Agent",
        Backend::V1 => "V1 后端 · 此数量不含主 Agent",
        Backend::Disabled => "子 Agent 已停用；启用后按模型选择后端",
    }
    .into();
    limit = limit.when("subagents", "true");
    let child_model = native_options
        .and_then(|options| options.get("subagentModel"))
        .cloned()
        .or_else(|| {
            snapshot
                .get("/agents/default_subagent_model")
                .map(|value| value.text())
        });
    let child_info = child_model
        .as_deref()
        .and_then(|model| catalog::read(snapshot, cli_path, model));
    vec![
        agents,
        limit,
        BoundSetting::new(
            "subagentModel", "model", "子 Agent 默认模型", "-c agents.default_subagent_model",
            Binding::Codex("agents.default_subagent_model", ValueType::Text),
        )
        .models(models::choices(snapshot))
        .when("subagents", "true")
        .describe("创建子 Agent 时的默认模型；显式指定的模型优先。未设置时按父会话和角色配置选择。")
        .current(snapshot.get("/agents/default_subagent_model"), "跟随父会话或角色模型")
        .unset_detail("跟随父会话或角色模型", "未单独设置子 Agent 模型；创建时使用父会话或角色指定的模型。"),
        generation::effort(BoundSetting::new(
            "subagentEffort", "model", "子 Agent 推理强度", "-c agents.default_subagent_reasoning_effort",
            Binding::Codex("agents.default_subagent_reasoning_effort", ValueType::Text),
        ), child_info.as_ref())
        .when("subagents", "true")
        .describe("创建子 Agent 时的默认推理档位；显式指定的档位优先。选择了默认子 Agent 模型时，按其模型目录提供档位。")
        .current(snapshot.get("/agents/default_subagent_reasoning_effort"), "跟随父会话或角色推理设置")
        .unset_detail("跟随父会话或角色推理设置", "未单独设置子 Agent 推理强度；创建时使用父会话或角色指定的档位。"),
        BoundSetting::new(
            "agentInterruptMessage", "tools", "记录 Agent 中断", "-c agents.interrupt_message",
            Binding::Codex("agents.interrupt_message", ValueType::Boolean),
        )
        .choices(&boolean())
        .when("subagents", "true")
        .describe("Agent 回合被中断时，在上下文中记录模型可见的中断消息。")
        .current(snapshot.get("/agents/interrupt_message"), "启用")
        .default(json!(true), "Codex 内置默认值"),
    ]
}
