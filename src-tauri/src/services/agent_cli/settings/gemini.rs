use super::*;

mod model_values;
pub(super) use model_values::aliases;
pub(super) use model_values::read as model_value;

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: &str,
    help: &str,
    persistent: bool,
) -> Vec<BoundSetting> {
    let mut fields = Vec::new();
    let fixed_model = !model.is_empty() && !model.starts_with("auto");
    fields.push(BoundSetting::new("contextWindow", "model", "上下文窗口", "模型的 inputTokenLimit", Binding::Readonly)
        .readonly().describe("Gemini CLI 按模型容量管理上下文，没有独立的启动窗口覆盖参数；可以调整压缩比例和输出上限。")
        .current(None, "未设置")
        .unset_detail("未设置", "Gemini 的设置文件没有提供当前模型的 inputTokenLimit；CLI 模型列表也未返回此容量。可调整压缩比例或输出上限。"));
    let compression = snapshot.get("/model/compressionThreshold");
    let mut compression = BoundSetting::new(
        "compressionThreshold",
        "model",
        "自动压缩比例",
        "model.compressionThreshold",
        Binding::Json("/model/compressionThreshold", ValueType::Percent),
    )
    .number(1, Some(100), "%")
    .describe("按上下文窗口的百分比触发整理。例如输入 50，原生配置写入 0.5。")
    .current(compression, "50% · 0.5")
    .default(json!(0.5), "Gemini 内置默认值 · model.compressionThreshold");
    if let Some(value) = compression
        .ui
        .current_value
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
    {
        compression.ui.current_value = Some((value * 100.0).round().to_string());
        compression.ui.current_label = format!("{}%", (value * 100.0).round());
    }
    fields.push(compression);
    for (id, label, field, minimum, description) in [
        ("thinkingBudget", "思考 token 预算", "thinkingConfig.thinkingBudget", -1, "适用于支持 thinkingBudget 的模型（如 Gemini 2.5）。−1 请求动态预算，0 请求关闭思考；具体范围与能否关闭由模型决定。"),
        ("outputLimit", "单次输出上限", "maxOutputTokens", 1, "限制一轮生成的 token 数，不会扩大模型的输出容量。只覆盖本次选中的模型，不改动辅助模型。"),
    ] {
        let mut option = BoundSetting::new(id, "model", label, &format!("modelConfigs.customOverrides → generateContentConfig.{field}"), Binding::GeminiModel(field))
            .number(minimum, None, "tokens").describe(description)
            .current(model_value(snapshot, model, field), "未设置")
            .unset_detail("未设置", "已读取的模型配置与继承关系没有提供此参数；具体默认值取决于所选模型，当前尚未取得。");
        if id == "thinkingBudget" { option = option.exclusive("thinking"); }
        fields.push(if fixed_model { option } else { option.locked("先选择明确的模型，才能为该模型设置生成参数") });
    }
    let effort = BoundSetting::new("effort", "model", "推理级别", "modelConfigs.customOverrides → thinkingConfig.thinkingLevel", Binding::GeminiModel("thinkingConfig.thinkingLevel"))
        .choices(&[("MINIMAL", "最少", "部分 Flash 模型支持"), ("LOW", "低", ""), ("MEDIUM", "中", "部分模型支持"), ("HIGH", "高", "")]).custom().exclusive("thinking")
        .describe("适用于支持 thinkingLevel 的模型（如 Gemini 3 系列）。按模型原生枚举填写；与 thinkingBudget 二选一，具体可用档位由该模型检查。")
        .current(model_value(snapshot, model, "thinkingConfig.thinkingLevel"), "未设置")
        .unset_detail("未设置", "当前模型配置没有声明 thinkingLevel，也未取得所选模型的确切默认级别。");
    fields.push(if fixed_model {
        effort
    } else {
        effort.locked("先选择明确的模型，才能设置其推理级别")
    });
    if persistent || help.contains("--approval-mode") {
        let mut choices = vec![
            ("default", "逐项确认", ""),
            ("auto_edit", "自动接受编辑", ""),
            ("plan", "规划模式", ""),
            (
                "yolo",
                "自动批准全部",
                "工具调用不再逐项询问，原生管理策略仍生效",
            ),
        ];
        if snapshot
            .get("/security/disableYoloMode")
            .is_some_and(|value| value.value == true)
        {
            choices.retain(|choice| choice.0 != "yolo");
        }
        fields.push(
            BoundSetting::new(
                "approval",
                "permissions",
                "操作审批",
                "--approval-mode / general.defaultApprovalMode",
                Binding::Flag("--approval-mode"),
            )
            .choices(&choices)
            .current(
                snapshot.get("/general/defaultApprovalMode"),
                "逐项确认 · default",
            )
            .default(json!("default"), "Gemini 内置默认值"),
        );
    }
    if persistent || help.contains("--sandbox") {
        let observed = snapshot
            .env("GEMINI_SANDBOX", None)
            .or_else(|| snapshot.get("/tools/sandbox"));
        let field = BoundSetting::new(
            "sandbox",
            "permissions",
            "命令沙盒",
            "GEMINI_SANDBOX / tools.sandbox",
            Binding::Env("GEMINI_SANDBOX"),
        )
        .choices(&[
            ("true", "启用原生沙盒", "按平台选择可用后端"),
            ("false", "关闭", ""),
            ("docker", "Docker", "需本机已配置 Docker"),
            ("podman", "Podman", "需本机已配置 Podman"),
        ])
        .custom()
        .describe("可填写已安装的原生沙盒后端或配置名。启动时由 Gemini 检查本机依赖。")
        .current(observed, "未设置")
        .unset_detail("未设置", "未设置 GEMINI_SANDBOX 或 tools.sandbox。Gemini 是否自动启用沙盒还与审批模式及本机依赖有关。");
        fields.push(
            if snapshot
                .get("/tools/sandbox")
                .is_some_and(|value| value.managed)
            {
                field.locked("沙盒后端由系统设置指定")
            } else {
                field
            },
        );
    }
    fields.push(BoundSetting::new("tools", "tools", "本次可用的内置工具", "tools.core", Binding::Json("/tools/core", ValueType::List))
        .list().custom().choices(&[
            ("read_file", "读取文件", ""), ("write_file", "写入文件", ""), ("replace", "编辑文件", ""),
            ("glob", "查找文件", ""), ("grep_search", "搜索内容", ""), ("list_directory", "浏览目录", ""),
            ("run_shell_command", "执行命令", ""), ("google_web_search", "网页搜索", ""), ("web_fetch", "读取网页", ""),
            ("save_memory", "保存记忆", ""), ("write_todos", "任务计划", ""), ("activate_skill", "调用 Skill", ""),
        ]).describe("自选后限制内置工具集合；原生禁止与确认规则继续生效。MCP、Skill 与扩展在资产页单独管理。")
        .current(snapshot.get("/tools/core"), "未额外限制内置工具").unset("未额外限制内置工具"));
    for (id, path, label) in [
        ("allowTools", "/tools/allowed", "免确认的工具规则"),
        ("denyTools", "/tools/exclude", "禁止的工具规则"),
        (
            "askTools",
            "/tools/confirmationRequired",
            "始终确认的工具规则",
        ),
    ] {
        fields.push(BoundSetting::new(id, "permissions", label, &path.trim_start_matches('/').replace('/', "."), Binding::JsonAppend(path))
            .rules().describe("每行一条原生工具规则，例如 run_shell_command(git status)。会合并当前配置，不清除已有规则。")
            .current(claude::rule_current(snapshot, path), "未添加此类规则").unset("未添加此类规则"));
    }
    // System settings are the top native layer. An invocation must not rewrite
    // those policy values merely because our provider overlay uses that layer.
    for field in &mut fields {
        let pointer = match &field.binding {
            Binding::Json(pointer, _) | Binding::JsonAppend(pointer) => Some(*pointer),
            Binding::GeminiModel(_) => Some("/modelConfigs"),
            _ => None,
        };
        if pointer
            .and_then(|pointer| snapshot.get(pointer))
            .is_some_and(|value| value.managed)
        {
            field.ui.locked_reason = "此项由 Gemini 系统设置管理".into();
        }
    }
    fields
}
