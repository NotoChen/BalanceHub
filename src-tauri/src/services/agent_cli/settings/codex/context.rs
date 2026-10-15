use super::*;

/// Matches Codex ModelInfo::auto_compact_token_limit. Its 90% compaction ceiling
/// is separate from usable_context_window's model-declared headroom percentage.
pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: Option<&catalog::ModelInfo>,
    native_options: Option<&std::collections::BTreeMap<String, String>>,
) -> Vec<BoundSetting> {
    let selected = |id: &str| {
        native_options
            .and_then(|options| options.get(id))
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .map(|value| Observed {
                value: json!(value),
                source: "本次启动选择".into(),
                managed: false,
                state: AgentSettingValueState::Configured,
            })
    };
    let metadata = |key: &str| model.and_then(|info| info.observed(key));
    let window = selected("contextWindow")
        .or_else(|| snapshot.get("/model_context_window"))
        .or_else(|| metadata("context_window"))
        .or_else(|| metadata("max_context_window"));
    let capacity = window
        .as_ref()
        .and_then(|value| value.value.as_i64())
        .filter(|value| *value > 0);
    let configured = selected("autoCompact")
        .or_else(|| snapshot.get("/model_auto_compact_token_limit"))
        .or_else(|| metadata("auto_compact_token_limit"));
    let configured_limit = configured.as_ref().and_then(|value| value.value.as_i64());
    let ceiling = capacity.map(|value| value.saturating_mul(9) / 10);
    let limit = match (configured_limit, ceiling) {
        (Some(configured), Some(ceiling)) => Some(configured.min(ceiling)),
        (configured, ceiling) => configured.or(ceiling),
    };
    // These adjacent token controls must use the same axis. The compaction
    // percentage still uses the selected context window as its denominator.
    let slider_maximum = [
        model
            .and_then(|info| info.slider_maximum())
            .or(Some(1_000_000)),
        capacity,
        configured_limit.filter(|value| *value > 0),
    ]
    .into_iter()
    .flatten()
    .max();
    let context = BoundSetting::new("contextWindow", "model", "上下文窗口", "-c model_context_window", Binding::Codex("model_context_window", ValueType::Integer))
        .number(1, None, "tokens")
        .slider_maximum(slider_maximum)
        .describe("告诉 Codex 模型可用的上下文容量。优先读取原生配置，再读取所选 CLI 的模型目录；这个值不会扩展服务端容量。输入可用窗口的预留比例与自动压缩比例是两个独立设置。")
        .current(window, "未设置")
        .unset_detail("未设置", "未设置 model_context_window，当前模型目录也没有返回容量。选择具体模型后可重新读取，或直接填写已知容量。");
    let mut compact = BoundSetting::new("autoCompact", "model", "自动压缩阈值", "-c model_auto_compact_token_limit", Binding::Codex("model_auto_compact_token_limit", ValueType::Integer))
        .number(1, None, "tokens")
        .slider_maximum(slider_maximum)
        .percentage_base(capacity)
        .describe("达到此 token 用量时整理会话历史。Codex 默认取上下文窗口的 90%；显式阈值与模型目录阈值仍受此上限约束。百分比以原始上下文窗口为基准，不把模型的输入预留比例当作压缩比例。")
        .current(configured, "未设置")
        .unset_detail("未设置", "未设置压缩阈值，也未读取到上下文窗口，暂时无法计算默认的 90% 阈值。");
    if let Some(limit) = limit {
        if compact.ui.current_value.is_none() {
            compact = compact.default(json!(limit), "Codex 原生计算：上下文窗口 × 90%");
        } else if configured_limit != Some(limit) {
            compact.ui.current_value = Some(limit.to_string());
            compact.ui.current_label = format!("{limit} tokens（按上下文窗口的 90% 封顶）");
            compact.ui.hint = format!("Codex 按窗口的 90% 封顶，实际压缩阈值为 {limit} tokens。");
            compact.ui.source.push_str(" · 受 Codex 原生压缩上限约束");
        }
    }
    vec![context, compact]
}
