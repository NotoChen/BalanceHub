use super::{catalog::ModelInfo, *};

pub(super) fn effort(mut field: BoundSetting, model_info: Option<&ModelInfo>) -> BoundSetting {
    field = field
        .choices(&[
            ("none", "关闭", "模型须支持 none"),
            ("minimal", "最少", "模型须支持 minimal"),
            ("low", "低", ""),
            ("medium", "中", ""),
            ("high", "高", ""),
            ("xhigh", "更高", ""),
            ("max", "最大", ""),
            ("ultra", "极致", "模型和客户端须支持 ultra"),
        ])
        .custom();
    if let Some(levels) = model_info
        .and_then(|info| info.value("supported_reasoning_levels"))
        .and_then(Value::as_array)
    {
        field.ui.choices = levels
            .iter()
            .filter_map(|level| {
                let value = level
                    .get("effort")
                    .and_then(Value::as_str)
                    .or_else(|| level.as_str())?;
                Some(choice(
                    value,
                    field
                        .ui
                        .choices
                        .iter()
                        .find(|choice| choice.value == value)
                        .map(|choice| choice.label.as_str())
                        .unwrap_or(value),
                    level
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ))
            })
            .collect();
        field.ui.allow_custom = false;
        if field.ui.choices.is_empty() {
            field.ui.locked_reason = "所选模型未声明可调推理强度".into();
        }
    }
    field
}

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: &str,
    model_info: Option<&ModelInfo>,
) -> Vec<BoundSetting> {
    let observed = |key| model_info.and_then(|info| info.observed(key));
    let verbosity = BoundSetting::new(
        "verbosity",
        "model",
        "回答详细程度",
        "-c model_verbosity",
        Binding::Codex("model_verbosity", ValueType::Text),
    )
    .choices(&[
        ("low", "简洁", ""),
        ("medium", "适中", ""),
        ("high", "详细", ""),
    ])
    .describe("控制回答的详细程度，与推理强度独立。只对支持 verbosity 的模型生效。")
    .current(
        snapshot
            .get("/model_verbosity")
            .or_else(|| observed("default_verbosity")),
        "未设置",
    )
    .unset_detail(
        "未设置",
        "未设置 model_verbosity，当前模型目录也没有返回 default_verbosity。",
    );
    let summary = BoundSetting::new(
        "reasoningSummary",
        "model",
        "推理摘要",
        "-c model_reasoning_summary",
        Binding::Codex("model_reasoning_summary", ValueType::Text),
    )
    .choices(&[
        ("auto", "自动", ""),
        ("concise", "简短", ""),
        ("detailed", "详细", ""),
        ("none", "不展示", ""),
    ])
    .describe("请求模型提供推理摘要；不会改变推理强度，也不代表展示完整的内部思考。")
    .current(
        snapshot
            .get("/model_reasoning_summary")
            .or_else(|| observed("default_reasoning_summary")),
        "未设置",
    )
    .unset_detail(
        "未设置",
        "未设置 model_reasoning_summary，当前模型目录也没有返回默认摘要方式。",
    );
    let mut service = BoundSetting::new(
        "serviceTier", "model", "服务层级", "-c service_tier",
        Binding::Codex("service_tier", ValueType::Text),
    )
    .custom()
    .describe("按当前模型声明的服务层级选择。Fast 可能增加用量或费用，最终可用性与计费由账号和服务端决定。")
    .current(snapshot.get("/service_tier").or_else(|| observed("default_service_tier")), "未设置")
    .unset_detail("未设置", "未设置 service_tier，当前模型目录也没有返回默认层级；服务端会按账号可用层级处理。");
    if let Some(tiers) = model_info
        .and_then(|info| info.value("service_tiers"))
        .and_then(Value::as_array)
    {
        service.ui.choices = tiers
            .iter()
            .filter_map(|tier| {
                let id = tier.get("id")?.as_str()?;
                Some(choice(
                    if id == "priority" { "fast" } else { id },
                    match id {
                        "priority" => "快速",
                        "flex" => "弹性",
                        _ => tier.get("name").and_then(Value::as_str).unwrap_or(id),
                    },
                    if id == "priority" {
                        "Fast 请求优先处理，可能增加用量或费用"
                    } else {
                        "使用当前模型声明的服务层级"
                    },
                ))
            })
            .collect();
    }
    let mut fields = vec![
        if model_info.and_then(|info| info.value("support_verbosity")) == Some(&json!(false)) {
            verbosity.locked("当前模型未提供回答详细程度参数")
        } else {
            verbosity
        },
        if model_info.and_then(|info| info.value("supports_reasoning_summary_parameter"))
            == Some(&json!(false))
        {
            summary.locked("当前模型不接受推理摘要参数")
        } else {
            summary
        },
        service,
    ];
    let review = BoundSetting::new(
        "reviewModel",
        "model",
        "代码审查模型",
        "-c review_model",
        Binding::Codex("review_model", ValueType::Text),
    )
    .models(models::choices(snapshot))
    .describe("供原生 /review 使用。没有单独设置时，使用当前会话的模型。")
    .current(snapshot.get("/review_model"), "跟随主会话模型")
    .unset_detail(
        "跟随主会话模型",
        "未单独设置代码审查模型，主会话模型目前也未确定。",
    );
    fields.push(if model.is_empty() {
        review
    } else {
        review.default(json!(model), "当前会话模型")
    });
    fields
}
