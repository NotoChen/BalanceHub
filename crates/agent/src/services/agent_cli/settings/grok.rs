use super::*;

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: &str,
    help: &str,
    persistent: bool,
) -> Vec<BoundSetting> {
    let mut fields = Vec::new();
    if persistent || help.contains("--reasoning-effort") {
        fields.push(BoundSetting::new("effort", "model", "推理强度", "--reasoning-effort / models.default_reasoning_effort", Binding::Flag("--reasoning-effort"))
            .custom().describe("Grok CLI 将原生强度值交给模型。官方 CLI 参考未规定统一档位，请填写该模型支持的值；不把其他 Agent 的档位套用过来。")
            .current(snapshot.get(&format!("/model/{}/reasoning_effort", native_model_pointer(model))).or_else(|| snapshot.get("/models/default_reasoning_effort")), "未设置")
            .unset_detail("未设置", "没有设置模型条目的 reasoning_effort 或 models.default_reasoning_effort；当前模型目录没有提供默认强度。"));
    }
    for (id, label, key, pointer, fallback) in [
        (
            "contextWindow",
            "上下文窗口",
            "model.<id>.context_window",
            format!("/model/{}/context_window", native_model_pointer(model)),
            "未设置",
        ),
        (
            "compressionThreshold",
            "自动压缩比例",
            "session.auto_compact_threshold_percent",
            "/session/auto_compact_threshold_percent".into(),
            "85%",
        ),
        (
            "outputLimit",
            "单次输出上限",
            "models.max_completion_tokens",
            "/models/max_completion_tokens".into(),
            "未设置",
        ),
    ] {
        let mut field = BoundSetting::new(id, "model", label, key, Binding::Readonly).readonly()
            .describe("此配置存在于 Grok 用户配置中，但当前 CLI 未提供可靠的单次启动覆盖入口；这里展示读取值，避免临时启动改写长期配置。")
            .current(snapshot.get(&pointer), fallback)
            .unset_detail(fallback, &format!("未设置 {key}，当前模型目录也未提供此参数的具体默认值。"));
        if id == "compressionThreshold" {
            field = field.default(
                json!(85),
                "Grok 内置默认值 · session.auto_compact_threshold_percent",
            );
            field.ui.current_label =
                format!("{}%", field.ui.current_value.as_deref().unwrap_or("85"));
        }
        fields.push(field);
    }
    if persistent || help.contains("--permission-mode") {
        fields.push(
            BoundSetting::new(
                "approval",
                "permissions",
                "操作审批",
                "--permission-mode / ui.permission_mode",
                Binding::Flag("--permission-mode"),
            )
            .choices(&[
                ("default", "逐项确认", ""),
                ("acceptEdits", "自动接受编辑", ""),
                ("auto", "自动审查", "按 Grok 原生自动审批机制处理"),
                ("dontAsk", "只用已授权权限", "需要询问的操作直接拒绝"),
                ("bypassPermissions", "跳过权限询问", "原生管理约束仍生效"),
                ("plan", "规划模式", ""),
            ])
            .current(snapshot.get("/ui/permission_mode"), "逐项确认")
            .default(json!("default"), "Grok 内置默认值"),
        );
    }
    if persistent || help.contains("--sandbox") {
        fields.push(BoundSetting::new("sandbox", "permissions", "文件与网络沙盒", "--sandbox / GROK_SANDBOX / sandbox.profile", Binding::Flag("--sandbox"))
            .choices(&[("off", "关闭沙盒", ""), ("workspace", "工作目录", ""), ("read-only", "只读", ""), ("strict", "严格", "")]).custom()
            .describe("还可填写 sandbox.toml 中已定义的自定义 profile 名称。具体文件和网络边界按原生 profile 执行。")
            .current(snapshot.env("GROK_SANDBOX", None).or_else(|| snapshot.get("/sandbox/profile")), "关闭沙盒")
            .default(json!("off"), "Grok 内置默认值"));
    }
    for (id, flag, path, label) in [
        (
            "allowTools",
            "--allow",
            "/permission/allow",
            "免确认的工具规则",
        ),
        ("denyTools", "--deny", "/permission/deny", "禁止的工具规则"),
    ] {
        if persistent || help.contains(flag) {
            fields.push(
                BoundSetting::new(id, "permissions", label, flag, Binding::FlagRules(flag))
                    .rules()
                    .describe("每行一条原生规则，例如 Bash(git *)、Read(src/**)、Edit(**/*.rs)。")
                    .current(claude::rule_current(snapshot, path), "未添加此类规则")
                    .unset("未添加此类规则"),
            );
        }
    }
    if persistent || help.contains("--tools") {
        fields.push(BoundSetting::new("tools", "tools", "本次可用的内置工具", "--tools", Binding::FlagList("--tools"))
            .list().nonempty().custom().describe("填写本机 Grok 提供的工具名称，可逐项添加。自选时至少填写一项；留空工具集的语义未在官方文档中声明，取消此项设置后按 Grok 当前配置加载工具集。")
            .current(None, "未额外限制内置工具").unset("未额外限制内置工具"));
    }
    if persistent || help.contains("--disable-web-search") {
        fields.push(
            BoundSetting::new(
                "webSearch",
                "tools",
                "网页搜索与读取",
                "--disable-web-search",
                Binding::Switch("--disable-web-search"),
            )
            .choices(&[(
                "disabled",
                "本次停用",
                "同时移除 web_search 与 web_fetch 工具",
            )])
            .describe(
                "原生启动参数只提供停用入口；未单独设置时按当前工具集及 GROK_WEB_FETCH 设置加载。",
            )
            .current(None, "未单独停用网页工具")
            .unset("未单独停用网页工具"),
        );
    }
    if persistent || help.contains("--no-plan") {
        fields.push(
            BoundSetting::new(
                "planTool",
                "tools",
                "计划模式工具",
                "--no-plan",
                Binding::Switch("--no-plan"),
            )
            .choices(&[("disabled", "本次停用", "这次会话不提供计划模式")])
            .current(None, "未单独停用计划模式")
            .unset("未单独停用计划模式"),
        );
    }
    for (id, label, env, pointer, default) in [
        ("webFetch", "网页读取", "GROK_WEB_FETCH", None, "0"),
        ("writeFiles", "文件写入工具", "GROK_WRITE_FILE", None, "1"),
        ("subagents", "子 Agent", "GROK_SUBAGENTS", None, "0"),
        ("memory", "跨会话记忆", "GROK_MEMORY", None, "0"),
        (
            "toolSearch",
            "MCP 工具按需发现",
            "GROK_TOOL_SEARCH",
            None,
            "1",
        ),
        (
            "lsp",
            "代码智能工具",
            "GROK_LSP_TOOLS",
            Some("/features/lsp_tools"),
            "0",
        ),
    ] {
        let mut field = BoundSetting::new(id, "tools", label, env, Binding::Env(env))
            .choices(&[("1", "启用", ""), ("0", "停用", "")])
            .current(
                snapshot
                    .env(env, None)
                    .or_else(|| pointer.and_then(|pointer| snapshot.get(pointer))),
                "未设置",
            )
            .default(json!(default), "Grok 内置默认值");
        if let Some(value) = field.ui.current_value.as_deref() {
            let normalized = match value {
                "true" => Some("1"),
                "false" => Some("0"),
                _ => None,
            };
            if let Some(normalized) = normalized {
                field.ui.current_value = Some(normalized.into());
                field.ui.current_label = if normalized == "1" {
                    "启用"
                } else {
                    "停用"
                }
                .into();
            }
        }
        fields.push(field);
    }
    fields
}
